//! `mustard-rt run emit-pipeline` — typed pipeline-event emitter.
//!
//! Records one of the known `pipeline.*` / `hygiene.*` / `pipeline.economy.*`
//! events defined in [`mustard_core::domain::model::event`] constants. Callers supply
//! the event kind, the spec name, and an optional JSON payload string; this
//! module validates both and routes the event through
//! [`crate::shared::events::route::emit`] to the NDJSON sink.
//!
//! ## Fail-open contract
//!
//! - **Unknown kind** → prints an error on stderr and exits with code 1.
//! - **Invalid JSON payload** → prints an error on stderr and exits with code 1.
//! - **Write error** → prints a warning on stderr and exits with code 0 (fail-open).
//!
//! This matches the pattern used by `emit_phase` and every other harness
//! emitter: telemetry is never load-bearing, so a write failure must never
//! break the pipeline.

use crate::shared::context::{project_dir, session_id};
use mustard_core::time::now_iso8601;
use mustard_core::io::claude_paths::ClaudePaths;
use mustard_core::io::fs;
use mustard_core::domain::model::event::{
    Actor, ActorKind, HarnessEvent, SCHEMA_VERSION,
    EVENT_PIPELINE_COMPLETE, EVENT_PIPELINE_DISPATCH_FAILURE, EVENT_PIPELINE_KIND,
    EVENT_PIPELINE_PAUSE, EVENT_PIPELINE_RESUME_MODE, EVENT_PIPELINE_SCOPE, EVENT_PIPELINE_STATUS,
    EVENT_PIPELINE_TASK_COMPLETE, EVENT_PIPELINE_TASK_DISPATCH, EVENT_PIPELINE_WAVE_COMPLETE,
    EVENT_PIPELINE_WAVE_START,
};
use mustard_core::{
    Flags, Outcome, SpecState, Stage, outcome_label, read_meta, stage_label, write_meta,
};
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

/// `pipeline.economy.operation.invoked` — a model operation was completed via
/// the `claude` CLI cold-path (scan interpret). Payload carries `operation`,
/// `duration_ms`, and `tokens_used: 0` (cost via CLI subscription, not API
/// key). Feeds the `/economia` dashboard (W12).
const EVENT_ECONOMY_OPERATION_INVOKED: &str = "pipeline.economy.operation.invoked";

/// The 20 valid pipeline event kind strings: the 9 legacy `pipeline.*` kinds,
/// plus the legacy `pipeline.phase` (alias-only), plus the `pipeline.wave.start`
/// signal, plus the 4 new canonical state-model kinds, plus the 3 W5
/// `hygiene.*` kinds, plus the 1 W2 `pipeline.economy.*` kind, plus the
/// `pipeline.kind` work-type signal (porta-unica). A literal list — no magic
/// alias resolution (cf. memory `project_emit_pipeline_kind_full_prefix`).
const KNOWN_KINDS: &[&str] = &[
    EVENT_PIPELINE_SCOPE,
    EVENT_PIPELINE_STATUS,
    EVENT_PIPELINE_TASK_DISPATCH,
    EVENT_PIPELINE_TASK_COMPLETE,
    EVENT_PIPELINE_WAVE_START,
    EVENT_PIPELINE_WAVE_COMPLETE,
    EVENT_PIPELINE_DISPATCH_FAILURE,
    EVENT_PIPELINE_PAUSE,
    EVENT_PIPELINE_RESUME_MODE,
    EVENT_PIPELINE_COMPLETE,
    EVENT_PIPELINE_KIND,
    EVENT_PIPELINE_PHASE,
    EVENT_PIPELINE_STAGE,
    EVENT_PIPELINE_OUTCOME,
    EVENT_PIPELINE_FLAG_SET,
    EVENT_PIPELINE_FLAG_CLEAR,
    EVENT_HYGIENE_DETECTED,
    EVENT_HYGIENE_AUTOCLOSE,
    EVENT_HYGIENE_SKIPPED,
    EVENT_ECONOMY_OPERATION_INVOKED,
];

/// Options for `mustard-rt run emit-pipeline`.
pub struct EmitPipelineOpts {
    /// Pipeline event kind — must be one of the `EVENT_PIPELINE_*` constants.
    pub kind: String,
    /// Spec name the event is attributed to.
    pub spec: String,
    /// Optional JSON payload string. When `None`, the event payload is `null`.
    pub payload: Option<String>,
    /// Bypass the QA gate on `pipeline.complete`. Used by trusted callers
    /// (notably `qa-run` itself when it needs to chain `pipeline.complete`
    /// inside its own flow, or an explicit user override).
    pub allow_no_qa: bool,
    /// Free-form natural-language request. Only consulted on
    /// `--kind pipeline.kind` for a spec-less run: it seeds the auto-branch
    /// slug (`{base}_{slug}`) when no `--spec` is present. Ignored otherwise.
    pub intent: Option<String>,
    /// Integration base branch the work branch is cut from. On
    /// `--kind pipeline.kind` the auto-branch becomes `{base}_{slug}` when this
    /// names one of the project's `git.flow` integration bases; otherwise the
    /// project's primary base is used. Agnostic — the base set is derived from
    /// `git.flow`, never hardcoded. Ignored for other kinds.
    pub base: Option<String>,
}

/// Parse the `--payload` JSON, tolerating a PowerShell quoting quirk.
///
/// PowerShell single-quotes are literal, so a caller using the bash habit of
/// backslash-escaping the inner quotes — `--payload '{\"wave\":1}'` — has those
/// backslashes PRESERVED: the arg arrives as the literal `{\"wave\":1}`, invalid
/// JSON ("key must be a string at line 1 column 2", the `\` right after `{`),
/// and the orchestrator burns a round-trip re-emitting (recurring field case,
/// sialia). Recover: if the first parse fails AND the raw still carries the `\"`
/// artefact, strip it and retry. A correctly-quoted payload (bash, or PowerShell
/// single-quoted *without* the escaping) parses on the first attempt, so a JSON
/// string value that legitimately contains `\"` is never reached by the fallback
/// and the original parse error is preserved when recovery also fails.
fn parse_payload_tolerant(raw: &str) -> Result<Value, serde_json::Error> {
    match serde_json::from_str::<Value>(raw) {
        Ok(v) => Ok(v),
        Err(first_err) => {
            if raw.contains("\\\"") {
                if let Ok(v) = serde_json::from_str::<Value>(&raw.replace("\\\"", "\"")) {
                    return Ok(v);
                }
            }
            Err(first_err)
        }
    }
}

