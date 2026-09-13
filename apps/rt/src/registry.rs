//! The module registry — which enforcement modules run for which event/tool.
//!
//! Open/Closed in practice (SOLID): adding a check is
//! *only* registering a [`Module`] here. The dispatcher reads the registry and
//! never changes. A module is keyed by the `(Trigger, tool)` pairs it applies
//! to, so an unrelated invocation skips it entirely instead of running it just
//! to have it self-`Allow`.

use crate::hooks::observe::amend_window_inject::AmendWindowInject;
use crate::hooks::observe::change_request_log::ChangeRequestLog;
use crate::hooks::observe::approval_marker_observer::ApprovalMarkerObserver;
use crate::hooks::observe::clarification_observer::ClarificationObserver;
use crate::hooks::observe::picker_approval_observer::PickerApprovalObserver;
use crate::hooks::observe::plan_approval_observer::PlanApprovalObserver;
use crate::hooks::bash::bash_command_gate::BashCommandGate;
use crate::hooks::task::context_budget_gate::ContextBudgetGate;
use crate::hooks::task::delegation_advisory::DelegationAdvisory;
use crate::hooks::write::active_spec_limit_gate::ActiveSpecLimitGate;
use crate::hooks::write::close_gate::CloseGate;
use crate::hooks::write::mold_gate::MoldGate;
use crate::hooks::write::scan_gate::ScanGate;
use crate::hooks::write::write_gate::WriteGate;
use crate::hooks::observe::prompt_observer::PromptObserver;
use crate::hooks::observe::rewave_observer::RewaveObserver;
use crate::hooks::observe::wave_complete_observer::WaveCompleteObserver;
use crate::hooks::observe::wave_start_observer::WaveStartObserver;
use crate::hooks::write::boundary_gate::BoundaryGate;
use crate::hooks::write::post_edit::PostEdit;
use crate::hooks::session::prompt_submit_inject::PromptSubmitInject;
use crate::hooks::session::session_cleanup_observer::SessionCleanupObserver;
use crate::hooks::session::session_start_inject::SessionStartInject;
use crate::hooks::session::dashboard_register_observer::DashboardRegisterObserver;
use crate::hooks::session::statusline_heal_observer::StatuslineHealObserver;
use crate::hooks::write::size_gate::SizeGate;
use crate::hooks::session::spec_hygiene_observer::SpecHygieneObserver;
use crate::hooks::task::subagent_inject::SubagentInject;
use crate::hooks::observe::tool_result_observer::ToolResultObserver;
use crate::hooks::task::main_context_counter::MainContextCounter;
use crate::hooks::task::metrics_observer::MetricsObserver;
use crate::hooks::task::skill_usage_observer::SkillUsageObserver;
use crate::hooks::task::end_of_turn_check::EndOfTurnCheck;
use crate::hooks::task::subagent_observer::SubagentObserver;
use crate::hooks::task::tool_use_counter::ToolUseCounter;
use crate::hooks::observe::wikilink_footer_observer::WikilinkFooterObserver;
use mustard_core::domain::model::contract::{Check, Observer, Trigger};

