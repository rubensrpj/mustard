//! `model_routing` — the model-selection gate for Task dispatches.
//!
//! ## Scope (b3 Wave 3, Task family)
//!
//! Ports `model-routing-gate.js` 1:1: a `PreToolUse(Task)` gate that compares
//! the model a Task dispatch selected against the pipeline routing table.
//! Upgrades (a more expensive model than required) are blocked; downgrades are
//! allowed (saving money is fine). When no model is specified the gate either
//! denies (an explorer, or a sonnet-expected dispatch in strict mode) or
//! advises.
//!
//! Consolidation here is trivial — it is a single JS hook — but the module
//! still implements [`Check`] behind the `mustard-core` contract so the
//! dispatcher treats it uniformly.
//!
//! ## Mode
//!
//! `MUSTARD_MODEL_GATE_MODE` — `strict` (default) / `warn` / `off`. The gate
//! resolves this itself (it is *not* the dispatcher's module-level mode), and
//! the dispatcher repasses the verdict without downgrade.

use mustard_core::error::Error;
use mustard_core::metrics::{MetricLine, emit_metric};
use mustard_core::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use serde_json::{Value, json};
use std::path::Path;

use crate::util::now_iso8601;

/// The model-routing enforcement module.
pub struct ModelRoutingGate;

// ---------------------------------------------------------------------------
// Model normalisation + cost rank
// ---------------------------------------------------------------------------

/// Normalise a raw model string to a rank key, or `None` if unknown.
/// Port of `normalizeModel` — substring match, haiku → opus → sonnet order.
fn normalize_model(raw: &str) -> Option<&'static str> {
    let s = raw.to_ascii_lowercase();
    if s.contains("haiku") {
        Some("haiku")
    } else if s.contains("opus") {
        Some("opus")
    } else if s.contains("sonnet") {
        Some("sonnet")
    } else {
        None
    }
}

/// Cost rank — higher is more expensive. Port of `MODEL_COST_RANK`. An unknown
/// model resolves to the sonnet tier (`2`), matching the JS `|| 2` fallback.
fn cost_rank(model: &str) -> u8 {
    match model {
        "haiku" => 1,
        "opus" => 3,
        _ => 2, // sonnet, and any unknown name
    }
}

// ---------------------------------------------------------------------------
// Mode
// ---------------------------------------------------------------------------

/// The `MUSTARD_MODEL_GATE_MODE` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GateMode {
    Strict,
    Warn,
    Off,
}

/// Resolve the gate mode. Port of `getMode`: lowercased, default `strict`, an
/// unrecognised value also falls back to `strict`.
fn gate_mode() -> GateMode {
    match std::env::var("MUSTARD_MODEL_GATE_MODE")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "warn" => GateMode::Warn,
        "off" => GateMode::Off,
        _ => GateMode::Strict,
    }
}

// ---------------------------------------------------------------------------
// Pipeline state
// ---------------------------------------------------------------------------

/// The fields of the newest pipeline-state file the gate cares about.
struct PipelineState {
    /// The pipeline `type` field, lowercased (`feature`, `bugfix`, …).
    type_lower: Option<String>,
    /// The raw `type` for metrics (`unknown` when absent).
    type_raw: String,
    /// The `scope` for metrics (`unknown` when absent).
    scope: String,
}

/// Load the newest `.json` pipeline-state under `<project>/.claude/.pipeline-states`
/// (excluding `*.metrics.json`). Port of `loadNewestPipelineState`. Fail-open:
/// any error → `None`.
fn load_newest_pipeline_state(project_dir: &str) -> Option<PipelineState> {
    let dir = Path::new(project_dir)
        .join(".claude")
        .join(".pipeline-states");
    let entries = std::fs::read_dir(&dir).ok()?;
    let mut best: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
    for entry in entries.filter_map(std::result::Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".json") || name.ends_with(".metrics.json") {
            continue;
        }
        let Ok(mtime) = entry.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        if best.as_ref().is_none_or(|(t, _)| mtime > *t) {
            best = Some((mtime, entry.path()));
        }
    }
    let (_, path) = best?;
    let text = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    let type_raw = value
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let type_lower = value
        .get("type")
        .and_then(|v| v.as_str())
        .map(str::to_ascii_lowercase);
    let scope = value
        .get("scope")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    Some(PipelineState {
        type_lower,
        type_raw,
        scope,
    })
}