/// Run `mustard-rt run emit-pipeline --kind <name> --spec <name> [--payload <json>]`.
///
/// Validates `kind` and the optional JSON payload, then appends the event to
/// the project store. Exits 1 on validation failure; fails open (exit 0) on
/// store errors.
///
/// **REVIEW/QA gate (2026-05-25):** when `kind == pipeline.complete`, refuses
/// the emission with exit code 2 unless either
/// 1. a `qa.result` event with `overall == "pass"` exists for the spec, or
/// 2. `--allow-no-qa` is set.
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

    // --- REVIEW/QA gate: pipeline.complete requires qa.result(overall=pass). ---
    //
    // Without this gate the orchestrator can emit `pipeline.complete` after
    // EXECUTE finishes the last wave, silently skipping REVIEW + QA. Fail-open
    // applies only to *event-store unreachable*: if we cannot read the spec's
    // events dir we take the conservative path (block emission), since allowing
    // a complete on a missing store would erase the gate entirely.
    if opts.kind == EVENT_PIPELINE_COMPLETE && !opts.allow_no_qa {
        let cwd = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from(project_dir()));
        if !qa_result_passed(&cwd, &opts.spec) {
            eprintln!(
                "BLOCKED: cannot emit pipeline.complete for {} — no qa.result event \
                 with overall=pass exists. Run: rtk mustard-rt run qa-run --spec {}",
                opts.spec, opts.spec
            );
            std::process::exit(2);
        }
    }

    // --- Parse optional payload ---
    //
    // A missing `--payload` is normally `null`. For `pipeline.complete` that
    // null breaks the projection (`serde_json::from_value::<PipelineComplete
    // Payload>(null)` → "invalid type: null, expected struct"). Default a bare
    // `pipeline.complete` to `{}` so a valid empty `PipelineCompletePayload` is
    // emitted (the projection is also hardened to tolerate null — both ends).
    let payload: Value = match opts.payload.as_deref() {
        None if opts.kind == EVENT_PIPELINE_COMPLETE => json!({}),
        None => Value::Null,
        Some(raw) => match parse_payload_tolerant(raw) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("emit-pipeline: invalid JSON payload: {e}");
                std::process::exit(1);
            }
        },
    };

    // W5: the event router (`route::emit`) opens the SQLite store on
    // demand for `pipeline.*` events; there is no need to open it eagerly here.

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
    // W5: route through `route::emit` so `pipeline.*` → SQLite while
    // `hygiene.*` / other non-pipeline kinds land in the per-spec NDJSON sink.
    let _ = crate::shared::events::route::emit(&project_dir(), &event);

    // Emit the canonical new-kind alias for a legacy transition. Same ts +
    // session as the legacy event. Emitting a *new* kind directly produces no
    // alias here (`alias_event` returns `None` for new kinds) — idempotency.
    if let Some(alias) = aliased {
        let _ = crate::shared::events::route::emit(&project_dir(), &alias);
    }

    // Sync spec.md header + meta.json whenever a pipeline.status or
    // pipeline.stage/outcome event carries a status transition. Every
    // transition that changes status calls sync_status on the corresponding
    // file (parent or wave). Fail-open: errors are warnings only — the event
    // has already been recorded.
    if kind_str == EVENT_PIPELINE_STATUS {
        if let Some(to) = payload_for_header.get("to").and_then(Value::as_str) {
            let cwd = std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from(project_dir()));
            let state = state_from_status_word(to);
            // Determine target path: wave-level transitions carry a `wave` field
            // and sync the wave's spec.md; top-level transitions sync the parent.
            let spec_path = if let Some(wave) = payload_for_header.get("wave").and_then(Value::as_u64) {
                wave_spec_path(&cwd, &spec_name, wave)
            } else {
                ClaudePaths::for_project(&cwd)
                    .and_then(|p| p.for_spec(&spec_name))
                    .ok()
                    .map(|sp| sp.dir().to_path_buf())
            };
            if let Some(path) = spec_path {
                if let Err(e) = crate::commands::spec::spec_scaffold::sync_status(state, &path) {
                    eprintln!("emit-pipeline: WARN: sync_status failed ({e}); headers may be stale");
                }
            }
        }
    }

    // `pipeline.wave.complete`: sync the wave's spec.md + meta.json to
    // Close/Completed AND bump the parent's progress fields. Fail-open.
    if kind_str == EVENT_PIPELINE_WAVE_COMPLETE {
        if let Some(wave) = payload_for_header.get("wave").and_then(Value::as_u64) {
            let cwd = std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from(project_dir()));
            if let Some(wave_path) = wave_spec_path(&cwd, &spec_name, wave) {
                let wave_done = SpecState::new(Stage::Close, Outcome::Completed, Flags::default())
                    .unwrap_or(SpecState {
                        stage: Stage::Close,
                        outcome: Outcome::Completed,
                        flags: Flags::default(),
                    });
                if let Err(e) =
                    crate::commands::spec::spec_scaffold::sync_status(wave_done, &wave_path)
                {
                    eprintln!(
                        "emit-pipeline: WARN: sync_status wave failed ({e}); wave headers may be stale"
                    );
                }
                // Backfill the wave's checklist by file existence: a completing
                // wave whose planned files are on disk must not close with
                // unchecked items. The PostToolUse auto-mark can miss a live edit
                // (subagent context, a non-Write tool); this is the deterministic
                // net at the wave boundary. Forward-only (never un-marks).
                reconcile_wave_checklist(&cwd, &wave_path);
            } else {
                eprintln!(
                    "emit-pipeline: WARN: no `wave-{wave}-*` directory under .claude/spec/{spec_name}; wave sync skipped"
                );
            }
            bump_parent_progress(&cwd, &spec_name, wave, &ts);
        }
    }

    // `pipeline.wave.start`: advance the STARTED wave's own meta.json Plan→Execute
    // (forward-only). Symmetric to the wave.complete sync above — without it the
    // active wave's sidecar stays "Plan" for its whole run (a reader of the
    // per-wave stage rendered an executing wave as PLANEJANDO). The dashboard's
    // wave-row projection flips InProgress off the EVENT itself. Fail-open.
    if kind_str == EVENT_PIPELINE_WAVE_START {
        if let Some(wave) = payload_for_header.get("wave").and_then(Value::as_u64) {
            let cwd = std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from(project_dir()));
            sync_wave_started(&cwd, &spec_name, wave, &ts);
        }
    }

    // `pipeline.kind` (porta-unica work-type signal): pre-compute the auto-branch
    // name the FIRST file mutation of this work unit will check out, and persist
    // it as the session's `pending-work-branch` marker. `work_branch_gate` reads
    // it back on the first Write/Edit. A read-only request never edits, so the
    // marker is simply never consumed. Fail-open — the emit already succeeded.
    if kind_str == EVENT_PIPELINE_KIND {
        let project = project_dir();
        let config = mustard_core::ProjectConfig::load(Path::new(&project));
        let base = resolve_base(opts.base.as_deref(), &config);
        let branch =
            compute_work_branch(&base, &spec_name, opts.intent.as_deref(), &sid, &ts, &project);
        crate::shared::context::set_pending_branch(&project, &sid, &branch);
    }

    // `pipeline.stage` / `pipeline.outcome`: patch the spec's `meta.json` so the
    // sidecar tracks the canonical state-model transition. Without this the
    // sidecar stays stuck at its last `pipeline.status`-synced value and
    // `active-specs` shows a phantom active spec after CLOSE. Reuses the
    // canonical `Meta` read-modify-write (no parallel writer). Fail-open.
    if kind_str == EVENT_PIPELINE_STAGE || kind_str == EVENT_PIPELINE_OUTCOME {
        let cwd = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from(project_dir()));
        patch_meta_for_transition(&cwd, &spec_name, &kind_str, &payload_for_header, &ts);
    }

    // `pipeline.complete`: the spec is done. Set `outcome = Completed` +
    // `stage = Close` (+ `phase = CLOSE`) in `meta.json` so `active-specs` no
    // longer lists it, AND emit the terminal `pipeline.status: completed` event
    // so the event projection agrees with the sidecar (no divergence). Without
    // the status emit a run-face `pipeline.complete` patched meta to Completed
    // while the event log's last status stayed mid-pipeline (or, for the legacy
    // close path, `closed-followup`). The QA gate above already guaranteed this
    // transition is legitimate. Fail-open.
    if kind_str == EVENT_PIPELINE_COMPLETE {
        let cwd = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from(project_dir()));
        patch_meta_complete(&cwd, &spec_name, &ts);
        emit_completed_status_if_needed(&cwd, &spec_name, &ts, &sid);
    }

    // Cleanup: remove the `.pipeline-states/{spec}.json` file when a terminal
    // event is emitted so `current_spec` step-3 (FS fallback) doesn't return
    // this closed spec in a future session. Fail-open: missing file is fine.
    let is_terminal = is_terminal_event(&kind_str, &payload_for_header);
    if is_terminal {
        let cwd = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from(project_dir()));
        if let Ok(paths) = ClaudePaths::for_project(&cwd) {
            let state_file = paths
                .pipeline_states_dir()
                .join(format!("{spec_name}.json"));
            let _ = fs::remove_file(&state_file);
        }
    }

    // Success echo — the emitter used to succeed in TOTAL silence, which made
    // the harness's own traceability tool opaque on the happy path (field
    // feedback, sialia 2026-07-09: "para um harness de rastreabilidade, a
    // própria ferramenta é opaca no sucesso"). One deterministic line: what
    // was recorded, for which spec. No timestamp/session in it (run outputs
    // are byte-compared in gates) — the NDJSON row carries those.
    println!("{}", json!({ "ok": true, "kind": kind_str, "spec": spec_name }));
}

/// Returns `true` when the spec has a `qa.result` event with
/// `overall == "pass"` in its per-spec NDJSON event log.
///
/// **Fail-open semantics:** a missing events dir, an unreadable file, or no
/// matching event all return `false` — meaning the gate stays *closed*. This
/// is the opposite of telemetry-style fail-open: we are guarding a verdict, so
/// the conservative outcome on missing data is to block (not allow). Callers
/// can opt out via `--allow-no-qa`.
fn qa_result_passed(cwd: &Path, spec: &str) -> bool {
    let events_dir = ClaudePaths::for_project(cwd)
        .and_then(|p| p.for_spec(spec))
        .ok()
        .map_or_else(
            || ClaudePaths::compose_unchecked(cwd).spec_dir().join(spec).join(".events"),
            |sp| sp.dir().join(".events"),
        );
    let mut events =
        mustard_core::view::projection::read_harness_events_from_ndjson_dir(&events_dir);
    // Chronological order — last matching event wins (mirrors `close_gate`).
    events.sort_by(|a, b| a.ts.cmp(&b.ts));
    let mut last_overall: Option<String> = None;
    for ev in events {
        if ev.event != "qa.result" {
            continue;
        }
        if let Some(ev_spec) = ev.payload.get("spec").and_then(Value::as_str) {
            if ev_spec != spec {
                continue;
            }
        }
        last_overall = ev
            .payload
            .get("overall")
            .and_then(Value::as_str)
            .map(str::to_string);
    }
    last_overall.as_deref() == Some("pass")
}

