//! `{reference_files}` — the spec's `## Files` / `## Arquivos` list plus a
//! compact structural summary (public signatures + declared entities) of the
//! listed files via tree-sitter. Never a file dump.

use crate::commands::spec::spec_sections::{is_heading, section_end};
use mustard_core::domain::ast::{extract_entities, extract_function_signatures, GrammarLoader};
use mustard_core::io::fs as mfs;
use std::fmt::Write as _;
use std::path::Path;

/// Build `{reference_files}` — the spec's `## Files` / `## Arquivos` list plus
/// a compact structural summary (public signatures + declared entities) of the
/// listed files via tree-sitter, never a file dump.
///
/// The `## Files` section drives the list; each path that resolves under the
/// subproject is parsed once through `mustard_core::domain::ast` (AST when a
/// grammar resolves, agnostic fallback otherwise) and reduced to its public
/// function names + entity names. Empty when the spec has no Files section.
pub(crate) fn build_reference_files(project: &Path, subproject: &str, spec_path: &Path) -> String {
    let spec_text = mfs::read_to_string(spec_path).unwrap_or_default();
    if spec_text.is_empty() {
        return String::new();
    }
    let files = files_section_paths(&spec_text);
    if files.is_empty() {
        return String::new();
    }
    let sub_root = project.join(subproject);
    // One shared grammar loader for every file (built once, with builtins so the
    // AST path is available for the common languages; the fallback floor covers
    // everything else). Anchored at the subproject so on-disk grammar overrides
    // resolve.
    let loader = GrammarLoader::with_builtins(&sub_root);

    let mut out = String::from("## Files\n");
    for rel in files.iter().take(20) {
        let _ = writeln!(out, "- `{rel}`");
        let abs = sub_root.join(rel);
        let abs = if abs.is_file() { abs } else { project.join(rel) };
        if !abs.is_file() {
            continue;
        }
        let Ok(source) = mfs::read_to_string(&abs) else {
            continue;
        };
        let lang_id = loader.language_id_for_path(&abs).unwrap_or_default();
        let summary = structural_summary(&loader, &source, &lang_id);
        if !summary.is_empty() {
            let _ = writeln!(out, "  - {summary}");
        }
    }
    out.trim_end().to_string()
}

/// Compact structural summary of one source file: up to a few public function
/// names and declared entity names. Returns `""` when nothing is extracted so
/// the caller omits the sub-bullet.
fn structural_summary(loader: &GrammarLoader, source: &str, lang_id: &str) -> String {
    let mut fns: Vec<String> = extract_function_signatures(loader, source, lang_id)
        .into_iter()
        .map(|s| s.name)
        .collect();
    fns.dedup();
    fns.truncate(6);
    let mut ents: Vec<String> = extract_entities(loader, source, lang_id)
        .into_iter()
        .map(|e| e.name)
        .collect();
    ents.dedup();
    ents.truncate(6);

    let mut parts: Vec<String> = Vec::new();
    if !ents.is_empty() {
        parts.push(format!("types: {}", ents.join(", ")));
    }
    if !fns.is_empty() {
        parts.push(format!("fns: {}", fns.join(", ")));
    }
    parts.join(" | ")
}

/// Extract the file paths listed under a spec's `## Files` / `## Arquivos`
/// section. Each line's first backtick-quoted token (or, failing that, the
/// first path-ish token) is taken as the path. Stops at the next `## ` heading.
pub(crate) fn files_section_paths(spec_text: &str) -> Vec<String> {
    let lines: Vec<&str> = spec_text.lines().collect();
    let Some(start) = lines.iter().position(|l| is_heading(l, "files")) else {
        return Vec::new();
    };
    let end = section_end(&lines, start);
    let mut out: Vec<String> = Vec::new();
    for line in &lines[start + 1..end] {
        if let Some(path) = first_path_token(line) {
            if !out.contains(&path) {
                out.push(path);
            }
        }
    }
    out
}

/// First path-like token in a `## Files` bullet: the content of the first
/// backtick pair when present, else the first whitespace-delimited token that
/// looks like a path (contains `/` or a dotted extension).
fn first_path_token(line: &str) -> Option<String> {
    if let Some(open) = line.find('`') {
        if let Some(close_rel) = line[open + 1..].find('`') {
            let inner = line[open + 1..open + 1 + close_rel].trim();
            if !inner.is_empty() {
                return Some(inner.replace('\\', "/"));
            }
        }
    }
    let stripped = line
        .trim_start()
        .trim_start_matches(['-', '*', ' '])
        .trim_start_matches(['[', 'x', ' ', ']'])
        .trim_start();
    let first = stripped.split_whitespace().next()?;
    let looks_pathy = first.contains('/')
        || first
            .rsplit_once('.')
            .is_some_and(|(_, ext)| !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric()));
    if looks_pathy {
        Some(first.trim_matches(['(', ')', ',']).replace('\\', "/"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Plant a workspace anchor so `ClaudePaths::for_project` accepts the temp dir.
    fn anchor(dir: &Path) {
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(dir.join("mustard.json"), b"{}").unwrap();
    }

    #[test]
    fn build_reference_files_lists_files_and_signatures() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        // A source file under the subproject with a public fn + struct.
        let sub = dir.path().join("api");
        std::fs::create_dir_all(sub.join("src")).unwrap();
        std::fs::write(
            sub.join("src").join("user.rs"),
            "pub struct User { id: i32 }\npub fn make_user() -> User { User { id: 0 } }\n",
        )
        .unwrap();
        let spec = dir.path().join("spec.md");
        std::fs::write(&spec, "# T\n## Files\n- `src/user.rs` — the user model\n## Tasks\n- x\n").unwrap();
        let refs = build_reference_files(dir.path(), "api", &spec);
        assert!(refs.contains("## Files"));
        assert!(refs.contains("src/user.rs"));
        // Structural summary surfaces the public fn / type name.
        assert!(refs.contains("make_user") || refs.contains("User"), "got: {refs}");
        // No spec Files section → empty.
        let empty_spec = dir.path().join("empty.md");
        std::fs::write(&empty_spec, "# T\n## Tasks\n- x\n").unwrap();
        assert!(build_reference_files(dir.path(), "api", &empty_spec).is_empty());
    }
}
