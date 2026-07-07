//! Wave 1a (2026-05-20, spec `dashboard-visual-overview`) — three Overview
//! aggregations live at the bottom of this file (`dashboard_token_summary`,
//! `dashboard_month_activity`, `dashboard_events_feed`). Onda 1
//! (`dashboard-sqlite-out-telemetria-ndjson`) deleted the SQLite source they
//! read; they now return their empty/scaffold payloads (NDJSON-backed rebuild
//! is Onda 2). The live spec views come from the `*_v2` adapter family below.
//!
//! `*_v2` adapter family that delegates to `mustard-core`.
//!
//! Each `*_v2` function is a thin adapter — it takes the cached workspace
//! event slice from [`crate::telemetry::workspace_harness_events_cached`]
//! (spec `performance-dashboard-rotas-lentas-cache`: no per-command disk
//! walk; the incremental cache re-reads only changed shards), folds it with
//! the matching projection function (`project_spec_view_with_header`,
//! `project_waves`, `project_quality`, `project_timeline`, `project_workspace`)
//! and maps the typed ViewModel into the JSON shape the frontend already
//! expects (so React contracts stay untouched). The legacy hand-rolled SQL functions
//! (`spec_card`, `spec_waves`, `spec_quality`, `spec_timeline`,
//! `workspace_summary`) were removed in Wave 2 of spec
//! `2026-05-20-sdd-domain-finalization`; the Tauri commands in `lib.rs`
//! already delegated to the `*_v2` adapters since Wave 4 of the audit.

// Onda 1 (spec `dashboard-sqlite-out-telemetria-ndjson`): the dead SQLite
// `db.rs` facade was deleted. The aggregations that depended on it (spec
// events, hygiene health, top-files, overview token/month/events) now resolve
// to their fail-open empty payloads directly; rebuilding them over the
// per-spec NDJSON sink is Onda 2. The `*_v2` adapter family below already
// reads NDJSON via `mustard_core::view::projection`.
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
    /// Digest adherence (spec `instrumentar-adesao-ao-digest-no`): whether the
    /// latest spec-scoped `analyze.digest.summary` event recorded any digest
    /// usage during ANALYZE. Folded by `spec_card_v2_with_counts`; serde
    /// default (`false`) keeps older payloads compatible.
    #[serde(default)]
    pub digest_used: bool,
    /// Companion to `digest_used`: source-file `Read`/`Grep`/`Glob` heartbeats
    /// that landed BEFORE the first digest query (all of them when the digest
    /// was never used). Serde default (`0`) keeps older payloads compatible.
    #[serde(default)]
    pub source_reads_before_digest: i64,
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
    /// Short one-line summary of the wave, parsed from the `wave-plan.md`
    /// `Summary` column. The `project_waves` event projection never reads the
    /// markdown, so this is filled in by `spec_waves_v2` from disk. Optional +
    /// serde default keeps older payloads compatible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
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

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
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

// Onda 1: `workspace_health_impl`, `spec_events`, and the entire `spec_action`
// family (`spec_action` + `reopen_target_status` + `emit_pipeline_status` +
// `emit_pipeline_removed` + `sync_spec_status_header`) were reachable only
// through the deleted SQLite `with_db` gate, which always short-circuited.
// They are removed here.
//
// Onda 2 rebuilt these in `lib.rs` over the NDJSON sink:
//   * `dashboard_spec_events`   → reshapes `spec_timeline_v2` per spec.
//   * `dashboard_spec_action`   → reopen/close/remove now emit `pipeline.status`
//                                 via `lib_emit_pipeline_status` (no longer the
//                                 "banco de dados indisponível" error fallback).
//   * `workspace_health`        → honest FS-walk + `hygiene.*` rollup.
// The live status writes also still go through `dashboard_spec_complete` /
// `_cancel` / `_reactivate`, which call `crate::lib_emit_ndjson`.

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
    // Build the attributed per-spec counts (folds spec-less session events) for
    // just this spec and delegate. `dashboard_active_pipelines` lists many specs
    // and builds the map ONCE, then calls `spec_card_v2_with_counts` per row.
    let counts = crate::telemetry::attributed_spec_counts(&std::path::PathBuf::from(repo_path));
    spec_card_v2_with_counts(repo_path, spec, &counts)
}

/// `spec_card_v2` over a pre-built attributed-counts map (see
/// [`crate::telemetry::attributed_spec_counts`]). Callers that render many
/// cards build the map once and pass it in so the workspace event log is walked
/// a single time rather than per row.
pub(crate) fn spec_card_v2_with_counts(
    repo_path: &str,
    spec: &str,
    counts: &std::collections::HashMap<String, crate::telemetry::AttributedSpecCounts>,
) -> Result<Option<SpecCard>, String> {
    let project = std::path::PathBuf::from(repo_path);
    // Cached workspace slice (spec `performance-dashboard-rotas-lentas-cache`):
    // the spec-detail route fans 5 commands out in parallel — each used to
    // re-walk ~10k NDJSON shards via `read_workspace_events`. The shared
    // incremental cache parses a shard once and serves the burst from memory.
    let events = crate::telemetry::workspace_harness_events_cached(&project);
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
    let mut card = spec_card_from_view(&view, children_count);
    // Prefer the meta.json lifecycle status (the project's canonical lifecycle
    // source) over the event-derived one. The event stream can lag the terminal
    // transition — a spec whose `meta.json` reads `stage=Close, outcome=Completed`
    // may have an event log that only reached `closed-followup` (no terminal
    // `pipeline.status: completed` was emitted). `meta_status_word` honours
    // stage+outcome+flags via `mustard_core::domain::meta::status_word`. Falls
    // back to the event-based `status` already on the card when meta.json is
    // absent/unreadable.
    if let Some(meta_status) = meta_status_word(&spec_md) {
        card.status = meta_status;
    }
    // Reconcile the card's stepper *phase* against `meta.json` the same way the
    // status word is. The event-fold `view.phase` stalls at `plan` because the
    // rt never emits a parent `pipeline.phase = EXECUTE` once a spec starts
    // executing, while `meta.json` already reads `phase = EXECUTE`. Without this
    // the card lights "Planejar" while the LIST shows "Executando". Forward-only
    // — a card already at QA/Close is never regressed. No meta.json → unchanged.
    if let Some(meta) = mustard_core::domain::meta::read_meta_beside(&spec_md) {
        card.phase = reconcile_phase_with_meta(&meta, &card.phase);
    }
    merge_attributed_counts(&mut card, counts.get(spec));
    // Digest adherence (spec `instrumentar-adesao-ao-digest-no`): fold the
    // latest `analyze.digest.summary` event for this spec. The payload keys
    // are camelCase (`digestUsed`, `sourceReadsBeforeDigest`) — emitted by
    // `mustard-rt run digest-adherence-finalize`. No event → the struct
    // defaults (digest_used=false, 0 reads) already on the card stand.
    if let Some(payload) = events
        .iter()
        .filter(|e| e.event == "analyze.digest.summary")
        .filter(|e| e.spec.as_deref() == Some(spec))
        .max_by(|a, b| a.ts.cmp(&b.ts))
        .map(|e| &e.payload)
    {
        card.digest_used = payload
            .get("digestUsed")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        card.source_reads_before_digest = payload
            .get("sourceReadsBeforeDigest")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
    }
    Ok(Some(card))
}

/// Derive the lifecycle status word for the spec whose `spec.md` is `spec_md`
/// from its sidecar `meta.json` — the project's canonical lifecycle source
/// (`stage` + `outcome` + `flags`). Delegates to
/// `mustard_core::domain::meta::status_word`, which maps e.g.
/// `(Close, Completed) → "completed"` and `followup_open → "closed-followup"`.
///
/// Returns `None` (so the caller keeps the event-derived status) when the
/// sidecar is absent/unreadable, OR when `status_word` yields the empty string
/// (its `_ => ""` arm, i.e. a non-terminal Plan/Active meta with no qualifier):
/// in that case the event stream is the better signal for the in-flight phase.
fn meta_status_word(spec_md: &std::path::Path) -> Option<String> {
    let meta = mustard_core::domain::meta::read_meta_beside(spec_md)?;
    let word = mustard_core::domain::meta::status_word(&meta);
    let reconciled = reconcile_status_with_phase(&meta, word);
    if reconciled.is_empty() {
        return None;
    }
    Some(reconciled.to_string())
}

