//! `mustard-rt run bugfix-cache` — bug root-cause cache for retry reuse.
//!
//! Ports the in-memory cache pseudo-code in `bugfix/SKILL.md` into a small
//! durable JSON store at `.claude/.bugfix-cache.json`. The cache key is a
//! `rootCauseHash`; values carry the affected files + the 1-line root cause
//! summary so a retry can skip a second Explore when the hash still matches.
//!
//! ## Deterministic key (F5-a item 3)
//!
//! The `rootCauseHash` used to be computed *externally* by the LLM in the
//! `/bugfix` ANALYZE step and handed to the binary via `--hash`. That made the
//! key non-reproducible: the same bug analysed twice could hash differently,
//! silently missing the cache. The hash is now computed **deterministically in
//! Rust** by [`root_cause_hash`] — `SHA-256` over the *sorted* affected paths
//! plus a normalised prefix of the error message — so the same (files, error)
//! always yields the same key. The caller passes the raw inputs (`--files` +
//! `--error`); the binary owns the hashing.
//!
//! An explicit `--hash` still works and takes priority (override / legacy-key
//! compatibility), so already-stored keys remain readable.
//!
//! Read mode (default — no `--summary`):
//! - `--hash X` → look up `X` directly.
//! - `--error "…" --files a,b,c` → compute the hash, then look it up.
//! - Hit → JSON with `found: true` + the cached entry; miss → `found: false`.
//!
//! Write mode (`--summary "…"` present, with either `--hash` or `--error`/`--files`):
//! - Records the entry, overwriting any prior value for the same hash.
//!
//! Pure JSON on disk; no DB. Fail-open on every read/write.

use serde_json::json;
use mustard_core::domain::model::event::ActorKind;
use crate::shared::context;
use crate::shared::events::economy;
use mustard_core::time::now_iso8601;
use mustard_core::io::fs::{read_to_string, write_atomic};
use mustard_core::ClaudePaths;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Options for `mustard-rt run bugfix-cache`.
#[derive(Debug, Clone)]
pub struct BugfixCacheOpts {
    /// Explicit hash override. When `None`, the hash is derived from
    /// `error` + `files` via [`root_cause_hash`].
    pub hash: Option<String>,
    /// Error message / failure signature — input to the deterministic hash.
    pub error: Option<String>,
    pub summary: Option<String>,
    pub files: Option<String>,
}

/// One cached entry. Lenient — unknown fields land in `extra` so a future
/// pipeline writer can extend the schema without breaking old readers.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CacheEntry {
    pub hash: String,
    pub summary: String,
    pub files: Vec<String>,
    pub created_at: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// JSON report.
#[derive(Debug, Serialize)]
pub struct CacheReport {
    pub mode: &'static str,
    pub hash: String,
    pub found: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry: Option<CacheEntry>,
}

/// Compute the deterministic `rootCauseHash` for a bug.
///
/// `SHA-256` (lowercase hex) over a canonical material string built from:
/// 1. the affected `files`, **sorted and de-duplicated** (path order must not
///    change the key) and trimmed, joined with `\n`;
/// 2. a separator (`\x1e`, the ASCII record separator — never appears in a
///    path or message);
/// 3. a **normalised prefix** of the error message: lowercased, internal
///    whitespace runs collapsed to a single space, trimmed, and capped at
///    [`ERROR_PREFIX_LEN`] bytes (volatile tail noise — line/column numbers,
///    pointers, timestamps — is dropped so the same root cause hashes stable).
///
/// Pure and reproducible: identical `(files, error)` ⇒ identical hash; any
/// change to the file set or the leading error text ⇒ a different hash.
#[must_use]
pub fn root_cause_hash(files: &[String], error_message: &str) -> String {
    let mut sorted: Vec<String> = files
        .iter()
        .map(|f| f.trim().to_string())
        .filter(|f| !f.is_empty())
        .collect();
    sorted.sort();
    sorted.dedup();

    let material = format!(
        "{}\u{1e}{}",
        sorted.join("\n"),
        normalize_error_prefix(error_message)
    );

    let mut h = crate::util::sha256::Sha256::new();
    h.update(material.as_bytes());
    h.hex_digest()
}

