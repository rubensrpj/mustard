//! [`InMemorySpecReader`] — test-only [`SpecReader`] backed by a `Vec<HarnessEvent>`.
//!
//! Same contract as [`SqliteSpecReader`] — the contract test in
//! `tests/reader_contract.rs` exercises both behind a single set of
//! assertions. Useful when a consumer wants to verify a projection without
//! standing up a `SQLite` database.

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
use crate::model::event::HarnessEvent;
use std::path::PathBuf;
use std::sync::RwLock;

/// Test double for [`SpecReader`]. Holds the event vector behind an
/// [`RwLock`] so [`Self::push`] mutations work through `&self`, matching the
/// production reader's borrowing shape.
#[derive(Default)]
pub struct InMemorySpecReader {
    events: RwLock<Vec<HarnessEvent>>,
    /// Optional override for "now" so workspace-summary tests are
    /// deterministic. `None` falls back to `SystemTime::now()` — same as the
    /// `SQLite` reader.
    now_ms_override: RwLock<Option<i64>>,
    /// Optional root directory under which `{root}/{spec}/spec.md` lives.
    ///
    /// Mirrors [`super::sqlite::SqliteSpecReader`]'s `project_dir` +
    /// `.claude/spec/` resolution: when this is set and the event log for a
    /// spec is empty, the projection falls back to parsing the on-disk
    /// `spec.md` header. `None` disables the fallback — most tests do not
    /// need it.
    spec_md_root: RwLock<Option<PathBuf>>,
}

impl InMemorySpecReader {
    /// Construct an empty reader.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct a reader pre-loaded with `events`.
    #[must_use]
    pub fn with_events(events: Vec<HarnessEvent>) -> Self {
        Self {
            events: RwLock::new(events),
            now_ms_override: RwLock::new(None),
            spec_md_root: RwLock::new(None),
        }
    }

    /// Pin a root directory under which `{root}/{spec}/spec.md` lives, so the
    /// header fallback (Wave 1) can be exercised in tests without standing up
    /// a `SQLite` store. `None` (default) keeps the fallback off.
    ///
    /// # Panics
    /// Only on lock poisoning.
    pub fn set_spec_md_root(&self, root: impl Into<PathBuf>) {
        *self
            .spec_md_root
            .write()
            .expect("spec-md-root lock poisoned") = Some(root.into());
    }

    /// Push a single event. Useful for incremental test builds.
    ///
    /// # Panics
    /// Only on lock poisoning, which would mean a prior call panicked
    /// holding the write lock — never happens in tests written for this crate.
    pub fn push(&self, ev: HarnessEvent) {
        self.events.write().expect("events lock poisoned").push(ev);
    }

    /// Pin "now" for `workspace_summary` calls. Tests use this to get
    /// stable `events_per_minute` and `tokens_saved_today` values.
    ///
    /// # Panics
    /// Only on lock poisoning.
    pub fn set_now_ms(&self, now_ms: i64) {
        *self
            .now_ms_override
            .write()
            .expect("now-ms lock poisoned") = Some(now_ms);
    }

    fn snapshot(&self) -> Vec<HarnessEvent> {
        self.events.read().expect("events lock poisoned").clone()
    }

    fn now_ms(&self) -> i64 {
        self.now_ms_override
            .read()
            .expect("now-ms lock poisoned")
            .unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
            })
    }

    /// Build a [`SpecSummary`] without populating `children_count`.
    ///
    /// Mirrors [`super::sqlite::SqliteSpecReader`]: the recursion-safe entry
    /// point used by `children_of` to resolve each child's status without
    /// re-entering itself.
    fn spec_summary_core(&self, spec: &str) -> Result<Option<SpecSummary>> {
        Ok(self.spec_view(spec)?.as_ref().map(SpecSummary::from))
    }

    /// Fold all in-memory `spec.link` events into the children of `parent`.
    ///
    /// First-seen reason wins per child.
    fn link_payloads_for(&self, parent: &str) -> Vec<(String, Option<String>)> {
        let events = self.snapshot();
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
        order
            .into_iter()
            .map(|name| {
                let reason = seen.remove(&name).unwrap_or(None);
                (name, reason)
            })
            .collect()
    }
}

impl SpecReader for InMemorySpecReader {
    #[allow(deprecated)] // `NoEvents` is the empty-stream sentinel — only the legacy enum carries it.
    fn spec_view(&self, spec: &str) -> Result<Option<SpecView>> {
        if spec.is_empty() {
            return Err(crate::reader::error::ReadError::invalid("spec name cannot be empty"));
        }
        let events = self.snapshot();
        let scoped: Vec<_> = events
            .iter()
            .filter(|e| e.spec.as_deref() == Some(spec))
            .cloned()
            .collect();
        if !scoped.is_empty() {
            return Ok(Some(project_spec_view(spec, &scoped)));
        }
        // Mirror SqliteSpecReader: when the event log is empty, try the
        // on-disk spec.md header. Only fires when the test has pinned a root
        // via `set_spec_md_root` — otherwise the reader stays purely
        // in-memory and returns `Ok(None)` as before.
        let maybe_path = self
            .spec_md_root
            .read()
            .expect("spec-md-root lock poisoned")
            .as_ref()
            .map(|root| root.join(spec).join("spec.md"));
        if let Some(path) = maybe_path {
            let view = project_spec_view_with_header(spec, &scoped, Some(path.as_path()), None);
            if view.status != SpecStatus::NoEvents
                || view.phase.is_some()
                || view.scope.is_some()
            {
                return Ok(Some(view));
            }
        }
        Ok(None)
    }