/// Forward-only reconciliation of the lifecycle `stage` against the `phase`
/// token both carried by `meta.json` (DEFECT 1 relief).
///
/// The rt writer can advance `meta.phase` to `"EXECUTE"` while leaving
/// `meta.stage` at `"Plan"` (the phase/stage write are not atomic; the stage
/// fix lands in rt separately). `mustard_core::status_word` keys off `stage`
/// alone, so a spec mid-execution renders "PLANEJANDO". Here we promote the
/// displayed status when `phase` is strictly MORE ADVANCED than `stage`, never
/// regressing a later stage.
///
/// Authoritative cases are left untouched: any terminal / qualifier word
/// (`completed`, `blocked`, `wave-failed`, `rejected`, `closed-followup`) wins
/// — those come from `outcome`/`flags`, not the in-flight `phase`. Only the
/// non-terminal words (`""` = queued/Plan, `"implementing"` = Execute) are
/// eligible for promotion, and only ever upward in the pipeline order.
fn reconcile_status_with_phase(
    meta: &mustard_core::domain::meta::Meta,
    word: &str,
) -> String {
    // Only the two non-terminal status words may be promoted; a terminal /
    // qualifier word is authoritative and returned verbatim.
    if !matches!(word, "" | "implementing") {
        return word.to_string();
    }
    // Rank the stage word the `status_word` reflects vs. the phase token. Both
    // map onto the same pipeline order so we can take the more-advanced one.
    let Some(phase_rank) = meta.phase.as_deref().and_then(phase_rank) else {
        return word.to_string();
    };
    // `word == "implementing"` already implies stage==Execute; `""` is anything
    // up to Plan. Derive the stage rank from the canonical `stage` field so a
    // future stage spelling stays correct.
    let stage_rank = meta
        .stage
        .as_deref()
        .and_then(stage_rank)
        .unwrap_or(0);
    if phase_rank <= stage_rank {
        return word.to_string();
    }
    // Phase is ahead of stage — render the phase's stage word. We only promote
    // up to Execute ("implementing"); QA/Close phases keep the event-/outcome-
    // derived terminal handling rather than inventing a non-terminal label.
    match phase_rank {
        r if r >= EXECUTE_RANK => "implementing".to_string(),
        _ => word.to_string(),
    }
}

/// Forward-only reconciliation of the card's *displayed phase* string against
/// `meta.json` (DEFECT 1 relief, card side).
///
/// The card's `phase` is mapped straight from `view.phase`, the event-fold
/// projection. That projection stalls at `plan` because the rt never emits a
/// parent `pipeline.phase = EXECUTE` once a Light/Full spec starts executing —
/// only `meta.json` advances (`stage=Plan`, `phase=EXECUTE`). The LIST status
/// already reconciles via [`reconcile_status_with_phase`]; this brings the
/// card's stepper node in line so it lights "Executar", not "Planejar".
///
/// `card_phase` is the lowercase phase string already on the card (`""` when
/// the view carried no phase). We promote it to the more-advanced of
/// {`meta.phase`, `meta.stage`, `card_phase`} — strictly forward-only, so a
/// card already at QA/Close is never regressed by a stale earlier token.
/// Returns the (possibly promoted) lowercase phase string.
fn reconcile_phase_with_meta(
    meta: &mustard_core::domain::meta::Meta,
    card_phase: &str,
) -> String {
    // Highest pipeline rank seen across the card's current phase, the meta
    // stage, and the meta phase — each contributes only when it parses.
    let card_rank = phase_rank(card_phase).unwrap_or(0);
    let stage_r = meta.stage.as_deref().and_then(stage_rank).unwrap_or(0);
    let phase_r = meta.phase.as_deref().and_then(phase_rank).unwrap_or(0);
    let best = card_rank.max(stage_r).max(phase_r);
    // Nothing ranked, or the card is already at/ahead of every signal → keep it.
    if best == 0 || best <= card_rank {
        return card_phase.to_string();
    }
    // Render the canonical phase word for the winning rank. Falls back to the
    // card's own phase if the rank has no phase spelling (it always does for
    // 1..=5, so this is a defensive no-op).
    phase_word_for_rank(best).unwrap_or(card_phase).to_string()
}

/// Inverse of [`phase_rank`]: the canonical lowercase phase token for a pipeline
/// rank, matching the spellings [`phase_string`] emits so the card and the
/// stepper agree. `None` for ranks outside the known 1..=5 range.
const fn phase_word_for_rank(rank: u8) -> Option<&'static str> {
    match rank {
        1 => Some("analyze"),
        2 => Some("plan"),
        3 => Some("execute"),
        4 => Some("qa"),
        5 => Some("close"),
        _ => None,
    }
}

/// Pipeline order rank for a canonical `stage` word (case-insensitive). Mirrors
/// `mustard_core::Stage::parse`'s legacy synonyms so a stage carrying
/// `"implementing"`/`"reviewing"` still ranks correctly. `None` for unknowns.
fn stage_rank(stage: &str) -> Option<u8> {
    match stage.trim().to_ascii_lowercase().as_str() {
        "analyze" => Some(1),
        "plan" | "planning" | "draft" | "approved" => Some(2),
        "execute" | "implementing" | "in-progress" | "in_progress" => Some(EXECUTE_RANK),
        "qa-review" | "qa_review" | "qareview" | "review" | "reviewing" | "qa" => Some(4),
        "close" => Some(5),
        _ => None,
    }
}

/// Pipeline order rank for a `phase` token (`ANALYZE`/`PLAN`/`EXECUTE`/`QA`/
/// `CLOSE`, case-insensitive). Aligned with [`stage_rank`] so the two are
/// directly comparable.
fn phase_rank(phase: &str) -> Option<u8> {
    match phase.trim().to_ascii_lowercase().as_str() {
        "analyze" => Some(1),
        "plan" => Some(2),
        "execute" => Some(EXECUTE_RANK),
        "qa" | "qa-review" | "review" => Some(4),
        "close" => Some(5),
        _ => None,
    }
}

/// Rank of the EXECUTE stage/phase — the promotion floor for "implementing".
const EXECUTE_RANK: u8 = 3;

/// Merge the attributed per-spec activity counts into a [`SpecCard`] built from
/// the `mustard_core` projection. The core fold keys on `event.spec` and so
/// misses spec-less session work (`tool.use` / `agent.*` under
/// `.claude/.session/{id}/.events/`); these counts include it.
///
/// We never *lower* a field — every attributed total is a superset of, or an
/// independent source for, the matching core value, so taking the larger keeps
/// explicit-spec events from being double-counted while surfacing the session
/// events the core fold dropped:
///   * `tools_used`     — attributed `tool.use` ⊇ explicit-spec `tool.use`.
///   * `files_touched`  — core counts `pipeline.task.complete.files_modified`;
///                        attributed counts distinct `tool.use` file targets.
///                        Different sources → max keeps the richer signal.
///   * `last_event_at`  — later of the two ISO timestamps.
///
/// `ac_passed`/`ac_total` (from `qa.result` — bound, explicit-spec events),
/// `status`/`phase`/`scope`/`current_wave`/`total_waves` (from attributed
/// `pipeline.*` lifecycle events) and `children_count` are left untouched: they
/// do not lose session events. No counts for this spec → card unchanged.
fn merge_attributed_counts(
    card: &mut SpecCard,
    counts: Option<&crate::telemetry::AttributedSpecCounts>,
) {
    let Some(c) = counts else { return };
    card.tools_used = card.tools_used.max(i64::from(c.tools_used));
    card.files_touched = card.files_touched.max(i64::from(c.files_touched));
    // Prefer the later non-empty timestamp; never blank an existing one.
    match (card.last_event_at.as_deref(), c.last_event_at.as_deref()) {
        (Some(cur), Some(attr)) if attr > cur => {
            card.last_event_at = Some(attr.to_string());
        }
        (None, Some(attr)) => card.last_event_at = Some(attr.to_string()),
        _ => {}
    }
}

