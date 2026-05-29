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
//! every SKILL.md frontmatter via [`mustard_core::domain::skill::frontmatter::parse`],
//! and walks the skills directories. Missing registry / unparseable
//! frontmatter degrade gracefully — they are skipped, not fatal.

use crate::shared::context::project_dir;
use mustard_core::io::fs as mfs;
use mustard_core::domain::entity_registry::EntityRegistry;
use mustard_core::domain::skill::discover;
use mustard_core::domain::skill::frontmatter::{parse as parse_fm, SkillFrontmatter, SkillScope, SkillTag};
use mustard_core::ClaudePaths;
use serde::Serialize;
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
    // Single canonical read of the entity-registry (v4). Entity names come from
    // the `e` map; cluster labels from `_patterns.{stack}.discovered[]`. The
    // prior hand-rolled `entity_names` walk iterated the document root and so
    // never matched in a v4 registry (entities live under `e`) — fixed here.
    let registry = EntityRegistry::load(project);
    let entity_names: Vec<String> = registry
        .entity_names()
        .into_iter()
        .filter(|name| {
            let lower = name.to_ascii_lowercase();
            tokens.iter().any(|t| lower.contains(t.as_str()))
        })
        .map(str::to_string)
        .collect();
    let cluster_labels = registry.cluster_labels(subproject);
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
        for candidate in discover::collect_skill_md(&root) {
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
        // Case-insensitive on BOTH sides. The bug this fixes: the registry's
        // `cluster_labels()` lower-cases its labels, but the skill's `appliesTo`
        // was compared verbatim — a skill written `appliesTo: [Service]`
        // (mixed-case, as the scan generator emits, matching the raw cluster
        // `label`) never matched the lower-cased `service`. `eq_ignore_ascii_case`
        // normalises both sides so the match no longer depends on either side's
        // casing.
        for cluster in &fm.applies_to {
            if cluster_labels.iter().any(|c| c.eq_ignore_ascii_case(cluster)) {
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
    use mustard_core::domain::skill::frontmatter::{SkillMetadata, SkillSource};
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
    fn applies_match_is_case_insensitive_regression() {
        // Regression for the case bug: a skill targeting a mixed-case cluster
        // label must match a mixed-case label in the registry. Neither side is
        // pre-lower-cased here, so the only way this scores is `eq_ignore_ascii_case`.
        let skill = fm("svc", vec![], vec!["Service"], vec![]);
        let (score, reasons) = score_skill(
            &skill,
            &[],
            &[],
            &["Service".to_string()],
            None,
        );
        assert!(
            reasons.iter().any(|r| r == "applies:Service"),
            "mixed-case appliesTo must match mixed-case cluster label; reasons {reasons:?}"
        );
        assert!(score >= 1.0, "applies match worth +1.0, got {score}");
        // And the lower-cased registry form (what `cluster_labels()` returns
        // today) must match an upper-cased skill `appliesTo` just the same.
        let (_, reasons_lc) = score_skill(
            &fm("svc", vec![], vec!["SERVICE"], vec![]),
            &[],
            &[],
            &["service".to_string()],
            None,
        );
        assert!(reasons_lc.iter().any(|r| r == "applies:SERVICE"));
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
