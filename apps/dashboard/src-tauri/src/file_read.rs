//! Safe, read-only file reader for the dashboard's code viewer.
//!
//! [`dashboard_read_file`] opens an arbitrary repository file the viewer points
//! at (from the Git card, the "most touched" list, README/CLAUDE.md, the
//! tracer) and returns its text plus enough metadata for the frontend to drive
//! syntax highlighting. It is a sibling of the other dashboard projections and
//! follows the same conventions: snake_case serde shape, `spawn_blocking` so a
//! large read never freezes the UI thread, and the FAIL-OPEN CONTRACT — a
//! missing file, a binary file, or a path that escapes the repo never surfaces
//! as an `Err` toast; each degrades to an `Ok(FileContent { readable: false })`
//! so the viewer renders an empty / "not available" state instead. `Err` is
//! reserved for an unrecoverable runtime fault (a join panic), never per-file.
//!
//! PATH SAFETY (critical): `rel_path` is resolved relative to `repo_path` and
//! both ends are canonicalized; the canonical target MUST stay inside the
//! canonical repo root (`starts_with`). Any `..` traversal that escapes the
//! repo is rejected with `readable: false` — a file outside the repo is never
//! exposed. Both `/` and `\` separators are accepted in `rel_path`.

use serde::Serialize;
use std::path::{Path, PathBuf};

/// Read text only up to ~1 MiB; a larger file is truncated to this prefix and
/// flagged `truncated: true`.
const MAX_BYTES: usize = 1024 * 1024;

/// Bytes inspected for the binary sniff (null byte / invalid UTF-8). 8 KiB is
/// the conventional "is this text?" window.
const SNIFF_BYTES: usize = 8 * 1024;

/// Read-only projection of one repository file for the code viewer. Every field
/// defaults to its empty form so a rejected / missing / binary file renders as
/// an empty state (`readable: false`) rather than an error toast.
#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct FileContent {
    /// The file's UTF-8 text, possibly truncated to [`MAX_BYTES`]. Empty when
    /// the file is binary, missing, or out of scope.
    pub content: String,
    /// Lowercase file extension with no leading dot (e.g. `rs`, `tsx`, `json`),
    /// for the frontend to map onto its Prism grammar. Empty when the file has
    /// no extension.
    pub language: String,
    /// On-disk size of the file in bytes (the full size, even when `content`
    /// was truncated). 0 when unknown.
    pub size_bytes: u64,
    /// `true` when the file exceeded [`MAX_BYTES`] and `content` holds only the
    /// leading prefix.
    pub truncated: bool,
    /// `true` when the file looked binary (a null byte in the first
    /// [`SNIFF_BYTES`], or invalid UTF-8). `content` is empty in that case — the
    /// viewer never receives raw binary.
    pub is_binary: bool,
    /// `true` only when text was read successfully. A missing file, a path that
    /// escaped the repo, a binary file, or any IO error all yield `false`.
    pub readable: bool,
}

/// Read `rel_path` (relative to `repo_path`) as text for the code viewer.
/// Always returns `Ok`; a missing file / traversal escape / binary file
/// degrades to `Ok(FileContent { readable: false, .. })` — never an `Err`
/// toast. `Err` is reserved for an unrecoverable runtime fault.
///
/// The path arrives from the front as `relPath`/`repoPath` (camelCase); Tauri
/// maps them to the snake_case arguments automatically.
#[tauri::command]
pub async fn dashboard_read_file(
    repo_path: String,
    rel_path: String,
) -> Result<FileContent, String> {
    // A join error (panic in the closure) degrades to an unreadable result,
    // never an Err toast — the failure-tolerant contract.
    let content =
        tauri::async_runtime::spawn_blocking(move || read_file_impl(&repo_path, &rel_path))
            .await
            .unwrap_or_default();
    Ok(content)
}

/// Resolve `rel_path` against `repo_path` and confirm the canonical target
/// stays inside the canonical repo root. Returns the validated absolute path,
/// or `None` when the repo is missing or the target escapes it (traversal).
/// Accepts both `/` and `\` separators in `rel_path`.
fn resolve_within_repo(repo_path: &str, rel_path: &str) -> Option<PathBuf> {
    // Canonicalize the repo root first; a non-existent repo is out of scope.
    let repo_root = Path::new(repo_path).canonicalize().ok()?;
    // Normalize separators so a `\`-style rel_path works on Unix test hosts and
    // a `/`-style one works everywhere. Then strip any leading separator so the
    // join treats it as relative, not absolute.
    let normalized = rel_path.replace('\\', "/");
    let trimmed = normalized.trim_start_matches('/');
    let candidate = repo_root.join(trimmed);
    // Canonicalize the candidate so `..` segments and symlinks are resolved
    // before the containment check — the only sound way to reject traversal.
    let resolved = candidate.canonicalize().ok()?;
    if resolved.starts_with(&repo_root) {
        Some(resolved)
    } else {
        None
    }
}

