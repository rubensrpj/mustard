//! Claude Code session transcript adapter — line-by-line JSONL.
//!
//! Each Claude Code session writes one JSONL file under
//! `~/.claude/projects/<encoded-cwd>/<session-id>.jsonl`. Every line is a JSON
//! object representing one entry in the conversation. Lines whose `type` is
//! `"assistant"` carry a nested `message.usage` block with the same shape
//! Anthropic returns from the HTTP API:
//!
//! ```json
//! { "type": "assistant",
//!   "message": { "model": "claude-opus-4-7",
//!     "usage": {
//!       "input_tokens": 100,
//!       "output_tokens": 250,
//!       "cache_creation_input_tokens": 0,
//!       "cache_read_input_tokens": 800
//!     } } }
//! ```
//!
//! This adapter walks the file line-by-line, extracts those usage blocks, and
//! returns one [`ApiCostFrame`] per assistant turn — *without* touching SQLite.
//! Pricing is computed locally via
//! [`crate::economy::estimator::model_pricing_usd_micros_per_million`].
//!
//! ## Tolerance
//!
//! Malformed lines (empty, not JSON, missing `message.usage`) are silently
//! skipped — the goal is to never lose a healthy frame just because a sibling
//! line is broken. A line that is *valid JSON* but lacks the usage block is
//! treated as not-an-assistant-turn and skipped without diagnostic noise.
//! Genuinely broken lines (parse error) emit a single `eprintln!` warning and
//! continue; the function returns `Err` only if the file itself cannot be
//! opened.

use std::path::Path;

use serde_json::Value;

use crate::economy::estimator::model_pricing_usd_micros_per_million;
use crate::economy::model::ApiCostFrame;
use crate::error::Result;

use super::IngestContext;
use super::time::now_iso;

/// Parse `transcript_path` as JSONL and return one [`ApiCostFrame`] per
/// assistant turn that carries a `message.usage` block.
///
/// Lines that do not parse as JSON are skipped with a single `eprintln!`
/// warning. Lines that parse but carry no usage block are silently skipped.
///
/// # Errors
///
/// Returns [`Error::Io`] only if the file cannot be opened. Per-line parse
/// errors are absorbed (fail-open) and never propagate.
pub fn ingest(transcript_path: &Path, ctx: &IngestContext) -> Result<Vec<ApiCostFrame>> {
    // Route the read through the canonical filesystem seam. A session
    // transcript is bounded JSONL read once during economy ingest (not a
    // per-tool hot path), so reading it whole rather than streaming is a fair
    // trade for keeping `std::fs` confined to `core::fs`.
    let contents = crate::fs::read_to_string(transcript_path)?;
    let mut out: Vec<ApiCostFrame> = Vec::new();

    for (lineno, text) in contents.lines().enumerate() {
        if text.trim().is_empty() {
            continue;
        }
        let value: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(_) => {
                eprintln!(
                    "transcript::ingest: malformed JSON at line {} of {}; skipping",
                    lineno + 1,
                    transcript_path.display()
                );
                continue;
            }
        };
        if let Some(frame) = translate_line(&value, ctx) {
            out.push(frame);
        }
    }

    Ok(out)
}

/// Translate one parsed JSONL row into an [`ApiCostFrame`], or `None` if it
/// is not an assistant turn with usage data.
fn translate_line(value: &Value, ctx: &IngestContext) -> Option<ApiCostFrame> {
    // Either `type == "assistant"` (Claude Code v1 shape) or simply a
    // `message.usage` block present (forward compatibility — accept any line
    // that carries usage even if the type label changes later).
    let usage = value.get("message").and_then(|m| m.get("usage"))?;
    let model = value
        .get("message")
        .and_then(|m| m.get("model"))
        .and_then(Value::as_str)
        .map(str::to_owned);

    let input_tokens = usage.get("input_tokens").and_then(Value::as_i64);
    let output_tokens = usage.get("output_tokens").and_then(Value::as_i64);
    let cache_read = usage
        .get("cache_read_input_tokens")
        .and_then(Value::as_i64);
    let cache_creation = usage
        .get("cache_creation_input_tokens")
        .and_then(Value::as_i64);

    // Skip if there is genuinely no token information — defensive in case
    // `usage` is present-but-empty.
    if input_tokens.is_none()
        && output_tokens.is_none()
        && cache_read.is_none()
        && cache_creation.is_none()
    {
        return None;
    }

    // Span id: prefer the request_id from the message (Anthropic returns this
    // as `id` at the top of the `message` block); fall back to the line's
    // top-level `uuid` field that Claude Code's transcript writer adds.
    let span_id = value
        .get("message")
        .and_then(|m| m.get("id"))
        .and_then(Value::as_str)
        .or_else(|| value.get("uuid").and_then(Value::as_str))
        .map_or_else(|| String::from("transcript-anon"), str::to_owned);

    let ts = value
        .get("timestamp")
        .and_then(Value::as_str)
        .map_or_else(now_iso, str::to_owned);

    let session_id = value
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| ctx.session_id.clone());

    let cost_usd_micros = price_frame(
        model.as_deref(),
        input_tokens,
        output_tokens,
        cache_read,
    );

    // W4 attribution: when the assistant turn carries a `tool_use` content
    // block, surface its `id` so the writer can persist it into
    // `spans.tool_use_id`. The reader joins this against the matching
    // `agent.start` event payload. The first tool_use block wins — assistant
    // turns rarely emit more than one Task dispatch per message, and the
    // reader's fallback temporal window covers the multi-dispatch edge.
    let mut extra = serde_json::Map::new();
    if let Some(tool_use_id) = extract_tool_use_id(value) {
        extra.insert("tool_use_id".to_owned(), Value::String(tool_use_id));
    }

    Some(ApiCostFrame {
        ts,
        session_id,
        span_id,
        model,
        spec: None,
        phase: None,
        input_tokens,
        output_tokens,
        cache_read_input_tokens: cache_read,
        cache_creation_input_tokens: cache_creation,
        cost_usd_micros,
        is_error: false,
        extra,
    })
}

