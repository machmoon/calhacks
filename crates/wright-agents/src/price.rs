//! What a response cost, priced from its own `usage` block.
//!
//! Rates are the claude-api skill table dated 2026-10-06 (copied into
//! `docs/research/harness.md`), USD per million tokens. Cache writes are
//! 1.25x input for the 5-minute TTL and 2x for the 1-hour TTL; cache reads
//! are the per-model rate the table names (`shared/prompt-caching.md`,
//! "Economics"). Thinking tokens bill as output and are already inside
//! `output_tokens`.
//!
//! A model this table does not know is priced at the per-bucket maximum of
//! the table ([`CEILING`]: the highest input, output and cache-read rate any
//! row charges, not one row's), never at zero: an unpriced call is the
//! "unpriced battery" defect from `docs/ARCHITECTURE.md` in another costume.

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rate {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
}

/// Claude Haiku 5.5 doubles past a 100K-token prompt (0.50 / 2.50).
const HAIKU_LONG_PROMPT: u64 = 100_000;

/// Every row of [`rate`]'s table at its highest tier (Haiku 5.5 long
/// prompt), used to derive [`CEILING`]. A row added to `rate` must be added
/// here too; `ceiling_covers_every_row` checks the two agree.
#[cfg(test)]
const KNOWN: &[&str] = &[
    "claude-fable-5-1",
    "claude-opus-5-5",
    "claude-sonnet-5-5",
    "claude-sonnet-5",
    "claude-haiku-5-5",
    "claude-opus-5",
    "claude-opus-4-8",
    "claude-opus-4-7",
    "claude-opus-4-6",
    "claude-haiku-4-5",
    "gpt-oss-120b",
];

/// The per-bucket maximum over the table: what an unknown model is charged.
/// Fable 5.1 sets input and output ($10 / $50); Opus 5 / 4.x set cache read
/// ($0.50, above Fable's $0.25).
pub const CEILING: Rate = Rate { input: 10.0, output: 50.0, cache_read: 0.50 };

/// Models `fallbacks: "default"` may answer a request for `model` with, as
/// far as the claude-api skill (`shared/model-migration.md`, 2026-10-06)
/// says: Sonnet 5.5 retries cyber / frontier_llm declines on Sonnet 5 and
/// names Opus 5.5 as a possible target; Opus 5 routes cyber to Opus 4.8;
/// Opus 5.5's targets are "Claude Opus 5 / claude-opus-4-8"; Fable 5.1's are
/// Opus 4.8 and Opus 5. The live list is `allowed_fallback_models` on
/// `/v1/models`, which this offline table cannot read, so Opus 4.8 (the
/// array form's standard substitute) is added for every Sonnet/Opus 5.x
/// request: over-authorizing costs a refused call, under-authorizing costs
/// a blown limit. Sticky routing can serve later turns from the same set.
pub fn fallback_models(model: &str) -> &'static [&'static str] {
    match model {
        "claude-sonnet-5-5" => &["claude-sonnet-5", "claude-opus-5-5", "claude-opus-4-8"],
        "claude-opus-5-5" => &["claude-opus-5", "claude-opus-4-8"],
        "claude-opus-5" => &["claude-opus-4-8"],
        "claude-fable-5-1" => &["claude-opus-4-8", "claude-opus-5"],
        _ => &[],
    }
}

/// Per-MTok rates by model id. `fallbacks: "default"` may answer with a
/// different model than requested ([`fallback_models`]), so the fallback
/// targets are here too; responses are priced by their own `model` field.
pub fn rate(model: &str, prompt_tokens: u64) -> Rate {
    let r = |input, output, cache_read| Rate { input, output, cache_read };
    match model {
        "claude-fable-5-1" => r(10.0, 50.0, 0.25),
        "claude-opus-5-5" => r(4.0, 20.0, 0.20),
        "claude-sonnet-5-5" | "claude-sonnet-5" => r(2.0, 10.0, 0.20),
        "claude-haiku-5-5" if prompt_tokens > HAIKU_LONG_PROMPT => r(0.50, 2.50, 0.05),
        "claude-haiku-5-5" => r(0.10, 0.50, 0.01),
        "claude-opus-5" | "claude-opus-4-8" | "claude-opus-4-7" | "claude-opus-4-6" => r(5.0, 25.0, 0.50),
        "claude-haiku-4-5" => r(1.0, 5.0, 0.10),
        // Foundry Global Standard (azurespeed.com, 2026-10-24); no cached rate is published
        "gpt-oss-120b" => r(0.15, 0.60, 0.15),
        // Anything unknown: the per-bucket top of the table.
        _ => CEILING,
    }
}

