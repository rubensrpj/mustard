//! `feature` — the research / "insumos" step of a feature request.
//!
//! Given a free-text client intent, this researches the repo through the
//! `scan` tool's DIGEST (never reading project source) and emits the structured
//! inputs an AI then uses to: decompose the request into units, identify
//! cross-cutting invariants, flag net-new gaps, and ask `scan spec` for each
//! unit. It is the deterministic grounding for the elicitation loop — the
//! "pesquisa no scan" that replaces reading files by hand.
//!
//! Output (stdout, pretty JSON): the intent, the digest findings (recurring
//! slices, shared contracts, hubs),
//! the anchor files to read (plus the per-anchor `anchorsDetail` audit —
//! score/terms — and the `report.reason` strength, so the orchestrator never
//! opens the scan JSON), and a `miss` flag + note. `miss=true` means no repo
//! precedent matched — the AI must treat it as net-new (do NOT conclude "absent"
//! blindly: the term index has false negatives and no synonyms; confirm by
//! reading). Fail-open: a missing model / unavailable tool yields a miss result.

use std::path::Path;

use mustard_core::domain::scan::{DigestQuery, DigestTerm};
use mustard_core::Scan;
use serde_json::json;

/// Extract domain terms from a free-text intent: lowercased alphanumeric runs
/// >=3 chars, DEDUPED (first-occurrence order preserved), capped. The digest
/// matches by token, so over-querying is harmless (it ORs); the AI refines. No
/// language/framework knowledge.
///
/// The orchestration layer now passes the cross-lingual translation INSIDE the
/// intent (`--intent "<PT words> <english translation>"`), so the same concept
/// arrives twice (e.g. "fornecedor … supplier"). The dedup collapses each
/// lowercased token to a single query term — a token appears once regardless of
/// how many times it (or its casing) recurs in the intent.
pub(crate) fn domain_terms(intent: &str) -> Vec<String> {
    // `seen` keys on the LOWERCASED form (the same value pushed), so a token
    // that recurs — including a PT word echoed by its EN translation when both
    // fold to the same lowercased string — is queried exactly once, in
    // first-occurrence order. BTreeSet keeps the guard deterministic.
    let mut seen = std::collections::BTreeSet::new();
    let mut out: Vec<String> = Vec::new();
    for raw in intent.split(|c: char| !c.is_alphanumeric()) {
        let t = raw.to_lowercase();
        if t.len() >= 3 && t.chars().any(|c| c.is_alphabetic()) && seen.insert(t.clone()) {
            out.push(t);
        }
        // Cap at 32: function words eat slots BEFORE the digest's query-side
        // stopword filter runs, so a tighter cap cut discriminative terms off
        // the tail of prose intents. Over-querying is harmless — the digest
        // ORs terms, drops natural-language glue, and reports per term.
        if out.len() >= 32 {
            break;
        }
    }
    out
}

/// `true` when the report's reason forbids planning on top of this payload.
/// The `weak`/`none` notes steer the orchestrator to a re-query, so rendering
/// the planning fields (anchors, anchorsDetail, slices, contracts, hubs,
/// matchedTerms) would charge the orchestrator's context for content the
/// contract tells it to discard. They are withheld from stdout (empty arrays
/// plus `planningWithheld: true`); the honest `sliceMatchCount` still reports
/// what existed, and the successful re-query returns the withheld fields. The
/// recorded `feature.query` event keeps the full
/// report either way — it goes to NDJSON, not to context.
///
/// Exception: a `bridged` weak answer is NOT withheld. There the weakness is
/// only "no literal hit", and the trigram RESCUE already matched the user's
/// vocabulary onto the code's by shared-root FORM — a re-query in the repo's own
/// words would merely re-find what the fuzzy rung bridged. So the planning
/// fields ride along (the note flags the approximate hit and to confirm by
/// reading).
fn withhold_planning(reason: &str, bridged: bool) -> bool {
    matches!(reason, "weak" | "none") && !bridged
}

/// Max `candidates` rows emitted on a NON-strong result — the menu the
/// orchestration-layer translator (a Haiku step that lives OUTSIDE this
/// command) selects from to re-query in the code's own vocabulary. The source
/// is the PUBLISHED domain-term index (`Scan::digest().terms`, already
/// `build_terms`-ranked and capped at the scan tool's `MAX_TERMS`); this is the
/// emission-side bound on top of that, so the menu stays a few KB regardless of
/// the catalogue's published cap. The published order is preserved verbatim
/// (byte-stable), never re-derived or re-sorted here.
const CANDIDATES_MAX: usize = 80;

/// `true` when the report's strength is NOT `strong` — i.e. the orchestrator
/// must re-query in the code's own vocabulary before planning. Drives whether
/// `candidates` (the translator's menu) is attached: emitted on
/// `weak`/`none`/`generated_only` or any legacy `miss`, omitted on `strong`
/// (the strong path stays lean — the anchors already ARE the evidence).
fn non_strong(reason: &str, miss: bool) -> bool {
    reason != "strong" && (matches!(reason, "weak" | "none" | "generated_only") || miss || reason.is_empty())
}

/// Project the PUBLISHED domain-term index into the bounded `candidates` menu:
/// each row is `{ term, count }` drawn verbatim from the catalogue
/// (`Scan::digest().terms`) — REUSED, never re-derived or re-sorted, so the
/// published rank order (frequency/rank desc, term asc) carries through
/// byte-stably. Bounded by [`CANDIDATES_MAX`] (rows). Pure (no spawn, no IO) so
/// the shape is unit-testable without the scan binary.
///
/// `samples` (a couple of "where this vocabulary lives" paths) was DROPPED from
/// stdout: the consumer — the orchestration layer that re-queries against this
/// menu — reads the `term` column ONLY, never the samples; they were emitted
/// solely for a manual fallback. Dropping them trims the weak/none JSON without
/// affecting the term menu.
fn candidates_from_index(index: &[DigestTerm]) -> Vec<serde_json::Value> {
    index
        .iter()
        .take(CANDIDATES_MAX)
        .map(|t| {
            json!({
                "term": t.term,
                "count": t.count,
            })
        })
        .collect()
}

