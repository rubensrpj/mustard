//! OTLP/JSON traces adapter — translates exported OpenTelemetry spans into
//! [`SpanRecord`]s.
//!
//! The Claude Code native OTLP exporter ships token usage as span attributes
//! whose keys follow the `gen_ai.*` semantic convention:
//!
//! | Attribute | Maps to [`SpanRecord`] field |
//! |---|---|
//! | `gen_ai.usage.input_tokens` | `input_tokens` |
//! | `gen_ai.usage.output_tokens` | `output_tokens` |
//! | `gen_ai.usage.cache_read_input_tokens` | `cache_read_input_tokens` |
//! | `gen_ai.usage.cache_creation_input_tokens` | `cache_creation_input_tokens` |
//! | `gen_ai.request.model` / `gen_ai.response.model` | `model` |
//! | `session.id` | `session_id` (falls back to [`IngestContext::session_id`]) |
//!
//! `cost_usd_micros` is computed locally via
//! [`crate::domain::economy::estimator::model_pricing_usd_micros_per_million`] — the
//! OTEL stream itself does not carry a priced cost, so the adapter prices
//! each span using the model name resolved from the attributes (or `None`
//! when no model was reported, in which case `cost_usd_micros` is `None`).
//!
//! Parsing is **lenient by design**: an unknown OTLP shape, a missing field,
//! or a span without any token-usage attribute is silently skipped — never
//! propagated as an error. The function returns `Err` only when the input
//! string is not parseable JSON at the top level (i.e. structurally broken).
//! Skipped spans never appear in the returned `Vec`.

use serde_json::Value;

use crate::domain::economy::estimator::model_pricing_usd_micros_per_million;
use crate::domain::economy::model::SpanRecord;
use crate::platform::error::{Error, Result};

use super::IngestContext;
use crate::platform::time::{now_iso8601, unix_secs_to_ymdhms};

/// Translate an OTLP/JSON `traces` payload into a list of [`SpanRecord`]s.
///
/// Spans that lack any of the `gen_ai.usage.*` attributes are skipped — the
/// adapter is interested only in spans that carry priced token usage. The
/// returned `Vec` may be empty even on a structurally valid payload.
///
/// # Errors
///
/// Returns [`Error::Parse`] if `otlp_json` is not valid JSON. Malformed sub-
/// structures (missing fields, wrong types) are skipped fail-open and do not
/// produce an error.
pub fn ingest(otlp_json: &str, ctx: &IngestContext) -> Result<Vec<SpanRecord>> {
    let root: Value = serde_json::from_str(otlp_json).map_err(Error::from)?;
    let mut out: Vec<SpanRecord> = Vec::new();

    // OTLP/JSON traces shape: `{ resourceSpans: [{ scopeSpans: [{ spans: [...] }] }] }`.
    // Read defensively: every `as_array` returns `None` for the wrong shape,
    // and `unwrap_or(&[])` keeps the loop silent on malformed payloads.
    let resource_spans = root
        .get("resourceSpans")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    for rs in resource_spans {
        let scope_spans = rs
            .get("scopeSpans")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for ss in scope_spans {
            let spans = ss
                .get("spans")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for span in spans {
                if let Some(record) = translate_span(&span, ctx) {
                    out.push(record);
                }
            }
        }
    }

    Ok(out)
}

