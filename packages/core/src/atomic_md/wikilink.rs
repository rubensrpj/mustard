//! Pure wikilink functions — extract, resolve, and render Obsidian-style
//! `[[name]]` references from markdown documents.
//!
//! ## Design
//!
//! - **Pure functions, no I/O side effects.** `find_outgoing_links` and
//!   `find_backlinks` are deterministic folds over `&str` / `&[MarkdownDoc]`.
//!   `resolve` and `render_footer` do read the filesystem (directory walk) but
//!   carry no mutable state.
//! - **Single extraction kernel.** One hand-rolled scanner (`scan_links`)
//!   recognises `[[name]]` tokens. No `regex` crate — the pattern is simple
//!   enough that a two-pointer scan is faster and keeps the dep tree lean.
//! - **Idempotent footer.** `render_footer` locates the sentinel comment pair
//!   `<!-- wikilinks-footer-start -->` / `<!-- wikilinks-footer-end -->`,
//!   replaces the block when present, or appends it when absent. Calling
//!   `render_footer` twice on the same body produces identical output.
//! - **Orphan marking.** Links that cannot be resolved to a real file get a
//!   `⚠ não resolvido` annotation in the footer.

use super::store::MarkdownDoc;
use std::path::{Path, PathBuf};

// Sentinel strings used to delimit the auto-generated footer block.
const FOOTER_START: &str = "<!-- wikilinks-footer-start -->";
const FOOTER_END: &str = "<!-- wikilinks-footer-end -->";

// ---------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------

/// Extract every `[[name]]` wikilink from `body`, returning the inner text.
///
/// Duplicates are preserved (callers that need a set can dedup). The scan is
/// O(n) in `body` length.
#[must_use]
pub fn find_outgoing_links(body: &str) -> Vec<String> {
    scan_links(body)
}

/// Find every document in `docs` that contains a `[[target]]` outgoing link.
///
/// Returns the paths of matching documents. The search is a linear scan over
/// `docs` — callers with large collections should pre-filter by directory.
#[must_use]
pub fn find_backlinks(target: &str, docs: &[MarkdownDoc]) -> Vec<PathBuf> {
    docs.iter()
        .filter(|doc| {
            // Use the pre-parsed body when available; skip docs with empty
            // bodies (lazy scan_dir result) by returning false — the caller
            // must call read_one first if they need full-body backlinks.
            !doc.body.is_empty() && find_outgoing_links(&doc.body).iter().any(|l| l == target)
        })
        .map(|doc| doc.path.clone())
        .collect()
}

/// Resolve a wikilink `name` to a filesystem path by searching `search_dirs`.
///
/// For each directory the function tries:
/// 1. `{dir}/{name}.md` — direct child.
/// 2. A recursive walk into subdirectories (one level at a time via `std::fs`).
///
/// Returns the first match found, or `None` when the file does not exist in
/// any of the supplied directories.
#[must_use]
pub fn resolve(name: &str, search_dirs: &[&Path]) -> Option<PathBuf> {
    let target = format!("{name}.md");
    for &dir in search_dirs {
        if let Some(p) = find_file_recursive(dir, &target) {
            return Some(p);
        }
    }
    None
}

