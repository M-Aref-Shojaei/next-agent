// Ported from 9Router (MIT, © 2024-2026 decolua and contributors): open-sse/providers/pricing.js @ 1855aa4
// Design adapted from OpenRouter docs: https://openrouter.ai/docs/guides/best-practices/prompt-caching
//! Model prices (USD per 1M tokens), output ceilings, context windows and
//! capabilities. Layers, first match per field wins: the user's `pricing.json`,
//! the imported OpenRouter catalog ([`super::catalog`]), then the
//! `pricing.json` shipped next to this file. Every cost is an estimate;
//! unknown models cost 0.
//!
//! Cache-aware cost: when a model has no explicit cache price, cache reads and
//! writes are billed at the input price times the provider's multiplier
//! (`cache_multipliers`, e.g. Anthropic 0.1× read / 1.25× write).

use super::translate::Usage;
use serde::Deserialize;
use std::collections::HashMap;

/// The data file shipped in the repo.
pub const BUILTIN_PRICING: &str = include_str!("pricing.json");

/// One price entry. Missing `input`/`output` = price unknown.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Price {
    #[serde(default)]
    pub input: Option<f64>,
    #[serde(default)]
    pub output: Option<f64>,
    #[serde(default)]
    pub cached: Option<f64>,
    #[serde(default)]
    pub cache_creation: Option<f64>,
    #[serde(default)]
    pub reasoning: Option<f64>,
    #[serde(default)]
    pub max_output: Option<u32>,
    /// Context window in tokens.
    #[serde(default)]
    pub context_length: Option<u64>,
    #[serde(default)]
    pub caps: Vec<String>,
    /// Supported request parameters (`tools`, `reasoning`…); empty = unknown.
    #[serde(default)]
    pub params: Vec<String>,
}

impl Price {
    /// Fill fields this entry lacks from `other` (a lower layer).
    fn fill(&mut self, other: &Self) {
        fn take<T: Clone>(slot: &mut Option<T>, from: Option<&T>) {
            if slot.is_none() {
                *slot = from.cloned();
            }
        }
        take(&mut self.input, other.input.as_ref());
        take(&mut self.output, other.output.as_ref());
        take(&mut self.cached, other.cached.as_ref());
        take(&mut self.cache_creation, other.cache_creation.as_ref());
        take(&mut self.reasoning, other.reasoning.as_ref());
        take(&mut self.max_output, other.max_output.as_ref());
        take(&mut self.context_length, other.context_length.as_ref());
        if self.caps.is_empty() {
            self.caps.clone_from(&other.caps);
        }
        if self.params.is_empty() {
            self.params.clone_from(&other.params);
        }
    }
}

/// Cache price multipliers of the input price.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
pub struct CacheMultiplier {
    #[serde(default)]
    pub read: Option<f64>,
    #[serde(default)]
    pub write: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct Pattern {
    #[serde(rename = "match")]
    pattern: String,
    #[serde(flatten)]
    price: Price,
}

/// One price file.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Table {
    #[serde(default)]
    free_providers: Vec<String>,
    #[serde(default)]
    providers: HashMap<String, HashMap<String, Price>>,
    #[serde(default)]
    models: HashMap<String, Price>,
    #[serde(default)]
    patterns: Vec<Pattern>,
    #[serde(default)]
    cache_multipliers: HashMap<String, CacheMultiplier>,
}

impl Table {
    /// A table of models keyed by id (used by the catalog import).
    #[must_use]
    pub fn of_models(models: HashMap<String, Price>) -> Self {
        Self {
            models,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.models.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }

    fn find(&self, provider: &str, model: &str) -> Option<&Price> {
        if let Some(price) = self.providers.get(provider).and_then(|m| m.get(model)) {
            return Some(price);
        }
        let bare = model.rsplit('/').next().unwrap_or(model);
        // `provider/model` first: the vendor's own entry beats another
        // vendor's listing of the same bare name (catalog imports).
        self.models
            .get(&format!("{provider}/{bare}"))
            .or_else(|| self.models.get(model))
            .or_else(|| self.models.get(bare))
            .or_else(|| self.models.get(&normalize(bare)))
            .or_else(|| {
                self.patterns
                    .iter()
                    .find(|p| glob(&p.pattern, bare))
                    .map(|p| &p.price)
            })
    }
}

/// `Claude-Sonnet-4.5-20250929:free` → `claude-sonnet-4-5`: lower case, dots
/// to dashes, no `:variant`, no trailing date.
#[must_use]
pub fn normalize(model: &str) -> String {
    let base = model.split(':').next().unwrap_or(model);
    let mut id = base.to_ascii_lowercase().replace('.', "-");
    if let Some((head, tail)) = id.rsplit_once('-') {
        if tail.len() == 8 && tail.bytes().all(|b| b.is_ascii_digit()) {
            id = head.to_string();
        }
    }
    id.strip_suffix("-latest").map(str::to_string).unwrap_or(id)
}

/// The layered price table.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pricing {
    /// Highest priority first.
    layers: Vec<Table>,
}

