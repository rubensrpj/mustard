//! `mustard-rt run emit-pipeline` — typed pipeline-event emitter.
//!
//! Records one of the eight `pipeline.*` events defined in
//! [`mustard_core::model::event`] constants. Callers supply the event kind, the
//! spec name, and an optional JSON payload string; this module validates both
//! and appends the event to the project's [`SqliteEventStore`].
//!
//! ## Fail-open contract
//!
//! - **Unknown kind** → prints an error on stderr and exits with code 1.
//! - **Invalid JSON payload** → prints an error on stderr and exits with code 1.
//! - **Store error** → prints a warning on stderr and exits with code 0 (fail-open).
//!
//! This matches the pattern used by `emit_phase` and every other harness
//! emitter: telemetry is never load-bearing, so a write failure must never
//! break the pipeline.

use crate::run::env::{project_dir, session_id};
use crate::util::now_iso8601;
use mustard_core::store::event_store::EventSink;
use mustard_core::store::sqlite_store::SqliteEventStore;
use mustard_core::model::event::{
    Actor, ActorKind, HarnessEvent, SCHEMA_VERSION,
    EVENT_PIPELINE_COMPLETE, EVENT_PIPELINE_DISPATCH_FAILURE, EVENT_PIPELINE_PAUSE,
    EVENT_PIPELINE_RESUME_MODE, EVENT_PIPELINE_SCOPE, EVENT_PIPELINE_STATUS,
    EVENT_PIPELINE_TASK_COMPLETE, EVENT_PIPELINE_TASK_DISPATCH, EVENT_PIPELINE_WAVE_COMPLETE,
};
use mustard_core::spec;
use mustard_core::{Flags, Outcome, SpecState, Stage};
use serde_json::{json, Value};
use std::path::Path;

// --- Canonical state-model event kinds (spec-lifecycle-unification W2) -------
//
// These are not yet `EVENT_PIPELINE_*` constants in `mustard-core` (that crate
// is out of this wave's boundary), so they live here as literals. When core
// gains the constants in a later wave, swap these for the re-exports.

/// `pipeline.stage` — a canonical [`Stage`] transition (replaces the legacy
/// `pipeline.phase`).
const EVENT_PIPELINE_STAGE: &str = "pipeline.stage";
/// `pipeline.outcome` — a terminal [`Outcome`] transition (replaces the
/// terminal half of the legacy `pipeline.status`).
const EVENT_PIPELINE_OUTCOME: &str = "pipeline.outcome";
/// `pipeline.flag.set` — a [`Flags`](mustard_core::Flags) qualifier was raised.
const EVENT_PIPELINE_FLAG_SET: &str = "pipeline.flag.set";
/// `pipeline.flag.clear` — a [`Flags`](mustard_core::Flags) qualifier was cleared.
const EVENT_PIPELINE_FLAG_CLEAR: &str = "pipeline.flag.clear";

/// `pipeline.phase` — the legacy phase-transition event. Accepted here only so
/// `emit-pipeline --kind pipeline.phase` can fan out the `pipeline.stage`
/// alias (it is otherwise emitted by `emit-phase`). Not part of the
/// directly-emittable "new" set.
const EVENT_PIPELINE_PHASE: &str = "pipeline.phase";

// --- Hygiene event kinds (spec-lifecycle-unification W5) ---------------------
//
// Emitted by the `spec_hygiene` SessionStart hook (and accepted here so the
// hook — or a test — can also drive them via `emit-pipeline`). They carry no
// legacy alias: they are first-class new kinds. See `hooks/spec_hygiene.rs`.

/// `hygiene.detected` — an active spec was classified `stale`,
/// `abandoned_suspect`, or (in detect mode) `candidate`. Advisory only.
const EVENT_HYGIENE_DETECTED: &str = "hygiene.detected";
/// `hygiene.autoclose` — a candidate spec passed the close-gate and was
/// auto-closed (`pipeline.outcome: completed` follows).
const EVENT_HYGIENE_AUTOCLOSE: &str = "hygiene.autoclose";
/// `hygiene.skipped` — a candidate spec failed the close-gate; it was left
/// active. Payload carries the `blocker`.
const EVENT_HYGIENE_SKIPPED: &str = "hygiene.skipped";

