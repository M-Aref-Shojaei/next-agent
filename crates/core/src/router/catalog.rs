// Design adapted from OpenRouter docs: https://openrouter.ai/docs/api-reference/list-available-models
//! Models catalog import. On a manual "Refresh" the app fetches OpenRouter's
//! public model list once (no API key is sent), saves the raw response in the
//! app data folder and layers it into [`Pricing`] between the user's
//! `pricing.json` and the shipped one. The catalog is never bundled or
//! redistributed (OpenRouter's terms).
//!
//! Prices in the catalog are USD per token as strings; we store USD per 1M.
//! `"-1"` means dynamic (unknown here) and becomes "no price"; `"0"` is free.
//! A `internal_reasoning` of `"0"` means "billed as output", so it is left
//! unset (the cost formula then uses the output price).

use super::pricing::{normalize, Price, Pricing, Table};
use crate::llm::LlmError;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

/// The public catalog endpoint.
pub const OPENROUTER_MODELS_URL: &str = "https://openrouter.ai/api/v1/models";
/// File name of the cached catalog in the app data folder.
pub const CATALOG_FILE: &str = "openrouter-models.json";
/// Responses larger than this are refused (the real list is ~1–2 MB).
const MAX_BYTES: usize = 8 * 1024 * 1024;
/// Redirects followed before the catalog request gives up.
const MAX_REDIRECTS: usize = 3;
const TOO_LARGE: &str = "the model catalog is too large";

/// USD per token as a string → USD per 1M tokens. `-1` (dynamic) and
/// unparsable values → `None`.
fn per_million(value: Option<&Value>) -> Option<f64> {
    let raw = match value? {
        Value::String(s) => s.trim().parse::<f64>().ok()?,
        Value::Number(n) => n.as_f64()?,
        _ => return None,
    };
    if raw < 0.0 {
        return None;
    }
    // Check after scaling too: a huge finite value can overflow to infinity.
    let scaled = (raw * 1e6 * 1e6).round() / 1e6;
    scaled.is_finite().then_some(scaled)
}

fn caps(modalities: Option<&Value>) -> Vec<String> {
    let mut caps = Vec::new();
    for m in modalities.and_then(Value::as_array).into_iter().flatten() {
        let cap = match m.as_str() {
            Some("image") => "vision",
            Some("file") => "pdf",
            Some("audio") => "audio",
            Some("video") => "video",
            _ => continue,
        };
        if !caps.iter().any(|c| c == cap) {
            caps.push(cap.to_string());
        }
    }
    caps
}

fn entry(model: &Value) -> Price {
    let pricing = model.get("pricing");
    let at = |key: &str| per_million(pricing.and_then(|p| p.get(key)));
    let positive_u64 = |v: Option<&Value>| v.and_then(Value::as_u64).filter(|n| *n > 0);
    Price {
        input: at("prompt"),
        output: at("completion"),
        cached: at("input_cache_read"),
        cache_creation: at("input_cache_write"),
        reasoning: at("internal_reasoning").filter(|p| *p > 0.0),
        max_output: positive_u64(model.pointer("/top_provider/max_completion_tokens"))
            .and_then(|n| u32::try_from(n).ok()),
        context_length: positive_u64(model.get("context_length")),
        caps: caps(model.pointer("/architecture/input_modalities")),
        params: model
            .get("supported_parameters")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
    }
}

/// Parse a `/api/v1/models` response into a price table keyed by the full
/// id (`anthropic/claude-sonnet-4.5`) and by the normalised bare name
/// (`claude-sonnet-4-5`), the first id winning for a bare name.
///
/// # Errors
/// A message when the text is not a catalog.
pub fn parse_openrouter(text: &str) -> Result<Table, String> {
    let body: Value = serde_json::from_str(text).map_err(|e| format!("catalog: {e}"))?;
    let list = body
        .get("data")
        .and_then(Value::as_array)
        .ok_or("catalog: no `data` array")?;
    let mut models: HashMap<String, Price> = HashMap::new();
    for model in list {
        let Some(id) = model.get("id").and_then(Value::as_str) else {
            continue;
        };
        let price = entry(model);
        // A `:free`/`:extended` variant is reachable only by its exact id: it
        // must never claim the plain model's names (a $0 variant would make
        // the paid model look free).
        if id.contains(':') {
            models.insert(id.to_string(), price);
            continue;
        }
        let mut names = vec![id.to_string()];
        if let Some(slug) = model.get("canonical_slug").and_then(Value::as_str) {
            names.push(slug.to_string());
        }
        let bare = id.rsplit('/').next().unwrap_or(id);
        names.push(normalize(bare));
        for name in names {
            models.entry(name).or_insert_with(|| price.clone());
        }
    }
    Ok(Table::of_models(models))
}