/// Returns `true` when the event kind + payload indicate a terminal pipeline
/// transition (spec is closed / completed / cancelled / abandoned).
fn is_terminal_event(kind: &str, payload: &Value) -> bool {
    if kind == EVENT_PIPELINE_COMPLETE {
        return true;
    }
    // `pipeline.status` or `pipeline.outcome` with a terminal `to`/`outcome`.
    if kind == EVENT_PIPELINE_STATUS || kind == EVENT_PIPELINE_OUTCOME {
        let to = payload
            .get("to")
            .or_else(|| payload.get("outcome"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let lower = to.trim().to_ascii_lowercase();
        // Wave 4 of deep-refactor (2026-05-25) added `superseded`/`absorbed`
        // as first-class terminal outcomes — both close the spec.
        return matches!(
            lower.as_str(),
            "completed" | "cancelled" | "abandoned" | "superseded" | "absorbed"
        );
    }
    false
}

/// Resolve the `wave-{N}-*` directory path for a spec. Returns `None` when
/// the spec directory does not exist or no matching wave subdirectory is found.
pub(crate) fn wave_spec_path(cwd: &Path, spec: &str, wave: u64) -> Option<std::path::PathBuf> {
    let spec_dir = ClaudePaths::for_project(cwd)
        .and_then(|p| p.for_spec(spec))
        .ok()?
        .dir()
        .to_path_buf();
    if !spec_dir.is_dir() {
        return None;
    }
    let prefix = format!("wave-{wave}-");
    fs::read_dir(&spec_dir)
        .ok()?
        .into_iter()
        .find(|e| e.file_name.starts_with(&prefix) && e.path.is_dir())
        .map(|e| e.path)
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


/// Uppercase phase token (`ANALYZE`/`PLAN`/`EXECUTE`/`QA`/`CLOSE`) for a
/// canonical [`Stage`]. This is the `meta.json#phase` spelling the dashboard
/// and `bump_parent_progress` already emit; the canonical state machine remains
/// `stage` + `outcome` + `flags`, but `phase` is kept in sync for the cards.
const fn phase_token_for_stage(stage: Stage) -> &'static str {
    match stage {
        Stage::Analyze => "ANALYZE",
        Stage::Plan => "PLAN",
        Stage::Execute => "EXECUTE",
        Stage::QaReview => "QA",
        Stage::Close => "CLOSE",
        // `Stage` is `#[non_exhaustive]`; a future variant falls back to the
        // mid-pipeline phase rather than panicking (this token is advisory).
        _ => "EXECUTE",
    }
}

/// Canonical pipeline position of a [`Stage`] (0..=4), in
/// `ANALYZE → PLAN → EXECUTE → QA/REVIEW → CLOSE` order. Used for forward-only
/// stage comparisons (e.g. `bump_parent_progress` never regresses a parent that
/// has already advanced past EXECUTE). `Stage` is `#[non_exhaustive]`; an
/// unknown future variant ranks at the terminal end so it is treated as "at
/// least as far along as Close" and never regressed.
const fn stage_rank(stage: Stage) -> u8 {
    match stage {
        Stage::Analyze => 0,
        Stage::Plan => 1,
        Stage::Execute => 2,
        Stage::QaReview => 3,
        Stage::Close => 4,
        _ => 4,
    }
}

/// Resolve the `meta.json` path for a spec — the wave's sidecar when the payload
/// carries a `wave` field, the top-level spec's sidecar otherwise. Returns
/// `None` when the spec (or wave) directory does not exist.
fn meta_path_for(cwd: &Path, spec: &str, payload: &Value) -> Option<std::path::PathBuf> {
    let dir = if let Some(wave) = payload.get("wave").and_then(Value::as_u64) {
        wave_spec_path(cwd, spec, wave)?
    } else {
        ClaudePaths::for_project(cwd)
            .and_then(|p| p.for_spec(spec))
            .ok()
            .map(|sp| sp.dir().to_path_buf())?
    };
    dir.is_dir().then(|| dir.join("meta.json"))
}

/// Patch a spec's `meta.json` for a `pipeline.stage` / `pipeline.outcome`
/// transition. Reuses the canonical [`Meta`](mustard_core::domain::meta::Meta)
/// read-modify-write (atomic via `write_meta`), preserving every other field:
///
/// - `pipeline.stage {stage: <s>}` → `stage` + `phase` updated; `outcome`
///   left as-is (a stage move keeps the spec Active).
/// - `pipeline.outcome {outcome: <o>}` → `outcome` updated; a terminal outcome
///   pins `stage = Close` + `phase = CLOSE` (matching [`SpecState::new`]).
///
/// `checkpoint` is always bumped to `ts`. Fail-open: a missing spec dir,
/// unparseable sidecar, or write failure all warn on stderr and return.
///
/// `pub(crate)` so sibling commands (notably `approve_spec`) can assert the
/// wave-aware sidecar patch in their own tests without going through the
/// process-global `run()` entry — it is the same routine `run()` calls after
/// writing a `pipeline.stage` / `pipeline.outcome` event.
pub(crate) fn patch_meta_for_transition(cwd: &Path, spec: &str, kind: &str, payload: &Value, ts: &str) {
    let Some(path) = meta_path_for(cwd, spec, payload) else {
        return;
    };
    let mut meta = read_meta(&path).unwrap_or_default();

    match kind {
        EVENT_PIPELINE_STAGE => {
            let Some(stage) = payload
                .get("stage")
                .and_then(Value::as_str)
                .and_then(Stage::parse)
            else {
                return;
            };
            meta.stage = Some(stage_label(stage).to_string());
            meta.phase = Some(phase_token_for_stage(stage).to_string());
        }
        EVENT_PIPELINE_OUTCOME => {
            let Some(outcome) = payload
                .get("outcome")
                .and_then(Value::as_str)
                .and_then(Outcome::parse)
            else {
                return;
            };
            meta.outcome = Some(outcome_label(outcome).to_string());
            // A terminal outcome only ever pairs with Close (SpecState invariant).
            if outcome != Outcome::Active {
                meta.stage = Some(stage_label(Stage::Close).to_string());
                meta.phase = Some(phase_token_for_stage(Stage::Close).to_string());
            }
        }
        _ => return,
    }

    meta.checkpoint = Some(ts.to_string());
    if let Err(e) = write_meta(&path, &meta) {
        eprintln!(
            "emit-pipeline: WARN: could not write {} ({e}); meta.json may be stale",
            path.display()
        );
    }
}

/// Patch a spec's **root** `meta.json` for a `pipeline.complete` event: the spec
/// is done, so `outcome = Completed`, `stage = Close`, `phase = CLOSE`. Reuses
/// the canonical [`Meta`](mustard_core::domain::meta::Meta) read-modify-write
/// (atomic), preserving every other field. Fail-open.
///
/// `pub(crate)` so the close flow (`complete_spec::mark_complete`) can re-use
/// the same sidecar-sync after it emits the terminal events directly via
/// `writer_ndjson` (that path bypasses `emit-pipeline run`, which is the bug
/// that left finished specs stuck at `Plan/Active`).
pub(crate) fn patch_meta_complete(cwd: &Path, spec: &str, ts: &str) {
    let Some(path) = meta_path_for(cwd, spec, &Value::Null) else {
        return;
    };
    let mut meta = read_meta(&path).unwrap_or_default();
    meta.stage = Some(stage_label(Stage::Close).to_string());
    meta.outcome = Some(outcome_label(Outcome::Completed).to_string());
    meta.phase = Some(phase_token_for_stage(Stage::Close).to_string());
    meta.checkpoint = Some(ts.to_string());
    if let Err(e) = write_meta(&path, &meta) {
        eprintln!(
            "emit-pipeline: WARN: could not write {} ({e}); meta.json may be stale",
            path.display()
        );
    }
}

/// Emit a terminal `pipeline.status: completed` event for `spec` so the event
/// projection lands on `completed` alongside the `pipeline.complete` audit
/// marker (whose payload only carries `closedAt` + `affectedFiles` and never
/// changes the projected status). Reuses the `ts`/`session_id` of the
/// triggering `pipeline.complete` so the pair correlates as one transition.
///
/// Idempotent — skips the emit when the projection already shows `completed`
/// or `cancelled` (mirrors `complete_spec::mark_complete`'s short-circuit), so
/// a second `pipeline.complete` (or the `complete_spec` path, which already
/// emitted its own `completed`) does not append a duplicate status flip.
///
/// Fail-open: a missing/unreadable events dir degrades to "emit" (the
/// conservative default — record the terminal status), and the route write is
/// itself best-effort.
fn emit_completed_status_if_needed(cwd: &Path, spec: &str, ts: &str, session_id: &str) {
    let events_dir = ClaudePaths::for_project(cwd)
        .and_then(|p| p.for_spec(spec))
        .ok()
        .map(|sp| sp.events_dir());
    if let Some(dir) = events_dir {
        let events =
            mustard_core::view::projection::read_harness_events_from_ndjson_dir(&dir);
        let current_status =
            crate::commands::event::event_projections::pipeline_state_from_events(&events, spec, None)
                .and_then(|v| v.status);
        if matches!(current_status.as_deref(), Some("completed" | "cancelled")) {
            return;
        }
    }

    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: ts.to_string(),
        session_id: session_id.to_string(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("emit-pipeline".to_string()),
            actor_type: None,
        },
        event: EVENT_PIPELINE_STATUS.to_string(),
        payload: json!({ "to": "completed" }),
        spec: Some(spec.to_string()),
    };
    let _ = crate::shared::events::route::emit(&cwd.to_string_lossy(), &event);
}

/// On `pipeline.wave.start`: advance the STARTED wave's own `meta.json` from
/// `Plan` to `Execute` — **forward-only** (a wave already at `Execute` or later,
/// e.g. `Close` from a late/duplicate start, is never regressed). The per-wave
/// sidecar otherwise stays `Plan` for the whole run (it only ever flips to
/// `Close` on `wave.complete`), so any reader of the per-wave stage rendered an
/// actively-running wave as PLANEJANDO. Fail-open: a missing wave dir /
/// unparseable sidecar / write failure all warn and return.
fn sync_wave_started(cwd: &Path, spec: &str, wave: u64, ts: &str) {
    let Some(wave_dir) = wave_spec_path(cwd, spec, wave) else {
        eprintln!(
            "emit-pipeline: WARN: no `wave-{wave}-*` directory under .claude/spec/{spec}; wave-start sync skipped"
        );
        return;
    };
    let path = wave_dir.join("meta.json");
    let mut meta = read_meta(&path).unwrap_or_default();
    let advance = match meta.stage.as_deref().and_then(Stage::parse) {
        None => true,
        Some(stage) => stage_rank(stage) < stage_rank(Stage::Execute),
    };
    if !advance {
        return;
    }
    meta.stage = Some(stage_label(Stage::Execute).to_string());
    meta.phase = Some(phase_token_for_stage(Stage::Execute).to_string());
    meta.checkpoint = Some(ts.to_string());
    if let Err(e) = write_meta(&path, &meta) {
        eprintln!(
            "emit-pipeline: WARN: could not write {} ({e}); wave meta.json may be stale",
            path.display()
        );
    }
}

/// Backfill a wave's checklist on completion: mark `done = true` for any item
/// whose target `path` exists on disk (relative to `cwd`). A wave's checklist
/// items are its planned files, so existence at completion == the work landed —
/// this is the deterministic net for the PostToolUse auto-mark's live misses (a
/// wave that closed with unchecked items whose files clearly exist). Forward-only
/// (never un-marks). Fail-open: an empty/unreadable sidecar is a no-op.
fn reconcile_wave_checklist(cwd: &Path, wave_dir: &Path) {
    let path = wave_dir.join("meta.json");
    let mut meta = read_meta(&path).unwrap_or_default();
    if meta.checklist.is_empty() {
        return;
    }
    let mut changed = false;
    for item in &mut meta.checklist {
        if item.done {
            continue;
        }
        if let Some(p) = item.path.as_deref() {
            if !p.trim().is_empty() && cwd.join(p).exists() {
                item.done = true;
                changed = true;
            }
        }
    }
    if changed {
        if let Err(e) = write_meta(&path, &meta) {
            eprintln!(
                "emit-pipeline: WARN: could not write {} ({e}); checklist reconcile lost",
                path.display()
            );
        }
    }
}

/// Path-explicit `pipeline.wave.start` emit: routes the event under `project`
/// and advances the started wave's meta `Plan→Execute` (via [`sync_wave_started`]).
///
/// `wave-advance` calls this for each wave it dispatches — the deterministic
/// "wave is starting" signal the dashboard's wave projection needs to flip the
/// row to `InProgress`. The env-var-based `wave_start_observer` cannot fire
/// (nothing sets `MUSTARD_ACTIVE_WAVE` — `std::env::set_var` is forbidden under
/// edition 2024), so the reliable emitter is the dispatch composite that already
/// KNOWS the wave and the project root. Takes an explicit `project` (not the
/// process cwd) so it is path-correct under test. Fail-open.
pub(crate) fn emit_wave_start(project: &Path, spec: &str, wave: u32) {
    let ts = now_iso8601();
    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: ts.clone(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("wave-advance".to_string()),
            actor_type: None,
        },
        event: EVENT_PIPELINE_WAVE_START.to_string(),
        payload: json!({ "wave": wave }),
        spec: Some(spec.to_string()),
    };
    let _ = crate::shared::events::route::emit(&project.to_string_lossy(), &event);
    sync_wave_started(project, spec, u64::from(wave), &ts);
}

