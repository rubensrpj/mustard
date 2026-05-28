//! `mustard-rt run bugfix-cache` — bug root-cause cache for retry reuse.
//!
//! Ports the in-memory cache pseudo-code in `bugfix/SKILL.md` into a small
//! durable JSON store at `.claude/.bugfix-cache.json`. The cache key is the
//! `rootCauseHash` computed by `/bugfix` ANALYZE; values carry the affected
//! files + the 1-line root cause summary so a retry can skip a second Explore
//! when the hash still matches.
//!
//! Read mode (default — only `--hash X`):
//! - Hit → JSON with `found: true` + the cached entry.
//! - Miss → JSON with `found: false`.
//!
//! Write mode (`--hash X --summary "..." --files a,b,c`):
//! - Records the entry, overwriting any prior value for the same hash.
//!
//! Pure JSON on disk; no DB. Fail-open on every read/write.

use crate::shared::context::{current_spec, session_id};
use mustard_core::time::now_iso8601;
use mustard_core::io::fs::{read_to_string, write_atomic};
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::ClaudePaths;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Options for `mustard-rt run bugfix-cache`.
#[derive(Debug, Clone)]
pub struct BugfixCacheOpts {
    pub hash: String,
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


/// CLI entry.
pub fn run(opts: BugfixCacheOpts) {
    let started = std::time::Instant::now();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let report = if let Some(summary) = opts.summary.clone() {
        let files: Vec<String> = opts
            .files
            .as_deref()
            .map(|s| {
                s.split(',')
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        match record(&cwd, &opts.hash, &summary, &files) {
            Ok(entry) => CacheReport {
                mode: "write",
                hash: opts.hash.clone(),
                found: true,
                entry: Some(entry),
            },
            Err(e) => {
                eprintln!("[bugfix-cache] WARN: write failed: {e}");
                CacheReport {
                    mode: "write",
                    hash: opts.hash.clone(),
                    found: false,
                    entry: None,
                }
            }
        }
    } else {
        let entry = lookup(&cwd, &opts.hash);
        CacheReport {
            mode: "read",
            hash: opts.hash.clone(),
            found: entry.is_some(),
            entry,
        }
    };
    let body = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
    println!("{body}");
    emit_economy(started.elapsed().as_millis());
}

fn emit_economy(duration_ms: u128) {
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| ".".to_string());
    let spec = current_spec(&cwd);
    let duration_capped = i64::try_from(duration_ms).unwrap_or(i64::MAX);
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("bugfix-cache".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: json!({
            "operation": "bugfix-cache",
            "duration_ms": duration_capped,
            "tokens_used": 0,
            "was_rust_only": true,
        }),
        spec,
    };
    let _ = crate::shared::events::route::emit(&cwd, &ev);
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
}
