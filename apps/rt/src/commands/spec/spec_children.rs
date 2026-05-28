//! `mustard-rt run spec-children --parent <slug>` — list sub-specs of a
//! parent spec, discovered via `### Parent: <slug>` headers in
//! `.claude/spec/*/spec.md`.
//!
//! Filesystem-only discovery
//! -------------------------
//!
//! W4A migration: the SQLite branch (`SqliteSpecReader::children_of` +
//! `correlate_waves`) was removed. Sub-spec discovery is now purely
//! header-driven — filesystem-versioned, cross-developer canonical, durable
//! across `git pull`. Wave correlation against the parent's `pipeline.wave.*`
//! timeline is OUT-OF-SCOPE here (it relied on SQLite-only `started_at` /
//! `completed_at`); a follow-up may reintroduce correlation via NDJSON walk.
//!
//! Every entry surfaces with `source = Header`. `started_at` / `completed_at`
//! / `reason` / `wave` default to `None`.
//!
//! Fail-open: any I/O failure silently degrades to an empty result. The
//! subcommand always emits valid JSON.

use mustard_core::io::fs;
use mustard_core::domain::spec;
use mustard_core::ClaudePaths;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Which source identified this child.
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChildSource {
    /// Discovered via `### Parent:` in the child's `spec.md` header.
    Header,
}

/// One sub-spec linked to a parent — header-driven entry used by the
/// dashboard.
#[derive(Serialize, Debug, Clone)]
pub struct ChildEntry {
    /// Child spec slug (the directory name under `.claude/spec/`).
    pub spec: String,
    /// Lifecycle status in kebab-case (matches the on-disk `### Status:`
    /// spelling, e.g. `planning`, `implementing`, `completed`). Falls back
    /// to `"unknown"` when the header has no status.
    pub status: String,
    /// First-event timestamp (ISO-8601). Header-only entries default `None`.
    pub started_at: Option<String>,
    /// Terminal-event timestamp (ISO-8601). Header-only entries default `None`.
    pub completed_at: Option<String>,
    /// Free-form `spec.link` payload reason (e.g. `"tactical-fix"`).
    /// Header-only entries default `None`.
    pub reason: Option<String>,
    /// Which source produced this entry — always `Header` post-W4A.
    pub source: ChildSource,
    /// Wave attribution. Header-only entries default `None` (no `started_at`
    /// to correlate against parent wave windows).
    pub wave: Option<u32>,
}

/// Strip surrounding `[[ ]]` from a wikilink target. Leaves any other text
/// untouched. Whitespace inside the brackets is trimmed.
fn strip_wikilink(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(inner) = trimmed.strip_prefix("[[").and_then(|s| s.strip_suffix("]]")) {
        inner.trim().to_string()
    } else {
        trimmed.to_string()
    }
}

/// Parse the `### Parent:` link and the lifecycle status out of a `spec.md`'s
/// leading window. Returns `(parent_slug, status_kebab_opt)` when a parent
/// header is found, else `None`.
///
/// The parent slug is normalised (surrounding `[[wikilink]]` brackets are
/// stripped). The status is resolved through the canonical
/// [`mustard_core::domain::spec`] parser — so the new `### Stage:`/`### Outcome:`
/// header *and* every legacy `### Status:` shape are understood — and projected
/// to the kebab-case status word the dashboard's sub-spec rows expect. A spec
/// with a `### Parent:` but no lifecycle header surfaces `status = None`
/// (callers default it to `"unknown"`).
fn parse_header_window(window: &str) -> Option<(String, Option<String>)> {
    let parent = spec::header_field(window, "Parent")
        .map(|raw| strip_wikilink(&raw))
        .filter(|s| !s.is_empty())?;
    let status = spec::parse_state(window).map(|st| spec::status_word(&st).to_string());
    Some((parent, status))
}

/// Read at most the first `cap` bytes of a file as UTF-8 (lossy on invalid
/// sequences). Returns `None` on any I/O failure.
fn read_header_window(path: &Path, cap: usize) -> Option<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; cap];
    let n = file.read(&mut buf).ok()?;
    buf.truncate(n);
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Scan `<project>/.claude/spec/*/spec.md`, returning every child slug whose
/// header declares `### Parent: <parent>` (raw or wikilinked). The returned
/// [`ChildEntry`] rows are tagged [`ChildSource::Header`] and carry the
/// status read from the header when present (else `"unknown"`).
///
/// Fail-open: missing `.claude/spec/` directory yields an empty result; per-
/// file I/O errors are silently skipped.
fn scan_filesystem(project: &Path, parent: &str) -> Vec<ChildEntry> {
    let Ok(paths) = ClaudePaths::for_project(project) else {
        return Vec::new();
    };
    let spec_root = paths.spec_dir();
    let Ok(entries) = fs::read_dir(&spec_root) else {
        return Vec::new();
    };
    let mut out: Vec<ChildEntry> = Vec::new();
    // Cap at 4 KiB — header section is always near the top of a spec.md.
    const HEADER_CAP: usize = 4096;
    for entry in entries {
        let dir_path = &entry.path;
        if !entry.is_dir {
            continue;
        }
        let spec_md = dir_path.join("spec.md");
        if !spec_md.is_file() {
            continue;
        }
        let Some(window) = read_header_window(&spec_md, HEADER_CAP) else {
            continue;
        };
        let Some((found_parent, status_opt)) = parse_header_window(&window) else {
            continue;
        };
        if found_parent != parent {
            continue;
        }
        let slug = entry.file_name.clone();
        out.push(ChildEntry {
            spec: slug,
            status: status_opt.unwrap_or_else(|| "unknown".to_string()),
            started_at: None,
            completed_at: None,
            reason: None,
            source: ChildSource::Header,
            wave: None,
        });
    }
    out
}

