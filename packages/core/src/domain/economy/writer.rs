//! Pure payload builders for the economy NDJSON event channel.
//!
//! W7A of [[2026-05-26-no-sqlite-git-source-of-truth]] retired the SQLite
//! sink. Writers no longer touch a database — they translate domain records
//! into `(event_name, payload)` tuples that the rt-side caller routes through
//! `apps/rt/src/run/event_route::emit` (the only place in the workspace that
//! knows how to write a `HarnessEvent` to NDJSON).
//!
//! This keeps `mustard-core` IO-free (no rusqlite, no filesystem) while
//! preserving the "single source of truth" for the shape of each economy
//! event: change the payload here, every emitter inherits the new shape.
//!
//! ## Builders
//!
//! | Builder | Event name | Source |
//! |---|---|---|
//! | [`savings_event`] | `pipeline.economy.savings.{source}` | Mustard interventions (rtk, budget, model routing, recipe) |
//! | [`context_frame_event`] | `pipeline.economy.context.frame` | dispatch hooks recording prompt composition |
//! | [`run_event`] | `pipeline.economy.run` | tracker.rs estimator (companion channel to OTEL's `pipeline.telemetry.run`) |
//!
//! ## Fail-open contract
//!
//! Every builder is pure — no errors are possible. A serialisation failure on
//! the `extra` map is impossible because `serde_json::Map<String, Value>` is
//! always serialisable. Callers can therefore use the result without a Result
//! wrapper.

use serde_json::{json, Value};

use super::model::{ContextCostFrame, SavingsRecord, SavingsSource, SpanRecord};

/// Build the NDJSON event for a [`SavingsRecord`].
///
/// Returns `(event_name, payload)` where `event_name` is
/// `"pipeline.economy.savings.{source}"` (dotted kebab-case suffix matching the
/// existing kinds emitted by `bash_guard`, `budget`, `model_routing`, etc.)
/// and `payload` is a JSON object the rt caller wraps in a `HarnessEvent`.
///
/// The shape mirrors what `bash_guard::record_savings`/`budget::record_output_cut`
/// already emit today — readers ([`super::reader::savings_breakdown`]) consume
/// the same fields.
#[must_use]
pub fn savings_event(rec: &SavingsRecord) -> (String, Value) {
    let event_name = format!(
        "pipeline.economy.savings.{}",
        savings_suffix(rec.source)
    );
    let payload = json!({
        "source": savings_source_string(rec.source),
        "tokens_saved": rec.tokens_saved,
        "model_target": rec.model_target,
        "project_path": rec.project_path.as_path().to_string_lossy(),
        "spec_id": rec.spec_id.as_ref().map(|s| s.0.clone()),
        "wave_id": rec.wave_id.as_ref().map(|w| w.0.clone()),
        "agent_id": rec.agent_id.as_ref().map(|a| a.0.clone()),
        "ts": rec.ts,
        // Forward the lenient `extra` map so adapter-specific fields survive
        // the round-trip into NDJSON. The reader does not rely on these but
        // dashboard drill-downs do.
        "extra": Value::Object(rec.extra.clone()),
    });
    (event_name, payload)
}

/// Build the NDJSON event for a [`ContextCostFrame`].
///
/// Returns `(event_name, payload)` for the dispatch hook to emit. `event_name`
/// is always `pipeline.economy.context.frame` — context frames are not split
/// per source. The reader ([`super::reader::context_routing_quality`])
/// aggregates these into prefix-stable / retry-overhead ratios.
#[must_use]
pub fn context_frame_event(rec: &ContextCostFrame) -> (String, Value) {
    let payload = json!({
        "agent_id": rec.agent_id.0,
        "wave_id": rec.wave_id.as_ref().map(|w| w.0.clone()),
        "spec_id": rec.spec_id.as_ref().map(|s| s.0.clone()),
        "project_path": rec.project_path.as_path().to_string_lossy(),
        "prompt_size_bytes": rec.prompt_size_bytes,
        "prefix_stable_bytes": rec.prefix_stable_bytes,
        "slice_bytes": rec.slice_bytes,
        "wave_slice_bytes": rec.wave_slice_bytes,
        "return_size_bytes": rec.return_size_bytes,
        "retry_overhead_bytes": rec.retry_overhead_bytes,
        "ts": rec.ts,
        "extra": Value::Object(rec.extra.clone()),
    });
    ("pipeline.economy.context.frame".to_string(), payload)
}

