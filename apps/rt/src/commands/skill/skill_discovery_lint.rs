//! `mustard-rt run doctor --check skill-discovery`
//!
//! Scans SKILL.md files under `.claude/commands/mustard/` (installed) and
//! `apps/cli/templates/commands/mustard/` (source) for "telltale phrases" that
//! indicate the SKILL is doing deterministic filesystem discovery work that
//! should live in a `mustard-rt run` subcommand instead of the LLM.
//!
//! Severity: WARN only — never blocks. The lint is advisory; humans decide
//! whether to migrate.

use mustard_core::domain::skill::discover::collect_skill_md;
use mustard_core::io::fs;
use mustard_core::ClaudePaths;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// One lint violation — a single telltale phrase found in a SKILL.md.
#[derive(Debug, Clone)]
pub struct Violation {
    /// Absolute or project-relative path to the SKILL.md file.
    pub path: String,
    /// The telltale phrase that triggered the violation.
    pub phrase: String,
    /// 1-based line number of the first match.
    pub line: usize,
}

/// Result returned by [`check`].
#[derive(Debug)]
pub struct LintReport {
    /// All violations found (may be empty).
    pub violations: Vec<Violation>,
    /// Total number of SKILL.md files scanned.
    pub scanned: usize,
}

// ---------------------------------------------------------------------------
// Telltale patterns
// ---------------------------------------------------------------------------

/// A telltale pattern: a static lowercase substring that, when found in a
/// SKILL.md line, signals potential LLM-side deterministic discovery.
struct Telltale {
    /// Lowercase needle — compared against a lowercased line.
    needle: &'static str,
    /// Human-readable phrase used in the violation report.
    phrase: &'static str,
}

/// All telltale patterns, in priority order.
const TELLTALES: &[Telltale] = &[
    Telltale {
        needle: "glob `.claude/",
        phrase: "Glob `.claude/",
    },
    Telltale {
        needle: "parse yaml frontmatter",
        phrase: "parse YAML frontmatter",
    },
    Telltale {
        needle: "iterate `registry.e",
        phrase: "Iterate `registry.e`",
    },
    // "for each spec" or "for each skill" followed within ~3 lines by
    // "read", "parse", or "iterate" is handled via the window scanner below.
    // These simple single-line needles catch the common cases first.
    Telltale {
        needle: "for each spec",
        phrase: "For each spec",
    },
    Telltale {
        needle: "for each skill",
        phrase: "For each skill",
    },
    Telltale {
        needle: "for each entity",
        phrase: "For each entity",
    },
    Telltale {
        needle: "for each wave",
        phrase: "For each wave",
    },
    Telltale {
        needle: "read .* lines",
        phrase: "read .* lines",
    },
];

// ---------------------------------------------------------------------------
// Whitelist / exemption checks
// ---------------------------------------------------------------------------

/// Returns true if the surrounding context of a violation should be exempted.
///
/// Exempted when ANY of:
/// - The line itself is inside an HTML comment (`<!-- ... -->`).
/// - A `<!-- example -->` tag appears within 2 lines before the match.
/// - The match is within an HTML `<details>` block (opened but not yet closed).
fn is_whitelisted(lines: &[&str], match_idx: usize) -> bool {
    let line = lines[match_idx];

    // 1. Line itself is an HTML comment.
    let trimmed = line.trim();
    if trimmed.starts_with("<!--") && trimmed.ends_with("-->") {
        return true;
    }

    // 2. A <!-- example --> tag within the 2 lines before the match.
    let look_back_start = match_idx.saturating_sub(2);
    for prior in &lines[look_back_start..match_idx] {
        if prior.to_ascii_lowercase().contains("<!-- example -->") {
            return true;
        }
    }

    // 3. Inside an open <details> block (scan from the start to match_idx).
    let mut details_depth: i32 = 0;
    for prior in &lines[..match_idx] {
        let lc = prior.to_ascii_lowercase();
        if lc.contains("<details") {
            details_depth += 1;
        }
        if lc.contains("</details>") {
            details_depth = (details_depth - 1).max(0);
        }
    }
    if details_depth > 0 {
        return true;
    }

    false
}

