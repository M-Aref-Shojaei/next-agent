//! HTTP client for the OpenAI-compatible router.

use super::settings::{RouterSettings, SecretStore};
use crate::llm::{
    ChatRequest, Completion, CompletionClient, LlmError, StreamAccumulator, ToolCall,
};
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use next_agent_process_manager::{HealthProbe, HealthReport, HealthStatus, ProbeFuture};
use std::time::{Duration, Instant};

/// Retries for 429 / 5xx / connection failures before a stream opens.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    /// Total attempts, including the first.
    pub attempts: u32,
    /// Delay before the second attempt; doubles each time.
    pub base_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            attempts: 3,
            base_delay: Duration::from_millis(500),
        }
    }
}

/// Router client. Cheap to clone (shares the connection pool).
#[derive(Clone)]
pub struct RouterClient {
    http: reqwest::Client,
    settings: RouterSettings,
    api_key: Option<String>,
    retry: RetryPolicy,
    /// Longest silence tolerated mid-stream.
    idle_timeout: Duration,
}

impl std::fmt::Debug for RouterClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RouterClient")
            .field("base_url", &self.settings.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .finish_non_exhaustive()
    }
}

impl RouterClient {
    /// Build a client for `settings`, authenticating with `api_key` if given.
    ///
    /// # Errors
    /// [`LlmError::Config`] if the HTTP client cannot be built.
    pub fn new(settings: RouterSettings, api_key: Option<String>) -> Result<Self, LlmError> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .user_agent(concat!("next-agent/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| LlmError::Config(error.to_string()))?;
        Ok(Self {
            http,
            settings,
            api_key: api_key.filter(|key| !key.is_empty()),
            retry: RetryPolicy::default(),
            idle_timeout: Duration::from_secs(120),
        })
    }

    /// Build a client reading the API key from a keychain.
    ///
    /// # Errors
    /// [`LlmError::Config`] if the keychain read fails.
    pub fn from_secret(
        settings: RouterSettings,
        secret: &dyn SecretStore,
    ) -> Result<Self, LlmError> {
        let key = secret
            .get()
            .map_err(|error| LlmError::Config(error.to_string()))?;
        Self::new(settings, key)
    }

    #[must_use]
    pub fn with_retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    #[must_use]
    pub fn with_idle_timeout(mut self, timeout: Duration) -> Self {
        self.idle_timeout = timeout;
        self
    }

    #[must_use]
    pub fn settings(&self) -> &RouterSettings {
        &self.settings
    }

    fn authed(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.api_key {
            Some(key) => builder.bearer_auth(key),
            None => builder,
        }
    }

    /// `GET {base}/models` → model ids.
    ///
    /// # Errors
    /// [`LlmError`] for transport, auth or format failures.
    pub async fn list_models(&self) -> Result<Vec<String>, LlmError> {
        let response = self
            .authed(self.http.get(self.settings.endpoint("models")))
            .timeout(self.settings.health_timeout)
            .send()
            .await
            .map_err(|error| LlmError::Unavailable(describe(&error)))?;
        let status = response.status().as_u16();
        if matches!(status, 401 | 403) {
            return Err(LlmError::Unauthorized);
        }
        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(LlmError::Http {
                status,
                body: crate::runner::truncate(&body, 500),
            });
        }
        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|error| LlmError::Protocol(error.to_string()))?;
        parse_models(&body)
    }

    /// Probe the router: `ok` (200 + models, fast), `degraded` (slow, auth
    /// error, odd answer), `down` (refused / timed out).
    pub async fn health(&self) -> HealthReport {
        let started = Instant::now();
        let result = self.list_models().await;
        let latency = started.elapsed();
        let report = match result {
            Ok(models) if models.is_empty() => {
                HealthReport::new(HealthStatus::Degraded, "router has no models configured")
            }
            Ok(_) if latency > self.settings.slow_threshold => HealthReport::new(
                HealthStatus::Degraded,
                format!("slow response ({} ms)", latency.as_millis()),
            ),
            Ok(models) => HealthReport::new(HealthStatus::Ok, format!("{} models", models.len())),
            Err(LlmError::Unauthorized) => {
                HealthReport::new(HealthStatus::Degraded, "unauthorized (check API key)")
            }
            Err(LlmError::Unavailable(detail)) => HealthReport::new(HealthStatus::Down, detail),
            Err(other) => HealthReport::new(HealthStatus::Degraded, other.to_string()),
        }
        .with_latency(latency);
        tracing::debug!(
            target: "router",
            status = report.status.as_str(),
            latency_ms = report.latency_ms,
            "router health"
        );
        report
    }

    /// Streaming chat completion; `on_delta` receives text as it arrives.
    ///
    /// # Errors
    /// [`LlmError`] for transport, auth, model or protocol failures.
    pub async fn chat(
        &self,
        request: &ChatRequest,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<Completion, LlmError> {
        let started = Instant::now();
        let response = self.open_stream(request).await?;
        let is_json = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("application/json"));
        let completion = if is_json {
            // Some OpenAI-compatible servers ignore `stream: true`.
            let body: serde_json::Value = response
                .json()
                .await
                .map_err(|error| LlmError::Protocol(error.to_string()))?;
            let completion = parse_full_completion(&body)?;
            if !completion.content.is_empty() {
                on_delta(&completion.content);
            }
            completion
        } else {
            self.read_stream(response, on_delta).await?
        };
        tracing::info!(
            target: "router",
            model = %request.model,
            latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            tool_calls = completion.tool_calls.len(),
            finish = completion.finish_reason.as_deref().unwrap_or(""),
            "router chat completed"
        );
        Ok(completion)
    }

    async fn open_stream(&self, request: &ChatRequest) -> Result<reqwest::Response, LlmError> {
        let mut body =
            serde_json::to_value(request).map_err(|error| LlmError::Protocol(error.to_string()))?;
        body["stream"] = serde_json::Value::Bool(true);
        let url = self.settings.endpoint("chat/completions");
        let attempts = self.retry.attempts.max(1);
        let mut delay = self.retry.base_delay;
        for attempt in 1..=attempts {
            let last = attempt == attempts;
            let sent = self
                .authed(self.http.post(&url))
                .header(reqwest::header::ACCEPT, "text/event-stream")
                .json(&body)
                .send()
                .await;
            let retry = match sent {
                Ok(response) if response.status().is_success() => return Ok(response),
                Ok(response) => {
                    let status = response.status().as_u16();
                    let text = response.text().await.unwrap_or_default();
                    let error = classify_status(status, &text, &request.model);
                    if !(status == 429 || status >= 500) || last {
                        return Err(error);
                    }
                    error
                }
                Err(error) => {
                    let mapped = LlmError::Unavailable(describe(&error));
                    if !(error.is_connect() || error.is_timeout()) || last {
                        return Err(mapped);
                    }
                    mapped
                }
            };
            tracing::warn!(target: "router", attempt, error = %retry, "router call failed; retrying");
            tokio::time::sleep(delay).await;
            delay *= 2;
        }
        Err(LlmError::Unavailable("no attempts made".into()))
    }

    async fn read_stream(
        &self,
        response: reqwest::Response,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<Completion, LlmError> {
        let mut events = response.bytes_stream().eventsource();
        let mut accumulator = StreamAccumulator::new();
        loop {
            let next = tokio::time::timeout(self.idle_timeout, events.next())
                .await
                .map_err(|_| {
                    LlmError::Unavailable(format!(
                        "stream stalled for {} s",
                        self.idle_timeout.as_secs()
                    ))
                })?;
            let Some(event) = next else { break };
            let event = event.map_err(|error| LlmError::Protocol(error.to_string()))?;
            let data = event.data.trim();
            if data == "[DONE]" {
                break;
            }
            if data.is_empty() {
                continue;
            }
            let chunk: serde_json::Value = serde_json::from_str(data)
                .map_err(|error| LlmError::Protocol(format!("bad SSE chunk: {error}")))?;
            if let Some(text) = accumulator.push(&chunk)? {
                on_delta(&text);
            }
        }
        Ok(accumulator.finish())
    }
}

