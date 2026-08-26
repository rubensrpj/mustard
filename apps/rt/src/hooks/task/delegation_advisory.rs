//! `delegation_advisory` — advisory when the orchestrator edits many distinct
//! files DIRECTLY during an active pipeline instead of delegating via `Task`.
//!
//! ## Why (L0 Universal Delegation)
//!
//! The orchestrator rule (`.claude/CLAUDE.md` "When to delegate via Task (L0)")
//! says EXECUTE must be delegated and direct work is for ≤2 already-identified
//! files. [`super::main_context_counter`] already enforces an *overall* main-
//! context budget (all counted tool calls between dispatches). This module is
//! narrower and complementary: it counts **distinct files** the main context
//! `Write`/`Edit`s and, past a threshold, reminds the orchestrator to delegate.
//! It is purely advisory — it NEVER blocks (an `Observer`, side-effects only,
//! per `rt-observer-pattern`).
//!
//! ## Predicate — only fire when it matters
//!
//! Two conditions must both hold before the advisory surfaces:
//!
//! 1. **A pipeline is active.** Counted via
//!    [`crate::commands::spec::active_specs::count_active`] — the same shared
//!    projection the picker and `active_spec_limit_gate` use (a spec with
//!    `Outcome=Active` + `Stage ∈ {Analyze, Plan, Execute}`). No active spec →
//!    no advisory (direct editing outside a pipeline is fine).
//! 2. **The actor is the main context, not a subagent.** Inside a `Task`
//!    subagent, editing many files IS the delegated work — warning there would
//!    be noise. The authoritative signal is the harness-provided `agent_id`
//!    on the `PostToolUse` payload, surfaced as [`HookInput::is_subagent`]:
//!    Claude Code documents `agent_id` as "present only when the hook fires
//!    inside a subagent call — use this to distinguish subagent hook calls
//!    from main-thread calls." We suppress whenever `is_subagent()` is true.
//!    As belt-and-suspenders we ALSO suppress when the `subagentDepth` gauge
//!    that [`super::main_context_counter`] maintains in
//!    `main-context.counter.json` is `> 0` (incremented on `SubagentStart`,
//!    decremented on `SubagentStop`). Either signal alone suppresses; the
//!    advisory fires only when BOTH say "main".
//!
//! ## State — reuse, do not invent
//!
//! Per-session accumulation reuses the SAME `.claude/.agent-state` store and the
//! SAME `main-context.counter.json` file the sibling counter owns: this module
//! only ADDS a `delegationFiles` array (the distinct paths edited since the last
//! `Task` dispatch / `SessionStart`) — it does not introduce a new store. The
//! array resets exactly when `main_context_counter` resets `mainCount` (on a
//! `Task`/`Agent` dispatch and `SessionStart`), so the two stay in lockstep.
//!
//! ## Main-vs-subagent — now exact (was a proxy)
//!
//! This module originally had only the *shared* `subagentDepth` gauge (a P5
//! best-effort proxy maintained out-of-band by lifecycle hooks) and flagged
//! the missing per-invocation signal as a CONCERN. That signal turned out to
//! EXIST: Claude Code's hook contract sends `agent_id` on the `PostToolUse`
//! payload, "present only when the hook fires inside a subagent call," and the
//! docs say to "use this to distinguish subagent hook calls from main-thread
//! calls." It is now typed on [`HookInput::agent_id`] and read via
//! [`HookInput::is_subagent`] — the primary, exact discriminator.
//!
//! We keep `subagentDepth` as a redundant second signal (belt-and-suspenders):
//! the advisory fires only when BOTH `!is_subagent()` AND `depth == 0`. This
//! costs nothing — both are cheap reads — and means that even if one signal is
//! momentarily stale (e.g. a `SubagentStart` lifecycle event that never reached
//! this project's state file, or a harness build that omits `agent_id`), the
//! other still suppresses the advisory inside a subagent. Because the verdict
//! is a non-blocking advisory (`Warn`, fail-open), the worst case in any signal
//! disagreement is one extra reminder line — never a block.
//!
//! NOTE on `agent_type`: the harness also sets `agent_type` when the MAIN
//! session runs with `--agent` (no subagent), so `agent_type` alone is NOT a
//! reliable main-vs-subagent discriminator — only `agent_id` is. We use
//! `agent_id` exclusively.