/// Translate a single OTLP span into a [`SpanRecord`], or `None` if the span
/// carries no token-usage attributes.
fn translate_span(span: &Value, ctx: &IngestContext) -> Option<SpanRecord> {
    let attrs = extract_attributes(span);

    let input_tokens = attrs.get_i64("gen_ai.usage.input_tokens");
    let output_tokens = attrs.get_i64("gen_ai.usage.output_tokens");
    // The cache split is optional — a span may report only the totals.
    let cache_read = attrs.get_i64("gen_ai.usage.cache_read_input_tokens");
    let cache_creation = attrs.get_i64("gen_ai.usage.cache_creation_input_tokens");

    // Filter: at least one token-usage attribute must be present, else this
    // span is not interesting to the cost pipeline (e.g. a generic tool span).
    if input_tokens.is_none()
        && output_tokens.is_none()
        && cache_read.is_none()
        && cache_creation.is_none()
    {
        return None;
    }

    let model = attrs
        .get_string("gen_ai.response.model")
        .or_else(|| attrs.get_string("gen_ai.request.model"));
    let session_id = attrs
        .get_string("session.id")
        .or_else(|| ctx.session_id.clone());

    // Span identity: OTLP spans expose `spanId` as a hex string. A synthesized
    // sentinel is used when absent so the writer's `INSERT OR REPLACE` still
    // has a primary key to anchor on.
    let span_id = span
        .get("spanId")
        .and_then(Value::as_str)
        .map_or_else(|| String::from("otel-anon"), str::to_owned);

    // OTLP `startTimeUnixNano` is a stringified u64. Convert to an ISO-8601
    // string for the writer's `ts` slot — when absent, fall back to the wall
    // clock at ingest time.
    let ts = span
        .get("startTimeUnixNano")
        .and_then(Value::as_str)
        .and_then(unix_nanos_to_iso)
        .unwrap_or_else(now_iso8601);

    let is_error = span
        .get("status")
        .and_then(|s| s.get("code"))
        .and_then(Value::as_i64)
        .is_some_and(|c| c == 2); // OTLP STATUS_CODE_ERROR = 2.

    let cost_usd_micros = price_span(
        model.as_deref(),
        input_tokens,
        output_tokens,
        cache_read,
    );

    // W4 attribution: collectors that wire `gen_ai.tool_use_id` as a span
    // attribute let the reader join this span to the originating `agent.start`
    // event without falling back to the temporal window. The Anthropic-vendored
    // exporter doesn't emit this today, but downstream collectors (e.g. an
    // OTLP processor that decorates spans from the Bedrock proxy) do.
    let mut extra = serde_json::Map::new();
    if let Some(tool_use_id) = attrs.get_string("gen_ai.tool_use_id") {
        extra.insert("tool_use_id".to_owned(), Value::String(tool_use_id));
    }

    Some(SpanRecord {
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
        is_error,
        extra,
    })
}

/// Compute a span's priced cost in micro-USD, or `None` if pricing cannot be
/// resolved (unknown model OR no input/output tokens reported).
fn price_span(
    model: Option<&str>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_read: Option<i64>,
) -> Option<i64> {
    let model = model?;
    let (in_per_m, out_per_m) = model_pricing_usd_micros_per_million(model);
    if in_per_m == 0 && out_per_m == 0 {
        return None;
    }
    let input = input_tokens.unwrap_or(0);
    let output = output_tokens.unwrap_or(0);
    // Cache-read tokens are billed at a discount but the public price list does
    // not expose a separate column today; fold them into the input bucket so
    // the totals at least represent "what we paid for". A finer split lands
    // when the pricing table grows a cache-read tier.
    let input = input.saturating_add(cache_read.unwrap_or(0));
    let cost = input
        .saturating_mul(in_per_m)
        .saturating_add(output.saturating_mul(out_per_m))
        / 1_000_000;
    Some(cost)
}

/// View into the OTLP `attributes` array as a small lookup helper.
///
/// OTLP attributes are encoded as `[{"key": "...", "value": {"stringValue":
/// "..."}}, ...]`. This wrapper flattens that into key lookups without
/// allocating a full `HashMap`.
struct AttributeView<'a> {
    raw: &'a [Value],
}

impl AttributeView<'_> {
    fn get(&self, key: &str) -> Option<&Value> {
        self.raw
            .iter()
            .find(|kv| kv.get("key").and_then(Value::as_str) == Some(key))
            .and_then(|kv| kv.get("value"))
    }

    fn get_i64(&self, key: &str) -> Option<i64> {
        let v = self.get(key)?;
        v.get("intValue")
            .and_then(|n| {
                // OTLP encodes int64 as either a JSON number or a stringified
                // integer (per spec, large ints SHOULD be strings).
                n.as_i64()
                    .or_else(|| n.as_str().and_then(|s| s.parse::<i64>().ok()))
            })
            .or_else(|| v.get("doubleValue").and_then(Value::as_f64).map(|d| {
                // Intentional truncation: OTLP double attributes used as counters fit in i64.
                #[allow(clippy::cast_possible_truncation)]
                { d as i64 }
            }))
    }

    fn get_string(&self, key: &str) -> Option<String> {
        let v = self.get(key)?;
        v.get("stringValue")
            .and_then(Value::as_str)
            .map(str::to_owned)
    }
}

fn extract_attributes(span: &Value) -> AttributeView<'_> {
    AttributeView {
        raw: span
            .get("attributes")
            .and_then(Value::as_array)
            .map_or(&[][..], |v| v.as_slice()),
    }
}

