//! `mustard-rt run analyze-validation` — a port of `scripts/analyze-validation.js`.
//!
//! WARN-level spec validator (never blocks the pipeline). Checks layer
//! coverage, file-reference resolvability, task-count sanity, and the
//! extended-light scope ↔ registry constraint. Emits one JSON line:
//! `{ "ok": bool, "issues": [{ severity, type, message, file? }] }`.

use crate::commands::spec::spec_sections::is_heading;
use mustard_core::fs;
use mustard_core::ClaudePaths;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// File extensions expected per declared agent layer.
fn layer_extensions(layer: &str) -> &'static [&'static str] {
    match layer {
        "Backend" => &[".ts", ".cs", ".py", ".go", ".rs"],
        "Frontend" => &[".tsx", ".jsx", ".vue", ".svelte", ".html", ".css"],
        "Database" => &[".sql", ".prisma", "schema.ts"],
        "Mobile" => &[".swift", ".kt", ".dart"],
        _ => &[],
    }
}

/// Emit a fatal `validator-crash` result and exit 1.
fn crash(message: &str) -> ! {
    let out = json!({
        "ok": false,
        "issues": [{ "severity": "ERROR", "type": "validator-crash", "message": message }],
    });
    println!("{out}");
    std::process::exit(1);
}

/// Extract the body lines of the `## Files` section.
fn files_section_lines(lines: &[&str]) -> Vec<String> {
    let mut in_files = false;
    let mut out = Vec::new();
    for line in lines {
        if is_heading(line, "files") {
            in_files = true;
            continue;
        }
        if in_files && line.starts_with("##") {
            in_files = false;
        }
        if in_files {
            out.push((*line).to_string());
        }
    }
    out
}

/// Find every `### {Word} Agent` header and return the agent name + the body
/// up to the next `##`/`###` heading.
fn agent_blocks(content: &str) -> Vec<(String, String)> {
    let mut blocks = Vec::new();
    let lines: Vec<&str> = content.split('\n').collect();
    for (i, line) in lines.iter().enumerate() {
        // `###\s+(\S.*?)\s+Agent`
        let Some(rest) = line.strip_prefix("###") else {
            continue;
        };
        if !rest.starts_with([' ', '\t']) {
            continue;
        }
        let rest = rest.trim_start();
        let Some(agent_pos) = rest.find(" Agent") else {
            continue;
        };
        let name = rest[..agent_pos].trim();
        if name.is_empty() {
            continue;
        }
        let mut body = String::new();
        for next in lines.iter().skip(i + 1) {
            let t = next.trim_start();
            if t.starts_with("## ") || t.starts_with("### ") {
                break;
            }
            body.push_str(next);
            body.push('\n');
        }
        blocks.push((name.to_string(), body));
    }
    blocks
}

/// Extract the first capture of a simple `key:\s*["']?value["']?` pattern,
/// case-insensitive. `value_chars` controls which chars belong to the value.
fn extract_kv<'a>(content: &'a str, key: &str) -> Option<&'a str> {
    let lower = content.to_lowercase();
    let key_lower = key.to_lowercase();
    let mut search = 0;
    while let Some(rel) = lower[search..].find(&key_lower) {
        let at = search + rel;
        let after = &content[at + key.len()..];
        let after_t = after.trim_start_matches([' ', '\t']);
        if !after_t.starts_with(':') {
            search = at + key.len();
            continue;
        }
        let mut val = after_t[1..].trim_start_matches([' ', '\t']);
        val = val.strip_prefix(['"', '\'']).unwrap_or(val);
        let end = val
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .unwrap_or(val.len());
        let token = &val[..end];
        if !token.is_empty() {
            return Some(token);
        }
        search = at + key.len();
    }
    None
}

/// Count `- [ ]` / `- [x]` checkbox markers in a block.
fn count_tasks(block: &str) -> usize {
    let mut count = 0;
    let bytes = block.as_bytes();
    let needle = b"- [";
    let mut i = 0;
    while i + 5 <= bytes.len() {
        if &bytes[i..i + 3] == needle {
            let c = bytes[i + 3];
            if (c == b' ' || c == b'x') && bytes[i + 4] == b']' {
                count += 1;
                i += 5;
                continue;
            }
        }
        i += 1;
    }
    count
}

/// Scan a string for `` `path.ext` `` tokens (backtick-wrapped file refs).
fn backtick_file_refs(text: &str) -> Vec<String> {
    let mut refs = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('`') {
        let after = &rest[open + 1..];
        if let Some(close) = after.find('`') {
            let token = &after[..close];
            // `[\w./-]+\.\w+` — path chars then a dotted extension.
            let ok = !token.is_empty()
                && token.contains('.')
                && token
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '/' | '-' | '_'))
                && token
                    .rsplit('.')
                    .next()
                    .is_some_and(|ext| !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
            if ok {
                refs.push(token.to_string());
            }
            rest = &after[close + 1..];
        } else {
            break;
        }
    }
    refs
}

