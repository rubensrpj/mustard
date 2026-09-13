//! Enforcement modules — one module per concern, behind the `mustard-core`
//! `Check` / `Observer` contract.
//!
//! Each module consolidates a *family* of the old JavaScript hooks: porting
//! 1:1 would preserve the fragmentation the migration exists to remove.
//!
//! - `bash_command_gate` — the Bash-tool family.
//! - The Task / Subagent family — `context_budget_gate` (prompt/return
//!   size), the tool-use /
//!   main-context counters (`tool_use_counter`, `main_context_counter`) plus
//!   the agent/tool/skill observers (`subagent_observer`, `metrics_observer`,
//!   `skill_usage_observer`).
//! - The Write/Edit family — [`size_gate`] (spec/skill size + skill
//!   validation), `boundary_gate` (the spec-boundary gate),
//!   [`post_edit`] (auto-format / checklist-auto-mark / guard-verify /
//!   pipeline-phase), [`close_gate`] (the pipeline-CLOSE sensor), and
//!   [`scan_gate`] (the pre-pipeline scan gate — blocks until grain.model.json).
//! - The session-lifecycle families — `session_start_inject`
//!   (harness-init / terrain census / spec-hygiene), `session_cleanup_observer`
//!   (`SessionEnd` cleanup), `prompt_submit_inject` (the `UserPromptSubmit` follow-up archival gate),
//!   and `spec_hygiene_observer` (the gated SessionStart auto-close).

pub mod observe;
pub mod session;
pub mod write;
pub mod task;
pub mod bash;
pub mod worktree_create;
// Spec A v4 / W4 — run-based alternative to Moment 1 of the regression gate.
