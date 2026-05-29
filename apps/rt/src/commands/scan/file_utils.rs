//! File-collection and path helpers shared across scanners — a port of
//! `registry/file-utils.js`.
//!
//! Only filesystem utilities live here: no scanning logic, no schema building.
//! Every function is fail-open (an unreadable directory yields an empty result,
//! never an error), matching the JS module's `try { … } catch { … }` shape.
//!
//! ## Single-pass file visitor (Wave 1 — project-profiler)
//!
//! [`visit`] walks a subproject root **once**, computes the ignore-set a single
//! time, and reads every regular file in parallel via rayon. The returned
//! [`VisitedFile`] vector is sorted by relative path so downstream consumers
//! (scanners, cluster discovery, description enrichment) see deterministic
//! input regardless of OS / filesystem order.
//!
//! To avoid touching the per-stack scanner detection logic, [`visit`]'s output
//! is also exposed as a process-local **read cache** ([`with_cache`]): while a
//! cache is active, every [`read_file_safe`] / [`collect_files`] call resolves
//! from memory instead of disk. Production callers wrap their scan in
//! `with_cache(visit(root, ext_hint), || scanner.scan(root))` — the scanners
//! themselves keep their existing signatures and bodies, but every file is read
//! exactly once per `Scanner::scan()` invocation.

use mustard_core::io::fs as mfs;
use rayon::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Universal directory skip-list — mirrors `DEFAULT_IGNORE` in `file-utils.js`.
pub const DEFAULT_IGNORE: &[&str] = &[
    "node_modules",
    "bin",
    "obj",
    "dist",
    ".next",
    "__pycache__",
    ".venv",
    "venv",
    "target",
    "build",
    ".git",
    "migrations",
    "Migrations",
];

/// Extract directory-name patterns from a `.gitignore` string.
///
/// A faithful port of `parseGitignoreDirs()` — conservative: keeps only entries
/// that look like a plain folder name (non-empty, no whitespace, no glob chars,
/// no slashes, not a negation, not a comment). Trailing slashes are stripped.
#[must_use]
pub fn parse_gitignore_dirs(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
            continue;
        }
        if line.starts_with('/') {
            continue; // path-anchored
        }
        if line
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '*' | '?' | '[' | ']'))
        {
            continue; // glob or whitespace
        }
        let name = line.strip_suffix('/').unwrap_or(line);
        if name.contains('/') {
            continue; // nested path, not a bare name
        }
        out.push(name.to_string());
    }
    out
}

/// Build the merged skip-set for a walk rooted at `dir`.
///
/// Combines `DEFAULT_IGNORE`, the explicit `ignore` argument, the
/// `MUSTARD_SCAN_IGNORE` env var (comma-separated), and directory entries
/// parsed from the subproject's `.gitignore` — exactly the four sources
/// `collectFiles` merges.
fn ignore_set(dir: &Path, ignore: &[&str]) -> BTreeSet<String> {
    let mut set: BTreeSet<String> = DEFAULT_IGNORE.iter().map(|s| (*s).to_string()).collect();
    for extra in ignore {
        set.insert((*extra).to_string());
    }
    if let Ok(env) = std::env::var("MUSTARD_SCAN_IGNORE") {
        for name in env.split(',') {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                set.insert(trimmed.to_string());
            }
        }
    }
    if let Ok(content) = mfs::read_to_string(dir.join(".gitignore")) {
        for name in parse_gitignore_dirs(&content) {
            set.insert(name);
        }
    }
    set
}

