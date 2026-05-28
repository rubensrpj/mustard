//! Knowledge extraction and inter-agent context selection.
//!
//! Two responsibilities, both ported from / built around
//! `_lib/knowledge-extract.js`:
//!
//! 1. **Extraction** — derive friction telemetry and (eventually) genuine
//!    knowledge patterns from parsed `pipeline-state` objects. This is a
//!    behavioural port of `derivePrescription`, `extractFrictionFromStates`,
//!    and `extractPatternsFromStates`.
//! 2. **Context selection** — decide *what* knowledge to inject into a
//!    delegated agent, given the target agent and the pipeline phase. Today
//!    Mustard hands the whole `knowledge.json` dump to every agent; the
//!    purpose of the [`ContextSelector`] trait is to make "inject only the
//!    relevant slice" an explicit, swappable seam.
//!
//! ## The relevance policy is deliberately *not* here
//!
//! Per the b2 spec (§ Preocupações, § Não-Objetivos), the concrete heuristic
//! for "what is relevant to this agent in this phase" is a design decision for
//! B3's ANALYZE phase — and it must never hardcode a technology. So this
//! module exposes the **API** and a single trivial, technology-agnostic
//! baseline ([`PassthroughSelector`]); it does **not** ship an opinionated
//! policy. A future wave plugs a real selector in by implementing
//! [`ContextSelector`] — that trait *is* the extension point.

use serde_json::Value;

// ===========================================================================
// Extraction — port of knowledge-extract.js
// ===========================================================================

/// Tool-call counts for one pipeline, the `metrics.toolBreakdown` object.
///
/// Only the four tool families the JS heuristics inspect are typed; an unknown
/// tool in the JSON is simply ignored, exactly as the JS `Number(breakdown.X)
/// || 0` reads do.
#[derive(Debug, Clone, Copy, Default)]
pub struct ToolBreakdown {
    /// `Bash` tool-call count.
    pub bash: u32,
    /// `Edit` tool-call count.
    pub edit: u32,
    /// `Write` tool-call count.
    pub write: u32,
    /// `Agent` (Task delegation) count.
    pub agent: u32,
}

impl ToolBreakdown {
    /// Read a [`ToolBreakdown`] from a `metrics.toolBreakdown` JSON value.
    /// Missing or non-numeric entries read as `0`.
    #[must_use]
    pub fn from_json(value: &Value) -> Self {
        let count = |key: &str| -> u32 {
            value
                .get(key)
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(0)
        };
        Self {
            bash: count("Bash"),
            edit: count("Edit"),
            write: count("Write"),
            agent: count("Agent"),
        }
    }
}

/// Pipeline-level metrics the extractor reads, the `metrics` object of a
/// `pipeline-state`. Mirrors the fields `knowledge-extract.js` consumes.
#[derive(Debug, Clone, Default)]
pub struct PipelineMetrics {
    /// Hook-level retry count (`metrics.retries`).
    pub retries: u32,
    /// Total tool/API call count (`metrics.apiCalls`).
    pub api_calls: u32,
    /// Per-tool breakdown (`metrics.toolBreakdown`).
    pub tool_breakdown: ToolBreakdown,
}

impl PipelineMetrics {
    /// Read [`PipelineMetrics`] from a `metrics` JSON value. A missing field
    /// reads as its default — fail-open, like the JS `Number(... ) || 0`.
    #[must_use]
    pub fn from_json(value: &Value) -> Self {
        let num = |key: &str| -> u32 {
            value
                .get(key)
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(0)
        };
        Self {
            retries: num("retries"),
            api_calls: num("apiCalls"),
            tool_breakdown: value
                .get("toolBreakdown")
                .map(ToolBreakdown::from_json)
                .unwrap_or_default(),
        }
    }
}