/// Fetch the catalog from `url` (HTTPS, or loopback for tests). Returns the
/// raw text (to save) and the number of models in it.
///
/// # Errors
/// [`LlmError`] for a refused URL, transport errors, non-2xx answers, an
/// oversized body or a body that is not a catalog.
pub async fn fetch(url: &str) -> Result<(String, usize), LlmError> {
    let parsed = reqwest::Url::parse(url).map_err(|e| LlmError::Config(e.to_string()))?;
    let loopback = matches!(parsed.host_str(), Some("127.0.0.1" | "localhost"));
    if parsed.scheme() != "https" && !(parsed.scheme() == "http" && loopback) {
        return Err(LlmError::Config("the catalog URL must use https".into()));
    }
    // Redirects: at most MAX_REDIRECTS, each to the same host over https on port 443.
    let host = parsed.host_str().unwrap_or_default().to_string();
    let redirects = reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() < MAX_REDIRECTS
            && attempt.url().scheme() == "https"
            && attempt.url().host_str() == Some(host.as_str())
            && attempt.url().port_or_known_default() == Some(443)
        {
            attempt.follow()
        } else {
            attempt.stop()
        }
    });
    let http = reqwest::Client::builder()
        .redirect(redirects)
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("next-agent/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| LlmError::Config(e.to_string()))?;
    let response = http
        .get(parsed)
        .send()
        .await
        .map_err(|e| LlmError::Unavailable(super::client::describe(&e)))?;
    let status = response.status().as_u16();
    if !response.status().is_success() {
        return Err(LlmError::Http {
            status,
            body: "the model catalog could not be downloaded".into(),
        });
    }
    if response
        .content_length()
        .is_some_and(|n| n > MAX_BYTES as u64)
    {
        return Err(LlmError::Protocol(TOO_LARGE.into()));
    }
    // Read in chunks so a body without a length cannot exceed the limit.
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| LlmError::Protocol(e.to_string()))?
    {
        if bytes.len() + chunk.len() > MAX_BYTES {
            return Err(LlmError::Protocol(TOO_LARGE.into()));
        }
        bytes.extend_from_slice(&chunk);
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| LlmError::Protocol("the model catalog is not UTF-8".into()))?;
    let count = parse_openrouter(&text).map_err(LlmError::Protocol)?.len();
    Ok((text, count))
}

/// Parse a saved catalog file, reusing the last parse while the file's
/// modification time and size are unchanged. Returns the pricing layer and
/// the number of models (both empty when the file is missing or bad).
#[must_use]
pub fn load_file(path: &std::path::Path) -> (Pricing, usize) {
    use std::sync::{Mutex, OnceLock, PoisonError};
    type Cached = (
        std::path::PathBuf,
        std::time::SystemTime,
        u64,
        Pricing,
        usize,
    );
    static LAST: OnceLock<Mutex<Option<Cached>>> = OnceLock::new();
    let Ok(meta) = std::fs::metadata(path) else {
        return (Pricing::default(), 0);
    };
    if meta.len() > MAX_BYTES as u64 {
        tracing::warn!(target: "router", "ignoring an oversized model catalog file");
        return (Pricing::default(), 0);
    }
    let stamp = (meta.modified().unwrap_or(std::time::UNIX_EPOCH), meta.len());
    let mut last = LAST
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some((p, modified, len, pricing, count)) = last.as_ref() {
        if p == path && (*modified, *len) == stamp {
            return (pricing.clone(), *count);
        }
    }
    let (pricing, count) = match std::fs::read_to_string(path).map(|t| parse_openrouter(&t)) {
        Ok(Ok(table)) => {
            let count = table.len();
            (Pricing::from_table(table), count)
        }
        Ok(Err(error)) => {
            tracing::warn!(target: "router", %error, "ignoring the saved model catalog");
            (Pricing::default(), 0)
        }
        Err(_) => (Pricing::default(), 0),
    };
    *last = Some((path.to_path_buf(), stamp.0, stamp.1, pricing.clone(), count));
    (pricing, count)
}

