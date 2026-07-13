//! `skill` — canonical schema for skill frontmatter (Wave 1 of
//! `2026-05-25-mustard-deep-refactor`).
//!
//! Owns the [`frontmatter::SkillFrontmatter`] type + parse/validate helpers
//! consumed by `mustard-rt run skill-resolve`, `mustard-rt run skills validate
//! --strict-frontmatter`, and the agent-prompt skill-injection layer.

pub mod frontmatter;

pub use frontmatter::{
    extract_frontmatter, parse, validate, ClusterMeta, SkillFrontmatter, SkillFrontmatterError,
    SkillMetadata, SkillScope, SkillSource, SkillTag,
};
