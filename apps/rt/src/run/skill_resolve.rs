//! `mustard-rt run skill-resolve` — deterministic skill matching for the
//! agent dispatch context (Wave 1 of `2026-05-25-mustard-deep-refactor`).
//!
//! ## What
//!
//! Given a free-text intent + subproject + pipeline phase, score every
//! discoverable SKILL.md (foundation + scan-generated) and emit the top-K
//! matches as JSON. The `agent-prompt-render` subcommand calls into
//! [`resolve`] in-process to fill the `{recommended_skills}` placeholder;
//! this CLI face is the same logic exposed for ad-hoc inspection and AC
//! testing.
//!
//! ## Score
//!
//! `score = tag_match + applies_match + entity_match + scope_match`
//!
//! - `tag_match`     — +1 for every verb in the intent that matches a tag.
//! - `applies_match` — +1 for every cluster label in the spec's subproject
//!   that the skill targets (empty `appliesTo` = matches all subprojects
//!   at weight 0.25).
//! - `entity_match`  — +1 for every entity name from the intent that is
//!   listed in the skill's `entities` array.
//! - `scope_match`   — +1 when the active `Phase` ∈ `scope`.
//!
//! ## Zero LLM, fail-open
//!
//! Pure Rust: tokenises the intent (alphanumeric runs, lowercased), reads
//! every SKILL.md frontmatter via [`mustard_core::skill::frontmatter::parse`],
//! and walks the skills directories. Missing registry / unparseable
//! frontmatter degrade gracefully — they are skipped, not fatal.

use crate::run::env::project_dir;
use mustard_core::fs as mfs;
use mustard_core::skill::frontmatter::{parse as parse_fm, SkillFrontmatter, SkillScope, SkillTag};
use mustard_core::ClaudePaths;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Options for `mustard-rt run skill-resolve`.
pub struct SkillResolveOpts {
    /// Free-text intent (verb + nouns) — required.
    pub intent: String,
    /// Subproject path relative to the project root (`apps/dashboard`).
    pub subproject: Option<String>,
    /// `ANALYZE` / `EXECUTE` / `REVIEW` / ... — case-insensitive.
    pub phase: Option<String>,
    /// Output JSON instead of the table form.
    pub json: bool,
    /// Top-K cap (default 5).
    pub top_k: usize,
}

/// One scored skill returned to the caller.
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedSkill {
    pub name: String,
    pub score: f64,
    pub reasons: Vec<String>,
    pub path: String,
}

/// CLI entry point.
pub fn run(opts: SkillResolveOpts) {
    let project = PathBuf::from(project_dir());
    let resolved = resolve(
        &project,
        &opts.intent,
        opts.subproject.as_deref(),
        opts.phase.as_deref(),
        opts.top_k.max(1),
    );
    if opts.json {
        let body = serde_json::json!({ "skills": resolved });
        println!("{}", serde_json::to_string_pretty(&body).unwrap_or_else(|_| "{}".into()));
    } else {
        if resolved.is_empty() {
            println!("skill-resolve: no skills matched.");
            return;
        }
        for s in &resolved {
            println!("  {:.2}  {}  ({})", s.score, s.name, s.reasons.join(", "));
        }
    }
}