/// Build the insumos payload for a successful digest query. Pure (no spawn, no
/// IO) so the payload shape — including the `stacks` passthrough and the
/// `candidates` menu — is unit-testable without the scan binary. `index` is the
/// PUBLISHED domain-term catalogue (`Scan::digest().terms`), used ONLY on a
/// non-strong result to build `candidates`; on a strong result it is ignored
/// (and the caller passes an empty slice, skipping the extra fetch).
fn payload(intent: &str, q: &DigestQuery, index: &[DigestTerm]) -> serde_json::Value {
    let withhold = withhold_planning(q.report.reason.as_str(), q.report.bridged);
    let mut out = json!({
        "intent": intent,
        // `queryTerms` (the echoed tokenization of `--intent`) was dropped from
        // STDOUT — the orchestrator already holds the intent it passed in, and
        // the report names every term that mattered. The recorded
        // `feature.query` / `analyze.digest.used` EVENTS keep `queryTerms` for
        // `lexicon-suggest` / adherence; only the stdout payload loses it.
        // Stacks the scan inferred for the model (registry-driven, see
        // `mustard_core::domain::vocabulary::stacks`) — copied into every
        // payload, hit or miss, so the orchestrator can specialize guidance.
        // Full contract (name + confidence + signals): the signals are the
        // auditable evidence that lets a low-confidence detection be
        // distrusted without another round-trip; stacks are few per repo, so
        // the verbosity cost is negligible.
        "stacks": q.detected_stacks.iter().map(|s| json!({
            "name": s.name,
            // Round through f64 to 2 decimals: a bare f32→f64 widening would
            // print `0.95f32` as `0.949999988079071`, breaking byte-stability.
            "confidence": (f64::from(s.confidence) * 100.0).round() / 100.0,
            "signals": s.signals,
        })).collect::<Vec<_>>(),
        "miss": q.miss,
        // Planning fields below render empty under `withhold` (weak/none
        // precedent) — see `withhold_planning`. `planningWithheld` is the
        // honest marker; the counts keep reporting the true sizes.
        "planningWithheld": withhold,
        // `matchedTerms` (term+count) was dropped — it duplicated `report.terms`
        // (which carries term + tier + the files), so it was pure payload weight.
        // `exemplarFiles`: the real reference-implementation files that
        // exemplify each slice — so the orchestrator opens the files to mirror
        // directly, not just the pattern label. Passed through from the scan
        // digest's per-slice `exemplar_files` (already most-complex-first,
        // deduped, capped at 4 by the scan tool).
        "slices": if withhold { Vec::new() } else { q.slices.iter().map(|s| json!({ "label": s.label, "recurrence": s.recurrence, "entities": s.entities, "exemplarFiles": s.exemplar_files })).collect::<Vec<_>>() },
        // Count of matched recurring slices — the deterministic signal the
        // scope classifier consumes: 1 = "mirrors a matched slice"
        // (light/extended-light); >=2 = multi-slice vocabulary overlap, which
        // counts toward "full" only alongside layer spread (layerCount >= 2 in
        // scope-classify) — alone it is precedent, not layer spanning.
        // Additive: the `slices` array is unchanged for existing consumers.
        "sliceMatchCount": q.slices.len(),
        // `slicesOmitted` (the per-query cap's trimmed-tail counter) was dropped
        // from STDOUT — a debug-only "there was more" signal the orchestrator
        // never acts on. The scan struct still carries it (the scan tests read
        // it directly); only this stdout payload stops emitting it.
        "contracts": if withhold { Vec::new() } else { q.contracts.iter().map(|c| json!({ "name": c.name, "implementors": c.implementors })).collect::<Vec<_>>() },
        "hubs": if withhold { Vec::new() } else { q.hubs.iter().map(|h| json!({ "module": h.module, "degree": h.degree })).collect::<Vec<_>>() },
        "anchors": if withhold { &[] as &[String] } else { &q.files[..] },
        // Per-anchor provenance (same order as `anchors`): the BM25F relevance
        // `scoreX1024` each anchor ranked with + the matched terms that carry it
        // (file→terms), so the orchestrator sees the relevance ORDER and the
        // drop-off — picking what to read without opening the scan JSON. The
        // score is live again: the anchor ranking is BM25F (fielded, path-
        // boosted), not the old score-less insumo union.
        "anchorsDetail": if withhold { Vec::new() } else { q.files_detail.iter().map(|d| json!({
            "file": d.file, "scoreX1024": d.score_x1024, "terms": d.terms,
        })).collect::<Vec<_>>() },
        // The honest per-term match report (scan's tier ladder) — the truth
        // about what matched. Per term: the tier that carried it (exact |
        // fold | stem | lexicon | none), the natural-language evidence and
        // the files where the vocabulary lives; aggregate matched k/n.
        "report": json!({
            "matched": q.report.matched,
            "total": q.report.total,
            "reason": q.report.reason,
            // Additive marker: a `weak` answer a curated lexicon bridge carried
            // (translated, not literal) — the planning fields are kept, not
            // withheld. See `withhold_planning` and `note`.
            "bridged": q.report.bridged,
            // `files` was dropped from STDOUT — it duplicated `anchorsDetail`
            // (the file→terms evidence map already in stdout), so it was pure
            // payload weight on a wide query (32 terms × N files each). The
            // `feature.query` EVENT keeps the full per-term `files` for
            // `lexicon-suggest`; only stdout loses them. Same precedent as the
            // `matchedTerms` drop above.
            "terms": q.report.terms.iter().map(|t| json!({
                "term": t.term, "tier": t.tier, "lang": t.lang,
            })).collect::<Vec<_>>(),
        }),
        "note": note(q),
    });
    // On a NON-strong result, attach the translator's menu: a bounded slice of
    // the PUBLISHED domain-term index so an orchestration-layer (Haiku) step can
    // map a cross-lingual intent onto the repo's real code vocabulary and
    // re-query. Omitted on `strong` — there the anchors already ARE the
    // evidence, and the strong path stays lean. No LLM call here: this command
    // only PUBLISHES the menu, deterministically. The non-strong fallback path
    // (scan unavailable) has no catalogue, so it passes an empty slice → an
    // empty `candidates`, honestly signalling "no vocabulary to offer".
    if non_strong(q.report.reason.as_str(), q.miss) {
        if let Some(obj) = out.as_object_mut() {
            obj.insert("candidates".to_string(), json!(candidates_from_index(index)));
        }
    }
    // Multi-concern split: when scan partitioned the query's concepts into ≥2
    // disconnected groups (no shared module, no import bridge), it returns one
    // `ConcernHit` per group, each with its OWN ranked anchors restricted to
    // that concern. Emit them as a labelled `concerns` array so the orchestrator
    // sees a clean per-concern ranking instead of the blended top-level list (a
    // dense concern would otherwise drown the others). Single-concern compat: an
    // empty `q.concerns` (one group, or an older scan binary) emits NO `concerns`
    // key — the flat `anchors`/`anchorsDetail` already IS that one concern, and
    // the existing stdout shape is unchanged byte-for-byte for current consumers.
    // The per-concern audit mirrors the top-level projection (`scoreX1024` +
    // carrying terms) so a consumer reads concerns and flat anchors identically.
    if !q.concerns.is_empty() {
        if let Some(obj) = out.as_object_mut() {
            obj.insert("concerns".to_string(), json!(concerns_payload(&q.concerns)));
        }
    }
    out
}

/// Project scan's per-concern split into the labelled `concerns` payload: one
/// row per concern, each carrying its own `anchors` (the concern's ranked
/// `files`), `anchorsDetail` (the parallel `score_x1024` + carrying terms,
/// rendered as `scoreX1024` exactly like the top-level audit), `concepts` (the
/// query concepts in this concern) and `reason` (the concern's own strength).
/// Pure (no spawn, no IO) so the shape is unit-testable without the scan binary;
/// byte-stable (the input order is preserved verbatim, never re-sorted here).
fn concerns_payload(concerns: &[mustard_core::domain::scan::ConcernHit]) -> Vec<serde_json::Value> {
    concerns
        .iter()
        .map(|c| {
            json!({
                "label": c.label,
                "concepts": c.concepts,
                "reason": c.reason,
                "anchors": c.files,
                "anchorsDetail": c.files_detail.iter().map(|d| json!({
                    "file": d.file, "scoreX1024": d.score_x1024, "terms": d.terms,
                })).collect::<Vec<_>>(),
            })
        })
        .collect()
}

/// Compact `feature.query` event payload: the RAW `--intent` text + the
/// queried terms + the honest match report (matched/total/reason + per-term
/// term/tier/lang). `intent` is additive and deliberately untokenized — the
/// user's own vocabulary (e.g. PT) stays visible and auditable even after
/// `domain_terms` tokenization, so a later `lexicon-suggest` reviewer can see
/// the demand exactly as it was phrased. The per-term `files` ride along —
/// they are the evidence `lexicon-suggest` cites when a later re-query
/// confirms a vocabulary bridge. Pure + deterministic; the payload carries no
/// timestamp of its own (the event channel stamps `ts`).
fn query_event_payload(intent: &str, terms: &[String], q: &DigestQuery) -> serde_json::Value {
    json!({
        "intent": intent,
        "queryTerms": terms,
        "report": {
            "matched": q.report.matched,
            "total": q.report.total,
            "reason": q.report.reason,
            "terms": q.report.terms.iter().map(|t| json!({
                "term": t.term, "tier": t.tier, "lang": t.lang, "files": t.files,
            })).collect::<Vec<_>>(),
        },
    })
}