/// W8A-2 adapter: build the wave list via `mustard-core` projections.
/// Empty `Vec` when the spec has no wave events.
///
/// Enrichment (dashboard layer): the event projection (`project_waves`) is
/// event-only and never reads the markdown, so the wave `role`/`summary` are
/// absent when no `pipeline.task.*` event carries them — the UI shows a bare
/// "ONDA N". We merge the `role` + `summary` parsed from the spec's
/// `wave-plan.md` table (and the `wave-N-{role}` dir name as a role fallback)
/// keyed by wave number, never overwriting a non-empty event-derived role.
pub fn spec_waves_v2(repo_path: &str, spec: &str) -> Result<Vec<SpecWave>, String> {
    let project = std::path::PathBuf::from(repo_path);
    let events = crate::telemetry::workspace_harness_events_cached(&project);
    let waves = mustard_core::view::projection::project_waves(spec, &events);
    let meta = wave_plan_meta(&project, spec);
    // DEFECT 2 relief: which wave numbers are running but not yet completed.
    // The core `project_waves` only promotes a wave to `InProgress` from a
    // `pipeline.task.dispatch`; a wave that surfaces solely through activity
    // events (`tool.use` / `agent.start` attributed to it) — or an explicit
    // `pipeline.wave.start` the core fold doesn't yet consume — stays `Queued`,
    // so the UI can't tell which wave is live. Derive the set here and promote.
    let running = running_wave_numbers(spec, &events);
    Ok(waves
        .iter()
        .map(|w| {
            let mut row = spec_wave_from_view(w);
            if let Some(info) = meta.get(&row.wave) {
                if row.role.as_deref().map_or(true, str::is_empty) {
                    row.role = info.role.clone();
                }
                if row.summary.is_none() {
                    row.summary = info.summary.clone();
                }
            }
            // Promote a still-queued wave to in_progress when it has live
            // activity. Never downgrade completed/failed/already-in_progress.
            if row.status == "queued"
                && u32::try_from(row.wave).is_ok_and(|n| running.contains(&n))
            {
                row.status = "in_progress".into();
            }
            row
        })
        .collect())
}

/// Wave numbers with live activity but no terminal `pipeline.wave.complete` /
/// `pipeline.wave.failed` event for this spec (DEFECT 2 relief).
///
/// A wave counts as "running" when, for this spec, the event stream carries an
/// explicit `pipeline.wave.start` for it, OR any activity event attributable to
/// the wave number — a `tool.use` / `agent.start` (`HarnessEvent.wave`), or a
/// `pipeline.task.dispatch` / `pipeline.task.complete` whose `payload.wave`
/// matches — and there is NO `pipeline.wave.complete` / `pipeline.wave.failed`
/// for that number. Terminal waves are excluded so a completed wave never
/// flickers back to in_progress.
///
/// `wave == 0` is the "outside a wave plan" sentinel and is ignored; only
/// real (1-based) wave numbers are returned.
fn running_wave_numbers(
    spec: &str,
    events: &[mustard_core::domain::model::event::HarnessEvent],
) -> std::collections::HashSet<u32> {
    use std::collections::HashSet;
    let mut active: HashSet<u32> = HashSet::new();
    let mut terminal: HashSet<u32> = HashSet::new();
    for ev in events.iter().filter(|e| e.spec.as_deref() == Some(spec)) {
        // Wave number: prefer the typed `payload.wave` of pipeline events,
        // else the record-level `HarnessEvent.wave` carried by work events.
        let payload_wave = ev
            .payload
            .get("wave")
            .and_then(serde_json::Value::as_u64)
            .and_then(|w| u32::try_from(w).ok());
        let record_wave = (ev.wave != 0).then_some(ev.wave);
        match ev.event.as_str() {
            "pipeline.wave.complete" | "pipeline.wave.failed" => {
                if let Some(w) = payload_wave.or(record_wave) {
                    terminal.insert(w);
                }
            }
            "pipeline.wave.start"
            | "pipeline.task.dispatch"
            | "pipeline.task.complete" => {
                if let Some(w) = payload_wave.or(record_wave) {
                    active.insert(w);
                }
            }
            "tool.use" | "agent.start" => {
                if let Some(w) = record_wave.or(payload_wave) {
                    active.insert(w);
                }
            }
            _ => {}
        }
    }
    active.retain(|w| *w != 0 && !terminal.contains(w));
    active
}

/// Wave 3 (2026-06-10, spec `checklist-progresso-por-onda`) — per-wave
/// checklist progress for one spec. Wave `0` is the spec's own sidecar
/// (items outside a wave plan); waves `1..` map to the `wave-N-{role}/`
/// sidecars. `total` counts the trackable items seeded in
/// `meta.json#checklist`; `done` is the live completion signal.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "snake_case")]
pub struct WaveChecklistProgress {
    pub wave: i64,
    pub done: i64,
    pub total: i64,
}

