//! Wave 1a (2026-05-20, spec `dashboard-visual-overview`) — three new
//! aggregations live at the bottom of this file (`dashboard_token_summary`,
//! `dashboard_month_activity`, `dashboard_events_feed`). They read the
//! `events` table directly via `db::with_db` and follow the fail-open
//! contract of the rest of the module (missing DB → empty payload).
//!
//! `*_v2` adapter family that delegates to `mustard-core`.
//!
//! Each `*_v2` function is a thin adapter — it opens a
//! [`mustard_core::SqliteSpecReader`], runs the projection, and maps the
//! typed ViewModel into the JSON shape the frontend already expects (so React
//! contracts stay untouched). The legacy hand-rolled SQL functions
//! (`spec_card`, `spec_waves`, `spec_quality`, `spec_timeline`,
//! `workspace_summary`) were removed in Wave 2 of spec
//! `2026-05-20-sdd-domain-finalization`; the Tauri commands in `lib.rs`
//! already delegated to the `*_v2` adapters since Wave 4 of the audit.

use rusqlite::{Connection, params};
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

// ── spec_events ───────────────────────────────────────────────────────────────

pub fn spec_events(
    conn: &Connection,
    spec: &str,
    filter: Option<EventFilter>,
) -> Result<Vec<TimelineEvent>, String> {
    let filter = filter.unwrap_or_default();

    // Build event kind filter fragment
    let kinds_clause = match &filter.kinds {
        Some(kinds) if !kinds.is_empty() => {
            let placeholders: Vec<String> =
                kinds.iter().enumerate().map(|(i, _)| format!("?{}", i + 2)).collect();
            format!("AND event IN ({})", placeholders.join(","))
        }
        _ => String::new(),
    };

    let sql = format!(
        "SELECT CAST(id AS TEXT), COALESCE(ts,''), \
                json_extract(payload,'$.phase'), \
                spec, \
                COALESCE(json_extract(payload,'$.subagent_type'), \
                         json_extract(payload,'$.agent_type'), \
                         actor_id), \
                COALESCE(json_extract(payload,'$.summary'), \
                         json_extract(payload,'$.description'), \
                         json_extract(payload,'$.msg'), \
                         event, '') \
         FROM events \
         WHERE spec=?1 {} \
         ORDER BY id DESC \
         LIMIT 500",
        kinds_clause
    );

    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return Ok(vec![]),
    };

    // Bind spec as first param; bind kinds in order if present
    let rows_result = if let Some(kinds) = &filter.kinds {
        if !kinds.is_empty() {
            // rusqlite doesn't support heterogeneous params! directly — use
            // a helper that constructs the query with literal placeholders
            // but we need to pass them one by one. Build params as a Vec<&dyn ToSql>.
            let mut all_params: Vec<Box<dyn rusqlite::types::ToSql>> =
                vec![Box::new(spec.to_string())];
            for k in kinds {
                all_params.push(Box::new(k.clone()));
            }
            let refs: Vec<&dyn rusqlite::types::ToSql> =
                all_params.iter().map(|b| b.as_ref()).collect();
            stmt.query_map(refs.as_slice(), map_timeline_row)
        } else {
            stmt.query_map(params![spec], map_timeline_row)
        }
    } else {
        stmt.query_map(params![spec], map_timeline_row)
    };

    let rows = match rows_result {
        Ok(r) => r,
        Err(_) => return Ok(vec![]),
    };

    let mut out: Vec<TimelineEvent> = rows.flatten().collect();

    // Apply optional in-process filters (wave, agent, q substring)
    if let Some(wave_num) = filter.wave {
        // We need the wave column — re-query with wave if filter is set.
        // For simplicity, do a second targeted query.
        let wave_sql = format!(
            "SELECT CAST(id AS TEXT), COALESCE(ts,''), \
                    json_extract(payload,'$.phase'), \
                    spec, \
                    COALESCE(json_extract(payload,'$.subagent_type'), \
                             json_extract(payload,'$.agent_type'), actor_id), \
                    COALESCE(json_extract(payload,'$.summary'), \
                             json_extract(payload,'$.description'), \
                             json_extract(payload,'$.msg'), event, '') \
             FROM events \
             WHERE spec=?1 AND wave=?2 {} \
             ORDER BY id DESC LIMIT 500",
            kinds_clause
        );
        let mut wstmt = match conn.prepare(&wave_sql) {
            Ok(s) => s,
            Err(_) => return Ok(out),
        };
        let wave_rows = wstmt.query_map(params![spec, wave_num], map_timeline_row);
        if let Ok(wr) = wave_rows {
            out = wr.flatten().collect();
        }
    }

    if let Some(agent_str) = &filter.agent {
        let a = agent_str.clone();
        out.retain(|e| e.agent.as_deref().map_or(false, |ag| ag.contains(a.as_str())));
    }
    if let Some(q) = &filter.q {
        let q = q.to_lowercase();
        out.retain(|e| {
            e.summary.to_lowercase().contains(&q)
                || e.phase.as_deref().map_or(false, |p| p.to_lowercase().contains(&q))
        });
    }

    Ok(out)
}

