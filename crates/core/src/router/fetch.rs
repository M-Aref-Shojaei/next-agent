//! Transport for the `http_fetch` tool. Lives with the router client so the
//! workspace has exactly one HTTP adapter. Unlike router calls, these requests
//! are agent tool calls and only run after the permission engine allowed them.

use crate::local_tools::{FetchedPage, HttpFetch};
use futures_util::StreamExt;
use std::time::Duration;

/// `reqwest`-backed [`HttpFetch`].
#[derive(Debug, Clone)]
pub struct ReqwestFetcher {
    http: reqwest::Client,
    timeout: Duration,
    max_body_bytes: usize,
}

impl ReqwestFetcher {
    /// # Errors
    /// Returns a message if the HTTP client cannot be built.
    pub fn new(timeout: Duration, max_body_bytes: usize) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::limited(5))
            .user_agent(concat!("next-agent/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            http,
            timeout,
            max_body_bytes,
        })
    }

    async fn get(&self, url: &str) -> Result<FetchedPage, String> {
        let parsed = reqwest::Url::parse(url).map_err(|error| format!("invalid URL: {error}"))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(format!("unsupported scheme `{}`", parsed.scheme()));
        }
        let response = self
            .http
            .get(parsed)
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|error| format!("request failed: {error}"))?;
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let mut body = Vec::new();
        let mut truncated = false;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| format!("reading body failed: {error}"))?;
            let room = self.max_body_bytes.saturating_sub(body.len());
            if chunk.len() > room {
                body.extend_from_slice(&chunk[..room]);
                truncated = true;
                break;
            }
            body.extend_from_slice(&chunk);
        }
        tracing::info!(target: "http_fetch", status, bytes = body.len(), "http_fetch completed");
        Ok(FetchedPage {
            status,
            content_type,
            body: String::from_utf8_lossy(&body).into_owned(),
            truncated,
        })
    }
}

impl HttpFetch for ReqwestFetcher {
    fn fetch(&self, url: &str) -> Result<FetchedPage, String> {
        super::block_on(self.get(url))
    }
}