/// Derive a prescription string from pipeline metrics. Port of
/// `derivePrescription`.
///
/// Returns `None` when no heuristic fires. The three heuristics, in the JS
/// first-match-wins order:
///
/// 1. **L0 violation** — `bash + edit > 3 * agent` and `retries > 2`: the
///    parent did heavy work it should have delegated.
/// 2. **Fragmentation** — `api_calls > 50` and `retries > 3`: a single scope
///    ballooned and should have been split.
/// 3. **Reactive iteration** — `edit > 15` and `write < 3`: tweaking files
///    instead of investigating first.
#[must_use]
pub fn derive_prescription(metrics: &PipelineMetrics) -> Option<&'static str> {
    let tb = metrics.tool_breakdown;

    if tb.bash + tb.edit > 3 * tb.agent && metrics.retries > 2 {
        return Some(
            "Next similar pipeline: delegate investigation via Task(general-purpose) \
             BEFORE editing files in sequence. Dominant Bash+Edit without Agent indicates \
             the parent did work that should have been delegated.",
        );
    }
    if metrics.api_calls > 50 && metrics.retries > 3 {
        return Some(
            "Next similar pipeline: split into at least 2 smaller pipelines. \
             A single scope with >50 API calls and >3 retries indicates scope-creep.",
        );
    }
    if tb.edit > 15 && tb.write < 3 {
        return Some(
            "Next similar pipeline: investigate with Read+Grep BEFORE editing. \
             High Edit with low Write count indicates trial-and-error iteration.",
        );
    }
    None
}

/// A friction telemetry entry — measured atrito, not knowledge.
///
/// Mirrors the objects `extractFrictionFromStates` pushes: `type` is always
/// `"friction"`, entries are tagged, and a [`FrictionEntry::prescription`] is
/// attached when [`derive_prescription`] fired (with a `prescriptive` tag).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrictionEntry {
    /// Stable name, keyed on for in-place updates (`high-hook-retry-<label>`
    /// or `heavy-pipeline-<label>`).
    pub name: String,
    /// Human-readable description of the friction signal.
    pub description: String,
    /// Tags — always includes `"friction"`, plus `"prescriptive"` when a
    /// prescription is attached.
    pub tags: Vec<String>,
    /// The honest retry count, present for the `high-hook-retry-*` entry.
    pub retry_count: Option<u32>,
    /// The API-call count, present for the `heavy-pipeline-*` entry.
    pub api_calls: Option<u32>,
    /// Actionable guidance, present when a [`derive_prescription`] heuristic
    /// fired for this pipeline.
    pub prescription: Option<String>,
}

/// Extract friction telemetry from parsed `pipeline-state` objects. Port of
/// `extractFrictionFromStates`.
///
/// Each state may yield up to two entries — a `high-hook-retry-*` entry when
/// `retries > 2`, and a `heavy-pipeline-*` entry when `api_calls > 50` — both
/// carrying the shared prescription if one fired. The `label` for an entry's
/// name is the state's `specName`, then `_file`, then `"unknown"`.
#[must_use]
pub fn extract_friction(states: &[Value]) -> Vec<FrictionEntry> {
    let mut friction = Vec::new();

    for state in states {
        if !state.is_object() {
            continue;
        }
        let metrics = state
            .get("metrics")
            .map(PipelineMetrics::from_json)
            .unwrap_or_default();
        let label = state
            .get("specName")
            .and_then(Value::as_str)
            .or_else(|| state.get("_file").and_then(Value::as_str))
            .unwrap_or("unknown");
        let prescription = derive_prescription(&metrics);

        if metrics.retries > 2 {
            let mut tags = vec!["hook-retry".into(), "pipeline".into(), "friction".into()];
            if prescription.is_some() {
                tags.push("prescriptive".into());
            }
            friction.push(FrictionEntry {
                name: format!("high-hook-retry-{label}"),
                description: format!(
                    "Pipeline triggered {} hook-level retries \
                     (sandbox/stash-pop/re-prompts — not agent redispatches).",
                    metrics.retries
                ),
                tags,
                retry_count: Some(metrics.retries),
                api_calls: None,
                prescription: prescription.map(str::to_string),
            });
        }

        if metrics.api_calls > 50 {
            let mut tags = vec!["optimization".into(), "pipeline".into(), "friction".into()];
            if prescription.is_some() {
                tags.push("prescriptive".into());
            }
            friction.push(FrictionEntry {
                name: format!("heavy-pipeline-{label}"),
                description: format!(
                    "Pipeline used {} API calls. Consider splitting into smaller scope.",
                    metrics.api_calls
                ),
                tags,
                retry_count: None,
                api_calls: Some(metrics.api_calls),
                prescription: prescription.map(str::to_string),
            });
        }
    }

    friction
}

