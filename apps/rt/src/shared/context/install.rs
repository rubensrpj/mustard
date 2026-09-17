//! O modo de instalação (compartilhado ou privado), descoberto e nunca
//! configurado, e o arquivo de instruções que carrega as Guards de uma pasta.

use mustard_core::io::fs;
use mustard_core::InstallMode;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use super::config::{file_fingerprint, FingerprintKey};

/// `CLAUDE.local.md` — the untracked instruction layer a private `scan --full`
/// writes a subproject's Guards into instead of that subproject's `CLAUDE.md`.
///
/// Claude Code discovers a subdirectory `CLAUDE.local.md` exactly as it
/// discovers a subdirectory `CLAUDE.md` — on demand, when a file in that
/// directory is read — and appends it AFTER the shared file in the same
/// directory, so a host repository's own Guards survive and ours are additive
/// (verified against the official memory documentation, 2026-08-17).
///
/// Re-exported from [`mustard_core`] rather than spelled again here: the
/// INSTALLER hides this name (it is a `footprint_rules` entry) and the scan
/// WRITES it. Two literals in two crates is how a mode ends up hiding a path
/// nobody produces — the same class of defect the resolvers below close on the
/// reading side.
pub use mustard_core::CLAUDE_LOCAL_MD;

/// `CLAUDE.md` — the shared instruction file, the only destination before the
/// private mode existed and still the only one an ordinary install writes.
///
/// Re-exported for the same reason as [`CLAUDE_LOCAL_MD`]: every reader of a
/// subproject's Guards resolves its filename through [`guards_file_name`] /
/// [`guards_file`] below, and a reader that retyped the literal is precisely the
/// defect those two exist to make impossible.
pub use mustard_core::CLAUDE_MD;

/// Per-`root` memo of the clone-local exclude file git resolves.
///
/// This is the expensive half — a `git rev-parse --git-path info/exclude`
/// process spawn — and its answer is a property of the repository's shape, which
/// does not change under a running hook, so `root` alone is a sound key.
fn exclude_path_cache() -> &'static Mutex<HashMap<PathBuf, Option<PathBuf>>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Option<PathBuf>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Process-wide cache of the resolved [`InstallMode`], keyed by
/// `(exclude file, its (mtime, len) fingerprint)`.
///
/// Same shape and the same reason as the config cache ([`super::config::project_config_cached`]): the verdict is read
/// repeatedly within one dispatch, and folding the fingerprint into the key
/// means a run that WRITES the exclude file (a private `run upsert`) is never
/// afterwards served the answer from before its own write.
fn install_mode_cache() -> &'static Mutex<HashMap<FingerprintKey, InstallMode>> {
    static CACHE: OnceLock<Mutex<HashMap<FingerprintKey, InstallMode>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Which footprint this project was installed with — autodetected, never
/// configured.
///
/// The mode lives in no versioned file and in no environment variable: a knob in
/// `mustard.json` would itself be the trace the private mode exists to remove
/// (the setting would announce the tool it hides), and an env var is state the
/// operator has to remember. It is chosen ONCE through `run upsert --private`
/// and thereafter read back off the clone-local exclude file that run wrote —
/// so the state is self-evident wherever the project is opened.
///
/// WHICH rules make an exclude file "private" is not decided here: the verdict
/// is [`mustard_core::carries_private_marks`], the same predicate the writer
/// consults, so the reader can never fall behind a mark the writer gained. What
/// this adds — and its only reason to exist beside
/// [`mustard_core::detect_install_mode`] — is the two caches below: a `run`
/// dispatch asks the question once per reader, and each miss costs a process
/// spawn plus a file read.
///
/// Fail-open: no git, no repository, or an unreadable exclude file answers
/// [`InstallMode::Shared`] — today's behaviour, unchanged.
#[must_use]
pub fn install_mode(root: &Path) -> InstallMode {
    let Some(path) = exclude_path_cached(root) else {
        return InstallMode::Shared;
    };
    let key = (path.clone(), file_fingerprint(&path));
    if let Ok(cache) = install_mode_cache().lock()
        && let Some(hit) = cache.get(&key) {
            return *hit;
        }
    let mode = match fs::read_to_string(&path) {
        Ok(body) if mustard_core::carries_private_marks(&body) => InstallMode::Private,
        _ => InstallMode::Shared,
    };
    if let Ok(mut cache) = install_mode_cache().lock() {
        cache.insert(key, mode);
    }
    mode
}

/// The clone-local exclude file for `root` through [`exclude_path_cache`].
fn exclude_path_cached(root: &Path) -> Option<PathBuf> {
    let key = root.to_path_buf();
    if let Ok(cache) = exclude_path_cache().lock()
        && let Some(hit) = cache.get(&key) {
            return hit.clone();
        }
    let resolved = mustard_core::exclude_file(root);
    if let Ok(mut cache) = exclude_path_cache().lock() {
        cache.insert(key, resolved.clone());
    }
    resolved
}