/// Convert an OTLP `startTimeUnixNano` (stringified u64) to ISO-8601.
///
/// Returns `None` for an unparseable input; the caller falls back to the
/// wall clock. The conversion is a simple proleptic Gregorian (the same
/// algorithm `writer::iso_to_epoch_ms` uses in reverse).
fn unix_nanos_to_iso(s: &str) -> Option<String> {
    let nanos: u128 = s.parse().ok()?;
    // Cast to i64 millis with saturating semantics; nanoseconds beyond i64::MAX
    // millis are not real timestamps.
    let ms_total: i64 = i64::try_from(nanos / 1_000_000).ok()?;
    let secs = ms_total / 1_000;
    // cast_sign_loss: ms_total is derived from a parsed u128 via i64::try_from, so it is non-negative.
    #[allow(clippy::cast_sign_loss)]
    let millis = (ms_total % 1_000) as u32;
    let (y, mo, d, h, mi, s) = unix_secs_to_ymdhms(secs);
    Some(format!(
        "{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{millis:03}Z"
    ))
}


#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_ctx() -> IngestContext {
        IngestContext {
            project_path: "/tmp/proj".into(),
            session_id: Some("ctx-session".into()),
        }
    }

    /// A minimal OTLP/JSON traces payload with one gen_ai span carrying token
    /// usage attributes. Hand-rolled rather than fetched so the test has no
    /// network dependency.
    const OTLP_FIXTURE: &str = r#"{
        "resourceSpans": [{
            "scopeSpans": [{
                "spans": [{
                    "spanId": "abc123",
                    "startTimeUnixNano": "1779321600000000000",
                    "attributes": [
                        {"key": "gen_ai.usage.input_tokens", "value": {"intValue": "1000"}},
                        {"key": "gen_ai.usage.output_tokens", "value": {"intValue": 500}},
                        {"key": "gen_ai.usage.cache_read_input_tokens", "value": {"intValue": "200"}},
                        {"key": "gen_ai.response.model", "value": {"stringValue": "claude-opus-4-7"}},
                        {"key": "session.id", "value": {"stringValue": "sess-xyz"}}
                    ],
                    "status": {"code": 1}
                }, {
                    "spanId": "no-tokens",
                    "attributes": [
                        {"key": "name", "value": {"stringValue": "tool-call"}}
                    ]
                }]
            }]
        }]
    }"#;

    #[test]
    fn ingest_extracts_one_span_skipping_tokenless() {
        let ctx = fixture_ctx();
        let out = ingest(OTLP_FIXTURE, &ctx).unwrap();
        assert_eq!(out.len(), 1, "the tool-call span must be skipped");
        let rec = &out[0];
        assert_eq!(rec.span_id, "abc123");
        assert_eq!(rec.input_tokens, Some(1000));
        assert_eq!(rec.output_tokens, Some(500));
        assert_eq!(rec.cache_read_input_tokens, Some(200));
        assert_eq!(rec.model.as_deref(), Some("claude-opus-4-7"));
        // session.id from the attribute wins over ctx fallback.
        assert_eq!(rec.session_id.as_deref(), Some("sess-xyz"));
        // Opus: 15/M input, 75/M output. Input (incl. cache) = 1200, Output = 500.
        // cost = (1200 * 15_000_000 + 500 * 75_000_000) / 1_000_000 = 18_000 + 37_500 = 55_500.
        assert_eq!(rec.cost_usd_micros, Some(55_500));
        assert!(!rec.is_error);
    }

    #[test]
    fn ingest_falls_back_to_ctx_session_when_attribute_absent() {
        let ctx = fixture_ctx();
        let payload = r#"{
            "resourceSpans": [{
                "scopeSpans": [{
                    "spans": [{
                        "spanId": "s1",
                        "attributes": [
                            {"key": "gen_ai.usage.input_tokens", "value": {"intValue": 1}}
                        ]
                    }]
                }]
            }]
        }"#;
        let out = ingest(payload, &ctx).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].session_id.as_deref(), Some("ctx-session"));
    }

    #[test]
    fn ingest_returns_empty_for_payload_without_resource_spans() {
        let ctx = fixture_ctx();
        let out = ingest("{}", &ctx).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn ingest_propagates_only_top_level_json_errors() {
        let ctx = fixture_ctx();
        let err = ingest("not-json", &ctx).unwrap_err();
        matches!(err, Error::Parse(_));
    }

    #[test]
    fn error_status_code_is_propagated() {
        let payload = r#"{
            "resourceSpans": [{
                "scopeSpans": [{
                    "spans": [{
                        "spanId": "err",
                        "attributes": [
                            {"key": "gen_ai.usage.input_tokens", "value": {"intValue": 10}}
                        ],
                        "status": {"code": 2}
                    }]
                }]
            }]
        }"#;
        let ctx = fixture_ctx();
        let out = ingest(payload, &ctx).unwrap();
        assert!(out[0].is_error);
    }
}