/// Which tool an `(event, tool)` registration entry applies to.
///
/// The JS `settings.json` matchers are one of: a literal tool name (`"Bash"`,
/// `"Task"`), an alternation (`"Task|Agent"` — expressed as two entries here),
/// the wildcard `".*"` (every tool), or absent (a non-tool lifecycle event
/// like `SubagentStart`). [`ToolMatch`] models the two cases a module can register.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolMatch {
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
    /// Early port stages register only `bash_command_gate`; later stages push their
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
                // `bash_command_gate` is both a `Check` and an `Observer`: the
                // command guard, the Windows-path check, the native redirect,
                // the commit review and the pull-request advisories as
                // PreToolUse(Bash) gates, plus `pr-detect` as PostToolUse(Bash)
                // telemetry.
                applies_to: &[
                    (Trigger::PreToolUse, ToolMatch::Named("Bash")),
                    (Trigger::PostToolUse, ToolMatch::Named("Bash")),
                ],
                check: Some(Box::new(BashCommandGate)),
                observer: Some(Box::new(BashCommandGate)),
            },
            // ── Task / Subagent family ───────────────────────────────────────
            Module {
                id: "context_budget_gate",
                // `context-budget` (PreToolUse(Task) prompt-size gate) +
                // `output-budget` (PostToolUse(Task) return-size advisory).
                // Both flow through the `Check` — the over-budget advisory is
                // an `Inject` verdict, not a raw stdout write (the old
                // `budget::observe` wrote to stdout, around the contract).
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
                // `main-context-counter` — enforces delegation on the orchestrator.
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
                // `tool.use`.
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
            // ── Write/Edit family ────────────────────────────────────────────
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
            // `write_gate` — o portão de escrita, nas cinco ferramentas de
            // arquivo. As regras, na ordem: segredo, arquivos que só o binário
            // grava, aprovação, branch da spec (só avisa) e base do
            // `git.flow`. A primeira que responde decide.
            Module {
                id: "write_gate",
                applies_to: &[
                    (Trigger::PreToolUse, ToolMatch::Named("Read")),
                    (Trigger::PreToolUse, ToolMatch::Named("Write")),
                    (Trigger::PreToolUse, ToolMatch::Named("Edit")),
                    (Trigger::PreToolUse, ToolMatch::Named("MultiEdit")),
                    (Trigger::PreToolUse, ToolMatch::Named("NotebookEdit")),
                ],
                check: Some(Box::new(WriteGate)),
                observer: None,
            },
            Module {
                id: "boundary_gate",
                // `boundary-gate` — PreToolUse(Write|Edit) spec-boundary gate.
                // The sensitive-file law lives in `permissions.deny` (first
                // line) + the `write_gate` above; boundary itself never
                // inspects Read.
                applies_to: &[
                    (Trigger::PreToolUse, ToolMatch::Named("Write")),
                    (Trigger::PreToolUse, ToolMatch::Named("Edit")),
                ],
                check: Some(Box::new(BoundaryGate)),
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
            // Hard cap on concurrently active pipelines. A
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
            // Skill-usage loop, the "during" hook: on a NEW file whose kind
            // matches a `{role}-pattern` mold of its subproject, a non-blocking
            // advisory points at the SKILL.md before the first byte lands.
            // Creation-only (no per-edit nagging); advisory-only by design
            // (mold enforcement belongs to REVIEW). Fail-open inside.
            Module {
                id: "mold_gate",
                applies_to: &[(Trigger::PreToolUse, ToolMatch::Named("Write"))],
                check: Some(Box::new(MoldGate)),
                observer: None,
            },
            Module {
                id: "delegation_advisory",
                // Advisory (delegate to subagents): on PostToolUse(Write|Edit)
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
            // ── session-lifecycle families ───────────────────────────────────
            // `spec_hygiene_observer` is registered *before* `session_start_inject`
            // so its gated auto-close (and the spec-header rewrite it performs)
            // runs ahead of the SessionStart memory injection. It is a pure
            // side effect (an `Observer`), and the dispatcher runs a module's
            // observer before its check, so registering it first preserves the
            // ordering.
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
                // `harness-init` + `spec-hygiene` + terrain census + declared
                // injectables (`mustard.json#inject`, `on: sessionStart`) —
                // the SessionStart bootstrap. A `Check` (terrain + injectables
                // compose into its single `Inject` verdict; a post-compaction
                // start re-arms the once-per-session markers).
                applies_to: &[(Trigger::SessionStart, ToolMatch::Any)],
                check: Some(Box::new(SessionStartInject)),
                observer: None,
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
                id: "statusline_heal_observer",
                // `statusline-heal` — SessionStart self-heal of the
                // `statusLine` entry in `.claude/settings.local.json` (points
                // it at the running binary). An `Observer` (pure side effect,
                // no verdict).
                applies_to: &[(Trigger::SessionStart, ToolMatch::Any)],
                check: None,
                observer: Some(Box::new(StatuslineHealObserver)),
            },
            Module {
                id: "dashboard_register_observer",
                // A project that USES Mustard announces itself to the
                // dashboard's machine-level list on SessionStart. `mustard
                // init` covers new installs; this covers every project that was
                // ALREADY installed, which would otherwise stay invisible
                // forever. Idempotent — an established project writes nothing.
                // An `Observer` (pure side effect, no verdict); opt out with
                // `MUSTARD_DASHBOARD_REGISTER=0`.
                applies_to: &[(Trigger::SessionStart, ToolMatch::Any)],
                check: None,
                observer: Some(Box::new(DashboardRegisterObserver)),
            },
            Module {
                id: "prompt_submit_inject",
                // `followup-cancel-gate` (amendment-window close, a side
                // effect) + declared injectables (`mustard.json#inject`,
                // `on: userPromptSubmit`) + the pipeline-in-flight
                // banner — composed into one `Inject`; never blocks.
                applies_to: &[(Trigger::UserPromptSubmit, ToolMatch::Any)],
                check: Some(Box::new(PromptSubmitInject)),
                observer: None,
            },
            // ── Context-injection optimisation ───────────────────────────────
            Module {
                id: "subagent_inject",
                // For Task dispatches without a declared SKILL, inject a
                // minimal CONTEXT.md + skills slice (resolved via the
                // `skill-resolve`).
                //
                // `SubagentStop` is added so the same module
                // can run the span-level regression eval per returning child
                // (never batched to wave end). The `SubagentStop` branch is fail-open and never
                // emits a blocking verdict — the per-child verdict lands in
                // `_review-spans.md` and the consolidation gate reads
                // the ledger at wave close.
                applies_to: &[
                    (Trigger::PreToolUse, ToolMatch::Named("Task")),
                    (Trigger::PreToolUse, ToolMatch::Named("Agent")),
                    (Trigger::SubagentStop, ToolMatch::Any),
                ],
                check: Some(Box::new(SubagentInject)),
                observer: None,
            },
            // Forgeable-approval gate — on the user's answer to the PLAN
            // approval `AskUserQuestion`, record `<spec>/.approved-by-user` when
            // it is a genuine approval of the active Full spec still awaiting
            // approval in PLAN. `approve-spec` requires that marker in strict
            // mode, so the orchestrator cannot self-approve. The marker is born
            // ONLY from the user's real `tool_response` (which the model does not
            // author). Pure Observer, fail-closed, never blocks.
            Module {
                id: "approval_marker_observer",
                applies_to: &[(Trigger::PostToolUse, ToolMatch::Named("AskUserQuestion"))],
                check: None,
                observer: Some(Box::new(ApprovalMarkerObserver)),
            },
            // Gravador de esclarecimentos — na MESMA pergunta, grava pergunta,
            // resposta escolhida e notas como `clarification` no material da
            // unidade ativa, sem depender de o assistente registrar. Não
            // destrava nada, então texto livre também conta. Sem unidade ativa,
            // nada é gravado. Observer puro, fail-open, nunca bloqueia.
            Module {
                id: "clarification_observer",
                applies_to: &[(Trigger::PostToolUse, ToolMatch::Named("AskUserQuestion"))],
                check: None,
                observer: Some(Box::new(ClarificationObserver)),
            },
            // Plan-mode approval recorder — the primary source of the same
            // `<spec>/.approved-by-user` marker. When the user ACCEPTS the
            // plan-mode plan (`ExitPlanMode` succeeds with the plan payload)
            // for an unapproved Full spec in PLAN, the marker is minted from
            // the harness `tool_response` (which the model does not author).
            // AskUserQuestion above stays as the fallback source. Pure
            // Observer, fail-closed, never blocks.
            Module {
                id: "plan_approval_observer",
                applies_to: &[(Trigger::PostToolUse, ToolMatch::Named("ExitPlanMode"))],
                check: None,
                observer: Some(Box::new(PlanApprovalObserver)),
            },
            // `end_of_turn_check` — a conferência do fim da resposta, o único
            // gancho do `Stop`. No `Stop` da sessão principal, passa o texto
            // final do turno pelas regras (`TurnRule`): as pendências
            // (`pending_gate.rs`) e a clareza (`clarity_check.rs`). O que elas
            // acham sai num bloqueio só; na reescrita que o bloqueio pediu
            // (`stop_hook_active`), a clareza só avisa o usuário. O `hooks.json`
            // dá 30 segundos ao `Stop`.
            Module {
                id: "end_of_turn_check",
                applies_to: &[(Trigger::Stop, ToolMatch::Any)],
                check: Some(Box::new(EndOfTurnCheck)),
                observer: None,
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
            // Picker approval recorder — the THIRD door onto the same
            // `<spec>/.approved-by-user` marker. When the user's OWN submitted
            // prompt is the picker's approve-and-implement form
            // (`/mustard:spec ar`) and a Full spec is awaiting approval in
            // PLAN, the marker is minted from that gesture instead of asking
            // for it a second time through plan mode. `UserPromptSubmit` fires
            // only on a person's submission; the one runtime-authored path onto
            // this trigger (a subagent's report / a background-task notice) is
            // refused by the observer's own first fact. Pure Observer,
            // fail-closed, never blocks the prompt.
            Module {
                id: "picker_approval_observer",
                applies_to: &[(Trigger::UserPromptSubmit, ToolMatch::Any)],
                check: None,
                observer: Some(Box::new(PickerApprovalObserver)),
            },
            // ── Wikilink footer ──────────────────────────────────────────────
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
            // ── session-bound amendment window ───────────────────────────────
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
            // ── auto-abertura por tipo (structural → automatic) ──────────────
            // Both are pure Observers — they emit/restructure as a side effect
            // and are structurally incapable of denying a write: re-wave and
            // wave-advance only restructure the plan, and are never gates.
            Module {
                id: "rewave_observer",
                // On the first EXECUTE write of a not-yet-decomposed
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
                // On SubagentStop, when the active wave's
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
        for want in ["context_budget_gate", "subagent_observer"] {
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
    fn exit_plan_mode_post_tool_use_runs_plan_approval_observer() {
        let registry = Registry::new();
        // The plan-mode approval recorder fires only on PostToolUse(ExitPlanMode).
        assert!(
            applicable_ids(&registry, Trigger::PostToolUse, Some("ExitPlanMode"))
                .contains(&"plan_approval_observer")
        );
        // Never on the Pre side, nor on an unrelated tool.
        assert!(
            !applicable_ids(&registry, Trigger::PreToolUse, Some("ExitPlanMode"))
                .contains(&"plan_approval_observer")
        );
        assert!(
            !applicable_ids(&registry, Trigger::PostToolUse, Some("AskUserQuestion"))
                .contains(&"plan_approval_observer")
        );
    }

    #[test]
    fn ask_user_question_post_tool_use_runs_approval_marker_observer() {
        let registry = Registry::new();
        // The approval recorder fires only on PostToolUse(AskUserQuestion).
        assert!(
            applicable_ids(&registry, Trigger::PostToolUse, Some("AskUserQuestion"))
                .contains(&"approval_marker_observer")
        );
        // Never on the Pre side, nor on an unrelated tool.
        assert!(
            !applicable_ids(&registry, Trigger::PreToolUse, Some("AskUserQuestion"))
                .contains(&"approval_marker_observer")
        );
        assert!(
            !applicable_ids(&registry, Trigger::PostToolUse, Some("Task"))
                .contains(&"approval_marker_observer")
        );
    }

    #[test]
    fn ask_user_question_post_tool_use_runs_clarification_observer() {
        let registry = Registry::new();
        // Ao lado do gravador de aprovação, na mesma pergunta respondida.
        let ids = applicable_ids(&registry, Trigger::PostToolUse, Some("AskUserQuestion"));
        assert!(ids.contains(&"clarification_observer"));
        assert!(ids.contains(&"approval_marker_observer"));
        // Nunca no lado Pre, nem numa ferramenta qualquer.
        assert!(
            !applicable_ids(&registry, Trigger::PreToolUse, Some("AskUserQuestion"))
                .contains(&"clarification_observer")
        );
        assert!(
            !applicable_ids(&registry, Trigger::PostToolUse, Some("Bash"))
                .contains(&"clarification_observer")
        );
        let module = registry.by_id("clarification_observer").expect("registered");
        assert!(module.check.is_none(), "a pure Observer never carries a verdict");
    }

    /// O fim da resposta é uma conferência só: o `end_of_turn_check` é o único
    /// módulo do `Stop`, um `Check` puro, e nunca roda no `Stop` de um
    /// subagente. Os ganchos que derrubavam os outros saíram.
    #[test]
    fn end_of_turn_check_is_the_only_module_on_stop() {
        let registry = Registry::new();
        assert_eq!(applicable_ids(&registry, Trigger::Stop, None), vec!["end_of_turn_check"]);
        let module = registry.by_id("end_of_turn_check").expect("registered");
        assert!(module.check.is_some() && module.observer.is_none());
        assert!(!applicable_ids(&registry, Trigger::SubagentStop, None).contains(&"end_of_turn_check"));
        for gone in [
            "stop_gate",
            "crystallise_nudge",
            "spec_doc_present",
            "session_stop_observer",
            "pending_gate",
            "clarity_check",
        ] {
            assert!(registry.by_id(gone).is_none(), "{gone} left the registry");
        }
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
            "approval_marker_observer",
            "clarification_observer",
            "plan_approval_observer",
            "size_gate",
            "write_gate",
            "boundary_gate",
            "close_gate",
            "scan_gate",
            "active_spec_limit_gate",
            "delegation_advisory",
            "post_edit",
            "spec_hygiene_observer",
            "session_start_inject",
            "session_cleanup_observer",
            "statusline_heal_observer",
            "prompt_submit_inject",
            "user_prompt_observer",
            "amend_window_inject",
            "rewave_observer",
            "wave_start_observer",
            "wave_complete_observer",
            "end_of_turn_check",
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
    fn the_session_families_apply_to_their_events() {
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
        // `statusline_heal_observer` also rides SessionStart.
        assert!(start.contains(&"statusline_heal_observer"));
        // `session_cleanup_observer` on SessionEnd.
        let end = applicable_ids(&registry, Trigger::SessionEnd, None);
        assert!(end.contains(&"session_cleanup_observer"));
        // `prompt_submit_inject` on UserPromptSubmit.
        assert!(applicable_ids(&registry, Trigger::UserPromptSubmit, None)
            .contains(&"prompt_submit_inject"));
        // `user_prompt_observer` also rides UserPromptSubmit.
        assert!(applicable_ids(&registry, Trigger::UserPromptSubmit, None)
            .contains(&"user_prompt_observer"));
    }

    /// O portão de escrita roda no `PreToolUse` das cinco ferramentas de
    /// arquivo, e só nelas, no lugar dos três ganchos que ele juntou.
    #[test]
    fn the_write_gate_runs_on_the_five_file_tools() {
        let registry = Registry::new();
        for tool in ["Read", "Write", "Edit", "MultiEdit", "NotebookEdit"] {
            assert!(
                applicable_ids(&registry, Trigger::PreToolUse, Some(tool)).contains(&"write_gate"),
                "write_gate missing on {tool}"
            );
            assert!(
                !applicable_ids(&registry, Trigger::PostToolUse, Some(tool)).contains(&"write_gate"),
                "write_gate never runs after {tool}"
            );
        }
        for tool in ["Bash", "Task", "Agent", "Skill"] {
            assert!(
                !applicable_ids(&registry, Trigger::PreToolUse, Some(tool)).contains(&"write_gate"),
                "write_gate is not a gate of {tool}"
            );
        }
        let module = registry.by_id("write_gate").expect("registered");
        assert!(module.check.is_some() && module.observer.is_none());
        for gone in ["secret_files", "work_branch_gate", "scope_guard"] {
            assert!(registry.by_id(gone).is_none(), "{gone} left the registry");
        }
    }

    #[test]
    fn write_edit_family_applies_on_pre_tool_use() {
        let registry = Registry::new();
        // The Write/Edit gates fire on PreToolUse(Write) and (Edit).
        for tool in ["Write", "Edit"] {
            let ids = applicable_ids(&registry, Trigger::PreToolUse, Some(tool));
            for want in ["size_gate", "write_gate", "boundary_gate", "close_gate"] {
                assert!(ids.contains(&want), "missing {want} for {tool}");
            }
        }
        // `boundary_gate` stays Write/Edit-only: it never inspects Read.
        let read_ids = applicable_ids(&registry, Trigger::PreToolUse, Some("Read"));
        assert!(!read_ids.contains(&"boundary_gate"));
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