/// Generate (or replace) the wikilinks footer block at the end of `body`.
///
/// - When `body` contains no `[[…]]` links, the function returns the body
///   stripped of any existing footer block (or unchanged if none existed).
/// - When links are present, each is resolved against `search_dirs` and
///   rendered as a markdown list item. Unresolvable links are annotated with
///   `⚠ não resolvido`.
/// - The function is **idempotent**: calling it twice on the same string
///   produces the same output.
#[must_use]
pub fn render_footer(body: &str, search_dirs: &[&Path]) -> String {
    // Strip any existing footer block first (idempotence).
    let stripped = strip_footer(body);
    let links = find_outgoing_links(&stripped);

    if links.is_empty() {
        // No links — return body without footer (and without the block if it
        // was previously present).
        return stripped.trim_end().to_string();
    }

    // Deduplicate while preserving order.
    let mut seen = std::collections::HashSet::new();
    let unique: Vec<&str> = links
        .iter()
        .map(String::as_str)
        .filter(|&l| seen.insert(l))
        .collect();

    let mut footer = String::from("\n\n");
    footer.push_str(FOOTER_START);
    footer.push('\n');
    for name in unique {
        match resolve(name, search_dirs) {
            Some(abs_path) => {
                // Emit a relative-looking path using the file name only when
                // the caller supplied no common base. Full path is always
                // correct for absolute consumers; for human-readable output
                // just the file name is cleaner.
                let display = abs_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(name);
                footer.push_str(&format!("- [{name}]({display})\n"));
            }
            None => {
                footer.push_str(&format!("- [{name}](?) ⚠ não resolvido\n"));
            }
        }
    }
    footer.push_str(FOOTER_END);

    let base = stripped.trim_end();
    format!("{base}{footer}")
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// Two-pointer `[[…]]` scanner. Returns every non-empty inner text in order.
fn scan_links(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 3 < len {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            // Search for closing `]]`.
            let start = i + 2;
            let mut j = start;
            while j + 1 < len {
                if bytes[j] == b']' && bytes[j + 1] == b']' {
                    let inner = &text[start..j];
                    if !inner.is_empty() && !inner.contains('\n') {
                        out.push(inner.trim().to_string());
                    }
                    i = j + 2;
                    break;
                }
                j += 1;
            }
            if j + 1 >= len {
                break; // unclosed — stop
            }
        } else {
            i += 1;
        }
    }
    out
}

/// Strip the `<!-- wikilinks-footer-start -->…<!-- wikilinks-footer-end -->`
/// block (and the two blank lines before it) from `body`.
fn strip_footer(body: &str) -> String {
    if let (Some(start), Some(end)) = (body.find(FOOTER_START), body.find(FOOTER_END)) {
        if start < end {
            let before = &body[..start];
            let after_end = end + FOOTER_END.len();
            let after = body.get(after_end..).unwrap_or("");
            // Trim trailing whitespace/newlines from the "before" part.
            let before_trimmed = before.trim_end();
            if after.trim().is_empty() {
                return before_trimmed.to_string();
            }
            return format!("{before_trimmed}\n{}", after.trim_start());
        }
    }
    body.to_string()
}

