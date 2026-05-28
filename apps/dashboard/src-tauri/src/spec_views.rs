//! Wave 1a (2026-05-20, spec `dashboard-visual-overview`) — three new
//! aggregations live at the bottom of this file (`dashboard_token_summary`,
//! `dashboard_month_activity`, `dashboard_events_feed`). They read the
//! `events` table directly via `db::with_db` and follow the fail-open
//! contract of the rest of the module (missing DB → empty payload).
//!
//! `*_v2` adapter family that delegates to `mustard-core`.
//!
//! Each `*_v2` function is a thin adapter — it walks
//! `<project>/.claude/spec/*/.events/*.ndjson` via
//! [`mustard_core::view::projection::read_workspace_events`], folds the resulting
//! slice with the matching projection function (`project_spec_view_with_header`,
//! `project_waves`, `project_quality`, `project_timeline`, `project_workspace`)
//! and maps the typed ViewModel into the JSON shape the frontend already
//! expects (so React contracts stay untouched). The legacy hand-rolled SQL functions
//! (`spec_card`, `spec_waves`, `spec_quality`, `spec_timeline`,
//! `workspace_summary`) were removed in Wave 2 of spec
//! `2026-05-20-sdd-domain-finalization`; the Tauri commands in `lib.rs`
//! already delegated to the `*_v2` adapters since Wave 4 of the audit.

// Wave 6A of [[2026-05-26-no-sqlite-git-source-of-truth]]: the SQLite reader
// was retired and `crate::db` now exposes only an opaque [`Connection`]
// placeholder. Functions in this module preserve their `&Connection` signature
// so call sites in `lib.rs` compile unchanged, but the bodies of the
// SQL-backed aggregations have been stubbed to fail-open defaults — the
// closures are never actually invoked since `db::with_db` returns `None`.
use crate::db::Connection;
use mustard_core::io::fs;
use serde::{Deserialize, Serialize};

// ── Shapes ───────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct SpecCard {
    pub spec: String,
    pub status: String,
    pub phase: String,
    pub scope: Option<String>,
    pub started_at: Option<String>,
    pub last_event_at: Option<String>,
    pub duration_ms: Option<i64>,
    pub current_wave: Option<i64>,
    pub total_waves: Option<i64>,
    pub ac_passed: i64,
    pub ac_total: i64,
    pub files_touched: i64,
    pub tools_used: i64,
    pub model: Option<String>,
    /// Sub-spec count derived from `spec.link` events with this spec as
    /// parent. Lets the dashboard render the `+N sub-specs` badge without
    /// fanning out one `useSpecChildren` query per rendered card (spec
    /// `2026-05-21-speccard-use-children-count`). Serde default = 0 keeps
    /// older clients/payloads compatible.
    #[serde(default)]
    pub children_count: u32,
}