/// How long a research window stays open before the age backstop closes it.
/// The window's PRIMARY close is deterministic — the first non-Read/Edit/Write
/// tool removes the marker (see `feature_outcome_observer`); this is only the
/// backstop for a window that is never followed by a non-research tool (e.g. a
/// session that ends mid-research). Generous enough to span a real research
/// burst with reading pauses, short enough that a stale marker cannot poison a
/// later, unrelated session-day.
const RESEARCH_WINDOW_SECS: i64 = 30 * 60;

/// Build the `active-research.json` marker body the digest-outcome observer
/// reads: `{terms, anchors:[{file, terms}], ts, opened_at, expires_at}`. The
/// anchors invert the digest's per-term report (`report.terms[].files`) into
/// file→terms, so the observer can mark a touched file `wasAnchor` and name the
/// query terms that declared it WITHOUT re-deriving the mapping. `opened_at` /
/// `expires_at` give the window an age backstop (mirroring the `WindowState`
/// pattern of `amend_window_inject`). Pure + deterministic (sorted files; each
/// file's terms in report order, deduped) so the body is testable without IO;
/// `ts` / `opened_at` is the event channel's wall clock (stamped by the caller).
fn active_research_marker(terms: &[String], q: &DigestQuery, ts: &str) -> serde_json::Value {
    use std::collections::BTreeMap;
    // file -> ordered, deduped list of terms that named it.
    let mut by_file: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for t in &q.report.terms {
        for f in &t.files {
            let entry = by_file.entry(f.clone()).or_default();
            if !entry.contains(&t.term) {
                entry.push(t.term.clone());
            }
        }
    }
    let anchors: Vec<serde_json::Value> = by_file
        .into_iter()
        .map(|(file, terms)| json!({ "file": file, "terms": terms }))
        .collect();
    // expires_at = opened_at (ts) + RESEARCH_WINDOW_SECS, derived from the same
    // wall clock so the marker carries a self-contained window.
    let expires_at = mustard_core::time::parse_iso_millis(ts)
        .map(|ms| mustard_core::time::millis_to_iso(ms + RESEARCH_WINDOW_SECS * 1000))
        .unwrap_or_else(|| ts.to_string());
    json!({
        "terms": terms,
        "anchors": anchors,
        "ts": ts,
        "opened_at": ts,
        "expires_at": expires_at,
        // `touched` gates the window close: it flips to `true` on the FIRST
        // outcome emission (the orchestrator's first anchor read). Until then a
        // non-research tool — including the Bash that ran `feature` itself, and
        // the ANALYZE Bash steps that follow — does NOT close the window.
        "touched": false,
    })
}

/// Drop the per-round `active-research.json` marker in the current session,
/// overwriting any prior round. This opens the correlation window the
/// `feature_outcome_observer` reads on each Read/Edit/Write. Fail-open: an
/// unresolved session or a failed write is a silent no-op — the marker is a
/// telemetry convenience, never a correctness dependency (a missing marker
/// just yields no `feature.outcome` events).
fn drop_research_marker(terms: &[String], q: &DigestQuery) {
    let cwd = crate::shared::context::project_dir();
    let Some(path) = crate::hooks::observe::feature_outcome_observer::marker_path_for(&cwd) else {
        return;
    };
    let body = active_research_marker(terms, q, &mustard_core::time::now_iso8601());
    let Ok(bytes) = serde_json::to_vec(&body) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = mustard_core::io::fs::create_dir_all(parent);
    }
    let _ = mustard_core::io::fs::write_atomic(&path, &bytes);
}

/// Record the research round as a `feature.query` harness event, attributed to
/// the active session/spec by the router's resolution chain (the same channel
/// `emit-event` uses). Fail-open: a failed write never blocks the research
/// output on stdout.
fn emit_query_event(payload: serde_json::Value) {
    use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
    let dir = crate::shared::context::project_dir();
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: mustard_core::time::now_iso8601(),
        session_id: crate::shared::context::session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Cli,
            id: Some("feature".to_string()),
            actor_type: None,
        },
        event: "feature.query".to_string(),
        payload,
        // None on purpose: the router resolves the active spec (env →
        // session→spec marker) so ANALYZE-time queries land beside the other
        // session events and post-PLAN queries attribute to the bound spec.
        spec: None,
    };
    let _ = crate::shared::events::route::emit(&dir, &ev);
}

/// Minimal `analyze.digest.used` payload: the queried terms + the legacy
/// hit/miss flag. This is the adherence MARKER `digest-adherence-finalize`
/// looks for ("the digest was consulted at this instant") — deliberately
/// smaller than the `feature.query` payload, whose report serves
/// `lexicon-suggest` instead.
fn digest_used_payload(terms: &[String], q: &DigestQuery) -> serde_json::Value {
    json!({
        "queryTerms": terms,
        "miss": q.miss,
    })
}

/// Record that the scan digest answered a research round, as an
/// `analyze.digest.used` harness event. Unlike [`emit_query_event`] the spec
/// is resolved HERE via [`crate::shared::context::current_spec`] (may be
/// `None`) so the marker carries the active spec when one is already bound.
/// Fail-open: a failed write never blocks the research output on stdout.
fn emit_digest_used_event(payload: serde_json::Value) {
    use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
    let dir = crate::shared::context::project_dir();
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: mustard_core::time::now_iso8601(),
        session_id: crate::shared::context::session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Cli,
            id: Some("feature".to_string()),
            actor_type: None,
        },
        event: "analyze.digest.used".to_string(),
        payload,
        spec: crate::shared::context::current_spec(&dir),
    };
    let _ = crate::shared::events::route::emit(&dir, &ev);
}

/// The contract line for a MULTI-CONCERN answer: scan split the query's
/// concepts into ≥2 disconnected groups (concerns), each with its own ranked
/// anchors in the `concerns` array. The AI must read ALL the splits before
/// reasoning — otherwise it focuses on the first (densest) concern and silently
/// drops the others, which is the exact bias this split exists to defeat.
/// Prepended to the reason note (one space joins them) only when `concerns >= 2`.
const ALL_BREAKS_FIRST_CONTRACT: &str = "this query split into MULTIPLE concerns — read EVERY entry in `concerns` (each is one independent sub-request with its OWN ranked anchors) and analyze them only AFTER all the splits returned; do NOT plan on concern #1 before reading the rest, or the densest concern silently drowns the others.";

/// The guidance note for the AI consuming the payload, keyed on the report's
/// reason (the truth); an empty reason means the payload came from an older
/// scan binary, so it falls back to the legacy `miss` flag. When scan split the
/// query into ≥2 concerns, the [`ALL_BREAKS_FIRST_CONTRACT`] line is prepended
/// so the AI reads every split before reasoning; a single-concern answer keeps
/// the base reason note verbatim (byte-stable for existing consumers).
fn note(q: &DigestQuery) -> String {
    let base = reason_note(q);
    if q.concerns.len() >= 2 {
        format!("{ALL_BREAKS_FIRST_CONTRACT} {base}")
    } else {
        base.to_string()
    }
}