/// Recursively collect every file with `extension` under `dir`.
///
/// Skips ignored directories, dot-directories, and the `ignore` argument —
/// a faithful port of `collectFiles()`. `extension` includes the dot
/// (e.g. `.rs`). Fail-open: unreadable directories are silently skipped.
///
/// When a [single-pass cache](with_cache) is installed for `dir` (or an
/// ancestor), the matching subset is served from memory without touching the
/// filesystem — the single read in [`visit`] is reused. Outside that scope the
/// function falls back to a fresh directory walk.
#[must_use]
pub fn collect_files(dir: &Path, extension: &str, ignore: &[&str]) -> Vec<PathBuf> {
    if let Some(cached) = cache_collect_files(dir, extension) {
        return cached;
    }
    let skip = ignore_set(dir, ignore);
    let mut results = Vec::new();
    walk(dir, extension, &skip, &mut results);
    results
}

fn walk(current: &Path, extension: &str, skip: &BTreeSet<String>, results: &mut Vec<PathBuf>) {
    let Ok(entries) = mfs::read_dir(current) else {
        return;
    };
    for entry in entries {
        let name: &str = &entry.file_name;
        if entry.is_dir {
            if skip.contains(name) || name.starts_with('.') {
                continue;
            }
            walk(&entry.path, extension, skip, results);
        } else if name.ends_with(extension) {
            results.push(entry.path);
        }
    }
}

/// Most frequent source-file extension under `dir`, including the leading dot
/// (e.g. `.rb`), or `None` when no source file is present.
///
/// This is the agnostic fallback the cluster / convention gates use when the
/// stack is unknown and [`super::project_conventions::primary_ext_for_stack`]
/// returns `None`: instead of zeroing, they discover the project's own dominant
/// extension and operate on it. Cache-aware — when a [`with_cache`] scope covers
/// `dir` the tally is served from the visited file list (no second disk walk);
/// otherwise a fresh source-file walk is performed (sharing [`is_source_file`]
/// with [`visit`], so the same files are considered). Ties break to the
/// lexicographically-smallest extension for determinism.
#[must_use]
pub fn dominant_source_extension(dir: &Path) -> Option<String> {
    let extra_exts = mustard_core::ProjectConfig::load(dir).source_extensions();
    let paths = source_files_under(dir, &extra_exts);
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for p in &paths {
        if let Some(ext) = p.file_name().and_then(|n| n.to_str()).and_then(extension_of) {
            *counts.entry(ext).or_insert(0) += 1;
        }
    }
    // `max_by_key` over a BTreeMap yields the *last* of equal-count keys; we
    // want the smallest extension on a tie, so fold manually.
    counts.into_iter().fold(None, |best, (ext, count)| match best {
        Some((_, bc)) if bc >= count => best,
        _ => Some((ext, count)),
    })
    .map(|(ext, _)| ext)
}

/// List every source file under `dir`, cache-aware. Used by
/// [`dominant_source_extension`]; shares the visitor's [`is_source_file`]
/// predicate so the extension tally reflects exactly what [`visit`] would open.
fn source_files_under(dir: &Path, extra_exts: &[String]) -> Vec<PathBuf> {
    if let Some(cached) = cache_source_files(dir, extra_exts) {
        return cached;
    }
    let skip = ignore_set(dir, &[]);
    let mut all = Vec::new();
    walk_all(dir, &skip, &mut all);
    all.retain(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| is_source_file(n, extra_exts))
    });
    all
}

/// Collect every regular file under `dir` (no extension filter) along with the
/// directory entries themselves. Internal helper for [`visit`] — keeps a single
/// directory walk that downstream code splits by extension in memory.
fn walk_all(current: &Path, skip: &BTreeSet<String>, results: &mut Vec<PathBuf>) {
    let Ok(entries) = mfs::read_dir(current) else {
        return;
    };
    for entry in entries {
        let name: &str = &entry.file_name;
        if entry.is_dir {
            if skip.contains(name) || name.starts_with('.') {
                continue;
            }
            walk_all(&entry.path, skip, results);
        } else {
            results.push(entry.path);
        }
    }
}

/// Relative path from `base` to `file_path`, normalised with forward slashes.
///
/// A faithful port of `relativePath()`.
#[must_use]
pub fn relative_path(base: &Path, file_path: &Path) -> String {
    let rel = file_path.strip_prefix(base).unwrap_or(file_path);
    rel.to_string_lossy().replace('\\', "/")
}

