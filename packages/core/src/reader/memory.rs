//! [`InMemorySpecReader`] — test-only [`SpecReader`] backed by a `Vec<HarnessEvent>`.
//!
//! Same contract as [`SqliteSpecReader`] — the contract test in
//! `tests/reader_contract.rs` exercises both behind a single set of
//! assertions. Useful when a consumer wants to verify a projection without
//! standing up a SQLite database.

use crate::reader::error::Result;
use crate::model::view::{
    QualityRollup, SpecFilter, SpecStatusFilter, SpecSummary, SpecView, TimeWindow, TimelineNode,
    WaveView, WorkspaceSummary,
};
use crate::projection::{
    project_quality, project_spec_view, project_timeline, project_waves, project_workspace,
};
use crate::reader::SpecReader;
use crate::model::event::HarnessEvent;
use std::sync::RwLock;

/// Test double for [`SpecReader`]. Holds the event vector behind an
/// [`RwLock`] so [`Self::push`] mutations work through `&self`, matching the
/// production reader's borrowing shape.
#[derive(Default)]
pub struct InMemorySpecReader {
    events: RwLock<Vec<HarnessEvent>>,
    /// Optional override for "now" so workspace-summary tests are
    /// deterministic. `None` falls back to `SystemTime::now()` — same as the
    /// SQLite reader.
    now_ms_override: RwLock<Option<i64>>,
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
        }
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
                    .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
                    .unwrap_or(0)
            })
    }
}

impl SpecReader for InMemorySpecReader {
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
        if scoped.is_empty() {
            return Ok(None);
        }
        Ok(Some(project_spec_view(spec, &scoped)))
    }

    fn spec_summary(&self, spec: &str) -> Result<Option<SpecSummary>> {
        Ok(self.spec_view(spec)?.as_ref().map(SpecSummary::from))
    }

    fn list_specs(&self, filter: &SpecFilter) -> Result<Vec<SpecSummary>> {
        let events = self.snapshot();
        let needle = filter
            .search
            .as_deref()
            .map(str::to_lowercase)
            .filter(|s| !s.is_empty());

        // Discover distinct non-orphan specs in the in-memory log.
        let mut names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for ev in &events {
            if let Some(spec) = &ev.spec {
                if spec != "__orphan__" {
                    names.insert(spec.clone());
                }
            }
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
            let summary: SpecSummary = (&view).into();
            let keep = match filter.status.as_ref().unwrap_or(&SpecStatusFilter::Any) {
                SpecStatusFilter::Any => true,
                SpecStatusFilter::Active => summary.status.is_active(),
                SpecStatusFilter::Closed => summary.status.is_terminal(),
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