// ---------------------------------------------------------------------------
// Expected-model resolution
// ---------------------------------------------------------------------------

/// The expected model for a dispatch + the human-readable reason.
struct Expected {
    model: &'static str,
    reason: &'static str,
}

/// Determine the expected model. Port of `determineExpected`.
///
/// `state` is `None` when there is no pipeline-state file.
fn determine_expected(
    subagent_type: &str,
    description: &str,
    state: Option<&PipelineState>,
) -> Expected {
    let agent_type = subagent_type.to_ascii_lowercase();

    // Rule 1: Explore is mechanical search → haiku.
    if agent_type == "explore" {
        return Expected {
            model: "haiku",
            reason: "Explore agents use haiku (mechanical search)",
        };
    }
    // Rule 2: Plan needs deep reasoning → opus.
    if agent_type == "plan" {
        return Expected {
            model: "opus",
            reason: "Plan agents use opus (architectural reasoning)",
        };
    }
    // Rule 2.5: description-verb override — an analysis verb at the start of
    // the description routes to sonnet, unless a high-stakes keyword appears.
    let desc_lower = description.trim().to_ascii_lowercase();
    let is_analysis_verb = starts_with_analysis_verb(&desc_lower);
    let is_high_stakes = contains_high_stakes(description);
    if is_analysis_verb && !is_high_stakes {
        return Expected {
            model: "sonnet",
            reason: "Analysis/review task — sonnet sufficient",
        };
    }
    // Rule 3: active pipeline drives the model.
    if let Some(state) = state {
        match state.type_lower.as_deref() {
            Some("feature") => {
                return Expected {
                    model: "opus",
                    reason: "Feature pipelines use opus (quality-first)",
                };
            }
            Some("bugfix") => {
                return Expected {
                    model: "opus",
                    reason: "Bugfix pipelines use opus (diagnosis needs deep reasoning)",
                };
            }
            _ => {}
        }
    }
    // Default: sonnet.
    Expected {
        model: "sonnet",
        reason: "Default model (analysis/review/planning)",
    }
}

/// `^(review|audit|validate|verify|check|inspect)\b` on the lowercased,
/// trimmed description. Port of `isAnalysisVerb`.
fn starts_with_analysis_verb(desc_lower: &str) -> bool {
    const VERBS: &[&str] = &[
        "review", "audit", "validate", "verify", "check", "inspect",
    ];
    for verb in VERBS {
        if let Some(rest) = desc_lower.strip_prefix(verb) {
            // `\b` after the verb: end-of-string or a non-word char.
            if rest
                .chars()
                .next()
                .is_none_or(|c| !c.is_alphanumeric() && c != '_')
            {
                return true;
            }
        }
    }
    false
}

/// `\b(security|critical|production)\b` anywhere in the (original-case)
/// description, case-insensitively. Port of `isHighStakes`.
fn contains_high_stakes(description: &str) -> bool {
    let lower = description.to_ascii_lowercase();
    const WORDS: &[&str] = &["security", "critical", "production"];
    WORDS.iter().any(|w| has_word(&lower, w))
}

/// `true` if `needle` appears in `haystack` with word boundaries on both
/// sides. Both arguments must already be lowercased.
fn has_word(haystack: &str, needle: &str) -> bool {
    let mut from = 0;
    while let Some(rel) = haystack[from..].find(needle) {
        let start = from + rel;
        let end = start + needle.len();
        let left_ok = start == 0
            || !haystack.as_bytes()[start - 1].is_ascii_alphanumeric()
                && haystack.as_bytes()[start - 1] != b'_';
        let right_ok = haystack
            .as_bytes()
            .get(end)
            .is_none_or(|c| !c.is_ascii_alphanumeric() && *c != b'_');
        if left_ok && right_ok {
            return true;
        }
        from = end;
    }
    false
}

// ---------------------------------------------------------------------------
// The gate
// ---------------------------------------------------------------------------

