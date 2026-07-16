//! Canonical YAML frontmatter schema for SKILL.md.
//!
//! ## What
//!
//! Every Mustard skill — both the foundation skills shipped under
//! `apps/cli/templates/skills/` and the scan-generated ones under
//! `{subproject}/.claude/skills/` — exposes a YAML frontmatter block. Before
//! Wave 1 of `2026-05-25-mustard-deep-refactor` the shape was implicit
//! (`name`, `description`, `source`). The new contract adds four fields
//! consumed by `skill-resolve` to score relevance deterministically:
//!
//! - `tags` — verbs the skill applies to (`add`, `fix`, `refactor`, ...).
//! - `appliesTo` — cluster labels the skill targets (empty = any).
//! - `scope` — pipeline scopes the skill is callable in.
//! - `entities` — optional list of registry entities the skill talks about.
//! - `metadata.generated_by` — `scan` or `foundation`.
//!
//! ## Design (lenient, fail-open)
//!
//! - Parsing is **lenient**: unknown frontmatter keys land in `extra`
//!   ([`serde(flatten)`]); unknown tag / scope tokens are dropped with a
//!   `Vec<String>` of soft warnings. A SKILL written for a future Mustard does
//!   not break older consumers.
//! - The YAML parser is a small, hand-rolled subset — enough for the
//!   key/value + flow-list / block-list shapes Mustard produces. Pulling in
//!   `serde_yaml` would bloat the workspace; we do not need full YAML.
//! - [`validate`] takes a `strict: bool`. The default (`false`) checks only
//!   the legacy invariants (`name` kebab-case, `description` non-empty); the
//!   strict pass (used by `--strict-frontmatter`) also requires
//!   `tags` / `applies_to` / `scope` / `metadata.generated_by`.

use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/// Verbs a skill applies to. The list is the union of every tag the
/// foundation + scan-generated skills carry today; new tags can be added
/// without breaking older deserialisers (unknown values are dropped).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SkillTag {
    /// Adding a new entity / endpoint / component.
    Add,
    /// Fixing a bug.
    Fix,
    /// Refactoring existing code without behaviour change.
    Refactor,
    /// Reviewing diffs or code under another agent.
    Review,
    /// Planning (Spec drafting / wave decomposition).
    Plan,
    /// Diagnosing an error / regression.
    Diagnose,
    /// Designing UI / craft work.
    Design,
    /// Documentation writing.
    Docs,
    /// Architectural deepening.
    Architecture,
    /// Testing / QA.
    Test,
    /// Performance work.
    Performance,
    /// Generic "any code work" (catch-all for foundation skills like karpathy).
    Any,
}

impl SkillTag {
    /// Lower-kebab spelling — round-trips through `serde`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Fix => "fix",
            Self::Refactor => "refactor",
            Self::Review => "review",
            Self::Plan => "plan",
            Self::Diagnose => "diagnose",
            Self::Design => "design",
            Self::Docs => "docs",
            Self::Architecture => "architecture",
            Self::Test => "test",
            Self::Performance => "performance",
            Self::Any => "any",
        }
    }

    /// Parse a free-form tag token, accepting common synonyms. Returns `None`
    /// for unknown values so the caller can drop them as soft warnings.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "add" | "create" | "new" | "feature" => Some(Self::Add),
            "fix" | "bugfix" | "patch" => Some(Self::Fix),
            "refactor" | "refactoring" => Some(Self::Refactor),
            "review" | "audit" => Some(Self::Review),
            "plan" | "planning" | "spec" => Some(Self::Plan),
            "diagnose" | "debug" | "diagnose-bug" => Some(Self::Diagnose),
            "design" | "ui" | "craft" => Some(Self::Design),
            "docs" | "documentation" | "doc" => Some(Self::Docs),
            "architecture" | "deepening" => Some(Self::Architecture),
            "test" | "testing" | "qa" => Some(Self::Test),
            "performance" | "perf" => Some(Self::Performance),
            "any" | "*" => Some(Self::Any),
            _ => None,
        }
    }
}

impl fmt::Display for SkillTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Pipeline scope a skill applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SkillScope {
    /// Any code-editing phase (EXECUTE).
    CodeEditing,
    /// REVIEW phase.
    Review,
    /// PLAN phase.
    Plan,
    /// ANALYZE phase.
    Analyze,
    /// QA phase.
    Qa,
}