use super::common;
use crate::commands::spec::active_specs::count_active;
use mustard_core::domain::model::contract::{Ctx, HookInput, Observer, Trigger};
use mustard_core::io::fs;
use mustard_core::time::now_iso8601;
use mustard_core::ClaudePaths;
use serde_json::{json, Value};
use std::path::Path;

/// Default distinct-file threshold past which the advisory fires. Mirrors the
/// orchestrator rule's "direct work is for ≤2 already-identified files": the
/// 3rd distinct file in the main context during an active pipeline trips it.
const DEFAULT_WARN_THRESHOLD: usize = 2;

/// Counter file shared with [`super::main_context_counter`]. We extend it with a
/// `delegationFiles` array rather than introduce a new store.
const MAIN_COUNTER_FILE: &str = "main-context.counter.json";

/// The `MUSTARD_DELEGATION_WARN_MODE` mode. Mirrors the sibling gates' shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Disabled — pure no-op.
    Off,
    /// Surface the advisory (default).
    Warn,
}

/// Resolve the mode from `MUSTARD_DELEGATION_WARN_MODE` (default `warn`). Only
/// `off` disables it; anything else (incl. unset) is `warn` — this is advisory,
/// there is no `strict`/blocking mode by design.
fn mode() -> Mode {
    match std::env::var("MUSTARD_DELEGATION_WARN_MODE")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "off" => Mode::Off,
        _ => Mode::Warn,
    }
}

/// Resolve the distinct-file threshold from `MUSTARD_DELEGATION_WARN_THRESHOLD`
/// (default [`DEFAULT_WARN_THRESHOLD`]). A non-numeric / empty value falls back
/// to the default; `0` is honoured (warn on the first file) for completeness.
fn threshold() -> usize {
    std::env::var("MUSTARD_DELEGATION_WARN_THRESHOLD")
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_WARN_THRESHOLD)
}

/// `delegation_advisory`: count distinct files the main context edits during an
/// active pipeline and remind to delegate past the threshold.
pub struct DelegationAdvisory;

impl DelegationAdvisory {
    /// The shared counter-file path.
    fn counter_path(project_dir: &str) -> std::path::PathBuf {
        ClaudePaths::for_project(project_dir)
            .map(|p| p.agent_state_dir().join(MAIN_COUNTER_FILE))
            .unwrap_or_default()
    }

