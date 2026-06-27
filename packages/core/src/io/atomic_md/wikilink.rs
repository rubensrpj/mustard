//! Pure wikilink functions — extract, resolve, and render Obsidian-style
//! `[[name]]` references from markdown documents.
//!
//! ## Design
//!
//! - **Pure functions, no I/O side effects.** `find_outgoing_links` and
//!   `find_backlinks` are deterministic folds over `&str` / `&[MarkdownDoc]`.
//!   `resolve` and `render_footer` do read the filesystem (directory walk) but
//!   carry no mutable state.
//! - **Single extraction kernel.** One hand-rolled scanner ([`scan_links`])
//!   is the *only* `[[…]]` byte-scanner in the workspace — the concept-graph
//!   (`commands::scan::graph`) and the resolver (`commands::scan::resolve`)
//!   consume it too, layering an id-charset filter on top rather than running
//!   a second scanner. No `regex` crate — the pattern is simple enough that a
//!   two-pointer scan is faster and keeps the dep tree lean.
//! - **Unified resolution.** [`resolve`] maps a `[[token]]` to a file by *two*
//!   keys in one pass over the same `search_dirs`: an exact frontmatter `id:`
//!   match wins first, then a `{token}.md` filename match. This lets a
//!   `[[rt.entity.user]]` reference in a memory/knowledge note resolve to the
//!   concept-graph node whose `id:` is `rt.entity.user`, while a plain
//!   `[[my-spec]]` still resolves by filename.
//! - **Idempotent footer.** `render_footer` locates the sentinel comment pair
//!   `<!-- wikilinks-footer-start -->` / `<!-- wikilinks-footer-end -->`,
//!   replaces the block when present, or appends it when absent. Calling
//!   `render_footer` twice on the same body produces identical output.
//! - **Orphan marking.** Links that cannot be resolved to a real file get a
//!   `⚠ unresolved` annotation in the footer (internal artefact ⇒ English).

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

/// The canonical `[[…]]` byte-scanner. Returns every non-empty inner token in
/// source order, trimmed and with newline-spanning candidates rejected.
///
/// This is the **single** `[[…]]` scanner in the workspace. The concept-graph
/// edge extractor and the resolver's dereference pass both consume this
/// primitive and apply their own id-charset filter on the result — there is no
/// second scanner. Duplicates are preserved; callers that want a set dedup.
#[must_use]
pub fn scan_links(text: &str) -> Vec<String> {
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

/// Resolve a wikilink `token` to a filesystem path by searching `search_dirs`.
///
/// A single recursive walk per directory resolves by **two** keys, with a
/// documented precedence:
///
/// 1. **Frontmatter `id:` exact match** — a markdown file whose YAML header
///    declares `id: {token}` wins first. This lets a concept-graph reference
///    such as `[[rt.entity.user]]` (where the node file may be named anything)
///    resolve to the node whose `id:` is `rt.entity.user`.
/// 2. **Filename match** — `{token}.md` anywhere in the tree.
///
/// Within a directory the walk records the first filename hit but keeps
/// scanning for an id hit, so an `id:` match always beats a filename match even
/// when the filename-named file is encountered first. Directories are searched
/// in the order supplied; the first directory that yields *any* match wins.
///
/// Returns the resolved path, or `None` when neither key matches in any
/// supplied directory.
#[must_use]
pub fn resolve(token: &str, search_dirs: &[&Path]) -> Option<PathBuf> {
    let filename = format!("{token}.md");
    for &dir in search_dirs {
        if let Some(p) = resolve_in_dir(dir, token, &filename) {
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
///   `⚠ unresolved`.
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
                footer.push_str(&format!("- [{name}](?) ⚠ unresolved\n"));
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

/// Recursively walk `dir` resolving `token` by frontmatter `id:` (precedence 1)
/// then by `{token}.md` filename (precedence 2).
///
/// A single walk handles both keys: an exact `id:` match returns immediately
/// (it always wins), while the first filename match is *remembered* and only
/// returned once the whole tree is exhausted without an id hit. This guarantees
/// the documented precedence regardless of directory-entry order.
fn resolve_in_dir(dir: &Path, token: &str, filename: &str) -> Option<PathBuf> {
    let mut filename_hit: Option<PathBuf> = None;
    resolve_walk(dir, token, filename, &mut filename_hit).or(filename_hit)
}

/// Inner walk: returns `Some(path)` immediately on a frontmatter `id:` match;
/// records the first filename match into `filename_hit` as a fallback.
fn resolve_walk(
    dir: &Path,
    token: &str,
    filename: &str,
    filename_hit: &mut Option<PathBuf>,
) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            subdirs.push(path);
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str());
        // Only `.md` files can carry a frontmatter id.
        if name.is_some_and(|n| n.ends_with(".md")) {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if frontmatter_id_matches(&content, token) {
                    return Some(path);
                }
            }
        }
        if filename_hit.is_none() && name == Some(filename) {
            *filename_hit = Some(path);
        }
    }
    for subdir in subdirs {
        if let Some(p) = resolve_walk(&subdir, token, filename, filename_hit) {
            return Some(p);
        }
    }
    None
}