/// Tactical-fix 2026-05-26: bump parent `meta.json` progress fields on a
/// `pipeline.wave.complete` event. Sets:
///   - `raw.currentWave = wave`
///   - `raw.completedWaves = [..., wave]` (deduplicated, sorted ascending)
///   - `phase = "EXECUTE"` when `wave < total_waves` or `total_waves` is None
///   - `phase = "CLOSE"` when `wave >= total_waves`
///   - `checkpoint = ts`
///
/// 2026-06-05 fix: on the EXECUTE branch, advance the native `stage` to
/// `Execute` too — **forward-only**. A wave-plan parent was left
/// `{stage:"Plan", phase:"EXECUTE"}` because the docstring's old promise to
/// "leave `stage` untouched" meant the dashboard (which reads `stage` via
/// `detect_stage`/`status_word`) showed PLANEJANDO all through execution. We
/// only ever push `stage` *forward*: if it is already `Execute` or a later
/// stage (`QaReview`/`Close`) we leave it be, never regressing it. The CLOSE
/// branch still never touches `stage` — that terminal transition stays driven
/// by `pipeline.status` / `pipeline.outcome`, not by an interior wave.
///
/// `outcome` is still left untouched here (a wave completing does not make the
/// parent terminal).
///
/// Fail-open: a missing spec dir, missing/unparseable sidecar, or write
/// failure all warn on stderr and return without propagating.
fn bump_parent_progress(cwd: &Path, spec: &str, wave: u64, ts: &str) {
    let Some(spec_dir) = ClaudePaths::for_project(cwd)
        .and_then(|p| p.for_spec(spec))
        .ok()
        .map(|sp| sp.dir().to_path_buf())
    else {
        return;
    };
    if !spec_dir.is_dir() {
        return;
    }
    let path = spec_dir.join("meta.json");
    let mut meta = read_meta(&path).unwrap_or_default();

    // Decide phase based on `total_waves` (native field).
    let new_phase = match meta.total_waves {
        Some(total) if wave >= u64::from(total) => "CLOSE",
        _ => "EXECUTE",
    };
    meta.phase = Some(new_phase.to_string());
    meta.checkpoint = Some(ts.to_string());

    // Advance the native `stage` to `Execute` on the EXECUTE branch — but
    // forward-only. The dashboard reads `stage` (not `phase`) as the lifecycle
    // source of truth, so a wave-plan parent stuck at `stage:"Plan"` rendered as
    // PLANEJANDO during execution. We only push forward: if the current stage
    // already ranks at `Execute` or later (`QaReview`/`Close`) we leave it
    // untouched, never regressing. The CLOSE branch never touches `stage` — that
    // terminal move stays driven by `pipeline.status`/`pipeline.outcome`.
    if new_phase == "EXECUTE" {
        let current = meta
            .stage
            .as_deref()
            .and_then(Stage::parse);
        let advance = match current {
            // No parseable stage yet, or an earlier stage than Execute: advance.
            None => true,
            Some(stage) => stage_rank(stage) < stage_rank(Stage::Execute),
        };
        if advance {
            meta.stage = Some(stage_label(Stage::Execute).to_string());
        }
    }

    // Ensure `raw` is an object before mutating progress fields. A
    // freshly-defaulted Meta carries `raw: Value::Null`.
    if !meta.raw.is_object() {
        meta.raw = json!({});
    }
    if let Some(obj) = meta.raw.as_object_mut() {
        // currentWave — always overwrite with the latest wave number.
        obj.insert("currentWave".to_string(), json!(wave));

        // completedWaves — read existing array (if any), push, dedupe + sort.
        let mut completed: Vec<u64> = obj
            .get("completedWaves")
            .and_then(Value::as_array)
            .map(|arr| arr.iter().filter_map(Value::as_u64).collect())
            .unwrap_or_default();
        completed.push(wave);
        completed.sort_unstable();
        completed.dedup();
        let completed_value: Vec<Value> = completed.into_iter().map(|n| json!(n)).collect();
        obj.insert("completedWaves".to_string(), Value::Array(completed_value));
    }

    if let Err(e) = write_meta(&path, &meta) {
        eprintln!(
            "emit-pipeline: WARN: could not write {} ({e}); parent meta.json may be stale",
            path.display()
        );
    }

    // Final-wave auto-settle: when the LAST wave completes (`phase → CLOSE`), the
    // parent must not linger at `{stage:Execute, outcome:Active, phase:CLOSE}` —
    // a state the dashboard reads (via `stage`) as "implementing" forever until
    // an operator runs `/close`. Decide by the QA gate + acceptance criteria
    // whether to finalize now or surface as "awaiting close". This is additive
    // to the progress writes above (never regresses them). Fail-open.
    if new_phase == "CLOSE" {
        settle_final_wave(cwd, spec, ts);
    }
}

/// On the FINAL `pipeline.wave.complete` (the wave that drives `phase → CLOSE`),
/// settle the parent's lifecycle instead of leaving it at
/// `{stage:Execute, outcome:Active, phase:CLOSE}` — the state the dashboard
/// renders as "implementing" until someone runs `/close`.
///
/// `qa_required` = the QA close-gate is active (`MUSTARD_QA_GATE_MODE != off`,
/// default `strict`, resolved by the SAME cascade the CLOSE gate uses) AND the
/// spec actually carries executable acceptance criteria (its own `## Acceptance
/// Criteria` items or a linked-capability AC — the exact union `qa-run` runs).
/// When it is FALSE — precisely the case where `qa-run` would `skip` — the spec
/// is auto-finalized exactly like `complete-spec`: [`patch_meta_complete`] →
/// `Close/Completed/CLOSE`, plus a `pipeline.complete` event and the terminal
/// `pipeline.status: completed` so the events log / dashboard / auto-verify all
/// see the close (matching [`crate::commands::spec::complete_spec`]). When it is
/// TRUE, the parent only advances `stage → QaReview` (outcome stays `Active`,
/// phase stays `CLOSE`) so it surfaces as "awaiting close"; the real finalize
/// stays with `/close` after QA passes.
///
/// Idempotent: a parent already at `Close/Completed` is left untouched, so a
/// straggling / duplicate final `wave.complete` does not re-finalize or
/// re-emit. Fail-open — every path degrades without panicking.
fn settle_final_wave(cwd: &Path, spec: &str, ts: &str) {
    let Some(path) = meta_path_for(cwd, spec, &Value::Null) else {
        return;
    };
    let meta = read_meta(&path).unwrap_or_default();
    let stage = meta.stage.as_deref().and_then(Stage::parse);
    let outcome = meta.outcome.as_deref().and_then(Outcome::parse);
    // Already finalized → nothing to do (idempotent).
    if stage == Some(Stage::Close) && outcome == Some(Outcome::Completed) {
        return;
    }

    let qa_required = crate::hooks::write::close_gate::qa_gate_active()
        && crate::commands::review::qa_run::spec_has_executable_acs(cwd, spec);

    if qa_required {
        // Surface as "awaiting close": advance `stage → QaReview` (forward-only),
        // keeping `outcome = Active` and `phase = CLOSE`. The real finalize is
        // `/close` after QA passes.
        let advance = match stage {
            None => true,
            Some(s) => stage_rank(s) < stage_rank(Stage::QaReview),
        };
        if !advance {
            return;
        }
        let mut meta = meta;
        meta.stage = Some(stage_label(Stage::QaReview).to_string());
        meta.phase = Some("CLOSE".to_string());
        meta.checkpoint = Some(ts.to_string());
        if let Err(e) = write_meta(&path, &meta) {
            eprintln!(
                "emit-pipeline: WARN: could not write {} ({e}); parent awaiting-close stage may be stale",
                path.display()
            );
        }
    } else {
        // No QA owed → finalize exactly like `complete-spec`.
        patch_meta_complete(cwd, spec, ts);
        emit_pipeline_complete(cwd, spec, ts);
        emit_completed_status_if_needed(cwd, spec, ts, &session_id());
    }
}