/// The per-reason base note (the single-concern guidance), keyed on the report's
/// reason. Split out from [`note`] so the multi-concern contract can prepend to
/// it without duplicating the reason ladder.
fn reason_note(q: &DigestQuery) -> &'static str {
    // A fuzzy (shared-root / morphology) bridge carried this answer — matched by
    // FORM, not literally. The planning fields are RETURNED (not withheld): the
    // trigram rescue already mapped the request vocabulary onto the code's, so a
    // re-query in the repo's words would only re-find the same files. Read the
    // anchors as evidence; confirm by reading before planning on top.
    if q.report.bridged {
        return "repo precedent found via a FUZZY shared-root bridge — your request vocabulary matched the code's by FORM, not literally (report.terms[].tier is `trigram`). `anchors` are EVIDENCE, returned not withheld: pick the files that fit and read them, and read the `hubs` (the computing logic often lives in a generically-named central service). The hit is approximate — confirm by reading before planning on top of it";
    }
    match q.report.reason.as_str() {
        "none" => {
            "no repo precedent matched — treat as net-new; the report names each missed term, so re-query the digest in the code's own vocabulary or dispatch an Explore before concluding 'absent'"
        }
        "weak" => {
            "weak precedent — under half the terms matched or only stem-derived hits; re-query the digest in the code's own vocabulary (the report names each matched term and its tier) and Explore before planning on top of this. Planning fields (anchors/slices/contracts/hubs) are withheld on weak precedent — the re-query returns them"
        }
        "generated_only" => {
            "matches live only in machine-written modules — regenerate or extend the generator's input; never edit the matched files directly"
        }
        "strong" => {
            "repo precedent found — `anchors` is RANKED by relevance (BM25F: rare domain terms, with a boost when your query names the file's path; `anchorsDetail` carries each anchor's `scoreX1024` + the terms that carry it, so the relevance order and the drop-off are visible). Read the top anchors that fit the request; also read the `hubs` — the logic that COMPUTES a behavior often lives in a generically-named central service, not the module named after the entity. Mirror the matched slices/contracts, then ask `scan spec` per unit"
        }
        _ if q.miss => {
            "no repo precedent matched — treat as net-new; the term index has no synonyms and false negatives, so confirm by reading the matched files, do not conclude 'absent' blindly"
        }
        _ => {
            "repo precedent found — `anchors` is RANKED by relevance (BM25F; `anchorsDetail` carries each anchor's `scoreX1024` + the terms that carry it). Read the top anchors that fit the request; also read the `hubs` (the computing logic often lives in a generically-named central service). Mirror the matched slices/contracts, then ask `scan spec` per unit"
        }
    }
}