/// The `model-routing-gate` decision for a `PreToolUse(Task)` dispatch.
///
/// Returns the verdict, 1:1 with `model-routing-gate.js`. `project_dir` is
/// where pipeline-state files are read from; `mode` is the resolved
/// `MUSTARD_MODEL_GATE_MODE` — passed in so the gate is testable without
/// mutating process environment.
fn model_routing_gate(input: &HookInput, project_dir: &str, mode: GateMode) -> Verdict {
    let tool_input = &input.tool_input;
    let raw_model = tool_input
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let subagent_type = tool_input
        .get("subagent_type")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let description = tool_input
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    // ── No model specified ─────────────────────────────────────────────────
    if raw_model.is_empty() {
        let state = load_newest_pipeline_state(project_dir);
        let expected = determine_expected(subagent_type, description, state.as_ref());
        let agent_type_lower = subagent_type.to_ascii_lowercase();
        let is_explorer =
            agent_type_lower == "explore" || agent_type_lower.contains("explorer");

        if is_explorer {
            emit_routing_metric(
                project_dir,
                "no-model-denied",
                &expected,
                "inherited",
                state.as_ref(),
                subagent_type,
                None,
                "prevention",
            );
            return Verdict::Deny {
                reason: format!(
                    "[Model Routing] Explorer agents must specify model explicitly \
                     (haiku or sonnet). Add model: \"haiku\" to your Task dispatch. \
                     {}.",
                    expected.reason
                ),
            };
        }

        // Non-explorer, expected sonnet, strict mode → deny.
        if expected.model == "sonnet" && mode == GateMode::Strict {
            emit_routing_metric(
                project_dir,
                "no-model-denied-sonnet",
                &expected,
                "inherited",
                state.as_ref(),
                subagent_type,
                None,
                "prevention",
            );
            return Verdict::Deny {
                reason: format!(
                    "[Model Routing] No model specified — this task should use \
                     model: '{}'. {}. Add model: '{}' to your Task dispatch (or set \
                     MUSTARD_MODEL_GATE_MODE=warn to downgrade to an advisory).",
                    expected.model, expected.reason, expected.model
                ),
            };
        }

        // Non-explorer, expected != opus → advisory. (Expected opus with an
        // inherited model is presumed to match → silent allow.)
        if expected.model != "opus" {
            emit_routing_metric(
                project_dir,
                "no-model-advisory",
                &expected,
                "inherited",
                state.as_ref(),
                subagent_type,
                None,
                "routing-advisory",
            );
            return Verdict::Inject {
                context: format!(
                    "[Model Gate] No model specified — this task should use \
                     model: '{}'. {}. Add model: '{}' to reduce costs.",
                    expected.model, expected.reason, expected.model
                ),
            };
        }
        return Verdict::Allow;
    }

    // ── Model specified ────────────────────────────────────────────────────
    let Some(model) = normalize_model(raw_model) else {
        // Unknown model name — cannot rank, skip.
        return Verdict::Allow;
    };

    if mode == GateMode::Off {
        return Verdict::Allow;
    }

    let state = load_newest_pipeline_state(project_dir);
    let expected = determine_expected(subagent_type, description, state.as_ref());
    let is_violation = cost_rank(model) > cost_rank(expected.model);

    // Emit the gate-check metric on every check.
    emit_routing_metric(
        project_dir,
        if is_violation { "violation" } else { "passed" },
        &expected,
        model,
        state.as_ref(),
        subagent_type,
        Some(mode),
        if is_violation && mode == GateMode::Strict {
            "prevention"
        } else {
            "routing"
        },
    );

    if !is_violation {
        return Verdict::Allow;
    }

    // ── Violation ──────────────────────────────────────────────────────────
    if mode == GateMode::Warn {
        return Verdict::Inject {
            context: format!(
                "[Model Gate] Expected {} for this task ({}). Consider using \
                 model: '{}' to reduce costs.",
                expected.model, expected.reason, expected.model
            ),
        };
    }
    // mode == strict
    Verdict::Deny {
        reason: format!(
            "[Model Gate] Task requires '{}' model, not '{}'. Reason: {}. \
             Re-dispatch with model: '{}'.",
            expected.model, model, expected.reason, expected.model
        ),
    }
}

/// Emit a `model-routing-gate` metric line. Fail-silent.
#[allow(clippy::too_many_arguments)]
fn emit_routing_metric(
    project_dir: &str,
    note: &str,
    expected: &Expected,
    actual: &str,
    state: Option<&PipelineState>,
    subagent_type: &str,
    mode: Option<GateMode>,
    category: &str,
) {
    let mut extras = serde_json::Map::new();
    extras.insert("expected".into(), json!(expected.model));
    extras.insert("actual".into(), json!(actual));
    extras.insert(
        "pipeline_type".into(),
        json!(state.map_or("none", |s| s.type_raw.as_str())),
    );
    extras.insert(
        "scope".into(),
        json!(state.map_or("none", |s| s.scope.as_str())),
    );
    extras.insert("reason".into(), json!(expected.reason));
    extras.insert("subagent_type".into(), json!(subagent_type));
    extras.insert("category".into(), json!(category));
    if let Some(mode) = mode {
        let mode_str = match mode {
            GateMode::Strict => "strict",
            GateMode::Warn => "warn",
            GateMode::Off => "off",
        };
        extras.insert("mode".into(), json!(mode_str));
    }
    let line = MetricLine::new(now_iso8601(), "model-routing-gate")
        .tokens_affected(0)
        .tokens_saved(0)
        .note(note)
        .extras(Value::Object(extras));
    let _ = emit_metric(Path::new(project_dir), &line);
}