/// Wave-3 (2026-05-20, spec `2026-05-20-tactical-fix-via-sub-spec`) — one
/// sub-spec linked to a parent via the `spec.link` event. Mirrors the JSON
/// shape consumed by the dashboard's "Sub-specs" tab; the Rust source of
/// truth is [`mustard_core::SpecChild`].
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct SpecChild {
    pub spec: String,
    /// kebab-case lifecycle status, mirroring the rest of the `*_v2` adapters.
    pub status: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub reason: Option<String>,
    /// Wave-6 (2026-05-21, spec `2026-05-21-dashboard-spec-tabs`): provenance of
    /// this child entry. `"event"` = found only in the SQLite `spec.link`
    /// projection; `"header"` = found only via the filesystem `### Parent:`
    /// header scan; `"both"` = present in both. Optional + serde default
    /// keeps older payloads (and the legacy SQLite-only path) compatible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Wave 2 (2026-05-21, spec `2026-05-21-dashboard-spec-tabs-polish`):
    /// the parent wave whose execution window contains this child's
    /// `started_at`. `None` when the child has no `started_at` or its start
    /// falls outside every wave window. The dashboard renders sub-specs
    /// nested under the matching wave row in the Ondas tab; rows with
    /// `wave == None` land in the "Sem onda correlacionada" bucket.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wave: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct SpecWave {
    pub wave: i64,
    pub role: Option<String>,
    pub status: String, // queued | in_progress | completed | failed
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub agent_type: Option<String>,
    pub files_changed: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct SpecQualityItem {
    pub ac_id: String,
    pub ac_label: Option<String>,
    pub status: String, // pass | fail | skip
    pub wave: Option<i64>,
    pub command: Option<String>,
    pub last_run_at: Option<String>,
    pub fail_reason: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct SpecTimelineNode {
    pub ts: String,
    pub kind: String, // phase | wave | qa | review | agent | tool
    pub label: String,
    pub phase: Option<String>,
    pub wave: Option<i64>,
    pub payload_summary: Option<String>,
}

/// Filter parameters for spec_events.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "snake_case")]
pub struct EventFilter {
    pub kinds: Option<Vec<String>>,
    pub wave: Option<i64>,
    pub agent: Option<String>,
    pub q: Option<String>,
}

/// Mirrors `telemetry_agg::TimelineEvent` — reused for spec_events output.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct TimelineEvent {
    pub id: String,
    pub ts: String,
    pub phase: Option<String>,
    pub spec: Option<String>,
    pub agent: Option<String>,
    pub summary: String,
}

/// Action kind for spec_action.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub enum SpecActionKind {
    Reopen,
    Close,
    Remove,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct SpecAction {
    pub action: String,
    pub spec: String,
    pub result: String,
    pub message: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct PhaseSegment {
    pub phase: String, // analyze | plan | execute | qa | close
    pub state: String, // completed | active | future
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct SpecTrack {
    pub spec: String,
    pub status: String,
    pub current_phase: String,
    pub current_wave: Option<i64>,
    pub total_waves: Option<i64>,
    pub agents_active: i64,
    pub last_event_at: Option<String>,
    pub blocked_reason: Option<String>,
    pub segments: Vec<PhaseSegment>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct WorkspaceAlert {
    pub kind: String, // wave_failed | qa_fail
    pub spec: String,
    pub wave: Option<i64>,
    pub message: String,
    pub ts: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct FileCount {
    pub path: String,
    pub count: i64,
}

/// Wave-4 (2026-05-20) — JSON shape for `mustard-rt run metrics wave-status`.
/// Mirrors `apps/rt/src/run/metrics_wave_status.rs::WaveStatus` so the
/// dashboard can deserialise the subprocess stdout straight into a typed
/// struct instead of `serde_json::Value`. Optional fields (`status`, `model`)
/// stay `Option` because the rt side serialises with `skip_serializing_if`.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "snake_case")]
pub struct MetricsWaveRow {
    pub name: String,
    pub status: Option<String>,
    pub tokens_saved: i64,
    pub duration_ms: i64,
    pub retries: i64,
    pub cross_wave_memory_bytes: i64,
    pub model: Option<String>,
}

/// Result of `dashboard_metrics_wave_status` — parent name plus per-wave rows.
/// Empty `waves` vec when the spec has no wave-plan or the subprocess fails;
/// the dashboard renders an empty state in that case.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "snake_case")]
pub struct MetricsWaveStatus {
    pub parent: Option<String>,
    pub waves: Vec<MetricsWaveRow>,
}

/// Wave-3 (2026-05-20, spec `mustard-wave-network-standard`) — one wikilink
/// occurrence emitted by `mustard-rt run wikilink-extract`. Mirrors the JSON
/// shape `{from, to, file, line}`.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct Wikilink {
    pub from: String,
    pub to: String,
    pub file: String,
    pub line: u32,
}

/// Wave-3 — full payload of `mustard-rt run wikilink-extract`: every wikilink
/// occurrence plus the list of orphan targets (referenced names that have no
/// resolvable spec file). The dashboard groups these into parent/waves/dependents
/// layers client-side.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "snake_case")]
pub struct WikilinkExtract {
    pub wikilinks: Vec<Wikilink>,
    pub orphans: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct WorkspaceSummary {
    pub events_per_minute: f64,
    /// `None` when the underlying projection has no token-savings data
    /// (e.g. RTK absent, no `rtk.savings` events emitted). The frontend
    /// renders "—" for `null` instead of silently presenting "0".
    pub tokens_saved_today: Option<i64>,
    pub specs_active_count: i64,
    pub spec_tracks: Vec<SpecTrack>,
    pub alerts: Vec<WorkspaceAlert>,
    pub top_files_today: Vec<FileCount>,
}

/// Wave-6 (2026-05-21, spec `spec-lifecycle-unification/wave-6-observability`) —
/// hygiene health roll-up for one project. Aggregates counts of active specs,
/// hygiene suspects (recent `hygiene.detected` events), auto-closures today,
/// and flag-bearing specs (`blocked`, `wave_failed`, `followup_open`).
///
/// Fail-open: any DB error → all-zeros struct so the card stays renderable.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "snake_case")]
pub struct WorkspaceHealth {
    /// Specs whose last `pipeline.status` outcome is `active`.
    pub active: i64,
    /// Distinct specs with a `hygiene.detected` event in the last 7 days that
    /// are still active (not yet closed by autoclose or user action).
    pub suspects: i64,
    /// `hygiene.autoclose` events in the last 24 hours.
    pub autoclose_today: i64,
    /// Active specs whose `SpecState.flags.blocked` is true.
    pub blocked: i64,
    /// Active specs whose `SpecState.flags.wave_failed` is true.
    pub wave_failed: i64,
    /// Active specs whose `SpecState.flags.followup_open` is true.
    pub followup_open: i64,
    /// ISO-8601 timestamp of the most recent `hygiene.*` event (nullable).
    pub last_hygiene_run_at: Option<String>,
    /// Slug list of suspect specs (distinct, for `/specs?filter=suspects` cross-reference).
    #[serde(default)]
    pub suspect_specs: Vec<String>,
}

/// Aggregate hygiene health data from the project's `mustard.db`.
///
/// Opens via `db::with_db` (the same helper used by all other aggregation
/// commands in this file). Fail-open: returns an all-zeros `WorkspaceHealth`
/// when the DB is absent, unreadable, or its schema is unexpected.
pub fn workspace_health_impl(_conn: &Connection) -> Result<WorkspaceHealth, String> {
    // Wave 6A no-sqlite stub: the SQLite event store was retired and
    // `db::with_db` never invokes this closure. Returns the fail-open
    // all-zeros struct the legacy aggregation produced on a missing DB.
    Ok(WorkspaceHealth::default())
}

// ── spec_events ───────────────────────────────────────────────────────────────

pub fn spec_events(
    _conn: &Connection,
    _spec: &str,
    _filter: Option<EventFilter>,
) -> Result<Vec<TimelineEvent>, String> {
    // Wave 6A no-sqlite stub: the SQLite-backed timeline join was retired;
    // closure never fires. Returns empty list per the fail-open contract.
    Ok(Vec::new())
}

// ── 6. spec_action ───────────────────────────────────────────────────────────

/// Wave-3 (2026-05-21-flatten-spec-layout-and-multi-collab): Close / Reopen
/// no longer move directories between `spec/active/` and `spec/completed/`.
/// The spec dir stays at `.claude/spec/{name}/` for its entire lifecycle;
/// the canonical status lives in the SQLite event store and in the
/// `### Status:` header of `spec.md` (kept in sync by
/// [`sync_spec_status_header`]).
///
/// `Close` emits `pipeline.status: completed`. `Reopen` emits
/// `pipeline.status: implementing` when prior events exist for this spec
/// (i.e. the pipeline already ran at least one EXECUTE wave), or
/// `pipeline.status: planning` when the store has no events for the spec
/// (treat as never-implemented). `Remove` deletes only `.claude/spec/{name}/`
/// — no multi-bucket search.
pub fn spec_action(
    _conn: &Connection,
    repo_path: &str,
    spec: &str,
    action: SpecActionKind,
) -> Result<SpecAction, String> {
    use std::path::Path;

    let spec_dir = Path::new(repo_path).join(".claude").join("spec").join(spec);

    match action {
        SpecActionKind::Reopen => {
            if !spec_dir.exists() {
                return Ok(SpecAction {
                    action:  "reopen".into(),
                    spec:    spec.into(),
                    result:  "error".into(),
                    message: Some("spec não encontrada".into()),
                });
            }
            let to = reopen_target_status(repo_path, spec);
            emit_pipeline_status(repo_path, spec, to);
            // Header sync is fail-open inside emit_pipeline_status; mirror
            // here so a stale store still gets a coherent on-disk header.
            sync_spec_status_header(repo_path, spec, to);
            Ok(SpecAction {
                action:  "reopen".into(),
                spec:    spec.into(),
                result:  "ok".into(),
                message: None,
            })
        }
        SpecActionKind::Close => {
            if !spec_dir.exists() {
                return Ok(SpecAction {
                    action:  "close".into(),
                    spec:    spec.into(),
                    result:  "error".into(),
                    message: Some("spec não encontrada".into()),
                });
            }
            emit_pipeline_status(repo_path, spec, "completed");
            sync_spec_status_header(repo_path, spec, "completed");
            Ok(SpecAction {
                action:  "close".into(),
                spec:    spec.into(),
                result:  "ok".into(),
                message: None,
            })
        }
        SpecActionKind::Remove => {
            if !spec_dir.exists() {
                return Ok(SpecAction {
                    action:  "remove".into(),
                    spec:    spec.into(),
                    result:  "error".into(),
                    message: Some("spec não encontrada".into()),
                });
            }
            fs::remove_dir_all(&spec_dir).map_err(|e| e.to_string())?;
            emit_pipeline_removed(repo_path, spec);
            Ok(SpecAction {
                action:  "remove".into(),
                spec:    spec.into(),
                result:  "ok".into(),
                message: None,
            })
        }
    }
}

/// Pick the `pipeline.status` value Reopen should emit. If the event store
/// already has events for this spec, the pipeline previously reached at least
/// EXECUTE — reopen back to `implementing`. Otherwise treat as a fresh spec
/// and emit `planning`. Fail-open: a missing/unwritable store falls back to
/// `implementing` (the historically expected value).
fn reopen_target_status(repo_path: &str, spec: &str) -> &'static str {
    // Wave 6A no-sqlite: the SQLite event log was retired. Fall back to the
    // filesystem signal — a spec dir with any prior NDJSON event under
    // `.claude/spec/{name}/.events/` means the pipeline already executed at
    // least one wave; reopen as `implementing`. An empty events directory
    // (or no spec dir at all) reopens as `planning`. Fail-open: any IO error
    // collapses to `implementing` (the historically expected default).
    let events_dir = std::path::Path::new(repo_path)
        .join(".claude")
        .join("spec")
        .join(spec)
        .join(".events");
    match std::fs::read_dir(&events_dir) {
        Ok(mut it) => {
            if it.next().is_some() {
                "implementing"
            } else {
                "planning"
            }
        }
        Err(_) => "implementing",
    }
}

// ── spec_action helpers ───────────────────────────────────────────────────────
//
// Wave 6A of `2026-05-26-no-sqlite-git-source-of-truth` retired the SQLite
// event store. `pipeline.status` / `pipeline.removed` are now emitted to the
// per-spec NDJSON sink via `crate::lib_emit_ndjson` (defined in `lib.rs`).
// A small fail-open header rewriter still mirrors
// `apps/rt/src/run/emit_pipeline.rs::sync_spec_status_header` so the
// canonical `### Status:` line in `spec.md` stays consistent with the event
// stream even when the dashboard does the writing.

/// Emit `pipeline.status: <to>` via the SQLite event store. Fail-open: any
/// error during store open / append is logged to stderr and swallowed —
/// telemetry is never load-bearing per the harness contract.
fn emit_pipeline_status(repo_path: &str, spec: &str, to: &str) {
    let payload = serde_json::json!({ "from": serde_json::Value::Null, "to": to });
    crate::lib_emit_ndjson(repo_path, spec, "pipeline.status", payload);
}

/// Emit `pipeline.removed` via the per-spec NDJSON sink. Fail-open mirror of
/// [`emit_pipeline_status`].
fn emit_pipeline_removed(repo_path: &str, spec: &str) {
    crate::lib_emit_ndjson(
        repo_path,
        spec,
        "pipeline.removed",
        serde_json::json!({ "removed": true }),
    );
}

/// Rewrite the `### Status:` line of `.claude/spec/{spec}/spec.md` to match
/// the freshly emitted `pipeline.status` value. Mirrors
/// `apps/rt/src/run/emit_pipeline.rs::sync_spec_status_header` — duplicated
/// here (15 lines) instead of importing because pulling `mustard-rt` into
/// the dashboard would create a workspace dependency cycle.
///
/// Fail-open contract: every failure path (missing file, missing header,
/// unwritable target) is a warn-and-return — the event has already been
/// recorded and is the authoritative source. We intentionally do NOT insert
/// a header when one is missing: that's a `spec.md` shape mutation and the
/// close-gate is the right place to enforce it.
fn sync_spec_status_header(repo_path: &str, spec: &str, to: &str) {
    let path = std::path::Path::new(repo_path)
        .join(".claude")
        .join("spec")
        .join(spec)
        .join("spec.md");

    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "sync_spec_status_header: read {} failed: {e}",
                path.display()
            );
            return;
        }
    };

    let mut lines: Vec<String> = content.lines().map(str::to_string).collect();
    let mut rewrote = false;
    for line in lines.iter_mut() {
        if line.trim_start().to_lowercase().starts_with("### status:") {
            *line = format!("### Status: {to}");
            rewrote = true;
            break;
        }
    }
    if !rewrote {
        eprintln!(
            "sync_spec_status_header: no `### Status:` header in {}",
            path.display()
        );
        return;
    }

    // Preserve trailing newline if the original had one.
    let mut out = lines.join("\n");
    if content.ends_with('\n') {
        out.push('\n');
    }
    if let Err(e) = fs::write_atomic(&path, out.as_bytes()) {
        eprintln!(
            "sync_spec_status_header: write {} failed: {e}",
            path.display()
        );
    }
}

// ===========================================================================
// Wave 4 adapters (2026-05-20) — `*_v2` family backed by `mustard-core`.
//
// These produce the *same* JSON shape as the legacy functions above (the
// shapes themselves did not move), but the projection layer is now the SDD
// domain crate. The Tauri commands in `lib.rs` call these — the legacy
// functions stay alongside until `spec_views_test.rs` is retired.
// ===========================================================================

/// W8A-2 adapter: build a [`SpecCard`] via `mustard-core` projections.
///
/// Walks the NDJSON workspace, folds the slice into a
/// [`mustard_core::SpecView`] via `project_spec_view_with_header` (so the
/// `spec.md` lifecycle header still seeds the view when the event stream is
/// empty), and maps the typed ViewModel into the JSON shape the React
/// frontend already consumes.
///
/// Returns `Ok(None)` when the projection is empty (no event evidence and no
/// usable header). The `lib.rs` command converts that to the empty-state
/// JSON payload.
pub fn spec_card_v2(repo_path: &str, spec: &str) -> Result<Option<SpecCard>, String> {
    let project = std::path::PathBuf::from(repo_path);
    let events = mustard_core::view::projection::read_workspace_events(&project);
    let spec_md = project.join(".claude").join("spec").join(spec).join("spec.md");
    let view = mustard_core::view::projection::project_spec_view_with_header(
        spec,
        &events,
        Some(&spec_md),
    );
    if view.is_empty() {
        return Ok(None);
    }
    // Spec `2026-05-21-speccard-use-children-count`: include the sub-spec
    // count up-front so the React card stops fanning out one
    // `useSpecChildren` query per rendered row. Re-fold the event slice on
    // `spec.link` payloads whose `parent` matches this spec.
    let children_count: u32 = events
        .iter()
        .filter(|e| e.event == "spec.link")
        .filter_map(|e| e.payload.get("parent").and_then(|p| p.as_str()))
        .filter(|p| *p == spec)
        .count()
        .try_into()
        .unwrap_or(u32::MAX);
    Ok(Some(spec_card_from_view(&view, children_count)))
}

/// W8A-2 adapter: build the wave list via `mustard-core` projections.
/// Empty `Vec` when the spec has no wave events.
pub fn spec_waves_v2(repo_path: &str, spec: &str) -> Result<Vec<SpecWave>, String> {
    let project = std::path::PathBuf::from(repo_path);
    let events = mustard_core::view::projection::read_workspace_events(&project);
    let waves = mustard_core::view::projection::project_waves(spec, &events);
    Ok(waves.iter().map(spec_wave_from_view).collect())
}

/// W8A-2 adapter: AC roll-up via `mustard-core` projections.
pub fn spec_quality_v2(repo_path: &str, spec: &str) -> Result<Vec<SpecQualityItem>, String> {
    let project = std::path::PathBuf::from(repo_path);
    let events = mustard_core::view::projection::read_workspace_events(&project);
    let rollup = mustard_core::view::projection::project_quality(spec, &events);
    Ok(rollup.criteria.iter().map(quality_item_from_view).collect())
}

/// W8A-2 adapter: timeline projection via `mustard-core` projections.
/// `All` window; the dashboard does its own client-side filtering when it
/// needs a narrower view.
pub fn spec_timeline_v2(repo_path: &str, spec: &str) -> Result<Vec<SpecTimelineNode>, String> {
    let project = std::path::PathBuf::from(repo_path);
    let events = mustard_core::view::projection::read_workspace_events(&project);
    let nodes = mustard_core::view::projection::project_timeline(
        spec,
        &events,
        mustard_core::TimeWindow::All,
    );
    Ok(nodes.iter().map(timeline_node_from_view).collect())
}

/// Wave-6 (2026-05-21, spec `2026-05-21-dashboard-spec-tabs`): list sub-specs
/// linked to `parent` via the **union** of two sources — the SQLite
/// `spec.link` projection AND a filesystem scan for `### Parent: <slug>` in
/// every `.claude/spec/*/spec.md` header. The union lives in `mustard-rt run
/// spec-children` so the Tauri command stays a thin subprocess wrapper (same
/// pattern as [`dashboard_spec_wave_files_run`] / [`dashboard_wikilink_extract_run`]).
///
/// Previously this delegated to a `SpecReader::children_of` projection that
/// queried only SQLite. Sub-specs created by `/mustard:tactical-fix`
/// (which write the `### Parent:` header at create time but don't always
/// have a `spec.link` event in the local store yet — e.g. another developer
/// pulled the files but not the SQLite db) were invisible.
///
/// Fail-open: a spawn failure surfaces as `Err` so the frontend can show
/// "mustard-rt not on PATH"; unparseable / empty stdout returns an empty
/// `Vec` so the panel renders the empty state.
pub fn spec_children_v2(repo_path: &str, parent: &str) -> Result<Vec<SpecChild>, String> {
    // Reject obvious traversal — `parent` is a single spec slug.
    if parent.is_empty()
        || parent.contains('/')
        || parent.contains('\\')
        || parent.contains("..")
    {
        return Err(format!("invalid parent name: {parent}"));
    }

    let mut cmd = mustard_rt_command(&["run", "spec-children", "--parent", parent]);
    cmd.current_dir(repo_path);
    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => return Err(format!("spawn mustard-rt: {e}")),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Output is a JSON array (`Vec<ChildEntryRaw>`). Trim any leading RTK
    // banner / log noise so the parse is robust — find the first `[` or `{`,
    // whichever comes first. `slice_json` (defined below) only handles `{`,
    // so we slice locally.
    let bracket = stdout.find('[');
    let brace = stdout.find('{');
    let json_start = match (bracket, brace) {
        (Some(b), Some(c)) => b.min(c),
        (Some(b), None) => b,
        (None, Some(c)) => c,
        (None, None) => return Ok(Vec::new()),
    };
    let json_slice = &stdout[json_start..];

    let entries: Vec<ChildEntryRaw> = match serde_json::from_str(json_slice) {
        Ok(v) => v,
        Err(_) => return Ok(Vec::new()),
    };
    Ok(entries
        .into_iter()
        .map(|e| SpecChild {
            spec: e.spec,
            status: e.status,
            started_at: e.started_at,
            completed_at: e.completed_at,
            reason: e.reason,
            source: Some(e.source),
            wave: e.wave,
        })
        .collect())
}

/// Raw row emitted by `mustard-rt run spec-children`. Kept private to this
/// module because the public payload toward the React side is [`SpecChild`].
#[derive(Deserialize)]
struct ChildEntryRaw {
    spec: String,
    status: String,
    #[serde(default)]
    started_at: Option<String>,
    #[serde(default)]
    completed_at: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    /// `"event" | "header" | "both"` — see [`SpecChild::source`].
    source: String,
    /// Wave 2 (spec `2026-05-21-dashboard-spec-tabs-polish`): parent wave
    /// number containing this child's `started_at`. Optional + serde default
    /// keeps older subprocess payloads (pre-Wave-2) compatible.
    #[serde(default)]
    wave: Option<u32>,
}

/// Wave 4 (2026-05-20, spec `mustard-wave-network-standard`) — invoke
/// `mustard-rt run metrics wave-status --spec <name>` and parse stdout into a
/// typed [`MetricsWaveStatus`]. Audit-2 in `metrics-audit.md` documents why
/// this exists; Audit-1 explains why the numbers may currently be all zeros
/// (writer/aggregator mismatch in `apps/rt/src/run/metrics_wave_status.rs`).
///
/// Subprocess invocation matches the project's existing convention:
/// `cmd /C mustard-rt ...` on Windows, `sh -c` elsewhere. The function never
/// returns an `Err` for "process failed" or "JSON garbage" — the dashboard
/// always gets *something* renderable (empty waves vec). The `Err` arm is
/// reserved for spawn errors so the frontend can show "mustard-rt not on
/// PATH" without crashing the page.
pub fn dashboard_metrics_wave_status_run(
    repo_path: &str,
    spec_name: &str,
) -> Result<MetricsWaveStatus, String> {
    // Reject obvious traversal — spec_name is a single directory under
    // .claude/spec/, never a path. Mirrors `dashboard_spec_markdown`.
    if spec_name.is_empty()
        || spec_name.contains('/')
        || spec_name.contains('\\')
        || spec_name.contains("..")
    {
        return Err(format!("invalid spec name: {spec_name}"));
    }

    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = crate::process_util::no_window_command("cmd");
        c.args([
            "/C",
            "mustard-rt",
            "run",
            "metrics",
            "wave-status",
            "--spec",
            spec_name,
        ]);
        c
    };
    #[cfg(not(target_os = "windows"))]
    let mut cmd = {
        let mut c = crate::process_util::no_window_command("mustard-rt");
        c.args(["run", "metrics", "wave-status", "--spec", spec_name]);
        c
    };

    cmd.current_dir(repo_path);

    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => return Err(format!("spawn mustard-rt: {e}")),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    // The rt binary prints the JSON document at the end of stdout. Some hook
    // installations also print a leading `[rtk] ...` banner; trim everything
    // before the first `{` so the parse is robust to that prefix.
    let json_start = stdout.find('{').unwrap_or(0);
    let json_slice = &stdout[json_start..];
    match serde_json::from_str::<MetricsWaveStatus>(json_slice) {
        Ok(parsed) => Ok(parsed),
        Err(_) => {
            // Subprocess emitted unparseable output (binary missing, panic,
            // schema drift). Surface an empty result so the dashboard renders
            // the empty state instead of throwing. The frontend's `parent`
            // null + empty waves combo is the agreed empty contract.
            Ok(MetricsWaveStatus {
                parent: Some(spec_name.to_string()),
                waves: Vec::new(),
            })
        }
    }
}