/// Run the research step: print the feature insumos JSON for `intent`.
///
/// PURE DETERMINISTIC — no `claude` subprocess. Cross-lingual translation now
/// lives in the ORCHESTRATION layer: the caller passes the english translation
/// INSIDE `--intent` (`--intent "<user prompt PT> <english translation>"`), so
/// this command only tokenizes the DISTINCT union of the terms it receives
/// (`domain_terms` dedups), queries the digest once, and prints the insumos. On
/// a NON-strong result the `candidates` menu still rides along — a deterministic
/// fallback the orchestration layer can re-query against.
pub fn run(intent: &str, root: &Path) {
    let terms = domain_terms(intent);
    let model = root.join(".claude").join("grain.model.json");

    let payload = match Scan::locate().digest_query(&model, &terms) {
        Ok(q) => {
            // Register the research round (queryTerms + compact report) so
            // `lexicon-suggest` can later correlate a `none`-tier query with
            // the successful re-query that bridged it. Only an answered query
            // is recorded — a spawn failure has no honest report to fold.
            // Both events are emitted BEFORE the println below so the stdout
            // contract stays byte-stable (telemetry never interleaves output).
            emit_query_event(query_event_payload(intent, &terms, &q));
            emit_digest_used_event(digest_used_payload(&terms, &q));
            // Open the digest-outcome correlation window: a per-round marker
            // the `feature_outcome_observer` reads to attribute each later
            // Read/Edit/Write to this query's anchors. Fail-open side effect.
            drop_research_marker(&terms, &q);
            // On a NON-strong result, fetch the PUBLISHED domain-term catalogue
            // (the `build_terms`-ranked, scan-capped index) so `payload` can
            // attach the `candidates` fallback menu. The fetch is gated on
            // non-strong so the strong (lean) path pays for no extra spawn; a
            // failed fetch degrades to an empty menu (fail-open). On `strong`,
            // `payload` ignores the slice, so an empty one is correct.
            let index: Vec<DigestTerm> = if non_strong(q.report.reason.as_str(), q.miss) {
                Scan::locate().digest(&model).map(|d| d.terms).unwrap_or_default()
            } else {
                Vec::new()
            };
            payload(intent, &q, &index)
        }
        Err(err) => {
            eprintln!("feature: scan digest unavailable: {err}");
            json!({
                "intent": intent,
                "stacks": [],
                "miss": true,
                "planningWithheld": true,
                "slices": [],
                "sliceMatchCount": 0,
                "contracts": [],
                "hubs": [],
                "anchors": [],
                "anchorsDetail": [],
                "report": { "matched": 0, "total": 0, "reason": "none", "terms": [] },
                // Non-strong (`none`/`miss`), so the `candidates` key is present
                // for a stable shape — but empty: the scan model is unavailable,
                // so there is no published vocabulary to offer the translator.
                "candidates": [],
                "note": "scan model unavailable — run `mustard-rt run scan` first; treat as net-new until then",
            })
        }
    };
    println!("{}", serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".into()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_terms_lowercases_dedups_and_drops_short() {
        let t = domain_terms("Add a Refund to the Order — refund the ORDER");
        assert!(t.contains(&"refund".to_string()));
        assert!(t.contains(&"order".to_string()));
        assert!(t.contains(&"the".to_string())); // >=3 chars kept; AI/digest filter relevance
        assert!(!t.contains(&"to".to_string())); // <3 dropped
        // dedup: "order"/"refund" appear once despite repeats
        assert_eq!(t.iter().filter(|x| *x == "order").count(), 1);
        assert_eq!(t.iter().filter(|x| *x == "refund").count(), 1);
    }

    #[test]
    fn domain_terms_caps_length() {
        let many = (0..50).map(|i| format!("term{i}")).collect::<Vec<_>>().join(" ");
        assert!(domain_terms(&many).len() <= 32);
    }

    #[test]
    fn domain_terms_keeps_tail_terms_of_long_prose_intents() {
        // Prose intents front-load function words (filtered only query-side,
        // by the digest); the discriminative vocabulary often sits past the
        // 16th unique token, where the old cap silently cut it.
        let filler = (0..20).map(|i| format!("word{i}")).collect::<Vec<_>>().join(" ");
        let t = domain_terms(&format!("{filler} hierarquia vencido"));
        assert!(t.len() > 16, "cap must not stop at 16: {t:?}");
        assert!(t.contains(&"hierarquia".to_string()), "term beyond position 16 kept: {t:?}");
        assert!(t.contains(&"vencido".to_string()), "term beyond position 16 kept: {t:?}");
    }

    #[test]
    fn stacks_facts_feature_payload_carries_stacks() {
        // The digest's `detected_stacks` pass through into the insumos payload
        // as `stacks` (name + confidence + signals), with the confidence
        // rendered as the clean 2-decimal value (no f32→f64 widening noise).
        let q: DigestQuery = serde_json::from_str(
            r#"{"query":["page"],"detected_stacks":[{"name":"nextjs","confidence":0.65,"signals":["dep:next","path:next.config.js"]},{"name":"laravel","confidence":0.95,"signals":["dep:laravel/framework"]}],"files":["pages/index.tsx"],"miss":false}"#,
        )
        .expect("digest payload with detected_stacks");
        let v = payload("add a page", &q, &[]);
        let stacks = v["stacks"].as_array().expect("stacks array");
        assert_eq!(stacks.len(), 2, "both detections carried: {v}");
        assert_eq!(stacks[0]["name"], "nextjs");
        assert_eq!(stacks[0]["confidence"], 0.65);
        assert_eq!(stacks[0]["signals"], json!(["dep:next", "path:next.config.js"]));
        assert_eq!(stacks[1]["name"], "laravel");
        assert_eq!(stacks[1]["confidence"], 0.95);
        // The echoed `queryTerms` and the debug-only `slicesOmitted` counter are
        // NOT in stdout — they were dropped to trim the payload (the orchestrator
        // holds the intent; the `feature.query` EVENT still keeps `queryTerms`).
        assert!(v.get("queryTerms").is_none(), "queryTerms dropped from stdout: {v}");
        assert!(v.get("slicesOmitted").is_none(), "slicesOmitted dropped from stdout: {v}");
        // Byte-stability: the serialized payload carries the clean decimals.
        let s = serde_json::to_string(&v).expect("payload serializes");
        assert!(s.contains("0.65"), "clean confidence missing: {s}");
        assert!(!s.contains("0.649999"), "f32 widening noise leaked: {s}");

        // No detections → an empty array, same shape as the fallback payload.
        let bare: DigestQuery = serde_json::from_str(r#"{"miss":true}"#).expect("bare digest");
        let v = payload("anything", &bare, &[]);
        assert_eq!(v["stacks"], json!([]), "empty stacks must stay an empty array: {v}");
        assert!(v.get("queryTerms").is_none(), "queryTerms stays dropped on the miss shape: {v}");
        assert!(v.get("slicesOmitted").is_none(), "slicesOmitted stays dropped on the miss shape: {v}");
    }

    #[test]
    fn active_research_marker_inverts_report_terms_into_file_anchors() {
        // The marker the digest-outcome observer reads inverts the per-term
        // report (term→files) into file→terms anchors, byte-stably (files
        // sorted by the BTreeMap; each file's terms in report order, deduped).
        let q: DigestQuery = serde_json::from_str(
            r#"{"query":["refund","order"],"miss":false,
                "report":{"matched":2,"total":2,"reason":"strong","terms":[
                    {"term":"refund","tier":"exact","lang":"","files":["src/refund.cs","src/shared.cs"]},
                    {"term":"order","tier":"exact","lang":"","files":["src/shared.cs","src/order.cs"]}]}}"#,
        )
        .expect("digest payload with report");
        let terms = vec!["refund".to_string(), "order".to_string()];
        let m = active_research_marker(&terms, &q, "2026-06-16T00:00:00.000Z");
        assert_eq!(m["terms"], json!(["refund", "order"]), "raw query terms carried");
        assert_eq!(m["ts"], json!("2026-06-16T00:00:00.000Z"));
        // Window fields: opened_at = ts, expires_at = ts + 30 min (the age backstop).
        assert_eq!(m["opened_at"], json!("2026-06-16T00:00:00.000Z"));
        assert_eq!(m["expires_at"], json!("2026-06-16T00:30:00.000Z"), "30-min age window: {m}");
        // `touched` opens false — the window survives the Bash that ran `feature`.
        assert_eq!(m["touched"], json!(false), "marker opens untouched: {m}");
        let anchors = m["anchors"].as_array().expect("anchors array");
        // 3 distinct files, sorted: order.cs, refund.cs, shared.cs.
        assert_eq!(anchors.len(), 3, "one row per distinct anchor file: {m}");
        assert_eq!(anchors[0]["file"], json!("src/order.cs"));
        assert_eq!(anchors[0]["terms"], json!(["order"]));
        assert_eq!(anchors[1]["file"], json!("src/refund.cs"));
        assert_eq!(anchors[1]["terms"], json!(["refund"]));
        // shared.cs is named by BOTH terms (report order: refund then order).
        assert_eq!(anchors[2]["file"], json!("src/shared.cs"));
        assert_eq!(anchors[2]["terms"], json!(["refund", "order"]), "multi-term anchor: {m}");

        // Byte-stable for the same inputs (the marker is correlation evidence).
        let a = serde_json::to_string(&active_research_marker(&terms, &q, "t")).expect("ser");
        let b = serde_json::to_string(&active_research_marker(&terms, &q, "t")).expect("ser");
        assert_eq!(a, b);

        // A miss with no report terms → empty anchors (the observer no-ops).
        let bare: DigestQuery = serde_json::from_str(r#"{"miss":true}"#).expect("bare");
        let m = active_research_marker(&[], &bare, "t");
        assert_eq!(m["anchors"], json!([]), "no anchors on a bare miss: {m}");
    }

    #[test]
    fn feature_query_event_payload_is_compact_and_deterministic() {
        // The recorded event carries ONLY {intent, queryTerms, report} — none
        // of the bulky insumos fields (anchors/slices/hubs/stacks) and no
        // timestamp of its own (the event channel stamps `ts`). Per-term
        // entries keep term/tier/lang + the evidence files lexicon-suggest
        // cites.
        let q: DigestQuery = serde_json::from_str(
            r#"{"query":["hierarquia"],"miss":true,"report":{"matched":0,"total":1,"reason":"none","terms":[{"term":"hierarquia","tier":"none","lang":"","files":[]}]}}"#,
        )
        .expect("digest payload with report");
        let terms = vec!["hierarquia".to_string()];
        let v = query_event_payload("hierarquia de títulos", &terms, &q);
        assert_eq!(v["queryTerms"], json!(["hierarquia"]));
        assert_eq!(v["report"]["matched"], 0);
        assert_eq!(v["report"]["total"], 1);
        assert_eq!(v["report"]["reason"], "none");
        assert_eq!(v["report"]["terms"][0]["term"], "hierarquia");
        assert_eq!(v["report"]["terms"][0]["tier"], "none");
        assert_eq!(v["report"]["terms"][0]["lang"], "");
        assert_eq!(v["report"]["terms"][0]["files"], json!([]));
        let obj = v.as_object().expect("object payload");
        assert_eq!(obj.len(), 3, "exactly intent + queryTerms + report: {v}");
        assert!(obj.get("ts").is_none(), "no own timestamp in the payload");
        // Byte-stable: the same inputs serialize to the same bytes.
        let a = serde_json::to_string(&query_event_payload("hierarquia de títulos", &terms, &q))
            .expect("serializes");
        let b = serde_json::to_string(&query_event_payload("hierarquia de títulos", &terms, &q))
            .expect("serializes");
        assert_eq!(a, b);
    }

    #[test]
    fn lexicon_suggest_feature_query_event_payload_carries_raw_intent() {
        // The demand stays visible PRE-tokenization: the recorded event keeps
        // the `--intent` text verbatim (accents, casing, function words — the
        // user's vocabulary), next to the tokenized queryTerms. This is what
        // makes a PT miss auditable by a later `lexicon-suggest` run instead
        // of being washed away by `domain_terms`.
        let q: DigestQuery = serde_json::from_str(
            r#"{"query":["previsao"],"miss":true,"report":{"matched":0,"total":1,"reason":"none","terms":[{"term":"previsao","tier":"none","lang":"","files":[]}]}}"#,
        )
        .expect("digest payload");
        let intent = "Adicionar PREVISÃO de lançamento à conta financeira";
        let terms = domain_terms(intent);
        let v = query_event_payload(intent, &terms, &q);
        assert_eq!(v["intent"], json!(intent), "raw intent verbatim: {v}");
        // The tokenized terms are lossy (lowercased, folded by the caller's
        // shell, deduped) — the payload must carry BOTH representations.
        assert_ne!(v["intent"], v["queryTerms"], "intent is not the token list");
    }

    #[test]
    fn feature_payload_exposes_anchors_detail_audit() {
        // `files_detail` passes through as `anchorsDetail` — per anchor, the
        // BM25F relevance `scoreX1024` + the matched terms that carry it
        // (file→terms provenance), so the orchestrator sees WHY each anchor is
        // in the set AND its relevance order/drop-off, without opening the scan
        // JSON. The score is live again (the ranking is BM25F, not the old
        // score-less insumo union).
        let q: DigestQuery = serde_json::from_str(
            r#"{"query":["refund"],"files":["src/refund.cs","src/tail.cs"],"files_detail":[{"file":"src/refund.cs","score_x1024":2048,"terms":["refund"]},{"file":"src/tail.cs","score_x1024":0,"terms":[]}],"miss":false,"report":{"matched":1,"total":1,"reason":"strong","terms":[]}}"#,
        )
        .expect("digest payload with files_detail");
        let v = payload("refund", &q, &[]);
        let detail = v["anchorsDetail"].as_array().expect("anchorsDetail array");
        assert_eq!(detail.len(), 2, "one provenance row per anchor: {v}");
        assert_eq!(detail[0]["file"], "src/refund.cs");
        assert_eq!(detail[0]["terms"], json!(["refund"]));
        assert_eq!(detail[0]["scoreX1024"], 2048, "the BM25F relevance score rides along: {v}");
        assert_eq!(detail[1]["scoreX1024"], 0, "tail anchor's score is honest: {v}");
        assert_eq!(detail[1]["terms"], json!([]), "tail anchor shows no carrying terms: {v}");
        // The reason rides in the same payload.
        assert_eq!(v["report"]["reason"], "strong");

        // Old scan binary (no files_detail): the field degrades to an empty
        // array, mirroring the miss-fallback payload's shape.
        let old: DigestQuery = serde_json::from_str(r#"{"miss":true}"#).expect("old digest");
        let v = payload("anything", &old, &[]);
        assert_eq!(v["anchorsDetail"], json!([]), "older payloads keep the shape: {v}");
    }

    #[test]
    fn analyze_digest_used_payload_is_minimal_marker() {
        // The adherence marker carries ONLY {queryTerms, miss} — it records
        // THAT the digest answered, not the full report (which lives on the
        // sibling `feature.query` event). Deterministic for the same inputs.
        let q: DigestQuery = serde_json::from_str(
            r#"{"query":["refund"],"miss":false,"report":{"matched":1,"total":1,"reason":"strong","terms":[]}}"#,
        )
        .expect("digest payload");
        let terms = vec!["refund".to_string()];
        let v = digest_used_payload(&terms, &q);
        assert_eq!(v["queryTerms"], json!(["refund"]));
        assert_eq!(v["miss"], json!(false));
        let obj = v.as_object().expect("object payload");
        assert_eq!(obj.len(), 2, "exactly queryTerms + miss: {v}");
        let a = serde_json::to_string(&digest_used_payload(&terms, &q)).expect("serializes");
        let b = serde_json::to_string(&digest_used_payload(&terms, &q)).expect("serializes");
        assert_eq!(a, b, "byte-stable for the same inputs");
    }

    #[test]
    fn feature_payload_exposes_match_report_and_reason_note() {
        // The digest's per-term report passes through verbatim (term, tier,
        // lang, files + matched k/n + reason), and the note is keyed on the
        // reason: `weak`/`none` steer to re-querying in the code's own
        // vocabulary / Explore instead of false confidence.
        let weak: DigestQuery = serde_json::from_str(
            r#"{"query":["cancelado"],"matched_terms":[{"term":"cancel","count":3,"samples":["src/cancel.cs"]}],"miss":false,"report":{"matched":1,"total":2,"reason":"weak","terms":[{"term":"cancelado","tier":"lexicon","lang":"pt-en","files":["src/cancel.cs"]},{"term":"hierarquia","tier":"none","lang":"","files":[]}]}}"#,
        )
        .expect("digest payload with report");
        let v = payload("cancelar titulo", &weak, &[]);
        assert_eq!(v["report"]["matched"], 1);
        assert_eq!(v["report"]["total"], 2);
        assert_eq!(v["report"]["reason"], "weak");
        assert_eq!(v["report"]["terms"][0]["tier"], "lexicon");
        assert_eq!(v["report"]["terms"][0]["lang"], "pt-en");
        assert_eq!(v["report"]["terms"][1]["tier"], "none");
        let note = v["note"].as_str().expect("note");
        assert!(note.contains("re-query") && note.contains("Explore"), "weak note steers to re-query/Explore: {note}");

        // `none` also steers away from false confidence.
        let none: DigestQuery = serde_json::from_str(
            r#"{"query":["zzz"],"miss":true,"report":{"matched":0,"total":1,"reason":"none","terms":[{"term":"zzz","tier":"none","lang":"","files":[]}]}}"#,
        )
        .expect("none-reason digest");
        let v = payload("zzz", &none, &[]);
        let note = v["note"].as_str().expect("note");
        assert!(note.contains("net-new") && note.contains("Explore"), "none note: {note}");

        // Old binary (empty reason): the legacy miss flag still drives the note.
        let old: DigestQuery = serde_json::from_str(r#"{"miss":true}"#).expect("old digest payload");
        let v = payload("anything", &old, &[]);
        assert_eq!(v["report"]["reason"], "", "old payload exposes the defaulted report honestly: {v}");
        assert!(v["note"].as_str().expect("note").contains("net-new"), "miss fallback note: {v}");
    }

    #[test]
    fn weak_report_withholds_planning_fields_and_drops_term_files() {
        // On `weak` the note forbids planning on top of the payload, so the
        // planning fields are withheld from stdout (the orchestrator would
        // pay for them and then discard them by contract): empty arrays +
        // `planningWithheld: true`. The honest counts keep the true sizes.
        // The per-term `files` are DROPPED from stdout (redundant with
        // `anchorsDetail`); only `{term, tier, lang}` remain. The recorded
        // `feature.query` event keeps the full per-term report incl. files.
        let weak: DigestQuery = serde_json::from_str(
            r#"{"query":["cancelado"],
                "matched_terms":[{"term":"cancel","count":3,"samples":["src/cancel.cs"]}],
                "slices":[{"label":"crud","recurrence":4,"entities":["Title"]},{"label":"list","recurrence":2,"entities":["Title"]}],
                "contracts":[{"name":"ITenant","implementors":2}],
                "hubs":[{"module":"src/service.cs","degree":9}],
                "files":["src/cancel.cs","src/other.cs"],
                "files_detail":[{"file":"src/cancel.cs","score_x1024":2048,"terms":["cancel"]}],
                "miss":false,
                "report":{"matched":1,"total":2,"reason":"weak","terms":[
                    {"term":"cancelado","tier":"lexicon","lang":"pt-en","files":["a.cs","b.cs","c.cs","d.cs","e.cs"]},
                    {"term":"hierarquia","tier":"none","lang":"","files":[]}]}}"#,
        )
        .expect("weak digest payload");
        let v = payload("cancelar titulo", &weak, &[]);
        assert_eq!(v["planningWithheld"], json!(true));
        for field in ["slices", "contracts", "hubs", "anchors", "anchorsDetail"] {
            assert_eq!(v[field], json!([]), "{field} must be withheld on weak: {v}");
        }
        // Honest counts survive the withholding.
        assert_eq!(v["sliceMatchCount"], 2, "true slice count stays visible: {v}");
        // The per-term report keeps {term, tier, lang} but DROPS `files` on
        // stdout (the evidence lives in `anchorsDetail` / the event).
        let row = &v["report"]["terms"][0];
        assert_eq!(row["term"], "cancelado");
        assert_eq!(row["tier"], "lexicon");
        assert_eq!(row["lang"], "pt-en");
        assert!(row.get("files").is_none(), "per-term files dropped from stdout: {v}");
        assert!(
            v["note"].as_str().expect("note").contains("withheld"),
            "weak note must explain the withholding: {v}"
        );

        // `strong` keeps the planning fields — the withholding is keyed on
        // the contract, not unconditional.
        let strong: DigestQuery = serde_json::from_str(
            r#"{"query":["cancel"],"files":["src/cancel.cs"],"miss":false,
                "report":{"matched":1,"total":1,"reason":"strong","terms":[]}}"#,
        )
        .expect("strong digest payload");
        let v = payload("cancel title", &strong, &[]);
        assert_eq!(v["planningWithheld"], json!(false));
        assert_eq!(v["anchors"], json!(["src/cancel.cs"]), "strong keeps anchors: {v}");
    }

    #[test]
    fn bridged_weak_returns_planning_with_a_caveat_note() {
        // A `weak` answer the scan flagged `bridged: true`: the only missing
        // strength is the absence of an exact/fold hit, and the trigram RESCUE
        // carried a non-thin query (the user's vocabulary matched the code's by
        // shared-root FORM). Unlike a plain weak, the planning fields are NOT
        // withheld — re-querying in the repo's words would just re-find what the
        // fuzzy rung already bridged. The note flags the approximate hit and to
        // confirm by reading.
        let bridged: DigestQuery = serde_json::from_str(
            r#"{"query":["cancelado"],
                "matched_terms":[{"term":"cancel","count":3,"samples":["src/cancel.cs"]}],
                "slices":[{"label":"crud","recurrence":4,"entities":["Title"]}],
                "contracts":[{"name":"ITenant","implementors":2}],
                "hubs":[{"module":"src/service.cs","degree":9}],
                "files":["src/cancel.cs"],
                "files_detail":[{"file":"src/cancel.cs","score_x1024":2048,"terms":["cancel"]}],
                "miss":false,
                "report":{"matched":1,"total":1,"reason":"weak","bridged":true,"terms":[
                    {"term":"cancelado","tier":"trigram","lang":"trigram","files":["src/cancel.cs"]}]}}"#,
        )
        .expect("bridged digest payload");
        let v = payload("cancelar titulo", &bridged, &[]);
        assert_eq!(v["planningWithheld"], json!(false), "a fuzzy bridge is not withheld: {v}");
        assert_eq!(v["anchors"], json!(["src/cancel.cs"]), "anchors returned for the bridged hit: {v}");
        assert_eq!(v["report"]["bridged"], json!(true), "the marker rides along: {v}");
        assert_eq!(v["sliceMatchCount"], 1, "honest counts stay: {v}");
        assert_ne!(v["slices"], json!([]), "planning fields survive the bridge: {v}");
        let note = v["note"].as_str().expect("note");
        assert!(note.contains("FUZZY"), "note flags the fuzzy bridge: {v}");
        // The bridged note must NOT steer a re-query (the plain-weak note does):
        // the fuzzy rescue already bridged the vocabulary.
        assert!(!note.contains("re-query"), "bridged note does not steer a re-query: {v}");
    }

    #[test]
    fn note_emits_all_breaks_first_contract() {
        // A multi-concern answer (scan split the query into ≥2 disconnected
        // groups) prepends the "analyze only after ALL breaks return" contract
        // to the note, and emits the labelled `concerns` array — each concern
        // with its OWN ranked anchors/anchorsDetail/reason. The contract steers
        // the AI to read every split before reasoning (so the densest concern
        // does not drown the others).
        let split: DigestQuery = serde_json::from_str(
            r#"{"query":["tenant","export"],"files":["t.cs","e.cs"],"miss":false,
                "report":{"matched":2,"total":2,"reason":"strong","terms":[]},
                "concerns":[
                    {"label":"tenant","concepts":["tenant"],"files":["t.cs"],
                     "files_detail":[{"file":"t.cs","score_x1024":2048,"terms":["tenant"]}],"reason":"strong"},
                    {"label":"export","concepts":["export"],"files":["e.cs"],
                     "files_detail":[{"file":"e.cs","score_x1024":1024,"terms":["export"]}],"reason":"weak"}]}"#,
        )
        .expect("multi-concern digest payload");
        let v = payload("tenant export", &split, &[]);
        let note = v["note"].as_str().expect("note");
        // The contract instruction rides on a ≥2-concern answer.
        assert!(note.contains("MULTIPLE concerns"), "multi-concern note carries the contract: {note}");
        assert!(note.contains("AFTER all the splits returned"), "the all-breaks-first instruction is present: {note}");
        // The base reason note (strong) is preserved after the contract prefix.
        assert!(note.contains("anchors"), "base reason note still rides along: {note}");

        // The labelled concerns array is emitted, each with its own anchors,
        // per-anchor audit (scoreX1024 + terms) and reason.
        let concerns = v["concerns"].as_array().expect("concerns array on a split");
        assert_eq!(concerns.len(), 2, "one row per concern: {v}");
        assert_eq!(concerns[0]["label"], "tenant");
        assert_eq!(concerns[0]["concepts"], json!(["tenant"]));
        assert_eq!(concerns[0]["reason"], "strong");
        assert_eq!(concerns[0]["anchors"], json!(["t.cs"]));
        assert_eq!(concerns[0]["anchorsDetail"][0]["scoreX1024"], 2048, "per-concern audit mirrors the top-level shape: {v}");
        assert_eq!(concerns[0]["anchorsDetail"][0]["terms"], json!(["tenant"]));
        assert_eq!(concerns[1]["label"], "export");
        assert_eq!(concerns[1]["reason"], "weak");

        // Single-concern compat: NO `concerns` key, and the note carries NO
        // contract prefix — the flat anchors already ARE the one concern, and
        // the existing stdout shape is unchanged.
        let single: DigestQuery = serde_json::from_str(
            r#"{"query":["tenant"],"files":["t.cs"],"miss":false,
                "report":{"matched":1,"total":1,"reason":"strong","terms":[]}}"#,
        )
        .expect("single-concern digest payload");
        let v = payload("tenant", &single, &[]);
        assert!(v.get("concerns").is_none(), "single concern emits no concerns key: {v}");
        let note = v["note"].as_str().expect("note");
        assert!(!note.contains("MULTIPLE concerns"), "single concern carries no contract prefix: {note}");

        // Exactly ONE concern returned by scan is still single-concern by
        // contract (1 group = the flat list) — the prefix only rides on ≥2.
        let one: DigestQuery = serde_json::from_str(
            r#"{"query":["tenant"],"files":["t.cs"],"miss":false,
                "report":{"matched":1,"total":1,"reason":"strong","terms":[]},
                "concerns":[{"label":"tenant","concepts":["tenant"],"files":["t.cs"],
                    "files_detail":[{"file":"t.cs","score_x1024":2048,"terms":["tenant"]}],"reason":"strong"}]}"#,
        )
        .expect("one-concern digest payload");
        let v = payload("tenant", &one, &[]);
        let note = v["note"].as_str().expect("note");
        assert!(!note.contains("MULTIPLE concerns"), "a single returned concern is not multi: {note}");
        // The concerns array is still emitted faithfully (scan returned it).
        assert_eq!(v["concerns"].as_array().expect("concerns").len(), 1, "the one concern is emitted: {v}");

        // Byte-stable: the same multi-concern input serializes identically.
        let a = serde_json::to_string(&payload("tenant export", &split, &[])).expect("ser");
        let b = serde_json::to_string(&payload("tenant export", &split, &[])).expect("ser");
        assert_eq!(a, b, "multi-concern payload is byte-stable");
    }

    /// Build a published-index slice (`Scan::digest().terms`) for the candidate
    /// tests — the catalogue order is preserved verbatim by the projection.
    fn idx(rows: &[(&str, usize, &[&str])]) -> Vec<DigestTerm> {
        rows.iter()
            .map(|(term, count, samples)| DigestTerm {
                term: (*term).to_string(),
                count: *count,
                specificity_x1024: 0,
                samples: samples.iter().map(|s| (*s).to_string()).collect(),
                purpose: None,
            })
            .collect()
    }

    #[test]
    fn non_strong_predicate_gates_the_candidates_menu() {
        // The menu rides on every NON-strong outcome and only those: weak,
        // none, generated_only, a legacy `miss`, and an empty reason (older
        // scan binary). `strong` is the single case that omits it.
        assert!(non_strong("weak", false));
        assert!(non_strong("none", false));
        assert!(non_strong("generated_only", false));
        assert!(non_strong("", true), "legacy miss with empty reason is non-strong");
        assert!(non_strong("", false), "empty reason (old binary) is non-strong");
        assert!(!non_strong("strong", false), "strong omits the menu");
        // A `miss` flag never overrides an explicit `strong` reason.
        assert!(!non_strong("strong", true), "explicit strong wins over the legacy flag");
    }

    #[test]
    fn candidates_from_index_is_bounded_byte_stable_and_reuses_publish_order() {
        // The menu is the PUBLISHED term index projected verbatim: {term, count}
        // in the catalogue's own order (NOT re-sorted here), bounded to
        // CANDIDATES_MAX rows. `samples` is NOT emitted — the translator reads
        // the `term` column only. Catalogue order is rank-desc (the scan tool's
        // `build_terms`) — kept as given so the menu matches byte-for-byte.
        let index = idx(&[
            ("supplier", 40, &["src/supplier.cs", "src/supplier_repo.cs", "src/extra.cs"]),
            ("contract", 22, &["src/contract.cs"]),
            ("payable", 9, &[]),
        ]);
        let c = candidates_from_index(&index);
        assert_eq!(c.len(), 3, "one row per catalogue term: {c:?}");
        // Order preserved verbatim from the catalogue (no re-sort).
        assert_eq!(c[0]["term"], "supplier");
        assert_eq!(c[1]["term"], "contract");
        assert_eq!(c[2]["term"], "payable");
        // Shape: term + count ONLY — samples dropped from stdout.
        assert_eq!(c[0]["count"], 40);
        assert!(c[0].get("samples").is_none(), "samples dropped from stdout: {c:?}");
        let row = c[0].as_object().expect("candidate row is an object");
        assert_eq!(row.len(), 2, "exactly term + count per row: {c:?}");
        // Row cap: a catalogue past CANDIDATES_MAX trims to the bound, head-first.
        let big: Vec<DigestTerm> = (0..CANDIDATES_MAX + 25)
            .map(|i| DigestTerm { term: format!("t{i:04}"), count: 1, specificity_x1024: 0, samples: Vec::new(), purpose: None })
            .collect();
        let cb = candidates_from_index(&big);
        assert_eq!(cb.len(), CANDIDATES_MAX, "row count bounded by CANDIDATES_MAX");
        assert_eq!(cb[0]["term"], "t0000", "head of the published order survives the cap");
        // Byte-stable for the same input.
        let a = serde_json::to_string(&json!(candidates_from_index(&index))).expect("ser");
        let b = serde_json::to_string(&json!(candidates_from_index(&index))).expect("ser");
        assert_eq!(a, b);
    }

    #[test]
    fn weak_result_includes_a_nonempty_bounded_candidates_menu() {
        // On a weak result the payload attaches `candidates` — the translator's
        // menu drawn from the PUBLISHED term index — even though the planning
        // fields are withheld. The menu is the repo's real code vocabulary, so
        // an orchestration-layer (Haiku) step can map a cross-lingual intent
        // onto it and re-query. Bounded, byte-stable shape.
        let weak: DigestQuery = serde_json::from_str(
            r#"{"query":["cancelado"],
                "files":["src/cancel.cs"],
                "miss":false,
                "report":{"matched":1,"total":2,"reason":"weak","terms":[
                    {"term":"cancelado","tier":"stem","lang":"pt","files":["src/cancel.cs"]},
                    {"term":"hierarquia","tier":"none","lang":"","files":[]}]}}"#,
        )
        .expect("weak digest payload");
        let index = idx(&[
            ("supplier", 40, &["src/supplier.cs", "src/supplier_repo.cs"]),
            ("contract", 22, &["src/contract.cs"]),
        ]);
        let v = payload("cancelar titulo", &weak, &index);
        // Planning is withheld (plain weak) but the menu is present.
        assert_eq!(v["planningWithheld"], json!(true), "weak still withholds planning: {v}");
        let cands = v["candidates"].as_array().expect("candidates present on weak");
        assert!(!cands.is_empty(), "candidates is the real vocabulary menu, not empty: {v}");
        assert_eq!(cands.len(), 2, "one row per published term: {v}");
        assert_eq!(cands[0]["term"], "supplier", "publish order preserved: {v}");
        assert_eq!(cands[0]["count"], 40);
        assert!(cands[0].get("samples").is_none(), "samples dropped from stdout: {v}");
        // Byte-stable: the same inputs serialize identically.
        let a = serde_json::to_string(&payload("cancelar titulo", &weak, &index)).expect("ser");
        let b = serde_json::to_string(&payload("cancelar titulo", &weak, &index)).expect("ser");
        assert_eq!(a, b, "candidates payload is byte-stable");
    }

    #[test]
    fn none_and_bridged_results_also_carry_the_candidates_menu() {
        // `none` (no precedent) is exactly when the translator needs the menu
        // most — it must be present and non-empty when a catalogue exists.
        let none: DigestQuery = serde_json::from_str(
            r#"{"query":["zzz"],"miss":true,"report":{"matched":0,"total":1,"reason":"none","terms":[{"term":"zzz","tier":"none","lang":"","files":[]}]}}"#,
        )
        .expect("none digest");
        let index = idx(&[("supplier", 40, &["src/supplier.cs"]), ("contract", 22, &[])]);
        let v = payload("zzz", &none, &index);
        let cands = v["candidates"].as_array().expect("candidates present on none");
        assert_eq!(cands.len(), 2, "none carries the full menu: {v}");
        assert_eq!(cands[0]["term"], "supplier");

        // A bridged weak is still NON-strong by reason, so the menu rides along
        // too (harmless — the translator can ignore it given the bridge).
        let bridged: DigestQuery = serde_json::from_str(
            r#"{"query":["cancelado"],"files":["src/cancel.cs"],"miss":false,
                "report":{"matched":1,"total":1,"reason":"weak","bridged":true,"terms":[
                    {"term":"cancelado","tier":"lexicon","lang":"pt-en","files":["src/cancel.cs"]}]}}"#,
        )
        .expect("bridged digest");
        let v = payload("cancelar titulo", &bridged, &index);
        assert!(v.get("candidates").is_some(), "bridged weak is non-strong → menu present: {v}");
        // The bridge still returns planning fields (the existing contract).
        assert_eq!(v["planningWithheld"], json!(false), "bridge keeps planning: {v}");
    }

    #[test]
    fn strong_result_omits_candidates() {
        // On `strong` the anchors already ARE the evidence; the menu is omitted
        // to keep the strong path lean — even if a catalogue is handed in.
        let strong: DigestQuery = serde_json::from_str(
            r#"{"query":["supplier"],"files":["src/supplier.cs"],"miss":false,
                "report":{"matched":1,"total":1,"reason":"strong","terms":[
                    {"term":"supplier","tier":"exact","lang":"","files":["src/supplier.cs"]}]}}"#,
        )
        .expect("strong digest");
        let index = idx(&[("supplier", 40, &["src/supplier.cs"]), ("contract", 22, &[])]);
        let v = payload("supplier", &strong, &index);
        assert!(v.get("candidates").is_none(), "strong omits the candidates menu: {v}");
        assert_eq!(v["anchors"], json!(["src/supplier.cs"]), "strong keeps anchors: {v}");
    }
}