impl Rate {
    /// The per-bucket maximum of two rates.
    pub fn max(self, o: Rate) -> Rate {
        Rate { input: self.input.max(o.input), output: self.output.max(o.output), cache_read: self.cache_read.max(o.cache_read) }
    }
}

/// Token counts from a Messages API `usage` object.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Usage {
    /// The uncached remainder of the prompt only.
    pub input: u64,
    pub output: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
    pub cache_read: u64,
}

impl Usage {
    /// Lenient: anything missing or malformed reads as 0. Billing uses
    /// [`Usage::parse`], which refuses to guess.
    pub fn from_json(u: &Value) -> Self {
        let n = |v: &Value| v.as_u64().unwrap_or(0);
        let created = n(&u["cache_creation_input_tokens"]);
        let h1 = n(&u["cache_creation"]["ephemeral_1h_input_tokens"]);
        Usage {
            input: n(&u["input_tokens"]),
            output: n(&u["output_tokens"]),
            cache_write_1h: h1.min(created),
            cache_write_5m: created.saturating_sub(h1),
            cache_read: n(&u["cache_read_input_tokens"]),
        }
    }
    /// Strict: `None` unless `u` is an object whose `input_tokens` and
    /// `output_tokens` are non-negative integers and whose cache fields, when
    /// present and not null, are too. A response this cannot read is booked
    /// at its authorized worst case, never at $0.
    pub fn parse(u: &Value) -> Option<Self> {
        let o = u.as_object()?;
        let opt = |v: Option<&Value>| match v {
            None | Some(Value::Null) => Some(0),
            Some(v) => v.as_u64(),
        };
        let input = o.get("input_tokens")?.as_u64()?;
        let output = o.get("output_tokens")?.as_u64()?;
        let created = opt(o.get("cache_creation_input_tokens"))?;
        let read = opt(o.get("cache_read_input_tokens"))?;
        let h1 = match o.get("cache_creation") {
            None | Some(Value::Null) => 0,
            Some(c) => opt(c.as_object()?.get("ephemeral_1h_input_tokens"))?,
        };
        Some(Usage {
            input,
            output,
            cache_write_1h: h1.min(created),
            cache_write_5m: created.saturating_sub(h1),
            cache_read: read,
        })
    }
    /// The whole prompt: `input_tokens + cache_creation + cache_read`
    /// (`shared/prompt-caching.md`: "input_tokens is the uncached remainder").
    pub fn prompt(&self) -> u64 {
        self.input + self.cache_write_5m + self.cache_write_1h + self.cache_read
    }
    pub fn usd(&self, model: &str) -> f64 {
        let r = rate(model, self.prompt());
        (self.input as f64 * r.input
            + self.cache_write_5m as f64 * r.input * 1.25
            + self.cache_write_1h as f64 * r.input * 2.0
            + self.cache_read as f64 * r.cache_read
            + self.output as f64 * r.output)
            / 1e6
    }
}

/// Worst case for a request not yet sent: the whole body read uncached
/// (about 3 bytes per token, an over-count for English and JSON), plus every
/// output token `max_tokens` allows. The budget refuses on this number.
pub fn worst_case_usd(model: &str, body: &Value) -> f64 {
    worst_case_any_usd(&[model], body)
}