/// Wave 4 adapter: workspace summary via `mustard-core`. Replaces the
/// broken `events_per_minute` and `tokens_saved_today` SQL with the
/// projection from `project_workspace`.
///
/// Wave 8 (2026-05-21, spec `2026-05-20-economia-moat-unification/wave-8-visao-geral-revamp`):
/// after delegating to mustard-core we override `top_files_today` with an
/// independent SQLite aggregation. The previous reader path replayed every
/// event from the store and then filtered by `today_start = now - (now %
/// 86_400_000)` in the projection. That UTC-midnight cut combined with the
/// post-CLOSE session rotation caused the ranking to drop to zero immediately
/// after a spec moved to `completed/` — even though the underlying `tool.use`
/// rows were still present in the events table. We now query the harness DB
/// directly across **all** session_ids for the local-day window so the list
/// stays populated after a CLOSE. Returns the mustard-core result unchanged
/// when the override fails-soft (DB missing / schema mismatch).
pub fn workspace_summary_v2(repo_path: &str) -> Result<WorkspaceSummary, String> {
    let project = std::path::PathBuf::from(repo_path);
    let events = mustard_core::view::projection::read_workspace_events(&project);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map_or(0_i64, |d| d.as_millis().min(i64::MAX as u128) as i64);
    let summary = mustard_core::view::projection::project_workspace(&events, now_ms);
    let mut out = workspace_summary_from_view(&summary);

    // Followup-fix (2026-05-21, spec `2026-05-21-economia-moat-followup-fixes`):
    // strip terminal-status specs from `spec_tracks` so the "PIPELINES ATIVOS"
    // hero card never lists a `completed` / `cancelled` / `closed-followup`
    // spec as if it were still in EXECUTE. The previous behaviour leaked the
    // last `pipeline.phase` event regardless of whether `pipeline.status` had
    // since moved the spec into a terminal bucket. The TS side mirrors the
    // same filter in `WorkspaceHero.tsx`; both layers stay in sync.
    out.spec_tracks
        .retain(|track| !is_terminal_status(track.status.as_str()));
    out.specs_active_count = i64::try_from(out.spec_tracks.len()).unwrap_or(i64::MAX);

    // Override the file ranking with a session-agnostic SQL aggregation so
    // post-CLOSE the list does not empty out. Fail-soft: keep the mustard-core
    // value when the DB is unavailable.
    let base = std::path::PathBuf::from(repo_path);
    if let Some(Ok(files)) = crate::db::with_db(&base, top_files_today_impl) {
        out.top_files_today = files;
    }
    Ok(out)
}