/// Derive the Prism language token from a path's extension: lowercase, no dot.
/// Empty when the file has no extension.
fn language_from_path(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// Synchronous body of [`dashboard_read_file`], kept separate so unit tests call
/// it directly without a Tauri runtime. Never panics on a missing/binary file;
/// every failure path returns an unreadable [`FileContent`].
fn read_file_impl(repo_path: &str, rel_path: &str) -> FileContent {
    // Path safety gate: out-of-repo / traversal / missing repo → not readable.
    let Some(path) = resolve_within_repo(repo_path, rel_path) else {
        return FileContent::default();
    };

    let language = language_from_path(&path);

    // Read the raw bytes (capped at MAX_BYTES + 1 so we can tell "exactly the
    // cap" from "larger than the cap"). A missing file / IO error is not
    // readable, never an Err.
    let raw = match read_capped(&path, MAX_BYTES + 1) {
        Some(bytes) => bytes,
        None => {
            return FileContent {
                language,
                ..Default::default()
            };
        }
    };

    // True on-disk size for the metadata, independent of how much we read.
    let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

    let truncated = raw.len() > MAX_BYTES;
    let bytes = if truncated { &raw[..MAX_BYTES] } else { &raw[..] };

    // Binary sniff: a null byte in the first SNIFF_BYTES marks binary outright;
    // otherwise non-UTF-8 content does. Binary files return empty content.
    let sniff_len = bytes.len().min(SNIFF_BYTES);
    if bytes[..sniff_len].contains(&0) {
        return FileContent {
            language,
            size_bytes,
            is_binary: true,
            ..Default::default()
        };
    }
    let content = match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => {
            return FileContent {
                language,
                size_bytes,
                is_binary: true,
                ..Default::default()
            };
        }
    };

    FileContent {
        content,
        language,
        size_bytes,
        truncated,
        is_binary: false,
        readable: true,
    }
}

/// Read at most `cap` bytes from `path`. Returns `None` on any IO error (a
/// missing file is indistinguishable from "no data here" — the fail-open
/// primitive). Reads only up to `cap` so a huge file never loads fully.
fn read_capped(path: &Path, cap: usize) -> Option<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    file.take(cap as u64).read_to_end(&mut buf).ok()?;
    Some(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_file_inside_repo_is_readable_with_language_and_content() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::write(base.join("hello.ts"), b"export const x = 1;\n").unwrap();

        let fc = read_file_impl(&base.to_string_lossy(), "hello.ts");
        assert!(fc.readable, "a text file inside the repo is readable");
        assert!(!fc.is_binary);
        assert!(!fc.truncated);
        assert_eq!(fc.language, "ts", "language is the lowercase extension");
        assert_eq!(fc.content, "export const x = 1;\n");
        assert_eq!(fc.size_bytes, 20);
    }

    #[test]
    fn backslash_separator_resolves_into_nested_dir() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::create_dir_all(base.join("src").join("api")).unwrap();
        std::fs::write(base.join("src").join("api").join("git.rs"), b"fn x() {}").unwrap();

        // A Windows-style separator must resolve the same as `/`.
        let fc = read_file_impl(&base.to_string_lossy(), "src\\api\\git.rs");
        assert!(fc.readable, "a backslash rel_path resolves into the repo");
        assert_eq!(fc.language, "rs");
        assert_eq!(fc.content, "fn x() {}");
    }

    #[test]
    fn traversal_escaping_the_repo_is_rejected() {
        // A secret file outside the repo must never be exposed via `..`.
        let outer = tempfile::tempdir().unwrap();
        std::fs::write(outer.path().join("secret.txt"), b"top secret").unwrap();
        let repo = outer.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        let fc = read_file_impl(&repo.to_string_lossy(), "../secret.txt");
        assert!(!fc.readable, "a traversal escaping the repo is not readable");
        assert!(fc.content.is_empty(), "out-of-repo content is never returned");
    }

    #[test]
    fn missing_file_is_not_readable_and_never_panics() {
        let dir = tempfile::tempdir().unwrap();
        let fc = read_file_impl(&dir.path().to_string_lossy(), "does-not-exist.rs");
        assert!(!fc.readable, "a missing file degrades to readable=false");
        assert!(fc.content.is_empty());
    }

    #[test]
    fn binary_file_returns_empty_content_flagged_binary() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        // A null byte in the sniff window marks the file binary.
        std::fs::write(base.join("blob.bin"), [0x00u8, 0x01, 0x02, 0x03]).unwrap();

        let fc = read_file_impl(&base.to_string_lossy(), "blob.bin");
        assert!(fc.is_binary, "a null byte marks the file binary");
        assert!(!fc.readable, "a binary file is not readable text");
        assert!(fc.content.is_empty(), "binary content is never returned");
    }
}