/// Read a file as UTF-8, returning `None` on any error — a port of `readFileSafe()`.
///
/// When a [single-pass cache](with_cache) covers `file_path`, the cached
/// content is returned without any filesystem call. Outside the cache scope (or
/// on a path the cache did not visit) the function falls back to a fresh disk
/// read.
#[must_use]
pub fn read_file_safe(file_path: &Path) -> Option<String> {
    if let Some(hit) = cache_read(file_path) {
        return Some(hit);
    }
    DISK_READ_COUNTER.with(|c| c.set(c.get() + 1));
    let result = mfs::read_to_string(file_path).ok();
    if result.is_some() {
        DISK_HIT_COUNTER.with(|c| c.set(c.get() + 1));
    }
    result
}

/// Most common parent folder across a list of relative file paths.
///
/// A faithful port of `inferCommonFolder()` — returns the most frequent parent
/// directory with a trailing slash, or `None` for an empty input. Wave 2
/// removed the per-language scanners that called this; it stays public for
/// forward-compat enrichment passes that may reintroduce a folder-locator.
#[must_use]
#[allow(dead_code)]
pub fn infer_common_folder(file_paths: &[String]) -> Option<String> {
    if file_paths.is_empty() {
        return None;
    }
    let mut counts: Vec<(String, usize)> = Vec::new();
    for fp in file_paths {
        let normalized = fp.replace('\\', "/");
        let dir = match normalized.rfind('/') {
            Some(idx) => normalized[..idx].to_string(),
            None => ".".to_string(),
        };
        if let Some(entry) = counts.iter_mut().find(|(d, _)| *d == dir) {
            entry.1 += 1;
        } else {
            counts.push((dir, 1));
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(dir, _)| format!("{dir}/"))
}

// ---------------------------------------------------------------------------
// Single-pass visitor + per-scan read cache (Wave 1 — project-profiler).
//
// The cache lives in a thread-local so it never escapes the call that
// installed it; the visitor itself parallelises the per-file disk read via
// rayon, so the cache is *populated* on a worker pool but *consumed* on the
// thread that calls `with_cache`. That is exactly what the registry pipeline
// needs: every per-faceta scan (`scan_entities`, `scan_enums`, …) and the
// agnostic helpers (`cluster_discovery`, `enrich_descriptions`) run on the
// caller thread and consult the cache without re-touching disk.
// ---------------------------------------------------------------------------

/// One file produced by the single-pass [`visit`] walk.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct VisitedFile {
    /// Absolute path to the file.
    pub abs: PathBuf,
    /// Relative path from the visited root, forward-slash normalised.
    pub rel: String,
    /// UTF-8 contents. `None` when the read failed (binary / permission).
    pub content: Option<String>,
}

/// The cached output of a single [`visit`] call — shared by `read_file_safe`
/// and `collect_files` while a [`with_cache`] scope is active.
#[derive(Debug, Clone)]
struct VisitCache {
    /// Visited root as the caller passed it (used for `starts_with` matching
    /// against the absolute paths produced by [`visit`]).
    root: PathBuf,
    /// Canonical form of `root` when available — extends cache hits to callers
    /// that pass the same logical directory in a different surface form.
    root_canon: Option<PathBuf>,
    /// Absolute path → file contents (only entries whose content is `Some`).
    by_abs: BTreeMap<PathBuf, String>,
    /// Every visited file path, in deterministic order.
    files: Vec<PathBuf>,
}

thread_local! {
    /// The cache stack — `with_cache` pushes on entry and pops on drop, so
    /// nested calls (e.g. cluster discovery inside a scan) inherit the
    /// innermost cache without leaking it to sibling threads.
    static CACHE_STACK: RefCell<Vec<VisitCache>> = const { RefCell::new(Vec::new()) };
}