    fn spec_summary(&self, spec: &str) -> Result<Option<SpecSummary>> {
        let Some(mut summary) = self.spec_summary_core(spec)? else {
            return Ok(None);
        };
        summary.children_count = u32::try_from(self.children_of(spec)?.len()).unwrap_or(u32::MAX);
        Ok(Some(summary))
    }

    fn list_specs(&self, filter: &SpecFilter) -> Result<Vec<SpecSummary>> {
        let events = self.snapshot();
        let needle = filter
            .search
            .as_deref()
            .map(str::to_lowercase)
            .filter(|s| !s.is_empty());

        // Discover distinct non-orphan specs in the in-memory log and, in the
        // same pass, fold `spec.link` events into a parent→child-set map.
        // Mirrors `link_payloads_for`'s dedupe; one snapshot, not N replays.
        let mut names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut counts: std::collections::HashMap<String, std::collections::HashSet<String>> =
            std::collections::HashMap::new();
        for ev in &events {
            if let Some(spec) = &ev.spec {
                if spec != "__orphan__" {
                    names.insert(spec.clone());
                }
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
            let Some(view) = self.spec_view(&name)? else {
                continue;
            };
            let mut summary: SpecSummary = (&view).into();
            summary.children_count = counts
                .get(&name)
                .map_or(0, |set| u32::try_from(set.len()).unwrap_or(u32::MAX));
            let keep = match filter.status.as_ref().unwrap_or(&SpecStatusFilter::Any) {
                SpecStatusFilter::Any => true,
                SpecStatusFilter::Active => summary.state.is_active(),
                SpecStatusFilter::Closed => summary.state.is_terminal(),
            };
            if keep {
                summaries.push(summary);
            }
        }
        summaries.sort_by(|a, b| b.last_event_at.cmp(&a.last_event_at).then(a.spec.cmp(&b.spec)));
        Ok(summaries)
    }

    fn waves(&self, spec: &str) -> Result<Vec<WaveView>> {
        if spec.is_empty() {
            return Err(crate::reader::error::ReadError::invalid("spec name cannot be empty"));
        }
        let events = self.snapshot();
        Ok(project_waves(spec, &events))
    }

    fn quality(&self, spec: &str) -> Result<QualityRollup> {
        if spec.is_empty() {
            return Err(crate::reader::error::ReadError::invalid("spec name cannot be empty"));
        }
        let events = self.snapshot();
        Ok(project_quality(spec, &events))
    }

    fn timeline(&self, spec: &str, window: TimeWindow) -> Result<Vec<TimelineNode>> {
        if spec.is_empty() {
            return Err(crate::reader::error::ReadError::invalid("spec name cannot be empty"));
        }
        let events = self.snapshot();
        Ok(project_timeline(spec, &events, window))
    }

    fn workspace_summary(&self) -> Result<WorkspaceSummary> {
        let events = self.snapshot();
        Ok(project_workspace(&events, self.now_ms()))
    }

    #[allow(deprecated)] // populates the derived legacy `status` field on SpecChild.
    fn children_of(&self, parent: &str) -> Result<Vec<SpecChild>> {
        if parent.is_empty() {
            return Err(crate::reader::error::ReadError::invalid(
                "parent spec name cannot be empty",
            ));
        }
        let links = self.link_payloads_for(parent);
        let mut children: Vec<SpecChild> = Vec::with_capacity(links.len());
        for (child, reason) in links {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::event::{Actor, ActorKind, SCHEMA_VERSION};
    use crate::projection::parse_iso_millis;
    use serde_json::json;

    fn ev(spec: &str, ts: &str, kind: &str) -> HarnessEvent {
        HarnessEvent {
            v: SCHEMA_VERSION,
            ts: ts.into(),
            session_id: "s1".into(),
            wave: 0,
            actor: Actor { kind: ActorKind::Hook, id: None, actor_type: None },
            event: kind.into(),
            payload: json!({}),
            spec: Some(spec.into()),
        }
    }

    #[test]
    fn empty_reader_yields_none_for_known_methods() {
        let reader = InMemorySpecReader::new();
        assert!(reader.spec_view("auth").unwrap().is_none());
        assert!(reader.waves("auth").unwrap().is_empty());
        assert_eq!(reader.quality("auth").unwrap().total, 0);
    }

    #[test]
    fn push_mutates_in_place_through_immutable_reference() {
        let reader = InMemorySpecReader::new();
        reader.push(ev("auth", "2026-05-20T10:00:00Z", "tool.use"));
        let view = reader.spec_view("auth").unwrap().unwrap();
        assert_eq!(view.tools_used, 1);
    }

    #[test]
    fn set_now_ms_pins_workspace_summary_window() {
        let reader = InMemorySpecReader::new();
        // Event at 2026-05-20T11:59:30Z = 30 seconds before our pinned NOW.
        reader.push(ev("auth", "2026-05-20T11:59:30Z", "tool.use"));
        let now_ms = parse_iso_millis("2026-05-20T12:00:00Z").expect("hard-coded ISO parses");
        reader.set_now_ms(now_ms);
        let summary = reader.workspace_summary().unwrap();
        assert!((summary.events_per_minute - 1.0).abs() < f64::EPSILON);
    }
}