// ---------------------------------------------------------------------------
// Contract impl
// ---------------------------------------------------------------------------

impl Check for ModelRoutingGate {
    /// Gate a `PreToolUse(Task)` dispatch on the selected model.
    ///
    /// Only `PreToolUse` + a `Task`/`Agent` tool runs the gate; any other
    /// invocation self-allows.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        if !matches!(input.tool_name.as_deref(), Some("Task") | Some("Agent")) {
            return Ok(Verdict::Allow);
        }
        let project_dir = if ctx.project_dir.is_empty() {
            input.cwd.as_deref().unwrap_or(".")
        } else {
            ctx.project_dir.as_str()
        };
        Ok(model_routing_gate(input, project_dir, gate_mode()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use tempfile::tempdir;

    /// Build a `PreToolUse(Task)` input.
    fn task_input(subagent: &str, model: Option<&str>, description: &str) -> HookInput {
        let mut ti = serde_json::Map::new();
        ti.insert("subagent_type".into(), json!(subagent));
        ti.insert("description".into(), json!(description));
        ti.insert("prompt".into(), json!("test"));
        if let Some(m) = model {
            ti.insert("model".into(), json!(m));
        }
        HookInput {
            tool_name: Some("Task".to_string()),
            tool_input: Value::Object(ti),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        }
    }

    /// Write a pipeline-state file under `project_dir`.
    fn write_state(project_dir: &Path, state: Value) {
        let dir = project_dir.join(".claude").join(".pipeline-states");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("test.json"), state.to_string()).unwrap();
    }

    /// Run the gate core directly with an explicit mode — keeps tests free of
    /// process-environment mutation (`cargo test` runs threads in parallel).
    fn run_mode(input: &HookInput, project_dir: &str, mode: GateMode) -> Verdict {
        model_routing_gate(input, project_dir, mode)
    }

    /// Run with the strict default mode.
    fn run(input: &HookInput, project_dir: &str) -> Verdict {
        run_mode(input, project_dir, GateMode::Strict)
    }

    // --- normalisation -----------------------------------------------------

    #[test]
    fn normalize_model_recognises_known_names() {
        assert_eq!(normalize_model("claude-3-haiku-20240307"), Some("haiku"));
        assert_eq!(normalize_model("claude-sonnet-4-5"), Some("sonnet"));
        assert_eq!(normalize_model("opus"), Some("opus"));
        assert_eq!(normalize_model("gpt-4"), None);
    }

    #[test]
    fn cost_rank_orders_models() {
        assert!(cost_rank("haiku") < cost_rank("sonnet"));
        assert!(cost_rank("sonnet") < cost_rank("opus"));
    }

    // --- expected-model resolution -----------------------------------------

    #[test]
    fn explore_expects_haiku() {
        let e = determine_expected("Explore", "search files", None);
        assert_eq!(e.model, "haiku");
    }

    #[test]
    fn plan_expects_opus() {
        let e = determine_expected("Plan", "design schema", None);
        assert_eq!(e.model, "opus");
    }

    #[test]
    fn analysis_verb_routes_to_sonnet() {
        let e = determine_expected("general-purpose", "Review the auth module", None);
        assert_eq!(e.model, "sonnet");
    }

    #[test]
    fn high_stakes_keyword_suppresses_analysis_verb_override() {
        // `isAnalysisVerb && !isHighStakes` — a high-stakes keyword suppresses
        // the rule-2.5 sonnet override, so resolution falls through to rule 3
        // (the pipeline type). With a feature pipeline that means opus, not the
        // sonnet the bare analysis verb would have produced.
        let state = PipelineState {
            type_lower: Some("feature".to_string()),
            type_raw: "feature".to_string(),
            scope: "full".to_string(),
        };
        let with_stakes = determine_expected(
            "general-purpose",
            "Review the security of the login flow",
            Some(&state),
        );
        assert_eq!(with_stakes.model, "opus");
        // Without the high-stakes keyword the same verb routes to sonnet even
        // inside the feature pipeline (rule 2.5 fires first).
        let no_stakes =
            determine_expected("general-purpose", "Review the login flow", Some(&state));
        assert_eq!(no_stakes.model, "sonnet");
    }

    // --- gate routing ------------------------------------------------------

    #[test]
    fn non_task_tool_allows() {
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: ".".to_string(),
            trigger: Some(Trigger::PreToolUse),
        };
        assert_eq!(
            ModelRoutingGate.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }

    #[test]
    fn non_pre_tool_use_trigger_allows() {
        let input = task_input("Explore", Some("opus"), "");
        let ctx = Ctx {
            project_dir: String::new(),
            trigger: Some(Trigger::PostToolUse),
        };
        assert_eq!(
            ModelRoutingGate.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }

    #[test]
    fn upgrade_is_blocked_explore_with_opus() {
        // Explore expects haiku; dispatching with opus is an upgrade → deny.
        let dir = tempdir().unwrap();
        let input = task_input("Explore", Some("opus"), "search");
        match run(&input, dir.path().to_str().unwrap()) {
            Verdict::Deny { reason } => assert!(reason.contains("haiku")),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn downgrade_is_allowed_plan_with_sonnet() {
        // Plan expects opus; dispatching with sonnet is a downgrade → allow.
        let dir = tempdir().unwrap();
        let input = task_input("Plan", Some("sonnet"), "design");
        assert_eq!(run(&input, dir.path().to_str().unwrap()), Verdict::Allow);
    }

    #[test]
    fn matching_model_allows() {
        let dir = tempdir().unwrap();
        let input = task_input("Explore", Some("haiku"), "search");
        assert_eq!(run(&input, dir.path().to_str().unwrap()), Verdict::Allow);
    }

    #[test]
    fn no_model_in_feature_pipeline_allows_silently() {
        // Feature pipeline → expected opus; inherited model presumed to match.
        let dir = tempdir().unwrap();
        write_state(
            dir.path(),
            json!({ "type": "feature", "scope": "full", "phaseName": "EXECUTE" }),
        );
        let input = task_input("general-purpose", None, "do work");
        assert_eq!(run(&input, dir.path().to_str().unwrap()), Verdict::Allow);
    }

    #[test]
    fn no_model_explorer_always_denies() {
        let dir = tempdir().unwrap();
        let input = task_input("Explore", None, "search");
        match run(&input, dir.path().to_str().unwrap()) {
            Verdict::Deny { reason } => assert!(reason.contains("Explorer")),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn no_model_sonnet_expected_denies_in_strict() {
        // No pipeline-state → expected sonnet → strict (default) denies.
        let dir = tempdir().unwrap();
        let input = task_input("general-purpose", None, "do work");
        match run(&input, dir.path().to_str().unwrap()) {
            Verdict::Deny { reason } => assert!(reason.contains("sonnet")),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn off_mode_skips_violation_check() {
        // Off mode: an Explore-with-opus upgrade that would deny in strict
        // passes through untouched.
        let dir = tempdir().unwrap();
        let input = task_input("Explore", Some("opus"), "search");
        assert_eq!(
            run_mode(&input, dir.path().to_str().unwrap(), GateMode::Off),
            Verdict::Allow
        );
    }

    #[test]
    fn warn_mode_advises_instead_of_denying() {
        // Warn mode: the Explore-with-opus upgrade injects an advisory, not a
        // deny.
        let dir = tempdir().unwrap();
        let input = task_input("Explore", Some("opus"), "search");
        match run_mode(&input, dir.path().to_str().unwrap(), GateMode::Warn) {
            Verdict::Inject { context } => assert!(context.contains("Model Gate")),
            other => panic!("expected Inject advisory, got {other:?}"),
        }
    }

    #[test]
    fn warn_mode_no_model_sonnet_advises_not_denies() {
        // No model + expected sonnet, warn mode → advisory, never deny.
        let dir = tempdir().unwrap();
        let input = task_input("general-purpose", None, "do work");
        match run_mode(&input, dir.path().to_str().unwrap(), GateMode::Warn) {
            Verdict::Inject { .. } => {}
            other => panic!("expected Inject advisory, got {other:?}"),
        }
    }

}