impl CompletionClient for RouterClient {
    fn complete(
        &self,
        request: &ChatRequest,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<Completion, LlmError> {
        super::block_on(self.chat(request, on_delta))
    }
}

impl HealthProbe for RouterClient {
    fn probe(&self) -> ProbeFuture<'_> {
        Box::pin(self.health())
    }
}

pub(crate) fn describe(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "timed out".into()
    } else if error.is_connect() {
        "connection failed".into()
    } else {
        error.to_string()
    }
}

pub(crate) fn classify_status(status: u16, body: &str, model: &str) -> LlmError {
    let lower = body.to_ascii_lowercase();
    let missing_model = lower.contains("model")
        && (lower.contains("not found") || lower.contains("does not exist"));
    match status {
        401 | 403 => LlmError::Unauthorized,
        404 => LlmError::ModelNotFound(model.to_string()),
        400 if missing_model => LlmError::ModelNotFound(model.to_string()),
        _ => LlmError::Http {
            status,
            body: crate::runner::truncate(body, 500),
        },
    }
}

fn parse_models(body: &serde_json::Value) -> Result<Vec<String>, LlmError> {
    let data = body
        .get("data")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| LlmError::Protocol("models response has no `data` array".into()))?;
    Ok(data
        .iter()
        .filter_map(|model| model.get("id").and_then(|id| id.as_str()))
        .map(str::to_string)
        .collect())
}