/// Route a `pipeline.complete` audit event for `spec`, matching
/// [`crate::commands::spec::complete_spec`]'s emit: the payload carries
/// `closedAt` + the affected-file set (union of harness `target.file` events and
/// the VCS diff), so the events log / dashboard / `verify_emit` all see the
/// close. Best-effort — the route write is fire-and-forget.
fn emit_pipeline_complete(cwd: &Path, spec: &str, ts: &str) {
    let affected = crate::commands::spec::complete_spec::collect_affected_files(cwd, spec);
    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: ts.to_string(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("emit-pipeline".to_string()),
            actor_type: None,
        },
        event: EVENT_PIPELINE_COMPLETE.to_string(),
        payload: json!({ "closedAt": ts, "affectedFiles": affected }),
        spec: Some(spec.to_string()),
    };
    let _ = crate::shared::events::route::emit(&cwd.to_string_lossy(), &event);
}


// --- Auto-branch name computation (porta-unica) ------------------------------

/// Resolve the effective integration base for the auto-branch prefix: the
/// caller-supplied `--base` when it names one of the project's integration
/// bases (`config.git.integration_bases()`), else the project's primary base
/// (`config.git.primary_base()`).
///
/// Agnostic — both the accepted set and the fallback come from `git.flow`; no
/// branch name is hardcoded here. Do NOT re-derive the base set ad hoc: the
/// core owns that derivation so `work_branch_gate` and this emitter agree.
fn resolve_base(requested: Option<&str>, config: &mustard_core::ProjectConfig) -> String {
    let bases = config.git.integration_bases();
    if let Some(b) = requested.map(str::trim).filter(|b| !b.is_empty()) {
        if bases.contains(b) {
            return b.to_string();
        }
    }
    config.git.primary_base()
}

/// Resolve the slug lang for the auto-branch from `mustard.json` — `lang`
/// (legacy) then `specLang`, defaulting to `pt-BR` (mirrors
/// [`mustard_core::ProjectConfig::i18n`] precedence). A branch is not
/// user-facing prose, but the slug helper still strips accents per-locale.
fn branch_lang(project: &str) -> String {
    let config = mustard_core::ProjectConfig::load(Path::new(project));
    config
        .lang
        .clone()
        .or(config.spec_lang.clone())
        .unwrap_or_else(|| "pt-BR".to_string())
}

/// A short, ref-safe fallback token from the session id. `unknown`/empty →
/// `work` so the branch always has a non-empty tail.
fn short_sid(sid: &str) -> String {
    let s = sid.trim();
    if s.is_empty() || s == "unknown" {
        return "work".to_string();
    }
    s.chars().take(8).collect()
}

