//! Token-count estimator for prompt previews.
//!
//! Wraps [`tiktoken-rs`](tiktoken_rs) so call sites can ask "roughly how many
//! tokens would Anthropic charge for this string under model X?" without
//! pulling in the dependency themselves. The answer is an *approximation* —
//! Anthropic's tokenizer is proprietary and `tiktoken-rs` ships the OpenAI
//! `cl100k_base` BPE table, which empirically lands within ±5% of Claude's
//! real count on English text. Bit-exact accuracy is a non-goal for W1; the
//! estimator only feeds preview UIs and budget-warning thresholds.
//!
//! The encoder is held behind `tiktoken-rs`'s built-in singleton so the
//! cl100k vocabulary (≈ 1MB of BPE pairs) is built exactly once per process.
//!
//! ## Pricing
//!
//! [`model_pricing_usd_micros_per_million`] returns the per-million-token price
//! for input/output as a tuple of micro-USD. This is the table the dashboard
//! reads to convert raw token counts into a USD figure for the cost cards.
//! Values mirror Anthropic's public price list as of 2026-05; updating it is
//! a one-line edit when a model line is repriced.

use std::sync::OnceLock;

use tiktoken_rs::{CoreBPE, cl100k_base_singleton};

/// Estimate the input-token count of `text` as Anthropic would price it under
/// `model`.
///
/// Returns an approximate count via the `cl100k_base` BPE. The result is
/// always non-negative and saturates at [`u32::MAX`] tokens (≈ 4 billion;
/// every real prompt is dwarfed by this).
///
/// `model` is currently ignored — the same encoder is used for every model
/// line, since the cl100k approximation is what we have. The signature
/// reserves the parameter for the day a Claude-specific tokenizer ships.
#[must_use]
pub fn estimate_input_tokens(text: &str, _model: &str) -> u32 {
    encode_count(text)
}

/// Estimate the output-token count of `text`.
///
/// Same approximation as [`estimate_input_tokens`] today; kept as a separate
/// entry point so a future implementation can apply an output-side
/// adjustment factor without touching every call site.
#[must_use]
pub fn estimate_output_tokens(text: &str, _model: &str) -> u32 {
    encode_count(text)
}

/// Per-million-token price for `model`, returned as `(input, output)` in
/// micro-USD.
///
/// Returns `(0, 0)` for an unknown model — the dashboard renders unpriced
/// rows as "—" rather than a fake total.
///
/// Source: Anthropic public pricing, snapshot 2026-05. Numbers below are in
/// micro-USD per million tokens, i.e. `price_per_token_micros * 1_000_000`.
#[must_use]
pub fn model_pricing_usd_micros_per_million(model: &str) -> (i64, i64) {
    // Normalise the model id: Anthropic returns names like "claude-opus-4-7"
    // sometimes with a trailing date stamp; the pricing tier is keyed on the
    // family prefix, so we match on the longest known prefix.
    let m = model.to_ascii_lowercase();
    if m.starts_with("claude-opus") {
        // $15/M input, $75/M output → 15_000_000 / 75_000_000 micro-USD per M.
        (15_000_000, 75_000_000)
    } else if m.starts_with("claude-3-5-sonnet") || m.starts_with("claude-sonnet") {
        // $3/M input, $15/M output.
        (3_000_000, 15_000_000)
    } else if m.starts_with("claude-3-5-haiku") || m.starts_with("claude-haiku") {
        // $0.80/M input, $4/M output.
        (800_000, 4_000_000)
    } else {
        (0, 0)
    }
}

/// One-time handle on the shared cl100k singleton. `OnceLock` shields the
/// inner reference for the lifetime of the process; calling
/// `cl100k_base_singleton()` directly is itself cheap but the indirection
/// keeps the call site clean and lets future code swap encoders without
/// touching the public API.
fn encoder() -> &'static CoreBPE {
    static CELL: OnceLock<&'static CoreBPE> = OnceLock::new();
    CELL.get_or_init(cl100k_base_singleton)
}

/// Count tokens for `text` via [`encoder`], saturating to [`u32::MAX`].
fn encode_count(text: &str) -> u32 {
    let tokens = encoder().encode_with_special_tokens(text);
    u32::try_from(tokens.len()).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_is_zero_tokens() {
        assert_eq!(estimate_input_tokens("", "claude-3-5-sonnet"), 0);
        assert_eq!(estimate_output_tokens("", "claude-3-5-sonnet"), 0);
    }

    #[test]
    fn short_input_within_tolerance() {
        // "hello world" is 2 tokens under cl100k. Claude tokenizers land
        // within ±1 token on strings this short, so accept 1-3.
        let count = estimate_input_tokens("hello world", "claude-3-5-sonnet");
        assert!((1..=3).contains(&count), "got {count}");
    }

    #[test]
    fn pricing_table_covers_three_tiers() {
        let (opus_in, opus_out) = model_pricing_usd_micros_per_million("claude-opus-4-7");
        assert!(opus_in > 0 && opus_out > 0);
        let (son_in, _) = model_pricing_usd_micros_per_million("claude-3-5-sonnet");
        let (hai_in, _) = model_pricing_usd_micros_per_million("claude-3-5-haiku");
        // Tier ordering must be opus > sonnet > haiku.
        assert!(opus_in > son_in);
        assert!(son_in > hai_in);
    }

    #[test]
    fn unknown_model_returns_zero_pricing() {
        assert_eq!(model_pricing_usd_micros_per_million("gpt-7"), (0, 0));
    }
}
