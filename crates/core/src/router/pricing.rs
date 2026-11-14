// Ported from 9Router (MIT, © 2024-2026 decolua and contributors): open-sse/providers/pricing.js @ 1855aa4
//! Model prices (USD per 1M tokens), output ceilings and input capabilities,
//! from the editable `pricing.json` shipped next to this file plus an optional
//! user override. Every cost is an estimate; unknown models cost 0.

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
    #[serde(default)]
    pub caps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct Pattern {
    #[serde(rename = "match")]
    pattern: String,
    #[serde(flatten)]
    price: Price,
}

/// The price table.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Pricing {
    #[serde(default)]
    free_providers: Vec<String>,
    #[serde(default)]
    providers: HashMap<String, HashMap<String, Price>>,
    #[serde(default)]
    models: HashMap<String, Price>,
    #[serde(default)]
    patterns: Vec<Pattern>,
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
        serde_json::from_str(text).map_err(|e| format!("pricing file: {e}"))
    }

    /// Layer `user` on top: its entries win, its patterns are tried first.
    #[must_use]
    pub fn with_override(mut self, user: Self) -> Self {
        for provider in user.free_providers {
            if !self.free_providers.contains(&provider) {
                self.free_providers.push(provider);
            }
        }
        for (provider, models) in user.providers {
            self.providers.entry(provider).or_default().extend(models);
        }
        self.models.extend(user.models);
        let mut patterns = user.patterns;
        patterns.append(&mut self.patterns);
        self.patterns = patterns;
        self
    }

    /// Entry for a model, in lookup order. Free providers get a zero price.
    #[must_use]
    pub fn lookup(&self, provider: &str, model: &str) -> Option<Price> {
        if let Some(price) = self.providers.get(provider).and_then(|m| m.get(model)) {
            return Some(price.clone());
        }
        let bare = model.rsplit('/').next().unwrap_or(model);
        let info = self
            .models
            .get(model)
            .or_else(|| self.models.get(bare))
            .cloned()
            .or_else(|| {
                self.patterns
                    .iter()
                    .find(|p| glob(&p.pattern, bare))
                    .map(|p| p.price.clone())
            });
        if self.free_providers.iter().any(|p| p == provider) {
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

    /// Capabilities (`vision`, `pdf`, `audio`, `video`) of a model.
    #[must_use]
    pub fn caps(&self, provider: &str, model: &str) -> Vec<String> {
        self.lookup(provider, model)
            .map_or_else(Vec::new, |p| p.caps)
    }

    /// Estimated cost of one request.
    #[must_use]
    pub fn cost(&self, provider: &str, model: &str, usage: &Usage) -> Cost {
        match self.lookup(provider, model) {
            Some(price) if price.input.is_some() && price.output.is_some() => Cost {
                usd: cost(usage, &price),
                known: true,
            },
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
    fn globs() {
        assert!(glob("claude-*", "claude-x"));
        assert!(glob("a*b*c", "aXbYc"));
        assert!(!glob("a*b", "ac"));
        assert!(glob("exact", "exact"));
        assert!(!glob("gpt-4o*", "gpt-4.1"));
    }
}
