//! `MarkdownStore` — the shared atomic markdown I/O layer.
//!
//! ## Responsibilities
//!
//! - **`scan_dir`** — discover every `*.md` in a directory tree, parse each
//!   file's YAML frontmatter eagerly, leave the body as an empty `String` (lazy
//!   body: callers that need the full text call [`MarkdownStore::read_one`]).
//! - **`read_one`** — read a single file's full text, parse frontmatter and
//!   return a complete [`MarkdownDoc`].
//! - **`write_atomic`** — serialise a [`MarkdownDoc`] back to disk via a
//!   sibling-tempfile + rename (using the canonical [`crate::io::fs::write_atomic`]
//!   seam) so a crash mid-write never leaves a torn file.
//!
//! ## Design
//!
//! - **No trait** — this is a concrete struct per the W1C spec constraint.
//! - **Fail-open.** Files that cannot be read are silently skipped in
//!   `scan_dir`; `read_one` returns `std::io::Result` so callers can decide.
//! - **Parallelism.** When the directory contains >50 `.md` files the scan
//!   switches to `rayon::par_iter` for the parse+frontmatter step.  With ≤50
//!   files a sequential walk is fast enough and avoids rayon's thread-pool
//!   overhead.

use super::frontmatter::{self, Frontmatter};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(feature = "rayon")]
use rayon::prelude::*;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A parsed markdown document.
///
/// - `path` — absolute (or caller-relative) path to the `.md` file.
/// - `frontmatter` — parsed YAML header, or `None` when absent / invalid.
/// - `body` — the document body after the frontmatter fence.
///   **May be an empty string** when the doc was returned by [`MarkdownStore::scan_dir`];
///   call [`MarkdownStore::read_one`] to populate the body.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkdownDoc {
    /// Path to the source `.md` file.
    pub path: PathBuf,
    /// YAML frontmatter, if present and parseable.
    pub frontmatter: Option<Frontmatter>,
    /// Document body (after the frontmatter fence).
    /// Empty when produced by the lazy `scan_dir` path.
    pub body: String,
}