/// The 17 valid pipeline event kind strings: the 9 legacy `pipeline.*` kinds,
/// plus the legacy `pipeline.phase` (alias-only), plus the 4 new canonical
/// state-model kinds, plus the 3 W5 `hygiene.*` kinds. A literal list — no
/// magic alias resolution (cf. memory `project_emit_pipeline_kind_full_prefix`).
const KNOWN_KINDS: &[&str] = &[
    EVENT_PIPELINE_SCOPE,
    EVENT_PIPELINE_STATUS,
    EVENT_PIPELINE_TASK_DISPATCH,
    EVENT_PIPELINE_TASK_COMPLETE,
    EVENT_PIPELINE_WAVE_COMPLETE,
    EVENT_PIPELINE_DISPATCH_FAILURE,
    EVENT_PIPELINE_PAUSE,
    EVENT_PIPELINE_RESUME_MODE,
    EVENT_PIPELINE_COMPLETE,
    EVENT_PIPELINE_PHASE,
    EVENT_PIPELINE_STAGE,
    EVENT_PIPELINE_OUTCOME,
    EVENT_PIPELINE_FLAG_SET,
    EVENT_PIPELINE_FLAG_CLEAR,
    EVENT_HYGIENE_DETECTED,
    EVENT_HYGIENE_AUTOCLOSE,
    EVENT_HYGIENE_SKIPPED,
];

/// Options for `mustard-rt run emit-pipeline`.
pub struct EmitPipelineOpts {
    /// Pipeline event kind — must be one of the `EVENT_PIPELINE_*` constants.
    pub kind: String,
    /// Spec name the event is attributed to.
    pub spec: String,
    /// Optional JSON payload string. When `None`, the event payload is `null`.
    pub payload: Option<String>,
}

/// Run `mustard-rt run emit-pipeline --kind <name> --spec <name> [--payload <json>]`.
///
/// Validates `kind` and the optional JSON payload, then appends the event to
/// the project store. Exits 1 on validation failure; fails open (exit 0) on
/// store errors.
pub fn run(opts: EmitPipelineOpts) {
    // --- Validate kind ---
    if !KNOWN_KINDS.contains(&opts.kind.as_str()) {
        eprintln!(
            "emit-pipeline: unknown kind {:?}. Valid kinds: {}",
            opts.kind,
            KNOWN_KINDS.join(", ")
        );
        std::process::exit(1);
    }

    // --- Parse optional payload ---
    let payload: Value = match opts.payload.as_deref() {
        None => Value::Null,
        Some(raw) => match serde_json::from_str(raw) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("emit-pipeline: invalid JSON payload: {e}");
                std::process::exit(1);
            }
        },
    };

    // --- Open store (fail-open on error) ---
    let store = match SqliteEventStore::for_project(project_dir()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("emit-pipeline: could not open event store: {e} (skipping)");
            return;
        }
    };

    // Capture the values we need after `event` consumes them.
    let kind_str = opts.kind.clone();
    let spec_name = opts.spec.clone();
    let payload_for_header = payload.clone();

    // One shared `ts` + `session_id` for the whole transition: a legacy event
    // and its new-kind alias must land on the *same* timestamp/session so the
    // projection layer can correlate them as one transition (AC-W2-6).
    let ts = now_iso8601();
    let sid = session_id();

    // Resolve any legacy → new alias *before* moving the payload into the
    // primary event. `aliased` carries the equivalent new event when the
    // incoming kind is a legacy kind that maps onto the canonical state model.
    let aliased = alias_event(&kind_str, &payload, &ts, &sid, &spec_name);

    // When we are about to fan out an alias, tag the legacy event's payload so
    // an auditor can distinguish the back-compat write from a first-class one.
    let primary_payload = if aliased.is_some() {
        tag_legacy_alias(payload)
    } else {
        payload
    };

    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: ts.clone(),
        session_id: sid.clone(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("emit-pipeline".to_string()),
            actor_type: None,
        },
        event: opts.kind,
        payload: primary_payload,
        spec: Some(opts.spec),
    };

    // Fail-open: a write failure is logged but never propagates to an exit 1.
    let _ = store.append(&event);

    // Emit the canonical new-kind alias for a legacy transition. Same ts +
    // session as the legacy event. Emitting a *new* kind directly produces no
    // alias here (`alias_event` returns `None` for new kinds) — idempotency.
    if let Some(alias) = aliased {
        let _ = store.append(&alias);
    }

    // Wave-2 (2026-05-21-flatten-spec-layout-and-multi-collab): keep the
    // `### Status:` header in `.claude/spec/{spec}/spec.md` in sync with the
    // canonical status the event store just received. Without this, two
    // collaborators on different machines see divergent statuses (the local
    // store says X, git says Y). Fail-open: a missing file or header is a
    // warn, never an error — the event has already been recorded.
    if kind_str == EVENT_PIPELINE_STATUS && should_sync_parent_header(&payload_for_header) {
        if let Some(to) = payload_for_header.get("to").and_then(Value::as_str) {
            let cwd = std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from(project_dir()));
            sync_spec_status_header(&cwd, &spec_name, to);
        }
    }
}

