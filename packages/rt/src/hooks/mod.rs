//! Enforcement modules — one module per concern, behind the `mustard-core`
//! `Check` / `Observer` contract.
//!
//! Each module consolidates a *family* of the old JavaScript hooks (b3 spec §
//! Arquitetura): porting 1:1 would preserve the fragmentation the migration
//! exists to remove.
//!
//! - Waves 1-2: [`bash_guard`] — the Bash-tool family.
//! - Wave 3: the Task / Subagent family — [`budget`] (prompt/return size),
//!   [`model_routing`] (model-selection gate), [`tracker`] (tool-use /
//!   main-context counters + agent/tool telemetry), [`skills_audit`]
//!   (recommended-skills count advisory).
//! - Wave 4: the Write/Edit family — [`size_gate`] (spec/skill size + skill
//!   validation), [`path_guard`] (sensitive-file + boundary gates),
//!   [`post_edit`] (auto-format / checklist-auto-mark / guard-verify /
//!   pipeline-phase), [`close_gate`] (the pipeline-CLOSE sensor), and
//!   [`enforce_registry`] (the entity-registry pre-pipeline gate).

pub mod bash_guard;
pub mod budget;
pub mod close_gate;
pub mod enforce_registry;
pub mod model_routing;
pub mod path_guard;
pub mod post_edit;
pub mod size_gate;
pub mod skills_audit;
pub mod tracker;