/// Build the NDJSON event for a [`SpanRecord`] (one Anthropic request worth
/// of token usage + priced cost).
///
/// Returns `(event_name, payload)`. The event name is `pipeline.economy.run`
/// — a companion channel to OTEL's `pipeline.telemetry.run`, so the same
/// reader aggregations work for both: tracker.rs's estimated runs and the
/// OTEL collector's measured spans.
///
/// Payload shape matches the legacy `telemetry::run_usage` columns 1:1 so the
/// dashboard reader can `serde_json::from_value::<SpanRecord>` directly.
#[must_use]
pub fn run_event(rec: &SpanRecord) -> (String, Value) {
    // Mirror `SpanRecord`'s serde shape, but pull `tool_use_id` / `wave_id` /
    // `agent_id` out of `extra` into top-level keys so the NDJSON consumer
    // does not have to dereference a nested map for attribution.
    let tool_use_id = rec
        .extra
        .get("tool_use_id")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let wave_id = rec
        .extra
        .get("wave_id")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let agent_id = rec
        .extra
        .get("agent_id")
        .and_then(|v| v.as_str())
        .map(str::to_owned);

    let payload = json!({
        "ts": rec.ts,
        "session_id": rec.session_id,
        "span_id": rec.span_id,
        "model": rec.model,
        "spec": rec.spec,
        "phase": rec.phase,
        "input_tokens": rec.input_tokens,
        "output_tokens": rec.output_tokens,
        "cache_read_input_tokens": rec.cache_read_input_tokens,
        "cache_creation_input_tokens": rec.cache_creation_input_tokens,
        "cost_usd_micros": rec.cost_usd_micros,
        "is_error": rec.is_error,
        "tool_use_id": tool_use_id,
        "wave_id": wave_id,
        "agent_id": agent_id,
        "extra": Value::Object(rec.extra.clone()),
    });
    ("pipeline.economy.run".to_string(), payload)
}

/// Estimate the token count an agent did NOT have to emit because
/// `recipe-match` injected a 90%-complete skeleton into its prompt.
///
/// The proxy is `skeleton.chars() / 4` — same heuristic the budget hook uses.
#[must_use]
pub fn injection_savings_tokens(skeleton: &str) -> i64 {
    let chars = skeleton.chars().count();
    i64::try_from(chars / 4).unwrap_or(i64::MAX)
}

/// Map [`SavingsSource`] to the kebab-case suffix used by the event name.
///
/// Mirrors the kinds already emitted: `bash-guard-block`, `rtk-rewrite`,
/// `model-routing-downgrade`, `budget-output-cut`, `recipe-injection`.
fn savings_suffix(source: SavingsSource) -> &'static str {
    match source {
        SavingsSource::RtkRewrite => "rtk-rewrite",
        SavingsSource::ModelRoutingDowngrade => "model-routing-downgrade",
        SavingsSource::BashGuardBlock => "bash-guard-block",
        SavingsSource::BudgetOutputCut => "budget-output-cut",
        SavingsSource::RecipeInjection => "recipe-injection",
        SavingsSource::ScanStructuralExtract => "scan-structural-extract",
        SavingsSource::ScanSkillRender => "scan-skill-render",
    }
}