thread_local! {
    /// Thread-local counter of disk reads performed by [`read_file_safe`] when
    /// no cache covered the path. Used by the single-pass parity test (AC-2)
    /// to assert that a scan reads each source file at most once.
    ///
    /// Thread-local (rather than a global `AtomicU64`) so concurrent `cargo
    /// test` workers do not race: every test sees its own counter, and the
    /// only path that bypasses the cache to bump it (`read_file_safe`) runs
    /// on the caller thread — the rayon-parallel reads inside [`visit`] go
    /// through `mfs::read_to_string` directly and never touch this counter.
    /// Production callers ignore it.
    static DISK_READ_COUNTER: Cell<u64> = const { Cell::new(0) };
    /// Companion counter: increments only when the disk read returned `Some` —
    /// i.e. when an *existing* file slipped past the cache. Tests assert this
    /// stays at zero, which is the real "single-pass" guarantee. Probes for
    /// absent files (`Cargo.toml`/`main.rs` shortcuts) are tracked by
    /// `DISK_READ_COUNTER` only. Same thread-local rationale as above.
    static DISK_HIT_COUNTER: Cell<u64> = const { Cell::new(0) };
}

/// Snapshot the disk-read attempt counter — test-only helper.
#[doc(hidden)]
#[must_use]
#[allow(dead_code)]
pub fn disk_read_count() -> u64 {
    DISK_READ_COUNTER.with(Cell::get)
}

/// Snapshot the disk-hit counter (disk reads that returned content) —
/// test-only helper.
#[doc(hidden)]
#[must_use]
#[allow(dead_code)]
pub fn disk_hit_count() -> u64 {
    DISK_HIT_COUNTER.with(Cell::get)
}

/// Reset both thread-local counters — test-only helper.
#[doc(hidden)]
#[allow(dead_code)]
pub fn reset_disk_read_count() {
    DISK_READ_COUNTER.with(|c| c.set(0));
    DISK_HIT_COUNTER.with(|c| c.set(0));
}

/// Known-stack source extensions — a fast-path allow-list, **not** a gate.
///
/// Wave 2 of project-profiler treated this as a closed allow-list: a file whose
/// extension was absent was silently dropped, so a `.c`/`.swift`/`.rb`/`.zig`
/// project produced zero visited files and every downstream gate (clusters,
/// conventions, roles) zeroed. F0-e makes the visitor agnostic: this list is
/// retained only so the well-known stacks keep their exact behaviour, while
/// [`is_source_file`] now *defaults to including* any unknown extension that is
/// not on the [`NON_SOURCE_EXTENSIONS`] deny-list.
const KNOWN_SOURCE_EXTENSIONS: &[&str] = &[
    ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs", ".cs", ".rs", ".go", ".java",
    ".kt", ".dart", ".py", ".php", ".prisma",
];

/// Binary / generated-asset / lockfile extensions the visitor refuses to open.
///
/// This is the *only* hard gate left in the visitor: an extension here is
/// skipped so the parallel read pool never opens a PNG, archive, or compiled
/// artefact just to discard it as non-UTF-8. Everything **not** on this list is
/// treated as plausible source code (the agnostic default), which is what lets
/// an exotic language (`.zig`, `.rb`, `.swift`, `.c`, `.h`, …) be visited
/// without a per-language allow-list entry.
///
/// Conservative on purpose: when in doubt an extension is *kept* (visited), not
/// dropped — a stray text file costs one cheap read, a dropped source file
/// zeroes a whole gate.
const NON_SOURCE_EXTENSIONS: &[&str] = &[
    // Images / media.
    ".png", ".jpg", ".jpeg", ".gif", ".bmp", ".ico", ".svg", ".webp", ".avif",
    ".mp3", ".mp4", ".wav", ".ogg", ".webm", ".mov", ".avi", ".mkv",
    ".woff", ".woff2", ".ttf", ".otf", ".eot",
    // Archives / compressed.
    ".zip", ".gz", ".tgz", ".tar", ".rar", ".7z", ".bz2", ".xz", ".zst",
    // Compiled / binary artefacts.
    ".exe", ".dll", ".so", ".dylib", ".o", ".obj", ".a", ".lib", ".class",
    ".pdb", ".bin", ".wasm", ".node", ".pyc", ".pyo", ".rlib",
    // Documents / data blobs.
    ".pdf", ".doc", ".docx", ".xls", ".xlsx", ".ppt", ".pptx",
    ".db", ".sqlite", ".sqlite3", ".dat", ".pack",
    // Lockfiles (large, generated, never source for scanning purposes).
    ".lock",
];