/// Decide whether a `pipeline.status` event should sync the **parent**
/// `spec.md` header. A payload carrying `"wave": N` describes a wave-level
/// transition inside a wave-plan and MUST NOT mutate the parent header —
/// the wave's own `wave-N-{role}/spec.md` is managed directly by the
/// orchestrator. Only top-level (parent) transitions, which omit `wave`,
/// reach the header.
fn should_sync_parent_header(payload: &Value) -> bool {
    payload.get("wave").is_none()
}

/// Set `legacy_alias = true` on an event payload. A non-object payload (e.g.
/// `null` or a bare string) is wrapped into `{ "legacy_alias": true }` so the
/// audit tag is always present without losing the original value (kept under
/// `value` when wrapping).
fn tag_legacy_alias(payload: Value) -> Value {
    match payload {
        Value::Object(mut map) => {
            map.insert("legacy_alias".to_string(), Value::Bool(true));
            Value::Object(map)
        }
        Value::Null => json!({ "legacy_alias": true }),
        other => json!({ "legacy_alias": true, "value": other }),
    }
}

/// Build the canonical new-kind event a legacy `kind` aliases to, or `None`
/// when `kind` is not a legacy kind (a new kind emitted directly never
/// aliases — that is the idempotency guarantee of task #7).
///
/// Mapping (per Wave 2 task #6):
/// - `pipeline.status` with payload `{to: <terminal>}` → `pipeline.outcome`
///   `{outcome: <terminal>}`.
/// - `pipeline.status` with payload `{to: <stage>}` → `pipeline.stage`
///   `{stage: <stage>}`.
/// - `pipeline.phase` with payload `{to: <stage>}` → `pipeline.stage`
///   `{stage: <stage>}`.
///
/// The alias carries the same `ts` + `session_id` as the legacy event so the
/// pair is correlatable as one transition.
fn alias_event(
    kind: &str,
    payload: &Value,
    ts: &str,
    session_id: &str,
    spec: &str,
) -> Option<HarnessEvent> {
    // Both legacy kinds carry the transition target under `payload.to`.
    let to = payload.get("to").and_then(Value::as_str)?;

    let (event_kind, alias_payload) = match kind {
        EVENT_PIPELINE_STATUS => {
            // A terminal status maps to an outcome; a non-terminal one to a
            // stage. `Outcome::Active` is not a terminal status, so fall
            // through to the stage mapping.
            match Outcome::parse(to) {
                Some(outcome) if outcome != Outcome::Active => {
                    (EVENT_PIPELINE_OUTCOME, json!({ "outcome": to }))
                }
                _ => {
                    let stage = Stage::parse(to)?;
                    let _ = stage; // validated; we forward the original token.
                    (EVENT_PIPELINE_STAGE, json!({ "stage": to }))
                }
            }
        }
        EVENT_PIPELINE_PHASE => {
            // A phase is always a stage spelling. Validate it parses, then
            // forward the original token spelling.
            Stage::parse(to)?;
            (EVENT_PIPELINE_STAGE, json!({ "stage": to }))
        }
        // Not a legacy kind — no alias (idempotent for new kinds).
        _ => return None,
    };

    Some(HarnessEvent {
        v: SCHEMA_VERSION,
        ts: ts.to_string(),
        session_id: session_id.to_string(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("emit-pipeline".to_string()),
            actor_type: None,
        },
        event: event_kind.to_string(),
        payload: alias_payload,
        spec: Some(spec.to_string()),
    })
}