/// Returns true for spec statuses that represent a finished / parked pipeline
/// — those rows should not appear in the "PIPELINES ATIVOS" hero list.
/// Centralised here so the same set is reused if other commands ever need
/// the same predicate. Kebab-case strings mirror `spec_status_string`.
fn is_terminal_status(status: &str) -> bool {
    matches!(status, "completed" | "closed-followup" | "cancelled")
}

/// Maximum entries in `top_files_today`. Mirrors the cap used by mustard-core
/// so the API contract is identical regardless of which path produced the list.
pub const TOP_FILES_CAP: usize = 10;

/// Aggregate `tool.use` events from today (UTC) by their target file path.
///
/// The query intentionally does **not** filter by `session_id` or by the
/// owning spec's status — every `tool.use` row from today contributes,
/// including those whose spec has already moved into `completed/`. We try the
/// modern payload shape (`$.file_path` / `$.tool_input.file_path`) plus the
/// legacy `$.target.file` to stay aligned with the projection in
/// `mustard-core::project_workspace`.
pub fn top_files_today_impl(_conn: &Connection) -> Result<Vec<FileCount>, String> {
    // Wave 6A no-sqlite stub: closure unreachable post-SQLite-removal.
    Ok(Vec::new())
}

// ── View → legacy JSON shape mappers ─────────────────────────────────────────
//
// These keep the React side unchanged. When you add a field to a
// `mustard_core::*View`, decide whether the dashboard needs it: if yes,
// extend the shape above AND its mapper; if no, leave the mapper alone.

