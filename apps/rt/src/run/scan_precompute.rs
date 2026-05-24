//! Pre-dispatch deterministic helpers for `run scan-orchestrate` — a port of
//! `scripts/scan/_precompute.js`.
//!
//! Pure, idempotent, fail-open helpers run before the orchestrator dispatches
//! Task agents: backup generated `*.md`, purge generated skills, ensure the
//! `notes.md` skeleton, and build the tooling / structure prompt blocks.

use mustard_core::fs;
use std::path::Path;

/// Directories never descended into — mirrors the JS `DEFAULT_IGNORE`.
const DEFAULT_IGNORE: &[&str] = &[
    "node_modules", ".git", ".next", "bin", "obj", "dist", "build",
    "migrations", "_backup", ".claude",
];

/// Whether a file carries the `<!-- mustard:generated` marker anywhere.
fn has_generated_marker(path: &Path) -> bool {
    fs::read_to_string(path)
        .is_ok_and(|c| c.contains("<!-- mustard:generated"))
}

/// Move every generated `*.md` (depth 1) in `commands_dir` into `_backup/`.
/// Returns the moved file names. Fail-open.
pub fn backup_generated_mds(commands_dir: &Path) -> Vec<String> {
    let mut moved = Vec::new();
    let Ok(entries) = fs::read_dir(commands_dir) else {
        return moved;
    };
    let backup_dir = commands_dir.join("_backup");
    for entry in entries {
        if entry.is_dir || !entry.file_name.ends_with(".md") || !has_generated_marker(&entry.path) {
            continue;
        }
        if fs::create_dir_all(&backup_dir).is_err() {
            continue;
        }
        // std::fs::rename has no facade equivalent — keep it for the move.
        if std::fs::rename(&entry.path, backup_dir.join(&entry.file_name)).is_ok() {
            moved.push(entry.file_name);
        }
    }
    moved
}

/// Remove every generated skill subdir of `skills_dir`. Returns removed names.
pub fn purge_generated_skills(skills_dir: &Path) -> Vec<String> {
    let mut removed = Vec::new();
    let Ok(entries) = fs::read_dir(skills_dir) else {
        return removed;
    };
    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        let skill_md = entry.path.join("SKILL.md");
        if fs::exists(&skill_md) && has_generated_marker(&skill_md) {
            // std::fs::remove_dir_all has no facade equivalent — keep it.
            if std::fs::remove_dir_all(&entry.path).is_ok() {
                removed.push(entry.file_name);
            }
        }
    }
    removed
}

/// Create `notes.md` in `commands_dir` if missing. Returns `true` if created.
/// Never overwrites a user-authored file.
pub fn ensure_notes_md(commands_dir: &Path, name: &str, role: &str) -> bool {
    let notes_path = commands_dir.join("notes.md");
    if fs::exists(&notes_path) {
        return false;
    }
    if fs::create_dir_all(commands_dir).is_err() {
        return false;
    }
    let content = format!(
        "# Notes: {name} ({role})\n\n\
         > Project-specific notes for {name}. Edit freely — this file is never overwritten by /scan.\n\n\
         ## Mandatory Patterns\n\n## Known Pitfalls\n\n## Observations\n"
    );
    fs::write_atomic(&notes_path, content.as_bytes()).is_ok()
}

/// Build the `## Tooling detected` block from `package.json` / `*.csproj` /
/// `pyproject.toml`. Returns `""` when nothing is detected.
pub fn build_tooling_block(subproject_path: &Path, stack: &str) -> String {
    let stack_lower = stack.to_lowercase();
    let is_net = stack_lower.contains(".net")
        || stack_lower.contains("csharp")
        || stack_lower.contains("c#");
    let is_python = stack_lower.contains("python")
        || stack_lower.contains("fastapi")
        || stack_lower.contains("django");

    let mut lines: Vec<String> = Vec::new();
    if !is_net && !is_python {
        let pkg_path = subproject_path.join("package.json");
        if let Ok(text) = fs::read_to_string(&pkg_path) {
            if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(scripts) = pkg.get("scripts").and_then(|s| s.as_object()) {
                    for key in ["build", "test", "lint", "typecheck", "type-check", "check"] {
                        if let Some(cmd) = scripts.get(key).and_then(|v| v.as_str()) {
                            let label = if key == "type-check" { "typecheck" } else { key };
                            lines.push(format!("- {label}: {cmd} (source: package.json scripts.{key})"));
                        }
                    }
                }
            }
        }
    } else if is_net {
        if let Ok(entries) = fs::read_dir(subproject_path) {
            if let Some(csproj) = entries
                .into_iter()
                .map(|e| e.file_name)
                .find(|n| n.ends_with(".csproj"))
            {
                lines.push(format!("- build: dotnet build (source: {csproj})"));
                lines.push(format!("- test: dotnet test (source: {csproj})"));
            }
        }
    } else if is_python {
        if let Ok(content) = fs::read_to_string(subproject_path.join("pyproject.toml")) {
            if content.contains("pytest") {
                lines.push("- test: pytest (source: pyproject.toml)".to_string());
            }
            if content.contains("ruff") {
                lines.push("- lint: ruff check . (source: pyproject.toml)".to_string());
            }
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    let mut out = vec!["## Tooling detected".to_string()];
    out.extend(lines);
    out.push(String::new());
    out.join("\n")
}

/// Build the `## Project structure` block — depth-1 dirs with file counts.
/// Returns `""` when one directory or fewer survives the ignore filter.
pub fn build_structure_block(subproject_path: &Path) -> String {
    let Ok(entries) = fs::read_dir(subproject_path) else {
        return String::new();
    };
    let dirs: Vec<mustard_core::fs::DirEntry> = entries
        .into_iter()
        .filter(|e| e.is_dir)
        .filter(|e| !DEFAULT_IGNORE.contains(&e.file_name.as_str()))
        .take(12)
        .collect();
    if dirs.len() <= 1 {
        return String::new();
    }
    let mut lines = vec!["## Project structure".to_string()];
    for dir in dirs {
        let count = fs::read_dir(&dir.path)
            .map_or(0, |es| es.iter().filter(|e| !e.is_dir).count());
        lines.push(format!("- {}/ — {count} files", dir.file_name));
    }
    lines.push(String::new());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn ensure_notes_creates_then_skips() {
        let dir = tempdir().unwrap();
        assert!(ensure_notes_md(dir.path(), "api", "general"));
        assert!(!ensure_notes_md(dir.path(), "api", "general"));
        assert!(dir.path().join("notes.md").exists());
    }

    #[test]
    fn purge_removes_only_generated_skills() {
        let dir = tempdir().unwrap();
        let generated = dir.path().join("gen-skill");
        let manual = dir.path().join("manual");
        std::fs::create_dir_all(&generated).unwrap();
        std::fs::create_dir_all(&manual).unwrap();
        std::fs::write(generated.join("SKILL.md"), "<!-- mustard:generated -->\nx").unwrap();
        std::fs::write(manual.join("SKILL.md"), "hand-written").unwrap();
        let removed = purge_generated_skills(dir.path());
        assert_eq!(removed, vec!["gen-skill".to_string()]);
        assert!(manual.exists());
        assert!(!generated.exists());
    }

    #[test]
    fn tooling_block_reads_package_json() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"build":"tsc","test":"vitest"}}"#,
        )
        .unwrap();
        let block = build_tooling_block(dir.path(), "TypeScript");
        assert!(block.contains("build: tsc"));
        assert!(block.contains("test: vitest"));
    }
}