/// [`worst_case_usd`] at the per-bucket highest rate among `models` (the
/// requested model and every model its fallbacks can answer with).
pub fn worst_case_any_usd(models: &[&str], body: &Value) -> f64 {
    let bytes = body.to_string().len() as u64;
    let prompt = bytes / 3 + 1;
    let max_out = body["max_tokens"].as_u64().unwrap_or(16_000);
    let r = models.iter().map(|m| rate(m, prompt)).fold(Rate { input: 0.0, output: 0.0, cache_read: 0.0 }, Rate::max);
    // No model at all is priced as an unknown one.
    let r = if models.is_empty() { CEILING } else { r };
    (prompt as f64 * r.input + max_out as f64 * r.output) / 1e6
}

/// The worst case of a request for `model` sent with `fallbacks`.
pub fn worst_case_with_fallbacks_usd(model: &str, body: &Value) -> f64 {
    let mut ms = vec![model];
    ms.extend_from_slice(fallback_models(model));
    worst_case_any_usd(&ms, body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sonnet_with_cache_prices_each_bucket() {
        let u = Usage::from_json(&json!({
            "input_tokens": 1000, "output_tokens": 2000,
            "cache_creation_input_tokens": 4000, "cache_read_input_tokens": 10000,
            "cache_creation": {"ephemeral_5m_input_tokens": 4000, "ephemeral_1h_input_tokens": 0}
        }));
        // 1000*2 + 4000*2.5 + 10000*0.2 + 2000*10 = 2000+10000+2000+20000 = 34000 / 1e6
        assert!((u.usd("claude-sonnet-5-5") - 0.034).abs() < 1e-12);
    }

    #[test]
    fn unknown_model_is_never_free() {
        let u = Usage { input: 1_000_000, ..Default::default() };
        assert_eq!(u.usd("claude-mystery-9"), 10.0);
    }

    #[test]
    fn ceiling_covers_every_row() {
        // The catch-all is the per-bucket max over the table, so an unknown
        // model never costs less than a known one in any bucket.
        let top = KNOWN.iter().map(|m| rate(m, u64::MAX)).fold(rate(KNOWN[0], 0), Rate::max);
        assert_eq!(top, CEILING);
        assert_eq!(rate("claude-mystery-9", 0), CEILING);
        let cached = Usage { cache_read: 1_000_000, ..Default::default() };
        assert!(cached.usd("claude-mystery-9") >= cached.usd("claude-opus-4-8"));
        assert_eq!(cached.usd("claude-mystery-9"), 0.50);
    }

    #[test]
    fn fallback_worst_case_prices_the_dearest_target() {
        let body = json!({"model": "claude-sonnet-5-5", "max_tokens": 8000, "messages": []});
        let plain = worst_case_usd("claude-sonnet-5-5", &body);
        let fb = worst_case_with_fallbacks_usd("claude-sonnet-5-5", &body);
        assert!((fb - worst_case_usd("claude-opus-4-8", &body)).abs() < 1e-12);
        assert!(fb > plain);
        // No fallbacks: unchanged.
        assert_eq!(worst_case_with_fallbacks_usd("claude-haiku-5-5", &body), worst_case_usd("claude-haiku-5-5", &body));
    }

    #[test]
    fn strict_usage_parse() {
        assert!(Usage::parse(&Value::Null).is_none());
        assert!(Usage::parse(&json!("lots")).is_none());
        assert!(Usage::parse(&json!({"output_tokens": 5})).is_none());
        assert!(Usage::parse(&json!({"input_tokens": -1, "output_tokens": 5})).is_none());
        assert!(Usage::parse(&json!({"input_tokens": 1, "output_tokens": "5"})).is_none());
        assert!(Usage::parse(&json!({"input_tokens": 1, "output_tokens": 5, "cache_read_input_tokens": 1.5})).is_none());
        let u = Usage::parse(&json!({"input_tokens": 1, "output_tokens": 5, "cache_creation_input_tokens": null})).unwrap();
        assert_eq!(u, Usage { input: 1, output: 5, ..Default::default() });
    }

    #[test]
    fn haiku_long_prompt_tier() {
        let u = Usage { input: 200_000, ..Default::default() };
        assert!((u.usd("claude-haiku-5-5") - 0.1).abs() < 1e-12);
    }
}
