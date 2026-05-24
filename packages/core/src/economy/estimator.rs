//! Token-count estimator for prompt previews.
//!
//! Wraps [`tiktoken-rs`](tiktoken_rs) so call sites can ask "roughly how many
//! tokens would Anthropic charge for this string under model X?" without
//! pulling in the dependency themselves. The answer is an *approximation* —
//! Anthropic's tokenizer is proprietary and `tiktoken-rs` ships the `OpenAI`
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

/// Compute a frame's cost in micro-USD, splitting tokens into the four
/// Anthropic-priced buckets.
///
/// ## Pricing buckets
///
/// Anthropic bills cache-aware: a single assistant turn can spend tokens in
/// four buckets, each at a different rate relative to the model's base
/// `(input, output)` price:
///
/// | Bucket | Rate |
/// |---|---|
/// | `input` (fresh, uncached) | `rate_in` |
/// | `cache_creation` (writing the prefix to cache) | `rate_in × 5/4` (1.25×) |
/// | `cache_read` (hitting an existing cache entry) | `rate_in / 10` (0.10×) |
/// | `output` (assistant tokens) | `rate_out` |
///
/// In Claude Code workloads the prefix is huge and cached, so most tokens
/// land in `cache_read` — billed at 10% of base. Treating them as full
/// `input` (which the legacy `price_frame` did) inflated cost estimates
/// 3-10× depending on cache hit ratio.
///
/// ## Fallback model
///
/// When `model` is `None` or names a model missing from the pricing table,
/// falls back to `claude-sonnet-4-6` — the project default per
/// `CLAUDE.md § Model Routing`. The fallback is logged via `eprintln!` so a
/// grep on mustard-rt logs surfaces every frame we had to estimate. If the
/// fallback model itself is missing from the table (someone removed the
/// row?), returns `None` rather than divide-by-zero.
///
/// ## Degenerate case
///
/// When all four bucket totals are zero, returns `None` — there is nothing
/// to price and a SQL NULL is the honest value. A misleading zero would
/// otherwise hide unpriced rows in aggregations.
///
/// All arithmetic is `saturating_*` over `i64`; no floats, no panics on
/// absurd token counts shipped by a bogus adapter.
#[must_use]
pub fn compute_cost_micros(
    model: Option<&str>,
    input: i64,
    cache_creation: i64,
    cache_read: i64,
    output: i64,
) -> Option<i64> {
    // The project default — kept in sync with `CLAUDE.md § Model Routing`.
    // Used both when the frame has no model attribute and when the attribute
    // names a model we don't have pricing for.
    const FALLBACK_MODEL: &str = "claude-sonnet-4-6";

    // ── Point 1: degenerate input ──────────────────────────────────────
    // No tokens to price in any bucket: every branch below would compute
    // zero anyway, and returning `None` keeps SQL aggregation honest (a
    // true "nothing happened" row stays NULL, not a misleading $0).
    let input = input.max(0);
    let cache_creation = cache_creation.max(0);
    let cache_read = cache_read.max(0);
    let output = output.max(0);
    if input == 0 && cache_creation == 0 && cache_read == 0 && output == 0 {
        return None;
    }

    // ── Point 2: resolve pricing, falling back when needed ─────────────
    // First try the model the frame declares. If that yields (0, 0) — either
    // because `model` was `None` or because the model is missing from the
    // pricing table — apply the documented sonnet fallback. We log each
    // fallback once per frame so the cause stays traceable in stderr.
    let (effective_model, in_per_m, out_per_m) = if let Some(m) = model {
        let (i, o) = model_pricing_usd_micros_per_million(m);
        if i == 0 && o == 0 {
            eprintln!(
                "compute_cost_micros: unknown model '{m}' — falling back to {FALLBACK_MODEL} pricing"
            );
            let (fi, fo) = model_pricing_usd_micros_per_million(FALLBACK_MODEL);
            (FALLBACK_MODEL, fi, fo)
        } else {
            (m, i, o)
        }
    } else {
        eprintln!(
            "compute_cost_micros: frame has no model attribute — falling back to {FALLBACK_MODEL} pricing"
        );
        let (fi, fo) = model_pricing_usd_micros_per_million(FALLBACK_MODEL);
        (FALLBACK_MODEL, fi, fo)
    };

    // ── Point 3: defensive — fallback model itself missing from table ──
    // Should never trip in normal operation (sonnet is the canonical entry).
    // If someone removes the row from the pricing table this prevents a
    // divide-by-zero blast. Return None so the row stays honest.
    if in_per_m == 0 && out_per_m == 0 {
        eprintln!(
            "compute_cost_micros: fallback model '{effective_model}' has no pricing entry; emitting NULL"
        );
        return None;
    }

    // ── Point 4: cache-aware linear cost in micro-USD ──────────────────
    // Rates from the pricing table are `micros_per_million_tokens`. Each
    // bucket's contribution:
    //
    //   input          → tokens × rate_in
    //   cache_creation → tokens × rate_in × 5 / 4      (1.25× write premium)
    //   cache_read     → tokens × rate_in / 10         (10% hit discount)
    //   output         → tokens × rate_out
    //
    // Integer multipliers (5/4 and 1/10) keep the math floatless. We compute
    // each bucket's micros first, then divide by 1_000_000 at the end.
    // `saturating_*` guards against an absurd token count (i64::MAX) shipped
    // by a bogus adapter.
    let input_micros = input.saturating_mul(in_per_m);
    let creation_micros = cache_creation
        .saturating_mul(in_per_m)
        .saturating_mul(5)
        / 4;
    let cache_read_micros = cache_read.saturating_mul(in_per_m) / 10;
    let output_micros = output.saturating_mul(out_per_m);

    let cost = input_micros
        .saturating_add(creation_micros)
        .saturating_add(cache_read_micros)
        .saturating_add(output_micros)
        / 1_000_000;
    Some(cost)
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

    // ── compute_cost_micros tests ──────────────────────────────────────

    #[test]
    fn compute_cost_micros_input_only_uses_base_rate() {
        // Sonnet: 3_000_000 micros/M input. 1_000 input tokens → 3_000 micros.
        let cost = compute_cost_micros(Some("claude-sonnet-4-6"), 1_000, 0, 0, 0)
            .expect("priced");
        assert_eq!(cost, 3_000);
    }

    #[test]
    fn compute_cost_micros_output_only_uses_output_rate() {
        // Sonnet: 15_000_000 micros/M output. 500 output tokens → 7_500 micros.
        let cost = compute_cost_micros(Some("claude-sonnet-4-6"), 0, 0, 0, 500)
            .expect("priced");
        assert_eq!(cost, 7_500);
    }

    #[test]
    fn compute_cost_micros_cache_read_is_one_tenth_of_input() {
        // 10_000 cache_read tokens at sonnet rates → 3_000 micros
        // (vs. 30_000 micros if treated as fresh input — the 10× regression
        // the cache_read bucket is meant to fix).
        let read_cost = compute_cost_micros(Some("claude-sonnet-4-6"), 0, 0, 10_000, 0)
            .expect("priced");
        let input_equivalent =
            compute_cost_micros(Some("claude-sonnet-4-6"), 10_000, 0, 0, 0)
                .expect("priced");
        assert_eq!(read_cost, input_equivalent / 10);
        assert_eq!(read_cost, 3_000);
    }

    #[test]
    fn compute_cost_micros_cache_creation_is_five_quarters_of_input() {
        // 4_000 cache_creation tokens at sonnet rates → 4_000 * 3_000_000 * 5/4 / 1_000_000
        //   = 12_000 * 5 / 4 = 15_000 micros (1.25× of 12_000 input-equivalent).
        let creation_cost = compute_cost_micros(Some("claude-sonnet-4-6"), 0, 4_000, 0, 0)
            .expect("priced");
        let input_equivalent =
            compute_cost_micros(Some("claude-sonnet-4-6"), 4_000, 0, 0, 0)
                .expect("priced");
        // 1.25 × 12_000 = 15_000.
        assert_eq!(creation_cost, input_equivalent * 5 / 4);
        assert_eq!(creation_cost, 15_000);
    }

    #[test]
    fn compute_cost_micros_realistic_mixed_frame() {
        // Realistic Claude Code turn: huge cached prefix, small fresh input,
        // moderate output, minor cache creation.
        //   input          =     500 → 500 * 3_000_000 = 1_500_000_000 micros
        //   cache_creation =   2_000 → 2_000 * 3_000_000 * 5/4 = 7_500_000_000
        //   cache_read     = 100_000 → 100_000 * 3_000_000 / 10 = 30_000_000_000
        //   output         =   1_000 → 1_000 * 15_000_000 = 15_000_000_000
        //   total micros (raw)        = 54_000_000_000
        //   total / 1_000_000 (cost)  = 54_000
        let cost = compute_cost_micros(
            Some("claude-sonnet-4-6"),
            500,
            2_000,
            100_000,
            1_000,
        )
        .expect("priced");
        assert_eq!(cost, 54_000);
    }

    #[test]
    fn compute_cost_micros_uses_sonnet_fallback_when_model_none() {
        // None should use sonnet pricing — same answer as explicitly naming sonnet.
        let with_none = compute_cost_micros(None, 1_000, 0, 0, 500).expect("priced");
        let with_sonnet =
            compute_cost_micros(Some("claude-sonnet-4-6"), 1_000, 0, 0, 500)
                .expect("priced");
        assert_eq!(with_none, with_sonnet);
        // 1_000 * 3M + 500 * 15M = 3_000 + 7_500 = 10_500 micros.
        assert_eq!(with_none, 10_500);
    }

    #[test]
    fn compute_cost_micros_uses_sonnet_fallback_for_unknown_model() {
        // Unknown model name → sonnet pricing.
        let cost = compute_cost_micros(Some("gpt-7-quantum"), 1_000, 0, 0, 500)
            .expect("priced");
        assert_eq!(cost, 10_500);
    }

    #[test]
    fn compute_cost_micros_degenerate_all_zero_returns_none() {
        assert_eq!(compute_cost_micros(None, 0, 0, 0, 0), None);
        assert_eq!(
            compute_cost_micros(Some("claude-sonnet-4-6"), 0, 0, 0, 0),
            None
        );
    }

    #[test]
    fn compute_cost_micros_opus_priced_per_tier() {
        // Opus: 15_000_000 micros/M input. 1_000 input → 15_000 micros.
        let cost = compute_cost_micros(Some("claude-opus-4-7"), 1_000, 0, 0, 0)
            .expect("priced");
        assert_eq!(cost, 15_000);
    }
}