fn map_timeline_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<TimelineEvent> {
    Ok(TimelineEvent {
        id:      row.get::<_, Option<String>>(0)?.unwrap_or_default(),
        ts:      row.get::<_, Option<String>>(1)?.unwrap_or_default(),
        phase:   row.get::<_, Option<String>>(2)?,
        spec:    row.get::<_, Option<String>>(3)?,
        agent:   row.get::<_, Option<String>>(4)?,
        summary: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
    })
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
            std::fs::remove_dir_all(&spec_dir).map_err(|e| e.to_string())?;
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
    use mustard_core::store::sqlite_store::SqliteEventStore;
    let Ok(store) = SqliteEventStore::for_project(repo_path) else {
        return "implementing";
    };
    match store.query(Some(spec)) {
        Ok(events) if !events.is_empty() => "implementing",
        Ok(_) => "planning",
        Err(_) => "implementing",
    }
}

// ── spec_action helpers ───────────────────────────────────────────────────────
//
// Wave-3 of `2026-05-21-flatten-spec-layout-and-multi-collab` collapsed
// these helpers down to the bare minimum: one direct `SqliteEventStore::append`
// for `pipeline.status` / `pipeline.removed`, and a small fail-open header
// rewriter that mirrors `apps/rt/src/run/emit_pipeline.rs::sync_spec_status_header`
// so the canonical `### Status:` line in `spec.md` stays consistent with the
// event store even when the dashboard does the writing (e.g. when the user
// reopens a spec from the desktop UI on a machine where `mustard-rt` on PATH
// is stale — the helper is duplicated rather than re-imported because pulling
// `mustard-rt` as a build dep here would create a workspace cycle).

/// Emit `pipeline.status: <to>` via the SQLite event store. Fail-open: any
/// error during store open / append is logged to stderr and swallowed —
/// telemetry is never load-bearing per the harness contract.
fn emit_pipeline_status(repo_path: &str, spec: &str, to: &str) {
    use mustard_core::model::event::{
        Actor, ActorKind, EVENT_PIPELINE_STATUS, HarnessEvent, PipelineStatusPayload,
        SCHEMA_VERSION,
    };
    use mustard_core::store::event_store::EventSink;
    use mustard_core::store::sqlite_store::SqliteEventStore;

    let store = match SqliteEventStore::for_project(repo_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("emit_pipeline_status: open store: {e}");
            return;
        }
    };

    let payload = serde_json::to_value(PipelineStatusPayload {
        from: None,
        to: to.to_string(),
    })
    .unwrap_or(serde_json::Value::Null);

    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        session_id: String::new(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Cli,
            id: Some("dashboard-spec-action".to_string()),
            actor_type: None,
        },
        event: EVENT_PIPELINE_STATUS.to_string(),
        payload,
        spec: Some(spec.to_string()),
    };

    if let Err(e) = store.append(&event) {
        eprintln!("emit_pipeline_status: append: {e}");
    }
}

