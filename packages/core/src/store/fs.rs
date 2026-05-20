//! Fail-open filesystem primitives shared by [`event_store`](super::event_store)
//! and [`pipeline_repo`](super::pipeline_repo).
//!
//! Two operations matter here:
//!
//! - **Atomic write** — never leave a file (a `pipeline-state`, the event log)
//!   half-written. The bytes are written to a temporary file in the *same*
//!   directory and then `rename`d over the destination. A rename within one
//!   directory is atomic on every platform Mustard targets, so a reader sees
//!   either the old file or the new one, never a torn write.
//! - **Append** — add a line to a file opened in append mode, creating it
//!   (and its parent directory) if missing. This backs the NDJSON event log.
//!
//! Every function returns a [`Result`]; none panics. A missing file on read
//! is reported as a recoverable [`Error`] so callers can fail open (an absent
//! event log replays as empty, not as an error).

use crate::error::{Error, Result};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Ensure the parent directory of `path` exists, creating it recursively.
///
/// A no-op when `path` has no parent or the directory already exists.
fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }
    Ok(())
}

/// Build a sibling temporary path for `path` in the same directory.
///
/// Same-directory placement is required: `rename` is only guaranteed atomic
/// when source and destination share a filesystem, and a sibling always does.
/// The name mixes the original file name with a clock-derived suffix and the
/// process id so two concurrent writers do not collide on the temp file.
fn temp_path_for(path: &Path) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let pid = std::process::id();
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("mustard");
    let temp_name = format!(".{file_name}.{pid}.{nanos}.tmp");
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(temp_name),
        _ => PathBuf::from(temp_name),
    }
}

/// Atomically write `contents` to `path`.
///
/// Writes to a sibling temporary file, flushes it to disk, then renames it
/// over `path`. A reader of `path` sees either the previous contents or the
/// full new contents — never a partial write. The parent directory is created
/// if it does not exist. On any failure the temporary file is removed on a
/// best-effort basis and an [`Error`] is returned; nothing panics.
///
/// # Errors
///
/// Returns [`Error::Io`] if the directory cannot be created, the temporary
/// file cannot be written, or the rename fails.
pub fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    ensure_parent_dir(path)?;
    let temp = temp_path_for(path);

    // Scope the file handle so it is closed before the rename. Windows
    // refuses to rename a file that still has an open handle.
    let write_result = (|| -> Result<()> {
        let mut file = File::create(&temp)?;
        file.write_all(contents)?;
        file.flush()?;
        file.sync_all()?;
        Ok(())
    })();

    if let Err(err) = write_result {
        // Best-effort cleanup; ignore the cleanup error and report the real one.
        let _ = fs::remove_file(&temp);
        return Err(err);
    }

    if let Err(err) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(Error::from(err));
    }
    Ok(())
}

/// Append a single `line` to the file at `path`, adding a trailing newline.
///
/// The file is opened in append mode (creating it, and any missing parent
/// directory, if needed). Append mode keeps concurrent writers from clobbering
/// one another's lines for payloads small enough to land in a single write —
/// which is the case for the NDJSON event log. The caller passes the line
/// *without* a trailing newline; exactly one `\n` is appended.
///
/// # Errors
///
/// Returns [`Error::Io`] if the directory cannot be created or the write fails.
pub fn append_line(path: &Path, line: &str) -> Result<()> {
    ensure_parent_dir(path)?;
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    file.flush()?;
    Ok(())
}

/// Read `path` to a `String`.
///
/// # Errors
///
/// Returns [`Error::NotFound`] when the file does not exist — distinct from a
/// genuine I/O failure so callers can fail open on absence (e.g. replay an
/// empty event log) while still surfacing real errors. Any other failure is
/// reported as [`Error::Io`].
pub fn read_to_string(path: &Path) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Err(Error::NotFound(path.display().to_string()))
        }
        Err(err) => Err(Error::from(err)),
    }
}

/// `true` if `path` exists on disk.
#[must_use]
pub fn exists(path: &Path) -> bool {
    path.exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn write_atomic_then_read_round_trips() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        write_atomic(&path, b"{\"hello\":1}").unwrap();
        assert_eq!(read_to_string(&path).unwrap(), "{\"hello\":1}");
    }

    #[test]
    fn write_atomic_overwrites_existing_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        write_atomic(&path, b"first").unwrap();
        write_atomic(&path, b"second").unwrap();
        assert_eq!(read_to_string(&path).unwrap(), "second");
    }

    #[test]
    fn write_atomic_creates_missing_parent_dir() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested").join("deep").join("state.json");
        write_atomic(&path, b"ok").unwrap();
        assert_eq!(read_to_string(&path).unwrap(), "ok");
    }

    #[test]
    fn write_atomic_leaves_no_temp_files() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        write_atomic(&path, b"data").unwrap();
        let entries: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(std::result::Result::ok)
            .map(|e| e.file_name())
            .collect();
        // Only the destination file should remain — no `.tmp` siblings.
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0], std::ffi::OsStr::new("state.json"));
    }

    #[test]
    fn append_line_adds_trailing_newline() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        append_line(&path, "{\"a\":1}").unwrap();
        append_line(&path, "{\"a\":2}").unwrap();
        assert_eq!(read_to_string(&path).unwrap(), "{\"a\":1}\n{\"a\":2}\n");
    }

    #[test]
    fn read_missing_file_reports_not_found() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("absent.json");
        match read_to_string(&path) {
            Err(Error::NotFound(_)) => {}
            other => panic!("expected NotFound, got {other:?}"),
        }
    }
}
