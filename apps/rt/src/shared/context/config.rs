//! A configuração do projeto (`mustard.json`), lida uma vez por versão do
//! arquivo em cada processo.

use mustard_core::ProjectConfig;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

/// A cheap fingerprint of one file — `(mtime, len)`, or `None` when it is absent
/// / unstat-able. A `stat` is far cheaper than the open + read + parse it lets a
/// cache hit skip, and folding it into the cache KEY (rather than invalidating
/// by hand) is what keeps a rewritten file from being served stale.
pub(super) fn file_fingerprint(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// A cheap fingerprint of `<root>/mustard.json` — see [`file_fingerprint`].
fn mustard_json_fingerprint(root: &Path) -> Option<(SystemTime, u64)> {
    file_fingerprint(&root.join("mustard.json"))
}

/// Key of a fingerprinted memo: the path an answer belongs to, plus the
/// [`file_fingerprint`] of the file the answer was derived from. Shared by the
/// two caches below, which are the same idea over two different files.
pub(super) type FingerprintKey = (PathBuf, Option<(SystemTime, u64)>);

/// Process-wide cache of [`ProjectConfig`] keyed by `(root, mustard.json
/// fingerprint)`.
///
/// Every gate in one `PreToolUse(Write|Edit)` dispatch independently needs the
/// project config (size / close / boundary / work-branch), so before this seam a
/// single dispatch re-read and re-parsed `mustard.json` 3-4 times. This collapses
/// them to ONE read+parse per file version: the first caller loads and stores, the
/// rest clone the cached value. Mirrors the process-wide, path-keyed memo
/// [`mustard_core::io::workspace::workspace_root`] already uses — the same
/// one-shot-process lifetime, so "process-wide" is "per dispatch" in production.
/// The `(mtime, len)` fingerprint re-loads a rewritten config, so an in-place
/// edit is never served stale (matters only to tests that mutate `mustard.json`).
fn config_cache() -> &'static Mutex<HashMap<FingerprintKey, ProjectConfig>> {
    static CACHE: OnceLock<Mutex<HashMap<FingerprintKey, ProjectConfig>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Load `<root>/mustard.json` through the process-wide [`config_cache`], returning
/// an owned [`ProjectConfig`] — a drop-in for [`ProjectConfig::load`] that skips
/// the disk read+parse when the same file version was already loaded this process.
/// Fail-open (defaults on any IO/parse error), inherited from the underlying load.
#[must_use]
pub fn project_config_cached(root: &Path) -> ProjectConfig {
    let key = (root.to_path_buf(), mustard_json_fingerprint(root));
    if let Ok(cache) = config_cache().lock()
        && let Some(hit) = cache.get(&key) {
            return hit.clone();
        }
    let config = ProjectConfig::load(root);
    if let Ok(mut cache) = config_cache().lock() {
        cache.insert(key, config.clone());
    }
    config
}