impl SkillScope {
    /// Lower-kebab spelling — round-trips through `serde`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CodeEditing => "code-editing",
            Self::Review => "review",
            Self::Plan => "plan",
            Self::Analyze => "analyze",
            Self::Qa => "qa",
        }
    }

    /// Parse a free-form scope token.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "code-editing" | "code_editing" | "execute" | "edit" => Some(Self::CodeEditing),
            "review" => Some(Self::Review),
            "plan" | "planning" => Some(Self::Plan),
            "analyze" | "explore" => Some(Self::Analyze),
            "qa" => Some(Self::Qa),
            _ => None,
        }
    }
}

/// Where a skill came from — informational, drives the validator's strict
/// pass (foundation skills must self-declare).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillSource {
    /// Scan-generated skill (lives under `{subproject}/.claude/skills/`).
    Scan,
    /// Hand-authored foundation skill shipped with the CLI templates.
    Foundation,
    /// Legacy fallback — frontmatter omitted `metadata.generated_by` and
    /// `source` field. Strict validation rejects this.
    Manual,
}

impl SkillSource {
    /// Lowercase spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Scan => "scan",
            Self::Foundation => "foundation",
            Self::Manual => "manual",
        }
    }

    /// Parse a free-form source token.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "scan" => Some(Self::Scan),
            "foundation" => Some(Self::Foundation),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Metadata + main schema
// ---------------------------------------------------------------------------

/// Cluster label attached by the scan generator (`{subproject}/.claude/skills/`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClusterMeta {
    /// Cluster label (kebab-case).
    #[serde(default)]
    pub label: String,
}

/// `metadata:` block of a SKILL frontmatter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillMetadata {
    /// `scan` | `foundation`. `None` ⇒ legacy SKILL (validator surfaces it
    /// under `--strict`).
    #[serde(default, rename = "generated_by")]
    pub generated_by: Option<SkillSource>,
    /// Optional cluster metadata for scan-generated skills.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cluster: Option<ClusterMeta>,
}