/// Core resolver — pure, no IO beyond reading SKILL.md files.
#[must_use]
pub fn resolve(
    project: &Path,
    intent: &str,
    subproject: Option<&str>,
    phase: Option<&str>,
    top_k: usize,
) -> Vec<ResolvedSkill> {
    let tokens = tokenise(intent);
    let entity_names = entity_names_for_intent(project, &tokens);
    let cluster_labels = cluster_labels_for_subproject(project, subproject);
    let active_scope = phase
        .and_then(SkillScope::parse)
        .or(Some(SkillScope::CodeEditing));

    let candidates = discover_skills(project, subproject);
    let mut scored: Vec<ResolvedSkill> = Vec::new();
    for (path, fm) in candidates {
        let (score, reasons) = score_skill(&fm, &tokens, &entity_names, &cluster_labels, active_scope);
        if score <= 0.0 {
            continue;
        }
        scored.push(ResolvedSkill {
            name: fm.name,
            score,
            reasons,
            path: path.to_string_lossy().to_string(),
        });
    }
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.cmp(&b.name))
    });
    scored.truncate(top_k);
    scored
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// Discover every SKILL.md across foundation + subproject roots.
fn discover_skills(project: &Path, subproject: Option<&str>) -> Vec<(PathBuf, SkillFrontmatter)> {
    let mut found: Vec<(PathBuf, SkillFrontmatter)> = Vec::new();
    let mut roots: Vec<PathBuf> = Vec::new();

    // 1. Foundation skills shipped with the CLI templates.
    roots.push(project.join("apps").join("cli").join("templates").join("skills"));
    // Also the installed location (`.claude/skills/` at the repo root).
    if let Ok(paths) = ClaudePaths::for_project(project) {
        roots.push(paths.skills_dir());
    }
    // 2. Subproject-specific skills.
    if let Some(sub) = subproject {
        if let Ok(sub_paths) = ClaudePaths::for_project(project.join(sub)) {
            roots.push(sub_paths.skills_dir());
        }
    }

    let mut seen: BTreeSet<String> = BTreeSet::new();
    for root in roots {
        let Ok(entries) = mfs::read_dir(&root) else {
            continue;
        };
        for entry in entries {
            if !entry.is_dir {
                continue;
            }
            let candidate = entry.path.join("SKILL.md");
            if !candidate.exists() {
                continue;
            }
            let Ok(text) = mfs::read_to_string(&candidate) else {
                continue;
            };
            let Ok(fm) = parse_fm(&text) else {
                continue;
            };
            if fm.name.is_empty() || !seen.insert(fm.name.clone()) {
                continue;
            }
            found.push((candidate, fm));
        }
    }
    found
}

/// Tokenise an intent — lowercase alphanumeric runs of length ≥3.
fn tokenise(intent: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut buf = String::new();
    for c in intent.chars() {
        if c.is_ascii_alphanumeric() {
            buf.push(c.to_ascii_lowercase());
        } else if !buf.is_empty() {
            if buf.len() >= 3 {
                out.push(buf.clone());
            }
            buf.clear();
        }
    }
    if !buf.is_empty() && buf.len() >= 3 {
        out.push(buf);
    }
    out
}

/// Cross intent tokens against `entity-registry.json` to surface entity
/// hits the skill might target.
fn entity_names_for_intent(project: &Path, tokens: &[String]) -> Vec<String> {
    let Ok(paths) = ClaudePaths::for_project(project) else {
        return Vec::new();
    };
    let registry_path = paths.entity_registry_json_path();
    let Ok(text) = mfs::read_to_string(&registry_path) else {
        return Vec::new();
    };
    let Ok(value): Result<Value, _> = serde_json::from_str(&text) else {
        return Vec::new();
    };
    let Some(obj) = value.as_object() else {
        return Vec::new();
    };
    let mut hits: Vec<String> = Vec::new();
    for (key, _) in obj {
        if key.starts_with('_') {
            continue;
        }
        let lower = key.to_ascii_lowercase();
        if tokens.iter().any(|t| lower.contains(t.as_str())) {
            hits.push(key.clone());
        }
    }
    hits
}

/// Read cluster labels for the subproject from `entity-registry.json`. Empty
/// when the registry is absent or the subproject has no clusters.
fn cluster_labels_for_subproject(project: &Path, subproject: Option<&str>) -> Vec<String> {
    let Ok(paths) = ClaudePaths::for_project(project) else {
        return Vec::new();
    };
    let registry_path = paths.entity_registry_json_path();
    let Ok(text) = mfs::read_to_string(&registry_path) else {
        return Vec::new();
    };
    let Ok(value): Result<Value, _> = serde_json::from_str(&text) else {
        return Vec::new();
    };
    let mut labels: BTreeSet<String> = BTreeSet::new();
    if let Some(patterns) = value.get("_patterns").and_then(Value::as_object) {
        for (_stack, body) in patterns {
            let Some(arr) = body.get("discovered").and_then(Value::as_array) else {
                continue;
            };
            for cluster in arr {
                if let (Some(sub), Some(label)) = (subproject, cluster.get("subprojectName").and_then(Value::as_str)) {
                    if !sub.ends_with(label) && label != sub {
                        continue;
                    }
                }
                if let Some(label) = cluster.get("label").and_then(Value::as_str) {
                    labels.insert(label.to_ascii_lowercase());
                }
            }
        }
    }
    labels.into_iter().collect()
}

// ---------------------------------------------------------------------------
// Scoring
// ---------------------------------------------------------------------------