/// A candidate knowledge pattern. Mirrors the entry shape of
/// `extractPatternsFromStates`.
///
/// `extractPatternsFromStates` is intentionally empty in the JS today — real
/// pattern heuristics were never added and friction was split out. [`extract_patterns`]
/// preserves that: it returns an empty `Vec` and stands as the extension point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgePattern {
    /// Pattern category, e.g. `"pattern"`, `"convention"`, `"decision"`.
    pub kind: String,
    /// Short pattern name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Free-form tags.
    pub tags: Vec<String>,
}

/// Extract genuine knowledge patterns from `pipeline-state` objects. Port of
/// `extractPatternsFromStates`.
///
/// Currently returns an empty `Vec` — the JS function is also intentionally
/// empty (friction telemetry was moved to [`extract_friction`]). This is the
/// extension point for real pattern-detection heuristics in a later wave.
#[must_use]
pub fn extract_patterns(states: &[Value]) -> Vec<KnowledgePattern> {
    let _ = states;
    Vec::new()
}

// ===========================================================================
// Context selection — the inter-agent injection API
// ===========================================================================

/// A unit of knowledge that *could* be injected into an agent's context.
///
/// Deliberately minimal and technology-neutral: an `id` to dedupe on, a
/// `kind` (pattern / decision / friction / convention …), the `text` to
/// inject, and free-form `tags`. A [`ContextSelector`] decides which of these
/// reach a given agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextItem {
    /// Stable identifier, used to dedupe.
    pub id: String,
    /// Category of knowledge (`"pattern"`, `"decision"`, `"friction"`, …).
    pub kind: String,
    /// The text injected into the agent's context.
    pub text: String,
    /// Free-form tags a selector may match on.
    pub tags: Vec<String>,
}

/// The request describing *who* the context is for.
///
/// A [`ContextSelector`] receives this alongside the candidate pool. The
/// fields are intentionally generic strings — `agent` is the target subagent
/// role/type, `phase` is the pipeline phase name — so no technology or role
/// taxonomy is baked into this crate. B3's ANALYZE defines the vocabulary and
/// the matching policy.
#[derive(Debug, Clone)]
pub struct SelectionRequest {
    /// The target agent (e.g. a role or `subagent_type`). Opaque to this crate.
    pub agent: String,
    /// The pipeline phase the agent will run in (e.g. `"EXECUTE"`). Opaque.
    pub phase: String,
}

impl SelectionRequest {
    /// Construct a request for `agent` running in `phase`.
    #[must_use]
    pub fn new(agent: impl Into<String>, phase: impl Into<String>) -> Self {
        Self {
            agent: agent.into(),
            phase: phase.into(),
        }
    }
}

/// The extension point for inter-agent context injection.
///
/// Implement this trait to define *which* knowledge reaches *which* agent.
/// `select` is handed the full candidate pool and a [`SelectionRequest`]; it
/// returns only the slice that should be injected — that is the token saving
/// the b2 spec asks for.
///
/// **This crate ships no opinionated policy.** The only built-in is
/// [`PassthroughSelector`], a trivial baseline. The real, possibly
/// per-stack-aware relevance heuristic is a B3-ANALYZE decision and lives in a
/// downstream crate / a later wave — it plugs in here by implementing this
/// trait. Keeping the policy out of `mustard-core` is deliberate (b2 spec §
/// Não-Objetivos): the crate must stay technology-agnostic.
pub trait ContextSelector {
    /// Choose the context items to inject for `request` from `candidates`.
    ///
    /// Implementations must be pure (no I/O) and total (never panic): a
    /// selection failure should degrade to returning fewer items, never crash
    /// the dispatcher.
    fn select(&self, request: &SelectionRequest, candidates: &[ContextItem]) -> Vec<ContextItem>;
}

/// The trivial baseline [`ContextSelector`]: returns every candidate unchanged.
///
/// This is **not** the intended production policy — it is the identity
/// behaviour Mustard has today (inject the whole dump). It exists so the API
/// is usable and testable before B3 supplies a real selector, and as the
/// reference for what a no-op implementation looks like.
#[derive(Debug, Clone, Copy, Default)]
pub struct PassthroughSelector;

impl ContextSelector for PassthroughSelector {
    fn select(&self, request: &SelectionRequest, candidates: &[ContextItem]) -> Vec<ContextItem> {
        let _ = request;
        candidates.to_vec()
    }
}