fn parse_full_completion(body: &serde_json::Value) -> Result<Completion, LlmError> {
    let choice = body
        .get("choices")
        .and_then(|choices| choices.as_array())
        .and_then(|choices| choices.first())
        .ok_or_else(|| LlmError::Protocol("completion has no choices".into()))?;
    let message = choice
        .get("message")
        .ok_or_else(|| LlmError::Protocol("choice has no message".into()))?;
    let tool_calls: Vec<ToolCall> = match message.get("tool_calls") {
        Some(calls) if !calls.is_null() => serde_json::from_value(calls.clone())
            .map_err(|error| LlmError::Protocol(error.to_string()))?,
        _ => Vec::new(),
    };
    Ok(Completion {
        content: message
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or_default()
            .to_string(),
        tool_calls,
        finish_reason: choice
            .get("finish_reason")
            .and_then(|r| r.as_str())
            .map(str::to_string),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn statuses_map_to_clear_errors() {
        assert_eq!(classify_status(401, "", "m"), LlmError::Unauthorized);
        assert_eq!(
            classify_status(404, "", "m"),
            LlmError::ModelNotFound("m".into())
        );
        assert_eq!(
            classify_status(400, "The model `m` does not exist", "m"),
            LlmError::ModelNotFound("m".into())
        );
        assert!(matches!(
            classify_status(500, "oops", "m"),
            LlmError::Http { status: 500, .. }
        ));
    }

    #[test]
    fn models_and_full_completions_parse() {
        assert_eq!(
            parse_models(&json!({"data":[{"id":"a"},{"id":"b"}]})).expect("models"),
            ["a", "b"]
        );
        assert!(parse_models(&json!({"models":[]})).is_err());
        let completion = parse_full_completion(&json!({
            "choices":[{"message":{"content":"hi","tool_calls":[{"id":"c","type":"function","function":{"name":"list_dir","arguments":"{}"}}]},"finish_reason":"tool_calls"}]
        }))
        .expect("completion");
        assert_eq!(completion.content, "hi");
        assert_eq!(completion.tool_calls[0].function.name, "list_dir");
    }

    #[test]
    fn debug_output_redacts_the_key() {
        let client =
            RouterClient::new(RouterSettings::default(), Some("sk-secret".into())).expect("client");
        let debug = format!("{client:?}");
        assert!(!debug.contains("sk-secret"));
        assert!(debug.contains("redacted"));
    }
}