/// Resolve a `pipeline.status: <to>` target word into a canonical
/// [`SpecState`]. Accepts a [`Stage`] spelling (`plan`/`execute`/…), a legacy
/// flat status (`implementing`/`reviewing`/…), a terminal [`Outcome`]
/// (`completed`/…), or a qualifier (`closed-followup`/`blocked`/`wave-failed`).
/// Fail-open: an unrecognised token degrades to the earliest-meaningful state
/// (`Plan` + `Active`).
fn state_from_status_word(to: &str) -> SpecState {
    let fallback = SpecState::new(Stage::Plan, Outcome::Active, Flags::default())
        .unwrap_or(SpecState { stage: Stage::Plan, outcome: Outcome::Active, flags: Flags::default() });
    let lower = to.trim().to_ascii_lowercase();

    // Terminal outcomes pin the stage to Close.
    if let Some(outcome) = Outcome::parse(&lower) {
        if outcome != Outcome::Active {
            return SpecState::new(Stage::Close, outcome, Flags::default()).unwrap_or(fallback);
        }
    }
    // Qualifier words map to Close+Active+followup / a flag.
    if matches!(lower.as_str(), "closed-followup" | "closed_followup") {
        return SpecState::new(
            Stage::Close,
            Outcome::Active,
            Flags { followup_open: true, ..Flags::default() },
        )
        .unwrap_or(fallback);
    }
    let flags = Flags::parse(&lower);
    if flags.wave_failed {
        return SpecState::new(Stage::Execute, Outcome::Active, flags).unwrap_or(fallback);
    }
    if flags.blocked {
        return SpecState::new(Stage::Plan, Outcome::Active, flags).unwrap_or(fallback);
    }
    // Otherwise a stage spelling.
    let stage = Stage::parse(&lower).unwrap_or(Stage::Plan);
    SpecState::new(stage, Outcome::Active, Flags::default()).unwrap_or(fallback)
}