/// Recursively walk `dir` looking for a file named `target` (exact match,
/// case-sensitive). Returns the first hit.
fn find_file_recursive(dir: &Path, target: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            subdirs.push(path);
        } else if path.file_name().and_then(|n| n.to_str()) == Some(target) {
            return Some(path);
        }
    }
    for subdir in subdirs {
        if let Some(p) = find_file_recursive(&subdir, target) {
            return Some(p);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use tempfile::tempdir;

    fn make_doc(body: &str) -> MarkdownDoc {
        MarkdownDoc {
            path: PathBuf::from("test.md"),
            frontmatter: None,
            body: body.to_string(),
        }
    }

    fn write_file(dir: &Path, name: &str) {
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "# {name}").unwrap();
    }

    // --- find_outgoing_links ---

    #[test]
    fn extracts_single_link() {
        let links = find_outgoing_links("See [[my-spec]] for details.");
        assert_eq!(links, vec!["my-spec"]);
    }

    #[test]
    fn extracts_multiple_links() {
        let links = find_outgoing_links("[[a]] and [[b]] and [[c]].");
        assert_eq!(links, vec!["a", "b", "c"]);
    }

    #[test]
    fn no_links_returns_empty() {
        let links = find_outgoing_links("No brackets here.");
        assert!(links.is_empty());
    }

    #[test]
    fn ignores_unclosed_bracket() {
        let links = find_outgoing_links("[[open but never closed");
        assert!(links.is_empty());
    }

    // --- find_backlinks ---

    #[test]
    fn finds_backlinks_across_docs() {
        let docs = vec![
            make_doc("Links to [[target-spec]] here."),
            make_doc("No links."),
            make_doc("Also links [[target-spec]] again."),
        ];
        let backlinks = find_backlinks("target-spec", &docs);
        assert_eq!(backlinks.len(), 2);
    }

    #[test]
    fn backlinks_timing_200_docs() {
        // find_backlinks over 200 docs must complete < 30 ms.
        let docs: Vec<MarkdownDoc> = (0..200u32)
            .map(|i| make_doc(&format!("Doc {i} links [[some-target]] here.")))
            .collect();
        let start = std::time::Instant::now();
        let hits = find_backlinks("some-target", &docs);
        let elapsed = start.elapsed();
        assert_eq!(hits.len(), 200);
        assert!(
            elapsed.as_millis() < 30,
            "find_backlinks took {elapsed:?} (limit: 30 ms)"
        );
    }

    // --- resolve ---

    #[test]
    fn resolves_direct_child() {
        let dir = tempdir().unwrap();
        write_file(dir.path(), "my-spec.md");
        let result = resolve("my-spec", &[dir.path()]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().file_name().unwrap(), "my-spec.md");
    }

    #[test]
    fn resolves_nested_file() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        write_file(&sub, "nested.md");
        let result = resolve("nested", &[dir.path()]);
        assert!(result.is_some());
    }

    #[test]
    fn returns_none_for_missing() {
        let dir = tempdir().unwrap();
        let result = resolve("nonexistent", &[dir.path()]);
        assert!(result.is_none());
    }

    // --- render_footer ---

    #[test]
    fn test_render_footer_resolved_link() {
        let dir = tempdir().unwrap();
        write_file(dir.path(), "spec-a.md");
        let body = "See [[spec-a]] here.";
        let result = render_footer(body, &[dir.path()]);
        assert!(result.contains(FOOTER_START));
        assert!(result.contains("[spec-a](spec-a.md)"));
        assert!(result.contains(FOOTER_END));
    }

    #[test]
    fn test_render_footer_orphan_link() {
        let dir = tempdir().unwrap();
        let body = "References [[ghost-spec]] which does not exist.";
        let result = render_footer(body, &[dir.path()]);
        assert!(result.contains("⚠ não resolvido"));
        assert!(result.contains("[ghost-spec](?)"));
    }

    #[test]
    fn test_render_footer_no_links_returns_body_unchanged() {
        let dir = tempdir().unwrap();
        let body = "No wikilinks here.";
        let result = render_footer(body, &[dir.path()]);
        assert!(!result.contains(FOOTER_START));
        assert_eq!(result.trim(), body.trim());
    }

    #[test]
    fn test_render_footer_idempotent() {
        let dir = tempdir().unwrap();
        write_file(dir.path(), "target.md");
        let body = "Link to [[target]] here.";
        let first = render_footer(body, &[dir.path()]);
        let second = render_footer(&first, &[dir.path()]);
        assert_eq!(first, second, "render_footer must be idempotent");
    }

    #[test]
    fn test_render_footer_replaces_existing_block() {
        let dir = tempdir().unwrap();
        write_file(dir.path(), "spec-b.md");
        // Pre-baked body that already has a stale footer.
        let body = format!(
            "Link to [[spec-b]].\n\n{FOOTER_START}\n- [old-entry](?)\n{FOOTER_END}"
        );
        let result = render_footer(&body, &[dir.path()]);
        // The old entry must be gone; the new one present.
        assert!(!result.contains("old-entry"));
        assert!(result.contains("[spec-b](spec-b.md)"));
        // Exactly one start sentinel.
        assert_eq!(result.matches(FOOTER_START).count(), 1);
    }

    #[test]
    fn render_footer_timing_under_2ms() {
        // render_footer on a realistic body must finish < 2 ms.
        let dir = tempdir().unwrap();
        write_file(dir.path(), "ref.md");
        let body = "Body with [[ref]] link.\n";
        let start = std::time::Instant::now();
        for _ in 0..100 {
            let _ = render_footer(body, &[dir.path()]);
        }
        let avg = start.elapsed() / 100;
        assert!(
            avg.as_millis() < 2,
            "render_footer avg {avg:?} exceeded 2 ms"
        );
    }
}
