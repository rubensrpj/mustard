//! Contract test — both [`SqliteSpecReader`] and [`InMemorySpecReader`]
//! must produce the same [`SpecView`] / [`QualityRollup`] / [`Vec<WaveView>`]
//! for the same event stream.
//!
//! This is the [Liskov] guarantee made operational: any future `SpecReader`
//! implementation must pass the same test, so consumers can swap one for
//! another without code changes.
//!
//! [Liskov]: https://en.wikipedia.org/wiki/Liskov_substitution_principle

use mustard_core::store::event_store::EventSink;
use mustard_core::store::sqlite_store::SqliteEventStore;
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::{
    AcStatus, InMemorySpecReader, Phase, SegmentState, SpecFilter, SpecReader, SpecStatus,
    SqliteSpecReader, TimeWindow, WaveStatus,
};
use serde_json::json;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

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

/// One representative event stream that exercises every projection branch.
fn fixture_stream() -> Vec<HarnessEvent> {
    vec![
        event(
            "auth",
            "2026-05-20T10:00:00Z",
            "pipeline.scope",
            json!({ "scope": "full", "lang": "pt", "model": "opus", "total_waves": 2, "is_wave_plan": true }),
        ),
        event(
            "auth",
            "2026-05-20T10:00:01Z",
            "pipeline.phase",
            json!({ "to": "execute" }),
        ),
        event(
            "auth",
            "2026-05-20T10:00:02Z",
            "pipeline.task.dispatch",
            json!({ "wave": 1, "name": "core", "agent": "general-purpose", "role": "impl" }),
        ),
        event(
            "auth",
            "2026-05-20T10:01:00Z",
            "pipeline.task.complete",
            json!({ "wave": 1, "name": "core", "files_modified": ["src/a.rs", "src/b.rs"] }),
        ),
        event(
            "auth",
            "2026-05-20T10:01:01Z",
            "pipeline.wave.complete",
            json!({ "wave": 1 }),
        ),
        event(
            "auth",
            "2026-05-20T10:01:30Z",
            "tool.use",
            json!({ "file_path": "src/a.rs" }),
        ),
        event(
            "auth",
            "2026-05-20T10:01:31Z",
            "agent.start",
            json!({}),
        ),
        event(
            "auth",
            "2026-05-20T10:02:00Z",
            "qa.result",
            json!({
                "criteria": [
                    { "id": "AC-1", "status": "pass" },
                    { "id": "AC-2", "status": "pass" },
                    { "id": "AC-3", "status": "fail" },
                ]
            }),
        ),
    ]
}

// ---------------------------------------------------------------------------
// Builders for the two readers, primed with the fixture stream.
// ---------------------------------------------------------------------------

fn build_sqlite_reader() -> (SqliteSpecReader, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteEventStore::for_project(dir.path()).unwrap();
    for ev in fixture_stream() {
        store.append(&ev).unwrap();
    }
    drop(store);
    let reader = SqliteSpecReader::for_project(dir.path()).unwrap();
    // Keep the TempDir alive for the lifetime of the reader so the DB file
    // does not get cleaned up mid-test.
    (reader, dir)
}

fn build_memory_reader() -> InMemorySpecReader {
    InMemorySpecReader::with_events(fixture_stream())
}

// ---------------------------------------------------------------------------
// Contract assertions — each one runs against BOTH readers via a closure.
// ---------------------------------------------------------------------------

fn assert_spec_view_matches_fixture<R: SpecReader>(reader: &R) {
    let view = reader.spec_view("auth").unwrap().expect("spec view present");
    assert_eq!(view.spec, "auth");
    assert_eq!(view.status, SpecStatus::Planning);
    assert_eq!(view.phase, Some(Phase::Execute));
    assert_eq!(view.lang.as_deref(), Some("pt"));
    assert_eq!(view.model.as_deref(), Some("opus"));
    assert_eq!(view.total_waves, Some(2));
    assert!(view.is_wave_plan);
    assert_eq!(view.completed_waves, vec![1]);
    assert_eq!(view.current_wave, Some(2));
    assert_eq!(view.tools_used, 1);
    assert_eq!(view.agents_dispatched, 1);
    assert_eq!(view.files_touched, 2);
    assert_eq!(view.ac_total, 3);
    assert_eq!(view.ac_passed, 2);
    assert_eq!(view.ac_failed, 1);
}

fn assert_quality_matches_fixture<R: SpecReader>(reader: &R) {
    let q = reader.quality("auth").unwrap();
    assert_eq!(q.total, 3);
    assert_eq!(q.passed, 2);
    assert_eq!(q.failed, 1);
    let ids: Vec<_> = q.criteria.iter().map(|c| c.id.clone()).collect();
    assert_eq!(ids, vec!["AC-1", "AC-2", "AC-3"]);
    assert_eq!(q.criteria[2].status, AcStatus::Fail);
}

fn assert_waves_match_fixture<R: SpecReader>(reader: &R) {
    let waves = reader.waves("auth").unwrap();
    assert_eq!(waves.len(), 1);
    let w = &waves[0];
    assert_eq!(w.wave, 1);
    assert_eq!(w.status, WaveStatus::Completed);
    assert_eq!(w.role.as_deref(), Some("impl"));
    assert_eq!(w.agent_type.as_deref(), Some("general-purpose"));
    assert_eq!(w.files_changed, vec!["src/a.rs".to_string(), "src/b.rs".to_string()]);
}

