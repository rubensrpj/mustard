//! Pre-dispatch deterministic helpers for `run scan-orchestrate` — a port of
//! `scripts/scan/_precompute.js`.
//!
//! Pure, idempotent, fail-open helpers run before the orchestrator dispatches
//! Task agents: backup generated `*.md`, purge generated skills, ensure the
//! `notes.md` skeleton, and build the tooling / structure prompt blocks.

use std::path::Path;

/// Directories never descended into — mirrors the JS `DEFAULT_IGNORE`.
const DEFAULT_IGNORE: &[&str] = &[
    "node_modules", ".git", ".next", "bin", "obj", "dist", "build",
    "migrations", "_backup", ".claude",
];

/// Whether a file carries the `<!-- mustard:generated` marker anywhere.
fn has_generated_marker(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .map(|c| c.contains("<!-- mustard:generated"))
        .unwrap_or(false)
}

/// Move every generated `*.md` (depth 1) in `commands_dir` into `_backup/`.
/// Returns the moved file names. Fail-open.
pub fn backup_generated_mds(commands_dir: &Path) -> Vec<String> {
    let mut moved = Vec::new();
    let Ok(entries) = std::fs::read_dir(commands_dir) else {
        return moved;
    };
    let backup_dir = commands_dir.join("_backup");
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if !path.is_file() || !name.ends_with(".md") || !has_generated_marker(&path) {
            continue;
        }
        if std::fs::create_dir_all(&backup_dir).is_err() {
            continue;
        }
        if std::fs::rename(&path, backup_dir.join(&name)).is_ok() {
            moved.push(name);
        }
    }
    moved
}

/// Remove every generated skill subdir of `skills_dir`. Returns removed names.
pub fn purge_generated_skills(skills_dir: &Path) -> Vec<String> {
    let mut removed = Vec::new();
    let Ok(entries) = std::fs::read_dir(skills_dir) else {
        return removed;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let skill_md = path.join("SKILL.md");
        if skill_md.exists() && has_generated_marker(&skill_md) {
            if std::fs::remove_dir_all(&path).is_ok() {
                removed.push(entry.file_name().to_string_lossy().to_string());
            }
        }
    }
    removed
}

/// Create `notes.md` in `commands_dir` if missing. Returns `true` if created.
/// Never overwrites a user-authored file.
pub fn ensure_notes_md(commands_dir: &Path, name: &str, role: &str) -> bool {
    let notes_path = commands_dir.join("notes.md");
    if notes_path.exists() {
        return false;
    }
    if std::fs::create_dir_all(commands_dir).is_err() {
        return false;
    }
    let content = format!(
        "# Notes: {name} ({role})\n\n\
         > Project-specific notes for {name}. Edit freely — this file is never overwritten by /scan.\n\n\
         ## Mandatory Patterns\n\n## Known Pitfalls\n\n## Observations\n"
    );
    std::fs::write(&notes_path, content).is_ok()
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
        if let Ok(text) = std::fs::read_to_string(&pkg_path) {
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
        if let Ok(entries) = std::fs::read_dir(subproject_path) {
            if let Some(csproj) = entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .find(|n| n.ends_with(".csproj"))
            {
                lines.push(format!("- build: dotnet build (source: {csproj})"));
                lines.push(format!("- test: dotnet test (source: {csproj})"));
            }
        }
    } else if is_python {
        if let Ok(content) = std::fs::read_to_string(subproject_path.join("pyproject.toml")) {
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
    let Ok(entries) = std::fs::read_dir(subproject_path) else {
        return String::new();
    };
    let dirs: Vec<std::path::PathBuf> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| !DEFAULT_IGNORE.contains(&n))
                .unwrap_or(false)
        })
        .take(12)
        .collect();
    if dirs.len() <= 1 {
        return String::new();
    }
    let mut lines = vec!["## Project structure".to_string()];
    for dir in dirs {
        let count = std::fs::read_dir(&dir)
            .map(|es| es.flatten().filter(|e| e.path().is_file()).count())
            .unwrap_or(0);
        let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
        lines.push(format!("- {name}/ — {count} files"));
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
