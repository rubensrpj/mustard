//! `fs` — the single canonical seam for **all filesystem access** in the
//! Mustard monorepo.
//!
//! Every `std::fs` call in `mustard-core` routes through this module (the lone
//! exception is [`real`], which *is* the `std::fs` implementation), and the
//! sibling crates (`mustard-rt`, `mustard-cli`, the dashboard backend) migrate
//! onto it in later passes. Concentrating the I/O here buys three things at
//! once:
//!
//! - **Cross-cutting policy in one place.** Fail-open error mapping ([missing
//!   file ⇒ `Error::NotFound`](crate::error::Error::NotFound), never a panic),
//!   atomic writes (tempfile + `rename`, so a crash never leaves a torn file),
//!   and the hook point where a path-guard will later live — all centralised.
//! - **Testability (Dependency Inversion).** Logic that must be unit-tested
//!   without a real disk depends on the [`Fs`] *trait* and is handed a
//!   [`memory::FakeFs`]. The production code path uses [`real::RealFs`].
//! - **A drop-in migration target.** The module-level free functions
//!   ([`read_to_string`], [`write_atomic`], …) mirror the `std::fs` surface, so
//!   the ~700 mechanical call-site migrations across the workspace are a textual
//!   `std::fs::X` → `mustard_core::fs::X` swap with **no dependency threaded
//!   through every function**.
//!
//! ## When to use the free functions vs `&dyn Fs`
//!
//! | Use… | When |
//! |---|---|
//! | **Free functions** ([`read_to_string`], [`write_atomic`], …) | The default. The vast majority of call sites only ever touch the real disk; threading a port through them is pure ceremony. They delegate to a process-wide [`RealFs`](real::RealFs). |
//! | **`&dyn Fs` parameter** | A function whose filesystem behaviour you want to exercise in a unit test *without* a `tempdir` — inject a [`FakeFs`](memory::FakeFs). Reserve this for hot paths and logic-heavy code; do not virally convert leaf helpers. |
//!
//! Both share the same [`Fs`] implementation, so behaviour (fail-open mapping,
//! atomic writes) is identical whichever you pick.
//!
//! ## Safety contract (inherited by every implementation)
//!
//! - **Fail-open.** A missing file on read is [`Error::NotFound`] — distinct
//!   from a genuine [`Error::Io`] — so callers can treat absence as "empty"
//!   without swallowing real failures. Nothing here panics.
//! - **Atomic writes.** [`Fs::write_atomic`] writes a sibling tempfile, flushes
//!   and `fsync`s it, then renames over the target.
//! - **Encoding is the caller's concern.** This layer moves bytes (and, for
//!   convenience, UTF-8 strings). CRLF / UTF-8 normalisation belongs to the
//!   string-handling caller, not to `fs`.

pub mod memory;
pub mod real;

use crate::error::Result;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// One entry yielded by [`Fs::read_dir`].
///
/// A flattened, owned snapshot of a directory entry — name, full path, and
/// whether it is a directory — so the trait can be object-safe (`&dyn Fs`) and
/// the [`FakeFs`](memory::FakeFs) can synthesise entries without a real
/// `std::fs::DirEntry` (which is not constructible outside `std`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// The final path component (file or directory name).
    pub file_name: String,
    /// The full path to the entry (`dir.join(file_name)`).
    pub path: PathBuf,
    /// `true` when the entry is a directory.
    pub is_dir: bool,
}

/// The filesystem port: the operations `mustard-core` actually performs,
/// abstracted so production code uses [`real::RealFs`] and tests inject a
/// [`memory::FakeFs`] (Dependency Inversion).
///
/// Object-safe by design — consumers take `&dyn Fs`. Every method is fail-open:
/// it returns [`Result`] and never panics, even on hostile input.
pub trait Fs {
    /// Read `path` to a `String`.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`](crate::error::Error::NotFound) when the file does
    /// not exist (distinct from a real I/O failure so callers can fail open on
    /// absence); [`Error::Io`](crate::error::Error::Io) otherwise.
    fn read_to_string(&self, path: &Path) -> Result<String>;

    /// Read `path` to a byte vector.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`](crate::error::Error::NotFound) when the file is
    /// absent; [`Error::Io`](crate::error::Error::Io) otherwise.
    fn read(&self, path: &Path) -> Result<Vec<u8>>;

    /// Atomically write `contents` to `path` (sibling tempfile + `rename`).
    /// The parent directory is created if missing. A reader sees either the old
    /// bytes or the full new bytes — never a partial write.
    ///
    /// # Errors
    ///
    /// [`Error::Io`](crate::error::Error::Io) if the directory cannot be
    /// created, the tempfile cannot be written, or the rename fails.
    fn write_atomic(&self, path: &Path, contents: &[u8]) -> Result<()>;

    /// Append `line` to `path` with a single trailing `\n`, creating the file
    /// and any missing parent directory. Backs append-only logs (NDJSON
    /// metrics). The caller passes the line *without* a trailing newline.
    ///
    /// # Errors
    ///
    /// [`Error::Io`](crate::error::Error::Io) if the directory cannot be
    /// created or the write fails.
    fn append_line(&self, path: &Path, line: &str) -> Result<()>;