/// Emit `pipeline.removed` via the SQLite event store. Fail-open like
/// [`emit_pipeline_status`].
fn emit_pipeline_removed(repo_path: &str, spec: &str) {
    use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
    use mustard_core::store::event_store::EventSink;
    use mustard_core::store::sqlite_store::SqliteEventStore;

    let store = match SqliteEventStore::for_project(repo_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("emit_pipeline_removed: open store: {e}");
            return;
        }
    };

    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        session_id: String::new(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Cli,
            id: Some("dashboard-spec-action".to_string()),
            actor_type: None,
        },
        event: "pipeline.removed".to_string(),
        payload: serde_json::json!({ "removed": true }),
        spec: Some(spec.to_string()),
    };

    if let Err(e) = store.append(&event) {
        eprintln!("emit_pipeline_removed: append: {e}");
    }
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

    let content = match std::fs::read_to_string(&path) {
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
    if let Err(e) = std::fs::write(&path, out) {
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

/// Wave 4 adapter: build a [`SpecCard`] via `mustard-core`.
///
/// Opens a [`mustard_core::SqliteSpecReader`] keyed by `repo_path`,
/// projects the per-spec view, then maps the typed ViewModel into the JSON
/// shape the React frontend already consumes.
///
/// Returns `Ok(None)` when the spec has zero events. The `lib.rs` command
/// converts that to the empty-state JSON payload.
pub fn spec_card_v2(repo_path: &str, spec: &str) -> Result<Option<SpecCard>, String> {
    use mustard_core::SpecReader;
    let reader = mustard_core::SqliteSpecReader::for_project(repo_path)
        .map_err(|e| format!("reader open: {e}"))?;
    let Some(view) = reader.spec_view(spec).map_err(|e| format!("spec_view: {e}"))? else {
        return Ok(None);
    };
    // Spec `2026-05-21-speccard-use-children-count`: include the sub-spec
    // count up-front so the React card stops fanning out one
    // `useSpecChildren` query per rendered row. Failure here is non-fatal —
    // the badge just renders absent.
    let children_count = reader
        .children_of(spec)
        .map(|c| u32::try_from(c.len()).unwrap_or(u32::MAX))
        .unwrap_or(0);
    Ok(Some(spec_card_from_view(&view, children_count)))
}

/// Wave 4 adapter: build the wave list via `mustard-core`. Empty `Vec`
/// when the spec has no wave events.
pub fn spec_waves_v2(repo_path: &str, spec: &str) -> Result<Vec<SpecWave>, String> {
    use mustard_core::SpecReader;
    let reader = mustard_core::SqliteSpecReader::for_project(repo_path)
        .map_err(|e| format!("reader open: {e}"))?;
    let waves = reader.waves(spec).map_err(|e| format!("waves: {e}"))?;
    Ok(waves.iter().map(spec_wave_from_view).collect())
}

/// Wave 4 adapter: AC roll-up via `mustard-core`.
pub fn spec_quality_v2(repo_path: &str, spec: &str) -> Result<Vec<SpecQualityItem>, String> {
    use mustard_core::SpecReader;
    let reader = mustard_core::SqliteSpecReader::for_project(repo_path)
        .map_err(|e| format!("reader open: {e}"))?;
    let rollup = reader.quality(spec).map_err(|e| format!("quality: {e}"))?;
    Ok(rollup.criteria.iter().map(quality_item_from_view).collect())
}

/// Wave 4 adapter: timeline projection via `mustard-core`. `All` window;
/// the dashboard does its own client-side filtering when it needs a narrower
/// view.
pub fn spec_timeline_v2(repo_path: &str, spec: &str) -> Result<Vec<SpecTimelineNode>, String> {
    use mustard_core::SpecReader;
    let reader = mustard_core::SqliteSpecReader::for_project(repo_path)
        .map_err(|e| format!("reader open: {e}"))?;
    let nodes = reader
        .timeline(spec, mustard_core::TimeWindow::All)
        .map_err(|e| format!("timeline: {e}"))?;
    Ok(nodes.iter().map(timeline_node_from_view).collect())
}

/// Wave-3 adapter (spec `2026-05-20-tactical-fix-via-sub-spec`): list
/// sub-specs linked to `parent` via `spec.link` events. Delegates to
/// [`mustard_core::SpecReader::children_of`] and maps the typed view into
/// the legacy JSON shape the dashboard consumes.
pub fn spec_children_v2(repo_path: &str, parent: &str) -> Result<Vec<SpecChild>, String> {
    use mustard_core::SpecReader;
    let reader = mustard_core::SqliteSpecReader::for_project(repo_path)
        .map_err(|e| format!("reader open: {e}"))?;
    let children = reader
        .children_of(parent)
        .map_err(|e| format!("children_of: {e}"))?;
    Ok(children
        .iter()
        .map(|c| SpecChild {
            spec: c.spec.clone(),
            status: spec_status_string(c.status).into(),
            started_at: c.started_at.clone(),
            completed_at: c.completed_at.clone(),
            reason: c.reason.clone(),
        })
        .collect())
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
    use std::process::Command;
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
        let mut c = Command::new("cmd");
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
        let mut c = Command::new("mustard-rt");
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
    use mustard_core::SpecReader;
    let reader = mustard_core::SqliteSpecReader::for_project(repo_path)
        .map_err(|e| format!("reader open: {e}"))?;
    let summary = reader
        .workspace_summary()
        .map_err(|e| format!("workspace_summary: {e}"))?;
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
pub fn top_files_today_impl(conn: &Connection) -> Result<Vec<FileCount>, String> {
    // `date('now')` evaluates to today's UTC midnight as a `YYYY-MM-DD` string;
    // comparing against `events.ts` (also ISO-8601 UTC) keeps the window in the
    // same time zone as the previous in-memory projection.
    let sql = "SELECT path, COUNT(*) AS c FROM ( \
                  SELECT COALESCE( \
                            json_extract(payload, '$.file_path'), \
                            json_extract(payload, '$.tool_input.file_path'), \
                            json_extract(payload, '$.target.file') \
                         ) AS path \
                  FROM events \
                  WHERE event = 'tool.use' \
                    AND ts >= date('now') \
               ) \
               WHERE path IS NOT NULL AND path != '' \
               GROUP BY path \
               ORDER BY c DESC, path ASC \
               LIMIT ?1";
    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(_) => return Ok(vec![]),
    };
    let rows = stmt
        .query_map(params![i64::try_from(TOP_FILES_CAP).unwrap_or(10)], |row| {
            Ok(FileCount {
                path: row.get::<_, String>(0)?,
                count: row.get::<_, i64>(1)?,
            })
        })
        .map(|r| r.flatten().collect::<Vec<_>>())
        .unwrap_or_default();
    Ok(rows)
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
        status: spec_status_string(view.status).into(),
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
        status: spec_status_string(view.status).into(),
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

const fn spec_status_string(status: mustard_core::SpecStatus) -> &'static str {
    use mustard_core::SpecStatus;
    match status {
        SpecStatus::NoEvents => "no-events",
        SpecStatus::Planning => "planning",
        SpecStatus::Implementing => "implementing",
        SpecStatus::Reviewing => "reviewing",
        SpecStatus::Qa => "qa",
        SpecStatus::ClosedFollowup => "closed-followup",
        SpecStatus::Completed => "completed",
        SpecStatus::Cancelled => "cancelled",
        SpecStatus::Abandoned => "abandoned",
        SpecStatus::Blocked => "blocked",
        SpecStatus::WaveFailed => "wave-failed",
    }
}

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
    let Ok(rd) = std::fs::read_dir(&base) else { return None };
    for entry in rd.flatten() {
        let parent_dir = entry.path();
        if !parent_dir.is_dir() {
            continue;
        }
        let child = parent_dir.join(spec_name);
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
        let mut c = std::process::Command::new("cmd");
        let mut full: Vec<&str> = vec!["/C", "mustard-rt"];
        full.extend_from_slice(args);
        c.args(&full);
        c
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut c = std::process::Command::new("mustard-rt");
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

fn token_summary_impl(conn: &Connection) -> Result<TokenSummary, String> {
    // Total saved across every token.saved event.
    let total_saved: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(CAST(json_extract(payload, '$.saved') AS INTEGER)), 0) \
             FROM events WHERE event = 'token.saved'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    // Top 5 pipelines by sum(payload.saved). Skip rows without a spec so the
    // bar list doesn't show a blank label.
    let mut stmt = match conn.prepare(
        "SELECT spec, COALESCE(SUM(CAST(json_extract(payload, '$.saved') AS INTEGER)), 0) AS s \
         FROM events \
         WHERE event = 'token.saved' AND spec IS NOT NULL AND spec != '' \
         GROUP BY spec \
         ORDER BY s DESC \
         LIMIT 5",
    ) {
        Ok(s) => s,
        Err(_) => {
            return Ok(TokenSummary {
                total_saved,
                top_pipelines: vec![],
            });
        }
    };
    let rows = stmt
        .query_map([], |row| {
            Ok(TopPipeline {
                spec: row.get::<_, String>(0)?,
                saved: row.get::<_, i64>(1)?,
            })
        })
        .map(|r| r.flatten().collect::<Vec<_>>())
        .unwrap_or_default();

    Ok(TokenSummary {
        total_saved,
        top_pipelines: rows,
    })
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
    conn: &Connection,
    year: i32,
    month: u32,
    mut out: Vec<DayActivity>,
) -> Result<Vec<DayActivity>, String> {
    // Month bounds in ISO-8601 text. ts strings are lexicographically
    // comparable for the canonical YYYY-MM-DDTHH:MM:SS format we emit.
    let start = format!("{:04}-{:02}-01", year, month);
    let end_excl = if month == 12 {
        format!("{:04}-01-01", year + 1)
    } else {
        format!("{:04}-{:02}-01", year, month + 1)
    };

    // Event counts per day.
    if let Ok(mut stmt) = conn.prepare(
        "SELECT substr(ts, 1, 10) AS d, COUNT(*) \
         FROM events \
         WHERE ts >= ?1 AND ts < ?2 \
         GROUP BY d",
    ) {
        if let Ok(rows) = stmt.query_map(params![start, end_excl], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        }) {
            for (date, count) in rows.flatten() {
                if let Some(slot) = out.iter_mut().find(|d| d.date == date) {
                    slot.event_count = i32::try_from(count).unwrap_or(i32::MAX);
                }
            }
        }
    }

    // Top phase per day — phase derived from pipeline.phase events.
    if let Ok(mut stmt) = conn.prepare(
        "SELECT d, phase FROM ( \
             SELECT substr(ts, 1, 10) AS d, \
                    json_extract(payload, '$.phase') AS phase, \
                    COUNT(*) AS c, \
                    ROW_NUMBER() OVER (PARTITION BY substr(ts, 1, 10) ORDER BY COUNT(*) DESC) AS rn \
             FROM events \
             WHERE ts >= ?1 AND ts < ?2 \
               AND event = 'pipeline.phase' \
               AND json_extract(payload, '$.phase') IS NOT NULL \
             GROUP BY d, phase \
         ) \
         WHERE rn = 1",
    ) {
        if let Ok(rows) = stmt.query_map(params![start, end_excl], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        }) {
            for (date, phase) in rows.flatten() {
                if let Some(slot) = out.iter_mut().find(|d| d.date == date) {
                    slot.top_phase = phase;
                }
            }
        }
    }

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
    let cap = limit.max(1).min(1000); // defensive cap; UI typically asks ≤200
    match crate::db::with_db(&base, |conn| events_feed_impl(conn, cap)) {
        Some(r) => r,
        None => Ok(vec![]),
    }
}

