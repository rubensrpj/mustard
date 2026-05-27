//! The module registry — which enforcement modules run for which event/tool.
//!
//! Open/Closed in practice (b3 spec § Arquitetura "SOLID"): adding a check is
//! *only* registering a [`Module`] here. The dispatcher reads the registry and
//! never changes. A module is keyed by the `(Trigger, tool)` pairs it applies
//! to, so an unrelated invocation skips it entirely instead of running it just
//! to have it self-`Allow`.

use crate::hooks::amend_capture::AmendCapture;
use crate::hooks::auto_capture_summary::AutoCaptureSummary;
use crate::hooks::bash_guard::BashGuard;
use crate::hooks::budget::BudgetGuard;
use crate::hooks::close_gate::CloseGate;
use crate::hooks::enforce_registry::EnforceRegistry;
use crate::hooks::knowledge::Knowledge;
use crate::hooks::model_routing::ModelRoutingGate;
use crate::hooks::notification::Notification;
use crate::hooks::path_guard::PathGuard;
use crate::hooks::post_edit::PostEdit;
use crate::hooks::pre_compact::PreCompact;
use crate::hooks::prompt_gate::PromptGate;
use crate::hooks::session_cleanup::SessionCleanup;
use crate::hooks::session_start::SessionStart;
use crate::hooks::size_gate::SizeGate;
use crate::hooks::skills_audit::SkillsAudit;
use crate::hooks::spec_hygiene::SpecHygiene;
use crate::hooks::stop::Stop;
use crate::hooks::stop_observer::{PreCompactMemorySnippet, SessionEndConsolidate, StopObserver};
use crate::hooks::subagent_inject::SubagentInject;
use crate::hooks::tool_result::ToolResult;
use crate::hooks::tracker::{
    MainContextCounter, MetricsTracker, SkillUsageTracker, SubagentTracker, ToolUseCounter,
};
use crate::hooks::wikilink_footer::WikilinkFooter;
use mustard_core::config::Mode;
use mustard_core::model::contract::{Check, Observer, Trigger};

/// Which tool an `(event, tool)` registration entry applies to.
///
/// The JS `settings.json` matchers are one of: a literal tool name (`"Bash"`,
/// `"Task"`), an alternation (`"Task|Agent"` — expressed as two entries here),
/// the wildcard `".*"` (every tool), or absent (a non-tool lifecycle event
/// like `SubagentStart`). [`ToolMatch`] models exactly those three cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolMatch {
    /// A non-tool lifecycle event — the harness invocation has no `tool_name`.
    ///
    /// No Wave-3 module needs this exact case (lifecycle modules use
    /// [`ToolMatch::Any`], which already matches a `None` tool); it is kept as
    /// API surface for a later wave that registers a `None`-tool-only module.
    #[allow(dead_code)]
    None,
    /// Every tool (the `".*"` matcher), and also non-tool events: the JS `.*`
    /// `PreToolUse` matcher fires for any invocation.
    Any,
    /// One specific tool name.
    Named(&'static str),
}

impl ToolMatch {
    /// `true` if this matcher applies to an invocation carrying `tool`.
    #[must_use]
    fn matches(self, tool: Option<&str>) -> bool {
        match self {
            Self::None => tool.is_none(),
            Self::Any => true,
            Self::Named(name) => tool == Some(name),
        }
    }
}

/// One enforcement concern. A module is a `Check`, an `Observer`, or both.
/// `bash_guard`, for example, is both — the four ported PreToolUse(Bash) gates
/// (`Check`) and the `pr-detect` PostToolUse(Bash) telemetry (`Observer`).
pub struct Module {
    /// Stable id used by `mustard-rt check <id>` and by the enforcement
    /// config (`MUSTARD_<ID>_MODE`). Lowercase, snake or kebab.
    pub id: &'static str,
    /// The `(Trigger, ToolMatch)` pairs this module applies to.
    pub applies_to: &'static [(Trigger, ToolMatch)],
    /// The gate behaviour, if this module decides anything. `None` for a
    /// pure-`Observer` module.
    pub check: Option<Box<dyn Check>>,
    /// The telemetry behaviour, if this module observes. `None` for a
    /// pure-`Check` module.
    pub observer: Option<Box<dyn Observer>>,
}