/// Run the validation. Returns the issues list.
fn validate(abs_path: &Path, content: &str) -> Vec<Value> {
    let lines: Vec<&str> = content.split('\n').collect();
    let mut issues: Vec<Value> = Vec::new();

    let file_lines = files_section_lines(&lines);
    let files_text = file_lines.join("\n");

    // Validation 1: layer coverage.
    for layer in ["Backend", "Frontend", "Database", "Mobile"] {
        let header = format!("### {layer} Agent");
        if !content.contains(&header) {
            continue;
        }
        let exts = layer_extensions(layer);
        let has_match = exts.iter().any(|ext| files_text.contains(ext));
        if !has_match {
            issues.push(json!({
                "severity": "WARN",
                "type": "layer-gap",
                "message": format!("Spec declares {layer} Agent but Files has no {layer} extensions"),
            }));
        }
    }

    // Validation 2: file refs resolvable.
    let spec_dir = abs_path.parent().unwrap_or_else(|| Path::new("."));
    for r in backtick_file_refs(&files_text) {
        let line_with_ref = file_lines
            .iter()
            .find(|l| l.contains(&format!("`{r}`")))
            .map_or("", String::as_str);
        let is_create = line_with_ref.to_lowercase().contains("(create)");
        let resolved = fs::exists(spec_dir.join(&r)) || fs::exists(Path::new(&r));
        if !is_create && !resolved {
            issues.push(json!({
                "severity": "WARN",
                "type": "missing-file",
                "file": r,
                "message": "File referenced but not found and not marked (create)",
            }));
        }
    }

    // Validation 3: task decomposition sane.
    for (agent_name, block) in agent_blocks(content) {
        let tasks = count_tasks(&block);
        if !(2..=10).contains(&tasks) {
            issues.push(json!({
                "severity": "WARN",
                "type": "task-count",
                "message": format!("{agent_name} Agent has {tasks} tasks (expected 2-10)"),
            }));
        }
    }

    // Validation 4: extended-light scope requires entity in registry.
    if let Some(scope) = extract_kv(content, "scope") {
        if scope.eq_ignore_ascii_case("extended-light") {
            if let Some(entity) = extract_kv(content, "entity") {
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                let registry_path: PathBuf = ClaudePaths::for_project(&cwd)
                    .map(|p| p.entity_registry_json_path())
                    .unwrap_or_default();
                if let Ok(registry) = fs::read_to_string(&registry_path) {
                    if !registry.to_lowercase().contains(&entity.to_lowercase()) {
                        issues.push(json!({
                            "severity": "WARN",
                            "type": "scope-mismatch",
                            "message": format!("Extended Light scope requires entity \"{entity}\" in registry, but not found. Reclassify as Full."),
                        }));
                    }
                } else {
                    issues.push(json!({
                        "severity": "WARN",
                        "type": "scope-mismatch",
                        "message": "Extended Light scope requires entity-registry.json, but file not found. Reclassify as Full.",
                    }));
                }
            }
        }
    }

    issues
}

/// Dispatch `mustard-rt run analyze-validation`.
pub fn run(spec: Option<&str>) {
    let Some(spec) = spec else {
        crash("No spec path provided. Use --spec <path>");
    };
    let abs_path = std::fs::canonicalize(spec)
        .unwrap_or_else(|_| PathBuf::from(spec));
    if !fs::exists(&abs_path) {
        crash(&format!("Spec file not found: {}", abs_path.display()));
    }
    let content = match fs::read_to_string(&abs_path) {
        Ok(c) => c,
        Err(e) => crash(&format!("{e}")),
    };
    let issues = validate(&abs_path, &content);
    let out = json!({ "ok": issues.is_empty(), "issues": issues });
    println!("{out}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn clean_spec_has_no_issues() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        let body = "# Spec\n## Files\n- `a.rs` (create)\n### Backend Agent\n- [ ] t1\n- [ ] t2\n";
        std::fs::write(&path, body).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let issues = validate(&path, &content);
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn flags_task_count_out_of_range() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(&path, "### Backend Agent\n- [ ] only one\n").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let issues = validate(&path, &content);
        assert!(issues.iter().any(|i| i["type"] == json!("task-count")));
    }

    #[test]
    fn flags_layer_gap() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(
            &path,
            "## Files\n- `a.txt` (create)\n### Frontend Agent\n- [ ] t1\n- [ ] t2\n",
        )
        .unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let issues = validate(&path, &content);
        assert!(issues.iter().any(|i| i["type"] == json!("layer-gap")));
    }
}