fn events_feed_impl(conn: &Connection, limit: u32) -> Result<Vec<FeedEvent>, String> {
    let mut stmt = match conn.prepare(
        "SELECT CAST(id AS TEXT), COALESCE(ts, ''), event, spec, \
                COALESCE(payload, '') \
         FROM events \
         ORDER BY ts DESC \
         LIMIT ?1",
    ) {
        Ok(s) => s,
        Err(_) => return Ok(vec![]),
    };

    let rows = stmt
        .query_map(params![limit as i64], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?.unwrap_or_default(),
            ))
        })
        .map(|r| r.flatten().collect::<Vec<_>>())
        .unwrap_or_default();

    let out = rows
        .into_iter()
        .map(|(id, ts, kind, spec, payload)| {
            let summary = summarise_payload(&kind, &payload);
            FeedEvent {
                id,
                ts,
                kind,
                spec,
                payload_summary: summary,
            }
        })
        .collect();
    Ok(out)
}

/// Build a short (≤120 char) human-readable summary for a feed row. Kind-aware
/// for the common pipeline events; otherwise falls back to the first useful
/// field (`summary` / `description` / `msg`) or a trimmed payload preview.
fn summarise_payload(kind: &str, payload: &str) -> String {
    let truncated = |s: &str| -> String {
        if s.chars().count() <= 120 {
            s.to_string()
        } else {
            s.chars().take(117).collect::<String>() + "..."
        }
    };

    let json: Option<serde_json::Value> = if payload.is_empty() {
        None
    } else {
        serde_json::from_str(payload).ok()
    };

    if let Some(v) = &json {
        match kind {
            "pipeline.status" => {
                let from = v.get("from").and_then(|x| x.as_str()).unwrap_or("");
                let to = v
                    .get("to")
                    .or_else(|| v.get("status"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("");
                if !from.is_empty() && !to.is_empty() {
                    return truncated(&format!("{from} → {to}"));
                }
                if !to.is_empty() {
                    return truncated(to);
                }
            }
            "pipeline.phase" => {
                if let Some(phase) = v.get("phase").and_then(|x| x.as_str()) {
                    return truncated(phase);
                }
            }
            "token.saved" => {
                if let Some(saved) = v.get("saved").and_then(|x| x.as_i64()) {
                    return truncated(&format!("saved {saved} tokens"));
                }
            }
            _ => {}
        }

        for key in ["summary", "description", "msg", "message", "label", "to", "status", "phase"] {
            if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
                if !s.is_empty() {
                    return truncated(s);
                }
            }
        }
    }

    if payload.is_empty() {
        return String::new();
    }
    truncated(payload)
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