/// Snake_case label for the `source` payload field — matches the enum
/// serde rename. Distinct from the kebab-case event suffix because the
/// dashboard groups by this string.
fn savings_source_string(source: SavingsSource) -> &'static str {
    source.as_str()
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::economy::scope::{AgentId, ProjectPath, SpecId, WaveId};
    use serde_json::Map;

    #[test]
    fn savings_event_kebab_suffix_matches_source() {
        let rec = SavingsRecord {
            ts: "2026-05-21T00:00:00Z".into(),
            source: SavingsSource::RtkRewrite,
            tokens_saved: 1200,
            model_target: Some("claude-3-5-sonnet".into()),
            project_path: ProjectPath::new("/tmp/p"),
            spec_id: Some(SpecId::new("spec-A")),
            wave_id: Some(WaveId::new("w1")),
            agent_id: Some(AgentId::new("explore")),
            extra: Map::new(),
        };
        let (ev, payload) = savings_event(&rec);
        assert_eq!(ev, "pipeline.economy.savings.rtk-rewrite");
        assert_eq!(payload["tokens_saved"], json!(1200));
        assert_eq!(payload["source"], json!("rtk_rewrite"));
        assert_eq!(payload["spec_id"], json!("spec-A"));
        assert_eq!(payload["wave_id"], json!("w1"));
        assert_eq!(payload["agent_id"], json!("explore"));
    }

    #[test]
    fn savings_event_covers_every_source_variant() {
        let cases = [
            (SavingsSource::RtkRewrite, "pipeline.economy.savings.rtk-rewrite"),
            (
                SavingsSource::ModelRoutingDowngrade,
                "pipeline.economy.savings.model-routing-downgrade",
            ),
            (
                SavingsSource::BashGuardBlock,
                "pipeline.economy.savings.bash-guard-block",
            ),
            (
                SavingsSource::BudgetOutputCut,
                "pipeline.economy.savings.budget-output-cut",
            ),
            (
                SavingsSource::RecipeInjection,
                "pipeline.economy.savings.recipe-injection",
            ),
            (
                SavingsSource::ScanStructuralExtract,
                "pipeline.economy.savings.scan-structural-extract",
            ),
            (
                SavingsSource::ScanSkillRender,
                "pipeline.economy.savings.scan-skill-render",
            ),
        ];
        for (src, expected_event) in cases {
            let rec = SavingsRecord {
                ts: "2026-05-21T00:00:00Z".into(),
                source: src,
                tokens_saved: 1,
                model_target: None,
                project_path: ProjectPath::new("/tmp/p"),
                spec_id: None,
                wave_id: None,
                agent_id: None,
                extra: Map::new(),
            };
            let (ev, _) = savings_event(&rec);
            assert_eq!(ev, expected_event);
        }
    }

    #[test]
    fn context_frame_event_carries_all_byte_fields() {
        let rec = ContextCostFrame {
            ts: "2026-05-21T00:00:00Z".into(),
            agent_id: AgentId::new("core-impl"),
            wave_id: Some(WaveId::new("w1")),
            spec_id: Some(SpecId::new("spec-A")),
            project_path: ProjectPath::new("/tmp/p"),
            prompt_size_bytes: Some(20_000),
            prefix_stable_bytes: Some(15_000),
            slice_bytes: Some(3_000),
            wave_slice_bytes: Some(1_500),
            return_size_bytes: Some(800),
            retry_overhead_bytes: Some(0),
            extra: Map::new(),
        };
        let (ev, payload) = context_frame_event(&rec);
        assert_eq!(ev, "pipeline.economy.context.frame");
        assert_eq!(payload["prompt_size_bytes"], json!(20_000));
        assert_eq!(payload["prefix_stable_bytes"], json!(15_000));
        assert_eq!(payload["agent_id"], json!("core-impl"));
    }

    #[test]
    fn run_event_promotes_extra_attribution_keys_to_top_level() {
        let mut extra = Map::new();
        extra.insert("tool_use_id".to_string(), json!("toolu_abc"));
        extra.insert("wave_id".to_string(), json!("w1"));
        extra.insert("agent_id".to_string(), json!("explore"));
        let rec = SpanRecord {
            ts: "2026-05-21T00:00:00Z".into(),
            session_id: Some("s-1".into()),
            span_id: "req-1".into(),
            model: Some("claude-opus-4-7".into()),
            spec: Some("spec-A".into()),
            phase: Some("EXECUTE".into()),
            input_tokens: Some(1000),
            output_tokens: Some(500),
            cache_read_input_tokens: Some(800),
            cache_creation_input_tokens: Some(0),
            cost_usd_micros: Some(25_000),
            is_error: false,
            extra,
        };
        let (ev, payload) = run_event(&rec);
        assert_eq!(ev, "pipeline.economy.run");
        assert_eq!(payload["tool_use_id"], json!("toolu_abc"));
        assert_eq!(payload["wave_id"], json!("w1"));
        assert_eq!(payload["agent_id"], json!("explore"));
        assert_eq!(payload["cost_usd_micros"], json!(25_000));
        assert_eq!(payload["spec"], json!("spec-A"));
    }

    #[test]
    fn injection_savings_tokens_proxies_chars_div_4() {
        assert_eq!(injection_savings_tokens(""), 0);
        assert_eq!(injection_savings_tokens("abcd"), 1);
        assert_eq!(injection_savings_tokens(&"x".repeat(100)), 25);
    }

}