/// Save a downloaded catalog atomically (temporary file, then rename), so
/// a crash never leaves a half-written file behind.
///
/// # Errors
/// The I/O error when writing or renaming fails.
pub fn save(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    use std::io::Write as _;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    // A unique temp name per writer, flushed to disk before the rename.
    let temp = path.with_extension(format!("json.{}.{nanos}.tmp", std::process::id()));
    let written = std::fs::File::create(&temp).and_then(|mut file| {
        file.write_all(text.as_bytes())?;
        file.sync_all()
    });
    written
        .and_then(|()| std::fs::rename(&temp, path))
        .inspect_err(|_| {
            let _ = std::fs::remove_file(&temp);
        })
}

/// [`fetch`] from synchronous code (not from an async worker).
///
/// # Errors
/// See [`fetch`].
pub fn fetch_blocking(url: &str) -> Result<(String, usize), LlmError> {
    super::block_on(fetch(url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SAMPLE: &str = r#"{"data":[
      {"id":"anthropic/claude-sonnet-4.5","canonical_slug":"anthropic/claude-4.5-sonnet-20250929",
       "context_length":1000000,
       "architecture":{"input_modalities":["text","image","file"],"output_modalities":["text"]},
       "pricing":{"prompt":"0.000003","completion":"0.000015","input_cache_read":"0.0000003",
                  "input_cache_write":"0.00000375","internal_reasoning":"0"},
       "top_provider":{"max_completion_tokens":64000},
       "supported_parameters":["tools","reasoning","max_tokens"]},
      {"id":"openrouter/auto","context_length":2000000,
       "pricing":{"prompt":"-1","completion":"-1"},"top_provider":{"max_completion_tokens":null}},
      {"id":"odd/inf","pricing":{"prompt":"inf","completion":"NaN"}},
      {"id":"meta-llama/llama-3.3-70b-instruct:free","context_length":0,
       "pricing":{"prompt":"0","completion":"0"},"supported_parameters":["max_tokens"]},
      {"no_id":true}
    ]}"#;

    #[test]
    fn parses_prices_sentinels_and_capabilities() {
        let pricing = Pricing::from_table(parse_openrouter(SAMPLE).expect("catalog"));
        let sonnet = pricing
            .lookup("anthropic", "claude-sonnet-4-5-20250929")
            .expect("by normalised name");
        assert_eq!(sonnet.input, Some(3.0));
        assert_eq!(sonnet.output, Some(15.0));
        assert_eq!(sonnet.cached, Some(0.3));
        assert_eq!(sonnet.cache_creation, Some(3.75));
        assert_eq!(sonnet.reasoning, None, "\"0\" reasoning = billed as output");
        assert_eq!(sonnet.max_output, Some(64000));
        assert_eq!(sonnet.context_length, Some(1_000_000));
        assert_eq!(sonnet.caps, ["vision", "pdf"]);
        assert!(pricing
            .lookup("openrouter", "anthropic/claude-sonnet-4.5")
            .is_some());

        // "-1" = dynamic: no price, so the cost is unknown.
        let auto = pricing
            .lookup("openrouter", "openrouter/auto")
            .expect("auto");
        assert_eq!((auto.input, auto.output), (None, None));
        assert_eq!(auto.max_output, None);
        let odd = pricing.lookup("x", "odd/inf").expect("odd");
        assert_eq!(
            (odd.input, odd.output),
            (None, None),
            "non-finite prices are ignored"
        );
        // "0" = free (known, zero); a 0 context means unknown.
        let free = pricing
            .lookup("openrouter", "meta-llama/llama-3.3-70b-instruct:free")
            .expect("free");
        assert_eq!((free.input, free.output), (Some(0.0), Some(0.0)));
        assert_eq!(free.context_length, None);
        assert!(!pricing
            .caps("openrouter", "meta-llama/llama-3.3-70b-instruct:free")
            .contains(&"tools".to_string()));
        assert!(parse_openrouter("{}").is_err());
    }

    #[tokio::test]
    async fn fetch_downloads_validates_and_refuses_plain_http() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_string(SAMPLE))
            .expect(1)
            .mount(&server)
            .await;
        let url = format!("{}/api/v1/models", server.uri());
        let (text, count) = fetch(&url).await.expect("fetch");
        assert!(count >= 3);
        assert_eq!(text, SAMPLE);
        let received = server.received_requests().await.unwrap_or_default();
        assert!(
            received[0].headers.get("authorization").is_none(),
            "no key is ever sent"
        );
        assert!(fetch("http://example.com/models").await.is_err());
        assert!(fetch("http://localhost.evil.test/models").await.is_err());

        // Redirects to another host are not followed (L3).
        let moved = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", "https://elsewhere.test/models"),
            )
            .mount(&moved)
            .await;
        assert!(fetch(&format!("{}/api/v1/models", moved.uri()))
            .await
            .is_err());

        let broken = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>"))
            .mount(&broken)
            .await;
        assert!(fetch(&format!("{}/x", broken.uri())).await.is_err());
    }

    /// M2 regression: a `:free` variant listed first must not make the paid
    /// model free under its plain names.
    #[test]
    fn variants_never_claim_the_plain_model_names() {
        let text = r#"{"data":[
          {"id":"meta/llama-4:free","canonical_slug":"meta/llama-4","pricing":{"prompt":"0","completion":"0"}},
          {"id":"meta/llama-4","pricing":{"prompt":"0.000001","completion":"0.000002"}}
        ]}"#;
        let pricing = Pricing::from_table(parse_openrouter(text).expect("catalog"));
        for name in ["meta/llama-4", "llama-4"] {
            assert_eq!(
                pricing.lookup("openrouter", name).and_then(|p| p.input),
                Some(1.0),
                "{name}"
            );
        }
        assert_eq!(
            pricing
                .lookup("openrouter", "meta/llama-4:free")
                .and_then(|p| p.input),
            Some(0.0)
        );
        // L1: a finite price that overflows when scaled is ignored.
        let huge = r#"{"data":[{"id":"x/huge","pricing":{"prompt":"1e300","completion":"1"}}]}"#;
        let pricing = Pricing::from_table(parse_openrouter(huge).expect("catalog"));
        assert_eq!(pricing.lookup("x", "x/huge").and_then(|p| p.input), None);
    }

    #[test]
    fn saved_catalogs_are_atomic_and_parsed_once_per_change() {
        let dir = std::env::temp_dir().join(format!("catalog-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join(CATALOG_FILE);
        save(&path, SAMPLE).expect("save");
        let leftovers = std::fs::read_dir(&dir)
            .expect("list")
            .filter(|e| {
                e.as_ref()
                    .is_ok_and(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            })
            .count();
        assert_eq!(leftovers, 0, "temp file renamed away");
        let (first, count) = load_file(&path);
        assert!(count >= 3);
        assert_eq!(load_file(&path).0, first, "cached parse");
        save(&path, r#"{"data":[]}"#).expect("replace");
        assert_eq!(load_file(&path).1, 0, "a changed file is parsed again");
        assert_eq!(load_file(&dir.join("missing.json")).1, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A third-party listing of a bare name must not price the vendor's model.
    #[test]
    fn the_vendors_own_entry_wins_for_its_provider() {
        let text = r#"{"data":[
          {"id":"cheapco/gpt-4o","pricing":{"prompt":"0.0000001","completion":"0.0000001"}},
          {"id":"openai/gpt-4o","pricing":{"prompt":"0.0000025","completion":"0.00001"}}
        ]}"#;
        let pricing = Pricing::from_table(parse_openrouter(text).expect("catalog"));
        assert_eq!(
            pricing.lookup("openai", "gpt-4o").and_then(|p| p.input),
            Some(2.5)
        );
    }
}