/// List sub-specs of `parent` — header-driven discovery.
///
/// W4A: SQLite-backed Set A (events) and wave correlation removed. Every
/// row is `ChildSource::Header`; output is sorted by slug ascending for
/// byte-stability.
#[must_use]
pub fn list_children(project: &Path, parent: &str) -> Vec<ChildEntry> {
    if parent.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<ChildEntry> = scan_filesystem(project, parent);
    out.sort_by(|a, b| a.spec.cmp(&b.spec));
    out
}

/// Dispatch `mustard-rt run spec-children --parent <slug>`. Emits the
/// resulting `Vec<ChildEntry>` as JSON to stdout. Fail-open: any error path
/// degrades to `[]` and exit `0`.
pub fn run(parent: Option<&str>) {
    let Some(parent) = parent else {
        eprintln!("Usage: mustard-rt run spec-children --parent <slug>");
        println!("[]");
        return;
    };
    let project = PathBuf::from(crate::shared::context::project_dir());
    let entries = list_children(&project, parent);
    match serde_json::to_string(&entries) {
        Ok(text) => println!("{text}"),
        Err(_) => println!("[]"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_spec(project: &Path, slug: &str, body: &str) {
        let spec_dir = project.join(".claude").join("spec").join(slug);
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), body).unwrap();
    }

    #[test]
    fn returns_header_only_entry() {
        let td = tempdir().unwrap();
        write_spec(
            td.path(),
            "child-a",
            "# Child A\n\n### Parent: parent-x\n### Status: completed\n",
        );
        let result = list_children(td.path(), "parent-x");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].spec, "child-a");
        assert_eq!(result[0].source, ChildSource::Header);
        assert_eq!(result[0].status, "completed");
        assert!(result[0].started_at.is_none());
        assert!(result[0].completed_at.is_none());
        assert!(result[0].reason.is_none());
        assert!(result[0].wave.is_none());
    }

    #[test]
    fn skips_unrelated_parents() {
        let td = tempdir().unwrap();
        write_spec(
            td.path(),
            "child-other",
            "# Child Other\n\n### Parent: parent-y\n",
        );
        let result = list_children(td.path(), "parent-x");
        assert!(result.is_empty(), "expected no entries for parent-x");
    }

    #[test]
    fn accepts_wikilinked_parent() {
        let td = tempdir().unwrap();
        write_spec(
            td.path(),
            "child-b",
            "# Child B\n\n### Parent: [[parent-x]]\n### Status: planning\n",
        );
        let result = list_children(td.path(), "parent-x");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].spec, "child-b");
        assert_eq!(result[0].source, ChildSource::Header);
        assert_eq!(result[0].status, "planning");
    }

    #[test]
    fn defaults_status_to_unknown_when_header_missing_status() {
        let td = tempdir().unwrap();
        write_spec(
            td.path(),
            "child-c",
            "# Child C\n\n### Parent: parent-x\n",
        );
        let result = list_children(td.path(), "parent-x");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].status, "unknown");
    }

    #[test]
    fn sorts_entries_by_slug() {
        let td = tempdir().unwrap();
        write_spec(td.path(), "z-child", "### Parent: p\n");
        write_spec(td.path(), "a-child", "### Parent: p\n");
        write_spec(td.path(), "m-child", "### Parent: p\n");
        let result = list_children(td.path(), "p");
        let slugs: Vec<&str> = result.iter().map(|e| e.spec.as_str()).collect();
        assert_eq!(slugs, vec!["a-child", "m-child", "z-child"]);
    }

    #[test]
    fn empty_parent_returns_empty_vec() {
        let td = tempdir().unwrap();
        write_spec(td.path(), "child", "### Parent: anything\n");
        let result = list_children(td.path(), "");
        assert!(result.is_empty());
    }

    #[test]
    fn parse_header_window_strips_wikilink() {
        let window = "# Title\n\n### Parent: [[my-parent]]\n### Status: draft\n";
        let parsed = parse_header_window(window).expect("should parse");
        assert_eq!(parsed.0, "my-parent");
        // `draft` maps to `planning` via `SpecStatus::parse`.
        assert_eq!(parsed.1.as_deref(), Some("planning"));
    }

    #[test]
    fn parse_header_window_unknown_status_degrades_to_none() {
        let window = "### Parent: p\n### Status: weird-status\n";
        let parsed = parse_header_window(window).expect("should parse");
        assert_eq!(parsed.0, "p");
        assert_eq!(parsed.1, None);
    }

    #[test]
    fn parse_header_window_returns_none_without_parent() {
        let window = "# Top-level spec\n\n### Status: planning\n";
        assert!(parse_header_window(window).is_none());
    }
}