/// Canonical SKILL.md frontmatter shape.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillFrontmatter {
    /// Skill name — kebab-case, must match the directory name.
    #[serde(default)]
    pub name: String,
    /// Human-readable description (used by Claude Code for auto-loading).
    #[serde(default)]
    pub description: String,
    /// Verbs the skill applies to.
    #[serde(default)]
    pub tags: Vec<SkillTag>,
    /// Cluster labels the skill targets. Empty = any cluster.
    #[serde(default, rename = "appliesTo")]
    pub applies_to: Vec<String>,
    /// Pipeline scopes the skill is callable in.
    #[serde(default)]
    pub scope: Vec<SkillScope>,
    /// Optional entity names from the registry the skill talks about.
    #[serde(default)]
    pub entities: Vec<String>,
    /// Metadata block — declares scan vs foundation provenance.
    #[serde(default)]
    pub metadata: SkillMetadata,
    /// Lenient catch-all for any other frontmatter key (`source`, `license`,
    /// `disable-model-invocation`, `version`, ...). Preserved on round-trip.
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Parse / validation error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SkillFrontmatterError {
    /// Frontmatter block not found (no leading `---` fences).
    #[error("missing YAML frontmatter")]
    MissingFrontmatter,
    /// Required field missing.
    #[error("missing field: {0}")]
    MissingField(String),
    /// `name` is not kebab-case.
    #[error("name not kebab-case: {0}")]
    NameNotKebab(String),
    /// `description` shorter than the minimum (Claude Code rejects <50 chars).
    #[error("description too short ({0} chars, min 50)")]
    DescriptionTooShort(usize),
    /// `description` longer than the recommended maximum. Mustard tolerates
    /// up to 1500 chars to accommodate richer foundation skills (e.g. `hallmark`
    /// at ~1150). Claude Code recommends ≤1024 but does not enforce it.
    #[error("description too long ({0} chars, max 1500)")]
    DescriptionTooLong(usize),
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse a SKILL.md raw text (or a raw frontmatter YAML body) into a
/// [`SkillFrontmatter`]. Lenient: unknown keys land in `extra`; unknown
/// tag / scope tokens are dropped silently.
///
/// # Errors
///
/// Returns [`SkillFrontmatterError::MissingFrontmatter`] when no `---` /
/// `---` fence pair can be located.
pub fn parse(raw: &str) -> Result<SkillFrontmatter, SkillFrontmatterError> {
    let yaml = extract_frontmatter(raw).ok_or(SkillFrontmatterError::MissingFrontmatter)?;
    Ok(parse_yaml(&yaml))
}

/// Validate a frontmatter shape.
///
/// In `strict=false` mode, the only invariants are the legacy ones:
/// `name` kebab-case + present, `description` length within `50..=1024`.
///
/// In `strict=true` mode, [`SkillFrontmatter::tags`],
/// [`SkillFrontmatter::applies_to`], [`SkillFrontmatter::scope`] and
/// [`SkillMetadata::generated_by`] must all be non-empty / non-`None`. This
/// is what `mustard-rt run skills validate --strict-frontmatter` enforces.
///
/// # Errors
///
/// Returns the collected [`SkillFrontmatterError`] variants. The returned
/// vector is non-empty.
pub fn validate(
    fm: &SkillFrontmatter,
    strict: bool,
) -> Result<(), Vec<SkillFrontmatterError>> {
    let mut errors: Vec<SkillFrontmatterError> = Vec::new();
    if fm.name.trim().is_empty() {
        errors.push(SkillFrontmatterError::MissingField("name".into()));
    } else if !is_kebab(&fm.name) {
        errors.push(SkillFrontmatterError::NameNotKebab(fm.name.clone()));
    }
    let desc_chars = fm.description.chars().count();
    if desc_chars == 0 {
        errors.push(SkillFrontmatterError::MissingField("description".into()));
    } else if desc_chars < 50 {
        errors.push(SkillFrontmatterError::DescriptionTooShort(desc_chars));
    } else if desc_chars > 1500 {
        // 1500 is Mustard's tolerance — Claude Code recommends ≤1024 but does
        // not enforce. `hallmark` foundation skill ships at ~1150 chars.
        errors.push(SkillFrontmatterError::DescriptionTooLong(desc_chars));
    }
    if strict {
        if fm.tags.is_empty() {
            errors.push(SkillFrontmatterError::MissingField("tags".into()));
        }
        // `applies_to` may be empty (= any cluster), but the *key* must be
        // declared. The lenient parser sets it to `[]` whether the key is
        // present or absent, so we cannot distinguish from the parsed value
        // alone — instead, strict mode requires the source raw to contain
        // the key. That check lives in `validate_strict_keys`.
        if fm.scope.is_empty() {
            errors.push(SkillFrontmatterError::MissingField("scope".into()));
        }
        if fm.metadata.generated_by.is_none() {
            errors.push(SkillFrontmatterError::MissingField(
                "metadata.generated_by".into(),
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Extract the YAML body between leading `---\n` and the next `\n---` fence.
/// Tolerates CRLF.
#[must_use]
pub fn extract_frontmatter(raw: &str) -> Option<String> {
    let normalized = raw.replace("\r\n", "\n");
    let rest = normalized.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    Some(rest[..end].to_string())
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// kebab-case check (`[a-z][a-z0-9-]+`). Canonical home — `apps/rt`'s
/// `skills::validate_skill` calls this directly instead of keeping a local copy.
///
/// Uses `chars().count()` for the length guard so a single multi-byte unicode
/// character (e.g. `"é"`, 2 bytes but 1 char) does not falsely pass as a
/// two-character name.
pub(crate) fn is_kebab(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && s.chars().count() >= 2
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Minimal YAML subset parser: top-level scalars, flow lists (`[a, b]`),
/// block lists (`- item` lines), quoted strings, and a single nested block
/// (`metadata:`). Anything more exotic lands in `extra` as untyped JSON.
fn parse_yaml(yaml: &str) -> SkillFrontmatter {
    let lines: Vec<&str> = yaml.lines().collect();
    let mut fm = SkillFrontmatter::default();
    let mut extra = serde_json::Map::<String, serde_json::Value>::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        // Skip blanks + comments.
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            i += 1;
            continue;
        }
        // Top-level only (column 0). A nested line (indented) without a
        // recognised parent has already been consumed below or is part of
        // an unknown block we skip.
        if line.starts_with([' ', '\t']) {
            i += 1;
            continue;
        }
        let (key, value) = match line.split_once(':') {
            Some((k, v)) => (k.trim().to_string(), v.trim_start().to_string()),
            None => {
                i += 1;
                continue;
            }
        };
        match key.as_str() {
            "name" => fm.name = unquote(value.trim()).to_string(),
            "description" => {
                // Support multi-line continuations (indented lines beneath).
                let mut acc = unquote(value.trim()).to_string();
                let mut j = i + 1;
                while j < lines.len() && lines[j].starts_with([' ', '\t']) {
                    let cont = lines[j].trim();
                    if !cont.is_empty() {
                        acc.push(' ');
                        acc.push_str(cont);
                    }
                    j += 1;
                }
                fm.description = acc;
                i = j;
                continue;
            }
            "tags" => {
                let (items, consumed) = read_list(&lines, i, &value);
                fm.tags = items.iter().filter_map(|t| SkillTag::parse(t)).collect();
                i += consumed;
                continue;
            }
            "appliesTo" | "applies_to" => {
                let (items, consumed) = read_list(&lines, i, &value);
                fm.applies_to = items;
                i += consumed;
                continue;
            }
            "scope" => {
                let (items, consumed) = read_list(&lines, i, &value);
                fm.scope = items.iter().filter_map(|s| SkillScope::parse(s)).collect();
                i += consumed;
                continue;
            }
            "entities" => {
                let (items, consumed) = read_list(&lines, i, &value);
                fm.entities = items;
                i += consumed;
                continue;
            }
            "metadata" => {
                // Read the indented block beneath `metadata:`.
                let (meta, consumed) = read_metadata_block(&lines, i, &value);
                fm.metadata = meta;
                i += consumed;
                continue;
            }
            other => {
                if !value.is_empty() {
                    extra.insert(other.to_string(), serde_json::Value::String(unquote(&value).to_string()));
                }
            }
        }
        i += 1;
    }
    fm.extra = serde_json::Value::Object(extra);
    fm
}

/// Parse a list value — either flow form `[a, b, c]` on the same line, or
/// a block list opening on the next indented lines (`- item`).
/// Returns `(items, lines_consumed)` where `lines_consumed` includes the
/// header line itself (≥1).
fn read_list(lines: &[&str], start: usize, header_value: &str) -> (Vec<String>, usize) {
    let header = header_value.trim();
    if header.starts_with('[') && header.ends_with(']') {
        // Flow list — single line.
        let inner = &header[1..header.len() - 1];
        let items = inner
            .split(',')
            .map(|s| unquote(s.trim()).to_string())
            .filter(|s| !s.is_empty())
            .collect();
        return (items, 1);
    }
    // Block list — header line plus indented `- item` siblings.
    let mut items: Vec<String> = Vec::new();
    let mut consumed = 1; // the header line
    let mut j = start + 1;
    while j < lines.len() {
        let line = lines[j];
        if !line.starts_with([' ', '\t']) {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            j += 1;
            consumed += 1;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("- ") {
            items.push(unquote(rest.trim()).to_string());
        }
        j += 1;
        consumed += 1;
    }
    (items, consumed)
}

/// Read the `metadata:` nested block. Recognises `generated_by` and a
/// `cluster:` sub-block carrying `label`.
fn read_metadata_block(
    lines: &[&str],
    start: usize,
    _header_value: &str,
) -> (SkillMetadata, usize) {
    let mut meta = SkillMetadata::default();
    let mut consumed = 1;
    let mut j = start + 1;
    while j < lines.len() {
        let line = lines[j];
        if !line.starts_with([' ', '\t']) {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            j += 1;
            consumed += 1;
            continue;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            match k.trim() {
                "generated_by" => {
                    meta.generated_by = SkillSource::parse(unquote(v.trim()));
                }
                "cluster" => {
                    // Nested `cluster:` block — read `label:` line beneath.
                    let mut k2 = j + 1;
                    let mut cluster = ClusterMeta::default();
                    while k2 < lines.len() {
                        let l = lines[k2];
                        let l_trim = l.trim();
                        // Stop once indentation drops back to the metadata level
                        // (two spaces) — heuristically: a line that does not
                        // start with at least 4 spaces.
                        if !l.starts_with("    ") && !l.starts_with("\t\t") {
                            break;
                        }
                        if let Some((kk, vv)) = l_trim.split_once(':') {
                            if kk.trim() == "label" {
                                cluster.label = unquote(vv.trim()).to_string();
                            }
                        }
                        k2 += 1;
                    }
                    meta.cluster = Some(cluster);
                    consumed += k2 - (j + 1);
                    j = k2;
                    continue;
                }
                _ => {}
            }
        }
        j += 1;
        consumed += 1;
    }
    (meta, consumed)
}

/// Strip a single layer of `"..."` or `'...'` quotes.
fn unquote(s: &str) -> &str {
    let trimmed = s.trim();
    if (trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2)
        || (trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2)
    {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_legacy_frontmatter() {
        let raw = "---\nname: foo\ndescription: A long enough description to clear the fifty character min.\nsource: manual\n---\nbody";
        let fm = parse(raw).expect("parses");
        assert_eq!(fm.name, "foo");
        assert!(fm.description.contains("clear the fifty"));
    }

    #[test]
    fn parses_canonical_t1_3_schema() {
        let raw = r"---
name: my-skill
description: Use when the user wants to do something that needs at least fifty characters.
tags: [add, fix]
appliesTo: [user-entity]
scope: [code-editing, review]
entities: [User]
metadata:
  generated_by: foundation
---
body
";
        let fm = parse(raw).expect("parses");
        assert_eq!(fm.tags, vec![SkillTag::Add, SkillTag::Fix]);
        assert_eq!(fm.applies_to, vec!["user-entity"]);
        assert_eq!(
            fm.scope,
            vec![SkillScope::CodeEditing, SkillScope::Review]
        );
        assert_eq!(fm.entities, vec!["User"]);
        assert_eq!(fm.metadata.generated_by, Some(SkillSource::Foundation));
    }

    #[test]
    fn parses_block_list_form() {
        let raw = "---\nname: x\ndescription: Use when the user wants to do something that needs at least fifty characters.\ntags:\n  - add\n  - refactor\nscope:\n  - code-editing\nmetadata:\n  generated_by: foundation\n---\n";
        let fm = parse(raw).expect("parses");
        assert_eq!(fm.tags, vec![SkillTag::Add, SkillTag::Refactor]);
        assert_eq!(fm.scope, vec![SkillScope::CodeEditing]);
    }

    #[test]
    fn missing_frontmatter_errors() {
        assert!(matches!(
            parse("just a body").unwrap_err(),
            SkillFrontmatterError::MissingFrontmatter
        ));
    }

    #[test]
    fn lenient_validate_passes_on_legacy_shape() {
        let raw = "---\nname: foo\ndescription: Use when the user wants to do something useful that fills at least fifty characters here.\nsource: manual\n---\n";
        let fm = parse(raw).unwrap();
        assert!(validate(&fm, false).is_ok());
    }

    #[test]
    fn strict_validate_rejects_legacy() {
        let raw = "---\nname: foo\ndescription: Use when the user wants to do something useful that fills at least fifty characters here.\nsource: manual\n---\n";
        let fm = parse(raw).unwrap();
        let err = validate(&fm, true).unwrap_err();
        // Strict requires tags + scope + metadata.generated_by — none are
        // present in the legacy shape.
        assert!(err
            .iter()
            .any(|e| matches!(e, SkillFrontmatterError::MissingField(k) if k == "tags")));
        assert!(err
            .iter()
            .any(|e| matches!(e, SkillFrontmatterError::MissingField(k) if k == "scope")));
        assert!(err.iter().any(|e| matches!(e,
            SkillFrontmatterError::MissingField(k) if k == "metadata.generated_by")));
    }

    #[test]
    fn strict_validate_accepts_canonical_schema() {
        let raw = r"---
name: good-skill
description: Use when the user wants to do something useful that fills at least fifty characters here.
tags: [add]
appliesTo: []
scope: [code-editing]
metadata:
  generated_by: foundation
---
";
        let fm = parse(raw).unwrap();
        assert!(validate(&fm, true).is_ok());
    }

    #[test]
    fn skill_tag_parses_synonyms() {
        assert_eq!(SkillTag::parse("bugfix"), Some(SkillTag::Fix));
        assert_eq!(SkillTag::parse("CREATE"), Some(SkillTag::Add));
        assert_eq!(SkillTag::parse("unknown-token"), None);
    }

    #[test]
    fn skill_scope_parses_synonyms() {
        assert_eq!(SkillScope::parse("execute"), Some(SkillScope::CodeEditing));
        assert_eq!(SkillScope::parse("planning"), Some(SkillScope::Plan));
        assert_eq!(SkillScope::parse("zzz"), None);
    }

    #[test]
    fn extra_preserves_unknown_keys() {
        let raw = "---\nname: x\ndescription: Use when the user wants something with enough characters to clear the limit.\nlicense: MIT\nversion: 1.0.0\n---\n";
        let fm = parse(raw).unwrap();
        let obj = fm.extra.as_object().expect("object");
        assert_eq!(obj.get("license").and_then(|v| v.as_str()), Some("MIT"));
        assert_eq!(obj.get("version").and_then(|v| v.as_str()), Some("1.0.0"));
    }

    #[test]
    fn is_kebab_check() {
        assert!(is_kebab("my-skill"));
        assert!(!is_kebab("MySkill"));
        assert!(!is_kebab("my_skill"));
    }

    #[test]
    fn is_kebab_unicode_char_count() {
        // "é" is 2 bytes but 1 char — must NOT pass the length guard.
        assert!(!is_kebab("é"), "single multi-byte char must fail");
        // "ab" is 2 chars (ASCII) — must pass.
        assert!(is_kebab("ab"), "two ASCII chars must pass");
    }
}