impl Module {
    /// `true` if this module is applicable to the given event/tool.
    #[must_use]
    pub fn matches(&self, trigger: Trigger, tool: Option<&str>) -> bool {
        self.applies_to
            .iter()
            .any(|(t, want_tool)| *t == trigger && want_tool.matches(tool))
    }
}

/// The set of registered enforcement modules.
pub struct Registry {
    modules: Vec<Module>,
}

impl Registry {
    /// Build the registry with every module Mustard ships.
    ///
    /// Early b3 waves register only `bash_guard`; later waves push their
    /// families (`budget`, `size_gate`, …) here, leaving the dispatcher
    /// untouched.
    #[must_use]
    // Registry::new() is a flat list of module registrations — refactoring into
    // helper functions would obscure the registry structure without reducing complexity.
    #[allow(clippy::too_many_lines)]
    pub fn new() -> Self {
        let modules = vec![
            Module {
                id: "bash_guard",
                // `bash_guard` is both a `Check` and an `Observer` — it ports
                // the full Bash family (5/5): `bash-safety`,
                // `bash-native-redirect`, `rtk-rewrite` and `review-gate` as
                // PreToolUse(Bash) gates, plus `pr-detect` as PostToolUse(Bash)
                // telemetry.
                applies_to: &[
                    (Trigger::PreToolUse, ToolMatch::Named("Bash")),
                    (Trigger::PostToolUse, ToolMatch::Named("Bash")),
                ],
                check: Some(Box::new(BashGuard)),
                observer: Some(Box::new(BashGuard)),
            },
            // ── Wave 3: Task / Subagent family ───────────────────────────────
            Module {
                id: "budget",
                // `context-budget` (PreToolUse(Task) prompt-size gate) +
                // `output-budget` (PostToolUse(Task) return-size advisory).
                // Both flow through the `Check` — the over-budget advisory is
                // an `Inject` verdict, not a raw stdout write (Wave-5 fix of
                // the Wave-3 `budget::observe` stdout-bypass Concern).
                applies_to: &[
                    (Trigger::PreToolUse, ToolMatch::Named("Task")),
                    (Trigger::PreToolUse, ToolMatch::Named("Agent")),
                    (Trigger::PostToolUse, ToolMatch::Named("Task")),
                    (Trigger::PostToolUse, ToolMatch::Named("Agent")),
                ],
                check: Some(Box::new(BudgetGuard)),
                observer: None,
            },
            Module {
                id: "model_routing",
                // `model-routing-gate` — PreToolUse(Task) model-selection gate.
                applies_to: &[
                    (Trigger::PreToolUse, ToolMatch::Named("Task")),
                    (Trigger::PreToolUse, ToolMatch::Named("Agent")),
                ],
                check: Some(Box::new(ModelRoutingGate)),
                observer: None,
            },
            Module {
                id: "tool_use_counter",
                // `tool-use-counter` — caps tool uses per Explore subagent.
                // The JS matcher is `.*` on PreToolUse (every tool counts),
                // plus the Subagent lifecycle and SessionStart.
                applies_to: &[
                    (Trigger::PreToolUse, ToolMatch::Any),
                    (Trigger::SubagentStart, ToolMatch::Any),
                    (Trigger::SubagentStop, ToolMatch::Any),
                    (Trigger::SessionStart, ToolMatch::Any),
                ],
                check: Some(Box::new(ToolUseCounter)),
                observer: None,
            },
            Module {
                id: "main_context_counter",
                // `main-context-counter` — enforces L0 on the orchestrator.
                applies_to: &[
                    (Trigger::PreToolUse, ToolMatch::Any),
                    (Trigger::SubagentStart, ToolMatch::Any),
                    (Trigger::SubagentStop, ToolMatch::Any),
                    (Trigger::SessionStart, ToolMatch::Any),
                ],
                check: Some(Box::new(MainContextCounter)),
                observer: None,
            },
            Module {
                id: "subagent_tracker",
                // `subagent-tracker` — `agent.start` / `agent.stop` telemetry.
                applies_to: &[
                    (Trigger::PreToolUse, ToolMatch::Named("Task")),
                    (Trigger::PreToolUse, ToolMatch::Named("Agent")),
                    (Trigger::PostToolUse, ToolMatch::Named("Task")),
                    (Trigger::PostToolUse, ToolMatch::Named("Agent")),
                ],
                check: None,
                observer: Some(Box::new(SubagentTracker)),
            },
            Module {
                id: "metrics_tracker",
                // `metrics-tracker` — `tool.use` heartbeat after a tool runs.
                applies_to: &[
                    (Trigger::PostToolUse, ToolMatch::Named("Bash")),
                    (Trigger::PostToolUse, ToolMatch::Named("Write")),
                    (Trigger::PostToolUse, ToolMatch::Named("Edit")),
                    (Trigger::PostToolUse, ToolMatch::Named("Task")),
                    (Trigger::PostToolUse, ToolMatch::Named("Agent")),
                    (Trigger::PostToolUse, ToolMatch::Named("Read")),
                ],
                check: None,
                observer: Some(Box::new(MetricsTracker)),
            },
            Module {
                id: "skill_usage_tracker",
                // `skill-usage-tracker` — `skill.invoked` event per Skill call.
                applies_to: &[(Trigger::PostToolUse, ToolMatch::Named("Skill"))],
                check: None,
                observer: Some(Box::new(SkillUsageTracker)),
            },
            Module {
                id: "tool_result",
                // `tool-result` — PostToolUse capture of rich tool output
                // (Bash stdout/stderr/exit, Edit/MultiEdit before/after, Write
                // content, Read content excerpt). Emits a `tool.result` event
                // the dashboard `<ExecutionTrace>` joins with the matching
                // `tool.use` (followup-2 § 4b/4c).
                applies_to: &[
                    (Trigger::PostToolUse, ToolMatch::Named("Bash")),
                    (Trigger::PostToolUse, ToolMatch::Named("Edit")),
                    (Trigger::PostToolUse, ToolMatch::Named("MultiEdit")),
                    (Trigger::PostToolUse, ToolMatch::Named("Write")),
                    (Trigger::PostToolUse, ToolMatch::Named("Read")),
                ],
                check: None,
                observer: Some(Box::new(ToolResult)),
            },
            Module {
                id: "skills_audit",
                // `recommended-skills-audit` — advisory count on PreToolUse(Task).
                applies_to: &[
                    (Trigger::PreToolUse, ToolMatch::Named("Task")),
                    (Trigger::PreToolUse, ToolMatch::Named("Agent")),
                ],
                check: Some(Box::new(SkillsAudit)),
                observer: None,
            },
            // ── Wave 4: Write/Edit family ────────────────────────────────────
            Module {
                id: "size_gate",
                // `spec-size-gate` + `skill-size-gate` + `skill-validate-gate` —
                // PreToolUse(Write|Edit) structural gates.
                applies_to: &[
                    (Trigger::PreToolUse, ToolMatch::Named("Write")),
                    (Trigger::PreToolUse, ToolMatch::Named("Edit")),
                ],
                check: Some(Box::new(SizeGate)),
                observer: None,
            },
            Module {
                id: "path_guard",
                // `file-guard` (PreToolUse(Read|Write|Edit) sensitive-file
                // gate) + `boundary-gate` (PreToolUse(Write|Edit) spec-boundary
                // gate). Registered on Read too so `file-guard` covers reads.
                applies_to: &[
                    (Trigger::PreToolUse, ToolMatch::Named("Read")),
                    (Trigger::PreToolUse, ToolMatch::Named("Write")),
                    (Trigger::PreToolUse, ToolMatch::Named("Edit")),
                ],
                check: Some(Box::new(PathGuard)),
                observer: None,
            },
            Module {
                id: "close_gate",
                // `close-gate` — PreToolUse(Write|Edit) pipeline-CLOSE sensor.
                applies_to: &[
                    (Trigger::PreToolUse, ToolMatch::Named("Write")),
                    (Trigger::PreToolUse, ToolMatch::Named("Edit")),
                ],
                check: Some(Box::new(CloseGate)),
                observer: None,
            },
            Module {
                id: "enforce_registry",
                // `enforce-registry` — PreToolUse(Skill) pre-pipeline gate.
                applies_to: &[(Trigger::PreToolUse, ToolMatch::Named("Skill"))],
                check: Some(Box::new(EnforceRegistry)),
                observer: None,
            },
            Module {
                id: "post_edit",
                // `auto-format` + `checklist-auto-mark` + `guard-verify` +
                // `pipeline-phase` — PostToolUse(Write|Edit). Both a `Check`
                // (guard-verify) and an `Observer` (the other three).
                applies_to: &[
                    (Trigger::PostToolUse, ToolMatch::Named("Write")),
                    (Trigger::PostToolUse, ToolMatch::Named("Edit")),
                ],
                check: Some(Box::new(PostEdit)),
                observer: Some(Box::new(PostEdit)),
            },
            // ── Wave 5: session-lifecycle families ───────────────────────────
            // `spec_hygiene` is registered *before* `session_start` so its
            // gated auto-close (and the spec-header rewrite it performs) runs
            // ahead of the SessionStart memory injection — the order the
            // dispatcher iterates modules in (spec-lifecycle-unification W5).
            Module {
                id: "spec_hygiene",
                // SessionStart-only side effect — emits `hygiene.*` events and,
                // for a green close-gate, auto-closes a candidate spec.
                applies_to: &[(Trigger::SessionStart, ToolMatch::Any)],
                check: Some(Box::new(SpecHygiene)),
                observer: None,
            },
            Module {
                id: "session_start",
                // `harness-init` + `session-memory` + `spec-hygiene` — the
                // SessionStart bootstrap. A `Check` (the memory-injection
                // payload is its `Inject` verdict).
                applies_to: &[(Trigger::SessionStart, ToolMatch::Any)],
                check: Some(Box::new(SessionStart)),
                observer: None,
            },
            Module {
                id: "knowledge",
                // `session-knowledge` + `memory-auto-extract` on SessionEnd,
                // `session-knowledge-inc` on PostToolUse(Task). Pure telemetry
                // — an `Observer`.
                applies_to: &[
                    (Trigger::SessionEnd, ToolMatch::Any),
                    (Trigger::PostToolUse, ToolMatch::Named("Task")),
                    (Trigger::PostToolUse, ToolMatch::Named("Agent")),
                ],
                check: None,
                observer: Some(Box::new(Knowledge)),
            },
            Module {
                id: "session_cleanup",
                // `session-cleanup` — SessionEnd stale-state cleanup. An
                // `Observer` (pure side effect, no verdict).
                applies_to: &[(Trigger::SessionEnd, ToolMatch::Any)],
                check: None,
                observer: Some(Box::new(SessionCleanup)),
            },
            Module {
                id: "pre_compact",
                // `pre-compact` — the PreCompact snapshot. A `Check` (the
                // snapshot is its `Inject` verdict).
                applies_to: &[(Trigger::PreCompact, ToolMatch::Any)],
                check: Some(Box::new(PreCompact)),
                observer: None,
            },
            Module {
                id: "prompt_gate",
                // `followup-cancel-gate` — UserPromptSubmit follow-up archival.
                // A `Check` (always allows; the archival is its side effect).
                applies_to: &[(Trigger::UserPromptSubmit, ToolMatch::Any)],
                check: Some(Box::new(PromptGate)),
                observer: None,
            },
            // ── W8 deep-refactor: context-injection optimisation ─────────────
            Module {
                id: "subagent_inject",
                // T8.3 — for Task dispatches without a declared SKILL, inject a
                // minimal CONTEXT.md + skills slice (resolved via W1's
                // `skill-resolve`).
                applies_to: &[
                    (Trigger::PreToolUse, ToolMatch::Named("Task")),
                    (Trigger::PreToolUse, ToolMatch::Named("Agent")),
                ],
                check: Some(Box::new(SubagentInject)),
                observer: None,
            },
            Module {
                id: "auto_capture_summary",
                // T8.4 — on Task return, parse `<MEMORY>` or `Resumo:` and
                // persist to `agent_memory`.
                applies_to: &[
                    (Trigger::PostToolUse, ToolMatch::Named("Task")),
                    (Trigger::PostToolUse, ToolMatch::Named("Agent")),
                ],
                check: None,
                observer: Some(Box::new(AutoCaptureSummary)),
            },
            Module {
                id: "stop_observer",
                // T8.5 — SubagentStop reinforcement: bump `last_used` on any
                // agent_memory row whose summary appeared in the output.
                applies_to: &[(Trigger::SubagentStop, ToolMatch::Any)],
                check: None,
                observer: Some(Box::new(StopObserver)),
            },
            Module {
                id: "session_end_consolidate",
                // T8.6 — SessionEnd promotion of high-confidence agent_memory
                // rows to permanent memory_decisions / memory_lessons rows.
                applies_to: &[(Trigger::SessionEnd, ToolMatch::Any)],
                check: None,
                observer: Some(Box::new(SessionEndConsolidate)),
            },
            Module {
                id: "pre_compact_memory_snippet",
                // T8.7 — add up to 3 recent agent_memory entries to the
                // PreCompact snapshot (in addition to the pre_compact module).
                applies_to: &[(Trigger::PreCompact, ToolMatch::Any)],
                check: Some(Box::new(PreCompactMemorySnippet)),
                observer: None,
            },
            // ── W9 deep-refactor: Stop + Notification triggers ───────────────
            Module {
                id: "stop",
                // `Stop` lifecycle observer — persists an `interrupted at wave N`
                // agent_memory row when there has been a recent edit, with a
                // 5-minute anti-spam guard between consecutive Stops.
                applies_to: &[(Trigger::Stop, ToolMatch::Any)],
                check: None,
                observer: Some(Box::new(Stop)),
            },
            Module {
                id: "notification",
                // `Notification` lifecycle observer — appends a single
                // `notification.received` event to the per-spec NDJSON log;
                // observe-only, no auto-resolution.
                applies_to: &[(Trigger::Notification, ToolMatch::Any)],
                check: None,
                observer: Some(Box::new(Notification)),
            },
            // ── W3E (no-sqlite git source of truth) — wikilink footer ────────
            Module {
                id: "wikilink_footer",
                // PostToolUse(Write|Edit) auto-footer renderer for
                // `.claude/{memory,knowledge,spec}/**/*.md`. Pure Observer —
                // the render logic lives in `mustard_core::atomic_md::wikilink`.
                applies_to: &[
                    (Trigger::PostToolUse, ToolMatch::Named("Write")),
                    (Trigger::PostToolUse, ToolMatch::Named("Edit")),
                ],
                check: None,
                observer: Some(Box::new(WikilinkFooter)),
            },
            // ── Wave 6: session-bound amendment window ───────────────────────
            Module {
                id: "amend_capture",
                // Tracks in-session edits after pipeline close.
                // Observer: PostToolUse(Bash|Write|Edit) + UserPromptSubmit.
                // Check: PreToolUse(Write|Edit) for look-ahead drift injection.
                applies_to: &[
                    (Trigger::PreToolUse, ToolMatch::Named("Write")),
                    (Trigger::PreToolUse, ToolMatch::Named("Edit")),
                    (Trigger::PostToolUse, ToolMatch::Named("Bash")),
                    (Trigger::PostToolUse, ToolMatch::Named("Write")),
                    (Trigger::PostToolUse, ToolMatch::Named("Edit")),
                    (Trigger::UserPromptSubmit, ToolMatch::Any),
                ],
                check: Some(Box::new(AmendCapture)),
                observer: Some(Box::new(AmendCapture)),
            },
        ];
        Self { modules }
    }