/// Manifest / configuration files the scanners load by full name. Treated as
/// "source" even though they have no source-file extension.
const SOURCE_FILENAMES: &[&str] = &[
    "package.json",
    "tsconfig.json",
    "Cargo.toml",
    "go.mod",
    "pubspec.yaml",
    "composer.json",
    "artisan",
    "pyproject.toml",
    "setup.py",
    "requirements.txt",
    "manage.py",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    ".gitignore",
];

/// Lowercased extension of `name` including the leading dot (`Foo.ZIG` →
/// `.zig`), or `None` when the name has no extension or is dotfile-only.
fn extension_of(name: &str) -> Option<String> {
    let dot = name.rfind('.')?;
    // A leading-dot file like `.gitignore` is handled by SOURCE_FILENAMES, not
    // here; treat `name` with the dot at index 0 as "no extension".
    if dot == 0 {
        return None;
    }
    Some(name[dot..].to_ascii_lowercase())
}

/// Decide whether [`visit`] should open `name` as plausible source.
///
/// Agnostic by default (F0-e): a file is **kept** unless its extension is on the
/// [`NON_SOURCE_EXTENSIONS`] binary/asset deny-list. The known-stack allow-list
/// and the manifest filenames are fast accepts; `extra_exts`
/// (`mustard.json#sourceExtensions`) force-includes user-named extensions even
/// if they would otherwise be denied. Extensionless files (e.g. `Makefile`,
/// `Dockerfile`) are kept — they are plausible source/manifest and cheap to
/// read.
fn is_source_file(name: &str, extra_exts: &[String]) -> bool {
    if SOURCE_FILENAMES.contains(&name) {
        return true;
    }
    if name.ends_with(".csproj") || name.ends_with(".sln") {
        return true;
    }
    let Some(ext) = extension_of(name) else {
        // No extension ⇒ keep (build scripts, Dockerfile, Makefile, …).
        return true;
    };
    // User-pinned extensions win over the deny-list.
    if extra_exts.iter().any(|e| e.eq_ignore_ascii_case(&ext)) {
        return true;
    }
    if KNOWN_SOURCE_EXTENSIONS.contains(&ext.as_str()) {
        return true;
    }
    // Agnostic default: everything not explicitly a binary/asset is source.
    !NON_SOURCE_EXTENSIONS.contains(&ext.as_str())
}

