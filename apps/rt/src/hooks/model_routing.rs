//! `model_routing` — the model-selection gate for Task dispatches.
//!
//! ## Scope (b3 Wave 3, Task family)
//!
//! A `PreToolUse(Task)` gate that compares the model a Task dispatch selected
//! against the pipeline routing table. Routing is fully structural — the
//! dispatch description is never parsed.
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

use mustard_core::economy::estimator;
use mustard_core::economy::writer;
use mustard_core::ClaudePaths;
use mustard_core::economy::{
    AgentId, ProjectPath, SavingsRecord, SavingsSource, SpecId, WaveId,
};
use mustard_core::error::Error;
use mustard_core::metrics::{MetricLine, emit_metric};
use mustard_core::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::store::sqlite_store::SqliteEventStore;
use rusqlite::Connection;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

use crate::run::current_spec;
use crate::util::now_iso8601;

/// Resolve the harness `SQLite` path the same way [`SqliteEventStore::for_project`]
/// does internally — env override `MUSTARD_DB_PATH` wins, else
/// `{project_dir}/.claude/.harness/mustard.db`. Kept private here so the
/// `mustard-core` surface need not grow a public connection accessor for W2.
fn economy_db_path(project_dir: &str) -> PathBuf {
    if let Ok(value) = std::env::var("MUSTARD_DB_PATH") {
        if !value.trim().is_empty() {
            return PathBuf::from(value);
        }
    }
    ClaudePaths::for_project(Path::new(project_dir))
        .map(|p| p.harness_dir().join("mustard.db"))
        .unwrap_or_else(|_| PathBuf::from(project_dir).join("mustard.db"))
}

/// Open a raw [`Connection`] to the harness DB, applying schema/migrations
/// via [`SqliteEventStore::for_project`] first. Returns `None` on failure;
/// the routing gate must remain fail-open.
fn open_economy_conn(project_dir: &str) -> Option<Connection> {
    let _ = SqliteEventStore::for_project(project_dir).ok()?;
    Connection::open(economy_db_path(project_dir)).ok()
}

/// Record a `ModelRoutingDowngrade` savings event for a Task dispatch that the
/// gate rerouted from a more expensive model to a cheaper one. `from_model`
/// and `to_model` are the normalised tier names (`"opus"`, `"sonnet"`,
/// `"haiku"`); `prompt` is the dispatch text the cheaper model will receive.
/// Fail-open on every error path — telemetry never blocks the verdict.
fn record_routing_downgrade(
    project_dir: &str,
    from_model: &str,
    to_model: &str,
    prompt: &str,
    subagent_type: &str,
) {
    if from_model == to_model || prompt.is_empty() {
        return;
    }
    let Some(conn) = open_economy_conn(project_dir) else {
        return;
    };
    // Approximate savings as the input-token count we would have spent on the
    // more expensive tier — the dashboard can re-price via the pricing table.
    let tokens = i64::from(estimator::estimate_input_tokens(prompt, from_model));
    let tokens = tokens.max(1);
    let agent_id = if subagent_type.is_empty() {
        "model_routing".to_string()
    } else {
        subagent_type.to_string()
    };
    let rec = SavingsRecord {
        ts: now_iso8601(),
        source: SavingsSource::ModelRoutingDowngrade,
        tokens_saved: tokens,
        model_target: Some(to_model.to_string()),
        project_path: ProjectPath::new(project_dir),
        spec_id: current_spec(project_dir).map(SpecId::new),
        wave_id: std::env::var("MUSTARD_ACTIVE_WAVE")
            .ok()
            .filter(|s| !s.is_empty())
            .map(WaveId::new),
        agent_id: Some(AgentId::new(agent_id)),
        extra: Map::new(),
    };
    let _ = writer::record_savings(&conn, rec);
}

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
    /// The pipeline `type` field, lowercased (`feature`, `bugfix`, …). The
    /// pipeline-state writers do not emit `type`, so this is usually `None` —
    /// it survives only to enrich the routing reason and metrics when present.
    type_lower: Option<String>,
    /// The raw `type` for metrics (`unknown` when absent).
    type_raw: String,
    /// The `scope` for metrics (`unknown` when absent).
    scope: String,
    /// The `status` field, lowercased (`draft`, `active`, `implementing`,
    /// `completed`, …). `None` when absent.
    status: Option<String>,
}

impl PipelineState {
    /// `true` when the pipeline-state is non-terminal — a pipeline is genuinely
    /// in progress. CLOSE deletes the state file and `session_cleanup` prunes
    /// terminal ones, so a present file is almost always active; an explicit
    /// terminal `status` is still treated as inactive as defence-in-depth.
    fn is_active(&self) -> bool {
        !matches!(
            self.status.as_deref(),
            Some("completed" | "cancelled" | "canceled" | "closed" | "done")
        )
    }
}