    /// Every module applicable to the given event/tool, in registration order.
    #[must_use]
    pub fn applicable(&self, trigger: Trigger, tool: Option<&str>) -> Vec<&Module> {
        self.modules
            .iter()
            .filter(|m| m.matches(trigger, tool))
            .collect()
    }

    /// The module with the given id, regardless of event/tool — used by
    /// `mustard-rt check <id>`.
    #[must_use]
    pub fn by_id(&self, id: &str) -> Option<&Module> {
        self.modules.iter().find(|m| m.id == id)
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

/// Module ids that are silenced during the v4 bootstrap refoundation.
///
/// When `MUSTARD_V4_BOOTSTRAP` is set to a non-empty string, these 12 v3
/// enforcement hooks return [`Mode::Off`] so that bootstrap waves can work in
/// a clean state without the v3 harness interfering. Every other module
/// continues to return its normal mode.
///
/// The list is canonical — do not reorder or add ids without a matching spec
/// update.
const BOOTSTRAP_DISABLED_IDS: &[&str] = &[
    "enforce_registry",
    "close_gate",
    "path_guard",
    "size_gate",
    "model_routing",
    "prompt_gate",
    "skills_audit",
    "spec_hygiene",
    "subagent_inject",
    "amend_capture",
    "auto_capture_summary",
    "knowledge",
];

/// Pure helper: compute the [`Mode`] for `id` given the bootstrap env value.
///
/// Separating I/O (`std::env::var`) from logic keeps tests deterministic —
/// the public `mode_for` wrapper reads the env and delegates here.
///
/// `bootstrap` mirrors what `std::env::var("MUSTARD_V4_BOOTSTRAP")` returns:
/// - `None`  → env var is unset → normal mode
/// - `Some("")` → set but empty → treated as unset (defensive: prevents
///   `MUSTARD_V4_BOOTSTRAP=` from silencing all 12 hooks accidentally)
/// - `Some(non-empty)` → bootstrap active; ids in [`BOOTSTRAP_DISABLED_IDS`]
///   get [`Mode::Off`]
#[must_use]
fn mode_for_with_env(id: &str, bootstrap: Option<&str>) -> Mode {
    match bootstrap {
        Some(val) if !val.is_empty() && BOOTSTRAP_DISABLED_IDS.contains(&id) => Mode::Off,
        _ => Mode::default(),
    }
}

/// The enforcement [`Mode`] for a module id.
///
/// Under normal conditions every module returns [`Mode::Strict`], matching the
/// JS hooks' treatment of an unset `MUSTARD_*_MODE` variable.
///
/// **Bootstrap mode (`MUSTARD_V4_BOOTSTRAP`):** when this env var is set to a
/// non-empty value, the 12 v3 hooks listed in [`BOOTSTRAP_DISABLED_IDS`]
/// return [`Mode::Off`] so that v4 refoundation waves can work in a clean
/// state. An empty string is treated as unset (defensive — prevents
/// `MUSTARD_V4_BOOTSTRAP=` from silently disabling all 12 hooks).
///
/// The dispatcher already honours [`Mode::Off`] correctly (exits early before
/// running the check); `dispatch.rs` does not need to be modified.
#[must_use]
pub fn mode_for(id: &str) -> Mode {
    let bootstrap = std::env::var("MUSTARD_V4_BOOTSTRAP").ok();
    mode_for_with_env(id, bootstrap.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ids of every module applicable to the given event/tool.
    fn applicable_ids(
        registry: &Registry,
        trigger: Trigger,
        tool: Option<&str>,
    ) -> Vec<&'static str> {
        registry
            .applicable(trigger, tool)
            .iter()
            .map(|m| m.id)
            .collect()
    }

    #[test]
    fn bash_guard_applies_to_bash_events() {
        let registry = Registry::new();
        // `bash_guard` is the Bash-tool gate for both Pre- and PostToolUse.
        assert!(applicable_ids(&registry, Trigger::PreToolUse, Some("Bash"))
            .contains(&"bash_guard"));
        assert!(applicable_ids(&registry, Trigger::PostToolUse, Some("Bash"))
            .contains(&"bash_guard"));
        // It does not apply to a Write tool or a bare lifecycle event.
        assert!(!applicable_ids(&registry, Trigger::PreToolUse, Some("Write"))
            .contains(&"bash_guard"));
    }

    #[test]
    fn wildcard_counters_apply_to_every_pre_tool_use() {
        let registry = Registry::new();
        // `tool_use_counter` / `main_context_counter` use `ToolMatch::Any` —
        // they fire on PreToolUse for any tool (the JS `.*` matcher).
        for tool in ["Bash", "Write", "Read", "Task"] {
            let ids = applicable_ids(&registry, Trigger::PreToolUse, Some(tool));
            assert!(ids.contains(&"tool_use_counter"), "missing for {tool}");
            assert!(ids.contains(&"main_context_counter"), "missing for {tool}");
        }
    }

    #[test]
    fn task_family_applies_on_pre_tool_use_task() {
        let registry = Registry::new();
        let ids = applicable_ids(&registry, Trigger::PreToolUse, Some("Task"));
        for want in ["budget", "model_routing", "subagent_tracker", "skills_audit"] {
            assert!(ids.contains(&want), "missing {want}");
        }
    }

    #[test]
    fn subagent_lifecycle_runs_only_the_counters() {
        let registry = Registry::new();
        // `SubagentStart` (a non-tool event) → only the two counters apply.
        let ids = applicable_ids(&registry, Trigger::SubagentStart, None);
        assert!(ids.contains(&"tool_use_counter"));
        assert!(ids.contains(&"main_context_counter"));
        assert!(!ids.contains(&"bash_guard"));
    }

    #[test]
    fn skill_post_tool_use_runs_skill_usage_tracker() {
        let registry = Registry::new();
        let ids = applicable_ids(&registry, Trigger::PostToolUse, Some("Skill"));
        assert!(ids.contains(&"skill_usage_tracker"));
    }

    #[test]
    fn by_id_finds_registered_modules() {
        let registry = Registry::new();
        for id in [
            "bash_guard",
            "budget",
            "model_routing",
            "tool_use_counter",
            "main_context_counter",
            "subagent_tracker",
            "metrics_tracker",
            "skill_usage_tracker",
            "tool_result",
            "skills_audit",
            "size_gate",
            "path_guard",
            "close_gate",
            "enforce_registry",
            "post_edit",
            "spec_hygiene",
            "session_start",
            "knowledge",
            "session_cleanup",
            "pre_compact",
            "prompt_gate",
            "amend_capture",
        ] {
            assert!(registry.by_id(id).is_some(), "by_id missing {id}");
        }
        assert!(registry.by_id("nonexistent").is_none());
    }

    #[test]
    fn wave5_session_families_apply_to_their_events() {
        let registry = Registry::new();
        // `session_start` on SessionStart.
        assert!(applicable_ids(&registry, Trigger::SessionStart, None)
            .contains(&"session_start"));
        // `spec_hygiene` also runs on SessionStart, *before* `session_start`.
        let start = applicable_ids(&registry, Trigger::SessionStart, None);
        assert!(start.contains(&"spec_hygiene"));
        let hyg_idx = start.iter().position(|id| *id == "spec_hygiene");
        let ss_idx = start.iter().position(|id| *id == "session_start");
        assert!(hyg_idx < ss_idx, "spec_hygiene must precede session_start");
        // `session_cleanup` + `knowledge` on SessionEnd.
        let end = applicable_ids(&registry, Trigger::SessionEnd, None);
        assert!(end.contains(&"session_cleanup"));
        assert!(end.contains(&"knowledge"));
        // `pre_compact` on PreCompact.
        assert!(applicable_ids(&registry, Trigger::PreCompact, None)
            .contains(&"pre_compact"));
        // `prompt_gate` on UserPromptSubmit.
        assert!(applicable_ids(&registry, Trigger::UserPromptSubmit, None)
            .contains(&"prompt_gate"));
        // `knowledge` also covers PostToolUse(Task).
        assert!(applicable_ids(&registry, Trigger::PostToolUse, Some("Task"))
            .contains(&"knowledge"));
    }

    // ── Bootstrap mode tests (S3-2.a … S3-2.c) ─────────────────────────────
    //
    // These tests drive `mode_for_with_env` directly so they are fully
    // deterministic — no process-global env mutation needed (M9).

    #[test]
    fn mode_for_unset_bootstrap_defaults_to_strict() {
        // env unset → None → all ids return Strict, including listed ones.
        for id in super::BOOTSTRAP_DISABLED_IDS {
            assert_eq!(
                super::mode_for_with_env(id, None),
                Mode::Strict,
                "expected Strict for listed id {id} when bootstrap unset"
            );
        }
        // An unlisted id also returns Strict.
        assert_eq!(
            super::mode_for_with_env("bash_guard", None),
            Mode::Strict,
            "expected Strict for unlisted id bash_guard when bootstrap unset"
        );
    }

    #[test]
    fn mode_for_set_bootstrap_returns_off_for_listed() {
        // env = "1" (non-empty) → listed ids return Off; unlisted return Strict.
        for id in super::BOOTSTRAP_DISABLED_IDS {
            assert_eq!(
                super::mode_for_with_env(id, Some("1")),
                Mode::Off,
                "expected Off for listed id {id} when bootstrap = \"1\""
            );
        }
        assert_eq!(
            super::mode_for_with_env("bash_guard", Some("1")),
            Mode::Strict,
            "expected Strict for unlisted id bash_guard when bootstrap = \"1\""
        );
    }

    #[test]
    fn mode_for_empty_string_bootstrap_defaults_to_strict() {
        // env = "" (set but empty) → treated as unset → Strict for all ids.
        for id in super::BOOTSTRAP_DISABLED_IDS {
            assert_eq!(
                super::mode_for_with_env(id, Some("")),
                Mode::Strict,
                "expected Strict for listed id {id} when bootstrap = \"\" (defensive)"
            );
        }
        assert_eq!(
            super::mode_for_with_env("bash_guard", Some("")),
            Mode::Strict,
            "expected Strict for bash_guard when bootstrap = \"\""
        );
    }

    // ────────────────────────────────────────────────────────────────────────

    #[test]
    fn write_edit_family_applies_on_pre_tool_use() {
        let registry = Registry::new();
        // Wave-4 Write/Edit gates fire on PreToolUse(Write) and (Edit).
        for tool in ["Write", "Edit"] {
            let ids = applicable_ids(&registry, Trigger::PreToolUse, Some(tool));
            for want in ["size_gate", "path_guard", "close_gate"] {
                assert!(ids.contains(&want), "missing {want} for {tool}");
            }
        }
        // `path_guard` (file-guard) also covers Read.
        assert!(
            applicable_ids(&registry, Trigger::PreToolUse, Some("Read")).contains(&"path_guard")
        );
        // `post_edit` runs on PostToolUse(Write|Edit).
        for tool in ["Write", "Edit"] {
            assert!(
                applicable_ids(&registry, Trigger::PostToolUse, Some(tool)).contains(&"post_edit")
            );
        }
        // `enforce_registry` runs on PreToolUse(Skill).
        assert!(
            applicable_ids(&registry, Trigger::PreToolUse, Some("Skill"))
                .contains(&"enforce_registry")
        );
    }
}
