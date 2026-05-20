//! [`SqliteSpecReader`] — production [`SpecReader`] adapter over
//! `SqliteEventStore`.
//!
//! Every method loads the relevant slice of events from the store and feeds
//! it to the matching projection in [`crate::projection`]. The reader itself
//! is a *thin* layer — it picks queries, never folds.

use crate::reader::error::Result;
use crate::model::view::{
    QualityRollup, SpecFilter, SpecStatusFilter, SpecSummary, SpecView, TimeWindow, TimelineNode,
    WaveView, WorkspaceSummary,
};
use crate::projection::{
    project_quality, project_spec_view, project_timeline, project_waves, project_workspace,
};
use crate::reader::SpecReader;
use crate::store::sqlite_store::SqliteEventStore;
use std::path::{Path, PathBuf};

/// Production [`SpecReader`] backed by the harness `mustard.db`.
///
/// Stores the project directory path and opens a fresh
/// [`SqliteEventStore`] per call. Two reasons not to keep a long-lived
/// connection:
///
/// 1. **`Send` + `Sync`.** `rusqlite::Connection` is neither (it holds a
///    `RefCell` internally). The [`SpecReader`] trait is `Send + Sync` so a
///    consumer can share readers across Tauri command handlers and threads.
///    Holding the connection inside the reader would force a `Mutex` and
///    serialize every query — pointless when SQLite's own WAL already permits
///    concurrent readers.
/// 2. **Migration freshness.** Opening the store runs
///    [`migrations::apply`](crate::store::migrations::apply) every time,
///    which is cheap when the database is already at the latest version
///    (one `SELECT` against `_mustard_meta`) and free of side effects.
#[derive(Clone, Debug)]
pub struct SqliteSpecReader {
    project_dir: PathBuf,
}

impl SqliteSpecReader {
    /// Build a reader for `project_dir`'s harness DB.
    ///
    /// Resolves the database path through
    /// [`SqliteEventStore::for_project`] on each call, which honours the
    /// `MUSTARD_DB_PATH` env var when set.
    ///
    /// # Errors
    ///
    /// Returns [`ReadError`](crate::reader::error::ReadError) if the DB cannot be
    /// opened during this initial probe.
    pub fn for_project(project_dir: impl AsRef<Path>) -> Result<Self> {
        // Verify we can open the store at least once — gives a clear error at
        // construction time rather than at first query.
        let _ = SqliteEventStore::for_project(project_dir.as_ref())?;
        Ok(Self {
            project_dir: project_dir.as_ref().to_path_buf(),
        })
    }

    /// Open a fresh store for one query. Cheap — WAL mode is on, schema is
    /// idempotent, migrations are version-gated.
    fn store(&self) -> Result<SqliteEventStore> {
        Ok(SqliteEventStore::for_project(&self.project_dir)?)
    }

    /// Return all distinct spec names known to the store, excluding the
    /// `__orphan__` sentinel.
    fn distinct_specs(&self) -> Result<Vec<String>> {
        let mut specs = self.store()?.distinct_specs()?;
        specs.retain(|s| s != "__orphan__");
        Ok(specs)
    }
}

impl SpecReader for SqliteSpecReader {
    fn spec_view(&self, spec: &str) -> Result<Option<SpecView>> {
        if spec.is_empty() {
            return Err(crate::reader::error::ReadError::invalid("spec name cannot be empty"));
        }
        let events = self.store()?.query(Some(spec))?;
        if events.is_empty() {
            return Ok(None);
        }
        Ok(Some(project_spec_view(spec, &events)))
    }

    fn spec_summary(&self, spec: &str) -> Result<Option<SpecSummary>> {
        Ok(self.spec_view(spec)?.as_ref().map(SpecSummary::from))
    }

    fn list_specs(&self, filter: &SpecFilter) -> Result<Vec<SpecSummary>> {
        let names = self.distinct_specs()?;
        let needle = filter
            .search
            .as_deref()
            .map(str::to_lowercase)
            .filter(|s| !s.is_empty());
        let mut summaries: Vec<SpecSummary> = Vec::with_capacity(names.len());
        for name in names {
            if let Some(n) = &needle {
                if !name.to_lowercase().contains(n) {
                    continue;
                }
            }
            let Some(view) = self.spec_view(&name)? else {
                continue;
            };
            let summary: SpecSummary = (&view).into();
            // Filter by status bucket if requested.
            let keep = match filter.status.as_ref().unwrap_or(&SpecStatusFilter::Any) {
                SpecStatusFilter::Any => true,
                SpecStatusFilter::Active => summary.status.is_active(),
                SpecStatusFilter::Closed => summary.status.is_terminal(),
            };
            if keep {
                summaries.push(summary);
            }
        }
        // Sort: most recently active first, then by name.
        summaries.sort_by(|a, b| b.last_event_at.cmp(&a.last_event_at).then(a.spec.cmp(&b.spec)));
        Ok(summaries)
    }