// ---------------------------------------------------------------------------
// File scanner
// ---------------------------------------------------------------------------

/// Scan a single SKILL.md file for violations.
fn scan_file(path: &Path, violations: &mut Vec<Violation>) {
    let Ok(content) = fs::read_to_string(path) else {
        return; // fail-open
    };

    let lines: Vec<&str> = content.lines().collect();

    for (idx, line) in lines.iter().enumerate() {
        let lc = line.to_ascii_lowercase();

        for telltale in TELLTALES {
            if lc.contains(telltale.needle) && !is_whitelisted(&lines, idx) {
                violations.push(Violation {
                    path: path.to_string_lossy().into_owned(),
                    phrase: telltale.phrase.to_string(),
                    line: idx + 1, // 1-based
                });
                // One violation per line (first matching telltale wins).
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Directory scanner
// ---------------------------------------------------------------------------

/// Glob `{root}/commands/mustard/*/SKILL.md` and return matching paths.
fn find_skill_files(root: &Path) -> Vec<PathBuf> {
    let commands_dir = root.join("commands").join("mustard");
    // Canonical SKILL.md walk (one level under `commands/mustard/`); sort for
    // a stable scan order across the installed + template bases.
    let mut skills = collect_skill_md(&commands_dir);
    skills.sort();
    skills
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Scan all SKILL.md files reachable from `root`.
///
/// Two search bases are probed:
/// - `{root}/.claude/` — the installed `.claude/` directory.
/// - `{root}/apps/cli/templates/` — the CLI template source (present only in
///   the Mustard monorepo; skipped when absent).
///
/// Violations from both bases are merged into a single report.
pub fn check(root: &Path) -> LintReport {
    let mut violations: Vec<Violation> = Vec::new();
    let mut scanned: usize = 0;

    // Base 1: installed .claude/
    let installed_skills = match ClaudePaths::for_project(root) {
        Ok(paths) => find_skill_files(&paths.claude_dir()),
        Err(_) => Vec::new(),
    };
    for path in &installed_skills {
        scan_file(path, &mut violations);
        scanned += 1;
    }

    // Base 2: source templates/ (monorepo only — skip when absent).
    let templates_dir = root.join("apps").join("cli").join("templates");
    if templates_dir.exists() {
        let template_skills = find_skill_files(&templates_dir);
        for path in &template_skills {
            // Deduplicate: skip if we already scanned a file with identical
            // content via the installed base (same filename in both trees).
            let file_name = path.file_name().unwrap_or_default();
            let parent_name = path
                .parent()
                .and_then(|p| p.file_name())
                .unwrap_or_default();

            // A simple guard: skip if the installed tree already has a SKILL.md
            // with the same parent directory name. This avoids double-counting
            // identical files in both trees.
            let already_scanned = installed_skills.iter().any(|s| {
                s.parent()
                    .and_then(|p| p.file_name())
                    .unwrap_or_default()
                    == parent_name
                    && s.file_name().unwrap_or_default() == file_name
            });

            if !already_scanned {
                scan_file(path, &mut violations);
                scanned += 1;
            }
        }
    }

    LintReport { violations, scanned }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs as std_fs;
    use tempfile::tempdir;

    fn write_skill(dir: &Path, skill_name: &str, content: &str) -> PathBuf {
        let skill_dir = dir
            .join("commands")
            .join("mustard")
            .join(skill_name);
        std_fs::create_dir_all(&skill_dir).unwrap();
        let path = skill_dir.join("SKILL.md");
        std_fs::write(&path, content).unwrap();
        path
    }

    // --- Case 1: violation detected ---

    #[test]
    fn glob_claude_spec_triggers_violation() {
        let tmp = tempdir().unwrap();
        let claude_dir = tmp.path().join(".claude");
        std_fs::create_dir_all(&claude_dir).unwrap();

        write_skill(
            &claude_dir,
            "test-skill",
            "# My Skill\n\nGlob `.claude/spec` to list active specs.\n",
        );

        let report = check(tmp.path());
        assert_eq!(report.scanned, 1);
        assert_eq!(
            report.violations.len(),
            1,
            "expected 1 violation, got: {:?}",
            report.violations.iter().map(|v| &v.phrase).collect::<Vec<_>>()
        );
        assert_eq!(report.violations[0].phrase, "Glob `.claude/");
        assert_eq!(report.violations[0].line, 3);
    }

    // --- Case 2: whitelist via <!-- example --> ---

    #[test]
    fn example_comment_whitelists_violation() {
        let tmp = tempdir().unwrap();
        let claude_dir = tmp.path().join(".claude");
        std_fs::create_dir_all(&claude_dir).unwrap();

        write_skill(
            &claude_dir,
            "clean-skill",
            "# Clean Skill\n\n<!-- example -->\nGlob `.claude/spec` (do NOT do this in a real skill).\n",
        );

        let report = check(tmp.path());
        assert_eq!(report.scanned, 1);
        assert_eq!(
            report.violations.len(),
            0,
            "expected 0 violations (whitelisted), got: {:?}",
            report.violations.iter().map(|v| &v.phrase).collect::<Vec<_>>()
        );
    }

    // --- Case 3: clean SKILL — only calls binary ---

    #[test]
    fn clean_skill_yields_no_violations() {
        let tmp = tempdir().unwrap();
        let claude_dir = tmp.path().join(".claude");
        std_fs::create_dir_all(&claude_dir).unwrap();

        write_skill(
            &claude_dir,
            "spec-skill",
            "# /mustard:spec\n\n## Flow\n\n1. Run: `rtk mustard-rt run active-specs --format table`\n2. Print the output verbatim.\n3. Ask the user which spec to resume.\n",
        );

        let report = check(tmp.path());
        assert_eq!(report.scanned, 1);
        assert_eq!(
            report.violations.len(),
            0,
            "expected 0 violations for clean skill, got: {:?}",
            report.violations.iter().map(|v| &v.phrase).collect::<Vec<_>>()
        );
    }

    // --- Case 4: details block whitelists ---

    #[test]
    fn details_block_whitelists_violation() {
        let tmp = tempdir().unwrap();
        let claude_dir = tmp.path().join(".claude");
        std_fs::create_dir_all(&claude_dir).unwrap();

        write_skill(
            &claude_dir,
            "details-skill",
            "# Skill\n<details>\n<summary>Deprecated approach</summary>\nGlob `.claude/spec` to list specs.\n</details>\n\nActual flow: use `mustard-rt run active-specs`.\n",
        );

        let report = check(tmp.path());
        assert_eq!(report.scanned, 1);
        assert_eq!(
            report.violations.len(),
            0,
            "expected 0 violations (inside <details>), got: {:?}",
            report.violations.iter().map(|v| &v.phrase).collect::<Vec<_>>()
        );
    }

    // --- Case 5: parse YAML frontmatter telltale ---

    #[test]
    fn parse_yaml_frontmatter_triggers_violation() {
        let tmp = tempdir().unwrap();
        let claude_dir = tmp.path().join(".claude");
        std_fs::create_dir_all(&claude_dir).unwrap();

        write_skill(
            &claude_dir,
            "yaml-skill",
            "# Skills\n\nFor each skill file:\n1. Parse YAML frontmatter to extract `name` and `description`.\n2. Build a table.\n",
        );

        let report = check(tmp.path());
        assert!(
            report.violations.len() >= 1,
            "expected at least 1 violation for 'parse YAML frontmatter'"
        );
        let has_yaml = report.violations.iter().any(|v| v.phrase.contains("YAML"));
        assert!(has_yaml, "expected 'parse YAML frontmatter' violation");
    }
}