/// Select context for an agent using `selector`.
///
/// A thin convenience wrapper over [`ContextSelector::select`] so callers have
/// one entry point regardless of which selector is plugged in. The selector is
/// the swappable policy; this function is stable.
#[must_use]
pub fn select_context<S: ContextSelector>(
    selector: &S,
    request: &SelectionRequest,
    candidates: &[ContextItem],
) -> Vec<ContextItem> {
    selector.select(request, candidates)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn derive_prescription_l0_violation() {
        let metrics = PipelineMetrics {
            retries: 3,
            api_calls: 10,
            tool_breakdown: ToolBreakdown {
                bash: 8,
                edit: 6,
                write: 1,
                agent: 1,
            },
        };
        // bash+edit=14 > 3*agent=3, retries=3 > 2 → heuristic 1.
        let p = derive_prescription(&metrics).expect("heuristic 1 fires");
        assert!(p.contains("delegate investigation"));
    }

    #[test]
    fn derive_prescription_fragmentation() {
        let metrics = PipelineMetrics {
            retries: 4,
            api_calls: 60,
            tool_breakdown: ToolBreakdown {
                bash: 1,
                edit: 1,
                write: 1,
                agent: 5,
            },
        };
        let p = derive_prescription(&metrics).expect("heuristic 2 fires");
        assert!(p.contains("split into at least 2"));
    }

    #[test]
    fn derive_prescription_none_when_quiet() {
        let metrics = PipelineMetrics::default();
        assert_eq!(derive_prescription(&metrics), None);
    }

    #[test]
    fn extract_friction_emits_retry_and_heavy_entries() {
        let states = vec![json!({
            "specName": "add-login",
            "metrics": {
                "retries": 5,
                "apiCalls": 80,
                "toolBreakdown": { "Bash": 2, "Edit": 2, "Write": 2, "Agent": 4 }
            }
        })];
        let friction = extract_friction(&states);
        assert_eq!(friction.len(), 2);
        assert_eq!(friction[0].name, "high-hook-retry-add-login");
        assert_eq!(friction[0].retry_count, Some(5));
        assert_eq!(friction[1].name, "heavy-pipeline-add-login");
        assert_eq!(friction[1].api_calls, Some(80));
    }

    #[test]
    fn extract_friction_skips_quiet_pipelines() {
        let states = vec![json!({
            "specName": "tiny",
            "metrics": { "retries": 1, "apiCalls": 10 }
        })];
        assert!(extract_friction(&states).is_empty());
    }

    #[test]
    fn extract_friction_label_falls_back_to_unknown() {
        let states = vec![json!({ "metrics": { "retries": 3 } })];
        let friction = extract_friction(&states);
        assert_eq!(friction[0].name, "high-hook-retry-unknown");
    }

    #[test]
    fn extract_patterns_is_empty_extension_point() {
        let states = vec![json!({ "specName": "x" })];
        assert!(extract_patterns(&states).is_empty());
    }

    #[test]
    fn passthrough_selector_returns_all_candidates() {
        let candidates = vec![
            ContextItem {
                id: "a".into(),
                kind: "pattern".into(),
                text: "use repo pattern".into(),
                tags: vec![],
            },
            ContextItem {
                id: "b".into(),
                kind: "decision".into(),
                text: "chose jiff".into(),
                tags: vec![],
            },
        ];
        let request = SelectionRequest::new("general-purpose", "EXECUTE");
        let selected = select_context(&PassthroughSelector, &request, &candidates);
        assert_eq!(selected, candidates);
    }

    /// A custom [`ContextSelector`] plugs in without touching this crate —
    /// proves the trait is the real extension point.
    #[test]
    fn custom_selector_can_filter() {
        struct KindFilter(&'static str);
        impl ContextSelector for KindFilter {
            fn select(
                &self,
                _request: &SelectionRequest,
                candidates: &[ContextItem],
            ) -> Vec<ContextItem> {
                candidates
                    .iter()
                    .filter(|c| c.kind == self.0)
                    .cloned()
                    .collect()
            }
        }
        let candidates = vec![
            ContextItem {
                id: "a".into(),
                kind: "pattern".into(),
                text: "p".into(),
                tags: vec![],
            },
            ContextItem {
                id: "b".into(),
                kind: "decision".into(),
                text: "d".into(),
                tags: vec![],
            },
        ];
        let request = SelectionRequest::new("Explore", "ANALYZE");
        let selected = select_context(&KindFilter("decision"), &request, &candidates);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].id, "b");
    }
}