    fn waves(&self, spec: &str) -> Result<Vec<WaveView>> {
        if spec.is_empty() {
            return Err(crate::reader::error::ReadError::invalid("spec name cannot be empty"));
        }
        let events = self.store()?.query(Some(spec))?;
        Ok(project_waves(spec, &events))
    }

    fn quality(&self, spec: &str) -> Result<QualityRollup> {
        if spec.is_empty() {
            return Err(crate::reader::error::ReadError::invalid("spec name cannot be empty"));
        }
        let events = self.store()?.query(Some(spec))?;
        Ok(project_quality(spec, &events))
    }

    fn timeline(&self, spec: &str, window: TimeWindow) -> Result<Vec<TimelineNode>> {
        if spec.is_empty() {
            return Err(crate::reader::error::ReadError::invalid("spec name cannot be empty"));
        }
        let events = self.store()?.query(Some(spec))?;
        Ok(project_timeline(spec, &events, window))
    }

    fn workspace_summary(&self) -> Result<WorkspaceSummary> {
        let events = self.store()?.replay()?;
        let now_ms = now_epoch_ms();
        Ok(project_workspace(&events, now_ms))
    }
}

/// Wall-clock `now` in epoch milliseconds. Used only by `workspace_summary`;
/// projections themselves are pure.
fn now_epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::view::SpecStatus;
    use crate::store::event_store::EventSink;
    use crate::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
    use serde_json::json;
    use tempfile::tempdir;

    fn open_reader(dir: &std::path::Path) -> SqliteSpecReader {
        SqliteSpecReader::for_project(dir).unwrap()
    }

    fn store_for(dir: &std::path::Path) -> SqliteEventStore {
        SqliteEventStore::for_project(dir).unwrap()
    }

    fn event(spec: &str, ts: &str, kind: &str, payload: serde_json::Value) -> HarnessEvent {
        HarnessEvent {
            v: SCHEMA_VERSION,
            ts: ts.into(),
            session_id: "s1".into(),
            wave: 0,
            actor: Actor { kind: ActorKind::Hook, id: None, actor_type: None },
            event: kind.into(),
            payload,
            spec: Some(spec.into()),
        }
    }

    #[test]
    fn spec_view_returns_none_for_unknown_spec() {
        let dir = tempdir().unwrap();
        let _ = store_for(dir.path());
        let reader = open_reader(dir.path());
        let view = reader.spec_view("never-existed").unwrap();
        assert!(view.is_none());
    }

    #[test]
    fn spec_view_projects_events_into_view() {
        let dir = tempdir().unwrap();
        let store = store_for(dir.path());
        store
            .append(&event(
                "auth",
                "2026-05-20T10:00:00Z",
                "pipeline.scope",
                json!({ "scope": "full", "lang": "pt" }),
            ))
            .unwrap();
        store
            .append(&event(
                "auth",
                "2026-05-20T10:00:01Z",
                "tool.use",
                json!({}),
            ))
            .unwrap();

        let reader = open_reader(dir.path());
        let view = reader.spec_view("auth").unwrap().unwrap();
        assert_eq!(view.spec, "auth");
        assert_eq!(view.status, SpecStatus::Planning);
        assert_eq!(view.tools_used, 1);
        assert_eq!(view.lang.as_deref(), Some("pt"));
    }

    #[test]
    fn empty_spec_name_returns_invalid_error() {
        let dir = tempdir().unwrap();
        let _ = store_for(dir.path());
        let reader = open_reader(dir.path());
        assert!(reader.spec_view("").is_err());
        assert!(reader.waves("").is_err());
        assert!(reader.quality("").is_err());
    }

    #[test]
    fn list_specs_excludes_orphans_and_applies_search() {
        let dir = tempdir().unwrap();
        let store = store_for(dir.path());
        for name in ["auth", "billing", "__orphan__"] {
            store
                .append(&event(name, "2026-05-20T10:00:00Z", "tool.use", json!({})))
                .unwrap();
        }
        let reader = open_reader(dir.path());

        let any = reader.list_specs(&SpecFilter::default()).unwrap();
        let names: Vec<_> = any.iter().map(|s| s.spec.clone()).collect();
        assert!(names.contains(&"auth".to_string()));
        assert!(names.contains(&"billing".to_string()));
        assert!(!names.contains(&"__orphan__".to_string()));

        let mut filter = SpecFilter::default();
        filter.search = Some("auth".into());
        let only_auth = reader.list_specs(&filter).unwrap();
        assert_eq!(only_auth.len(), 1);
        assert_eq!(only_auth[0].spec, "auth");
    }
}