/// Walk `root` once and read every relevant source file in parallel.
///
/// The ignore-set is computed exactly once (default skip list + explicit
/// `ignore` argument + `MUSTARD_SCAN_IGNORE` env + parsed `.gitignore`) and
/// reused by the directory walk. The returned vector is sorted by relative
/// path so downstream consumers (entity / enum scanners, cluster discovery)
/// see a deterministic order regardless of OS or filesystem reporting order.
///
/// Agnostic by default (F0-e): every file is opened as plausible source unless
/// its extension is on the [`NON_SOURCE_EXTENSIONS`] binary/asset deny-list, so
/// an exotic-language project (`.zig`, `.rb`, `.swift`, …) is visited without a
/// per-stack allow-list entry. `mustard.json#sourceExtensions` (read from
/// `root`) force-includes user-named extensions on top of that.
///
/// Fail-open: unreadable files yield `VisitedFile { content: None, .. }` and
/// missing directories are silently skipped.
#[must_use]
pub fn visit(root: &Path, ignore: &[&str]) -> Vec<VisitedFile> {
    let skip = ignore_set(root, ignore);
    let extra_exts = mustard_core::ProjectConfig::load(root).source_extensions();
    let mut paths = Vec::new();
    walk_all(root, &skip, &mut paths);
    // Filter out only binaries / generated assets — keeps the parallel read
    // pool from opening every PNG and lockfile while never dropping a plausible
    // source file just because its language is unknown.
    paths.retain(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| is_source_file(n, &extra_exts))
    });
    // Sort by absolute path; the relative form is stably derived from it.
    paths.sort();
    // Parallel UTF-8 reads. Rayon's default pool work-steals across cores.
    paths
        .into_par_iter()
        .map(|abs| {
            let rel = relative_path(root, &abs);
            let content = mfs::read_to_string(&abs).ok();
            VisitedFile { abs, rel, content }
        })
        .collect()
}

/// `true` if a [`with_cache`] scope on the current thread already covers `root`
/// (i.e. `root` equals the cache root or sits under it in either the original
/// or the canonical form). Lets nested callers avoid re-visiting the same tree.
///
/// Kept public for [`ensure_cache`] and any future nesting consumer; Wave 2's
/// generic interpreter does not call it directly.
#[must_use]
#[allow(dead_code)]
pub fn cache_covers(root: &Path) -> bool {
    let root_canon = mfs::canonicalize(root).ok();
    CACHE_STACK.with(|stack| {
        let stack = stack.borrow();
        stack.iter().any(|entry| {
            let original = root == entry.root.as_path() || root.starts_with(&entry.root);
            let canonical = match (&root_canon, &entry.root_canon) {
                (Some(d), Some(r)) => d == r || d.starts_with(r),
                _ => false,
            };
            original || canonical
        })
    })
}

/// Install a single-pass cache for `root` for the duration of `body`, **unless**
/// an enclosing scope already covers it. Lets the registry pipeline visit each
/// subproject once at the outer scope and nest scanner / cluster-discovery /
/// enrichment calls inside without re-walking.
///
/// Wave 2's generic interpreter does not call this directly (the registry
/// pipeline does the outer `with_cache`); it stays public so a future caller
/// that owns its own scope can opt in.
#[allow(dead_code)]
pub fn ensure_cache<R>(root: &Path, ignore: &[&str], body: impl FnOnce() -> R) -> R {
    if cache_covers(root) {
        return body();
    }
    let visited = visit(root, ignore);
    with_cache(root, visited, body)
}

/// Install `visited` as the active read cache for the duration of `body`.
///
/// All [`read_file_safe`] / [`collect_files`] calls performed during `body`
/// (transitively, on the same thread) resolve from the cache without touching
/// disk. The cache is popped on return, panic, or early exit, so it can never
/// leak across calls.
pub fn with_cache<R>(root: &Path, visited: Vec<VisitedFile>, body: impl FnOnce() -> R) -> R {
    let mut by_abs = BTreeMap::new();
    let mut files = Vec::with_capacity(visited.len());
    for v in visited {
        files.push(v.abs.clone());
        if let Some(c) = v.content {
            by_abs.insert(v.abs, c);
        }
    }
    // Canonicalise the root so callers that pass the same path with different
    // surface forms (extra trailing slash, `./prefix`) still hit the cache.
    let root_canon = mfs::canonicalize(root).ok();
    let entry = VisitCache {
        root: root.to_path_buf(),
        root_canon,
        by_abs,
        files,
    };
    CACHE_STACK.with(|stack| stack.borrow_mut().push(entry));
    // Guard restores the stack even if `body` panics.
    let _guard = PopGuard;
    body()
}