fn score_skill(
    fm: &SkillFrontmatter,
    intent_tokens: &[String],
    entity_names: &[String],
    cluster_labels: &[String],
    active_scope: Option<SkillScope>,
) -> (f64, Vec<String>) {
    let mut score = 0.0_f64;
    let mut reasons: Vec<String> = Vec::new();

    // --- tag match: token → SkillTag::parse → fm.tags ---
    for token in intent_tokens {
        if let Some(parsed) = SkillTag::parse(token) {
            if fm.tags.contains(&parsed) || fm.tags.contains(&SkillTag::Any) {
                score += 1.0;
                reasons.push(format!("tag:{token}"));
                break; // count tag-match category once
            }
        }
    }
    // Fallback: description verb hit (gives foundation skills with `any` tag
    // a chance to surface).
    if reasons.is_empty() {
        let desc_lower = fm.description.to_ascii_lowercase();
        for token in intent_tokens {
            if desc_lower.contains(token.as_str()) {
                score += 0.25;
                reasons.push(format!("desc:{token}"));
                break;
            }
        }
    }

    // --- applies_to match ---
    if fm.applies_to.is_empty() {
        // Skill targets everything — small base weight so universal skills
        // still surface but specific ones win.
        score += 0.25;
        reasons.push("applies:any".into());
    } else {
        for cluster in &fm.applies_to {
            let target = cluster.to_ascii_lowercase();
            if cluster_labels.iter().any(|c| c == &target) {
                score += 1.0;
                reasons.push(format!("applies:{cluster}"));
                break;
            }
        }
    }

    // --- entity match ---
    for entity in entity_names {
        if fm
            .entities
            .iter()
            .any(|e| e.eq_ignore_ascii_case(entity))
        {
            score += 1.0;
            reasons.push(format!("entity:{entity}"));
            break;
        }
    }

    // --- scope match ---
    if let Some(active) = active_scope {
        if fm.scope.contains(&active) {
            score += 1.0;
            reasons.push(format!("scope:{}", active.as_str()));
        } else if fm.scope.is_empty() {
            score += 0.25;
            reasons.push("scope:any".into());
        }
    }

    (score, reasons)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::skill::frontmatter::{SkillMetadata, SkillSource};
    use tempfile::tempdir;

    fn fm(name: &str, tags: Vec<SkillTag>, applies: Vec<&str>, scope: Vec<SkillScope>) -> SkillFrontmatter {
        SkillFrontmatter {
            name: name.into(),
            description: "Use when you want to do something that fills enough of the description column.".into(),
            tags,
            applies_to: applies.into_iter().map(String::from).collect(),
            scope,
            entities: vec![],
            metadata: SkillMetadata {
                generated_by: Some(SkillSource::Foundation),
                cluster: None,
            },
            extra: serde_json::Value::Null,
        }
    }

    #[test]
    fn tokenises_intent_lowercase_alphanumeric() {
        let toks = tokenise("Refactor User CRUD!");
        assert_eq!(toks, vec!["refactor", "user", "crud"]);
    }

    #[test]
    fn scores_tag_and_scope_match() {
        let skill = fm("foo", vec![SkillTag::Refactor], vec![], vec![SkillScope::CodeEditing]);
        let (score, reasons) = score_skill(
            &skill,
            &["refactor".into(), "user".into()],
            &[],
            &[],
            Some(SkillScope::CodeEditing),
        );
        // tag(1) + applies:any(0.25) + scope(1) = 2.25
        assert!((score - 2.25).abs() < 0.01, "got {score}, reasons {reasons:?}");
    }

    #[test]
    fn fallback_description_match_when_no_tag() {
        let mut skill = fm("foo", vec![], vec![], vec![]);
        skill.description = "Use when refactoring shared modules and it needs at least fifty chars.".into();
        let (score, _) = score_skill(&skill, &["refactor".into()], &[], &[], None);
        assert!(score > 0.0);
    }

    #[test]
    fn resolve_end_to_end_filesystem() {
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join("apps").join("cli").join("templates").join("skills").join("alpha");
        std::fs::create_dir_all(&skills_dir).unwrap();
        std::fs::write(
            skills_dir.join("SKILL.md"),
            "---\nname: alpha\ndescription: Use when refactoring user CRUD code so it stays maintainable enough.\ntags: [refactor]\nappliesTo: []\nscope: [code-editing]\nmetadata:\n  generated_by: foundation\n---\nbody\n",
        )
        .unwrap();
        let resolved = resolve(dir.path(), "Refactor user CRUD", None, Some("EXECUTE"), 5);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].name, "alpha");
        assert!(resolved[0].score >= 2.0);
    }
}