/// `true` when `content` has a leading `---` frontmatter block declaring
/// `id: {token}` (exact, after trimming surrounding whitespace).
fn frontmatter_id_matches(content: &str, token: &str) -> bool {
    let Some(stripped) = content.strip_prefix("---\n") else {
        return false;
    };
    let Some(end) = stripped.find("\n---") else {
        return false;
    };
    stripped[..end].lines().any(|line| {
        line.strip_prefix("id:")
            .is_some_and(|rest| rest.trim() == token)
    })
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

    #[test]
    fn resolves_by_frontmatter_id() {
        // A concept-graph node whose *filename* differs from its `id:` still
        // resolves when referenced by id (`[[rt.entity.user]]`).
        let dir = tempdir().unwrap();
        let node = dir.path().join("node-42.md");
        let mut f = std::fs::File::create(&node).unwrap();
        writeln!(f, "---\nid: rt.entity.user\nkind: entity\n---\n# User").unwrap();
        let result = resolve("rt.entity.user", &[dir.path()]);
        assert!(result.is_some(), "id-frontmatter match must resolve");
        assert_eq!(result.unwrap().file_name().unwrap(), "node-42.md");
    }

    #[test]
    fn resolve_prefers_id_over_filename() {
        // Two candidates: `{token}.md` by name, and another file whose
        // frontmatter id equals the token. Documented precedence: id wins.
        let dir = tempdir().unwrap();
        // Filename match (no id), would-be precedence-2 winner.
        write_file(dir.path(), "rt.entity.user.md");
        // Id match under a different filename — must beat the filename hit.
        let by_id = dir.path().join("canonical.md");
        let mut f = std::fs::File::create(&by_id).unwrap();
        writeln!(f, "---\nid: rt.entity.user\n---\n# canonical").unwrap();

        let result = resolve("rt.entity.user", &[dir.path()]).expect("resolves");
        assert_eq!(
            result.file_name().unwrap(),
            "canonical.md",
            "frontmatter id must take precedence over filename"
        );
    }

    #[test]
    fn resolve_falls_back_to_filename_without_id() {
        // No frontmatter id anywhere — plain filename resolution still works.
        let dir = tempdir().unwrap();
        write_file(dir.path(), "my-note.md");
        let result = resolve("my-note", &[dir.path()]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().file_name().unwrap(), "my-note.md");
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
        assert!(result.contains("⚠ unresolved"));
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
    #[ignore = "wall-clock perf microbenchmark: avg <2 ms is flaky under the parallel test load of the close gate (CPU contention spikes the average). Run explicitly with `cargo test -- --ignored`."]
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
