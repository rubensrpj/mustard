//! Compatibility façade for the agent dispatch-prompt renderer.
//!
//! The renderer was split into cohesive sub-engines under [`super::render`]
//! (`prompt_ref`, `role`, `sections`, `retry`, `capabilities`, `skills`,
//! `reference`, and the `render` compositor). This module re-exports that
//! engine's public surface so the historical
//! `crate::commands::agent::agent_prompt_render::X` import paths — used by
//! `cli.rs`, `wave_advance`, `dispatch_plan`, `wave_scaffold` and the
//! `subagent_inject` hook — keep resolving unchanged. No behaviour lives here.

pub use super::render::{
    recommended_subagent_type, run, EmitMode, RenderMode, EPISTEMIC_FLOOR, PROMPT_REF_MARKER,
};
pub(crate) use super::render::render_prompt_ref_at;
// `read_task_steps` / `files_section_paths` are reached through this façade only
// by in-crate test code (`wave_scaffold` tests); the bin build never references
// them, so gate the re-export on `cfg(test)` to keep that build warning-free.
#[cfg(test)]
pub(crate) use super::render::{files_section_paths, read_task_steps};