    /// `true` if `path` exists on disk.
    fn exists(&self, path: &Path) -> bool;

    /// List the immediate entries of directory `path` (non-recursive).
    /// Order is unspecified — callers that need determinism sort the result.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`](crate::error::Error::NotFound) when `path` does not
    /// exist; [`Error::Io`](crate::error::Error::Io) otherwise.
    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>>;

    /// Recursively create `path` and all missing parent directories. A no-op
    /// when the directory already exists.
    ///
    /// # Errors
    ///
    /// [`Error::Io`](crate::error::Error::Io) on failure.
    fn create_dir_all(&self, path: &Path) -> Result<()>;

    /// Remove the file at `path`.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`](crate::error::Error::NotFound) when `path` does not
    /// exist; [`Error::Io`](crate::error::Error::Io) otherwise.
    fn remove_file(&self, path: &Path) -> Result<()>;

    /// The last-modified time of `path`.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`](crate::error::Error::NotFound) when `path` does not
    /// exist; [`Error::Io`](crate::error::Error::Io) when the platform does not
    /// expose a modified time or the metadata read fails.
    fn modified(&self, path: &Path) -> Result<SystemTime>;
}

/// The process-wide default [`Fs`] backing the module-level free functions.
///
/// `RealFs` is zero-sized and stateless, so a `const` instance is free and
/// needs no synchronisation.
const DEFAULT: real::RealFs = real::RealFs;

/// A shared reference to the default real filesystem.
///
/// Handy when a `&dyn Fs` is required but the call site genuinely wants the
/// real disk (e.g. wiring a production struct that takes a port).
#[must_use]
pub fn real() -> &'static dyn Fs {
    &DEFAULT
}

// ---------------------------------------------------------------------------
// Module-level convenience free functions (backed by the default `RealFs`).
//
// These are the drop-in replacement for `std::fs::X`. Prefer them; reach for
// `&dyn Fs` only where a unit test needs to inject `FakeFs`.
// ---------------------------------------------------------------------------

/// Read `path` to a `String` via the default real filesystem. See
/// [`Fs::read_to_string`].
///
/// # Errors
///
/// [`Error::NotFound`](crate::error::Error::NotFound) on absence, else
/// [`Error::Io`](crate::error::Error::Io).
pub fn read_to_string(path: &Path) -> Result<String> {
    DEFAULT.read_to_string(path)
}

/// Read `path` to bytes via the default real filesystem. See [`Fs::read`].
///
/// # Errors
///
/// [`Error::NotFound`](crate::error::Error::NotFound) on absence, else
/// [`Error::Io`](crate::error::Error::Io).
pub fn read(path: &Path) -> Result<Vec<u8>> {
    DEFAULT.read(path)
}

/// Atomically write `contents` to `path` via the default real filesystem. See
/// [`Fs::write_atomic`].
///
/// # Errors
///
/// [`Error::Io`](crate::error::Error::Io) on failure.
pub fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    DEFAULT.write_atomic(path, contents)
}

/// Append a newline-terminated `line` to `path` via the default real
/// filesystem. See [`Fs::append_line`].
///
/// # Errors
///
/// [`Error::Io`](crate::error::Error::Io) on failure.
pub fn append_line(path: &Path, line: &str) -> Result<()> {
    DEFAULT.append_line(path, line)
}

/// `true` if `path` exists. See [`Fs::exists`].
#[must_use]
pub fn exists(path: &Path) -> bool {
    DEFAULT.exists(path)
}

/// List the immediate entries of directory `path` via the default real
/// filesystem. See [`Fs::read_dir`].
///
/// # Errors
///
/// [`Error::NotFound`](crate::error::Error::NotFound) on absence, else
/// [`Error::Io`](crate::error::Error::Io).
pub fn read_dir(path: &Path) -> Result<Vec<DirEntry>> {
    DEFAULT.read_dir(path)
}

/// Recursively create `path` via the default real filesystem. See
/// [`Fs::create_dir_all`].
///
/// # Errors
///
/// [`Error::Io`](crate::error::Error::Io) on failure.
pub fn create_dir_all(path: &Path) -> Result<()> {
    DEFAULT.create_dir_all(path)
}

/// Remove the file at `path` via the default real filesystem. See
/// [`Fs::remove_file`].
///
/// # Errors
///
/// [`Error::NotFound`](crate::error::Error::NotFound) on absence, else
/// [`Error::Io`](crate::error::Error::Io).
pub fn remove_file(path: &Path) -> Result<()> {
    DEFAULT.remove_file(path)
}

/// The last-modified time of `path` via the default real filesystem. See
/// [`Fs::modified`].
///
/// # Errors
///
/// [`Error::NotFound`](crate::error::Error::NotFound) on absence, else
/// [`Error::Io`](crate::error::Error::Io).
pub fn modified(path: &Path) -> Result<SystemTime> {
    DEFAULT.modified(path)
}