/// Map [`mustard_core::SpecView`] into the legacy [`SpecCard`] JSON shape.
///
/// `children_count` is computed by the caller (`spec_card_v2`) and threaded
/// in here so the mapper stays a pure projection over the view.
fn spec_card_from_view(view: &mustard_core::SpecView, children_count: u32) -> SpecCard {
    SpecCard {
        spec: view.spec.clone(),
        status: mustard_core::domain::spec::status_word(&view.state).into(),
        phase: view
            .phase
            .map_or_else(String::new, |p| phase_string(p).to_string()),
        scope: view.scope.map(|s| scope_string(s).to_string()),
        started_at: view.started_at.clone(),
        last_event_at: view.last_event_at.clone(),
        duration_ms: view.duration_ms,
        current_wave: view.current_wave.map(i64::from),
        total_waves: view.total_waves.map(i64::from),
        ac_passed: i64::from(view.ac_passed),
        ac_total: i64::from(view.ac_total),
        files_touched: i64::from(view.files_touched),
        tools_used: i64::from(view.tools_used),
        model: view.model.clone(),
        children_count,
    }
}

/// Map [`mustard_core::WaveView`] → legacy [`SpecWave`].
fn spec_wave_from_view(view: &mustard_core::WaveView) -> SpecWave {
    SpecWave {
        wave: i64::from(view.wave),
        role: view.role.clone(),
        status: wave_status_string(view.status).into(),
        started_at: view.started_at.clone(),
        completed_at: view.completed_at.clone(),
        agent_type: view.agent_type.clone(),
        files_changed: i64::try_from(view.files_changed.len()).unwrap_or(i64::MAX),
    }
}

/// Map [`mustard_core::AcceptanceCriterion`] → legacy [`SpecQualityItem`].
fn quality_item_from_view(view: &mustard_core::AcceptanceCriterion) -> SpecQualityItem {
    SpecQualityItem {
        ac_id: view.id.clone(),
        ac_label: Some(view.label.clone()).filter(|s| !s.is_empty()),
        status: ac_status_string(view.status).into(),
        wave: view.wave.map(i64::from),
        command: view.command.clone(),
        last_run_at: view.last_run_at.clone(),
        fail_reason: view.fail_reason.clone(),
    }
}

/// Map [`mustard_core::TimelineNode`] → legacy [`SpecTimelineNode`].
fn timeline_node_from_view(view: &mustard_core::TimelineNode) -> SpecTimelineNode {
    SpecTimelineNode {
        ts: view.ts.clone(),
        kind: timeline_kind_string(view.kind).into(),
        label: view.label.clone(),
        phase: view.phase.map(|p| phase_string(p).to_string()),
        wave: view.wave.map(i64::from),
        payload_summary: if view.payload_summary.is_empty() {
            None
        } else {
            Some(view.payload_summary.clone())
        },
    }
}

/// Map [`mustard_core::WorkspaceSummary`] → legacy [`WorkspaceSummary`].
fn workspace_summary_from_view(view: &mustard_core::WorkspaceSummary) -> WorkspaceSummary {
    WorkspaceSummary {
        events_per_minute: view.events_per_minute,
        // Preserve `None` end-to-end so the frontend can render "—" when
        // token-savings data is unavailable instead of misrepresenting it
        // as a literal "0 tokens economizados". Spec
        // `2026-05-20-dashboard-ux-honest` Wave 1.
        tokens_saved_today: view.tokens_saved_today,
        specs_active_count: i64::from(view.specs_active_count),
        spec_tracks: view.spec_tracks.iter().map(spec_track_from_view).collect(),
        alerts: view.alerts.iter().map(workspace_alert_from_view).collect(),
        top_files_today: view
            .top_files_today
            .iter()
            .map(|f| FileCount {
                path: f.path.clone(),
                count: i64::from(f.count),
            })
            .collect(),
    }
}

fn spec_track_from_view(view: &mustard_core::SpecTrack) -> SpecTrack {
    SpecTrack {
        spec: view.spec.clone(),
        status: mustard_core::domain::spec::status_word(&view.state).into(),
        current_phase: view
            .current_phase
            .map_or_else(String::new, |p| phase_string(p).to_string()),
        current_wave: view.current_wave.map(i64::from),
        total_waves: view.total_waves.map(i64::from),
        agents_active: i64::from(view.agents_active),
        last_event_at: view.last_event_at.clone(),
        blocked_reason: view.blocked_reason.clone(),
        segments: view
            .segments
            .iter()
            .map(|seg| PhaseSegment {
                phase: phase_string(seg.phase).to_string(),
                state: segment_state_string(seg.state).into(),
            })
            .collect(),
    }
}

fn workspace_alert_from_view(view: &mustard_core::WorkspaceAlert) -> WorkspaceAlert {
    WorkspaceAlert {
        kind: workspace_alert_kind_string(view.kind).into(),
        spec: view.spec.clone(),
        wave: None, // legacy shape had wave; the new view's message carries it
        message: view.message.clone(),
        ts: Some(view.ts.clone()),
    }
}

// ── Enum → legacy string mappers ─────────────────────────────────────────────
//
// Centralised so a rename in `mustard_core` only needs one edit. The
// strings match what the React side already understands — match against
// these in case a future rename breaks UI rendering.