/// Walk `message.content[]` and return the first `tool_use` block's id.
///
/// Claude Code's transcript shape:
///
/// ```json
/// { "message": { "content": [
///     {"type": "text", "text": "..."},
///     {"type": "tool_use", "id": "toolu_01ABC...", "name": "Task", "input": {...}}
/// ] } }
/// ```
fn extract_tool_use_id(value: &Value) -> Option<String> {
    value
        .get("message")?
        .get("content")?
        .as_array()?
        .iter()
        .find(|c| c.get("type").and_then(Value::as_str) == Some("tool_use"))
        .and_then(|c| c.get("id"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// Compute the cost of a single frame from its token counts.
///
/// ## Fallback policy
///
/// Anthropic never returns tokens for free — but some spans reach this code
/// without a `model` attribute (mustard-rt's own spans, MCP tool calls, older
/// Claude Code exporter versions). Returning `None` in that case made
/// `cost_usd_micros` collapse to SQL NULL, which the aggregator silently
/// turned into `$0.00` in the "Custo estimado por spec / onda" table — a
/// false zero that hid real spend.
///
/// Policy: when `model` is missing or unknown, fall back to `claude-sonnet-4-6`
/// pricing — the project's documented default in `CLAUDE.md § Model Routing`.
/// The fallback is logged via `eprintln!` to stderr so a grep on the
/// mustard-rt logs surfaces every span we had to estimate. The numeric
/// answer is **always an estimate** in this path — callers that need to know
/// the model is unknown should inspect the span attribute, not the cost.
///
/// Only returns `None` for the genuinely degenerate case where both input
/// AND output tokens are zero (or absent) — there is nothing to price.
fn price_frame(
    model: Option<&str>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_read: Option<i64>,
) -> Option<i64> {
    // The project default — kept in sync with `CLAUDE.md § Model Routing`
    // (Default: sonnet). Used both when the span has no model attribute
    // and when the attribute names a model we don't have pricing for.
    const FALLBACK_MODEL: &str = "claude-sonnet-4-6";

    // ── Point 1: degenerate input ──────────────────────────────────────
    // No tokens to price: every branch below would compute zero anyway,
    // and returning `None` keeps the SQL aggregation honest (a true
    // "nothing happened" row is still NULL, not a misleading $0).
    let input = input_tokens.unwrap_or(0).saturating_add(cache_read.unwrap_or(0));
    let output = output_tokens.unwrap_or(0);
    if input == 0 && output == 0 {
        return None;
    }

    // ── Point 2: resolve pricing, falling back when needed ─────────────
    // First try the model the span declares. If that yields (0, 0) — either
    // because `model` was `None` or because the model is missing from the
    // pricing table — apply the documented sonnet fallback. We log each
    // fallback once per span so the cause stays traceable in stderr.
    let (effective_model, in_per_m, out_per_m) = match model {
        Some(m) => {
            let (i, o) = model_pricing_usd_micros_per_million(m);
            if i == 0 && o == 0 {
                // Branch A: model is named but unknown to our pricing table.
                // Examples: a future opus revision shipped before we update
                // the table, or a MCP-only model id we don't track.
                eprintln!(
                    "price_frame: unknown model '{m}' — falling back to {FALLBACK_MODEL} pricing"
                );
                let (fi, fo) = model_pricing_usd_micros_per_million(FALLBACK_MODEL);
                (FALLBACK_MODEL, fi, fo)
            } else {
                (m, i, o)
            }
        }
        None => {
            // Branch B: span carries no model attribute at all.
            // Most common cause is a span emitted by mustard-rt's own hooks
            // (no `gen_ai.request.model`) or an OTEL exporter version that
            // dropped the attribute. Same fallback as Branch A.
            eprintln!(
                "price_frame: span has no model attribute — falling back to {FALLBACK_MODEL} pricing"
            );
            let (fi, fo) = model_pricing_usd_micros_per_million(FALLBACK_MODEL);
            (FALLBACK_MODEL, fi, fo)
        }
    };

    // ── Point 3: defensive — fallback model itself missing from table ──
    // Should never trip in normal operation (sonnet is the canonical entry),
    // but if someone removes the row from the pricing table this prevents a
    // divide-by-zero blast. Return None so the row stays honest.
    if in_per_m == 0 && out_per_m == 0 {
        eprintln!(
            "price_frame: fallback model '{effective_model}' has no pricing entry; emitting NULL"
        );
        return None;
    }

    // ── Point 4: linear cost in micro-USD ──────────────────────────────
    // Pricing table units: `micros_per_million_tokens`. Multiply tokens by
    // that rate, divide by 1_000_000 to get total micros for this frame.
    // `saturating_*` guards against an absurd token count (i64::MAX) shipped
    // by a bogus adapter.
    let cost = input
        .saturating_mul(in_per_m)
        .saturating_add(output.saturating_mul(out_per_m))
        / 1_000_000;
    Some(cost)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use tempfile::tempdir;

    fn fixture_ctx() -> IngestContext {
        IngestContext {
            project_path: "/tmp/p".into(),
            session_id: Some("fallback-sess".into()),
        }
    }

    #[test]
    fn ingest_returns_one_frame_per_assistant_line_skipping_other_lines() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        // Three lines: assistant with usage, a system line w/o usage, broken
        // JSON, then an empty line (silently skipped, not warned).
        let body = concat!(
            r#"{"type":"assistant","sessionId":"s-1","timestamp":"2026-05-21T00:00:00Z","message":{"id":"req-1","model":"claude-3-5-sonnet","usage":{"input_tokens":200,"output_tokens":50,"cache_read_input_tokens":100,"cache_creation_input_tokens":0}}}"#,
            "\n",
            r#"{"type":"system","content":"hello"}"#,
            "\n",
            "this-is-not-json",
            "\n\n",
        );
        crate::fs::write_atomic(&path, body.as_bytes()).unwrap();

        let ctx = fixture_ctx();
        let frames = ingest(&path, &ctx).unwrap();
        assert_eq!(frames.len(), 1);
        let frame = &frames[0];
        assert_eq!(frame.span_id, "req-1");
        assert_eq!(frame.input_tokens, Some(200));
        assert_eq!(frame.cache_read_input_tokens, Some(100));
        assert_eq!(frame.session_id.as_deref(), Some("s-1"));
        // Sonnet: 3/M input, 15/M output. Input (incl. cache) = 300, Output = 50.
        // cost = (300 * 3_000_000 + 50 * 15_000_000) / 1_000_000 = 900 + 750 = 1_650.
        assert_eq!(frame.cost_usd_micros, Some(1_650));
    }

    #[test]
    fn price_frame_falls_back_to_sonnet_when_model_is_none() {
        // Branch B: span without a model attribute. Caller has tokens but no
        // model. Old behaviour: return None → NULL → misleading $0. New
        // behaviour: return Some(...) using sonnet pricing.
        let cost = price_frame(None, Some(1_000), Some(500), None);
        assert!(cost.is_some(), "expected sonnet fallback, got None");
        let micros = cost.expect("computed");
        // Sonnet @ 3/M input + 15/M output = 1000*3 + 500*15 = 3000 + 7500 = 10_500 micros.
        assert_eq!(micros, 10_500);
    }

    #[test]
    fn price_frame_falls_back_to_sonnet_for_unknown_model() {
        // Branch A: model named but not in our pricing table (a future opus
        // build, a fictional gpt-7, etc.). Same fallback as Branch B.
        let cost = price_frame(Some("gpt-7-quantum"), Some(1_000), Some(500), None);
        assert!(cost.is_some(), "expected sonnet fallback for unknown model");
        assert_eq!(cost.expect("computed"), 10_500);
    }

    #[test]
    fn price_frame_returns_none_for_degenerate_empty_frame() {
        // The one case where None is still correct: no tokens at all.
        // Nothing to price; SQL NULL is honest here.
        assert_eq!(price_frame(None, Some(0), Some(0), Some(0)), None);
        assert_eq!(price_frame(None, None, None, None), None);
    }

    #[test]
    fn ingest_returns_error_when_file_missing() {
        let ctx = fixture_ctx();
        let err = ingest(Path::new("/definitely/not/there.jsonl"), &ctx).unwrap_err();
        // The canonical seam maps a missing file to `NotFound` (distinct from a
        // real I/O failure), so callers can fail open on absence.
        assert!(matches!(err, Error::NotFound(_)));
    }

    #[test]
    fn ingest_falls_back_to_ctx_session_id_when_field_absent() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let body = concat!(
            r#"{"type":"assistant","message":{"model":"claude-haiku","usage":{"input_tokens":1,"output_tokens":1}}}"#,
            "\n",
        );
        crate::fs::write_atomic(&path, body.as_bytes()).unwrap();

        let ctx = fixture_ctx();
        let frames = ingest(&path, &ctx).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].session_id.as_deref(), Some("fallback-sess"));
    }
}