/// Max bytes of the normalised error message that feed the hash. Long enough to
/// capture the failure's identity, short enough to drop the volatile tail.
const ERROR_PREFIX_LEN: usize = 200;

/// Lowercase, collapse internal whitespace to single spaces, trim, and cap at
/// [`ERROR_PREFIX_LEN`] bytes (on a char boundary).
fn normalize_error_prefix(error_message: &str) -> String {
    let collapsed: String = error_message
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if collapsed.len() <= ERROR_PREFIX_LEN {
        collapsed
    } else {
        // Truncate on a char boundary at or below the byte cap.
        let mut end = ERROR_PREFIX_LEN;
        while end > 0 && !collapsed.is_char_boundary(end) {
            end -= 1;
        }
        collapsed[..end].to_string()
    }
}

fn cache_path(cwd: &Path) -> PathBuf {
    // `.bugfix-cache.json` lives directly under `.claude/`; no typed accessor
    // exists for this legacy file, so route via `claude_dir()` to keep the
    // boundary owned by `ClaudePaths`. An I1 rejection collapses to an empty
    // path which the caller treats as a cache miss (fail-open).
    ClaudePaths::for_project(cwd)
        .map(|p| p.claude_dir().join(".bugfix-cache.json"))
        .unwrap_or_default()
}