fn assert_timeline_matches_fixture<R: SpecReader>(reader: &R) {
    let timeline = reader.timeline("auth", TimeWindow::All).unwrap();
    // Eight events in the fixture.
    assert_eq!(timeline.len(), 8);
    // First event is the scope, last is the qa.result.
    assert_eq!(timeline.first().unwrap().raw_event, "pipeline.scope");
    assert_eq!(timeline.last().unwrap().raw_event, "qa.result");
}

fn assert_list_specs_returns_auth<R: SpecReader>(reader: &R) {
    let list = reader.list_specs(&SpecFilter::default()).unwrap();
    assert!(list.iter().any(|s| s.spec == "auth"));
}

fn assert_workspace_summary_sees_auth<R: SpecReader>(reader: &R) {
    let summary = reader.workspace_summary().unwrap();
    assert!(summary.spec_tracks.iter().any(|t| t.spec == "auth"));
    let track = summary
        .spec_tracks
        .iter()
        .find(|t| t.spec == "auth")
        .unwrap();
    // Segments: Analyze + Plan completed, Execute active, Qa + Close future.
    assert_eq!(track.segments.len(), 5);
    let exec = track
        .segments
        .iter()
        .find(|s| s.phase == Phase::Execute)
        .unwrap();
    assert_eq!(exec.state, SegmentState::Active);
}

// ---------------------------------------------------------------------------
// Tests — each pair runs the same assertion against both readers.
// ---------------------------------------------------------------------------

#[test]
fn spec_view_contract_sqlite() {
    let (reader, _dir) = build_sqlite_reader();
    assert_spec_view_matches_fixture(&reader);
}

#[test]
fn spec_view_contract_memory() {
    let reader = build_memory_reader();
    assert_spec_view_matches_fixture(&reader);
}

#[test]
fn quality_contract_sqlite() {
    let (reader, _dir) = build_sqlite_reader();
    assert_quality_matches_fixture(&reader);
}

#[test]
fn quality_contract_memory() {
    let reader = build_memory_reader();
    assert_quality_matches_fixture(&reader);
}

#[test]
fn waves_contract_sqlite() {
    let (reader, _dir) = build_sqlite_reader();
    assert_waves_match_fixture(&reader);
}

#[test]
fn waves_contract_memory() {
    let reader = build_memory_reader();
    assert_waves_match_fixture(&reader);
}

#[test]
fn timeline_contract_sqlite() {
    let (reader, _dir) = build_sqlite_reader();
    assert_timeline_matches_fixture(&reader);
}

#[test]
fn timeline_contract_memory() {
    let reader = build_memory_reader();
    assert_timeline_matches_fixture(&reader);
}

#[test]
fn list_specs_contract_sqlite() {
    let (reader, _dir) = build_sqlite_reader();
    assert_list_specs_returns_auth(&reader);
}

#[test]
fn list_specs_contract_memory() {
    let reader = build_memory_reader();
    assert_list_specs_returns_auth(&reader);
}

#[test]
fn workspace_summary_contract_sqlite() {
    let (reader, _dir) = build_sqlite_reader();
    assert_workspace_summary_sees_auth(&reader);
}

#[test]
fn workspace_summary_contract_memory() {
    let reader = build_memory_reader();
    // The fixture's events are dated 2026-05-20; pin "now" to that day so the
    // segment state is computed against the right calendar window.
    reader.set_now_ms(1_779_609_600_000); // 2026-05-20T12:00:00Z
    assert_workspace_summary_sees_auth(&reader);
}

// ---------------------------------------------------------------------------
// Invariants — properties that must hold for any event stream.
// ---------------------------------------------------------------------------

#[test]
fn empty_event_stream_yields_consistent_empties_across_readers() {
    let (sqlite_reader, _dir) = {
        let dir = tempfile::tempdir().unwrap();
        let _store = SqliteEventStore::for_project(dir.path()).unwrap();
        let reader = SqliteSpecReader::for_project(dir.path()).unwrap();
        (reader, dir)
    };
    let memory_reader = InMemorySpecReader::new();

    for reader in [
        &sqlite_reader as &dyn SpecReader,
        &memory_reader as &dyn SpecReader,
    ] {
        assert!(reader.spec_view("nothing").unwrap().is_none());
        assert!(reader.waves("nothing").unwrap().is_empty());
        assert_eq!(reader.quality("nothing").unwrap().total, 0);
        assert!(reader.timeline("nothing", TimeWindow::All).unwrap().is_empty());
        assert!(reader.list_specs(&SpecFilter::default()).unwrap().is_empty());
    }
}

#[test]
fn empty_spec_name_is_invalid_across_readers() {
    let (sqlite_reader, _dir) = {
        let dir = tempfile::tempdir().unwrap();
        let _store = SqliteEventStore::for_project(dir.path()).unwrap();
        let reader = SqliteSpecReader::for_project(dir.path()).unwrap();
        (reader, dir)
    };
    let memory_reader = InMemorySpecReader::new();

    for reader in [
        &sqlite_reader as &dyn SpecReader,
        &memory_reader as &dyn SpecReader,
    ] {
        assert!(reader.spec_view("").is_err());
        assert!(reader.waves("").is_err());
        assert!(reader.quality("").is_err());
        assert!(reader.timeline("", TimeWindow::All).is_err());
    }
}