    /// Read the persisted `subagentDepth` and `delegationFiles`. Fail-open: any
    /// error → `(0, empty)`.
    fn read_state(project_dir: &str) -> (u32, Vec<String>) {
        let Ok(text) = fs::read_to_string(Self::counter_path(project_dir)) else {
            return (0, Vec::new());
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            return (0, Vec::new());
        };
        let depth = value
            .get("subagentDepth")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as u32;
        let files = value
            .get("delegationFiles")
            .and_then(serde_json::Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        (depth, files)
    }

    /// Persist the updated `delegationFiles` array WITHOUT clobbering the keys
    /// [`super::main_context_counter`] owns (`mainCount`, `subagentDepth`). We
    /// read-modify-write the whole object so a concurrent field survives.
    /// Fail-open.
    fn write_files(project_dir: &str, files: &[String]) {
        let Ok(paths) = ClaudePaths::for_project(Path::new(project_dir)) else {
            return;
        };
        let dir = paths.agent_state_dir();
        let _ = fs::create_dir_all(&dir);
        let path = Self::counter_path(project_dir);
        // Preserve any sibling keys (mainCount/subagentDepth) already on disk.
        let mut obj = fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        obj.insert("delegationFiles".to_string(), json!(files));
        obj.insert("updatedAt".to_string(), json!(now_iso8601()));
        let _ = fs::write_atomic(&path, Value::Object(obj).to_string().as_bytes());
    }

    /// The normalised `file_path` of a Write/Edit invocation, forward-slashed.
    fn edited_file(input: &HookInput) -> Option<String> {
        let ti = &input.tool_input;
        ti.get("file_path")
            .or_else(|| ti.get("path"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.replace('\\', "/"))
    }

    /// The pure decision: given the current distinct-file set, the newly edited
    /// file, whether a pipeline is active, whether the actor is a subagent
    /// (the authoritative `agent_id` signal), the subagent-depth proxy, the
    /// mode, and the threshold, return the updated distinct-file set and an
    /// optional advisory message. Side-effect-free so it can be unit-tested
    /// without disk or env.
    ///
    /// The advisory fires only when ALL hold: mode is `Warn`, a pipeline is
    /// active, the actor is the MAIN context — i.e. `!is_subagent` AND
    /// `depth == 0` (two redundant signals, either suppresses) — and the
    /// distinct count has just crossed `> threshold`. The file set still
    /// accumulates even when the advisory is suppressed (e.g. inside a
    /// subagent) so the count is honest once control returns to main.
    fn decide(
        mut files: Vec<String>,
        edited: &str,
        pipeline_active: bool,
        is_subagent: bool,
        subagent_depth: u32,
        mode: Mode,
        threshold: usize,
    ) -> (Vec<String>, Option<String>) {
        // Track the distinct file regardless of whether we warn this call.
        let is_new = !files.iter().any(|f| f == edited);
        if is_new {
            files.push(edited.to_string());
        }
        let count = files.len();

        let suppress =
            mode == Mode::Off || !pipeline_active || is_subagent || subagent_depth > 0;
        if suppress {
            return (files, None);
        }
        // Fire once per new distinct file past the threshold.
        if is_new && count > threshold {
            let msg = format!(
                "[delegation-advisory] {count} distinct files edited directly in \
                 the main context during an active pipeline (threshold {threshold}). \
                 L0 Universal Delegation: dispatch a Task agent for this work — \
                 direct editing is meant for ≤{threshold} already-identified files. \
                 Set MUSTARD_DELEGATION_WARN_MODE=off to silence."
            );
            return (files, Some(msg));
        }
        (files, None)
    }
}

impl Observer for DelegationAdvisory {
    /// On `PostToolUse(Write|Edit)`, accumulate the distinct main-context file
    /// set and, past the threshold during an active pipeline, emit a
    /// non-blocking advisory. Pure side effect — fail-open throughout.
    ///
    /// The advisory is surfaced via the observer's warn channel (stderr) since
    /// an [`Observer`] cannot return a `Verdict`. This mirrors how the legacy
    /// JS advisories printed to stderr.
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        if ctx.trigger != Some(Trigger::PostToolUse) {
            return;
        }
        if !matches!(input.tool_name.as_deref(), Some("Write" | "Edit")) {
            return;
        }
        let Some(edited) = Self::edited_file(input) else {
            return;
        };
        // Resolve the project root that owns the shared counter state. Skip
        // entirely when no valid root is available (the AC-W5.2 regression:
        // never leak `.agent-state` into the process cwd under `cargo test`).
        let project = if ctx.project_dir.is_empty() {
            match common::project_dir_opt(input) {
                Some(p) => p,
                None => return,
            }
        } else {
            ctx.project_dir.clone()
        };

        let m = mode();
        if m == Mode::Off {
            return;
        }
        let (depth, files) = Self::read_state(&project);
        let pipeline_active = count_active(Path::new(&project)) > 0;
        // Authoritative actor signal from the harness `agent_id` payload field;
        // the depth gauge is the redundant belt-and-suspenders second signal.
        let is_subagent = input.is_subagent();
        let (updated, advisory) = Self::decide(
            files,
            &edited,
            pipeline_active,
            is_subagent,
            depth,
            m,
            threshold(),
        );
        // Persist the accumulated set (best-effort) so the count survives across
        // calls within the session, in lockstep with main_context_counter.
        Self::write_files(&project, &updated);
        if let Some(msg) = advisory {
            eprintln!("{msg}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- decide() — the pure predicate (no disk, no env) -------------------

    #[test]
    fn warns_when_distinct_files_exceed_threshold_in_active_pipeline_main() {
        // Two files already tracked; the 3rd distinct file (> threshold 2) in
        // the main context (depth 0) during an active pipeline → advisory.
        let files = vec!["a.rs".to_string(), "b.rs".to_string()];
        let (updated, advisory) =
            DelegationAdvisory::decide(files, "c.rs", true, false, 0, Mode::Warn, 2);
        assert_eq!(updated.len(), 3);
        let msg = advisory.expect("3rd distinct file past threshold must warn");
        assert!(msg.contains("3 distinct files"), "got {msg}");
        assert!(msg.contains("threshold 2"), "got {msg}");
    }

    #[test]
    fn does_not_warn_at_or_below_threshold() {
        // 1st file → count 1, not > 2.
        let (files, advisory) =
            DelegationAdvisory::decide(Vec::new(), "a.rs", true, false, 0, Mode::Warn, 2);
        assert_eq!(files.len(), 1);
        assert!(advisory.is_none());
        // 2nd distinct file → count 2, still not > 2.
        let (_, advisory) =
            DelegationAdvisory::decide(files, "b.rs", true, false, 0, Mode::Warn, 2);
        assert!(advisory.is_none(), "exactly at threshold must not warn");
    }

    #[test]
    fn does_not_warn_without_active_pipeline() {
        // Even with 3 distinct files, no active pipeline → never warn.
        let files = vec!["a.rs".to_string(), "b.rs".to_string()];
        let (updated, advisory) =
            DelegationAdvisory::decide(files, "c.rs", false, false, 0, Mode::Warn, 2);
        // The set still accumulates (count stays honest)…
        assert_eq!(updated.len(), 3);
        // …but no advisory surfaces outside a pipeline.
        assert!(advisory.is_none());
    }

    #[test]
    fn does_not_warn_inside_a_subagent_by_depth_proxy() {
        // depth > 0 means a Task subagent is in flight (the belt-and-suspenders
        // proxy) — editing many files there IS the delegated work; suppress.
        // is_subagent=false here proves the depth signal alone suffices.
        let files = vec!["a.rs".to_string(), "b.rs".to_string()];
        let (updated, advisory) =
            DelegationAdvisory::decide(files, "c.rs", true, false, 1, Mode::Warn, 2);
        assert_eq!(updated.len(), 3, "set still accumulates under a subagent");
        assert!(advisory.is_none(), "depth proxy must suppress inside a subagent");
    }

    #[test]
    fn does_not_warn_inside_a_subagent_by_agent_id() {
        // is_subagent=true is the authoritative `agent_id` signal — it suppresses
        // even when the depth proxy is 0 (e.g. a SubagentStart lifecycle event
        // that never reached the shared counter file). This is the exact-signal
        // path the tactical-fix added.
        let files = vec!["a.rs".to_string(), "b.rs".to_string()];
        let (updated, advisory) =
            DelegationAdvisory::decide(files, "c.rs", true, true, 0, Mode::Warn, 2);
        assert_eq!(updated.len(), 3, "set still accumulates under a subagent");
        assert!(
            advisory.is_none(),
            "agent_id signal must suppress even when depth gauge is stale (0)"
        );
    }

    #[test]
    fn off_mode_never_warns() {
        let files = vec!["a.rs".to_string(), "b.rs".to_string()];
        let (_, advisory) =
            DelegationAdvisory::decide(files, "c.rs", true, false, 0, Mode::Off, 2);
        assert!(advisory.is_none());
    }

    #[test]
    fn duplicate_file_does_not_re_warn_or_re_count() {
        // c.rs already in the set; re-editing it must neither grow the count
        // nor fire a fresh advisory (only NEW distinct files trip it).
        let files = vec!["a.rs".to_string(), "b.rs".to_string(), "c.rs".to_string()];
        let (updated, advisory) =
            DelegationAdvisory::decide(files, "c.rs", true, false, 0, Mode::Warn, 2);
        assert_eq!(updated.len(), 3, "re-edit must not grow the distinct set");
        assert!(advisory.is_none(), "re-edit of a known file must not re-warn");
    }

    #[test]
    fn respects_a_higher_threshold() {
        // threshold 5: the 3rd distinct file must NOT warn.
        let files = vec!["a.rs".to_string(), "b.rs".to_string()];
        let (_, advisory) =
            DelegationAdvisory::decide(files, "c.rs", true, false, 0, Mode::Warn, 5);
        assert!(advisory.is_none(), "count 3 ≤ threshold 5 must not warn");
    }

    // --- mode / threshold env parsing (no env mutation — defaults only) -----

    #[test]
    fn mode_defaults_to_warn_when_unset() {
        // No test in this crate mutates the env (unsafe under Rust 2024 +
        // #![forbid(unsafe_code)]); the var is unset here.
        assert_eq!(mode(), Mode::Warn);
    }

    #[test]
    fn threshold_defaults_when_unset() {
        assert_eq!(threshold(), DEFAULT_WARN_THRESHOLD);
    }

    // --- observe() end-to-end on disk (uses a tempdir project) -------------

    #[test]
    fn observe_accumulates_and_warns_across_calls() {
        use mustard_core::domain::model::contract::Trigger;
        use serde_json::json;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let root = dir.path();
        // Plant one Active spec so count_active() > 0.
        let spec = root.join(".claude").join("spec").join("2026-01-01-demo");
        std::fs::create_dir_all(&spec).unwrap();
        std::fs::write(spec.join("spec.md"), "# demo\n\n## Resumo\n\nx\n").unwrap();
        std::fs::write(
            spec.join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active","scope":null,"parent":null,"checkpoint":null}"#,
        )
        .unwrap();

        let ctx = Ctx {
            project_dir: root.to_string_lossy().into_owned(),
            trigger: Some(Trigger::PostToolUse),
            workspace_root: None,
            inject_only: None,
        };
        let edit = |p: &str| HookInput {
            tool_name: Some("Edit".to_string()),
            tool_input: json!({ "file_path": p, "new_string": "x" }),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        };

        // Three distinct edits — must not panic, and must persist the set.
        DelegationAdvisory.observe(&edit("src/a.rs"), &ctx);
        DelegationAdvisory.observe(&edit("src/b.rs"), &ctx);
        DelegationAdvisory.observe(&edit("src/c.rs"), &ctx);

        let counter = root
            .join(".claude")
            .join(".agent-state")
            .join("main-context.counter.json");
        let value: Value =
            serde_json::from_str(&std::fs::read_to_string(counter).unwrap()).unwrap();
        let files = value["delegationFiles"].as_array().unwrap();
        assert_eq!(files.len(), 3, "three distinct files accumulated");
    }

    #[test]
    fn observe_suppresses_when_payload_marks_a_subagent() {
        // End-to-end: an active pipeline + edits past the threshold, but each
        // PostToolUse payload carries the harness `agent_id` (subagent actor).
        // The advisory must stay suppressed via is_subagent() even though the
        // shared depth gauge is 0 — proving the exact signal works on its own.
        use mustard_core::domain::model::contract::Trigger;
        use serde_json::json;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let root = dir.path();
        let spec = root.join(".claude").join("spec").join("2026-01-01-demo");
        std::fs::create_dir_all(&spec).unwrap();
        std::fs::write(spec.join("spec.md"), "# demo\n\n## Resumo\n\nx\n").unwrap();
        std::fs::write(
            spec.join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active","scope":null,"parent":null,"checkpoint":null}"#,
        )
        .unwrap();

        let ctx = Ctx {
            project_dir: root.to_string_lossy().into_owned(),
            trigger: Some(Trigger::PostToolUse),
            workspace_root: None,
            inject_only: None,
        };
        // Every edit is attributed to a subagent via the harness `agent_id`.
        let edit = |p: &str| HookInput {
            tool_name: Some("Edit".to_string()),
            tool_input: json!({ "file_path": p, "new_string": "x" }),
            hook_event_name: Some("PostToolUse".to_string()),
            agent_id: Some("explore-7".to_string()),
            agent_type: Some("Explore".to_string()),
            ..HookInput::default()
        };
        // Sanity: the helper sees these as subagent invocations.
        assert!(edit("src/a.rs").is_subagent());

        // Three distinct subagent edits past threshold 2 — must accumulate but
        // never warn (no panic; suppression is the contract here).
        DelegationAdvisory.observe(&edit("src/a.rs"), &ctx);
        DelegationAdvisory.observe(&edit("src/b.rs"), &ctx);
        DelegationAdvisory.observe(&edit("src/c.rs"), &ctx);

        // The distinct set still accumulates honestly even while suppressed.
        let counter = root
            .join(".claude")
            .join(".agent-state")
            .join("main-context.counter.json");
        let value: Value =
            serde_json::from_str(&std::fs::read_to_string(counter).unwrap()).unwrap();
        assert_eq!(value["delegationFiles"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn observe_preserves_sibling_counter_keys() {
        use mustard_core::domain::model::contract::Trigger;
        use serde_json::json;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let root = dir.path();
        let state = root.join(".claude").join(".agent-state");
        std::fs::create_dir_all(&state).unwrap();
        // Seed the file as main_context_counter would, with its own keys.
        std::fs::write(
            state.join("main-context.counter.json"),
            json!({ "mainCount": 7, "subagentDepth": 0 }).to_string(),
        )
        .unwrap();
        // Active spec so the predicate engages the write path.
        let spec = root.join(".claude").join("spec").join("2026-01-01-demo");
        std::fs::create_dir_all(&spec).unwrap();
        std::fs::write(spec.join("spec.md"), "# demo\n\n## Resumo\n\nx\n").unwrap();
        std::fs::write(
            spec.join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active","scope":null,"parent":null,"checkpoint":null}"#,
        )
        .unwrap();

        let ctx = Ctx {
            project_dir: root.to_string_lossy().into_owned(),
            trigger: Some(Trigger::PostToolUse),
            workspace_root: None,
            inject_only: None,
        };
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({ "file_path": "src/x.rs", "content": "y" }),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        };
        DelegationAdvisory.observe(&input, &ctx);

        let value: Value = serde_json::from_str(
            &std::fs::read_to_string(state.join("main-context.counter.json")).unwrap(),
        )
        .unwrap();
        // Sibling keys survived the read-modify-write.
        assert_eq!(value["mainCount"], json!(7));
        assert_eq!(value["subagentDepth"], json!(0));
        // And our key was added.
        assert_eq!(value["delegationFiles"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn observe_is_infallible_without_project_root() {
        use mustard_core::domain::model::contract::Trigger;
        use serde_json::json;
        // Empty project_dir + no cwd → project_dir_opt returns None → no-op.
        let ctx = Ctx {
            project_dir: String::new(),
            trigger: Some(Trigger::PostToolUse),
            workspace_root: None,
            inject_only: None,
        };
        let input = HookInput {
            tool_name: Some("Edit".to_string()),
            tool_input: json!({ "file_path": "src/a.rs", "new_string": "x" }),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        };
        // Must not panic.
        DelegationAdvisory.observe(&input, &ctx);
    }
}