/// A computed cost.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cost {
    pub usd: f64,
    /// `false` when the model has no price entry (`usd` is then 0).
    pub known: bool,
}

impl Pricing {
    /// The table shipped with the app.
    ///
    /// # Panics
    /// Never in practice: the shipped file is checked by tests.
    #[must_use]
    pub fn builtin() -> Self {
        Self::from_json(BUILTIN_PRICING).expect("shipped pricing.json is valid")
    }

    /// Parse a pricing file.
    ///
    /// # Errors
    /// Returns the JSON error message.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let table: Table = serde_json::from_str(text).map_err(|e| format!("pricing file: {e}"))?;
        Ok(Self::from_table(table))
    }

    #[must_use]
    pub fn from_table(table: Table) -> Self {
        Self {
            layers: vec![table],
        }
    }

    /// Put `top` above every existing layer: its fields win.
    #[must_use]
    pub fn with_override(mut self, top: Self) -> Self {
        let mut layers = top.layers;
        layers.append(&mut self.layers);
        self.layers = layers;
        self
    }

    /// Entry for a model: each field from the highest layer that has it.
    /// Free providers get a zero price.
    #[must_use]
    pub fn lookup(&self, provider: &str, model: &str) -> Option<Price> {
        let mut info: Option<Price> = None;
        for found in self.layers.iter().filter_map(|t| t.find(provider, model)) {
            match &mut info {
                Some(price) => price.fill(found),
                None => info = Some(found.clone()),
            }
        }
        let free = self
            .layers
            .iter()
            .any(|t| t.free_providers.iter().any(|p| p == provider));
        if free {
            return Some(Price {
                input: Some(0.0),
                output: Some(0.0),
                ..info.unwrap_or_default()
            });
        }
        info
    }

    /// Output-token ceiling for a model, if known.
    #[must_use]
    pub fn max_output(&self, provider: &str, model: &str) -> Option<u32> {
        self.lookup(provider, model).and_then(|p| p.max_output)
    }

    /// Context window of a model, if known.
    #[must_use]
    pub fn context_length(&self, provider: &str, model: &str) -> Option<u64> {
        self.lookup(provider, model).and_then(|p| p.context_length)
    }

    /// Capabilities (`vision`, `pdf`, `audio`, `video`, `tools`) of a model.
    /// `tools` is assumed unless the parameter list is known and lacks it.
    #[must_use]
    pub fn caps(&self, provider: &str, model: &str) -> Vec<String> {
        let Some(price) = self.lookup(provider, model) else {
            return vec!["tools".into()];
        };
        let mut caps = price.caps;
        if price.params.is_empty() || price.params.iter().any(|p| p == "tools") {
            caps.push("tools".into());
        }
        caps
    }

    /// Cache multipliers for a provider, else for the model's vendor prefix
    /// (`anthropic/claude…` through OpenRouter).
    fn multiplier(&self, provider: &str, model: &str) -> CacheMultiplier {
        let vendor = model.split_once('/').map_or("", |(v, _)| v);
        let mut out = CacheMultiplier::default();
        for key in [provider, vendor] {
            for table in &self.layers {
                if let Some(m) = table.cache_multipliers.get(key) {
                    out.read = out.read.or(m.read);
                    out.write = out.write.or(m.write);
                }
            }
        }
        out
    }

    /// Estimated cost of one request.
    #[must_use]
    pub fn cost(&self, provider: &str, model: &str, usage: &Usage) -> Cost {
        match self.lookup(provider, model) {
            Some(mut price) if price.input.is_some() && price.output.is_some() => {
                let input = price.input.unwrap_or(0.0);
                let m = self.multiplier(provider, model);
                price.cached = price.cached.or(m.read.map(|r| input * r));
                price.cache_creation = price.cache_creation.or(m.write.map(|w| input * w));
                Cost {
                    usd: cost(usage, &price),
                    known: true,
                }
            }
            _ => Cost {
                usd: 0.0,
                known: false,
            },
        }
    }
}

