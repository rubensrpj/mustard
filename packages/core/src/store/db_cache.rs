//! [`DbCache`] — open-once-and-reuse a [`SqliteEventStore`] per database path.
//!
//! The harness spawns a fresh process per hook event, so a single-shot tool
//! never benefits from caching. The multi-project consumers do: the dashboard
//! (Wave 3) holds several projects open at once, and the long-lived `rt`
//! processes (Wave 2) touch the same database repeatedly within one process.
//! Re-running [`SqliteEventStore::new`] on every access pays the open cost each
//! time even with the `user_version` fast-path; caching the store skips it
//! entirely after the first open.
//!
//! This is **not** a connection pool — no external crate, no checkout/return
//! protocol. It is a thin map from path to a shared store. Reuse keeps the
//! existing [`EventSink`](super::event_store::EventSink) trait; no parallel
//! access trait is introduced.
//!
//! [`SqliteEventStore`] owns a [`rusqlite::Connection`], which is `Send` but
//! not `Sync`. Wrapping each store in a `Mutex` makes the shared handle
//! `Send + Sync` so the dashboard can hold the cache behind an `Arc` across
//! Tauri command handlers. WAL mode already permits concurrent *readers* at the
//! `SQLite` layer; the `Mutex` only serializes access to the one cached
//! connection — separate paths never contend.

use crate::error::Result;
use crate::store::sqlite_store::SqliteEventStore;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// A shared, lock-guarded handle to one cached store.
///
/// Cloning is cheap (an `Arc` bump) and every clone points at the same
/// underlying [`SqliteEventStore`].
pub type SharedStore = Arc<Mutex<SqliteEventStore>>;

/// Caches one [`SqliteEventStore`] per database path, opening lazily on first
/// request and reusing it thereafter.
///
/// Cheap to clone — the inner map is shared behind an `Arc`. All methods are
/// fail-open in the same sense as the store: they return [`Result`] and never
/// panic (a poisoned lock is recovered, since the guarded data is just a map).
#[derive(Clone, Default)]
pub struct DbCache {
    inner: Arc<Mutex<HashMap<PathBuf, SharedStore>>>,
}

impl std::fmt::Debug for DbCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let len = self.inner.lock().map_or(0, |m| m.len());
        f.debug_struct("DbCache").field("cached_paths", &len).finish()
    }
}

impl DbCache {
    /// Construct an empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the shared store for `path`, opening and caching it on first use.
    ///
    /// Subsequent calls with the same `path` return the **same**
    /// [`SharedStore`] (the identical `Arc`), so the underlying connection is
    /// opened exactly once per path.
    ///
    /// # Errors
    ///
    /// Returns the error from [`SqliteEventStore::new`] when the first open of
    /// a path fails; the failed path is not cached, so a later call retries.
    pub fn get(&self, path: impl AsRef<Path>) -> Result<SharedStore> {
        let key = path.as_ref().to_path_buf();
        // Recover from a poisoned lock: the guarded data is a plain map, so a
        // panic in another thread cannot leave it in a logically broken state.
        let mut map = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(existing) = map.get(&key) {
            return Ok(Arc::clone(existing));
        }
        let store = SqliteEventStore::new(&key)?;
        let shared: SharedStore = Arc::new(Mutex::new(store));
        map.insert(key, Arc::clone(&shared));
        Ok(shared)
    }

    /// Number of distinct paths currently cached. Mainly for diagnostics/tests.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.lock().map_or(0, |m| m.len())
    }

    /// Whether the cache holds no open stores.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::event_store::EventSink;
    use crate::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
    use serde_json::json;
    use tempfile::tempdir;

    fn sample_event() -> HarnessEvent {
        // W5: only `pipeline.*` events are persisted by the SqliteEventStore
        // sink — tool/agent/qa events route through the per-spec NDJSON writer.
        // The round-trip check below replays from `pipeline_events`, so seed
        // with a lifecycle event so the assert matches the store's contract.
        HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-05-22T00:00:00.000Z".to_string(),
            session_id: "s-cache".to_string(),
            wave: 0,
            actor: Actor { kind: ActorKind::Hook, id: None, actor_type: None },
            event: "pipeline.scope".to_string(),
            payload: json!({}),
            spec: None,
        }
    }

    #[test]
    fn get_returns_same_instance_for_same_path() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("mustard.db");
        let cache = DbCache::new();

        let a = cache.get(&db).unwrap();
        let b = cache.get(&db).unwrap();

        // Same path -> identical Arc (opened once).
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn distinct_paths_get_distinct_instances() {
        let dir = tempdir().unwrap();
        let cache = DbCache::new();
        let a = cache.get(dir.path().join("a.db")).unwrap();
        let b = cache.get(dir.path().join("b.db")).unwrap();
        assert!(!Arc::ptr_eq(&a, &b));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn shared_store_is_usable_through_the_mutex() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("mustard.db");
        let cache = DbCache::new();
        let shared = cache.get(&db).unwrap();
        {
            let store = shared.lock().unwrap();
            store.append(&sample_event()).unwrap();
        }
        // A second handle sees the write — same connection.
        let shared2 = cache.get(&db).unwrap();
        let store2 = shared2.lock().unwrap();
        assert_eq!(store2.replay().unwrap().len(), 1);
    }
}
