//! [`SqliteSpecReader`] — production [`SpecReader`] adapter over
//! `SqliteEventStore`.
//!
//! Every method loads the relevant slice of events from the store and feeds
//! it to the matching projection in [`crate::projection`]. The reader itself
//! is a *thin* layer — it picks queries, never folds.

use crate::reader::error::Result;
#[allow(deprecated)] // empty-view detection and child fallback still read the legacy SpecStatus.
use crate::model::view::SpecStatus;
use crate::model::view::{
    Outcome, QualityRollup, SpecChild, SpecFilter, SpecState, SpecStatusFilter, SpecSummary,
    SpecView, Stage, TimeWindow, TimelineNode, WaveView, WorkspaceSummary,
};
use crate::projection::{
    project_quality, project_spec_view, project_spec_view_with_header, project_timeline,
    project_waves, project_workspace,
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
///    serialize every query — pointless when `SQLite`'s own WAL already permits
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

    /// Build a [`SpecSummary`] without populating `children_count`.
    ///
    /// [`SpecReader::children_of`] resolves each child's status by calling
    /// back into the reader — and the public [`Self::spec_summary`] populates
    /// `children_count` by calling `children_of`. Sharing a single entry
    /// point would recurse forever when a child has its own link events. This
    /// internal variant breaks the cycle: it produces the lean summary
    /// directly from the rich view and leaves `children_count = 0`.
    fn spec_summary_core(&self, spec: &str) -> Result<Option<SpecSummary>> {
        Ok(self.spec_view(spec)?.as_ref().map(SpecSummary::from))
    }

    /// Fold all `spec.link` events into the children of `parent`.
    ///
    /// Returns one `(child_name, reason)` tuple per distinct child, with the
    /// first-seen reason winning when the same pair is linked more than once.
    /// Reads via [`SqliteEventStore::replay`] and filters in Rust — keeps the
    /// store's public surface untouched at the cost of a full event scan,
    /// which is fine for the harness's event volumes.
    fn link_payloads_for(&self, parent: &str) -> Result<Vec<(String, Option<String>)>> {
        let events = self.store()?.replay()?;
        let mut seen: std::collections::BTreeMap<String, Option<String>> =
            std::collections::BTreeMap::new();
        let mut order: Vec<String> = Vec::new();
        for ev in &events {
            if ev.event != "spec.link" {
                continue;
            }
            let Some(payload_parent) =
                ev.payload.get("parent").and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            if payload_parent != parent {
                continue;
            }
            let Some(child) = ev.payload.get("child").and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            let reason = ev
                .payload
                .get("reason")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            if !seen.contains_key(child) {
                seen.insert(child.to_string(), reason);
                order.push(child.to_string());
            }
        }
        Ok(order
            .into_iter()
            .map(|name| {
                let reason = seen.remove(&name).unwrap_or(None);
                (name, reason)
            })
            .collect())
    }
}

impl SqliteSpecReader {
    /// Resolve the on-disk `spec.md` path for `spec` under this project.
    ///
    /// The path is flat — no `active/`, `completed/`, or `superseded/`
    /// sub-buckets — because Wave 2 / Wave 5 of
    /// `2026-05-21-flatten-spec-layout-and-multi-collab` removes those buckets
    /// from the repo. Returns the path unconditionally; the projection itself
    /// fails open (`std::fs::read_to_string` → `None`) when the file is missing.
    fn spec_md_path(&self, spec: &str) -> std::path::PathBuf {
        let base = self.project_dir.join(".claude").join("spec").join(spec);
        let primary = base.join("spec.md");
        if primary.exists() {
            return primary;
        }
        let wave_plan = base.join("wave-plan.md");
        if wave_plan.exists() {
            return wave_plan;
        }
        primary
    }

    /// Scan `.claude/spec/{spec}/wave-N-{role}/` to build a planned wave
    /// list when no task events exist yet. Returns waves sorted by number.
    fn waves_from_disk(&self, spec: &str) -> Vec<WaveView> {
        let base = self.project_dir.join(".claude").join("spec").join(spec);
        let Ok(entries) = std::fs::read_dir(&base) else {
            return Vec::new();
        };
        let mut planned: Vec<WaveView> = Vec::new();
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(rest) = name.strip_prefix("wave-") else {
                continue;
            };
            // `wave-N-{role}` — split on first `-` after the number.
            let Some((num_str, role)) = rest.split_once('-') else {
                continue;
            };
            let Ok(num) = num_str.parse::<u32>() else {
                continue;
            };
            let mut view = WaveView::queued(num);
            view.role = Some(role.to_string());
            planned.push(view);
        }
        planned.sort_by_key(|w| w.wave);
        planned
    }
}

impl SpecReader for SqliteSpecReader {
    #[allow(deprecated)] // `NoEvents` is the empty-stream sentinel — only the legacy enum carries it.
    fn spec_view(&self, spec: &str) -> Result<Option<SpecView>> {
        if spec.is_empty() {
            return Err(crate::reader::error::ReadError::invalid("spec name cannot be empty"));
        }
        let events = self.store()?.query(Some(spec))?;
        if !events.is_empty() {
            return Ok(Some(project_spec_view(spec, &events)));
        }
        // Empty event log: fall back to the spec.md header (Wave 1 of
        // 2026-05-21-flatten-spec-layout-and-multi-collab). A teammate who
        // pulled the repo sees the canonical status without re-emitting
        // events. The synthetic-emit hook stays off here — the dashboard
        // only reads; the backfill path is driven by `mustard-rt run
        // rebuild-specs` (Wave 5).
        let path = self.spec_md_path(spec);
        let view = project_spec_view_with_header(spec, &events, Some(path.as_path()), None);
        if view.status == SpecStatus::NoEvents && view.phase.is_none() && view.scope.is_none() {
            // Header was missing or empty — the spec is genuinely unknown.
            return Ok(None);
        }
        Ok(Some(view))
    }

