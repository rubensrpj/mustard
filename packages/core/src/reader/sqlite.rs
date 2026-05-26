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
    project_waves, project_workspace, read_harness_events_from_ndjson_dir,
};
use crate::reader::SpecReader;
use crate::store::sqlite_store::SqliteEventStore;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// Production [`SpecReader`] backed by the harness `mustard.db`.
///
/// Opens the store **once** and reuses it for every query rather than
/// reopening per call. The store owns a `rusqlite::Connection`, which is
/// `Send` but not `Sync`; wrapping it in an `Arc<Mutex<…>>` keeps the reader
/// `Clone + Send + Sync` (the [`SpecReader`] trait is `Send + Sync` so Tauri
/// command handlers can share it). The `Mutex` only serializes access to the
/// single cached connection; `SQLite`'s WAL still lets a separate writer process
/// proceed concurrently.
///
/// Reusing the store avoids paying [`SqliteEventStore::new`]'s open cost on
/// every method — even with the `user_version` fast-path the open is not free
/// (file open, pragma round-trips). `project_dir` is retained for the
/// filesystem fallbacks (`spec.md` header reads, planned-wave scans).
#[derive(Clone, Debug)]
pub struct SqliteSpecReader {
    project_dir: PathBuf,
    store: Arc<Mutex<SqliteEventStore>>,
}

impl SqliteSpecReader {
    /// Build a reader for `project_dir`'s harness DB.
    ///
    /// Resolves the database path through [`SqliteEventStore::for_project`],
    /// which honours the `MUSTARD_DB_PATH` env var when set, and opens the
    /// store once up front.
    ///
    /// # Errors
    ///
    /// Returns [`ReadError`](crate::reader::error::ReadError) if the DB cannot be
    /// opened.
    pub fn for_project(project_dir: impl AsRef<Path>) -> Result<Self> {
        let store = SqliteEventStore::for_project(project_dir.as_ref())?;
        Ok(Self {
            project_dir: project_dir.as_ref().to_path_buf(),
            store: Arc::new(Mutex::new(store)),
        })
    }

