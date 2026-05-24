//! `mustard-rt run wave-files` — return the real declared-files count and
//! markdown body of a wave's sub-spec.
//!
//! The dashboard "Ondas" tab originally rendered `wave.files_changed` (the
//! count of `tool.use` events with `tool_name in (Write|Edit)` for a wave) —
//! that is zero for waves that ran before the tracker existed or in a parallel
//! session. The canonical answer is "how many files does the sub-spec declare
//! in the `## Arquivos` (or `## Files`) section". This subcommand parses that
//! section and returns the count plus the full markdown body, so the dashboard
//! can both show the real count and pop open a drawer with the wave markdown.
//!
//! Fail-open: any I/O error degrades to `{"count":0,"markdown":"","path":null}`
//! and exit `0`.

use mustard_core::fs;
use serde_json::json;
use std::path::{Path, PathBuf};

/// Resolve the wave's spec.md path under
/// `.claude/spec/{spec}/wave-{wave}-*/spec.md`. The role suffix is treated
/// as a glob (any non-empty role). If multiple match, the lexicographically
/// first one wins (stable across runs). Returns `None` when none match.
///
/// Wave 2 (2026-05-21, spec `2026-05-21-dashboard-spec-tabs-polish`): when
/// `wave == 0`, the resolver treats the spec's top-level markdown as "Onda
/// #0" — first preferring `<spec_dir>/wave-plan.md` (the wave-plan parent),
/// falling back to `<spec_dir>/spec.md` (single-spec without wave layout).
/// This lets the dashboard's "Onda #0" row in the Ondas tab open the parent
/// markdown using the same drawer wiring as numbered waves.
fn resolve_wave_spec_path(project_dir: &str, spec: &str, wave: u32) -> Option<PathBuf> {
    let spec_dir = Path::new(project_dir)
        .join(".claude")
        .join("spec")
        .join(spec);
    if !spec_dir.is_dir() {
        return None;
    }
    if wave == 0 {
        // Parent spec markdown — prefer `wave-plan.md` when present (wave-plan
        // parents), otherwise fall back to `spec.md` (single-spec runs).
        let wave_plan = spec_dir.join("wave-plan.md");
        if wave_plan.is_file() {
            return Some(wave_plan);
        }
        let parent_spec = spec_dir.join("spec.md");
        if parent_spec.is_file() {
            return Some(parent_spec);
        }
        return None;
    }
    let prefix = format!("wave-{wave}-");
    let mut candidates: Vec<PathBuf> = fs::read_dir(&spec_dir)
        .ok()?
        .into_iter()
        .filter_map(|entry| {
            if !entry.file_name.starts_with(&prefix) {
                return None;
            }
            // Require something after the `wave-N-` prefix (the role segment).
            if entry.file_name.len() <= prefix.len() {
                return None;
            }
            if !entry.is_dir {
                return None;
            }
            let spec_md = entry.path.join("spec.md");
            if spec_md.is_file() { Some(spec_md) } else { None }
        })
        .collect();
    candidates.sort();
    candidates.into_iter().next()
}

/// Parse the `## Arquivos` (or `## Files`) block and count declared entries.
///
/// Counting rules (per spec):
/// - Lines inside a fenced code block (between triple-backtick fences): each
///   non-blank line counts as 1, unless it starts with `//` or `#` (a comment).
/// - Bullet lines (`- ` or `* ` at line start, outside a code fence): each
///   counts as 1.
/// - Section boundary is the next `## ` heading (case-sensitive prefix).
/// - Returns `0` when no matching heading is present.
pub(crate) fn parse_arquivos_count(text: &str) -> usize {
    // Find the start of the section.
    let mut lines = text.lines();
    let mut in_section = false;
    let mut in_fence = false;
    let mut count: usize = 0;

    for line in lines.by_ref() {
        let trimmed_start = line.trim_start();
        if !in_section {
            // Match either Portuguese "## Arquivos" or English "## Files".
            // Case-sensitive on the prefix per spec.
            if line == "## Arquivos" || line == "## Files" {
                in_section = true;
            }
            continue;
        }

        // End of section: next `## ` heading (but not `### ` etc.) — case-sensitive.
        // We do NOT terminate on `### ` sub-headings.
        if !in_fence && line.starts_with("## ") {
            break;
        }
        // Also catch the bare `##` followed by nothing (rare but defensive).
        if !in_fence && (line == "## Arquivos" || line == "## Files") {
            // Re-entering the same heading — ignore.
            continue;
        }

        // Toggle code fence on triple-backtick lines (with optional language tag).
        let fence_candidate = trimmed_start;
        if fence_candidate.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }

        if in_fence {
            // Inside a code fence: every non-blank, non-comment line counts.
            let content = line.trim();
            if content.is_empty() {
                continue;
            }
            if content.starts_with("//") || content.starts_with('#') {
                continue;
            }
            count += 1;
            continue;
        }

        // Outside a code fence: count bullet lines.
        if trimmed_start.starts_with("- ") || trimmed_start.starts_with("* ") {
            count += 1;
        }
    }

    count
}

/// Emit the fail-open empty payload (`count:0, markdown:"", path:null`).
fn emit_empty() {
    let payload = json!({
        "count": 0,
        "markdown": "",
        "path": serde_json::Value::Null,
    });
    println!("{payload}");
}