/// Fold the per-wave checklist progress (`done`/`total`) for `spec` from two
/// sources, events-first:
///
///   * **Totals (+ done floor)** — the `meta.json#checklist` sidecars (the
///     canonical home, seeded per wave by `wave-scaffold`): the spec's own
///     sidecar is wave `0`, each `wave-N-{role}/meta.json` is wave `N`.
///   * **Live `done` signal** — distinct `checklist.item.marked` NDJSON
///     events for this spec, grouped by their payload `wave`. The events
///     land before the dashboard re-reads disk (the watcher invalidates on
///     `.events/*.ndjson` writes), so progress updates without polling.
///
/// `done = max(meta done, distinct marked events)`, clamped to `total` when a
/// total is known — stale events from a re-seeded checklist never overshoot.
/// A wave seen only through events (legacy markdown checklist, no sidecar)
/// keeps `total = 0`; the frontend renders the done count without inventing a
/// denominator. Fail-open: no sidecars and no events → empty vec.
pub fn spec_checklist_progress_v2(
    repo_path: &str,
    spec: &str,
) -> Result<Vec<WaveChecklistProgress>, String> {
    if spec.is_empty() || spec.contains('/') || spec.contains('\\') || spec.contains("..") {
        return Err(format!("invalid spec name: {spec}"));
    }
    let project = std::path::PathBuf::from(repo_path);
    let spec_dir = project.join(".claude").join("spec").join(spec);

    // wave -> (total, done) from the meta.json sidecars.
    let mut per_wave: std::collections::HashMap<i64, (i64, i64)> =
        std::collections::HashMap::new();
    let mut add_meta = |wave: i64, dir: &std::path::Path| {
        let Some(meta) = mustard_core::domain::meta::read_meta(&dir.join("meta.json")) else {
            return;
        };
        if meta.checklist.is_empty() {
            return;
        }
        let total = i64::try_from(meta.checklist.len()).unwrap_or(i64::MAX);
        let done = i64::try_from(meta.checklist.iter().filter(|i| i.done).count())
            .unwrap_or(i64::MAX);
        let entry = per_wave.entry(wave).or_insert((0, 0));
        entry.0 = entry.0.saturating_add(total);
        entry.1 = entry.1.saturating_add(done);
    };
    add_meta(0, &spec_dir);
    if let Ok(rd) = fs::read_dir(&spec_dir) {
        for entry in rd {
            if !entry.is_dir {
                continue;
            }
            if let Some((n, _role)) = parse_wave_dir_name(&entry.file_name) {
                add_meta(n, &entry.path);
            }
        }
    }

    // wave -> distinct marked item labels from `checklist.item.marked` events.
    let events = crate::telemetry::walk_ndjson_events_cached(&project);
    let mut marked: std::collections::HashMap<i64, std::collections::HashSet<String>> =
        std::collections::HashMap::new();
    for v in events.iter() {
        if crate::telemetry::event_name_of(v)
            != mustard_core::domain::model::event::EVENT_CHECKLIST_ITEM_MARKED
        {
            continue;
        }
        let payload = v.get("payload");
        // Payloads are self-contained (spec + wave repeated on purpose);
        // fall back to the envelope's correlation fields for sparse lines.
        let ev_spec = payload
            .and_then(|p| p.get("spec"))
            .or_else(|| v.get("spec"))
            .and_then(|s| s.as_str())
            .unwrap_or("");
        if ev_spec != spec {
            continue;
        }
        let wave = payload
            .and_then(|p| p.get("wave"))
            .or_else(|| v.get("wave"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        let Some(item) = payload
            .and_then(|p| p.get("item"))
            .and_then(|s| s.as_str())
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        marked.entry(wave).or_default().insert(item.to_string());
    }

    // Union of waves seen in either source, ascending.
    let mut waves: std::collections::BTreeSet<i64> = per_wave.keys().copied().collect();
    waves.extend(marked.keys().copied());
    Ok(waves
        .into_iter()
        .map(|w| {
            let (total, done_meta) = per_wave.get(&w).copied().unwrap_or((0, 0));
            let done_events = i64::try_from(
                marked.get(&w).map_or(0, std::collections::HashSet::len),
            )
            .unwrap_or(i64::MAX);
            let mut done = done_meta.max(done_events);
            if total > 0 {
                done = done.min(total);
            }
            WaveChecklistProgress { wave: w, done, total }
        })
        .collect())
}

/// W8A-2 adapter: AC roll-up via `mustard-core` projections.
///
/// Enrichment (dashboard layer): `project_quality` reads each AC's `label` from
/// the `qa.result` event, falling back to the bare id when the event carries no
/// label (the common case — the QA writer does not embed the criterion text).
/// That is why the UI shows "AC-1 AC-1". We parse the criterion descriptions
/// out of the spec's `spec.md` `## Acceptance Criteria` / `## Critérios de
/// Aceitação` section and override the label when the projection only had the
/// id. The pass/fail status and command are untouched.
pub fn spec_quality_v2(repo_path: &str, spec: &str) -> Result<Vec<SpecQualityItem>, String> {
    let project = std::path::PathBuf::from(repo_path);
    let events = crate::telemetry::workspace_harness_events_cached(&project);
    let rollup = mustard_core::view::projection::project_quality(spec, &events);
    let descriptions = ac_descriptions(&project, spec);
    Ok(rollup
        .criteria
        .iter()
        .map(|c| {
            let mut item = quality_item_from_view(c);
            // Replace the label only when the projection fell back to the bare
            // id (`label == id`, surfaced here as `ac_label == Some(ac_id)`):
            // an event-supplied label always wins.
            let is_bare_id = item.ac_label.as_deref() == Some(item.ac_id.as_str())
                || item.ac_label.is_none();
            if is_bare_id {
                if let Some(text) = descriptions.get(&item.ac_id) {
                    item.ac_label = Some(text.clone());
                }
            }
            item
        })
        .collect())
}

/// Parse the spec's `## Acceptance Criteria` / `## Critérios de Aceitação`
/// section out of `spec.md` into an `id → description` map. Mirrors the rt-side
/// AC parser (`apps/rt/src/commands/review/qa_run.rs`) but extracts the
/// **description** (text between the id→desc separator and any inline
/// `Command:` marker) rather than the command. Duplicated rather than imported
/// because `mustard-rt` is a binary crate (a workspace dep cycle otherwise).
///
/// Fail-open: a missing/unreadable `spec.md` or an absent section yields an
/// empty map, so the caller keeps the bare-id labels.
fn ac_descriptions(
    project: &std::path::Path,
    spec: &str,
) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let spec_md = project.join(".claude").join("spec").join(spec).join("spec.md");
    let Ok(text) = fs::read_to_string(&spec_md) else {
        return out;
    };
    let Some(section) = ac_section_body(&text) else {
        return out;
    };
    for line in section.lines() {
        if let Some((id, desc)) = parse_ac_description(line) {
            // First occurrence wins (the canonical block lists each id once).
            out.entry(id).or_insert(desc);
        }
    }
    out
}

/// Extract the body of the `## Acceptance Criteria` (EN) / `## Critérios de
/// Aceitação` (PT) section — every line from the heading (exclusive) up to the
/// next `## ` heading or EOF. Heading match mirrors `spec_sections::is_heading`
/// for the `acceptance-criteria` key.
fn ac_section_body(markdown: &str) -> Option<String> {
    const HEADINGS: [&str; 2] = ["acceptance criteria", "critérios de aceitação"];
    let lines: Vec<&str> = markdown.split('\n').collect();
    let start = lines.iter().position(|l| {
        let Some(rest) = l.strip_prefix("##") else { return false };
        let after_ws = rest.trim_start_matches([' ', '\t']);
        if after_ws.len() == rest.len() {
            return false; // `##` with no following whitespace is not a heading
        }
        let lower = after_ws.to_lowercase();
        HEADINGS.iter().any(|h| {
            lower.strip_prefix(h).is_some_and(|tail| {
                // Word boundary after the heading name.
                tail.chars()
                    .next()
                    .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'))
            })
        })
    })?;
    let mut end = lines.len();
    for (i, l) in lines.iter().enumerate().skip(start + 1) {
        if l.starts_with("## ") {
            end = i;
            break;
        }
    }
    Some(lines[start + 1..end].join("\n"))
}

/// Parse one AC line into `(id, description)`. Recognises the drafter's
/// canonical `- **AC-1** — desc.` shape and the historical `- [ ] AC-1: desc …`
/// shape. The description is the text after the id→desc separator, with any
/// inline `Command:` marker (and everything after it) trimmed off, and any
/// trailing `**` bold close stripped. Returns `None` for non-AC lines.
fn parse_ac_description(line: &str) -> Option<(String, String)> {
    let t = line.trim_start();
    let rest = t.strip_prefix('-')?.trim_start();
    // Optional `[ ]` / `[x]` / `[X]` checkbox.
    let rest = match rest.strip_prefix('[') {
        Some(after_open) => {
            let mark = after_open.chars().next()?;
            if !matches!(mark, ' ' | 'x' | 'X') {
                return None;
            }
            after_open[mark.len_utf8()..].strip_prefix(']')?.trim_start()
        }
        None => rest,
    };
    // Optional leading bold `**`.
    let (rest, bold) = match rest.strip_prefix("**") {
        Some(r) => (r.trim_start(), true),
        None => (rest, false),
    };
    if !rest.to_lowercase().starts_with("ac-") {
        return None;
    }
    let after_ac = &rest[3..];
    // ID = `[A-Za-z0-9]+(-[A-Za-z0-9]+)*`.
    let first_end = after_ac
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(after_ac.len());
    if first_end == 0 {
        return None;
    }
    let mut id_end = first_end;
    loop {
        let tail = &after_ac[id_end..];
        if !tail.starts_with('-') {
            break;
        }
        let seg_len = tail[1..]
            .find(|c: char| !c.is_ascii_alphanumeric())
            .unwrap_or(tail[1..].len());
        if seg_len == 0 {
            break;
        }
        id_end += 1 + seg_len;
    }
    let id = format!("AC-{}", &after_ac[..id_end]).to_uppercase();
    let after_id = &after_ac[id_end..];
    // Strip the id→desc separator (`.`/`:`/`—`/`--`/`-`), handling the bold
    // shapes where `**` may close before or after the separator.
    let after_sep: &str = if bold {
        let stripped = after_id.trim_start();
        if let Some(r) = strip_ac_separator(stripped) {
            r.trim_start().strip_prefix("**").unwrap_or(r)
        } else if let Some(r) = stripped.strip_prefix("**") {
            strip_ac_separator(r.trim_start())?
        } else {
            return None;
        }
    } else {
        strip_ac_separator(after_id.trim_start())?
    };
    // Trim any inline `Command:` marker (and the rest of the line) and any
    // dangling bold close, then normalise whitespace.
    let mut desc = after_sep;
    if let Some(idx) = desc.to_lowercase().find("command:") {
        desc = &desc[..idx];
    }
    let desc = desc.trim().trim_end_matches("**").trim();
    if desc.is_empty() {
        return None;
    }
    Some((id, desc.to_string()))
}

/// Strip the AC id→description separator from the front of `s`. Accepts `.`,
/// `:`, the em-dash `—`, `--`, or a single `-`. Mirrors `strip_separator` in
/// the rt-side qa_run parser.
fn strip_ac_separator(s: &str) -> Option<&str> {
    if let Some(rest) = s.strip_prefix('.').or_else(|| s.strip_prefix(':')) {
        return Some(rest);
    }
    if let Some(rest) = s.strip_prefix('—') {
        return Some(rest);
    }
    s.strip_prefix("--").or_else(|| s.strip_prefix('-'))
}

/// Role + summary for one wave, parsed from `wave-plan.md`.
struct WavePlanInfo {
    role: Option<String>,
    summary: Option<String>,
}

/// Parse the spec's `wave-plan.md` table into a `wave-number → (role, summary)`
/// map. The table row shape is `| N | [[wave-N-role]] | role | deps | summary |`
/// (see `apps/rt/src/commands/wave/wave_scaffold.rs::render_wave_plan`). The
/// `role` column is column 3 and `summary` is the last column; an escaped `\|`
/// inside a summary is unescaped. The `wave-N-{role}` dir name is also scanned
/// as a role fallback for waves whose table row is missing a role.
///
/// Fail-open: a missing/unreadable `wave-plan.md` yields an empty map.
fn wave_plan_meta(
    project: &std::path::Path,
    spec: &str,
) -> std::collections::HashMap<i64, WavePlanInfo> {
    let mut out: std::collections::HashMap<i64, WavePlanInfo> = std::collections::HashMap::new();
    let spec_dir = project.join(".claude").join("spec").join(spec);

    // Role fallback from the on-disk `wave-N-{role}/` directory names.
    if let Ok(rd) = fs::read_dir(&spec_dir) {
        for entry in rd {
            if !entry.is_dir {
                continue;
            }
            if let Some((n, role)) = parse_wave_dir_name(&entry.file_name) {
                out.entry(n).or_insert(WavePlanInfo {
                    role: Some(role),
                    summary: None,
                });
            }
        }
    }

    // Table rows from `wave-plan.md` — authoritative for role + the summary.
    if let Ok(text) = fs::read_to_string(&spec_dir.join("wave-plan.md")) {
        for line in text.lines() {
            let Some((n, role, summary)) = parse_wave_plan_row(line) else {
                continue;
            };
            let info = out.entry(n).or_insert(WavePlanInfo {
                role: None,
                summary: None,
            });
            if role.is_some() {
                info.role = role;
            }
            if summary.is_some() {
                info.summary = summary;
            }
        }
    }

    out
}

/// Parse a `wave-{N}-{role}` directory name into `(N, role)`. Mirrors the
/// matching logic in `dashboard_spec_waves_planned_run`.
fn parse_wave_dir_name(name: &str) -> Option<(i64, String)> {
    let rest = name.strip_prefix("wave-")?;
    let dash_idx = rest.find('-').filter(|&i| i > 0)?;
    let (num_str, role_with_dash) = rest.split_at(dash_idx);
    let role = &role_with_dash[1..];
    if role.is_empty() {
        return None;
    }
    let n: i64 = num_str.parse().ok()?;
    Some((n, role.to_string()))
}

/// Parse one `| N | [[link]] | role | deps | summary |` markdown table row into
/// `(wave_number, role, summary)`. Returns `None` for the header/separator rows
/// and any non-data line (a row whose first cell is not a bare integer).
fn parse_wave_plan_row(line: &str) -> Option<(i64, Option<String>, Option<String>)> {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') {
        return None;
    }
    // Split on unescaped `|`. Cells are the pieces between the outer pipes.
    let cells: Vec<&str> = trimmed.trim_matches('|').split('|').map(str::trim).collect();
    // Expect at least: wave | spec | role | deps | summary.
    if cells.len() < 5 {
        return None;
    }
    // Column 0 must be a bare wave number (skips the header `Wave` + the
    // `---|---` separator row).
    let n: i64 = cells[0].parse().ok()?;
    let role = Some(cells[2])
        .map(str::to_string)
        .filter(|r| !r.is_empty() && r != "—" && r != "-");
    let summary = cells
        .last()
        .map(|s| s.replace("\\|", "|"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "—" && s != "-");
    Some((n, role, summary))
}

/// W8A-2 adapter: timeline projection via `mustard-core` projections.
/// `All` window; the dashboard does its own client-side filtering when it
/// needs a narrower view.
pub fn spec_timeline_v2(repo_path: &str, spec: &str) -> Result<Vec<SpecTimelineNode>, String> {
    let project = std::path::PathBuf::from(repo_path);
    let events = crate::telemetry::workspace_harness_events_cached(&project);
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
    // Cached + complete slice (spec/wave/session sinks) — see
    // `workspace_harness_events_cached`. Session-attributed activity that the
    // old spec-only core walk missed now counts toward the workspace totals.
    let events = crate::telemetry::workspace_harness_events_cached(&project);
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

    // Onda 1: the legacy session-agnostic SQLite override of `top_files_today`
    // is gone (it short-circuited to a no-op once `db.rs` became a facade).
    // `out.top_files_today` keeps the value from the mustard-core NDJSON
    // projection (`project_workspace`) above. A faithful session-agnostic
    // NDJSON ranking is Onda 2.
    Ok(out)
}

/// Returns true for spec statuses that represent a finished / parked pipeline
/// — those rows should not appear in the "PIPELINES ATIVOS" hero list.
/// Centralised here so the same set is reused if other commands ever need
/// the same predicate. Kebab-case strings mirror `spec_status_string`.
fn is_terminal_status(status: &str) -> bool {
    matches!(status, "completed" | "closed-followup" | "cancelled")
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
        // Folded from the latest `analyze.digest.summary` event by
        // `spec_card_v2_with_counts` after this mapper runs; the core
        // projection carries no digest-adherence signal.
        digest_used: false,
        source_reads_before_digest: 0,
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
        // Enriched from `wave-plan.md` by `spec_waves_v2` after this mapper runs;
        // the event projection carries no summary.
        summary: None,
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

/// Wave-3 — the `wikilink-extract` verb never shipped in `mustard-rt` (audit
/// 2026-07-07: no matching `RunCmd` variant ever existed), so every spawn here
/// failed and degraded to the empty extract. Return the empty extract directly
/// instead of paying a doomed subprocess per render — the frontend keeps
/// rendering the exact same empty state it always did.
pub fn dashboard_wikilink_extract_run(
    _repo_path: &str,
    _spec_name: &str,
) -> Result<WikilinkExtract, String> {
    Ok(WikilinkExtract::default())
}

/// Cross-wave memory for a spec, sourced from the LIVE unified knowledge store
/// (markdown rows under `.claude/memory/`, ranked by the recall decay curve) via
/// `mustard-rt run memory search --spec <name>`. The dead `memory cross-wave`
/// verb (which read the removed `agent.memory` SQLite path) no longer exists; the
/// `search` verb is the live equivalent — it returns every active memory row
/// carrying this spec across ALL its waves (`--spec` filters on the frontmatter
/// `spec` field, so wave-scoped summaries surface here), ranked by effective
/// confidence. `search` emits a JSON array of rows; this function renders that
/// into a compact markdown block (rendering is a dashboard concern — the ranking
/// is owned by rt and reused as-is). The `wave` parameter is no longer consulted:
/// the live verb has no per-wave reach — it returns the spec's full accumulated
/// memory and the drawer shows priors regardless of the current wave.
///
/// Empty string when the spec has no memory rows (the common case — early waves
/// carry no priors). `Err` is reserved for spawn failures, matching the prior
/// contract; an unparseable payload degrades to an empty block.
pub fn dashboard_memory_cross_wave_run(
    repo_path: &str,
    spec: &str,
    _wave: u32,
) -> Result<String, String> {
    if spec.is_empty() || spec.contains('/') || spec.contains('\\') || spec.contains("..") {
        return Err(format!("invalid spec name: {spec}"));
    }
    let mut cmd = mustard_rt_command(&["run", "memory", "search", "--spec", spec]);
    cmd.current_dir(repo_path);
    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => return Err(format!("spawn mustard-rt: {e}")),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(render_memory_rows_markdown(slice_json_array(&stdout)))
}

/// One row of `mustard-rt run memory search` output. Mirrors the producer's
/// `SearchRow` (knowledge/memory.rs); only the fields the drawer renders are
/// decoded, and every field tolerates absence so a shape change never makes the
/// drawer error (it just renders less). Field names are the JSON keys the
/// producer emits (snake_case via serde default).
#[derive(Deserialize, Default)]
struct MemorySearchRow {
    #[serde(default)]
    wave: Option<i64>,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    details: Option<String>,
}

/// Trim leading log/banner noise so `serde_json::from_str` sees a pure JSON
/// document starting at the first `[` (the `search` payload is an array; the
/// object-oriented [`slice_json`] would stop at a stray `{` inside it).
fn slice_json_array(stdout: &str) -> &str {
    match stdout.find('[') {
        Some(i) => &stdout[i..],
        None => stdout,
    }
}

/// Render the `memory search` JSON rows into the markdown block the cross-wave
/// drawer expects. Empty (or unparseable) input yields an empty string so the
/// frontend renders its empty state, preserving the prior "empty when nothing to
/// report" contract. Each row becomes a bullet tagged with its wave/role; the
/// `details` body, when present, follows as an indented note.
fn render_memory_rows_markdown(json: &str) -> String {
    let rows: Vec<MemorySearchRow> = serde_json::from_str(json).unwrap_or_default();
    let mut out = String::new();
    for row in rows {
        let summary = row.summary.trim();
        if summary.is_empty() {
            continue;
        }
        let tag = match (row.wave, row.role.as_deref().map(str::trim).filter(|s| !s.is_empty())) {
            (Some(w), Some(r)) => format!("wave {w} · {r}"),
            (Some(w), None) => format!("wave {w}"),
            (None, Some(r)) => r.to_string(),
            (None, None) => String::new(),
        };
        if tag.is_empty() {
            out.push_str(&format!("- {summary}\n"));
        } else {
            out.push_str(&format!("- **{tag}** — {summary}\n"));
        }
        if let Some(details) = row.details.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            out.push_str(&format!("  {details}\n"));
        }
    }
    out.trim_end().to_string()
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
// for the redesigned Overview page. Onda 1
// (`dashboard-sqlite-out-telemetria-ndjson`) removed the SQLite reader these
// opened; each command now returns its empty/scaffold payload directly and
// only returns `Err` for argument-validation failures (e.g. invalid month).
// The NDJSON-backed rebuild of these projections is Onda 2.
//
// Schema notes (legacy events table — kept for the Onda 2 rebuild reference):
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

/// `dashboard_token_summary` — Onda 2: total tokens saved + top pipelines by
/// savings, folded from the NDJSON savings channel. `total_saved` comes from
/// the core `savings_breakdown` (project scope); the per-spec ranking is folded
/// directly from `pipeline.economy.savings.*` events grouped by `spec`.
/// Off-main-thread wrapper for [`dashboard_token_summary_impl`] (cold cache
/// pays the full workspace parse). A join error degrades to a zeroed summary.
#[tauri::command]
pub async fn dashboard_token_summary(project_path: String) -> Result<TokenSummary, String> {
    tauri::async_runtime::spawn_blocking(move || dashboard_token_summary_impl(project_path))
        .await
        .unwrap_or_else(|_| Ok(TokenSummary::default()))
}

fn dashboard_token_summary_impl(project_path: String) -> Result<TokenSummary, String> {
    use mustard_core::domain::economy::scope::ProjectPath as CoreProjectPath;
    use mustard_core::domain::economy::EconomyScope as CoreScope;

    let root = std::path::PathBuf::from(&project_path);
    let breakdown = mustard_core::domain::economy::savings_breakdown(
        &root,
        CoreScope::Project(CoreProjectPath::new(&root)),
    )
    .unwrap_or_default();
    let total_saved = breakdown.total_tokens_saved;

    // Per-spec savings ranking from the raw savings events (the core breakdown
    // carries no spec dimension at project scope).
    let events = crate::telemetry::walk_ndjson_events_cached(&root);
    let mut by_spec: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for v in events.iter() {
        if !crate::telemetry::event_name_of(v).starts_with("pipeline.economy.savings.") {
            continue;
        }
        let payload = v.get("payload");
        let tokens = payload
            .and_then(|p| p.get("tokens_saved"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        let spec = v
            .get("spec")
            .or_else(|| payload.and_then(|p| p.get("spec_id")))
            .and_then(|s| s.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("unattributed")
            .to_string();
        *by_spec.entry(spec).or_insert(0) += tokens;
    }
    let mut top_pipelines: Vec<TopPipeline> = by_spec
        .into_iter()
        .map(|(spec, saved)| TopPipeline { spec, saved })
        .collect();
    top_pipelines.sort_by(|a, b| b.saved.cmp(&a.saved));
    top_pipelines.truncate(10);

    Ok(TokenSummary {
        total_saved,
        top_pipelines,
    })
}

/// `dashboard_month_activity` — Onda 2: one entry per day of the given month
/// with the real event count + the day's top pipeline phase, folded from the
/// complete NDJSON walker. Days with no events keep `event_count: 0` /
/// `top_phase: None`, so the scaffold shape is preserved exactly.
/// Off-main-thread wrapper for [`dashboard_month_activity_impl`] (cold cache
/// pays the full workspace parse). A join error degrades to an empty list.
#[tauri::command]
pub async fn dashboard_month_activity(
    project_path: String,
    year: i32,
    month: u32,
) -> Result<Vec<DayActivity>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        dashboard_month_activity_impl(project_path, year, month)
    })
    .await
    .unwrap_or_else(|_| Ok(Vec::new()))
}

fn dashboard_month_activity_impl(
    project_path: String,
    year: i32,
    month: u32,
) -> Result<Vec<DayActivity>, String> {
    if !(1..=12).contains(&month) {
        return Err(format!("invalid month: {month}"));
    }
    let root = std::path::PathBuf::from(&project_path);
    let month_prefix = format!("{year:04}-{month:02}-");
    let events = crate::telemetry::walk_ndjson_events_cached(&root);

    // date -> (event_count, phase -> count)
    let mut per_day: std::collections::HashMap<String, (i32, std::collections::HashMap<String, i32>)> =
        std::collections::HashMap::new();
    for v in events.iter() {
        let ts = v.get("ts").and_then(|t| t.as_str()).unwrap_or("");
        if !ts.starts_with(&month_prefix) || ts.len() < 10 {
            continue;
        }
        let date = ts[..10].to_string();
        let entry = per_day.entry(date).or_insert((0, std::collections::HashMap::new()));
        entry.0 += 1;
        if crate::telemetry::event_name_of(v) == "pipeline.phase" {
            if let Some(p) = v
                .get("payload")
                .and_then(|p| p.get("to").or_else(|| p.get("phase")))
                .and_then(|x| x.as_str())
                .filter(|s| !s.is_empty())
            {
                *entry.1.entry(p.to_string()).or_insert(0) += 1;
            }
        }
    }

    let days_in_month = days_in_month(year, month);
    let out: Vec<DayActivity> = (1..=days_in_month)
        .map(|d| {
            let date = format!("{year:04}-{month:02}-{d:02}");
            match per_day.get(&date) {
                Some((count, phases)) => DayActivity {
                    date,
                    event_count: *count,
                    top_phase: phases
                        .iter()
                        .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
                        .map(|(p, _)| p.clone()),
                },
                None => DayActivity {
                    date,
                    event_count: 0,
                    top_phase: None,
                },
            }
        })
        .collect();
    Ok(out)
}

/// `dashboard_events_feed` — Onda 2: chronological cross-spec feed (newest
/// first) folded from the complete NDJSON walker, in the `FeedEvent` shape.
/// Off-main-thread wrapper for [`dashboard_events_feed_impl`] (cold cache
/// pays the full workspace parse). A join error degrades to an empty list.
#[tauri::command]
pub async fn dashboard_events_feed(
    project_path: String,
    limit: u32,
) -> Result<Vec<FeedEvent>, String> {
    tauri::async_runtime::spawn_blocking(move || dashboard_events_feed_impl(project_path, limit))
        .await
        .unwrap_or_else(|_| Ok(Vec::new()))
}

fn dashboard_events_feed_impl(
    project_path: String,
    limit: u32,
) -> Result<Vec<FeedEvent>, String> {
    let cap = limit.clamp(1, 1000) as usize;
    let root = std::path::PathBuf::from(&project_path);
    let events = crate::telemetry::walk_ndjson_events_cached(&root);
    let mut ordered: Vec<&serde_json::Value> = events.iter().collect();
    ordered.sort_by(|a, b| {
        let ta = a.get("ts").and_then(|t| t.as_str()).unwrap_or("");
        let tb = b.get("ts").and_then(|t| t.as_str()).unwrap_or("");
        tb.cmp(ta)
    });
    let rows: Vec<FeedEvent> = ordered
        .into_iter()
        .filter(|v| v.get("ts").and_then(|t| t.as_str()).is_some())
        .take(cap)
        .enumerate()
        .map(|(i, v)| {
            let kind = crate::telemetry::event_name_of(v).to_string();
            let ts = v.get("ts").and_then(|t| t.as_str()).unwrap_or("").to_string();
            let payload = v.get("payload");
            let spec = v
                .get("spec")
                .or_else(|| payload.and_then(|p| p.get("spec")))
                .and_then(|s| s.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let payload_summary = feed_payload_summary(&kind, payload);
            FeedEvent {
                id: format!("feed-{i}"),
                ts,
                kind,
                spec,
                payload_summary,
            }
        })
        .collect();
    Ok(rows)
}

/// ≤120-char human summary for a feed event, derived per event family from the
/// payload. Kept local to `spec_views` (the `lib.rs` `event_summary` covers the
/// `RecentEvent`-shaped feeds; this mirrors it for the `FeedEvent` shape).
fn feed_payload_summary(kind: &str, payload: Option<&serde_json::Value>) -> String {
    let s = match kind {
        "tool.use" => {
            let tool = payload
                .and_then(|p| p.get("tool").or_else(|| p.get("tool_name")))
                .and_then(|x| x.as_str())
                .unwrap_or("tool");
            let target = payload
                .and_then(|p| p.get("target"))
                .and_then(|t| t.as_object())
                .and_then(|o| {
                    o.get("file_path")
                        .or_else(|| o.get("file"))
                        .or_else(|| o.get("command"))
                        .or_else(|| o.get("description"))
                })
                .and_then(|x| x.as_str())
                .unwrap_or("");
            if target.is_empty() { tool.to_string() } else { format!("{tool} · {target}") }
        }
        "pipeline.phase" => {
            let to = payload.and_then(|p| p.get("to")).and_then(|x| x.as_str()).unwrap_or("");
            format!("→ {to}")
        }
        "pipeline.status" => {
            let to = payload.and_then(|p| p.get("to")).and_then(|x| x.as_str()).unwrap_or("");
            format!("status → {to}")
        }
        // Digest adherence (spec `instrumentar-adesao-ao-digest-no`). Payload
        // keys are camelCase — see `digest-adherence-finalize` in rt.
        "analyze.digest.summary" => {
            let used = payload
                .and_then(|p| p.get("digestUsed"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let before = payload
                .and_then(|p| p.get("sourceReadsBeforeDigest"))
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
            if used {
                format!("digest usado · {before} reads antes")
            } else {
                format!("digest não usado · {before} reads diretos")
            }
        }
        "analyze.digest.used" => {
            let terms = payload
                .and_then(|p| p.get("queryTerms"))
                .and_then(serde_json::Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(serde_json::Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            if terms.is_empty() {
                "digest consultado".to_string()
            } else {
                format!("digest consultado · {terms}")
            }
        }
        other => other.to_string(),
    };
    s.chars().take(120).collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::meta::Meta;
    use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
    use serde_json::json;

    // ── DEFECT 1: stage ⨯ phase reconciliation ───────────────────────────────

    fn meta(stage: Option<&str>, phase: Option<&str>, outcome: Option<&str>) -> Meta {
        Meta {
            stage: stage.map(str::to_string),
            phase: phase.map(str::to_string),
            outcome: outcome.map(str::to_string),
            ..Meta::default()
        }
    }

    #[test]
    fn phase_execute_promotes_plan_stage_to_implementing() {
        // The exact defect: rt advanced phase to EXECUTE but left stage=Plan.
        let m = meta(Some("Plan"), Some("EXECUTE"), Some("Active"));
        let word = mustard_core::domain::meta::status_word(&m); // "" for Plan
        assert_eq!(reconcile_status_with_phase(&m, word), "implementing");
    }

    #[test]
    fn phase_never_regresses_a_later_stage() {
        // stage already at QaReview, phase only PLAN → keep the later stage
        // (status_word("qa-review") = "" today, so reconciliation must NOT
        // invent "implementing" from the earlier phase).
        let m = meta(Some("QaReview"), Some("PLAN"), Some("Active"));
        let word = mustard_core::domain::meta::status_word(&m);
        assert_eq!(reconcile_status_with_phase(&m, word), word);
    }

    #[test]
    fn terminal_completed_is_authoritative_over_phase() {
        // A closed spec must stay "completed" even if a stale phase says EXECUTE.
        let m = meta(Some("Close"), Some("EXECUTE"), Some("Completed"));
        let word = mustard_core::domain::meta::status_word(&m); // "completed"
        assert_eq!(word, "completed");
        assert_eq!(reconcile_status_with_phase(&m, word), "completed");
    }

    #[test]
    fn blocked_flag_is_authoritative_over_phase() {
        let mut m = meta(Some("Plan"), Some("EXECUTE"), Some("Active"));
        m.flags = mustard_core::domain::meta::MetaFlags(mustard_core::Flags {
            blocked: true,
            ..mustard_core::Flags::default()
        });
        let word = mustard_core::domain::meta::status_word(&m); // "blocked"
        assert_eq!(reconcile_status_with_phase(&m, word), "blocked");
    }

    #[test]
    fn matching_phase_and_stage_is_unchanged() {
        // phase == stage (both Execute) → already "implementing", no change.
        let m = meta(Some("Execute"), Some("EXECUTE"), Some("Active"));
        let word = mustard_core::domain::meta::status_word(&m); // "implementing"
        assert_eq!(reconcile_status_with_phase(&m, word), "implementing");
    }

    #[test]
    fn card_phase_promoted_to_execute_when_meta_phase_ahead() {
        // The card defect: view.phase folds to "plan" (rt never emits parent
        // pipeline.phase=EXECUTE) but meta reads {stage:Plan, phase:EXECUTE}.
        // The card phase must resolve to "execute" so the stepper lights
        // "Executar", matching the LIST status "Executando".
        let m = meta(Some("Plan"), Some("EXECUTE"), Some("Active"));
        assert_eq!(reconcile_phase_with_meta(&m, "plan"), "execute");
    }

    #[test]
    fn card_phase_never_regressed_below_its_view() {
        // Card already at QA/Close from the event stream — a stale earlier meta
        // token must never pull the stepper back.
        let m = meta(Some("Plan"), Some("PLAN"), Some("Active"));
        assert_eq!(reconcile_phase_with_meta(&m, "qa"), "qa");
        assert_eq!(reconcile_phase_with_meta(&m, "close"), "close");
    }

    #[test]
    fn card_phase_unchanged_when_meta_blank() {
        // No parseable stage/phase in meta → keep whatever the view carried.
        let m = meta(None, None, None);
        assert_eq!(reconcile_phase_with_meta(&m, "plan"), "plan");
        assert_eq!(reconcile_phase_with_meta(&m, ""), "");
    }

    // ── DEFECT 2: in-progress wave derivation ────────────────────────────────

    fn ev(spec: &str, kind: &str, wave: u32, payload: serde_json::Value) -> HarnessEvent {
        HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-06-05T10:00:00Z".into(),
            session_id: "s".into(),
            wave,
            actor: Actor { kind: ActorKind::Hook, id: None, actor_type: None },
            event: kind.into(),
            payload,
            spec: Some(spec.into()),
        }
    }

    #[test]
    fn wave_with_activity_no_complete_is_running() {
        // A tool.use attributed to wave 2 (record-level), no completion → running.
        let events = vec![ev("alpha", "tool.use", 2, json!({ "tool": "Bash" }))];
        let running = running_wave_numbers("alpha", &events);
        assert!(running.contains(&2));
    }

    #[test]
    fn explicit_wave_start_marks_running() {
        let events = vec![ev("alpha", "pipeline.wave.start", 0, json!({ "wave": 3 }))];
        let running = running_wave_numbers("alpha", &events);
        assert!(running.contains(&3));
    }

    #[test]
    fn completed_wave_is_not_running() {
        let events = vec![
            ev("alpha", "tool.use", 1, json!({ "tool": "Read" })),
            ev("alpha", "pipeline.wave.complete", 0, json!({ "wave": 1 })),
        ];
        let running = running_wave_numbers("alpha", &events);
        assert!(!running.contains(&1), "a completed wave must not be running");
    }

    #[test]
    fn other_specs_activity_is_ignored() {
        let events = vec![ev("beta", "tool.use", 4, json!({ "tool": "Bash" }))];
        let running = running_wave_numbers("alpha", &events);
        assert!(running.is_empty());
    }

    #[test]
    fn wave_zero_sentinel_is_ignored() {
        // wave==0 means "outside a wave plan" — never a real wave row.
        let events = vec![ev("alpha", "tool.use", 0, json!({ "tool": "Bash" }))];
        let running = running_wave_numbers("alpha", &events);
        assert!(running.is_empty());
    }

    // ── Wave 3 (spec `checklist-progresso-por-onda`): per-wave progress ──────

    fn write_file(root: &std::path::Path, rel: &str, body: &str) {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, body).unwrap();
    }

    #[test]
    fn checklist_progress_folds_meta_totals_and_marked_events() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Spec's own sidecar (wave 0): 2 items, 1 already done in meta.
        write_file(
            tmp.path(),
            ".claude/spec/alpha/meta.json",
            r#"{"stage":"Execute","outcome":"Active","checklist":[{"label":"root a","done":true},{"label":"root b"}]}"#,
        );
        // Wave 1 sidecar: 3 items, none done in meta yet.
        write_file(
            tmp.path(),
            ".claude/spec/alpha/wave-1-impl/meta.json",
            r#"{"stage":"Execute","outcome":"Active","checklist":[{"label":"w1 a"},{"label":"w1 b"},{"label":"w1 c"}]}"#,
        );
        // One marked event for wave 1 — duplicated line must dedupe; an event
        // for ANOTHER spec must be ignored.
        let line = r#"{"v":1,"ts":"2026-06-10T10:00:00Z","sessionId":"s","wave":1,"actor":{"kind":"hook","id":"checklist-auto-mark"},"event":"checklist.item.marked","payload":{"spec":"alpha","wave":1,"item":"w1 a"},"spec":"alpha"}"#;
        let other = r#"{"v":1,"ts":"2026-06-10T10:00:01Z","sessionId":"s","wave":1,"actor":{"kind":"hook","id":"checklist-auto-mark"},"event":"checklist.item.marked","payload":{"spec":"beta","wave":1,"item":"x"},"spec":"beta"}"#;
        write_file(
            tmp.path(),
            ".claude/spec/alpha/.events/events.ndjson",
            &format!("{line}\n{line}\n{other}\n"),
        );

        let repo = tmp.path().to_string_lossy().into_owned();
        crate::telemetry::invalidate_events_cache(&repo);
        let rows = spec_checklist_progress_v2(&repo, "alpha").unwrap();
        assert_eq!(rows.len(), 2);
        let w0 = rows.iter().find(|r| r.wave == 0).expect("wave 0");
        assert_eq!((w0.done, w0.total), (1, 2), "meta done flag counts");
        let w1 = rows.iter().find(|r| r.wave == 1).expect("wave 1");
        assert_eq!((w1.done, w1.total), (1, 3), "deduped event marks count");
    }

    #[test]
    fn checklist_progress_empty_when_no_sidecars_and_no_events() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_file(tmp.path(), ".claude/spec/alpha/spec.md", "# alpha\n");
        let repo = tmp.path().to_string_lossy().into_owned();
        crate::telemetry::invalidate_events_cache(&repo);
        let rows = spec_checklist_progress_v2(&repo, "alpha").unwrap();
        assert!(rows.is_empty(), "no checklist data → honest empty payload");
    }

    #[test]
    fn checklist_progress_event_only_wave_has_no_invented_total() {
        // Legacy markdown checklist: events exist but no sidecar was seeded.
        let tmp = tempfile::TempDir::new().unwrap();
        let line = r#"{"event":"checklist.item.marked","ts":"2026-06-10T10:00:00Z","spec":"alpha","wave":2,"payload":{"spec":"alpha","wave":2,"item":"legacy"}}"#;
        write_file(
            tmp.path(),
            ".claude/spec/alpha/.events/events.ndjson",
            &format!("{line}\n"),
        );
        let repo = tmp.path().to_string_lossy().into_owned();
        crate::telemetry::invalidate_events_cache(&repo);
        let rows = spec_checklist_progress_v2(&repo, "alpha").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].wave, rows[0].done, rows[0].total), (2, 1, 0));
    }

    #[test]
    fn checklist_progress_clamps_done_to_total() {
        // A re-seeded (shrunk) checklist must not overshoot from stale events.
        let tmp = tempfile::TempDir::new().unwrap();
        write_file(
            tmp.path(),
            ".claude/spec/alpha/wave-1-impl/meta.json",
            r#"{"stage":"Execute","outcome":"Active","checklist":[{"label":"only"}]}"#,
        );
        let mk = |item: &str| {
            format!(
                r#"{{"event":"checklist.item.marked","ts":"2026-06-10T10:00:00Z","spec":"alpha","wave":1,"payload":{{"spec":"alpha","wave":1,"item":"{item}"}}}}"#,
            )
        };
        write_file(
            tmp.path(),
            ".claude/spec/alpha/.events/events.ndjson",
            &format!("{}\n{}\n{}\n", mk("stale a"), mk("stale b"), mk("only")),
        );
        let repo = tmp.path().to_string_lossy().into_owned();
        crate::telemetry::invalidate_events_cache(&repo);
        let rows = spec_checklist_progress_v2(&repo, "alpha").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].done, rows[0].total), (1, 1), "done clamped to total");
    }

    #[test]
    fn checklist_progress_rejects_traversal_spec_names() {
        assert!(spec_checklist_progress_v2(".", "../evil").is_err());
        assert!(spec_checklist_progress_v2(".", "a/b").is_err());
        assert!(spec_checklist_progress_v2(".", "").is_err());
    }

    // ── Digest adherence fold (spec `instrumentar-adesao-ao-digest-no`) ──────

    #[test]
    fn spec_card_folds_latest_digest_summary_for_the_spec() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Older summary says unused — the NEWEST one must win. A summary for
        // ANOTHER spec must be ignored even though it is newer still.
        // `kind` is mandatory for the typed NDJSON reader (`io::events::Event`);
        // real writer lines carry both `event` and `kind`.
        let old = r#"{"kind":"analyze","event":"analyze.digest.summary","ts":"2026-06-10T10:00:00Z","spec":"alpha","wave":0,"payload":{"spec":"alpha","digestUsed":false,"sourceReadsBeforeDigest":7,"sourceReadsTotal":7}}"#;
        let new = r#"{"kind":"analyze","event":"analyze.digest.summary","ts":"2026-06-10T11:00:00Z","spec":"alpha","wave":0,"payload":{"spec":"alpha","digestUsed":true,"sourceReadsBeforeDigest":2,"sourceReadsTotal":9}}"#;
        let other = r#"{"kind":"analyze","event":"analyze.digest.summary","ts":"2026-06-10T12:00:00Z","spec":"beta","wave":0,"payload":{"spec":"beta","digestUsed":false,"sourceReadsBeforeDigest":99,"sourceReadsTotal":99}}"#;
        write_file(
            tmp.path(),
            ".claude/spec/alpha/.events/events.ndjson",
            &format!("{old}\n{new}\n{other}\n"),
        );
        let repo = tmp.path().to_string_lossy().into_owned();
        crate::telemetry::invalidate_events_cache(&repo);
        let card = spec_card_v2(&repo, "alpha").unwrap().expect("card");
        assert!(card.digest_used, "latest summary for the spec wins");
        assert_eq!(card.source_reads_before_digest, 2);
    }

    #[test]
    fn spec_card_defaults_digest_fields_when_no_summary_event() {
        let tmp = tempfile::TempDir::new().unwrap();
        let line = r#"{"kind":"pipeline","event":"pipeline.phase","ts":"2026-06-10T10:00:00Z","spec":"alpha","wave":0,"payload":{"to":"ANALYZE"}}"#;
        write_file(
            tmp.path(),
            ".claude/spec/alpha/.events/events.ndjson",
            &format!("{line}\n"),
        );
        let repo = tmp.path().to_string_lossy().into_owned();
        crate::telemetry::invalidate_events_cache(&repo);
        let card = spec_card_v2(&repo, "alpha").unwrap().expect("card");
        assert!(!card.digest_used, "absent summary → default false");
        assert_eq!(card.source_reads_before_digest, 0);
    }

    #[test]
    fn feed_summary_renders_digest_events() {
        let used = json!({"digestUsed": true, "sourceReadsBeforeDigest": 3});
        assert_eq!(
            feed_payload_summary("analyze.digest.summary", Some(&used)),
            "digest usado · 3 reads antes"
        );
        let unused = json!({"digestUsed": false, "sourceReadsBeforeDigest": 5});
        assert_eq!(
            feed_payload_summary("analyze.digest.summary", Some(&unused)),
            "digest não usado · 5 reads diretos"
        );
        let query = json!({"queryTerms": ["auth", "token"], "miss": false});
        assert_eq!(
            feed_payload_summary("analyze.digest.used", Some(&query)),
            "digest consultado · auth, token"
        );
        assert_eq!(
            feed_payload_summary("analyze.digest.used", None),
            "digest consultado"
        );
    }
}