const fn phase_string(p: mustard_core::Phase) -> &'static str {
    use mustard_core::Phase;
    match p {
        Phase::Analyze => "analyze",
        Phase::Plan => "plan",
        Phase::Execute => "execute",
        Phase::Qa => "qa",
        Phase::Close => "close",
    }
}

const fn scope_string(s: mustard_core::Scope) -> &'static str {
    use mustard_core::Scope;
    match s {
        Scope::Full => "full",
        Scope::Light => "light",
        Scope::Touch => "touch",
    }
}

const fn wave_status_string(s: mustard_core::WaveStatus) -> &'static str {
    use mustard_core::WaveStatus;
    match s {
        WaveStatus::Queued => "queued",
        WaveStatus::InProgress => "in_progress",
        WaveStatus::Completed => "completed",
        WaveStatus::Failed => "failed",
    }
}

const fn ac_status_string(s: mustard_core::AcStatus) -> &'static str {
    use mustard_core::AcStatus;
    match s {
        AcStatus::Pass => "pass",
        AcStatus::Fail => "fail",
        AcStatus::Skip => "skip",
        AcStatus::Pending => "pending",
    }
}

const fn timeline_kind_string(k: mustard_core::TimelineKind) -> &'static str {
    use mustard_core::TimelineKind;
    match k {
        TimelineKind::Scope => "scope",
        TimelineKind::Phase => "phase",
        TimelineKind::Status => "status",
        TimelineKind::Task => "task",
        TimelineKind::Wave => "wave",
        TimelineKind::Qa => "qa",
        TimelineKind::Review => "review",
        TimelineKind::Agent => "agent",
        TimelineKind::Tool => "tool",
        TimelineKind::Decision => "decision",
        TimelineKind::Other => "other",
    }
}

const fn segment_state_string(s: mustard_core::SegmentState) -> &'static str {
    use mustard_core::SegmentState;
    match s {
        SegmentState::Completed => "completed",
        SegmentState::Active => "active",
        SegmentState::Future => "future",
    }
}

const fn workspace_alert_kind_string(k: mustard_core::WorkspaceAlertKind) -> &'static str {
    use mustard_core::WorkspaceAlertKind;
    match k {
        WorkspaceAlertKind::Blocked => "blocked",
        WorkspaceAlertKind::QaFail => "qa_fail",
        WorkspaceAlertKind::WaveFailed => "wave_failed",
        WorkspaceAlertKind::ReviewRejected => "review_rejected",
        WorkspaceAlertKind::BuildBroken => "build_broken",
    }
}

// ===========================================================================
// Wave 3 (2026-05-20) — wikilink graph + cross-wave memory bridges.
//
// The frontend `SpecNetworkTab` shells out to `mustard-rt run wikilink-extract`
// once per spec to render the graph, and `mustard-rt run memory cross-wave`
// once per detected wave for the markdown panel. Both helpers follow the same
// fail-open contract as `dashboard_metrics_wave_status_run`: subprocess
// failures resolve to an empty payload so the dashboard renders an empty
// state instead of throwing. `Err` is reserved for spawn failures so the UI
// can surface "mustard-rt not on PATH".
// ===========================================================================

/// Locate the spec directory for `spec_name` under the flat layout introduced
/// by wave-3 of `2026-05-21-flatten-spec-layout-and-multi-collab`: every spec
/// lives directly at `.claude/spec/{name}/` regardless of lifecycle state.
/// Bucket subdirectories (`active/`, `completed/`, `cancelled/`) are gone.
///
/// Wave-plan children stay nested one level deep inside their parent
/// (`.claude/spec/{parent}/{name}/`) — that nesting is intrinsic to the
/// wave-plan layout and survives the flatten. We resolve children by scanning
/// one level under `spec/` (each entry is a potential parent dir).
fn resolve_spec_dir(repo_path: &str, spec_name: &str) -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    if spec_name.is_empty()
        || spec_name.contains('/')
        || spec_name.contains('\\')
        || spec_name.contains("..")
    {
        return None;
    }
    let base = PathBuf::from(repo_path).join(".claude").join("spec");

    // Direct hit: `.claude/spec/{spec_name}/`.
    let direct = base.join(spec_name);
    if direct.is_dir() {
        return Some(direct);
    }

    // Wave child nested inside a wave-plan parent:
    // `.claude/spec/{parent}/{spec_name}/`.
    let Ok(rd) = fs::read_dir(&base) else { return None };
    for entry in rd {
        if !entry.is_dir {
            continue;
        }
        let child = entry.path.join(spec_name);
        if child.is_dir() {
            return Some(child);
        }
    }
    None
}

/// Build a `Command` that invokes `mustard-rt` with the given args. Uses
/// `cmd /C` on Windows so the binary is resolved against PATH the same way
/// `dashboard_metrics_wave_status_run` does it.
fn mustard_rt_command(args: &[&str]) -> std::process::Command {
    #[cfg(target_os = "windows")]
    {
        let mut c = crate::process_util::no_window_command("cmd");
        let mut full: Vec<&str> = vec!["/C", "mustard-rt"];
        full.extend_from_slice(args);
        c.args(&full);
        c
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut c = crate::process_util::no_window_command("mustard-rt");
        c.args(args);
        c
    }
}

/// Trim any RTK banner / leading log noise so `serde_json::from_str` sees a
/// pure JSON document starting at the first `{`.
fn slice_json(stdout: &str) -> &str {
    match stdout.find('{') {
        Some(i) => &stdout[i..],
        None => stdout,
    }
}

/// Wave-3 — invoke `mustard-rt run wikilink-extract --spec-dir <dir>` for
/// `spec_name`, parse the JSON, return the typed payload. Fail-open: spawn
/// errors surface as `Err`; everything else (missing dir, unparseable JSON)
/// returns an empty extract so the frontend renders the empty state.
pub fn dashboard_wikilink_extract_run(
    repo_path: &str,
    spec_name: &str,
) -> Result<WikilinkExtract, String> {
    let Some(spec_dir) = resolve_spec_dir(repo_path, spec_name) else {
        return Ok(WikilinkExtract::default());
    };
    let dir_str = spec_dir.to_string_lossy().to_string();
    let mut cmd = mustard_rt_command(&["run", "wikilink-extract", "--spec-dir", &dir_str]);
    cmd.current_dir(repo_path);
    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => return Err(format!("spawn mustard-rt: {e}")),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    match serde_json::from_str::<WikilinkExtract>(slice_json(&stdout)) {
        Ok(parsed) => Ok(parsed),
        Err(_) => Ok(WikilinkExtract::default()),
    }
}

/// Wave-3 — invoke `mustard-rt run memory cross-wave --spec <name> --wave <n>`
/// and return the markdown payload (stdout). Empty string when the subprocess
/// has nothing to report (the most common case — earlier waves carry no
/// memory). `Err` is reserved for spawn failures.
pub fn dashboard_memory_cross_wave_run(
    repo_path: &str,
    spec: &str,
    wave: u32,
) -> Result<String, String> {
    if spec.is_empty() || spec.contains('/') || spec.contains('\\') || spec.contains("..") {
        return Err(format!("invalid spec name: {spec}"));
    }
    let wave_str = wave.to_string();
    let mut cmd = mustard_rt_command(&[
        "run", "memory", "cross-wave", "--spec", spec, "--wave", &wave_str,
    ]);
    cmd.current_dir(repo_path);
    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => return Err(format!("spawn mustard-rt: {e}")),
    };
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Wave 2 (2026-05-21, spec `2026-05-21-dashboard-spec-tabs`) — payload for
/// `dashboard_spec_wave_files`. `count` is the number of files declared in the
/// wave sub-spec's `## Arquivos` section; `markdown` is the full wave-N spec
/// markdown so the drawer can render it without a second round-trip; `path`
/// is the resolved on-disk path or `None` when the wave sub-spec is missing.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "snake_case")]
pub struct WaveFilesPayload {
    pub count: u32,
    pub markdown: String,
    pub path: Option<String>,
}