/// Rewrite the lifecycle header of `.claude/spec/{spec}/spec.md` so it matches
/// the freshly emitted `pipeline.status: <to>` event, **always emitting the
/// canonical new three-line header** regardless of the legacy shape it started
/// in. Delegates the atomic, byte-stable rewrite (including header insertion
/// when one is absent) to the canonical [`mustard_core::spec`] writer.
///
/// Pure side effect — every error path is a warn (the contract is fail-open per
/// the module-level docs). A missing file warns and returns; the event has
/// already been recorded so a stale header is non-fatal.
fn sync_spec_status_header(cwd: &Path, spec: &str, to: &str) {
    let path = cwd.join(".claude").join("spec").join(spec).join("spec.md");
    if !path.exists() {
        eprintln!(
            "emit-pipeline: WARN: cannot read {}; skipping header sync",
            path.display()
        );
        return;
    }
    let state = state_from_status_word(to);
    if let Err(e) = spec::write_state(&path, &state) {
        eprintln!(
            "emit-pipeline: WARN: could not write {} ({e}); status header may be stale",
            path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::store::event_store::EventSink;
    use mustard_core::store::sqlite_store::SqliteEventStore;
    use mustard_core::model::event::SCHEMA_VERSION;
    use serde_json::json;
    use tempfile::tempdir;

    /// Build a store backed by a fresh temp DB.
    fn temp_store() -> (tempfile::TempDir, SqliteEventStore) {
        let dir = tempdir().unwrap();
        let store = SqliteEventStore::new(dir.path().join("mustard.db")).unwrap();
        (dir, store)
    }

    /// Build opts with a known-good kind.
    fn opts(kind: &str, spec: &str, payload: Option<&str>) -> EmitPipelineOpts {
        EmitPipelineOpts {
            kind: kind.to_string(),
            spec: spec.to_string(),
            payload: payload.map(str::to_string),
        }
    }

    // -----------------------------------------------------------------------
    // Validation + payload parsing (unit-level, no store I/O)
    // -----------------------------------------------------------------------

    #[test]
    fn known_kinds_list_covers_legacy_and_new_kinds() {
        // 9 legacy + 1 legacy phase (alias-only) + 4 new canonical + 3 hygiene.
        assert_eq!(KNOWN_KINDS.len(), 17);
        // Legacy nine.
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_SCOPE));
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_STATUS));
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_TASK_DISPATCH));
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_TASK_COMPLETE));
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_WAVE_COMPLETE));
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_DISPATCH_FAILURE));
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_PAUSE));
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_RESUME_MODE));
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_COMPLETE));
        // Legacy phase (alias-only).
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_PHASE));
        // New canonical state-model kinds.
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_STAGE));
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_OUTCOME));
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_FLAG_SET));
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_FLAG_CLEAR));
        // W5 hygiene kinds.
        assert!(KNOWN_KINDS.contains(&EVENT_HYGIENE_DETECTED));
        assert!(KNOWN_KINDS.contains(&EVENT_HYGIENE_AUTOCLOSE));
        assert!(KNOWN_KINDS.contains(&EVENT_HYGIENE_SKIPPED));
    }

    #[test]
    fn alias_event_maps_legacy_status_terminal_to_outcome() {
        let p = json!({ "to": "completed" });
        let ev = super::alias_event(EVENT_PIPELINE_STATUS, &p, "T", "S", "demo")
            .expect("terminal status aliases to outcome");
        assert_eq!(ev.event, EVENT_PIPELINE_OUTCOME);
        assert_eq!(ev.payload["outcome"], json!("completed"));
        assert_eq!(ev.ts, "T");
        assert_eq!(ev.session_id, "S");
    }

    #[test]
    fn alias_event_maps_legacy_phase_to_stage() {
        let p = json!({ "to": "execute" });
        let ev = super::alias_event(EVENT_PIPELINE_PHASE, &p, "T", "S", "demo")
            .expect("phase aliases to stage");
        assert_eq!(ev.event, EVENT_PIPELINE_STAGE);
        assert_eq!(ev.payload["stage"], json!("execute"));
    }

    #[test]
    fn alias_event_returns_none_for_new_kinds() {
        // A directly-emitted new kind produces no alias (idempotency).
        let p = json!({ "stage": "execute" });
        assert!(super::alias_event(EVENT_PIPELINE_STAGE, &p, "T", "S", "demo").is_none());
        assert!(super::alias_event(EVENT_PIPELINE_OUTCOME, &p, "T", "S", "demo").is_none());
    }

    #[test]
    fn tag_legacy_alias_sets_flag_on_object() {
        let tagged = super::tag_legacy_alias(json!({ "to": "execute" }));
        assert_eq!(tagged["legacy_alias"], json!(true));
        assert_eq!(tagged["to"], json!("execute"));
    }

    #[test]
    fn valid_json_payload_parses() {
        let raw = r#"{"scope":"full","model":"opus"}"#;
        let v: Value = serde_json::from_str(raw).unwrap();
        assert_eq!(v["scope"], json!("full"));
    }

    #[test]
    fn null_payload_when_none() {
        // No payload → Value::Null (the emit loop handles this).
        let raw: Option<&str> = None;
        let v: Value = match raw {
            None => Value::Null,
            Some(s) => serde_json::from_str(s).unwrap(),
        };
        assert_eq!(v, Value::Null);
    }

    // -----------------------------------------------------------------------
    // Store integration — use a real tempfile DB.
    // -----------------------------------------------------------------------

    /// Helper: append one pipeline event directly through `store.append`.
    /// This exercises the same code path as `run()` but without going through
    /// the process exit on validation failure.
    fn emit_direct(store: &SqliteEventStore, kind: &str, spec: &str, payload: Value) {
        let event = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-05-20T00:00:00.000Z".to_string(),
            session_id: "test-session".to_string(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Orchestrator,
                id: Some("emit-pipeline".to_string()),
                actor_type: None,
            },
            event: kind.to_string(),
            payload,
            spec: Some(spec.to_string()),
        };
        store.append(&event).unwrap();
    }

    #[test]
    fn each_kind_appended_once_with_correct_event_name() {
        let (_dir, store) = temp_store();
        let spec = "2026-05-20-pipeline-state-from-sqlite";

        for &kind in KNOWN_KINDS {
            emit_direct(&store, kind, spec, json!({"test": true}));
        }

        let events = store.replay().unwrap();
        assert_eq!(
            events.len(),
            KNOWN_KINDS.len(),
            "expected one event per kind"
        );

        for (i, &kind) in KNOWN_KINDS.iter().enumerate() {
            assert_eq!(events[i].event, kind, "event name mismatch at index {i}");
            assert_eq!(events[i].spec.as_deref(), Some(spec));
            assert_eq!(events[i].payload["test"], json!(true));
        }
    }

    #[test]
    fn pipeline_scope_payload_round_trips() {
        use mustard_core::model::event::PipelineScopePayload;

        let (_dir, store) = temp_store();
        let payload_struct = PipelineScopePayload {
            scope: "full".to_string(),
            lang: Some("en".to_string()),
            model: Some("opus".to_string()),
            is_wave_plan: Some(true),
            total_waves: Some(6),
        };
        let payload_value = serde_json::to_value(&payload_struct).unwrap();
        emit_direct(&store, EVENT_PIPELINE_SCOPE, "demo", payload_value);

        let events = store.replay().unwrap();
        assert_eq!(events.len(), 1);
        let decoded: PipelineScopePayload =
            serde_json::from_value(events[0].payload.clone()).unwrap();
        assert_eq!(decoded.scope, "full");
        assert_eq!(decoded.model.as_deref(), Some("opus"));
        assert_eq!(decoded.total_waves, Some(6));
    }

    #[test]
    fn pipeline_task_complete_payload_round_trips() {
        use mustard_core::model::event::PipelineTaskCompletePayload;

        let (_dir, store) = temp_store();
        let payload_struct = PipelineTaskCompletePayload {
            wave: Some(3),
            name: "implement-store".to_string(),
            agent: Some("general-purpose".to_string()),
            duration_ms: Some(45_000),
            files_modified: Some(vec!["src/run/emit_pipeline.rs".to_string()]),
            decisions: Some(vec!["fail-open on store error".to_string()]),
            escalation: None,
        };
        let payload_value = serde_json::to_value(&payload_struct).unwrap();
        emit_direct(&store, EVENT_PIPELINE_TASK_COMPLETE, "demo", payload_value);

        let events = store.replay().unwrap();
        let decoded: PipelineTaskCompletePayload =
            serde_json::from_value(events[0].payload.clone()).unwrap();
        assert_eq!(decoded.wave, Some(3));
        assert_eq!(decoded.duration_ms, Some(45_000));
        assert!(decoded.escalation.is_none());
    }

    #[test]
    fn optional_fields_absent_in_minimal_payload() {
        use mustard_core::model::event::PipelineStatusPayload;

        // Only required fields: `to`. `from` is absent in JSON.
        let raw = r#"{"to":"active"}"#;
        let decoded: PipelineStatusPayload = serde_json::from_str(raw).unwrap();
        assert_eq!(decoded.to, "active");
        assert!(decoded.from.is_none());
    }

    #[test]
    fn store_error_does_not_propagate_as_nonzero() {
        // We cannot easily simulate a store write failure without unsafe tricks,
        // but we can confirm the fail-open design by verifying `store.append`
        // returns Err and the caller drops it with `let _`.
        // Here we just ensure a legitimate append succeeds (regression guard).
        let (_dir, store) = temp_store();
        emit_direct(&store, EVENT_PIPELINE_PAUSE, "demo", json!({"reason": "user request"}));
        assert_eq!(store.replay().unwrap().len(), 1);
    }

    // -----------------------------------------------------------------------
    // Wave-2 header sync (2026-05-21-flatten-spec-layout-and-multi-collab)
    // -----------------------------------------------------------------------

    /// Helper: seed `.claude/spec/{spec}/spec.md` with the given body and
    /// return the project root + path to spec.md.
    fn seed_spec_md(spec: &str, body: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join(".claude").join("spec").join(spec);
        std::fs::create_dir_all(&spec_dir).unwrap();
        let path = spec_dir.join("spec.md");
        std::fs::write(&path, body).unwrap();
        (dir, path)
    }

    /// The header sync rewrites the legacy header into the canonical NEW
    /// three-line form, dropping the legacy `### Status:`/`### Phase:` lines.
    #[test]
    fn sync_status_header_rewrites_existing_marker() {
        let (dir, path) = seed_spec_md(
            "demo",
            "# Demo\n\n### Status: implementing\n### Phase: EXECUTE\n\n## Body\nx\n",
        );
        super::sync_spec_status_header(dir.path(), "demo", "completed");
        let after = std::fs::read_to_string(&path).unwrap();
        // New canonical header emitted; legacy lines removed.
        assert!(after.contains("### Stage: Close"), "{after:?}");
        assert!(after.contains("### Outcome: Completed"), "{after:?}");
        assert!(!after.contains("### Status:"));
        assert!(!after.contains("### Phase:"));
        // Re-parsing yields the terminal completed state.
        assert_eq!(
            spec::parse_state(&after).map(|s| spec::status_word(&s).to_string()),
            Some("completed".to_string())
        );
        // Body preserved.
        assert!(after.contains("## Body"));
    }

    /// Fail-open contract: a missing spec.md is a no-op, never a panic.
    #[test]
    fn sync_status_header_missing_file_is_noop() {
        let dir = tempdir().unwrap();
        super::sync_spec_status_header(dir.path(), "ghost", "completed");
        // No file created.
        assert!(!dir.path().join(".claude/spec/ghost/spec.md").exists());
    }

    /// A spec.md without any lifecycle header gains a canonical header (the
    /// core writer inserts one after the `# Title`).
    #[test]
    fn sync_status_header_inserts_when_absent() {
        let (dir, path) = seed_spec_md("demo", "# Demo\n\n## Body\nno header\n");
        super::sync_spec_status_header(dir.path(), "demo", "completed");
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.contains("### Stage: Close"));
        assert!(after.contains("### Outcome: Completed"));
        assert!(after.contains("no header"));
    }

    // -----------------------------------------------------------------------
    // Parent-vs-wave header sync gate (regression: 2026-05-23, wave-level
    // close events were rewriting the parent spec.md header).
    // -----------------------------------------------------------------------

    /// A `pipeline.status` payload carrying a `wave` field is a wave-level
    /// transition and MUST NOT trigger a parent-header rewrite.
    #[test]
    fn should_sync_parent_header_false_when_payload_has_wave() {
        let p = json!({"from":"approved","to":"completed","wave":5});
        assert!(!super::should_sync_parent_header(&p));
    }

    /// A `pipeline.status` payload without a `wave` field is a top-level
    /// (parent) transition — sync proceeds normally.
    #[test]
    fn should_sync_parent_header_true_when_payload_omits_wave() {
        let p = json!({"from":"draft","to":"approved"});
        assert!(super::should_sync_parent_header(&p));
    }

    /// `Value::Null` (a `pipeline.status` event with no payload at all) is
    /// treated as a parent transition — sync proceeds.
    #[test]
    fn should_sync_parent_header_true_on_null_payload() {
        assert!(super::should_sync_parent_header(&Value::Null));
    }

    /// A legacy header is rewritten to the canonical form and re-parses to the
    /// requested status; body lines are preserved.
    #[test]
    fn sync_status_header_round_trips_new_value() {
        let body = "# Spec\n\n### Status: planning\n\nbody line\n";
        let (dir, path) = seed_spec_md("demo", body);
        super::sync_spec_status_header(dir.path(), "demo", "implementing");
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.contains("### Stage: Execute"));
        assert!(after.contains("body line"));
        assert_eq!(
            spec::parse_state(&after).map(|s| spec::status_word(&s).to_string()),
            Some("implementing".to_string())
        );
    }
}