    fn spec_summary(&self, spec: &str) -> Result<Option<SpecSummary>> {
        let Some(mut summary) = self.spec_summary_core(spec)? else {
            return Ok(None);
        };
        // Populate the sub-spec count by replaying the link log. `children_of`
        // routes through `spec_summary_core` for each child, so this stays
        // recursion-free.
        summary.children_count = u32::try_from(self.children_of(spec)?.len()).unwrap_or(u32::MAX);
        Ok(Some(summary))
    }

    fn list_specs(&self, filter: &SpecFilter) -> Result<Vec<SpecSummary>> {
        let mut names: Vec<String> = self.distinct_specs()?;
        // Also surface specs that exist on disk but have no events yet — a
        // teammate who pulled the repo or a draft wave-plan never approved
        // would otherwise stay invisible. Wave 1 of the flatten-spec spec
        // gave us the header fallback in `spec_view`; this is the listing
        // side of the same fix.
        let spec_root = self.project_dir.join(".claude").join("spec");
        if let Ok(entries) = std::fs::read_dir(&spec_root) {
            let seen: std::collections::HashSet<&str> = names.iter().map(String::as_str).collect();
            let mut extras: Vec<String> = Vec::new();
            for entry in entries.flatten() {
                if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                    continue;
                }
                let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                if seen.contains(name.as_str()) {
                    continue;
                }
                let base = entry.path();
                if base.join("spec.md").exists() || base.join("wave-plan.md").exists() {
                    extras.push(name);
                }
            }
            names.extend(extras);
        }
        let needle = filter
            .search
            .as_deref()
            .map(str::to_lowercase)
            .filter(|s| !s.is_empty());

        // Hoist the link-event replay out of the per-spec loop: one full scan
        // builds a parent→child-set map, mirroring the dedupe semantics of
        // `link_payloads_for`. Avoids O(N) replays for N specs.
        let events = self.store()?.replay()?;
        let mut counts: std::collections::HashMap<String, std::collections::HashSet<String>> =
            std::collections::HashMap::new();
        for ev in &events {
            if ev.event != "spec.link" {
                continue;
            }
            let Some(parent) = ev.payload.get("parent").and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            let Some(child) = ev.payload.get("child").and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            counts
                .entry(parent.to_string())
                .or_default()
                .insert(child.to_string());
        }

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
            let mut summary: SpecSummary = (&view).into();
            summary.children_count = counts
                .get(&name)
                .map_or(0, |set| u32::try_from(set.len()).unwrap_or(u32::MAX));
            // Filter by status bucket if requested.
            let keep = match filter.status.as_ref().unwrap_or(&SpecStatusFilter::Any) {
                SpecStatusFilter::Any => true,
                SpecStatusFilter::Active => summary.state.is_active(),
                SpecStatusFilter::Closed => summary.state.is_terminal(),
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
        let from_events = project_waves(spec, &events);
        if !from_events.is_empty() {
            return Ok(from_events);
        }
        // No events for this spec yet — surface the planned wave structure
        // by scanning `wave-N-{role}/` subdirectories under the spec dir.
        // Mirrors the wave-1 filesystem-fallback philosophy for waves
        // (a draft wave plan a teammate pulled in stays visible).
        Ok(self.waves_from_disk(spec))
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

    #[allow(deprecated)] // populates the derived legacy `status` field on SpecChild.
    fn children_of(&self, parent: &str) -> Result<Vec<SpecChild>> {
        if parent.is_empty() {
            return Err(crate::reader::error::ReadError::invalid(
                "parent spec name cannot be empty",
            ));
        }
        let links = self.link_payloads_for(parent)?;
        let mut children: Vec<SpecChild> = Vec::with_capacity(links.len());
        for (child, reason) in links {
            // Look up the child's own state; `spec_summary_core` returns the
            // base summary without re-entering `children_of`. The legacy
            // `status` field is derived from `state` for back-compat.
            #[allow(deprecated)]
            let (state, status, started_at, completed_at) = match self.spec_summary_core(&child)? {
                Some(sum) => (
                    sum.state.clone(),
                    sum.status,
                    sum.started_at.clone(),
                    if sum.state.is_terminal() {
                        sum.last_event_at.clone()
                    } else {
                        None
                    },
                ),
                None => (
                    SpecState {
                        stage: Stage::Plan,
                        outcome: Outcome::Active,
                        flags: crate::model::view::Flags::default(),
                    },
                    SpecStatus::NoEvents,
                    None,
                    None,
                ),
            };
            children.push(SpecChild {
                spec: child,
                state,
                status,
                started_at,
                completed_at,
                reason,
            });
        }
        Ok(children)
    }
}

/// Wall-clock `now` in epoch milliseconds. Used only by `workspace_summary`;
/// projections themselves are pure.
fn now_epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

#[cfg(test)]
#[allow(deprecated)] // tests assert against the legacy SpecStatus path intentionally.
mod tests {
    use super::*;
    #[allow(deprecated)]
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