/// Wave 1 (2026-05-21, spec `2026-05-21-dashboard-spec-tabs-polish`) — one
/// wave declared on disk (a `wave-N-{role}/` subdir of the spec). Surfaces
/// the structure the wave-plan declared, independent of whether the SQLite
/// event stream has any `wave.*` events for it yet. The dashboard unions
/// this with `SpecWave[]` from the SQLite projection so the "Ondas" tab
/// shows the full plan during EXECUTE — waves only present here render as
/// `status="queued"`.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct SpecWavePlanned {
    pub wave: u32,
    pub role: Option<String>,
    pub declared_files_count: u32,
}

/// Local copy of `apps/rt/src/run/wave_files.rs::parse_arquivos_count`.
/// Duplicated rather than imported because `mustard-rt` is a binary crate, not
/// a lib, and pulling it in here would create a workspace dep cycle. Keep the
/// counting rules in sync: bullet (`- ` / `* `) lines outside fenced code, or
/// non-blank non-comment lines inside the fenced block following the `##
/// Arquivos` / `## Files` heading; section ends at the next `## ` heading.
fn parse_arquivos_count(text: &str) -> usize {
    let mut in_section = false;
    let mut in_fence = false;
    let mut count: usize = 0;

    for line in text.lines() {
        let trimmed_start = line.trim_start();
        if !in_section {
            if line == "## Arquivos" || line == "## Files" {
                in_section = true;
            }
            continue;
        }
        if !in_fence && line.starts_with("## ") {
            // Re-entering the same heading is a no-op; any other `## ` ends.
            if line == "## Arquivos" || line == "## Files" {
                continue;
            }
            break;
        }
        if trimmed_start.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            let content = line.trim();
            if content.is_empty() {
                continue;
            }
            if content.starts_with("//") || content.starts_with('#') {
                continue;
            }
            count += 1;
            continue;
        }
        if trimmed_start.starts_with("- ") || trimmed_start.starts_with("* ") {
            count += 1;
        }
    }
    count
}

/// Wave 1 — scan `<repo>/.claude/spec/{spec}/` for `wave-N-{role}/` subdirs and
/// return one [`SpecWavePlanned`] per match, sorted by wave number ascending.
/// Resolves files in-process (no `mustard-rt` subprocess) because otherwise we
/// would spawn once per wave per spec drawer open. Fail-open: any I/O hiccup
/// degrades to an empty list; only an outright invalid spec name surfaces as
/// `Err`.
pub fn dashboard_spec_waves_planned_run(
    repo_path: &str,
    spec: &str,
) -> Result<Vec<SpecWavePlanned>, String> {
    if spec.is_empty() || spec.contains('/') || spec.contains('\\') || spec.contains("..") {
        return Err(format!("invalid spec name: {spec}"));
    }

    let spec_dir = std::path::Path::new(repo_path)
        .join(".claude")
        .join("spec")
        .join(spec);
    if !spec_dir.is_dir() {
        return Ok(Vec::new());
    }

    let Ok(rd) = fs::read_dir(&spec_dir) else {
        return Ok(Vec::new());
    };

    let mut out: Vec<SpecWavePlanned> = Vec::new();
    for entry in rd {
        let path = &entry.path;
        if !entry.is_dir {
            continue;
        }
        let name = entry.file_name.clone();
        // Match `wave-{digits}-{role}` exactly. Role is the remainder after
        // the second dash; must be non-empty.
        let Some(rest) = name.strip_prefix("wave-") else { continue };
        let dash_idx = match rest.find('-') {
            Some(i) if i > 0 => i,
            _ => continue,
        };
        let (num_str, role_with_dash) = rest.split_at(dash_idx);
        let role = &role_with_dash[1..]; // skip the '-'
        if role.is_empty() {
            continue;
        }
        let wave_num: u32 = match num_str.parse() {
            Ok(n) => n,
            Err(_) => continue,
        };

        // Count declared files via the wave sub-spec's `## Arquivos` block.
        // Missing / unreadable spec.md degrades to 0.
        let spec_md = path.join("spec.md");
        let declared = match fs::read_to_string(&spec_md) {
            Ok(text) => u32::try_from(parse_arquivos_count(&text)).unwrap_or(u32::MAX),
            Err(_) => 0,
        };

        out.push(SpecWavePlanned {
            wave: wave_num,
            role: Some(role.to_string()),
            declared_files_count: declared,
        });
    }

    out.sort_by_key(|w| w.wave);
    Ok(out)
}

/// Wave 2 — invoke `mustard-rt run wave-files --spec <name> --wave <N>` and
/// parse the JSON stdout into a typed [`WaveFilesPayload`]. Subprocess
/// invocation mirrors `dashboard_metrics_wave_status_run` (Windows uses
/// `cmd /C mustard-rt …`, POSIX uses the binary directly). Fail-open:
/// missing wave sub-spec → `{count:0, markdown:"", path:null}`; subprocess
/// JSON parse failure → same empty payload. `Err` is reserved for spawn
/// failures so the frontend can surface "mustard-rt not on PATH".
pub fn dashboard_spec_wave_files_run(
    repo_path: &str,
    spec: &str,
    wave: u32,
) -> Result<WaveFilesPayload, String> {
    if spec.is_empty() || spec.contains('/') || spec.contains('\\') || spec.contains("..") {
        return Err(format!("invalid spec name: {spec}"));
    }
    let wave_str = wave.to_string();
    let mut cmd = mustard_rt_command(&[
        "run", "wave-files", "--spec", spec, "--wave", &wave_str,
    ]);
    cmd.current_dir(repo_path);
    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => return Err(format!("spawn mustard-rt: {e}")),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    match serde_json::from_str::<WaveFilesPayload>(slice_json(&stdout)) {
        Ok(parsed) => Ok(parsed),
        Err(_) => Ok(WaveFilesPayload::default()),
    }
}

// ===========================================================================
// Wave 3 (spec-lifecycle-unification) — spec-children-tree.
//
// `dashboard_spec_children_tree` shells out to `mustard-rt run
// spec-children-tree --spec NAME` and returns the parsed `ChildrenTree`. The
// shapes below mirror `apps/rt/src/run/spec_children_tree.rs` 1:1 (`WaveChild`,
// `AcChild`, `ChildrenTree`) plus `mustard_core::SpecChild` (the `subspecs`
// element). They are `Deserialize`-only here: we never re-serialise these on
// the backend, the React side reads them straight from the command result.
// Same fail-open + JSON-slice contract as `spec_children_v2` /
// `dashboard_spec_wave_files_run`.
// ===========================================================================

/// One wave row — mirrors `WaveChild` in `spec_children_tree.rs`. `status` is
/// the kebab-case `WaveStatus` (`queued | in-progress | completed | failed`).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct WaveChild {
    pub idx: u32,
    pub role: String,
    pub status: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub duration_ms: Option<i64>,
}