    /// Lock the shared store. A poisoned lock is recovered — the guarded data
    /// is a connection handle, and a panic in a prior query cannot corrupt it
    /// (a genuine corruption would surface as a query error on the next use).
    fn store(&self) -> MutexGuard<'_, SqliteEventStore> {
        self.store.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Return all distinct spec names known to the store, excluding the
    /// `__orphan__` sentinel.
    fn distinct_specs(&self) -> Result<Vec<String>> {
        let mut specs = self.store().distinct_specs()?;
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
    /// Reads via [`SqliteEventStore::replay`] for any lifecycle-event-shaped
    /// link, plus the parent's per-spec NDJSON directory (W5 — `spec.link`
    /// events live there alongside tool/agent records).
    fn link_payloads_for(&self, parent: &str) -> Result<Vec<(String, Option<String>)>> {
        let mut events = self.store().replay()?;
        let mut ndjson = read_harness_events_from_ndjson_dir(&self.ndjson_events_dir(parent));
        events.append(&mut ndjson);
        events.sort_by(|a, b| a.ts.cmp(&b.ts));
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
    /// fails open (`crate::fs::read_to_string` → `None`) when the file is missing.
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

    /// On-disk events directory the NDJSON writer uses for `spec`.
    ///
    /// Path: `{project_dir}/.claude/spec/{spec}/.events/`. The directory is
    /// optional — a brand-new spec has no tool/agent events yet and the
    /// folder simply does not exist; [`read_harness_events_from_ndjson_dir`]
    /// returns an empty `Vec` in that case.
    fn ndjson_events_dir(&self, spec: &str) -> std::path::PathBuf {
        self.project_dir
            .join(".claude")
            .join("spec")
            .join(spec)
            .join(".events")
    }

    /// Return the merged event slice for `spec`: lifecycle events from the
    /// `pipeline_events` SQLite index plus tool / agent / qa events from the
    /// per-spec NDJSON directory, sorted by `ts`.
    ///
    /// This is the W5 successor to "query the events table" — every projection
    /// in this reader feeds off it so the slice the SQLite reader hands to the
    /// pure folds matches what the in-memory reader sees.
    fn merged_events_for(&self, spec: &str) -> Result<Vec<crate::model::event::HarnessEvent>> {
        let mut events = self.store().query(Some(spec))?;
        let mut ndjson = read_harness_events_from_ndjson_dir(&self.ndjson_events_dir(spec));
        events.append(&mut ndjson);
        events.sort_by(|a, b| a.ts.cmp(&b.ts));
        Ok(events)
    }

    /// Scan `.claude/spec/{spec}/wave-N-{role}/` to build a planned wave
    /// list when no task events exist yet. Returns waves sorted by number.
    fn waves_from_disk(&self, spec: &str) -> Vec<WaveView> {
        let base = self.project_dir.join(".claude").join("spec").join(spec);
        let Ok(entries) = crate::fs::read_dir(&base) else {
            return Vec::new();
        };
        let mut planned: Vec<WaveView> = Vec::new();
        for entry in entries {
            if !entry.is_dir {
                continue;
            }
            let name = entry.file_name;
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
        let events = self.merged_events_for(spec)?;
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
        if let Ok(entries) = crate::fs::read_dir(&spec_root) {
            let seen: std::collections::HashSet<&str> = names.iter().map(String::as_str).collect();
            let mut extras: Vec<String> = Vec::new();
            for entry in entries {
                if !entry.is_dir {
                    continue;
                }
                if seen.contains(entry.file_name.as_str()) {
                    continue;
                }
                let base = entry.path;
                if base.join("spec.md").exists() || base.join("wave-plan.md").exists() {
                    extras.push(entry.file_name);
                }
            }
            names.extend(extras);
        }
        let needle = filter
            .search
            .as_deref()
            .map(str::to_lowercase)
            .filter(|s| !s.is_empty());

        // ONE full replay feeds the whole listing — no per-spec store open or
        // `query()` round-trip (the old N+1). In a single pass we both (a)
        // group every event under its spec so each spec is projected from its
        // in-memory slice, and (b) build the parent→child-set map for the
        // sub-spec count, mirroring the dedupe semantics of `link_payloads_for`.
        let events = self.store().replay()?;
        let mut by_spec: std::collections::HashMap<&str, Vec<&crate::model::event::HarnessEvent>> =
            std::collections::HashMap::new();
        let mut counts: std::collections::HashMap<String, std::collections::HashSet<String>> =
            std::collections::HashMap::new();
        for ev in &events {
            if let Some(spec) = ev.spec.as_deref() {
                by_spec.entry(spec).or_default().push(ev);
            }
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
            // Project from the in-memory event slice when the spec has events;
            // otherwise fall back to the on-disk `spec.md` header (a teammate's
            // pulled draft with no events yet). The SQLite slice carries only
            // `pipeline.*` lifecycle events (W5); merge in the per-spec NDJSON
            // (tool/agent/qa events) so the projection sees the full timeline
            // — mirrors what `merged_events_for` does on the single-spec path.
            let summary_opt: Option<SpecSummary> = match by_spec.get(name.as_str()) {
                Some(slice) => {
                    let mut owned: Vec<crate::model::event::HarnessEvent> =
                        slice.iter().map(|e| (*e).clone()).collect();
                    let mut ndjson =
                        read_harness_events_from_ndjson_dir(&self.ndjson_events_dir(&name));
                    owned.append(&mut ndjson);
                    owned.sort_by(|a, b| a.ts.cmp(&b.ts));
                    Some((&project_spec_view(&name, &owned)).into())
                }
                None => self.spec_view(&name)?.as_ref().map(SpecSummary::from),
            };
            let Some(mut summary) = summary_opt else {
                continue;
            };
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
        let events = self.merged_events_for(spec)?;
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
        let events = self.merged_events_for(spec)?;
        Ok(project_quality(spec, &events))
    }

    fn timeline(&self, spec: &str, window: TimeWindow) -> Result<Vec<TimelineNode>> {
        if spec.is_empty() {
            return Err(crate::reader::error::ReadError::invalid("spec name cannot be empty"));
        }
        let events = self.merged_events_for(spec)?;
        Ok(project_timeline(spec, &events, window))
    }

    fn workspace_summary(&self) -> Result<WorkspaceSummary> {
        // Bound the scan to a recent window instead of replaying the whole
        // append-only log. The workspace card surfaces current activity, so a
        // generous lookback keeps every live spec visible while letting the
        // `idx_events_ts` index prune ancient telemetry from the scan.
        let now_ms = now_epoch_ms();
        let since = iso_cutoff(now_ms, WORKSPACE_LOOKBACK_DAYS);
        let events = self.store().replay_since(since.as_deref())?;
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

/// How far back `workspace_summary` scans the event log. Generous on purpose —
/// the workspace card must keep every live spec visible; the window only exists
/// to let `idx_events_ts` prune long-dead telemetry from the scan.
const WORKSPACE_LOOKBACK_DAYS: i64 = 120;

/// Wall-clock `now` in epoch milliseconds. Used only by `workspace_summary`;
/// projections themselves are pure.
fn now_epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Build the inclusive lower-bound timestamp `now_ms - lookback_days`, as a
/// prefix ISO-8601 string `YYYY-MM-DDTHH:MM:SS` (no trailing `Z`).
///
/// The bound is deliberately *un*terminated: stored timestamps may carry
/// millisecond precision (`…SS.mmmZ`) or a trailing `Z`, and `Z`/`.` sort
/// *after* the digit positions, so a seconds-only prefix is a correct lexical
/// lower bound for either shape. Returns `None` when `now_ms` is non-positive
/// (clock unset) so the caller falls back to an unbounded replay rather than
/// silently dropping every event.
fn iso_cutoff(now_ms: i64, lookback_days: i64) -> Option<String> {
    if now_ms <= 0 {
        return None;
    }
    let cutoff_secs = now_ms / 1_000 - lookback_days * 86_400;
    if cutoff_secs <= 0 {
        return None;
    }
    let (y, mo, d, h, mi, s) = epoch_secs_to_ymdhms(cutoff_secs);
    Some(format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}"))
}

/// Howard Hinnant's days-from-civil algorithm (reverse) → `(y, mo, d, h, mi,
/// s)` in UTC. Mirrors `economy::sources::time::epoch_secs_to_ymdhms`; kept
/// local because that one is `pub(super)` to the economy module.
// cast_sign_loss: Howard Hinnant's algorithm guarantees calendar values are non-negative.
// many_single_char_names: single-char names are idiomatic for this well-known algorithm.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::many_single_char_names)]
fn epoch_secs_to_ymdhms(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    let h = (tod / 3600) as u32;
    let mi = ((tod % 3600) / 60) as u32;
    let s = (tod % 60) as u32;
    (y, m, d, h, mi, s)
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
        // W5: only `pipeline.*` events land in SQLite (`pipeline_events`); tool
        // events live in per-spec NDJSON files and are out of this reader's
        // scope. The view here is built solely from the lifecycle index, so
        // `tools_used` stays 0 — the NDJSON path is exercised by
        // `crate::projection::timeline` tests, not the SQLite reader's.
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
                "pipeline.phase",
                json!({ "phase": "PLAN" }),
            ))
            .unwrap();

        let reader = open_reader(dir.path());
        let view = reader.spec_view("auth").unwrap().unwrap();
        assert_eq!(view.spec, "auth");
        assert_eq!(view.status, SpecStatus::Planning);
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
        // W5: `distinct_specs` reads from `pipeline_events`, so seeding has to
        // go through a `pipeline.*` event (the only kind the SQLite sink writes).
        // Tool events live in NDJSON and are not part of the listing surface.
        let dir = tempdir().unwrap();
        let store = store_for(dir.path());
        for name in ["auth", "billing", "__orphan__"] {
            store
                .append(&event(name, "2026-05-20T10:00:00Z", "pipeline.scope", json!({})))
                .unwrap();
        }
        let reader = open_reader(dir.path());

        let any = reader.list_specs(&SpecFilter::default()).unwrap();
        let names: Vec<_> = any.iter().map(|s| s.spec.clone()).collect();
        assert!(names.contains(&"auth".to_string()));
        assert!(names.contains(&"billing".to_string()));
        assert!(!names.contains(&"__orphan__".to_string()));

        let filter = SpecFilter {
            search: Some("auth".into()),
            ..SpecFilter::default()
        };
        let only_auth = reader.list_specs(&filter).unwrap();
        assert_eq!(only_auth.len(), 1);
        assert_eq!(only_auth[0].spec, "auth");
    }
}