/// 9Router's formula. `usage.completion` includes reasoning tokens, so the
/// plain output share is `completion − reasoning`:
/// `(max(0, input − cached − cacheCreation)·in + cached·(cachedPrice‖in)
///   + output·out + reasoning·(reasoningPrice‖out)
///   + cacheCreation·(cacheCreationPrice‖in)) / 1e6`
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn cost(usage: &Usage, price: &Price) -> f64 {
    let input = price.input.unwrap_or(0.0);
    let output = price.output.unwrap_or(0.0);
    let fresh = (usage.prompt - usage.cached - usage.cache_creation).max(0) as f64;
    let plain_output = (usage.completion - usage.reasoning).max(0) as f64;
    (fresh * input
        + usage.cached as f64 * price.cached.unwrap_or(input)
        + plain_output * output
        + usage.reasoning as f64 * price.reasoning.unwrap_or(output)
        + usage.cache_creation as f64 * price.cache_creation.unwrap_or(input))
        / 1e6
}

/// `*` matches any run of characters; everything else is literal.
fn glob(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let mut rest = text;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            match rest.strip_prefix(part) {
                Some(r) => rest = r,
                None => return false,
            }
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else if let Some(at) = rest.find(part) {
            rest = &rest[at + part.len()..];
        } else {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(prompt: i64, completion: i64, cached: i64, creation: i64, reasoning: i64) -> Usage {
        Usage {
            prompt,
            completion,
            cached,
            cache_creation: creation,
            reasoning,
        }
    }

    #[test]
    fn shipped_file_parses_and_resolves() {
        let pricing = Pricing::builtin();
        assert_eq!(
            pricing.lookup("openai", "gpt-4o").and_then(|p| p.input),
            Some(2.5)
        );
        // Vendor prefix stripped (OpenRouter style ids).
        assert_eq!(
            pricing
                .lookup("openrouter", "openai/gpt-4o-mini")
                .and_then(|p| p.output),
            Some(0.6)
        );
        // Glob patterns.
        assert_eq!(
            pricing
                .lookup("anthropic", "claude-sonnet-4-5-20250929")
                .and_then(|p| p.input),
            Some(3.0)
        );
        assert_eq!(
            pricing.max_output("anthropic", "claude-3-haiku-20240307"),
            Some(4096)
        );
        assert!(pricing
            .caps("gemini", "gemini-2.5-flash")
            .contains(&"audio".to_string()));
        // Free providers cost 0 but are known.
        let free = pricing.cost("ollama", "llama3.2", &usage(1000, 1000, 0, 0, 0));
        assert!(free.known && free.usd == 0.0);
        // Unknown = 0, labelled unknown.
        let unknown = pricing.cost("custom", "mystery-model", &usage(1000, 1000, 0, 0, 0));
        assert!(!unknown.known && unknown.usd == 0.0);
        // A pattern with only caps is not a price.
        assert!(
            !pricing
                .cost("anthropic", "claude-future-9", &usage(1, 1, 0, 0, 0))
                .known
        );
    }

    #[test]
    fn cost_formula_matches_the_spec() {
        let price = Price {
            input: Some(3.0),
            output: Some(15.0),
            cached: Some(0.3),
            cache_creation: Some(3.75),
            ..Price::default()
        };
        // 1M prompt of which 200k cached and 100k cache writes; 50k output.
        let u = usage(1_000_000, 50_000, 200_000, 100_000, 0);
        let expected =
            (700_000.0 * 3.0 + 200_000.0 * 0.3 + 50_000.0 * 15.0 + 100_000.0 * 3.75) / 1e6;
        assert!((cost(&u, &price) - expected).abs() < 1e-12);
        // Fallbacks: cached/creation default to the input price, reasoning to output.
        let plain = Price {
            input: Some(1.0),
            output: Some(2.0),
            ..Price::default()
        };
        let u = usage(1_000_000, 1_000_000, 500_000, 0, 400_000);
        let expected = (500_000.0 + 500_000.0 + 600_000.0 * 2.0 + 400_000.0 * 2.0) / 1e6;
        assert!((cost(&u, &plain) - expected).abs() < 1e-12);
        // Never negative when cached exceeds prompt.
        assert!(cost(&usage(10, 0, 20, 0, 0), &plain) >= 0.0);
    }

    #[test]
    fn overrides_win() {
        let user = Pricing::from_json(
            r#"{"models":{"gpt-4o":{"input":1,"output":1}},"providers":{"openrouter":{"x/y":{"input":9,"output":9}}},"patterns":[{"match":"claude-*","input":7,"output":7}]}"#,
        )
        .expect("user");
        let merged = Pricing::builtin().with_override(user);
        assert_eq!(
            merged.lookup("openai", "gpt-4o").and_then(|p| p.input),
            Some(1.0)
        );
        assert_eq!(
            merged.lookup("openrouter", "x/y").and_then(|p| p.input),
            Some(9.0)
        );
        assert_eq!(
            merged
                .lookup("anthropic", "claude-sonnet-4")
                .and_then(|p| p.input),
            Some(7.0)
        );
    }

    #[test]
    fn cache_multipliers_apply_without_explicit_cache_prices() {
        let table = Pricing::from_json(
            r#"{"models":{"m":{"input":10,"output":10}},"cache_multipliers":{"anthropic":{"read":0.1,"write":1.25},"deepseek":{"read":0.1}}}"#,
        )
        .expect("table");
        let u = usage(1_000_000, 0, 500_000, 100_000, 0);
        // 400k fresh·10 + 500k·1 + 100k·12.5
        let anthropic = table.cost("anthropic", "m", &u).usd;
        assert!((anthropic - (4.0 + 0.5 + 1.25)).abs() < 1e-9);
        // Via OpenRouter, the vendor prefix picks the multiplier.
        let routed = table.cost("openrouter", "anthropic/m", &u).usd;
        assert!((routed - anthropic).abs() < 1e-9);
        // No write multiplier: writes cost the input price.
        let deepseek = table.cost("deepseek", "m", &u).usd;
        assert!((deepseek - (4.0 + 0.5 + 1.0)).abs() < 1e-9);
        // Unknown provider: cached tokens at the full input price.
        assert!((table.cost("other", "m", &u).usd - 10.0).abs() < 1e-9);
    }

    #[test]
    fn layers_fill_missing_fields_and_ids_normalize() {
        let builtin = Pricing::from_json(
            r#"{"models":{"claude-sonnet-4-5":{"input":3,"output":15,"max_output":64000,"caps":["vision"]}}}"#,
        )
        .expect("builtin");
        let catalog = Pricing::from_json(
            r#"{"models":{"claude-sonnet-4-5":{"input":2,"output":14,"context_length":200000,"params":["reasoning"]}}}"#,
        )
        .expect("catalog");
        let user =
            Pricing::from_json(r#"{"models":{"claude-sonnet-4-5":{"input":1}}}"#).expect("user");
        let merged = builtin.with_override(catalog).with_override(user);
        let price = merged
            .lookup("anthropic", "claude-sonnet-4.5-20250929")
            .expect("found");
        assert_eq!(price.input, Some(1.0), "user wins");
        assert_eq!(price.output, Some(14.0), "catalog beats builtin");
        assert_eq!(price.max_output, Some(64000), "builtin fills the gap");
        assert_eq!(price.context_length, Some(200_000));
        assert_eq!(
            merged.caps("anthropic", "claude-sonnet-4-5"),
            ["vision"],
            "params known without tools"
        );
        assert_eq!(normalize("Gemini-2.5-Pro:free"), "gemini-2-5-pro");
        assert_eq!(normalize("claude-3-5-haiku-latest"), "claude-3-5-haiku");
        assert!(Pricing::builtin()
            .caps("x", "unknown")
            .contains(&"tools".to_string()));
    }

    #[test]
    fn globs() {
        assert!(glob("claude-*", "claude-x"));
        assert!(glob("a*b*c", "aXbYc"));
        assert!(!glob("a*b", "ac"));
        assert!(glob("exact", "exact"));
        assert!(!glob("gpt-4o*", "gpt-4.1"));
    }
}