/// One acceptance-criterion row — mirrors `AcChild`. `status` is the lowercase
/// `AcStatus` (`pass | fail | skip | pending`).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct AcChild {
    pub id: String,
    pub label: String,
    pub status: String,
    pub last_run_at: Option<String>,
    pub evidence: Option<String>,
}

/// Lifecycle qualifiers — mirrors `mustard_core::Flags`.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "snake_case")]
pub struct StateFlags {
    #[serde(default)]
    pub blocked: bool,
    #[serde(default)]
    pub wave_failed: bool,
    #[serde(default)]
    pub followup_open: bool,
}

/// Canonical lifecycle state — mirrors `mustard_core::SpecState`. `stage` and
/// `outcome` are kebab-case enums on the core side; carried as `String` here.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct SpecStateJson {
    pub stage: String,
    pub outcome: String,
    #[serde(default)]
    pub flags: StateFlags,
}

/// One linked sub-spec — mirrors `mustard_core::SpecChild`, the `subspecs`
/// element of the children tree.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct SubSpecChild {
    pub spec: String,
    pub state: SpecStateJson,
    /// Legacy flat status, derived from `state` (kebab-case).
    pub status: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub reason: Option<String>,
}

/// Full projection from `mustard-rt run spec-children-tree --spec NAME` —
/// mirrors `ChildrenTree` in `apps/rt/src/run/spec_children_tree.rs`.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "snake_case")]
pub struct ChildrenTree {
    #[serde(default)]
    pub spec: String,
    #[serde(default)]
    pub waves: Vec<WaveChild>,
    #[serde(default)]
    pub acs: Vec<AcChild>,
    #[serde(default)]
    pub subspecs: Vec<SubSpecChild>,
}

/// Invoke `mustard-rt run spec-children-tree --spec NAME` and parse the JSON
/// document into a typed [`ChildrenTree`]. Fail-open: a spawn failure surfaces
/// as `Err` (so the UI can show "mustard-rt not on PATH"); unparseable / empty
/// stdout returns an empty tree so the row renders a clean empty state.
pub fn spec_children_tree_run(repo_path: &str, spec: &str) -> Result<ChildrenTree, String> {
    if spec.is_empty() || spec.contains('/') || spec.contains('\\') || spec.contains("..") {
        return Err(format!("invalid spec name: {spec}"));
    }
    let mut cmd = mustard_rt_command(&["run", "spec-children-tree", "--spec", spec]);
    cmd.current_dir(repo_path);
    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => return Err(format!("spawn mustard-rt: {e}")),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    match serde_json::from_str::<ChildrenTree>(slice_json(&stdout)) {
        Ok(parsed) => Ok(parsed),
        Err(_) => Ok(ChildrenTree {
            spec: spec.to_string(),
            ..ChildrenTree::default()
        }),
    }
}

// ===========================================================================
// Wave 1a (2026-05-20, spec `dashboard-visual-overview`) — three aggregations
// for the redesigned Overview page. Each command opens the project's
// `mustard.db` via `crate::db::with_db`, falls back to an empty payload when
// the harness store is missing/empty, and only returns `Err` for genuinely
// unrecoverable conditions (currently: invalid month, prepare/query failures
// are coerced to empty results so the UI renders an empty state).
//
// Schema notes (events table):
//   * the "kind" referenced in the spec maps to column `event`
//   * payload is a JSON column; sub-fields are extracted via
//     `json_extract(payload, '$.<name>')`
//   * `ts` is ISO-8601 text and lexicographically sortable
// ===========================================================================

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct TopPipeline {
    pub spec: String,
    pub saved: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "snake_case")]
pub struct TokenSummary {
    pub total_saved: i64,
    pub top_pipelines: Vec<TopPipeline>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct DayActivity {
    /// `YYYY-MM-DD`
    pub date: String,
    pub event_count: i32,
    pub top_phase: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct FeedEvent {
    pub id: String,
    /// ISO-8601 (as stored in `events.ts`).
    pub ts: String,
    /// Spec field name is `kind`; underlying column is `events.event`.
    pub kind: String,
    pub spec: Option<String>,
    /// ≤120 chars derived from payload.
    pub payload_summary: String,
}

/// `dashboard_token_summary` — aggregate `events` where `event = 'token.saved'`,
/// sum `payload.saved`, group top 5 by `spec`.
#[tauri::command]
pub fn dashboard_token_summary(project_path: String) -> Result<TokenSummary, String> {
    let base = std::path::PathBuf::from(&project_path);
    match crate::db::with_db(&base, token_summary_impl) {
        Some(r) => r,
        None => Ok(TokenSummary::default()),
    }
}

fn token_summary_impl(_conn: &Connection) -> Result<TokenSummary, String> {
    // Wave 6A no-sqlite stub: closure unreachable; token savings are read
    // from NDJSON `pipeline.economy.savings.*` directly by other commands.
    Ok(TokenSummary::default())
}

/// `dashboard_month_activity` — emit one entry per day of the given month
/// (1..N) even with 0 events; `top_phase` is the phase with the most events
/// that day, derived from `pipeline.phase` events' `payload.phase`.
#[tauri::command]
pub fn dashboard_month_activity(
    project_path: String,
    year: i32,
    month: u32,
) -> Result<Vec<DayActivity>, String> {
    if !(1..=12).contains(&month) {
        return Err(format!("invalid month: {month}"));
    }
    let base = std::path::PathBuf::from(&project_path);
    let days_in_month = days_in_month(year, month);
    let scaffold: Vec<DayActivity> = (1..=days_in_month)
        .map(|d| DayActivity {
            date: format!("{:04}-{:02}-{:02}", year, month, d),
            event_count: 0,
            top_phase: None,
        })
        .collect();

    match crate::db::with_db(&base, |conn| month_activity_impl(conn, year, month, scaffold.clone())) {
        Some(r) => r,
        None => Ok(scaffold),
    }
}

fn month_activity_impl(
    _conn: &Connection,
    _year: i32,
    _month: u32,
    out: Vec<DayActivity>,
) -> Result<Vec<DayActivity>, String> {
    // Wave 6A no-sqlite stub: returns the pre-built day scaffold with zero
    // counts. Closure unreachable post-SQLite-removal; callers should rebuild
    // this projection from NDJSON `pipeline.phase` events when needed.
    Ok(out)
}

/// `dashboard_events_feed` — chronological-reverse feed, `ORDER BY ts DESC`
/// with the caller-supplied `LIMIT`. `payload_summary` is a ≤120-char humanised
/// rendering of the payload (e.g. `"draft → implementing"` for
/// `pipeline.status`).
#[tauri::command]
pub fn dashboard_events_feed(
    project_path: String,
    limit: u32,
) -> Result<Vec<FeedEvent>, String> {
    let base = std::path::PathBuf::from(&project_path);
    let cap = limit.clamp(1, 1000); // defensive cap; UI typically asks ≤200
    match crate::db::with_db(&base, |conn| events_feed_impl(conn, cap)) {
        Some(r) => r,
        None => Ok(vec![]),
    }
}

fn events_feed_impl(_conn: &Connection, _limit: u32) -> Result<Vec<FeedEvent>, String> {
    // Wave 6A no-sqlite stub: closure unreachable; the events feed should be
    // derived from NDJSON in a follow-up sub-spec.
    Ok(Vec::new())
}

/// Number of days in `month` for the given `year` (Gregorian).
const fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            // Leap year rule: divisible by 4, except centuries not divisible by 400.
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}