/// RAII guard that pops the innermost cache off the thread-local stack on
/// drop, even if `body` panics. Defined at module scope (rather than inside
/// [`with_cache`]) so the `Drop` impl satisfies the pedantic "items after
/// statements" lint without changing observable behaviour.
struct PopGuard;
impl Drop for PopGuard {
    fn drop(&mut self) {
        CACHE_STACK.with(|stack| {
            let _ = stack.borrow_mut().pop();
        });
    }
}

/// Resolve `path` against the active cache, returning the stored contents on
/// hit. Tries the innermost cache first; falls through caches for which the
/// path is outside the visited root.
fn cache_read(path: &Path) -> Option<String> {
    CACHE_STACK.with(|stack| {
        let stack = stack.borrow();
        let canon = mfs::canonicalize(path).ok();
        for entry in stack.iter().rev() {
            // Direct hit on the absolute path the cache stored.
            if let Some(hit) = entry.by_abs.get(path) {
                return Some(hit.clone());
            }
            if let Some(canon) = &canon {
                if let Some(hit) = entry.by_abs.get(canon) {
                    return Some(hit.clone());
                }
            }
            // Some callers pass a path the visitor *should* have produced but
            // never read (e.g. an existing-but-empty file). Fall through to
            // disk in that case rather than returning `None` as "hit".
        }
        None
    })
}

/// List cached files matching `extension` under `dir`, when a cache covers the
/// directory. Returns `None` when no active cache contains `dir` or any of its
/// children — the caller then falls back to a fresh `walk()`.
fn cache_collect_files(dir: &Path, extension: &str) -> Option<Vec<PathBuf>> {
    CACHE_STACK.with(|stack| {
        let stack = stack.borrow();
        if stack.is_empty() {
            return None;
        }
        let dir_canon = mfs::canonicalize(dir).ok();
        for entry in stack.iter().rev() {
            // `dir` covered by this cache when it equals or sits under the
            // cache root in either the original or the canonical form.
            let covers_original = dir == entry.root.as_path() || dir.starts_with(&entry.root);
            let covers_canon = match (&dir_canon, &entry.root_canon) {
                (Some(d), Some(r)) => d == r || d.starts_with(r),
                _ => false,
            };
            if !(covers_original || covers_canon) {
                continue;
            }
            // Filter the cache's flat file list to entries that sit under
            // `dir` (matching the surface form the visitor produced) and end
            // with the requested extension.
            let mut out: Vec<PathBuf> = entry
                .files
                .iter()
                .filter(|p| {
                    // Match against the visitor's own absolute paths first;
                    // those are derived from `entry.root` so a `starts_with`
                    // against the cache's view of `dir` is the safe check.
                    p.starts_with(dir)
                        || dir_canon.as_ref().is_some_and(|d| p.starts_with(d))
                })
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.ends_with(extension))
                })
                .cloned()
                .collect();
            out.sort();
            return Some(out);
        }
        None
    })
}