/// Load the newest `.json` pipeline-state under `<project>/.claude/.pipeline-states`
/// (excluding `*.metrics.json`). Port of `loadNewestPipelineState`. Fail-open:
/// any error → `None`.
fn load_newest_pipeline_state(project_dir: &str) -> Option<PipelineState> {
    let paths = ClaudePaths::for_project(Path::new(project_dir)).ok()?;
    let dir = paths.pipeline_states_dir();
    let entries = std::fs::read_dir(&dir).ok()?;
    let mut best: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
    for entry in entries.filter_map(std::result::Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !std::path::Path::new(&name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json")) || name.ends_with(".metrics.json") {
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
    let status = value
        .get("status")
        .and_then(|v| v.as_str())
        .map(str::to_ascii_lowercase);
    Some(PipelineState {
        type_lower,
        type_raw,
        scope,
        status,
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

/// Determine the expected model from the agent type and the active pipeline.
///
/// Routing is fully structural — it never parses the dispatch description.
/// `state` is `None` when there is no pipeline-state file.
fn determine_expected(subagent_type: &str, state: Option<&PipelineState>) -> Expected {
    let agent_type = subagent_type.to_ascii_lowercase();

    // Rule 1: Explore is read-only search → sonnet. Sonnet is the floor for
    // exploration (fewer missed matches on ambiguous naming conventions);
    // haiku stays available as an explicit downgrade since it ranks below
    // sonnet and downgrades are allowed.
    if agent_type == "explore" {
        return Expected {
            model: "sonnet",
            reason: "Explore agents use sonnet (read-only search, quality-first)",
        };
    }
    // Rule 2: Plan needs deep reasoning → opus.
    if agent_type == "plan" {
        return Expected {
            model: "opus",
            reason: "Plan agents use opus (architectural reasoning)",
        };
    }
    // Rule 3: an active pipeline drives the model. Feature and bugfix
    // pipelines both route to opus (quality-first), so the routing decision
    // does not need the pipeline `type` — the presence of a non-terminal
    // pipeline-state file is sufficient. This matters because the
    // pipeline-state writers do not emit a `type` field; matching on it (as
    // the original JS port did) always missed and silently downgraded
    // in-pipeline dispatches to sonnet.
    if let Some(state) = state {
        if state.is_active() {
            let reason = match state.type_lower.as_deref() {
                Some("bugfix") => {
                    "Bugfix pipeline active — opus (diagnosis needs deep reasoning)"
                }
                Some("feature") => "Feature pipeline active — opus (quality-first)",
                _ => "Active pipeline — opus (quality-first)",
            };
            return Expected {
                model: "opus",
                reason,
            };
        }
    }
    // Default: opus. User directive 2026-05-27 — every spec starts on the
    // best model available. Downgrades (sonnet/haiku) require an explicit
    // `model:` field on the Task dispatch, which the gate already permits as
    // a conscious choice. The gate must NEVER silently downgrade an
    // unspecified dispatch to sonnet — that broke Plan/general-purpose
    // dispatches during spec planning and is recorded as
    // [[feedback_spec_starts_opus_judgment_downgrade]] in user memory.
    Expected {
        model: "opus",
        reason: "Default model — quality first (spec sempre Opus; downgrade só por julgamento explícito)",
    }
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
// model_routing_gate is a single sequential flow; splitting would require threading
// many local variables without clarity gain.
#[allow(clippy::too_many_lines)]
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

    // ── No model specified ─────────────────────────────────────────────────
    if raw_model.is_empty() {
        let state = load_newest_pipeline_state(project_dir);
        let expected = determine_expected(subagent_type, state.as_ref());
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
                     (sonnet, or haiku to downgrade). Add model: \"sonnet\" to your \
                     Task dispatch. {}.",
                    expected.reason
                ),
            };
        }

        // Non-explorer, expected sonnet, strict mode → deny.
        // (Note 2026-05-27: with default flipped to opus, this branch now
        // only fires if Rule 1/2/3 above explicitly chose sonnet — currently
        // none do for non-Explore agents. Branch kept for forward compat
        // when future routing rules might select sonnet for a specific case.)
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
    let expected = determine_expected(subagent_type, state.as_ref());
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

    // ── Violation: a more expensive model was selected than required ──────
    // The dispatch will be denied (strict) or advised (warn) to use the
    // cheaper `expected.model` instead — that delta is the savings we record
    // for the W5 dashboard. Best-effort; never blocks the verdict.
    let prompt = tool_input
        .get("prompt")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    record_routing_downgrade(project_dir, model, expected.model, prompt, subagent_type);

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
        if !matches!(input.tool_name.as_deref(), Some("Task" | "Agent")) {
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
        let paths = ClaudePaths::for_project(project_dir).unwrap();
        let dir = paths.pipeline_states_dir();
        fs::create_dir_all(&dir).unwrap();
        fs::write(paths.pipeline_state_file("test"), state.to_string()).unwrap();
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
    fn explore_expects_sonnet() {
        let e = determine_expected("Explore", None);
        assert_eq!(e.model, "sonnet");
    }

    #[test]
    fn plan_expects_opus() {
        let e = determine_expected("Plan", None);
        assert_eq!(e.model, "opus");
    }

    #[test]
    fn typeless_active_pipeline_expects_opus() {
        // Real pipeline-state files carry no `type` field — only specName,
        // status, scope (phase lives in SQLite `pipeline.phase` events, not
        // the JSON). An active pipeline must still route to opus, otherwise
        // an EXECUTE-wave impl dispatch is downgraded.
        let state = PipelineState {
            type_lower: None,
            type_raw: "unknown".to_string(),
            scope: "full".to_string(),
            status: Some("implementing".to_string()),
        };
        let e = determine_expected("general-purpose", Some(&state));
        assert_eq!(e.model, "opus");
    }

    #[test]
    fn terminal_pipeline_state_falls_through_to_default_opus() {
        // A terminal status is inactive — routing falls through to the
        // default, which is opus per user directive 2026-05-27 (every spec
        // starts on the best model; downgrades require explicit signal).
        let state = PipelineState {
            type_lower: None,
            type_raw: "unknown".to_string(),
            scope: "full".to_string(),
            status: Some("completed".to_string()),
        };
        let e = determine_expected("general-purpose", Some(&state));
        assert_eq!(e.model, "opus");
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
            workspace_root: None,
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
            workspace_root: None,
        };
        assert_eq!(
            ModelRoutingGate.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }

    #[test]
    fn upgrade_is_blocked_explore_with_opus() {
        // Explore expects sonnet; dispatching with opus is an upgrade → deny.
        let dir = tempdir().unwrap();
        let input = task_input("Explore", Some("opus"), "search");
        match run(&input, dir.path().to_str().unwrap()) {
            Verdict::Deny { reason } => assert!(reason.contains("sonnet")),
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
        // Explore now expects sonnet — an exact match is allowed.
        let dir = tempdir().unwrap();
        let input = task_input("Explore", Some("sonnet"), "search");
        assert_eq!(run(&input, dir.path().to_str().unwrap()), Verdict::Allow);
    }

    #[test]
    fn explore_with_haiku_is_allowed_as_downgrade() {
        // Explore expects sonnet; haiku ranks below it, so an explicit haiku
        // dispatch is a downgrade — still allowed (opt-in cost saving).
        let dir = tempdir().unwrap();
        let input = task_input("Explore", Some("haiku"), "search");
        assert_eq!(run(&input, dir.path().to_str().unwrap()), Verdict::Allow);
    }

    #[test]
    fn typeless_pipeline_state_allows_opus_dispatch() {
        // Regression: a feature pipeline writes a pipeline-state file with no
        // `type` field. An EXECUTE-wave impl dispatch with model: "opus" must
        // be allowed, not blocked down to sonnet.
        let dir = tempdir().unwrap();
        write_state(
            dir.path(),
            json!({
                "specName": "2026-05-19-mustard-doctor",
                "status": "implementing",
                "scope": "full"
            }),
        );
        let input = task_input(
            "general-purpose",
            Some("opus"),
            "Implement mustard-doctor Wave 1",
        );
        assert_eq!(run(&input, dir.path().to_str().unwrap()), Verdict::Allow);
    }

    #[test]
    fn no_model_in_feature_pipeline_allows_silently() {
        // Feature pipeline → expected opus; inherited model presumed to match.
        let dir = tempdir().unwrap();
        write_state(
            dir.path(),
            json!({ "type": "feature", "scope": "full" }),
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
    fn no_model_general_purpose_defaults_to_opus_and_allows() {
        // No pipeline-state, no explicit model → expected opus (per user
        // directive 2026-05-27, every spec starts on the best model).
        // Inherited model is presumed to match → silent allow. The gate
        // MUST NOT downgrade to sonnet on unspecified dispatches; doing so
        // blocked Plan/general-purpose Opus dispatches during planning of
        // [[2026-05-26-no-sqlite-git-source-of-truth]].
        let dir = tempdir().unwrap();
        let input = task_input("general-purpose", None, "do work");
        assert_eq!(run(&input, dir.path().to_str().unwrap()), Verdict::Allow);
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
    fn warn_mode_no_model_general_purpose_still_allows_via_opus_default() {
        // Warn mode mirrors strict mode for the new opus-default policy:
        // unspecified dispatches are allowed silently because expected=opus
        // and the inherited model is presumed to match.
        let dir = tempdir().unwrap();
        let input = task_input("general-purpose", None, "do work");
        assert_eq!(
            run_mode(&input, dir.path().to_str().unwrap(), GateMode::Warn),
            Verdict::Allow
        );
    }

}