/// Sanitise `{base}_{slug}` into a valid git ref: keep `[A-Za-z0-9-_./]`,
/// map everything else to `-`, collapse `..` runs (git forbids them), and trim
/// leading `-`/`.`/`/` and trailing `/`/`.`. Never empty — floors to `work`.
fn sanitize_git_ref(raw: &str) -> String {
    let mut out: String = raw
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '/' => ch,
            _ => '-',
        })
        .collect();
    while out.contains("..") {
        out = out.replace("..", "-");
    }
    let trimmed = out
        .trim_start_matches(|c| c == '-' || c == '.' || c == '/')
        .trim_end_matches(|c| c == '/' || c == '.');
    if trimmed.is_empty() {
        "work".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Compute the auto-branch name for a `pipeline.kind` work-type signal:
/// `{base}_{slug}`, sanitised to a valid git ref. The `{base}_` prefix records
/// the integration branch the work is cut from, so the gate (and `/git`) can
/// recover the PR-target from the name alone. Slug precedence:
/// 1. `--spec` when present (already a slug);
/// 2. else `--intent` slugified for the project's lang;
/// 3. else a date-based fallback (`YYYY-MM-DD` from the event `ts`) suffixed
///    with a short session id for uniqueness.
/// Never fails — every branch degrades to a valid ref.
fn compute_work_branch(
    base: &str,
    spec: &str,
    intent: Option<&str>,
    sid: &str,
    ts: &str,
    project: &str,
) -> String {
    let slug = if !spec.trim().is_empty() {
        spec.trim().to_string()
    } else if let Some(intent) = intent.map(str::trim).filter(|s| !s.is_empty()) {
        crate::commands::spec::spec_slug::for_lang(intent, &branch_lang(project))
    } else {
        // Date-based fallback from the shared event timestamp, plus a short
        // session id so two spec-less/intent-less runs on the same day differ.
        let date = ts.split('T').next().unwrap_or("").trim();
        if date.is_empty() {
            short_sid(sid)
        } else {
            format!("{date}-{}", short_sid(sid))
        }
    };
    sanitize_git_ref(&format!("{base}_{slug}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::model::event::SCHEMA_VERSION;
    use serde_json::json;
    use std::path::Path;
    use tempfile::tempdir;

    // -----------------------------------------------------------------------
    // Validation + payload parsing (unit-level, no store I/O)
    // -----------------------------------------------------------------------

    #[test]
    fn known_kinds_list_covers_legacy_and_new_kinds() {
        // 9 legacy + 1 legacy phase (alias-only) + 1 wave.start + 4 new
        // canonical + 3 hygiene + 1 economy (W2 mustard-unification) + 1
        // pipeline.kind (porta-unica work-type signal).
        assert_eq!(KNOWN_KINDS.len(), 20);
        // Legacy nine.
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_SCOPE));
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_STATUS));
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_TASK_DISPATCH));
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_TASK_COMPLETE));
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_WAVE_START));
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_WAVE_COMPLETE));
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_DISPATCH_FAILURE));
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_PAUSE));
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_RESUME_MODE));
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_COMPLETE));
        // Work-type signal (porta-unica).
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_KIND));
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
        // W2 economy kind.
        assert!(KNOWN_KINDS.contains(&EVENT_ECONOMY_OPERATION_INVOKED));
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

    /// Field bug (sialia, recurring): PowerShell single-quotes preserve the
    /// bash-style `\"` escaping literally, so `--payload '{\"wave\":1}'` reaches
    /// the binary as `{\"wave\":1}` and `serde_json` rejects it ("key must be a
    /// string at line 1 column 2"). The tolerant parser recovers it instead of
    /// forcing the orchestrator to re-emit.
    #[test]
    fn parse_payload_tolerant_recovers_powershell_escaped_json() {
        let ps = r#"{\"wave\":1,\"duration_ms\":536342}"#;
        let v = super::parse_payload_tolerant(ps).expect("recovers escaped payload");
        assert_eq!(v["wave"], json!(1));
        assert_eq!(v["duration_ms"], json!(536342));

        // A correctly-quoted payload parses on the first try (unchanged path).
        assert_eq!(super::parse_payload_tolerant(r#"{"wave":1}"#).unwrap()["wave"], json!(1));

        // Genuinely broken JSON (no `\"` artefact) still errors — no masking.
        assert!(super::parse_payload_tolerant("{not json").is_err());

        // A JSON string value that legitimately holds `\"` parses first try, so
        // the fallback never fires and the value is preserved exactly.
        let with_quote = r#"{"note":"she said \"hi\""}"#;
        let decoded = super::parse_payload_tolerant(with_quote).expect("valid escaped string");
        assert_eq!(decoded["note"], json!("she said \"hi\""));
    }

    // -----------------------------------------------------------------------
    // Auto-branch name computation (porta-unica)
    // -----------------------------------------------------------------------

    #[test]
    fn compute_work_branch_prefers_spec_slug_off_primary_base() {
        // base = the primary/`*` base → `{base}_{slug}`, kind dropped from name.
        let b = super::compute_work_branch("dev", "2026-07-02-my-spec", None, "sess-abcdef12", "2026-07-02T10:00:00.000Z", "/no/project");
        assert_eq!(b, "dev_2026-07-02-my-spec");
        // Task example.
        let b2 = super::compute_work_branch("dev", "parcelas-virtuais", None, "sess-abcdef12", "2026-07-02T10:00:00.000Z", "/no/project");
        assert_eq!(b2, "dev_parcelas-virtuais");
    }

    #[test]
    fn compute_work_branch_off_non_primary_base() {
        // base = a non-primary integration base (e.g. `main`) → prefix records it.
        let b = super::compute_work_branch("main", "close-gate-windows", None, "sess-abcdef12", "2026-07-02T10:00:00.000Z", "/no/project");
        assert_eq!(b, "main_close-gate-windows");
    }

    #[test]
    fn compute_work_branch_falls_back_to_intent_slug() {
        // No spec → the intent is slugified (pt-BR strips accents by default).
        let b = super::compute_work_branch("main", "", Some("Corrigir botão de login"), "sess-abcdef12", "2026-07-02T10:00:00.000Z", "/no/project");
        assert_eq!(b, "main_corrigir-botao-login");
    }

    #[test]
    fn compute_work_branch_date_fallback_when_no_spec_or_intent() {
        // No spec, no intent → date-from-ts + short session id.
        let b = super::compute_work_branch("dev", "", None, "sess-abcdef1234", "2026-07-02T10:00:00.000Z", "/no/project");
        assert_eq!(b, "dev_2026-07-02-sess-abc");
    }

    #[test]
    fn compute_work_branch_sanitizes_unsafe_slug() {
        // A spec with unsafe chars is sanitised into a valid ref.
        let b = super::compute_work_branch("dev", "weird ..slug/", None, "unknown", "2026-07-02T10:00:00.000Z", "/no/project");
        // ".." collapsed, spaces mapped to '-', trailing '/' trimmed.
        assert_eq!(b, "dev_weird--slug");
        assert!(!b.contains(".."), "no `..` runs in a git ref");
        assert!(!b.starts_with('-'), "no leading dash");
    }

    #[test]
    fn resolve_base_honours_requested_when_in_bases_else_primary() {
        // Standard two-tier flow → integration bases {dev, main}, primary = dev.
        let mut config = mustard_core::ProjectConfig::default();
        config.git.flow.insert("*".to_string(), "dev".to_string());
        config.git.flow.insert("dev".to_string(), "main".to_string());
        // A requested base that IS an integration base is used verbatim.
        assert_eq!(super::resolve_base(Some("main"), &config), "main");
        assert_eq!(super::resolve_base(Some("dev"), &config), "dev");
        // A requested base that is NOT an integration base → primary (flow["*"]).
        assert_eq!(super::resolve_base(Some("feature/x"), &config), "dev");
        // No request → primary.
        assert_eq!(super::resolve_base(None, &config), "dev");

        // Agnostic: a develop/master project resolves against ITS bases.
        let mut dm = mustard_core::ProjectConfig::default();
        dm.git.flow.insert("*".to_string(), "develop".to_string());
        dm.git.flow.insert("develop".to_string(), "master".to_string());
        assert_eq!(super::resolve_base(Some("master"), &dm), "master");
        assert_eq!(super::resolve_base(Some("dev"), &dm), "develop", "unknown base → primary");
    }

    // -----------------------------------------------------------------------
    // NDJSON integration — all events land in per-spec `.events/` dirs.
    // -----------------------------------------------------------------------

    /// Route one event through the event-router (the same path `run()` takes).
    /// All events land in the per-spec NDJSON `.events/` directory.
    fn emit_routed(project: &Path, kind: &str, spec: &str, payload: Value) {
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
        crate::shared::events::route::emit(project.to_str().unwrap(), &event);
    }

    #[test]
    fn each_kind_appended_once_with_correct_event_name() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let spec = "2026-05-20-pipeline-state-ndjson";

        for &kind in KNOWN_KINDS {
            emit_routed(project, kind, spec, json!({"test": true}));
        }

        // All events land in the per-spec NDJSON `.events/` directory.
        let events_dir = project.join(".claude").join("spec").join(spec).join(".events");
        let mut events = mustard_core::view::projection::read_harness_events_from_ndjson_dir(&events_dir);
        // Filter the auto-emitted economy.event.written sidecars (T5.8) — they
        // are not first-class kinds, just per-write breadcrumbs.
        events.retain(|e| e.event != "pipeline.economy.event.written");

        let counts: std::collections::BTreeMap<&str, usize> = KNOWN_KINDS
            .iter()
            .map(|k| (*k, events.iter().filter(|e| e.event == *k).count()))
            .collect();

        for &kind in KNOWN_KINDS {
            assert_eq!(
                counts.get(kind).copied(),
                Some(1),
                "expected exactly one event for kind {kind}; counts: {counts:?}"
            );
        }
    }

    #[test]
    fn pipeline_scope_payload_round_trips() {
        use mustard_core::domain::model::event::PipelineScopePayload;

        let dir = tempdir().unwrap();
        let spec = "demo-scope";
        let payload_struct = PipelineScopePayload {
            scope: "full".to_string(),
            lang: Some("en".to_string()),
            model: Some("opus".to_string()),
            is_wave_plan: Some(true),
            total_waves: Some(6),
        };
        let payload_value = serde_json::to_value(&payload_struct).unwrap();
        emit_routed(dir.path(), EVENT_PIPELINE_SCOPE, spec, payload_value);

        let events_dir = dir.path().join(".claude").join("spec").join(spec).join(".events");
        let mut events = mustard_core::view::projection::read_harness_events_from_ndjson_dir(&events_dir);
        events.retain(|e| e.event == EVENT_PIPELINE_SCOPE);
        assert_eq!(events.len(), 1);
        let decoded: PipelineScopePayload =
            serde_json::from_value(events[0].payload.clone()).unwrap();
        assert_eq!(decoded.scope, "full");
        assert_eq!(decoded.model.as_deref(), Some("opus"));
        assert_eq!(decoded.total_waves, Some(6));
    }

    #[test]
    fn pipeline_task_complete_payload_round_trips() {
        use mustard_core::domain::model::event::PipelineTaskCompletePayload;

        let dir = tempdir().unwrap();
        let spec = "demo-task";
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
        emit_routed(dir.path(), EVENT_PIPELINE_TASK_COMPLETE, spec, payload_value);

        let events_dir = dir.path().join(".claude").join("spec").join(spec).join(".events");
        let mut events = mustard_core::view::projection::read_harness_events_from_ndjson_dir(&events_dir);
        events.retain(|e| e.event == EVENT_PIPELINE_TASK_COMPLETE);
        assert_eq!(events.len(), 1);
        let decoded: PipelineTaskCompletePayload =
            serde_json::from_value(events[0].payload.clone()).unwrap();
        assert_eq!(decoded.wave, Some(3));
        assert_eq!(decoded.duration_ms, Some(45_000));
        assert!(decoded.escalation.is_none());
    }

    #[test]
    fn optional_fields_absent_in_minimal_payload() {
        use mustard_core::domain::model::event::PipelineStatusPayload;

        // Only required fields: `to`. `from` is absent in JSON.
        let raw = r#"{"to":"active"}"#;
        let decoded: PipelineStatusPayload = serde_json::from_str(raw).unwrap();
        assert_eq!(decoded.to, "active");
        assert!(decoded.from.is_none());
    }

    // -----------------------------------------------------------------------
    // REVIEW/QA gate on `pipeline.complete` (2026-05-25 deep-refactor follow-up)
    // -----------------------------------------------------------------------

    /// `qa_result_passed` returns `false` when the spec has no `.events/` dir
    /// — the gate must stay closed (block emission).
    #[test]
    fn qa_result_passed_false_when_no_events_dir() {
        let dir = tempdir().unwrap();
        // Spec dir does not even exist.
        assert!(!super::qa_result_passed(dir.path(), "ghost-spec"));
    }

    /// `qa_result_passed` returns `true` only when the most recent `qa.result`
    /// for the spec has `overall == "pass"`.
    #[test]
    fn qa_result_passed_requires_overall_pass() {
        let dir = tempdir().unwrap();
        let spec = "qa-gate-spec";
        // Emit a failing qa.result first, then a passing one.
        emit_routed(
            dir.path(),
            "qa.result",
            spec,
            json!({ "spec": spec, "overall": "fail", "criteria": [] }),
        );
        emit_routed(
            dir.path(),
            "qa.result",
            spec,
            json!({ "spec": spec, "overall": "pass", "criteria": [] }),
        );
        assert!(super::qa_result_passed(dir.path(), spec));
    }

    /// A failing-only spec → gate stays closed.
    #[test]
    fn qa_result_passed_false_when_only_fail() {
        let dir = tempdir().unwrap();
        let spec = "qa-fail-only";
        emit_routed(
            dir.path(),
            "qa.result",
            spec,
            json!({ "spec": spec, "overall": "fail", "criteria": [] }),
        );
        assert!(!super::qa_result_passed(dir.path(), spec));
    }

    /// A skip-only spec → gate stays closed (skip != pass).
    #[test]
    fn qa_result_passed_false_when_overall_skip() {
        let dir = tempdir().unwrap();
        let spec = "qa-skip-only";
        emit_routed(
            dir.path(),
            "qa.result",
            spec,
            json!({ "spec": spec, "overall": "skip", "criteria": [] }),
        );
        assert!(!super::qa_result_passed(dir.path(), spec));
    }

    /// Last-write-wins: a passing event followed by a failing one means the
    /// most recent verdict is FAIL → gate stays closed.
    #[test]
    fn qa_result_passed_uses_most_recent_event() {
        let dir = tempdir().unwrap();
        let spec = "qa-regression";
        // First a pass with an early ts, then a fail with a later ts.
        let ev_pass = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-05-20T00:00:00.000Z".to_string(),
            session_id: "test-session".to_string(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Cli,
                id: Some("qa-run".to_string()),
                actor_type: None,
            },
            event: "qa.result".to_string(),
            payload: json!({ "spec": spec, "overall": "pass", "criteria": [] }),
            spec: Some(spec.to_string()),
        };
        let ev_fail = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-05-21T00:00:00.000Z".to_string(),
            session_id: "test-session".to_string(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Cli,
                id: Some("qa-run".to_string()),
                actor_type: None,
            },
            event: "qa.result".to_string(),
            payload: json!({ "spec": spec, "overall": "fail", "criteria": [] }),
            spec: Some(spec.to_string()),
        };
        let _ = crate::shared::events::route::emit(dir.path().to_str().unwrap(), &ev_pass);
        let _ = crate::shared::events::route::emit(dir.path().to_str().unwrap(), &ev_fail);
        assert!(!super::qa_result_passed(dir.path(), spec));
    }

    #[test]
    fn write_error_does_not_propagate_as_nonzero() {
        // Confirm the fail-open design: a legitimate emit writes one event to
        // the NDJSON sink and the file is readable afterward (regression guard).
        let dir = tempdir().unwrap();
        let spec = "demo-failopen";
        emit_routed(dir.path(), EVENT_PIPELINE_PAUSE, spec, json!({"reason": "user request"}));
        let events_dir = dir.path().join(".claude").join("spec").join(spec).join(".events");
        let mut events = mustard_core::view::projection::read_harness_events_from_ndjson_dir(&events_dir);
        events.retain(|e| e.event == EVENT_PIPELINE_PAUSE);
        assert_eq!(events.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Tactical-fix 2026-05-26: pipeline.wave.complete drives meta-sync
    //
    // `sync_wave_meta_sidecar` was inlined into `spec_scaffold::sync_status`
    // during the W2-residuals sweep; the wave-meta write is now exercised
    // through the higher-level `bump_parent_progress` regression below + the
    // end-to-end projection tests in `tests/pipeline_state_projection_test.rs`.
    // -----------------------------------------------------------------------

    /// `bump_parent_progress` sets `currentWave` + extends `completedWaves`
    /// (dedupe + sort) and picks `EXECUTE` vs `CLOSE` based on `totalWaves`.
    #[test]
    fn wave_complete_bumps_parent_progress() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join(".claude").join("spec").join("foo");
        std::fs::create_dir_all(&spec_dir).unwrap();
        let meta_path = spec_dir.join("meta.json");
        // Parent meta with totalWaves=4, isWavePlan=true, no progress yet.
        std::fs::write(
            &meta_path,
            br#"{"stage":"Execute","outcome":"Active","phase":"PLAN","scope":"full","lang":"pt-BR","checkpoint":null,"isWavePlan":true,"totalWaves":4}"#,
        )
        .unwrap();

        let ts1 = "2026-05-26T00:00:00Z";
        super::bump_parent_progress(dir.path(), "foo", 1, ts1);

        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(&meta_path).unwrap()).unwrap();
        assert_eq!(v["phase"], json!("EXECUTE"), "{v}");
        assert_eq!(v["currentWave"], json!(1), "{v}");
        assert_eq!(v["completedWaves"], json!([1]), "{v}");
        assert_eq!(v["checkpoint"], json!(ts1), "{v}");

        // Second call with the terminal wave (4 of 4). Expect:
        //   phase = CLOSE
        //   currentWave = 4
        //   completedWaves = [1, 4] (dedup + sort preserved)
        let ts2 = "2026-05-26T01:00:00Z";
        super::bump_parent_progress(dir.path(), "foo", 4, ts2);

        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(&meta_path).unwrap()).unwrap();
        assert_eq!(v["phase"], json!("CLOSE"), "{v}");
        assert_eq!(v["currentWave"], json!(4), "{v}");
        assert_eq!(v["completedWaves"], json!([1, 4]), "{v}");
        assert_eq!(v["checkpoint"], json!(ts2), "{v}");

        // Third call with a repeat (wave=1) keeps completedWaves deduped.
        super::bump_parent_progress(dir.path(), "foo", 1, "2026-05-26T02:00:00Z");
        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(&meta_path).unwrap()).unwrap();
        assert_eq!(v["completedWaves"], json!([1, 4]), "{v}");
    }

    /// Regression (2026-06-26): `reconcile_wave_checklist` marks `done` for items
    /// whose target file exists on disk and leaves the rest — the deterministic
    /// backfill for the auto-mark's live misses (a wave closing with unchecked
    /// items whose files clearly exist).
    #[test]
    fn reconcile_wave_checklist_marks_existing_files_only() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        let wave_dir = cwd.join(".claude").join("spec").join("s").join("wave-1-rt");
        std::fs::create_dir_all(&wave_dir).unwrap();
        std::fs::create_dir_all(cwd.join("src")).unwrap();
        std::fs::write(cwd.join("src").join("done.rs"), b"x").unwrap();
        std::fs::write(
            wave_dir.join("meta.json"),
            br#"{"stage":"Execute","outcome":"Active","checklist":[{"label":"src/done.rs","path":"src/done.rs","done":false},{"label":"src/missing.rs","path":"src/missing.rs","done":false}]}"#,
        )
        .unwrap();

        super::reconcile_wave_checklist(cwd, &wave_dir);

        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(wave_dir.join("meta.json")).unwrap())
                .unwrap();
        assert_eq!(v["checklist"][0]["done"], json!(true), "existing file marked: {v}");
        assert_eq!(v["checklist"][1]["done"], json!(false), "missing file untouched: {v}");
    }

    /// DEFECT 1 (2026-06-05): an EXECUTE-branch `bump_parent_progress` advances
    /// the native `stage` from `Plan` to `Execute` (forward-only) so the
    /// dashboard stops rendering PLANEJANDO during wave execution.
    #[test]
    fn wave_complete_advances_parent_stage_to_execute() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join(".claude").join("spec").join("foo");
        std::fs::create_dir_all(&spec_dir).unwrap();
        let meta_path = spec_dir.join("meta.json");
        // Parent stuck at stage=Plan with an interior wave (totalWaves=3) — the
        // exact live-confirmed bad state: phase advances, stage does not.
        std::fs::write(
            &meta_path,
            br#"{"stage":"Plan","outcome":"Active","phase":"PLAN","scope":"full","lang":"pt-BR","checkpoint":null,"isWavePlan":true,"totalWaves":3}"#,
        )
        .unwrap();

        super::bump_parent_progress(dir.path(), "foo", 1, "2026-06-05T00:00:00Z");

        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(&meta_path).unwrap()).unwrap();
        assert_eq!(v["phase"], json!("EXECUTE"), "{v}");
        assert_eq!(v["stage"], json!("Execute"), "phase+stage agree: {v}");
        assert_eq!(v["outcome"], json!("Active"), "outcome untouched: {v}");
    }

    /// DEFECT 1: a stage already at `QaReview` is NOT regressed to `Execute` by
    /// an interior wave.complete (forward-only guard).
    #[test]
    fn wave_complete_does_not_regress_later_stage() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join(".claude").join("spec").join("bar");
        std::fs::create_dir_all(&spec_dir).unwrap();
        let meta_path = spec_dir.join("meta.json");
        // A later wave already drove the parent to QaReview; a straggling
        // wave.complete must not pull it back to Execute.
        std::fs::write(
            &meta_path,
            br#"{"stage":"QaReview","outcome":"Active","phase":"QA","scope":"full","lang":"pt-BR","checkpoint":null,"isWavePlan":true,"totalWaves":5}"#,
        )
        .unwrap();

        super::bump_parent_progress(dir.path(), "bar", 2, "2026-06-05T01:00:00Z");

        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(&meta_path).unwrap()).unwrap();
        // phase still tracks the interior wave (advisory), but stage stays QaReview.
        assert_eq!(v["phase"], json!("EXECUTE"), "{v}");
        assert_eq!(v["stage"], json!("QaReview"), "stage not regressed: {v}");
    }

    /// FINAL-WAVE AUTO-SETTLE — no acceptance criteria (the case `qa-run` would
    /// `skip`): the last `wave.complete` auto-finalizes the parent exactly like
    /// `complete-spec` (`stage=Close, outcome=Completed, phase=CLOSE`) and lands
    /// a `pipeline.complete` event, while preserving the progress writes.
    #[test]
    fn final_wave_auto_finalizes_when_no_acceptance_criteria() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join(".claude").join("spec").join("no-ac");
        std::fs::create_dir_all(&spec_dir).unwrap();
        // spec.md WITHOUT a `## Acceptance Criteria` section → qa-run would skip,
        // so the spec owes no QA pass and can finalize on the final wave.
        std::fs::write(spec_dir.join("spec.md"), b"# No AC\n\nNarrative only.\n").unwrap();
        std::fs::write(
            spec_dir.join("meta.json"),
            br#"{"stage":"Execute","outcome":"Active","phase":"EXECUTE","scope":"full","lang":"pt-BR","isWavePlan":true,"totalWaves":2}"#,
        )
        .unwrap();

        // Final wave (2 of 2) → phase CLOSE → auto-settle.
        super::bump_parent_progress(dir.path(), "no-ac", 2, "2026-07-02T00:00:00Z");

        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(spec_dir.join("meta.json")).unwrap())
                .unwrap();
        assert_eq!(v["stage"], json!("Close"), "auto-finalized to Close: {v}");
        assert_eq!(v["outcome"], json!("Completed"), "outcome Completed: {v}");
        assert_eq!(v["phase"], json!("CLOSE"), "{v}");
        // Progress writes survive the finalize (patch_meta_complete preserves raw).
        assert_eq!(v["currentWave"], json!(2), "{v}");
        assert_eq!(v["completedWaves"], json!([2]), "{v}");

        // The pipeline.complete audit event landed in the per-spec NDJSON sink.
        let events_dir = spec_dir.join(".events");
        let events =
            mustard_core::view::projection::read_harness_events_from_ndjson_dir(&events_dir);
        assert!(
            events.iter().any(|e| e.event == EVENT_PIPELINE_COMPLETE),
            "pipeline.complete must be emitted on auto-finalize",
        );
    }

    /// FINAL-WAVE AUTO-SETTLE — acceptance criteria present + strict QA gate
    /// (the default): the last `wave.complete` must NOT finalize. It advances the
    /// parent to `stage=QaReview` (outcome `Active`, phase `CLOSE`) so it surfaces
    /// as "awaiting close"; no `pipeline.complete` is emitted — `/close` owns the
    /// real finalize after QA passes.
    #[test]
    fn final_wave_awaits_close_when_acceptance_criteria_present() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join(".claude").join("spec").join("with-ac");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("spec.md"),
            b"# With AC\n\n## Acceptance Criteria\n- [ ] AC-1: builds. Command: `true`\n",
        )
        .unwrap();
        std::fs::write(
            spec_dir.join("meta.json"),
            br#"{"stage":"Execute","outcome":"Active","phase":"EXECUTE","scope":"full","lang":"pt-BR","isWavePlan":true,"totalWaves":2}"#,
        )
        .unwrap();

        super::bump_parent_progress(dir.path(), "with-ac", 2, "2026-07-02T01:00:00Z");

        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(spec_dir.join("meta.json")).unwrap())
                .unwrap();
        assert_eq!(v["stage"], json!("QaReview"), "awaits QA/close, not finalized: {v}");
        assert_eq!(v["outcome"], json!("Active"), "stays Active until /close: {v}");
        assert_eq!(v["phase"], json!("CLOSE"), "{v}");

        // NOT finalized → no pipeline.complete audit event.
        let events_dir = spec_dir.join(".events");
        let events =
            mustard_core::view::projection::read_harness_events_from_ndjson_dir(&events_dir);
        assert!(
            !events.iter().any(|e| e.event == EVENT_PIPELINE_COMPLETE),
            "a QA-owing spec must not auto-emit pipeline.complete",
        );
    }

    // -----------------------------------------------------------------------
    // BUG 1 (2026-06-01): emit-pipeline patches meta.json on canonical state
    // transitions (pipeline.stage / pipeline.outcome / pipeline.complete).
    // -----------------------------------------------------------------------

    /// Seed a top-level spec dir with a `meta.json` and return both paths.
    fn seed_spec_meta(root: &Path, spec: &str, body: &str) -> std::path::PathBuf {
        let spec_dir = root.join(".claude").join("spec").join(spec);
        std::fs::create_dir_all(&spec_dir).unwrap();
        let meta_path = spec_dir.join("meta.json");
        std::fs::write(&meta_path, body.as_bytes()).unwrap();
        meta_path
    }

    /// AC-a: a `pipeline.stage {stage: "execute"}` event patches `meta.json`
    /// `stage` (+ `phase`), bumps `checkpoint`, and preserves other fields.
    #[test]
    fn stage_transition_patches_meta_stage() {
        let dir = tempdir().unwrap();
        let meta_path = seed_spec_meta(
            dir.path(),
            "demo",
            r#"{"stage":"Plan","outcome":"Active","phase":"PLAN","scope":"full","lang":"pt-BR","checkpoint":null}"#,
        );

        let ts = "2026-06-01T10:00:00Z";
        super::patch_meta_for_transition(
            dir.path(),
            "demo",
            EVENT_PIPELINE_STAGE,
            &json!({ "stage": "execute" }),
            ts,
        );

        let v: Value = serde_json::from_str(&std::fs::read_to_string(&meta_path).unwrap()).unwrap();
        assert_eq!(v["stage"], json!("Execute"), "{v}");
        assert_eq!(v["phase"], json!("EXECUTE"), "{v}");
        // Outcome stays Active through a stage move; other fields preserved.
        assert_eq!(v["outcome"], json!("Active"), "{v}");
        assert_eq!(v["scope"], json!("full"), "{v}");
        assert_eq!(v["lang"], json!("pt-BR"), "{v}");
        assert_eq!(v["checkpoint"], json!(ts), "{v}");
    }

    /// A `pipeline.outcome {outcome: "completed"}` event pins `stage = Close`
    /// + `phase = CLOSE` alongside the terminal outcome.
    #[test]
    fn outcome_transition_pins_close_on_terminal() {
        let dir = tempdir().unwrap();
        let meta_path = seed_spec_meta(
            dir.path(),
            "demo",
            r#"{"stage":"Execute","outcome":"Active","phase":"EXECUTE","scope":"full","lang":"en-US","checkpoint":null}"#,
        );

        super::patch_meta_for_transition(
            dir.path(),
            "demo",
            EVENT_PIPELINE_OUTCOME,
            &json!({ "outcome": "completed" }),
            "2026-06-01T11:00:00Z",
        );

        let v: Value = serde_json::from_str(&std::fs::read_to_string(&meta_path).unwrap()).unwrap();
        assert_eq!(v["outcome"], json!("Completed"), "{v}");
        assert_eq!(v["stage"], json!("Close"), "{v}");
        assert_eq!(v["phase"], json!("CLOSE"), "{v}");
    }

    /// AC-b: `pipeline.complete` sets `outcome = Completed`, `stage = Close`,
    /// `phase = CLOSE` in `meta.json` and preserves scope/lang.
    #[test]
    fn complete_sets_outcome_completed_and_stage_close() {
        let dir = tempdir().unwrap();
        let meta_path = seed_spec_meta(
            dir.path(),
            "demo",
            r#"{"stage":"QaReview","outcome":"Active","phase":"QA","scope":"light","lang":"pt-BR","checkpoint":null}"#,
        );

        let ts = "2026-06-01T12:00:00Z";
        super::patch_meta_complete(dir.path(), "demo", ts);

        let v: Value = serde_json::from_str(&std::fs::read_to_string(&meta_path).unwrap()).unwrap();
        assert_eq!(v["outcome"], json!("Completed"), "{v}");
        assert_eq!(v["stage"], json!("Close"), "{v}");
        assert_eq!(v["phase"], json!("CLOSE"), "{v}");
        assert_eq!(v["scope"], json!("light"), "{v}");
        assert_eq!(v["lang"], json!("pt-BR"), "{v}");
        assert_eq!(v["checkpoint"], json!(ts), "{v}");
    }

    /// Fail-open: a missing spec directory is a silent no-op (no panic, no
    /// created file).
    #[test]
    fn patch_meta_complete_noop_when_spec_missing() {
        let dir = tempdir().unwrap();
        super::patch_meta_complete(dir.path(), "ghost", "2026-06-01T12:00:00Z");
        assert!(!dir.path().join(".claude").join("spec").join("ghost").exists());
    }

    /// Helper: project status for `spec` from its per-spec NDJSON window.
    fn projected_status(project: &Path, spec: &str) -> Option<String> {
        let events_dir = project.join(".claude").join("spec").join(spec).join(".events");
        let events =
            mustard_core::view::projection::read_harness_events_from_ndjson_dir(&events_dir);
        crate::commands::event::event_projections::pipeline_state_from_events(&events, spec, None)
            .and_then(|v| v.status)
    }

    /// Run-face consistency (the `emit_pipeline.rs:306` fix): when
    /// `pipeline.complete` is handled it ALSO emits `pipeline.status: completed`
    /// so the event projection agrees with the meta sidecar. Here the spec is
    /// mid-pipeline (status `implementing`), so the terminal status is emitted
    /// and the projection ends on `completed`.
    #[test]
    fn complete_also_emits_completed_status_when_not_terminal() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let spec = "demo-runface";
        // Seed a non-terminal status so the projection starts mid-pipeline.
        emit_routed(project, EVENT_PIPELINE_STATUS, spec, json!({ "to": "implementing" }));
        assert_eq!(projected_status(project, spec).as_deref(), Some("implementing"));

        super::emit_completed_status_if_needed(project, spec, "2026-06-04T00:00:00Z", "sid");
        assert_eq!(
            projected_status(project, spec).as_deref(),
            Some("completed"),
            "run-face pipeline.complete must drive the projection to completed",
        );
    }

    /// Idempotent: a spec already projected `completed` does not get a duplicate
    /// terminal status flip (mirrors the `mark_complete` short-circuit).
    #[test]
    fn complete_status_emit_is_idempotent_when_already_completed() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let spec = "demo-runface-idem";
        emit_routed(project, EVENT_PIPELINE_STATUS, spec, json!({ "to": "completed" }));

        let before = {
            let events_dir = project.join(".claude").join("spec").join(spec).join(".events");
            mustard_core::view::projection::read_harness_events_from_ndjson_dir(&events_dir)
                .iter()
                .filter(|e| e.event == EVENT_PIPELINE_STATUS)
                .count()
        };
        super::emit_completed_status_if_needed(project, spec, "2026-06-04T00:00:00Z", "sid");
        let after = {
            let events_dir = project.join(".claude").join("spec").join(spec).join(".events");
            mustard_core::view::projection::read_harness_events_from_ndjson_dir(&events_dir)
                .iter()
                .filter(|e| e.event == EVENT_PIPELINE_STATUS)
                .count()
        };
        assert_eq!(before, after, "no duplicate pipeline.status when already completed");
    }
}
