//! `mustard-rt run wikilink-extract` — scan a spec directory tree, extract
//! every `[[wikilink]]` occurrence, persist them into the `wikilinks` table.
//!
//! Used by the wave-network spec (`2026-05-20-mustard-wave-network-standard`)
//! to power the dashboard "Network" tab. Walks every `.md` file under
//! `--spec-dir` recursively, parses `[[name]]` references with a small
//! single-pass scanner (no `regex` dependency — `mustard-rt` deliberately
//! avoids one), and writes results through
//! [`mustard_core::store::wikilinks`].
//!
//! Output (stdout, pretty JSON):
//!
//! ```json
//! {
//!   "wikilinks": [{ "from": "...", "to": "...", "file": "...", "line": 12 }, ...],
//!   "orphans":   ["..."]
//! }
//! ```
//!
//! An `orphan` is a `to` value whose name does not match any directory under
//! `.claude/spec/`. Orphan detection is best-effort — a missing `.claude/spec`
//! tree yields zero orphans rather than an error. Wave-2 of
//! `2026-05-21-flatten-spec-layout-and-multi-collab` removed the
//! `active/` / `completed/` buckets; spec dirs live flat under `.claude/spec/`.
//!
//! Exit code is always `0` (fail-open).

use crate::run::env::project_dir;
use mustard_core::fs;
use mustard_core::store::sqlite_store::SqliteEventStore;
use mustard_core::store::wikilinks::{self, Wikilink};
use rusqlite::Connection;
use serde_json::json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Recursively collect every `.md` file under `root`. Returns `(absolute,
/// relative)` pairs — the relative path is what lands in the `file` column.
fn collect_markdown(root: &Path) -> Vec<(PathBuf, String)> {
    let mut out: Vec<(PathBuf, String)> = Vec::new();
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries {
            if entry.is_dir {
                stack.push(entry.path.clone());
                continue;
            }
            if !entry.file_name.ends_with(".md") {
                continue;
            }
            let rel = entry.path
                .strip_prefix(root)
                .unwrap_or(&entry.path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((entry.path, rel));
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

/// Single-pass scan of `content` for `[[name]]` occurrences. `name` matches
/// the spec regex `[a-zA-Z0-9_\-]+`; anything else closes the candidate. The
/// returned tuples are `(target, line)` with 1-based line numbers.
fn extract_links(content: &str) -> Vec<(String, u32)> {
    let mut out: Vec<(String, u32)> = Vec::new();
    let bytes = content.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;
    let mut line: u32 = 1;
    while i < len {
        let b = bytes[i];
        if b == b'\n' {
            line += 1;
            i += 1;
            continue;
        }
        if b == b'[' && i + 1 < len && bytes[i + 1] == b'[' {
            // Find matching `]]`. The token body is restricted to the
            // [a-zA-Z0-9_-] set; the first byte outside the set aborts the
            // match without advancing `i` past `[[` so the outer loop keeps
            // moving one byte at a time.
            let start = i + 2;
            let mut j = start;
            while j < len {
                let c = bytes[j];
                if c.is_ascii_alphanumeric() || c == b'_' || c == b'-' {
                    j += 1;
                    continue;
                }
                break;
            }
            if j > start && j + 1 < len && bytes[j] == b']' && bytes[j + 1] == b']' {
                // SAFETY: the slice is restricted to ASCII bytes — it is
                // guaranteed valid UTF-8, so the from_utf8 is infallible. We
                // still go through std::str::from_utf8 to avoid `unsafe`.
                if let Ok(name) = std::str::from_utf8(&bytes[start..j]) {
                    out.push((name.to_string(), line));
                }
                i = j + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Derive the `from` field for a markdown file: its parent directory name
/// when the file lives in a subdir under `root`, or the file stem when the
/// file is at the root of `--spec-dir`.
fn derive_from(root: &Path, abs_path: &Path) -> String {
    let rel = abs_path.strip_prefix(root).unwrap_or(abs_path);
    let mut comps: Vec<&std::ffi::OsStr> = rel
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(n) => Some(n),
            _ => None,
        })
        .collect();
    if comps.len() <= 1 {
        // File at root of --spec-dir → use file stem (filename minus `.md`).
        return abs_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
    }
    // Drop the file name itself; keep the immediate parent directory.
    comps.pop();
    let parent = comps
        .last()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    if parent.is_empty() {
        // Defensive: should never happen given `comps.len() > 1`.
        return abs_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
    }
    parent
}

/// Collect every spec directory name under `.claude/spec/` (flat layout).
/// Returns an empty set when the tree is missing — orphan reporting is purely
/// advisory.
fn known_specs(project: &Path) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    let root = project.join(".claude").join("spec");
    let Ok(entries) = fs::read_dir(&root) else {
        return out;
    };
    for entry in entries {
        if entry.is_dir {
            out.insert(entry.file_name);
        }
    }
    out
}

/// Open the project SQLite store, derive a rusqlite [`Connection`] from its
/// backing file path, and ensure the `wikilinks` table exists. The store
/// itself is held only for the duration of the call so the schema's
/// `IF NOT EXISTS` clauses fire once and then we reuse the connection for
/// inserts (the store owns its own connection privately).
fn open_conn(project: &Path) -> Option<Connection> {
    let store = SqliteEventStore::for_project(project).ok()?;
    let db_path = store.path().to_path_buf();
    let conn = Connection::open(&db_path).ok()?;
    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
    wikilinks::ensure_schema(&conn).ok()?;
    Some(conn)
}

/// Run `mustard-rt run wikilink-extract --spec-dir <dir>`.
///
/// Fail-open: a missing `--spec-dir`, a non-existent directory, or an
/// unreachable SQLite database all degrade to a JSON result with empty
/// arrays — never a non-zero exit.
pub fn run(spec_dir_arg: Option<&str>) {
    let Some(spec_dir_arg) = spec_dir_arg else {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "wikilinks": [], "orphans": [] }))
                .unwrap_or_else(|_| "{}".to_string())
        );
        eprintln!("Usage: wikilink-extract --spec-dir <dir>");
        return;
    };
    let spec_dir = if Path::new(spec_dir_arg).is_absolute() {
        PathBuf::from(spec_dir_arg)
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(spec_dir_arg)
    };
    let project = PathBuf::from(project_dir());

    let mut links: Vec<Wikilink> = Vec::new();
    if spec_dir.exists() {
        for (abs, rel) in collect_markdown(&spec_dir) {
            let Ok(content) = fs::read_to_string(&abs) else {
                continue;
            };
            let from = derive_from(&spec_dir, &abs);
            if from.is_empty() {
                continue;
            }
            for (to, line) in extract_links(&content) {
                links.push(Wikilink {
                    from: from.clone(),
                    to,
                    file: rel.clone(),
                    line,
                });
            }
        }
    }

    // Persist (best-effort — emit JSON regardless of DB success).
    if let Some(conn) = open_conn(&project) {
        let _ = wikilinks::upsert_batch(&conn, &links);
    }

    // Orphan detection: every `to` whose name does not match a known spec dir.
    let known = known_specs(&project);
    let mut orphans: BTreeSet<String> = BTreeSet::new();
    if !known.is_empty() {
        for link in &links {
            if !known.contains(&link.to) {
                orphans.insert(link.to.clone());
            }
        }
    }

    let out = json!({
        "wikilinks": links,
        "orphans": orphans.into_iter().collect::<Vec<_>>(),
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".to_string())
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn extracts_basic() {
        let text = "# title\n[[alpha]] and [[beta-1]]\n[[gamma_x]]\n";
        let links = extract_links(text);
        assert_eq!(links.len(), 3);
        assert_eq!(links[0].0, "alpha");
        assert_eq!(links[0].1, 2);
        assert_eq!(links[1].0, "beta-1");
        assert_eq!(links[1].1, 2);
        assert_eq!(links[2].0, "gamma_x");
        assert_eq!(links[2].1, 3);
    }

    #[test]
    fn extracts_ignores_unclosed_and_invalid_chars() {
        // Unclosed `[[`, empty `[[]]`, and a `[[has space]]` are all rejected.
        let text = "[[unclosed\n[[]] [[has space]] [[good]]\n";
        let links = extract_links(text);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].0, "good");
        assert_eq!(links[0].1, 2);
    }

    #[test]
    fn derive_from_uses_parent_dir_for_nested() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("wave-1-rt-infra");
        std::fs::create_dir_all(&nested).unwrap();
        let spec = nested.join("spec.md");
        std::fs::write(&spec, "x").unwrap();
        assert_eq!(derive_from(dir.path(), &spec), "wave-1-rt-infra");
    }

    #[test]
    fn derive_from_uses_file_stem_for_root_md() {
        let dir = tempdir().unwrap();
        let wave_plan = dir.path().join("wave-plan.md");
        std::fs::write(&wave_plan, "x").unwrap();
        assert_eq!(derive_from(dir.path(), &wave_plan), "wave-plan");
    }

    #[test]
    fn collect_markdown_walks_recursively() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "x").unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("b.md"), "x").unwrap();
        std::fs::write(sub.join("c.txt"), "x").unwrap();
        let files = collect_markdown(dir.path());
        let rels: Vec<&str> = files.iter().map(|(_, r)| r.as_str()).collect();
        assert!(rels.contains(&"a.md"));
        assert!(rels.contains(&"sub/b.md"));
        assert!(!rels.iter().any(|r| r.ends_with(".txt")));
    }
}