fn load(cwd: &Path) -> BTreeMap<String, CacheEntry> {
    let Ok(text) = read_to_string(cache_path(cwd)) else {
        return BTreeMap::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn save(cwd: &Path, map: &BTreeMap<String, CacheEntry>) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(map)
        .unwrap_or_else(|_| "{}".to_string());
    write_atomic(cache_path(cwd), format!("{text}\n").as_bytes())
        .map_err(|e| std::io::Error::other(e.to_string()))
}

/// Pure read: return the cached entry for `hash`, if any.
#[must_use]
pub fn lookup(cwd: &Path, hash: &str) -> Option<CacheEntry> {
    load(cwd).get(hash).cloned()
}

/// Pure write: record a new entry, returning the saved value.
pub fn record(
    cwd: &Path,
    hash: &str,
    summary: &str,
    files: &[String],
) -> std::io::Result<CacheEntry> {
    let mut map = load(cwd);
    let entry = CacheEntry {
        hash: hash.to_string(),
        summary: summary.to_string(),
        files: files.to_vec(),
        created_at: now_iso8601(),
        extra: BTreeMap::new(),
    };
    map.insert(hash.to_string(), entry.clone());
    save(cwd, &map)?;
    Ok(entry)
}


/// Split a comma-separated `--files` value into trimmed, non-empty paths.
fn parse_files(files: Option<&str>) -> Vec<String> {
    files
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// CLI entry.
pub fn run(opts: BugfixCacheOpts) {
    let started = std::time::Instant::now();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let files = parse_files(opts.files.as_deref());

    // Resolve the effective key: an explicit `--hash` wins (override / legacy
    // compat); otherwise derive it deterministically from `--files` + `--error`.
    let hash = match opts.hash.clone() {
        Some(h) => h,
        None => root_cause_hash(&files, opts.error.as_deref().unwrap_or("")),
    };

    let report = if let Some(summary) = opts.summary.clone() {
        match record(&cwd, &hash, &summary, &files) {
            Ok(entry) => CacheReport {
                mode: "write",
                hash: hash.clone(),
                found: true,
                entry: Some(entry),
            },
            Err(e) => {
                eprintln!("[bugfix-cache] WARN: write failed: {e}");
                CacheReport {
                    mode: "write",
                    hash: hash.clone(),
                    found: false,
                    entry: None,
                }
            }
        }
    } else {
        let entry = lookup(&cwd, &hash);
        CacheReport {
            mode: "read",
            hash: hash.clone(),
            found: entry.is_some(),
            entry,
        }
    };
    let body = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
    println!("{body}");
    economy::emit_operation(&context::cwd(), ActorKind::Orchestrator, "bugfix-cache", started.elapsed().as_millis() as u64, None, json!({}));
}


#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn lookup_miss_returns_none() {
        let dir = tempdir().unwrap();
        assert!(lookup(dir.path(), "abc").is_none());
    }

    #[test]
    fn record_then_lookup_round_trips() {
        let dir = tempdir().unwrap();
        let files = vec!["a.rs".to_string(), "b.rs".to_string()];
        let entry = record(dir.path(), "h1", "null guard missing", &files).unwrap();
        assert_eq!(entry.hash, "h1");
        let hit = lookup(dir.path(), "h1").expect("should be cached");
        assert_eq!(hit.summary, "null guard missing");
        assert_eq!(hit.files, files);
    }

    #[test]
    fn record_overwrites_existing_hash() {
        let dir = tempdir().unwrap();
        record(dir.path(), "h1", "v1", &[]).unwrap();
        record(dir.path(), "h1", "v2", &["new.rs".to_string()]).unwrap();
        let hit = lookup(dir.path(), "h1").unwrap();
        assert_eq!(hit.summary, "v2");
        assert_eq!(hit.files, vec!["new.rs".to_string()]);
    }

    #[test]
    fn report_serializes_to_required_fields() {
        let r = CacheReport {
            mode: "read",
            hash: "h".to_string(),
            found: false,
            entry: None,
        };
        let v = serde_json::to_value(r).unwrap();
        assert_eq!(v["mode"], json!("read"));
        assert_eq!(v["hash"], json!("h"));
        assert_eq!(v["found"], json!(false));
    }

    #[test]
    fn missing_cache_file_lookup_is_failopen() {
        let dir = tempdir().unwrap();
        // No file at all → fall-through to miss; never panics.
        assert!(lookup(dir.path(), "anything").is_none());
    }

    #[test]
    fn root_cause_hash_is_reproducible() {
        let files = vec!["src/a.rs".to_string(), "src/b.rs".to_string()];
        let h1 = root_cause_hash(&files, "TypeError: cannot read x");
        let h2 = root_cause_hash(&files, "TypeError: cannot read x");
        assert_eq!(h1, h2, "same inputs ⇒ same hash");
        assert_eq!(h1.len(), 64, "lowercase sha256 hex");
        assert!(h1.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn root_cause_hash_is_order_independent_for_files() {
        let a = vec!["src/b.rs".to_string(), "src/a.rs".to_string()];
        let b = vec!["src/a.rs".to_string(), "src/b.rs".to_string()];
        // Files are sorted before hashing, so path order does not change the key.
        assert_eq!(root_cause_hash(&a, "boom"), root_cause_hash(&b, "boom"));
        // Duplicates collapse too.
        let dup = vec!["src/a.rs".to_string(), "src/a.rs".to_string(), "src/b.rs".to_string()];
        assert_eq!(root_cause_hash(&dup, "boom"), root_cause_hash(&b, "boom"));
    }

    #[test]
    fn root_cause_hash_differs_on_different_inputs() {
        let files = vec!["src/a.rs".to_string()];
        let base = root_cause_hash(&files, "null pointer in handler");
        // Different error → different hash.
        assert_ne!(base, root_cause_hash(&files, "index out of bounds"));
        // Different file set → different hash.
        assert_ne!(base, root_cause_hash(&["src/c.rs".to_string()], "null pointer in handler"));
    }

    #[test]
    fn root_cause_hash_normalizes_error_whitespace_and_case() {
        let files = vec!["src/a.rs".to_string()];
        // Case + internal whitespace collapse to one canonical form.
        assert_eq!(
            root_cause_hash(&files, "TypeError:  Cannot   read"),
            root_cause_hash(&files, "typeerror: cannot read"),
        );
    }

    #[test]
    fn root_cause_hash_drops_volatile_tail_past_prefix_cap() {
        let files = vec!["src/a.rs".to_string()];
        let head = "x".repeat(ERROR_PREFIX_LEN);
        // Two messages share the first ERROR_PREFIX_LEN chars but differ after.
        let a = format!("{head} at line 42");
        let b = format!("{head} at line 9999 pointer 0xdeadbeef");
        assert_eq!(root_cause_hash(&files, &a), root_cause_hash(&files, &b));
    }
}
