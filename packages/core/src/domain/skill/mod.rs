//! `skill` — canonical schema for skill frontmatter.
//!
//! Owns the [`frontmatter::SkillFrontmatter`] type + parse/validate helpers.
//! The parser is what the binary reads a skill through: the skill search that
//! pairs a task with a skill, the wave prompt and the agent's skill list (both
//! show the skill's description), and the work-branch census (which tells a
//! scan-written skill apart by its `source`).

pub mod frontmatter;

pub use frontmatter::{
    extract_frontmatter, parse, validate, ClusterMeta, SkillFrontmatter, SkillFrontmatterError,
    SkillMetadata, SkillScope, SkillSource, SkillTag,
};
