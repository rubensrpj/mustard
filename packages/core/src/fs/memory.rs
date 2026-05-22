//! [`FakeFs`] — an in-memory [`Fs`](super::Fs) for filesystem-free unit tests.
//!
//! Mirrors the [`InMemorySpecReader`](crate::reader::memory::InMemorySpecReader)
//! style: state lives behind an [`RwLock`] so the trait's `&self` methods can
//! mutate, and the type is `Send + Sync`. A test injects a `FakeFs` wherever a
//! `&dyn Fs` is taken, asserting on filesystem effects without a `tempdir`.
//!
//! It is intentionally a *flat* path → bytes map with a synthetic directory
//! model derived from path prefixes — enough to exercise read / write / append
//! / `read_dir` / existence logic, not a faithful POSIX filesystem. Atomicity
//! is trivially satisfied (a write is a single map insert).

use super::{DirEntry, Fs};
use crate::error::{Error, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::SystemTime;

/// In-memory filesystem test double. Files are a `path → bytes` map; directories
/// are implied by the paths of the files they contain (plus any explicitly
/// created via [`Fs::create_dir_all`]).
#[derive(Default)]
pub struct FakeFs {
    files: RwLock<BTreeMap<PathBuf, Vec<u8>>>,
    /// Directories created explicitly (so an empty `create_dir_all` target is
    /// reported by [`Fs::exists`] / [`Fs::read_dir`] even with no files in it).
    dirs: RwLock<BTreeSet<PathBuf>>,
}

impl FakeFs {
    /// An empty fake filesystem.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed a file directly (test-setup convenience), bypassing the atomic
    /// write path. Equivalent to a successful prior `write_atomic`.
    pub fn seed(&self, path: impl Into<PathBuf>, contents: impl Into<Vec<u8>>) {
        let path = path.into();
        self.mark_parents(&path);
        if let Ok(mut files) = self.files.write() {
            files.insert(path, contents.into());
        }
    }

    /// Record every ancestor directory of `path` as existing.
    fn mark_parents(&self, path: &Path) {
        if let Ok(mut dirs) = self.dirs.write() {
            let mut cur = path.parent();
            while let Some(p) = cur {
                if p.as_os_str().is_empty() {
                    break;
                }
                dirs.insert(p.to_path_buf());
                cur = p.parent();
            }
        }
    }
}

impl Fs for FakeFs {
    fn read_to_string(&self, path: &Path) -> Result<String> {
        let bytes = self.read(path)?;
        String::from_utf8(bytes)
            .map_err(|e| Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))
    }

    fn read(&self, path: &Path) -> Result<Vec<u8>> {
        let files = self
            .files
            .read()
            .map_err(|_| Error::Io(std::io::Error::other("fakefs lock poisoned")))?;
        files
            .get(path)
            .cloned()
            .ok_or_else(|| Error::NotFound(path.display().to_string()))
    }

    fn write_atomic(&self, path: &Path, contents: &[u8]) -> Result<()> {
        self.mark_parents(path);
        let mut files = self
            .files
            .write()
            .map_err(|_| Error::Io(std::io::Error::other("fakefs lock poisoned")))?;
        files.insert(path.to_path_buf(), contents.to_vec());
        Ok(())
    }

    fn append_line(&self, path: &Path, line: &str) -> Result<()> {
        self.mark_parents(path);
        let mut files = self
            .files
            .write()
            .map_err(|_| Error::Io(std::io::Error::other("fakefs lock poisoned")))?;
        let buf = files.entry(path.to_path_buf()).or_default();
        buf.extend_from_slice(line.as_bytes());
        buf.push(b'\n');
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        let is_file = self.files.read().is_ok_and(|f| f.contains_key(path));
        let is_dir = self.dirs.read().is_ok_and(|d| d.contains(path));
        is_file || is_dir
    }

    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let known_dir = self.dirs.read().is_ok_and(|d| d.contains(path));
        let files = self
            .files
            .read()
            .map_err(|_| Error::Io(std::io::Error::other("fakefs lock poisoned")))?;
        let dirs = self
            .dirs
            .read()
            .map_err(|_| Error::Io(std::io::Error::other("fakefs lock poisoned")))?;

        // A directory "exists" if it was created explicitly or some file lives
        // beneath it; otherwise it is NotFound (mirroring real `read_dir`).
        let has_children = files.keys().any(|p| p.parent() == Some(path))
            || dirs.iter().any(|p| p.parent() == Some(path));
        if !known_dir && !has_children {
            return Err(Error::NotFound(path.display().to_string()));
        }

        // Immediate children only, de-duplicated by name.
        let mut seen: BTreeMap<String, DirEntry> = BTreeMap::new();
        for p in files.keys() {
            if p.parent() == Some(path) {
                if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                    seen.entry(name.to_string()).or_insert(DirEntry {
                        file_name: name.to_string(),
                        path: p.clone(),
                        is_dir: false,
                    });
                }
            }
        }
        for p in dirs.iter() {
            if p.parent() == Some(path) {
                if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                    seen.insert(
                        name.to_string(),
                        DirEntry {
                            file_name: name.to_string(),
                            path: p.clone(),
                            is_dir: true,
                        },
                    );
                }
            }
        }
        Ok(seen.into_values().collect())
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        let mut dirs = self
            .dirs
            .write()
            .map_err(|_| Error::Io(std::io::Error::other("fakefs lock poisoned")))?;
        let mut cur = Some(path);
        while let Some(p) = cur {
            if p.as_os_str().is_empty() {
                break;
            }
            dirs.insert(p.to_path_buf());
            cur = p.parent();
        }
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        let mut files = self
            .files
            .write()
            .map_err(|_| Error::Io(std::io::Error::other("fakefs lock poisoned")))?;
        files
            .remove(path)
            .map(|_| ())
            .ok_or_else(|| Error::NotFound(path.display().to_string()))
    }

    fn modified(&self, path: &Path) -> Result<SystemTime> {
        // No clock model — report a fixed epoch for any file that exists.
        if self.files.read().is_ok_and(|f| f.contains_key(path)) {
            Ok(SystemTime::UNIX_EPOCH)
        } else {
            Err(Error::NotFound(path.display().to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_then_read_round_trips() {
        let fs = FakeFs::new();
        let p = Path::new("/a/b/spec.md");
        fs.write_atomic(p, b"hello").unwrap();
        assert_eq!(fs.read_to_string(p).unwrap(), "hello");
        assert!(fs.exists(p));
    }

    #[test]
    fn read_missing_is_not_found() {
        let fs = FakeFs::new();
        match fs.read(Path::new("/nope")) {
            Err(Error::NotFound(_)) => {}
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn append_accumulates_lines() {
        let fs = FakeFs::new();
        let p = Path::new("/log.jsonl");
        fs.append_line(p, "a").unwrap();
        fs.append_line(p, "b").unwrap();
        assert_eq!(fs.read_to_string(p).unwrap(), "a\nb\n");
    }

    #[test]
    fn read_dir_lists_immediate_children() {
        let fs = FakeFs::new();
        fs.seed("/root/x.md", "x");
        fs.seed("/root/sub/y.md", "y");
        fs.create_dir_all(Path::new("/root/empty")).unwrap();
        let mut entries = fs.read_dir(Path::new("/root")).unwrap();
        entries.sort_by(|a, b| a.file_name.cmp(&b.file_name));
        let names: Vec<_> = entries.iter().map(|e| e.file_name.as_str()).collect();
        assert_eq!(names, vec!["empty", "sub", "x.md"]);
        let sub = entries.iter().find(|e| e.file_name == "sub").unwrap();
        assert!(sub.is_dir);
        let x = entries.iter().find(|e| e.file_name == "x.md").unwrap();
        assert!(!x.is_dir);
    }

    #[test]
    fn read_dir_missing_is_not_found() {
        let fs = FakeFs::new();
        match fs.read_dir(Path::new("/ghost")) {
            Err(Error::NotFound(_)) => {}
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn remove_file_then_gone() {
        let fs = FakeFs::new();
        let p = Path::new("/f.txt");
        fs.write_atomic(p, b"y").unwrap();
        fs.remove_file(p).unwrap();
        assert!(!fs.exists(p));
        assert!(matches!(fs.remove_file(p), Err(Error::NotFound(_))));
    }

    #[test]
    fn create_dir_all_then_exists() {
        let fs = FakeFs::new();
        let d = Path::new("/deep/nested/dir");
        fs.create_dir_all(d).unwrap();
        assert!(fs.exists(d));
        assert!(fs.exists(Path::new("/deep")));
    }
}
