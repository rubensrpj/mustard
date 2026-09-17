//! `{skills_list}` — the target subproject's skill shelf: one line per
//! `<subproject>/.claude/skills/*/SKILL.md` (`- name — description`), names and
//! trigger descriptions only (never bodies) so the `## SKILLS` section stays
//! PREFIX-STABLE (cache-safe).

use std::fmt::Write as _;
use std::path::Path;

/// Build `{skills_list}` — the target subproject's skill shelf: one line per
/// `<subproject>/.claude/skills/*/SKILL.md` (`- name — description`), sorted
/// by folder name for byte-stable output, preceded by the load instruction.
/// Names and trigger descriptions only — never bodies — so the `## SKILLS`
/// section stays PREFIX-STABLE (cache-safe) exactly as the agent-prompt ref
/// documents. Empty (the section collapses) when the subproject has no
/// readable skills. Fail-open: an unparseable SKILL.md contributes its folder
/// name without a description rather than dropping the shelf.
pub(crate) fn build_skills_list(project: &Path, subproject: &str) -> String {
    let skills_dir = project.join(subproject).join(".claude").join("skills");
    let Ok(entries) = std::fs::read_dir(&skills_dir) else {
        return String::new();
    };
    let mut rows: Vec<(String, String)> = Vec::new();
    for entry in entries.flatten() {
        let skill_md = entry.path().join("SKILL.md");
        let Ok(text) = std::fs::read_to_string(&skill_md) else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let description = mustard_core::domain::skill::frontmatter::parse(&text)
            .map(|fm| fm.description)
            .unwrap_or_default();
        rows.push((name, description));
    }
    if rows.is_empty() {
        return String::new();
    }
    rows.sort();
    let mut out = String::from(
        "This subproject has skills — its module molds and conventions. BEFORE creating \
         or refactoring a module of a kind listed below, load the matching skill (Skill \
         tool, or Read its SKILL.md) and follow it; deviations are review findings:\n",
    );
    for (name, description) in rows {
        if description.is_empty() {
            let _ = writeln!(out, "- {name}");
        } else {
            let _ = writeln!(out, "- {name} — {description}");
        }
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skills_list_renders_shelf_sorted_and_fails_open() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let skills = root.join("apps/api/.claude/skills");
        // Two parseable skills + one broken (no frontmatter) — the broken one
        // still contributes its name (fail-open), never sinks the shelf.
        for (folder, desc) in [
            ("api-service-pattern", "Use when adding or refactoring a service."),
            ("api-log-pattern", "Use when adding or refactoring an audit log entity."),
        ] {
            let d = skills.join(folder);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(
                d.join("SKILL.md"),
                format!("---\nname: {folder}\ndescription: \"{desc}\"\nsource: scan\n---\n\nbody\n"),
            )
            .unwrap();
        }
        let broken = skills.join("api-odd-pattern");
        std::fs::create_dir_all(&broken).unwrap();
        std::fs::write(broken.join("SKILL.md"), "# no frontmatter\n").unwrap();

        let out = build_skills_list(root, "apps/api");
        assert!(out.contains("- api-log-pattern — Use when adding or refactoring an audit log entity."), "{out}");
        assert!(out.contains("- api-service-pattern — Use when adding"), "{out}");
        assert!(out.contains("- api-odd-pattern"), "broken skill keeps its name: {out}");
        // Sorted by name: log < odd < service.
        let (log, odd, svc) = (
            out.find("api-log-pattern").unwrap(),
            out.find("api-odd-pattern").unwrap(),
            out.find("api-service-pattern").unwrap(),
        );
        assert!(log < odd && odd < svc, "shelf must be sorted: {out}");
        // No skills dir → empty (the ## SKILLS section collapses).
        assert!(build_skills_list(root, "apps/none").is_empty());
    }
}