/// List the cached source files under `dir`, when a cache covers it. The
/// cache's flat file list is exactly the set [`visit`] kept (binaries / assets
/// already filtered out before [`with_cache`] stored it), so this only narrows
/// by directory coverage — no per-extension filter. Returns `None` when no
/// active cache contains `dir`, so [`source_files_under`] falls back to a walk.
/// `_extra_exts` is accepted for signature symmetry with the walk fallback but
/// is unused: the cache was already built with the visit-time override applied.
fn cache_source_files(dir: &Path, _extra_exts: &[String]) -> Option<Vec<PathBuf>> {
    CACHE_STACK.with(|stack| {
        let stack = stack.borrow();
        if stack.is_empty() {
            return None;
        }
        let dir_canon = mfs::canonicalize(dir).ok();
        for entry in stack.iter().rev() {
            let covers_original = dir == entry.root.as_path() || dir.starts_with(&entry.root);
            let covers_canon = match (&dir_canon, &entry.root_canon) {
                (Some(d), Some(r)) => d == r || d.starts_with(r),
                _ => false,
            };
            if !(covers_original || covers_canon) {
                continue;
            }
            let mut out: Vec<PathBuf> = entry
                .files
                .iter()
                .filter(|p| {
                    p.starts_with(dir) || dir_canon.as_ref().is_some_and(|d| p.starts_with(d))
                })
                .cloned()
                .collect();
            out.sort();
            return Some(out);
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parse_gitignore_keeps_plain_names_only() {
        let content = "# comment\nnode_modules\n/anchored\n*.log\n  \nvendor/\n!keep\nsrc/nested\n";
        assert_eq!(
            parse_gitignore_dirs(content),
            vec!["node_modules".to_string(), "vendor".to_string()]
        );
    }

    #[test]
    fn collect_files_skips_ignored_dirs() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "").unwrap();
        std::fs::create_dir(dir.path().join("target")).unwrap();
        std::fs::write(dir.path().join("target").join("b.rs"), "").unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src").join("c.rs"), "").unwrap();

        let mut found: Vec<String> = collect_files(dir.path(), ".rs", &[])
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        found.sort();
        assert_eq!(found, vec!["a.rs".to_string(), "c.rs".to_string()]);
    }

    #[test]
    fn infer_common_folder_picks_most_frequent() {
        let paths = vec![
            "src/domain/user.rs".to_string(),
            "src/domain/order.rs".to_string(),
            "src/api/route.rs".to_string(),
        ];
        assert_eq!(infer_common_folder(&paths), Some("src/domain/".to_string()));
    }

    #[test]
    fn infer_common_folder_empty_is_none() {
        assert_eq!(infer_common_folder(&[]), None);
    }

    // --- F0-e: agnostic visitor / dominant-extension fallback --------------

    #[test]
    fn visit_includes_exotic_extensions() {
        // A project in a language with no per-stack allow-list entry must still
        // be visited — the pre-F0-e closed allow-list zeroed these.
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("main.zig"), "pub fn main() void {}").unwrap();
        std::fs::write(dir.path().join("user_service.rb"), "class UserService; end").unwrap();
        std::fs::write(dir.path().join("net.c"), "int main(){return 0;}").unwrap();
        std::fs::write(dir.path().join("net.h"), "#pragma once").unwrap();
        // A binary asset must still be skipped.
        std::fs::write(dir.path().join("logo.png"), [0u8, 1, 2, 3]).unwrap();

        let visited = visit(dir.path(), &[]);
        let names: BTreeSet<String> = visited.iter().map(|v| v.rel.clone()).collect();
        assert!(names.contains("main.zig"), "expected .zig visited, got {names:?}");
        assert!(names.contains("user_service.rb"), "expected .rb visited");
        assert!(names.contains("net.c"), "expected .c visited");
        assert!(names.contains("net.h"), "expected .h visited");
        assert!(!names.contains("logo.png"), "binary asset must be skipped");
        assert!(!visited.is_empty());
    }

    #[test]
    fn dominant_source_extension_picks_most_frequent() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.rb"), "").unwrap();
        std::fs::write(dir.path().join("b.rb"), "").unwrap();
        std::fs::write(dir.path().join("c.rb"), "").unwrap();
        std::fs::write(dir.path().join("d.zig"), "").unwrap();
        assert_eq!(dominant_source_extension(dir.path()), Some(".rb".to_string()));
    }

    #[test]
    fn dominant_source_extension_empty_is_none() {
        let dir = tempdir().unwrap();
        assert_eq!(dominant_source_extension(dir.path()), None);
    }

    #[test]
    fn source_extensions_override_force_includes() {
        // `.weirdbin` would normally be visited anyway (not on the deny-list),
        // so prove the override force-includes a deny-listed extension instead.
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"), r#"{"sourceExtensions":["bin"]}"#).unwrap();
        std::fs::write(dir.path().join("blob.bin"), "actually source here").unwrap();
        let visited = visit(dir.path(), &[]);
        assert!(visited.iter().any(|v| v.rel == "blob.bin"));
    }
}