/// The instruction-file name this install OWNS at `root`: [`CLAUDE_LOCAL_MD`]
/// under a private install, [`CLAUDE_MD`] otherwise.
///
/// This is the OWNED answer — the file a census of pending Guards scaffolds
/// looks for. It never falls back: a private install must never be pointed at
/// the file the host repository versions, so "owned" here is exact rather than
/// tolerant.
/// Readers want [`guards_file`] instead.
#[must_use]
pub fn guards_file_name(root: &Path) -> &'static str {
    if install_mode(root).is_private() {
        CLAUDE_LOCAL_MD
    } else {
        CLAUDE_MD
    }
}

/// The instruction file that carries `dir`'s Guards — **the one resolver every
/// reader goes through**.
///
/// Prefers the name this install owns ([`guards_file_name`]) and falls back to
/// [`CLAUDE_MD`] when it is not on disk. Both halves matter:
///
/// - *prefer* — a private `scan --full` writes the Guards to `CLAUDE.local.md`.
///   A reader still opening `CLAUDE.md` gets the CLIENT's file, or nothing, and
///   the harness ships with its central artifact inert. Every consumer named by
///   a repo-wide search for the literal join reads through here, so the mode is
///   decided ONCE: N call sites each choosing a filename IS the defect.
/// - *fall back* — a clone can be made private before its first full scan, and a
///   shared install that later goes private still has its Guards in the shared
///   file. Reading nothing at all is not what the private mode promises; it
///   promises the host's git sees nothing, which the exclude file already
///   delivers.
///
/// Under a shared install both halves are `CLAUDE.md`, so an ordinary project's
/// behaviour is byte-identical to before this existed.
#[must_use]
pub fn guards_file(root: &Path, dir: &Path) -> PathBuf {
    let owned = dir.join(guards_file_name(root));
    if owned.is_file() {
        owned
    } else {
        dir.join(CLAUDE_MD)
    }
}

/// Whether `name` names an instruction file in EITHER layer.
///
/// For the classifiers that receive a path they did not resolve (a write
/// gate's target, a doc linter's directory walk) and only need to
/// know "is this an instruction file at all?". Deliberately mode-blind: a
/// refusal that only recognised the mode's own name would wave through the other
/// layer's file, which is the wrong direction for a guard.
#[must_use]
pub fn is_guards_file_name(name: &str) -> bool {
    name == CLAUDE_MD || name == CLAUDE_LOCAL_MD
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// The install writes `mustard_core::footprint_rules()`; this module reads the
    /// result back. Pin the seam end-to-end rather than restating either side's
    /// list: if the footprint ever renamed or dropped a mark, no exclude file
    /// would carry it, `install_mode` would answer `Shared` forever, and the next
    /// `scan --full` would write straight into a client's own `CLAUDE.md` — with
    /// every other test still green.
    ///
    /// The second half pins the OTHER direction, the one that lives in this
    /// crate: `scan_claude` writes [`CLAUDE_LOCAL_MD`], and a name no install
    /// rule hides is a private scan that git can see.
    #[test]
    fn a_freshly_written_footprint_reads_back_as_private() {
        let rules = mustard_core::footprint_rules();
        assert!(
            mustard_core::carries_private_marks(&rules.join("\n")),
            "an exclude file carrying the whole footprint must detect as private: {rules:?}",
        );
        assert!(
            rules.iter().any(|r| r == CLAUDE_LOCAL_MD),
            "{CLAUDE_LOCAL_MD} is what a private scan writes; no rule hides it: {rules:?}",
        );
    }

    // -----------------------------------------------------------------------
    // guards_file — the ONE resolver every reader of a subproject's Guards uses
    // -----------------------------------------------------------------------

    /// A tree git knows nothing about is a shared install, so the resolver names
    /// the shared file — whether or not a local layer happens to sit beside it.
    /// This is the half that keeps an ordinary project byte-identical to before.
    #[test]
    fn a_shared_install_resolves_the_shared_file_even_beside_a_local_layer() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("apps").join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join(CLAUDE_LOCAL_MD), "stray\n").unwrap();
        std::fs::write(sub.join(CLAUDE_MD), "theirs\n").unwrap();

        assert_eq!(guards_file_name(dir.path()), CLAUDE_MD);
        assert_eq!(guards_file(dir.path(), &sub), sub.join(CLAUDE_MD));
    }

    /// Both instruction layers are recognised by name, and nothing else is —
    /// the mode-blind test the refusals and the doc linter classify with.
    #[test]
    fn only_the_two_instruction_layers_are_guards_file_names() {
        assert!(is_guards_file_name(CLAUDE_MD));
        assert!(is_guards_file_name(CLAUDE_LOCAL_MD));
        assert!(!is_guards_file_name("claude.md"), "the name is case-sensitive on disk");
        assert!(!is_guards_file_name("CLAUDE.local.md.bak"));
        assert!(!is_guards_file_name("README.md"));
    }

    /// Fail-open: a tree git knows nothing about is a shared install, not an
    /// error and not a panic.
    #[test]
    fn a_tree_without_a_repository_reads_as_shared() {
        let dir = tempdir().unwrap();
        assert_eq!(install_mode(dir.path()), InstallMode::Shared);
    }
}
