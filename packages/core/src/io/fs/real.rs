//! [`RealFs`] — the production [`Fs`](super::Fs) implementation over
//! `std::fs`.
//!
//! This is the **only** module in `mustard-core` that calls `std::fs`
//! directly. Every other call site routes through the [`fs`](super) free
//! functions or a `&dyn Fs`, so the cross-cutting policy (fail-open `NotFound`
//! mapping, atomic writes) lives in exactly one place. The atomic-write
//! primitive was lifted verbatim from the former `store::fs` module, which now
//! re-exports it from here.

use super::{DirEntry, Fs};
use crate::platform::error::{Error, Result};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The real, `std::fs`-backed filesystem. Zero-sized and stateless.
#[derive(Debug, Clone, Copy, Default)]
pub struct RealFs;

/// Map a raw [`std::io::Error`] to the crate error, keeping `NotFound` distinct
/// so callers can fail open on absence without swallowing real failures.
fn map_io(path: &Path, err: std::io::Error) -> Error {
    if err.kind() == std::io::ErrorKind::NotFound {
        Error::NotFound(path.display().to_string())
    } else {
        Error::from(err)
    }
}

/// Ensure the parent directory of `path` exists, creating it recursively. A
/// no-op when `path` has no parent or the directory already exists.
fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty() && !parent.exists() {
            fs::create_dir_all(parent)?;
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

impl Fs for RealFs {
    fn read_to_string(&self, path: &Path) -> Result<String> {
        fs::read_to_string(path).map_err(|e| map_io(path, e))
    }

    fn read(&self, path: &Path) -> Result<Vec<u8>> {
        fs::read(path).map_err(|e| map_io(path, e))
    }

    fn write_atomic(&self, path: &Path, contents: &[u8]) -> Result<()> {
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

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let mut out = Vec::new();
        let entries = fs::read_dir(path).map_err(|e| map_io(path, e))?;
        for entry in entries {
            let entry = entry?;
            let entry_path = entry.path();
            let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
            let file_name = entry.file_name().to_string_lossy().into_owned();
            out.push(DirEntry {
                file_name,
                path: entry_path,
                is_dir,
            });
        }
        Ok(out)
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        fs::create_dir_all(path).map_err(|e| map_io(path, e))
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        fs::remove_file(path).map_err(|e| map_io(path, e))
    }

    fn modified(&self, path: &Path) -> Result<SystemTime> {
        let meta = fs::metadata(path).map_err(|e| map_io(path, e))?;
        meta.modified().map_err(Error::from)
    }

    fn remove_dir_all(&self, path: &Path) -> Result<()> {
        match fs::remove_dir_all(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Error::from(e)),
        }
    }

    fn remove_dir(&self, path: &Path) -> Result<()> {
        fs::remove_dir(path).map_err(|e| map_io(path, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn fs() -> RealFs {
        RealFs
    }

    #[test]
    fn write_atomic_then_read_round_trips() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs().write_atomic(&path, b"{\"hello\":1}").unwrap();
        assert_eq!(fs().read_to_string(&path).unwrap(), "{\"hello\":1}");
        assert_eq!(fs().read(&path).unwrap(), b"{\"hello\":1}");
    }

    #[test]
    fn write_atomic_overwrites_existing_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs().write_atomic(&path, b"first").unwrap();
        fs().write_atomic(&path, b"second").unwrap();
        assert_eq!(fs().read_to_string(&path).unwrap(), "second");
    }

    #[test]
    fn write_atomic_creates_missing_parent_dir() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested").join("deep").join("state.json");
        fs().write_atomic(&path, b"ok").unwrap();
        assert_eq!(fs().read_to_string(&path).unwrap(), "ok");
    }

    #[test]
    fn write_atomic_leaves_no_temp_files() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs().write_atomic(&path, b"data").unwrap();
        let entries = fs().read_dir(dir.path()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].file_name, "state.json");
    }

    /// Sem a posição atômica de fim, quem grava de vários lados no mesmo arquivo
    /// se exclui pela trava do `LockedFile`: com ela, nenhuma linha se perde
    /// nem se mistura com a do vizinho.
    #[test]
    fn appends_from_many_threads_through_the_lock_lose_no_line() {
        use super::super::lock::LockedFile;
        const THREADS: usize = 8;
        const LINES: usize = 40;
        let dir = tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        std::thread::scope(|scope| {
            for thread in 0..THREADS {
                let path = &path;
                scope.spawn(move || {
                    for n in 0..LINES {
                        let mut file = LockedFile::exclusive(path).unwrap();
                        file.append_line(&format!("{{\"t\":{thread},\"n\":{n}}}")).unwrap();
                    }
                });
            }
        });
        let text = std::fs::read_to_string(&path).unwrap();
        let mut lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), THREADS * LINES, "every appended line is in the file");
        lines.sort_unstable();
        lines.dedup();
        assert_eq!(lines.len(), THREADS * LINES, "no line was overwritten by another");
    }

    #[test]
    fn read_missing_file_reports_not_found() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("absent.json");
        match fs().read_to_string(&path) {
            Err(Error::NotFound(_)) => {}
            other => panic!("expected NotFound, got {other:?}"),
        }
        match fs().read(&path) {
            Err(Error::NotFound(_)) => {}
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn read_dir_missing_reports_not_found() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("nope");
        match fs().read_dir(&missing) {
            Err(Error::NotFound(_)) => {}
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn create_dir_all_and_exists() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("a").join("b");
        assert!(!fs().exists(&nested));
        fs().create_dir_all(&nested).unwrap();
        assert!(fs().exists(&nested));
    }

    #[test]
    fn remove_file_then_missing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("x.txt");
        fs().write_atomic(&path, b"y").unwrap();
        fs().remove_file(&path).unwrap();
        assert!(!fs().exists(&path));
        match fs().remove_file(&path) {
            Err(Error::NotFound(_)) => {}
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn modified_returns_a_time() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("m.txt");
        fs().write_atomic(&path, b"z").unwrap();
        assert!(fs().modified(&path).is_ok());
    }

    #[test]
    fn remove_dir_all_removes_tree() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("tree");
        fs().create_dir_all(&root.join("sub")).unwrap();
        fs().write_atomic(&root.join("sub").join("f.txt"), b"x").unwrap();
        fs().remove_dir_all(&root).unwrap();
        assert!(!fs().exists(&root));
    }

    #[test]
    fn remove_dir_all_absent_is_ok() {
        let dir = tempdir().unwrap();
        // Calling remove_dir_all on a path that never existed must succeed.
        fs().remove_dir_all(&dir.path().join("does_not_exist")).unwrap();
    }

    #[test]
    fn remove_dir_removes_empty_dir() {
        let dir = tempdir().unwrap();
        let empty = dir.path().join("empty");
        fs().create_dir_all(&empty).unwrap();
        fs().remove_dir(&empty).unwrap();
        assert!(!fs().exists(&empty));
    }

    #[test]
    fn remove_dir_missing_is_not_found() {
        let dir = tempdir().unwrap();
        match fs().remove_dir(&dir.path().join("ghost")) {
            Err(Error::NotFound(_)) => {}
            other => panic!("expected NotFound, got {other:?}"),
        }
    }
}