/// Dispatch `mustard-rt run wave-files --spec <slug> --wave <N>`.
pub fn run(spec: Option<&str>, wave: Option<u32>) {
    let (Some(spec), Some(wave)) = (spec, wave) else {
        eprintln!("[wave-files] usage: mustard-rt run wave-files --spec <slug> --wave <N>");
        emit_empty();
        return;
    };

    let project_dir = crate::run::env::project_dir();
    let Some(path) = resolve_wave_spec_path(&project_dir, spec, wave) else {
        emit_empty();
        return;
    };

    let Ok(markdown) = fs::read_to_string(&path) else {
        emit_empty();
        return;
    };

    let count = parse_arquivos_count(&markdown);
    let path_str = path.to_string_lossy().into_owned();
    let payload = json!({
        "count": count,
        "markdown": markdown,
        "path": path_str,
    });
    println!("{payload}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn counts_files_from_arquivos_block() {
        let fixture = "# Spec\n\n## Arquivos\n\n```\npath/a.rs — comment\npath/b.rs\npath/c.rs\n```\n\n## Outra\n\nresto\n";
        assert_eq!(parse_arquivos_count(fixture), 3);
    }

    #[test]
    fn counts_files_from_bullets() {
        let fixture = "# Spec\n\n## Arquivos\n\n- file1.rs\n- file2.rs\n\n## Outra\n";
        assert_eq!(parse_arquivos_count(fixture), 2);
    }

    #[test]
    fn counts_mixed_bullets_and_fence() {
        // Section has a 2-entry fence and a 1-entry bullet list outside it.
        let fixture = "## Arquivos\n\n```\nsrc/a.rs\nsrc/b.rs\n```\n\n- src/c.rs\n\n## Outra\n";
        assert_eq!(parse_arquivos_count(fixture), 3);
    }

    #[test]
    fn returns_zero_when_arquivos_section_absent() {
        let fixture = "# Spec\n\n## Resumo\n\nNada aqui.\n";
        assert_eq!(parse_arquivos_count(fixture), 0);
    }

    #[test]
    fn returns_zero_when_file_missing() {
        // Direct path-resolver miss: nonexistent project → no candidate.
        let dir = tempdir().unwrap();
        let resolved = resolve_wave_spec_path(
            dir.path().to_str().unwrap(),
            "nonexistent-spec",
            1,
        );
        assert!(resolved.is_none());
    }

    #[test]
    fn resolves_wave_spec_path_with_role_suffix() {
        let dir = tempdir().unwrap();
        let wave_dir = dir
            .path()
            .join(".claude")
            .join("spec")
            .join("my-feat")
            .join("wave-1-backend");
        std::fs::create_dir_all(&wave_dir).unwrap();
        std::fs::write(
            wave_dir.join("spec.md"),
            "## Arquivos\n\n- a.rs\n- b.rs\n\n## Outra\n",
        )
        .unwrap();
        let resolved = resolve_wave_spec_path(
            dir.path().to_str().unwrap(),
            "my-feat",
            1,
        )
        .expect("should resolve wave-1-backend/spec.md");
        let body = std::fs::read_to_string(&resolved).unwrap();
        assert_eq!(parse_arquivos_count(&body), 2);
    }

    #[test]
    fn fence_skips_comment_and_blank_lines() {
        // Comments (`//`, `#`) and blanks inside a fence don't count.
        let fixture = "## Arquivos\n\n```\n// header comment\n\nsrc/a.rs\n# another comment\nsrc/b.rs\n```\n\n## Outra\n";
        assert_eq!(parse_arquivos_count(fixture), 2);
    }

    #[test]
    fn resolves_wave_zero_to_wave_plan_when_present() {
        // Wave 2 (spec 2026-05-21-dashboard-spec-tabs-polish): wave=0 acts as
        // the "Onda #0" pointer to the parent markdown — `wave-plan.md` wins
        // when it exists.
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join(".claude").join("spec").join("parent-x");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("wave-plan.md"), "# Wave plan\n").unwrap();
        std::fs::write(spec_dir.join("spec.md"), "# Spec\n").unwrap();
        let resolved = resolve_wave_spec_path(
            dir.path().to_str().unwrap(),
            "parent-x",
            0,
        )
        .expect("wave=0 must resolve when wave-plan.md exists");
        assert!(resolved.ends_with("wave-plan.md"));
    }

    #[test]
    fn resolves_wave_zero_falls_back_to_spec_md() {
        // Single-spec (no wave plan): wave=0 resolves to `spec.md`.
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join(".claude").join("spec").join("single");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), "# Single\n").unwrap();
        let resolved = resolve_wave_spec_path(
            dir.path().to_str().unwrap(),
            "single",
            0,
        )
        .expect("wave=0 must fall back to spec.md");
        assert!(resolved.ends_with("spec.md"));
    }

    #[test]
    fn resolves_wave_zero_returns_none_when_no_markdown() {
        // Empty spec dir → no candidate.
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join(".claude").join("spec").join("empty");
        std::fs::create_dir_all(&spec_dir).unwrap();
        let resolved = resolve_wave_spec_path(
            dir.path().to_str().unwrap(),
            "empty",
            0,
        );
        assert!(resolved.is_none());
    }
}