impl MarkdownDoc {
    /// Serialise this document back to a UTF-8 string suitable for writing to
    /// disk.
    ///
    /// If `frontmatter` is `Some`, the output starts with the YAML block
    /// (reconstructed from the JSON object) between `---` fences. The `body`
    /// follows immediately.
    #[must_use]
    pub fn to_markdown(&self) -> String {
        match &self.frontmatter {
            None => self.body.clone(),
            Some(fm) => {
                let mut out = String::from("---\n");
                if let Some(obj) = fm.as_object() {
                    for (k, v) in obj {
                        let val_str = match v {
                            serde_json::Value::String(s) => s.clone(),
                            serde_json::Value::Array(arr) => {
                                let joined: Vec<String> = arr
                                    .iter()
                                    .filter_map(|x| x.as_str().map(str::to_string))
                                    .collect();
                                format!("[{}]", joined.join(", "))
                            }
                            serde_json::Value::Null => String::new(),
                            other => other.to_string(),
                        };
                        out.push_str(&format!("{k}: {val_str}\n"));
                    }
                }
                out.push_str("---\n");
                out.push_str(&self.body);
                out
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

/// Stateless markdown store — all state lives in the returned `Vec<MarkdownDoc>`
/// or on disk. The struct exists to group the three operations under a common
/// namespace.
#[derive(Debug, Default)]
pub struct MarkdownStore;

impl MarkdownStore {
    /// Create a new (stateless) store handle.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Recursively discover every `*.md` file under `dir`.
    ///
    /// Frontmatter is parsed eagerly; the body is **not** read (it is set to
    /// `String::new()`). Files that cannot be read are silently skipped.
    ///
    /// When the discovered file count exceeds 50, the parse step runs in
    /// parallel via `rayon` (requires the `rayon` feature).
    #[must_use]
    pub fn scan_dir(dir: &Path) -> Vec<MarkdownDoc> {
        let paths = collect_md_paths(dir);
        if paths.is_empty() {
            return Vec::new();
        }

        #[cfg(feature = "rayon")]
        if paths.len() > 50 {
            return paths
                .par_iter()
                .filter_map(|p| parse_header_only(p))
                .collect();
        }

        paths.iter().filter_map(|p| parse_header_only(p)).collect()
    }

    /// Read one `.md` file in full (frontmatter + body).
    ///
    /// # Errors
    ///
    /// Returns an [`io::Error`] when the file cannot be read.
    pub fn read_one(path: &Path) -> io::Result<MarkdownDoc> {
        let text = fs::read_to_string(path)?;
        let (fm, body) = frontmatter::parse(&text);
        Ok(MarkdownDoc {
            path: path.to_path_buf(),
            frontmatter: fm,
            body: body.to_string(),
        })
    }

    /// Write `doc` to `path` atomically (sibling temp-file + `fs::rename`).
    ///
    /// Uses the canonical [`crate::io::fs::write_atomic`] seam.
    ///
    /// # Errors
    ///
    /// Returns an [`io::Error`] when the write or rename fails.
    pub fn write_atomic(path: &Path, doc: &MarkdownDoc) -> io::Result<()> {
        let content = doc.to_markdown();
        // Delegate to the canonical atomic-write primitive in crate::io::fs.
        crate::io::fs::write_atomic(path, content.as_bytes())
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// Walk `dir` recursively, collecting every `*.md` path.
fn collect_md_paths(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_md_paths_inner(dir, &mut out);
    out
}

fn collect_md_paths_inner(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_md_paths_inner(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path);
        }
    }
}

/// Read just enough of `path` to parse the frontmatter header; body is empty.
fn parse_header_only(path: &Path) -> Option<MarkdownDoc> {
    let text = fs::read_to_string(path).ok()?;
    let (fm, _body) = frontmatter::parse(&text);
    Some(MarkdownDoc {
        path: path.to_path_buf(),
        frontmatter: fm,
        body: String::new(),
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

    fn write_md(dir: &Path, name: &str, content: &str) {
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    #[test]
    fn scan_dir_finds_md_files() {
        let dir = tempdir().unwrap();
        write_md(dir.path(), "a.md", "---\nstage: Execute\n---\nbody");
        write_md(dir.path(), "b.md", "no frontmatter");
        let docs = MarkdownStore::scan_dir(dir.path());
        assert_eq!(docs.len(), 2);
        let a = docs.iter().find(|d| d.path.file_name().unwrap() == "a.md").unwrap();
        assert_eq!(
            a.frontmatter.as_ref().and_then(|f| f.get_str("stage")),
            Some("Execute")
        );
        assert!(a.body.is_empty(), "scan_dir leaves body empty");
    }

    #[test]
    fn read_one_populates_body() {
        let dir = tempdir().unwrap();
        write_md(dir.path(), "spec.md", "---\nstage: Plan\n---\n## Body\nContent here.\n");
        let path = dir.path().join("spec.md");
        let doc = MarkdownStore::read_one(&path).unwrap();
        assert!(doc.body.contains("Content here."));
        assert_eq!(
            doc.frontmatter.as_ref().and_then(|f| f.get_str("stage")),
            Some("Plan")
        );
    }

    #[test]
    fn write_atomic_round_trips() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("out.md");
        let doc = MarkdownDoc {
            path: path.clone(),
            frontmatter: None,
            body: "Hello, world!\n".to_string(),
        };
        MarkdownStore::write_atomic(&path, &doc).unwrap();
        let read_back = fs::read_to_string(&path).unwrap();
        assert_eq!(read_back, "Hello, world!\n");
    }

    /// Timing benchmark: 200 `.md` files, cold scan must finish < 100 ms.
    ///
    /// This test uses `std::time::Instant` — it is advisory (a slow CI machine
    /// may take longer) but catches pathological regressions locally.
    #[test]
    fn bench_scan_200_files_under_100ms() {
        let dir = tempdir().unwrap();
        for i in 0..200u32 {
            write_md(
                dir.path(),
                &format!("doc-{i:03}.md"),
                &format!("---\nindex: {i}\ntitle: Doc {i}\n---\n## Body {i}\nSome content here.\n"),
            );
        }
        let start = std::time::Instant::now();
        let docs = MarkdownStore::scan_dir(dir.path());
        let elapsed = start.elapsed();
        assert_eq!(docs.len(), 200);
        assert!(
            elapsed.as_millis() < 100,
            "scan_dir of 200 files took {elapsed:?} (limit: 100 ms)"
        );
    }
}
