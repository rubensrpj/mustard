//! The module registry — which enforcement modules run for which event/tool.
//!
//! Open/Closed in practice (b3 spec § Arquitetura "SOLID"): adding a check is
//! *only* registering a [`Module`] here. The dispatcher reads the registry and
//! never changes. A module is keyed by the `(Trigger, tool)` pairs it applies
//! to, so an unrelated invocation skips it entirely instead of running it just
//! to have it self-`Allow`.

use crate::hooks::observe::amend_window_inject::AmendWindowInject;
use crate::hooks::observe::change_request_log::ChangeRequestLog;
use crate::hooks::observe::feature_outcome_observer::FeatureOutcomeObserver;
use crate::hooks::observe::agent_summary_observer::AgentSummaryObserver;
use crate::hooks::bash::bash_command_gate::BashCommandGate;
use crate::hooks::task::context_budget_gate::ContextBudgetGate;
use crate::hooks::task::delegation_advisory::DelegationAdvisory;
use crate::hooks::write::active_spec_limit_gate::ActiveSpecLimitGate;
use crate::hooks::write::close_gate::CloseGate;
use crate::hooks::write::scan_gate::ScanGate;
use crate::hooks::write::scope_guard::ScopeGuard;
use crate::hooks::session::session_knowledge_observer::SessionKnowledgeObserver;
use crate::hooks::observe::notification_observer::NotificationObserver;
use crate::hooks::observe::prompt_observer::PromptObserver;
use crate::hooks::observe::rewave_observer::RewaveObserver;
use crate::hooks::observe::wave_complete_observer::WaveCompleteObserver;
use crate::hooks::observe::wave_start_observer::WaveStartObserver;
use crate::hooks::write::path_gate::PathGate;
use crate::hooks::write::post_edit::PostEdit;
use crate::hooks::session::pre_compact_inject::PreCompactInject;
use crate::hooks::write::pre_edit_intent_gate::PreEditIntentGate;
use crate::hooks::session::prompt_submit_inject::PromptSubmitInject;
use crate::hooks::session::session_cleanup_observer::SessionCleanupObserver;
use crate::hooks::session::session_start_inject::SessionStartInject;
use crate::hooks::write::size_gate::SizeGate;
use crate::hooks::task::skills_advisory::SkillsAdvisory;
use crate::hooks::session::spec_hygiene_observer::SpecHygieneObserver;
use crate::hooks::observe::session_stop_observer::SessionStopObserver;
use crate::hooks::observe::subagent_stop_observer::SubagentStopObserver;
use crate::hooks::observe::memory_promote_observer::MemoryPromoteObserver;
use crate::hooks::task::subagent_inject::SubagentInject;
use crate::hooks::observe::tool_result_observer::ToolResultObserver;
use crate::hooks::task::main_context_counter::MainContextCounter;
use crate::hooks::task::metrics_observer::MetricsObserver;
use crate::hooks::task::skill_usage_observer::SkillUsageObserver;
use crate::hooks::task::subagent_observer::SubagentObserver;
use crate::hooks::task::tool_use_counter::ToolUseCounter;
use crate::hooks::observe::wikilink_footer_observer::WikilinkFooterObserver;
use mustard_core::domain::model::contract::{Check, Observer, Trigger};

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
/// `bash_command_gate`, for example, is both — the four ported PreToolUse(Bash) gates
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
    /// Early b3 waves register only `bash_command_gate`; later waves push their
    /// families (`budget`, `size_gate`, …) here, leaving the dispatcher
    /// untouched.
    #[must_use]
    // Registry::new() is a flat list of module registrations — refactoring into
    // helper functions would obscure the registry structure without reducing complexity.
    #[allow(clippy::too_many_lines)]
    pub fn new() -> Self {
        let modules = vec![
            Module {
                id: "bash_command_gate",
                // `bash_command_gate` is both a `Check` and an `Observer` — it ports
                // the full Bash family (5/5): `bash-safety`,
                // `bash-native-redirect`, `rtk-rewrite` and `review-gate` as
                // PreToolUse(Bash) gates, plus `pr-detect` as PostToolUse(Bash)
                // telemetry.
                applies_to: &[
                    (Trigger::PreToolUse, ToolMatch::Named("Bash")),
                    (Trigger::PostToolUse, ToolMatch::Named("Bash")),
                ],
                check: Some(Box::new(BashCommandGate)),
                observer: Some(Box::new(BashCommandGate)),
            },
            // ── Wave 3: Task / Subagent family ───────────────────────────────
            Module {
                id: "context_budget_gate",
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
                check: Some(Box::new(ContextBudgetGate)),
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
                id: "subagent_observer",
                // `subagent-tracker` — `agent.start` / `agent.stop` telemetry.
                applies_to: &[
                    (Trigger::PreToolUse, ToolMatch::Named("Task")),
                    (Trigger::PreToolUse, ToolMatch::Named("Agent")),
                    (Trigger::PostToolUse, ToolMatch::Named("Task")),
                    (Trigger::PostToolUse, ToolMatch::Named("Agent")),
                ],
                check: None,
                observer: Some(Box::new(SubagentObserver)),
            },
            Module {
                id: "metrics_observer",
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
                observer: Some(Box::new(MetricsObserver)),
            },
            Module {
                id: "skill_usage_observer",
                // `skill-usage-tracker` — `skill.invoked` event per Skill call.
                applies_to: &[(Trigger::PostToolUse, ToolMatch::Named("Skill"))],
                check: None,
                observer: Some(Box::new(SkillUsageObserver)),
            },
            Module {
                id: "tool_result_observer",
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
                observer: Some(Box::new(ToolResultObserver)),
            },
            Module {
                id: "skills_advisory",
                // `recommended-skills-audit` — advisory count on PreToolUse(Task).
                applies_to: &[
                    (Trigger::PreToolUse, ToolMatch::Named("Task")),
                    (Trigger::PreToolUse, ToolMatch::Named("Agent")),
                ],
                check: Some(Box::new(SkillsAdvisory)),
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
                id: "path_gate",
                // `file-guard` (PreToolUse(Read|Write|Edit) sensitive-file
                // gate) + `boundary-gate` (PreToolUse(Write|Edit) spec-boundary
                // gate). Registered on Read too so `file-guard` covers reads.
                applies_to: &[
                    (Trigger::PreToolUse, ToolMatch::Named("Read")),
                    (Trigger::PreToolUse, ToolMatch::Named("Write")),
                    (Trigger::PreToolUse, ToolMatch::Named("Edit")),
                ],
                check: Some(Box::new(PathGate)),
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
                id: "scan_gate",
                // `scan-gate` — PreToolUse(Skill) pre-pipeline gate (grain model).
                applies_to: &[(Trigger::PreToolUse, ToolMatch::Named("Skill"))],
                check: Some(Box::new(ScanGate)),
                observer: None,
            },
            // F4-d item 1 — hard cap on concurrently active pipelines. A
            // PreToolUse(Skill) gate sibling to `scan_gate`: it sits
            // on the entry of `/feature` and `/bugfix` and refuses (strict) or
            // warns (default) when opening another pipeline would exceed
            // `mustard.json#maxActiveSpecs` (default 10). Mode via
            // `MUSTARD_MAX_ACTIVE_SPECS_MODE` (off|warn|strict). Fail-open: a
            // counting error can only under-count, never trip the cap.
            Module {
                id: "active_spec_limit_gate",
                applies_to: &[(Trigger::PreToolUse, ToolMatch::Named("Skill"))],
                check: Some(Box::new(ActiveSpecLimitGate)),
                observer: None,
            },
            // Spec A v4 / W4 — opt-in pre-edit intent check (Moment 1 of the
            // regression gate). Gated by `MUSTARD_V4_GATE_ENABLED=1` inside
            // the module so the v3 harness keeps its semantics by default.
            Module {
                id: "pre_edit_intent_gate",
                applies_to: &[
                    (Trigger::PreToolUse, ToolMatch::Named("Write")),
                    (Trigger::PreToolUse, ToolMatch::Named("Edit")),
                ],
                check: Some(Box::new(PreEditIntentGate)),
                observer: None,
            },
            // Full-scope approval hard-gate (D5 — spec-scaffold-lifecycle-gate).
            // Denies a PreToolUse(Write|Edit) of a PRODUCTION file when the
            // active spec is `scope=full`, `stage=Plan`, and has no `/spec`
            // approval event. Registered on Task|Agent too (the prompt's "covers
            // Task dispatch") — the module itself passes Task through so the
            // legitimate Full-scope PLAN dispatch is never trapped; the
            // production-file protection re-fires on the subagent's own
            // Write/Edit calls. Fail-open inside the module.
            Module {
                id: "scope_guard",
                applies_to: &[
                    (Trigger::PreToolUse, ToolMatch::Named("Write")),
                    (Trigger::PreToolUse, ToolMatch::Named("Edit")),
                    (Trigger::PreToolUse, ToolMatch::Named("Task")),
                    (Trigger::PreToolUse, ToolMatch::Named("Agent")),
                ],
                check: Some(Box::new(ScopeGuard)),
                observer: None,
            },
            // The digest-outcome SIGNAL — fires on EVERY PostToolUse so it can
            // discipline the research window: while the `active-research.json`
            // marker (dropped by `feature::run`) is open, a Read/Edit/Write
            // correlates the touched file with the digest's anchors and emits
            // `feature.outcome {file, wasAnchor, terms}`; the FIRST tool that is
            // NOT Read/Edit/Write CLOSES the window (removes the marker) so the
            // implementation phase's reads/edits do not leak in. Pure Observer —
            // telemetry only, never a verdict (a tool always proceeds),
            // fail-open. Folded by `run digest-precision`.
            Module {
                id: "feature_outcome_observer",
                applies_to: &[(Trigger::PostToolUse, ToolMatch::Any)],
                check: None,
                observer: Some(Box::new(FeatureOutcomeObserver)),
            },
            Module {
                id: "delegation_advisory",
                // Advisory (L0 Universal Delegation): on PostToolUse(Write|Edit)
                // it counts DISTINCT files the main context edits during an
                // active pipeline and, past a threshold, reminds the
                // orchestrator to delegate via Task. Pure Observer —
                // side-effects only, NEVER blocks (it cannot return a verdict).
                applies_to: &[
                    (Trigger::PostToolUse, ToolMatch::Named("Write")),
                    (Trigger::PostToolUse, ToolMatch::Named("Edit")),
                ],
                check: None,
                observer: Some(Box::new(DelegationAdvisory)),
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
            // `spec_hygiene_observer` is registered *before* `session_start_inject`
            // so its gated auto-close (and the spec-header rewrite it performs)
            // runs ahead of the SessionStart memory injection. It is a pure
            // side effect (an `Observer`), and the dispatcher runs a module's
            // observer before its check, so registering it first preserves the
            // ordering (spec-lifecycle-unification W5).
            Module {
                id: "spec_hygiene_observer",
                // SessionStart-only side effect — emits `hygiene.*` events and,
                // for a green close-gate, auto-closes a candidate spec. No
                // verdict (its output is the event stream) → an `Observer`.
                applies_to: &[(Trigger::SessionStart, ToolMatch::Any)],
                check: None,
                observer: Some(Box::new(SpecHygieneObserver)),
            },
            Module {
                id: "session_start_inject",
                // `harness-init` + `session-memory` + `spec-hygiene` — the
                // SessionStart bootstrap. A `Check` (the memory-injection
                // payload is its `Inject` verdict).
                applies_to: &[(Trigger::SessionStart, ToolMatch::Any)],
                check: Some(Box::new(SessionStartInject)),
                observer: None,
            },
            Module {
                id: "session_knowledge_observer",
                // `session-knowledge` + `memory-auto-extract` on SessionEnd,
                // `session-knowledge-inc` on PostToolUse(Task). Pure telemetry
                // — an `Observer`.
                applies_to: &[
                    (Trigger::SessionEnd, ToolMatch::Any),
                    (Trigger::PostToolUse, ToolMatch::Named("Task")),
                    (Trigger::PostToolUse, ToolMatch::Named("Agent")),
                ],
                check: None,
                observer: Some(Box::new(SessionKnowledgeObserver)),
            },
            Module {
                id: "session_cleanup_observer",
                // `session-cleanup` — SessionEnd stale-state cleanup. An
                // `Observer` (pure side effect, no verdict).
                applies_to: &[(Trigger::SessionEnd, ToolMatch::Any)],
                check: None,
                observer: Some(Box::new(SessionCleanupObserver)),
            },
            Module {
                id: "pre_compact_inject",
                // `pre-compact` — the PreCompact snapshot. A `Check` (the
                // snapshot is its `Inject` verdict).
                applies_to: &[(Trigger::PreCompact, ToolMatch::Any)],
                check: Some(Box::new(PreCompactInject)),
                observer: None,
            },
            Module {
                id: "prompt_submit_inject",
                // `followup-cancel-gate` — UserPromptSubmit follow-up archival.
                // A `Check` (always allows; the archival is its side effect).
                applies_to: &[(Trigger::UserPromptSubmit, ToolMatch::Any)],
                check: Some(Box::new(PromptSubmitInject)),
                observer: None,
            },
            // ── W8 deep-refactor: context-injection optimisation ─────────────
            Module {
                id: "subagent_inject",
                // T8.3 — for Task dispatches without a declared SKILL, inject a
                // minimal CONTEXT.md + skills slice (resolved via W1's
                // `skill-resolve`).
                //
                // Spec A v4 / W5.T5.2 adds `SubagentStop` so the same module
                // can run the span-level regression eval per returning child
                // (AC-A-5). The `SubagentStop` branch is fail-open and never
                // emits a blocking verdict — the per-child verdict lands in
                // `_review-spans.md` and AC-A-7's consolidation gate reads
                // the ledger at wave close.
                applies_to: &[
                    (Trigger::PreToolUse, ToolMatch::Named("Task")),
                    (Trigger::PreToolUse, ToolMatch::Named("Agent")),
                    (Trigger::SubagentStop, ToolMatch::Any),
                ],
                check: Some(Box::new(SubagentInject)),
                observer: None,
            },
            Module {
                id: "agent_summary_observer",
                // T8.4 — on Task return, parse `<MEMORY>` or `Resumo:` and
                // persist to `agent_memory`.
                applies_to: &[
                    (Trigger::PostToolUse, ToolMatch::Named("Task")),
                    (Trigger::PostToolUse, ToolMatch::Named("Agent")),
                ],
                check: None,
                observer: Some(Box::new(AgentSummaryObserver)),
            },
            Module {
                id: "subagent_stop_observer",
                // T8.5 — SubagentStop reinforcement: bump `last_used` on any
                // agent_memory row whose summary appeared in the output.
                applies_to: &[(Trigger::SubagentStop, ToolMatch::Any)],
                check: None,
                observer: Some(Box::new(SubagentStopObserver)),
            },
            Module {
                id: "memory_promote_observer",
                // T8.6 — SessionEnd promotion of high-confidence agent_memory
                // rows to permanent memory_decisions / memory_lessons rows.
                applies_to: &[(Trigger::SessionEnd, ToolMatch::Any)],
                check: None,
                observer: Some(Box::new(MemoryPromoteObserver)),
            },
            // ── W9 deep-refactor: Stop + Notification triggers ───────────────
            Module {
                id: "session_stop_observer",
                // `Stop` lifecycle observer — touches the 5-minute anti-spam
                // marker AND captures the orchestrator's own `<MEMORY>…</MEMORY>`
                // blocks from its final output as Knowledge (the capture point
                // for light, direct `/task`/bugfix work that dispatches no
                // subagent). Main session only — never SubagentStop.
                applies_to: &[(Trigger::Stop, ToolMatch::Any)],
                check: None,
                observer: Some(Box::new(SessionStopObserver)),
            },
            Module {
                id: "notification_observer",
                // `Notification` lifecycle observer — appends a single
                // `notification.received` event to the per-spec NDJSON log;
                // observe-only, no auto-resolution.
                applies_to: &[(Trigger::Notification, ToolMatch::Any)],
                check: None,
                observer: Some(Box::new(NotificationObserver)),
            },
            Module {
                id: "user_prompt_observer",
                // `UserPromptSubmit` lifecycle observer — appends a single
                // `user.prompt {prompt}` event to the per-spec NDJSON log (or
                // the per-session sink under `.claude/.session/{id}/.events/`
                // when no spec is resolvable), so the dashboard can render
                // "what I asked" in the trace. Observe-only, unconditional,
                // never blocks the prompt.
                applies_to: &[(Trigger::UserPromptSubmit, ToolMatch::Any)],
                check: None,
                observer: Some(Box::new(PromptObserver)),
            },
            // ── W3E (no-sqlite git source of truth) — wikilink footer ────────
            Module {
                id: "wikilink_footer_observer",
                // PostToolUse(Write|Edit) auto-footer renderer for
                // `.claude/{memory,knowledge,spec}/**/*.md`. Pure Observer —
                // the render logic lives in `mustard_core::io::atomic_md::wikilink`.
                applies_to: &[
                    (Trigger::PostToolUse, ToolMatch::Named("Write")),
                    (Trigger::PostToolUse, ToolMatch::Named("Edit")),
                ],
                check: None,
                observer: Some(Box::new(WikilinkFooterObserver)),
            },
            // ── Wave 6: session-bound amendment window ───────────────────────
            Module {
                id: "amend_window_inject",
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
                check: Some(Box::new(AmendWindowInject)),
                observer: Some(Box::new(AmendWindowInject)),
            },
            // Mid-pipeline counterpart to `amend_window_inject`: records every
            // user request made WHILE a spec is Active to
            // `.claude/spec/{id}/change-requests.ndjson` + a
            // `pipeline.change.request` event, so chat-driven changes no longer
            // vanish. Pure Observer — side-effects only, never blocks.
            Module {
                id: "change_request_log",
                applies_to: &[(Trigger::UserPromptSubmit, ToolMatch::Any)],
                check: None,
                observer: Some(Box::new(ChangeRequestLog)),
            },
            // ── FASE 4-c: auto-abertura por tipo (structural → automatic) ────
            // Both are pure Observers — they emit/restructure as a side effect
            // and are structurally incapable of denying a write (decision 6:
            // re-wave / wave-advance are advisory restructuring, never gates).
            Module {
                id: "rewave_observer",
                // F4-c item 1 — on the first EXECUTE write of a not-yet-decomposed
                // spec, fire `exec_rewave_check::decompose_if_signaled` (idempotent
                // via the `wave-plan.md` guard). PreToolUse(Write|Edit), fail-open.
                applies_to: &[
                    (Trigger::PreToolUse, ToolMatch::Named("Write")),
                    (Trigger::PreToolUse, ToolMatch::Named("Edit")),
                ],
                check: None,
                observer: Some(Box::new(RewaveObserver)),
            },
            Module {
                id: "wave_start_observer",
                // DEFECT 2 (2026-06-05) — on SubagentStart, when an active wave
                // is resolvable (MUSTARD_ACTIVE_SPEC/WAVE), auto-emit
                // `pipeline.wave.start` once (idempotent via the NDJSON event
                // check; suppressed if the wave already completed). The
                // counterpart to `wave_complete_observer`: it lets the dashboard
                // mark a wave InProgress from an explicit signal. SubagentStart,
                // fail-open, never denies.
                applies_to: &[(Trigger::SubagentStart, ToolMatch::Any)],
                check: None,
                observer: Some(Box::new(WaveStartObserver)),
            },
            Module {
                id: "wave_complete_observer",
                // F4-c item 2 — on SubagentStop, when the active wave's
                // `_review-spans.md` ledger is clean (≥1 child returned, no red),
                // auto-emit `pipeline.wave.complete` (idempotent via the NDJSON
                // event check). SubagentStop, fail-open.
                applies_to: &[(Trigger::SubagentStop, ToolMatch::Any)],
                check: None,
                observer: Some(Box::new(WaveCompleteObserver)),
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
    fn bash_command_gate_applies_to_bash_events() {
        let registry = Registry::new();
        // `bash_command_gate` is the Bash-tool gate for both Pre- and PostToolUse.
        assert!(applicable_ids(&registry, Trigger::PreToolUse, Some("Bash"))
            .contains(&"bash_command_gate"));
        assert!(applicable_ids(&registry, Trigger::PostToolUse, Some("Bash"))
            .contains(&"bash_command_gate"));
        // It does not apply to a Write tool or a bare lifecycle event.
        assert!(!applicable_ids(&registry, Trigger::PreToolUse, Some("Write"))
            .contains(&"bash_command_gate"));
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
        for want in ["context_budget_gate", "subagent_observer", "skills_advisory"] {
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
        assert!(!ids.contains(&"bash_command_gate"));
    }

    #[test]
    fn skill_post_tool_use_runs_skill_usage_observer() {
        let registry = Registry::new();
        let ids = applicable_ids(&registry, Trigger::PostToolUse, Some("Skill"));
        assert!(ids.contains(&"skill_usage_observer"));
    }

    #[test]
    fn by_id_finds_registered_modules() {
        let registry = Registry::new();
        for id in [
            "bash_command_gate",
            "context_budget_gate",
            "tool_use_counter",
            "main_context_counter",
            "subagent_observer",
            "metrics_observer",
            "skill_usage_observer",
            "tool_result_observer",
            "skills_advisory",
            "size_gate",
            "path_gate",
            "close_gate",
            "scan_gate",
            "scope_guard",
            "active_spec_limit_gate",
            "delegation_advisory",
            "feature_outcome_observer",
            "post_edit",
            "spec_hygiene_observer",
            "session_start_inject",
            "session_knowledge_observer",
            "session_cleanup_observer",
            "pre_compact_inject",
            "prompt_submit_inject",
            "user_prompt_observer",
            "amend_window_inject",
            "rewave_observer",
            "wave_start_observer",
            "wave_complete_observer",
        ] {
            assert!(registry.by_id(id).is_some(), "by_id missing {id}");
        }
        assert!(registry.by_id("nonexistent").is_none());
    }

    #[test]
    fn fase4c_auto_open_observers_apply_to_their_events() {
        let registry = Registry::new();
        // `rewave_observer` joins the PreToolUse(Write|Edit) family.
        for tool in ["Write", "Edit"] {
            assert!(
                applicable_ids(&registry, Trigger::PreToolUse, Some(tool))
                    .contains(&"rewave_observer"),
                "rewave_observer missing for {tool}"
            );
        }
        // It does not fire on a Read, nor on SubagentStop.
        assert!(!applicable_ids(&registry, Trigger::PreToolUse, Some("Read"))
            .contains(&"rewave_observer"));
        // `wave_complete_observer` fires on SubagentStop (any tool / none).
        assert!(applicable_ids(&registry, Trigger::SubagentStop, None)
            .contains(&"wave_complete_observer"));
        // It does not fire on a plain PreToolUse(Write).
        assert!(!applicable_ids(&registry, Trigger::PreToolUse, Some("Write"))
            .contains(&"wave_complete_observer"));
        // `wave_start_observer` is the symmetric counterpart: it fires on
        // SubagentStart (any tool / none), not on SubagentStop.
        assert!(applicable_ids(&registry, Trigger::SubagentStart, None)
            .contains(&"wave_start_observer"));
        assert!(!applicable_ids(&registry, Trigger::SubagentStop, None)
            .contains(&"wave_start_observer"));
    }

    #[test]
    fn wave5_session_families_apply_to_their_events() {
        let registry = Registry::new();
        // `session_start_inject` on SessionStart.
        assert!(applicable_ids(&registry, Trigger::SessionStart, None)
            .contains(&"session_start_inject"));
        // `spec_hygiene_observer` also runs on SessionStart, *before* `session_start_inject`.
        let start = applicable_ids(&registry, Trigger::SessionStart, None);
        assert!(start.contains(&"spec_hygiene_observer"));
        let hyg_idx = start.iter().position(|id| *id == "spec_hygiene_observer");
        let ss_idx = start.iter().position(|id| *id == "session_start_inject");
        assert!(hyg_idx < ss_idx, "spec_hygiene_observer must precede session_start_inject");
        // `session_cleanup_observer` + `session_knowledge_observer` on SessionEnd.
        let end = applicable_ids(&registry, Trigger::SessionEnd, None);
        assert!(end.contains(&"session_cleanup_observer"));
        assert!(end.contains(&"session_knowledge_observer"));
        // `pre_compact_inject` on PreCompact.
        assert!(applicable_ids(&registry, Trigger::PreCompact, None)
            .contains(&"pre_compact_inject"));
        // `prompt_submit_inject` on UserPromptSubmit.
        assert!(applicable_ids(&registry, Trigger::UserPromptSubmit, None)
            .contains(&"prompt_submit_inject"));
        // `user_prompt_observer` also rides UserPromptSubmit.
        assert!(applicable_ids(&registry, Trigger::UserPromptSubmit, None)
            .contains(&"user_prompt_observer"));
        // `session_knowledge_observer` also covers PostToolUse(Task).
        assert!(applicable_ids(&registry, Trigger::PostToolUse, Some("Task"))
            .contains(&"session_knowledge_observer"));
    }

    #[test]
    fn write_edit_family_applies_on_pre_tool_use() {
        let registry = Registry::new();
        // Wave-4 Write/Edit gates fire on PreToolUse(Write) and (Edit).
        for tool in ["Write", "Edit"] {
            let ids = applicable_ids(&registry, Trigger::PreToolUse, Some(tool));
            for want in ["size_gate", "path_gate", "close_gate", "scope_guard"] {
                assert!(ids.contains(&want), "missing {want} for {tool}");
            }
        }
        // `scope_guard` also rides PreToolUse(Task|Agent) (covers dispatch).
        for tool in ["Task", "Agent"] {
            assert!(
                applicable_ids(&registry, Trigger::PreToolUse, Some(tool)).contains(&"scope_guard"),
                "scope_guard missing for {tool}"
            );
        }
        // `path_gate` (file-guard) also covers Read.
        assert!(
            applicable_ids(&registry, Trigger::PreToolUse, Some("Read")).contains(&"path_gate")
        );
        // `post_edit` runs on PostToolUse(Write|Edit).
        for tool in ["Write", "Edit"] {
            assert!(
                applicable_ids(&registry, Trigger::PostToolUse, Some(tool)).contains(&"post_edit")
            );
        }
        // `delegation_advisory` rides PostToolUse(Write|Edit) too.
        for tool in ["Write", "Edit"] {
            assert!(
                applicable_ids(&registry, Trigger::PostToolUse, Some(tool))
                    .contains(&"delegation_advisory"),
                "delegation_advisory missing for {tool}"
            );
        }
        // It does not fire on a PreToolUse(Write) nor on a Read.
        assert!(!applicable_ids(&registry, Trigger::PreToolUse, Some("Write"))
            .contains(&"delegation_advisory"));
        assert!(!applicable_ids(&registry, Trigger::PostToolUse, Some("Read"))
            .contains(&"delegation_advisory"));
        // `feature_outcome_observer` (the digest-outcome SIGNAL) now fires on
        // EVERY PostToolUse tool — Read/Edit/Write emit outcomes, any OTHER
        // tool closes the research window — and never on the Pre side.
        for tool in ["Read", "Edit", "Write", "Bash", "Task", "Grep"] {
            assert!(
                applicable_ids(&registry, Trigger::PostToolUse, Some(tool))
                    .contains(&"feature_outcome_observer"),
                "feature_outcome_observer missing for PostToolUse({tool})"
            );
        }
        assert!(!applicable_ids(&registry, Trigger::PreToolUse, Some("Read"))
            .contains(&"feature_outcome_observer"));
        // `scan_gate` + `active_spec_limit_gate` run on
        // PreToolUse(Skill) — the two pipeline-entry gates.
        for want in ["scan_gate", "active_spec_limit_gate"] {
            assert!(
                applicable_ids(&registry, Trigger::PreToolUse, Some("Skill")).contains(&want),
                "missing {want} on PreToolUse(Skill)"
            );
        }
    }
}
