//! Enforcement modules — one module per concern, behind the `mustard-core`
//! `Check` / `Observer` contract.
//!
//! Each module consolidates a *family* of the old JavaScript hooks (b3 spec §
//! Arquitetura): porting 1:1 would preserve the fragmentation the migration
//! exists to remove.
//!
//! - Waves 1-2: [`bash_guard`] — the Bash-tool family.
//! - Wave 3: the Task / Subagent family — `context_budget_gate` (prompt/return
//!   size), `model_routing_gate` (model-selection gate), the tool-use /
//!   main-context counters (`tool_use_counter`, `main_context_counter`) plus
//!   the agent/tool/skill observers (`subagent_observer`, `metrics_observer`,
//!   `skill_usage_observer`), and `skills_advisory` (recommended-skills count
//!   advisory).
//! - Wave 4: the Write/Edit family — [`size_gate`] (spec/skill size + skill
//!   validation), [`path_guard`] (sensitive-file + boundary gates),
//!   [`post_edit`] (auto-format / checklist-auto-mark / guard-verify /
//!   pipeline-phase), [`close_gate`] (the pipeline-CLOSE sensor), and
//!   [`entity_registry_gate`] (the entity-registry pre-pipeline gate).
//! - Wave 5: the session-lifecycle families — `session_start_inject`
//!   (harness-init / session-memory / spec-hygiene), `session_knowledge_observer`
//!   (session-knowledge / -inc / memory-auto-extract), `session_cleanup_observer`
//!   (`SessionEnd` cleanup), `pre_compact_inject` (the `PreCompact` snapshot),
//!   `prompt_submit_inject` (the `UserPromptSubmit` follow-up archival gate),
//!   and `spec_hygiene_observer` (the gated SessionStart auto-close).

pub mod observe;
pub mod session;
pub mod write;
pub mod task;
pub mod bash;
// Spec A v4 / W4 — run-based alternative to Moment 1 of the regression gate.
