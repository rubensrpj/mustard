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
//! opens the scan JSON), the wide `candidates` pool (~25 fused rows with
//! per-file evidence — the menu the ORCHESTRATOR selects 5-10 files from,
//! in-session, no subprocess), and a `miss` flag + note. `miss=true` means no
//! repo precedent matched — the AI must treat it as net-new (do NOT conclude
//! "absent" blindly: the term index has false negatives and no synonyms;
//! confirm by reading). Fail-open: a missing model / unavailable tool yields
//! a miss result.

use std::path::Path;

use mustard_core::domain::scan::{DigestQuery, DigestTerm, FileDetail, RankFile};
use mustard_core::io::fs as mfs;
use mustard_core::Scan;
use serde_json::{json, Value};

#[path = "feature_retrieval.rs"]
// The RRF/fusion cluster (pure, spawn-free) lives in the sibling file; declared
// here via #[path] so it is a child of `feature` without touching commands/mod.rs.
mod feature_retrieval;

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
/// what existed, and the successful re-query returns the withheld fields.
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

/// Max `vocabulary` rows emitted on a NON-strong result — the menu the
/// ORCHESTRATOR (the Claude session that ran this command) re-queries against
/// in the code's own vocabulary. The source is the PUBLISHED domain-term index
/// (`Scan::digest().terms`, already `build_terms`-ranked and capped at the
/// scan tool's `MAX_TERMS`); this is the emission-side bound on top of that,
/// so the menu stays a few KB regardless of the catalogue's published cap. The
/// published order is preserved verbatim (byte-stable), never re-derived or
/// re-sorted here.
const VOCABULARY_MAX: usize = 80;

/// `true` when the report's strength is NOT `strong` — i.e. the orchestrator
/// must re-query in the code's own vocabulary before planning. Drives whether
/// `vocabulary` (the re-query menu) is attached: emitted on
/// `weak`/`none`/`generated_only` or any legacy `miss`, omitted on `strong`
/// (the strong path stays lean — the anchors already ARE the evidence).
fn non_strong(reason: &str, miss: bool) -> bool {
    reason != "strong" && (matches!(reason, "weak" | "none" | "generated_only") || miss || reason.is_empty())
}

/// Project the PUBLISHED domain-term index into the bounded `vocabulary` menu:
/// each row is `{ term, count }` drawn verbatim from the catalogue
/// (`Scan::digest().terms`) — REUSED, never re-derived or re-sorted, so the
/// published rank order (frequency/rank desc, term asc) carries through
/// byte-stably. Bounded by [`VOCABULARY_MAX`] (rows). Pure (no spawn, no IO) so
/// the shape is unit-testable without the scan binary.
///
/// `samples` (a couple of "where this vocabulary lives" paths) was DROPPED from
/// stdout: the consumer — the orchestrator re-querying against this menu —
/// reads the `term` column ONLY, never the samples; they were emitted solely
/// for a manual fallback. Dropping them trims the weak/none JSON without
/// affecting the term menu.
fn vocabulary_from_index(index: &[DigestTerm]) -> Vec<serde_json::Value> {
    index
        .iter()
        .take(VOCABULARY_MAX)
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
/// `vocabulary` menu — is unit-testable without the scan binary. `index` is the
/// PUBLISHED domain-term catalogue (`Scan::digest().terms`), used ONLY on a
/// non-strong result to build `vocabulary`; on a strong result it is ignored
/// (and the caller passes an empty slice, skipping the extra fetch).
fn payload(intent: &str, q: &DigestQuery, index: &[DigestTerm]) -> serde_json::Value {
    let withhold = withhold_planning(q.report.reason.as_str(), q.report.bridged);
    let mut out = json!({
        "intent": intent,
        // `queryTerms` (the echoed tokenization of `--intent`) was dropped from
        // STDOUT — the orchestrator already holds the intent it passed in, and
        // the report names every term that mattered. The recorded
        // `analyze.digest.used` EVENT keeps `queryTerms` for adherence; only
        // the stdout payload loses it.
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
            // payload weight on a wide query (32 terms × N files each). Same
            // precedent as the `matchedTerms` drop above.
            "terms": q.report.terms.iter().map(|t| json!({
                "term": t.term, "tier": t.tier, "lang": t.lang,
            })).collect::<Vec<_>>(),
        }),
        "note": note(q),
    });
    // On a NON-strong result, attach the re-query menu: a bounded slice of the
    // PUBLISHED domain-term index so the orchestrator can map a cross-lingual
    // intent onto the repo's real code vocabulary and re-query. Omitted on
    // `strong` — there the anchors already ARE the evidence, and the strong
    // path stays lean. Deterministic: this command only PUBLISHES the menu.
    // The non-strong fallback path (scan unavailable) has no catalogue, so it
    // passes an empty slice → an empty `vocabulary`, honestly signalling "no
    // vocabulary to offer".
    if non_strong(q.report.reason.as_str(), q.miss) {
        if let Some(obj) = out.as_object_mut() {
            obj.insert("vocabulary".to_string(), json!(vocabulary_from_index(index)));
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

/// Minimal `analyze.digest.used` payload: the queried terms + the legacy
/// hit/miss flag. This is the adherence MARKER `digest-adherence-finalize`
/// looks for ("the digest was consulted at this instant").
fn digest_used_payload(terms: &[String], q: &DigestQuery) -> serde_json::Value {
    json!({
        "queryTerms": terms,
        "miss": q.miss,
    })
}

/// Record that the scan digest answered a research round, as an
/// `analyze.digest.used` harness event. The spec is resolved HERE via [`crate::shared::context::current_spec`] (may be
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

// ---------------------------------------------------------------------------
// Rank-query preparation for the ONE `feature-bundle` spawn: the automatic
// gloss for a non-English intent, the scan-time equivalence expansion and the
// direct identifier-match floor - the inputs the bundle ranks with. The RRF
// fusion of that rank pool with the digest anchors now lives in the sibling
// `feature_retrieval` module. Every rung is FAIL-OPEN: a missing translator /
// dictionary degrades the rank query; the fused field always renders.
// ---------------------------------------------------------------------------

/// `scan rank`'s direct identifier-match floor (the tool default, pinned
/// explicitly — the calibrated product contract).
const RANK_DIRECT_BASE: u64 = 100_000;

/// English function words for the language vote — the EN side of the scan
/// dictionary's `is_non_english` heuristic, embedded compactly (a router,
/// not a classifier).
const EN_STOP: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "if", "of", "at", "by", "for", "with", "about", "into",
    "through", "before", "after", "to", "from", "in", "out", "on", "off", "over", "under", "again",
    "then", "once", "here", "there", "when", "where", "why", "how", "all", "any", "both", "each",
    "few", "more", "most", "some", "such", "no", "not", "only", "same", "than", "too", "very",
    "is", "are", "was", "were", "been", "being", "be", "have", "has", "had", "does", "did", "this",
    "that", "these", "those", "will", "would", "can", "could", "should", "must", "it", "its",
    "his", "her", "our", "their", "your", "you", "they", "she", "what", "which", "who", "as",
];

/// Portuguese function words — the accent-bearing romance side of the vote.
const PT_STOP: &[&str] = &[
    "o", "a", "os", "as", "um", "uma", "uns", "umas", "de", "do", "da", "dos", "das", "no", "na",
    "nos", "nas", "ao", "aos", "à", "às", "pelo", "pela", "pelos", "pelas", "em", "por", "para",
    "com", "sem", "sob", "sobre", "entre", "até", "e", "ou", "mas", "que", "se", "não", "sim",
    "é", "são", "foi", "foram", "ser", "sendo", "era", "eram", "está", "estão", "estava", "tem",
    "têm", "tinha", "há", "já", "mais", "menos", "muito", "muitos", "como", "quando", "onde",
    "qual", "quais", "quem", "isso", "isto", "esse", "essa", "esses", "essas", "este", "esta",
    "estes", "estas", "ele", "ela", "eles", "elas", "você", "nós", "eu", "seu", "sua", "seus",
    "suas", "meu", "minha", "nosso", "nossa", "também", "depois", "antes", "agora", "aqui",
    "cada", "todo", "toda", "todos", "todas", "outro", "outra", "outros", "outras", "mesmo",
    "mesma", "ainda", "então", "pois", "porque",
];

/// `true` when the intent reads as NON-English: the PT function words outvote
/// the English ones, or the vote ties WITH Latin-accent evidence present —
/// the exact rule of the scan dictionary's `is_non_english`. Biased to send:
/// a falsely-English verdict merely skips the gloss; a falsely-foreign one
/// costs a single no-op MT call (the sidecar passes English through).
fn looks_non_english(intent: &str) -> bool {
    let mut en_hits = 0usize;
    let mut pt_hits = 0usize;
    for word in intent.split(|c: char| !c.is_alphanumeric()) {
        if word.is_empty() {
            continue;
        }
        let w = word.to_lowercase();
        if EN_STOP.contains(&w.as_str()) {
            en_hits += 1;
        }
        if PT_STOP.contains(&w.as_str()) {
            pt_hits += 1;
        }
    }
    let lower = intent.to_lowercase();
    let has_accent = super::scan_equivalences::fold_tok(&lower) != lower;
    pt_hits > en_hits || (has_accent && pt_hits >= en_hits)
}

/// Auto-gloss: translate a non-English-looking intent through the OPTIONAL
/// local `mustard-translate` sidecar, returning the English text to ride
/// inside the effective intent. `None` (no gloss, original behavior) when the
/// intent votes English, the translator is absent or fails, the sidecar
/// detected English anyway, or the translation adds nothing — fail-open.
fn auto_gloss(intent: &str) -> Option<String> {
    if !looks_non_english(intent) {
        return None;
    }
    let translation = crate::shared::translate::Translate::locate()?.text(intent)?;
    if translation.detected == "en" {
        return None;
    }
    let en = translation.en.trim().to_string();
    if en.is_empty() || en.eq_ignore_ascii_case(intent.trim()) {
        return None;
    }
    Some(en)
}

/// Expand the raw intent with the scan-time equivalence tokens (the measured
/// C2 query shape: raw PT + added EN tokens): each intent token ≥3 chars is
/// accent-folded and looked up EXACTLY in the `grain.equivalences.json` map;
/// hits append their tokens, deduped across the whole intent in
/// first-occurrence order. No hit → the intent passes through verbatim.
fn expand_query(intent: &str, equiv: &std::collections::BTreeMap<String, Vec<String>>) -> String {
    let mut added: Vec<String> = Vec::new();
    for tok in intent.split(|c: char| !c.is_alphanumeric()) {
        if tok.chars().count() < 3 {
            continue;
        }
        let key = super::scan_equivalences::fold_tok(tok);
        if let Some(toks) = equiv.get(&key) {
            for t in toks {
                if !added.contains(t) {
                    added.push(t.clone());
                }
            }
        }
    }
    if added.is_empty() {
        intent.to_string()
    } else {
        format!("{intent} {}", added.join(" "))
    }
}

/// Attach the ADDITIVE retrieval fields to the insumos payload: `insumos`
/// (the RRF-fused top-10 short-list — ALWAYS present, possibly empty),
/// `candidates` (the WIDE fused pool with per-file evidence — ALWAYS present,
/// the in-session selection menu) and `gloss` (only when the auto-gloss
/// fired). No existing field is touched — the run output is byte-compared in
/// gates, and every key is additive + deterministic.
fn attach_retrieval(
    v: &mut Value,
    intent: &str,
    gloss: Option<&str>,
    detail: &[FileDetail],
    rank_rows: &[RankFile],
    equiv: &std::collections::BTreeMap<String, Vec<String>>,
) {
    // The bundle already ran the ranker; slice its pool into the two shapes the
    // fusion consumes: the top-INSUMOS_MAX file list (insumos) and the full rows
    // with per-file terms (candidates). rank@10 == rank_detail@25[..10] (the
    // ranker sorts by a total order, then truncates), so this is byte-identical
    // to the two separate `rank` spawns it replaces.
    let insumos_rank: Vec<String> =
        rank_rows.iter().take(feature_retrieval::INSUMOS_MAX).map(|r| r.file.clone()).collect();
    let pool_rank: Vec<(String, Vec<String>)> =
        rank_rows.iter().map(|r| (r.file.clone(), r.terms.clone())).collect();
    let rows = feature_retrieval::insumos_rows(&insumos_rank, detail);
    let pool = feature_retrieval::build_pool(&pool_rank, detail);
    let uncovered = uncovered_terms(intent, equiv, &pool);
    let pool_rows = feature_retrieval::candidates_rows(&pool);
    if let Some(obj) = v.as_object_mut() {
        obj.insert("insumos".to_string(), json!(rows));
        obj.insert("candidates".to_string(), json!(pool_rows));
        obj.insert("uncovered".to_string(), json!(uncovered));
        if let Some(en) = gloss {
            obj.insert("gloss".to_string(), json!(en));
        }
    }
}

/// The pool's BLIND SPOTS — request concepts with NO representation in the
/// candidate evidence. For each intent token (≥4 chars, at least one letter,
/// not an EN/PT function word), the probe set is its accent-folded form plus
/// every equivalence expansion of it; the concept counts covered when ANY
/// probe matches ANY candidate's matched-term evidence (folded; exact, or
/// prefix-either-way with both sides ≥4 chars, so `cliente`/`clientes` and
/// `lista`/`listar` join). Everything else is emitted as a `{term, tried}`
/// row, term-ascending (byte-stable). Deliberately a LOWER bound on
/// blindness: a term matched by an irrelevant file still counts covered — the
/// field DIRECTS the existence gate (one targeted enumeration per row), it
/// never replaces it.
fn uncovered_terms(
    intent: &str,
    equiv: &std::collections::BTreeMap<String, Vec<String>>,
    pool: &[feature_retrieval::Candidate],
) -> Vec<Value> {
    let evidence: Vec<String> = pool
        .iter()
        .flat_map(|c| c.terms.iter())
        .map(|t| super::scan_equivalences::fold_tok(&t.to_lowercase()))
        .collect();
    let hits = |probe: &str| {
        evidence.iter().any(|e| {
            e == probe
                || (probe.len() >= 4
                    && e.len() >= 4
                    && (e.starts_with(probe) || probe.starts_with(e.as_str())))
        })
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut rows: Vec<(String, Vec<String>)> = Vec::new();
    for raw in intent.split(|c: char| !c.is_alphanumeric()) {
        if raw.chars().count() < 4 || !raw.chars().any(|c| c.is_alphabetic()) {
            continue;
        }
        let lower = raw.to_lowercase();
        if EN_STOP.contains(&lower.as_str()) || PT_STOP.contains(&lower.as_str()) {
            continue;
        }
        let folded = super::scan_equivalences::fold_tok(&lower);
        if !seen.insert(folded.clone()) {
            continue;
        }
        let mut tried = vec![folded.clone()];
        for t in equiv.get(&folded).map(Vec::as_slice).unwrap_or(&[]) {
            let f = super::scan_equivalences::fold_tok(&t.to_lowercase());
            if !tried.contains(&f) {
                tried.push(f);
            }
        }
        if tried.iter().any(|p| hits(p)) {
            continue;
        }
        rows.push((lower, tried));
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows.into_iter().map(|(term, tried)| json!({ "term": term, "tried": tried })).collect()
}

/// Run the research step: print the feature insumos JSON for `intent`.
///
/// PURE DETERMINISTIC — no `claude` subprocess (an earlier opt-in LLM hop
/// spawned one per call and died of its cold start; the selection now happens
/// IN-SESSION, over the published `candidates` pool). Cross-lingual
/// translation arrives two fail-open ways: the ORCHESTRATION layer may pass
/// the english translation INSIDE `--intent` (`--intent "<user prompt PT>
/// <english>"`), and a non-English-looking intent is ALSO auto-glossed
/// through the optional local `mustard-translate` sidecar (`"<original> --
/// <en>"` feeds the digest tokenization; `domain_terms` dedups the union).
/// The digest is queried once; the pagerank short-list is RRF-fused into the
/// additive `insumos` field (top-10) and the WIDE `candidates` pool (~25 rows
/// with per-file evidence) the orchestrator selects from. On a NON-strong
/// result the `vocabulary` menu still rides along — a deterministic fallback
/// the orchestrator can re-query against.
pub fn run(intent: &str, root: &Path) {
    let gloss = auto_gloss(intent);
    let effective = gloss
        .as_ref()
        .map_or_else(|| intent.to_string(), |en| format!("{intent} -- {en}"));
    let terms = domain_terms(&effective);
    let model = root.join(".claude").join("grain.model.json");
    let dict = root.join(".claude").join("grain.dictionary.json");
    // The equivalence map + expanded rank query are computed ONCE here (they were
    // reloaded inside each of the removed rank/rank_detail spawns) and fed to the
    // single bundle call; `uncovered_terms` reuses the same map. `expand_query`
    // uses the ORIGINAL intent (not the gloss-augmented `effective`), matching the
    // old insumos_rows/rank_pool contract.
    let equiv = super::scan_equivalences::load_equivalences(root);
    let rank_query = expand_query(intent, &equiv);

    let payload = match Scan::locate().feature_bundle(&model, &dict, &terms, &rank_query, feature_retrieval::POOL_MAX, RANK_DIRECT_BASE) {
        Ok(bundle) => {
            let q = &bundle.digest;
            // The adherence marker is emitted BEFORE the println below so the
            // stdout contract stays byte-stable (telemetry never interleaves
            // output).
            emit_digest_used_event(digest_used_payload(&terms, q));
            // On a NON-strong result the `vocabulary` fallback menu rides the FULL
            // term index - the bundle already returned it from the SAME model parse
            // (no second `digest` spawn). On `strong`, `payload` ignores the slice,
            // so the empty one is correct and the lean path stays lean.
            let index: &[DigestTerm] = if non_strong(q.report.reason.as_str(), q.miss) {
                &bundle.terms
            } else {
                &[]
            };
            let mut v = payload(intent, q, index);
            // Additive retrieval fields: the RRF-fused `insumos` short-list + the
            // wide `candidates` selection pool (+ `gloss` when the auto-gloss
            // fired), fused IN-SESSION from the bundle rank pool - no spawn.
            attach_retrieval(&mut v, intent, gloss.as_deref(), &q.files_detail, &bundle.rank, &equiv);
            v
        }
        Err(err) => {
            eprintln!("feature: scan bundle unavailable: {err}");
            let mut v = json!({
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
                // Non-strong (`none`/`miss`), so the `vocabulary` key is present
                // for a stable shape — but empty: the scan model is unavailable,
                // so there is no published vocabulary to re-query against.
                "vocabulary": [],
                "note": "scan model unavailable — run `mustard-rt run scan` first; treat as net-new until then",
            });
            // `insumos` + `candidates` are part of the stable shape — attached
            // on the fallback too (an unavailable digest usually means an
            // unavailable ranker, so both degrade to empty lists, honestly).
            attach_retrieval(&mut v, intent, gloss.as_deref(), &[], &[], &equiv);
            v
        }
    };
    // The FULL digest goes to a file (the single source of truth for the long
    // tail); stdout carries only a COMPACT summary. Field report: the orchestrator
    // paid the ~22 KB payload twice — once as full stdout, once re-reading a
    // self-captured file — and used only ~6 anchors + the reason. Writing the file
    // here removes the capture dance, and the compact stdout drops the
    // reference-only bulk (`report.terms`, `vocabulary`) the common path never
    // reads inline — `candidates` (the in-session selection pool) stays inline:
    // the orchestrator selects from it without a second read. Fail-open: a write
    // failure just means the deep-tail detail is unavailable; stdout (the
    // actionable summary) still prints.
    let digest_path = root.join(".claude").join("feature-digest.json");
    let full = serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".into());
    let _ = mfs::write_atomic(&digest_path, full.as_bytes());
    println!(
        "{}",
        serde_json::to_string_pretty(&compact_digest(&payload)).unwrap_or_else(|_| "{}".into())
    );
}

/// Trim the full digest payload to the COMPACT stdout summary: the actionable
/// fields the orchestrator reads inline (anchors + per-anchor provenance,
/// `candidates` — the in-session selection pool with its evidence lines —
/// slices, contracts, hubs, stacks, `report.reason`, `note`) MINUS the
/// reference-only bulk that belongs in the file — the 32-term `report.terms`
/// tier list (debug; the validator re-runs its own digest) and the
/// `vocabulary` re-query menu (only consulted on a weak/none re-translation,
/// a file-read deep-dive). A `detail` pointer names the file with the
/// complete payload.
fn compact_digest(full: &Value) -> Value {
    let mut c = full.clone();
    if let Some(obj) = c.as_object_mut() {
        obj.remove("vocabulary");
        if let Some(rep) = obj.get_mut("report").and_then(Value::as_object_mut) {
            if let Some(terms) = rep.remove("terms") {
                rep.insert("termCount".to_string(), json!(terms.as_array().map_or(0, Vec::len)));
            }
        }
        obj.insert("detail".to_string(), json!(".claude/feature-digest.json"));
    }
    c
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
        // NOT in stdout — they were dropped to trim the payload (the
        // orchestrator holds the intent).
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
        // THAT the digest answered, not the full report. Deterministic for
        // the same inputs.
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
        // `anchorsDetail`); only `{term, tier, lang}` remain.
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
    fn non_strong_predicate_gates_the_vocabulary_menu() {
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
    fn vocabulary_from_index_is_bounded_byte_stable_and_reuses_publish_order() {
        // The menu is the PUBLISHED term index projected verbatim: {term, count}
        // in the catalogue's own order (NOT re-sorted here), bounded to
        // VOCABULARY_MAX rows. `samples` is NOT emitted — the re-query reads
        // the `term` column only. Catalogue order is rank-desc (the scan tool's
        // `build_terms`) — kept as given so the menu matches byte-for-byte.
        let index = idx(&[
            ("supplier", 40, &["src/supplier.cs", "src/supplier_repo.cs", "src/extra.cs"]),
            ("contract", 22, &["src/contract.cs"]),
            ("payable", 9, &[]),
        ]);
        let c = vocabulary_from_index(&index);
        assert_eq!(c.len(), 3, "one row per catalogue term: {c:?}");
        // Order preserved verbatim from the catalogue (no re-sort).
        assert_eq!(c[0]["term"], "supplier");
        assert_eq!(c[1]["term"], "contract");
        assert_eq!(c[2]["term"], "payable");
        // Shape: term + count ONLY — samples dropped from stdout.
        assert_eq!(c[0]["count"], 40);
        assert!(c[0].get("samples").is_none(), "samples dropped from stdout: {c:?}");
        let row = c[0].as_object().expect("vocabulary row is an object");
        assert_eq!(row.len(), 2, "exactly term + count per row: {c:?}");
        // Row cap: a catalogue past VOCABULARY_MAX trims to the bound, head-first.
        let big: Vec<DigestTerm> = (0..VOCABULARY_MAX + 25)
            .map(|i| DigestTerm { term: format!("t{i:04}"), count: 1, specificity_x1024: 0, samples: Vec::new(), purpose: None })
            .collect();
        let cb = vocabulary_from_index(&big);
        assert_eq!(cb.len(), VOCABULARY_MAX, "row count bounded by VOCABULARY_MAX");
        assert_eq!(cb[0]["term"], "t0000", "head of the published order survives the cap");
        // Byte-stable for the same input.
        let a = serde_json::to_string(&json!(vocabulary_from_index(&index))).expect("ser");
        let b = serde_json::to_string(&json!(vocabulary_from_index(&index))).expect("ser");
        assert_eq!(a, b);
    }

    #[test]
    fn weak_result_includes_a_nonempty_bounded_vocabulary_menu() {
        // On a weak result the payload attaches `vocabulary` — the re-query
        // menu drawn from the PUBLISHED term index — even though the planning
        // fields are withheld. The menu is the repo's real code vocabulary, so
        // the orchestrator can map a cross-lingual intent onto it and
        // re-query. Bounded, byte-stable shape.
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
        let vocab = v["vocabulary"].as_array().expect("vocabulary present on weak");
        assert!(!vocab.is_empty(), "vocabulary is the real re-query menu, not empty: {v}");
        assert_eq!(vocab.len(), 2, "one row per published term: {v}");
        assert_eq!(vocab[0]["term"], "supplier", "publish order preserved: {v}");
        assert_eq!(vocab[0]["count"], 40);
        assert!(vocab[0].get("samples").is_none(), "samples dropped from stdout: {v}");
        // Byte-stable: the same inputs serialize identically.
        let a = serde_json::to_string(&payload("cancelar titulo", &weak, &index)).expect("ser");
        let b = serde_json::to_string(&payload("cancelar titulo", &weak, &index)).expect("ser");
        assert_eq!(a, b, "vocabulary payload is byte-stable");
    }

    #[test]
    fn none_and_bridged_results_also_carry_the_vocabulary_menu() {
        // `none` (no precedent) is exactly when the re-query needs the menu
        // most — it must be present and non-empty when a catalogue exists.
        let none: DigestQuery = serde_json::from_str(
            r#"{"query":["zzz"],"miss":true,"report":{"matched":0,"total":1,"reason":"none","terms":[{"term":"zzz","tier":"none","lang":"","files":[]}]}}"#,
        )
        .expect("none digest");
        let index = idx(&[("supplier", 40, &["src/supplier.cs"]), ("contract", 22, &[])]);
        let v = payload("zzz", &none, &index);
        let vocab = v["vocabulary"].as_array().expect("vocabulary present on none");
        assert_eq!(vocab.len(), 2, "none carries the full menu: {v}");
        assert_eq!(vocab[0]["term"], "supplier");

        // A bridged weak is still NON-strong by reason, so the menu rides along
        // too (harmless — the orchestrator can ignore it given the bridge).
        let bridged: DigestQuery = serde_json::from_str(
            r#"{"query":["cancelado"],"files":["src/cancel.cs"],"miss":false,
                "report":{"matched":1,"total":1,"reason":"weak","bridged":true,"terms":[
                    {"term":"cancelado","tier":"lexicon","lang":"pt-en","files":["src/cancel.cs"]}]}}"#,
        )
        .expect("bridged digest");
        let v = payload("cancelar titulo", &bridged, &index);
        assert!(v.get("vocabulary").is_some(), "bridged weak is non-strong → menu present: {v}");
        // The bridge still returns planning fields (the existing contract).
        assert_eq!(v["planningWithheld"], json!(false), "bridge keeps planning: {v}");
    }

    #[test]
    fn strong_result_omits_vocabulary() {
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
        assert!(v.get("vocabulary").is_none(), "strong omits the vocabulary menu: {v}");
        assert_eq!(v["anchors"], json!(["src/supplier.cs"]), "strong keeps anchors: {v}");
    }

    #[test]
    fn expand_query_folds_keys_exactly_and_dedups_added_tokens() {
        let mut eq = std::collections::BTreeMap::new();
        eq.insert("conciliacao".to_string(), vec!["reconciliation".to_string(), "bank".to_string()]);
        eq.insert("extrato".to_string(), vec!["statement".to_string(), "bank".to_string()]);
        // Accent-folded lookup hits; `bank` (shared by both terms) appends once.
        let q = expand_query("onde é feita a conciliação do extrato bancário", &eq);
        assert_eq!(q, "onde é feita a conciliação do extrato bancário reconciliation bank statement");
        // No key hit (incl. sub-3-char tokens) → the intent passes verbatim.
        assert_eq!(expand_query("do it", &eq), "do it");
        assert_eq!(expand_query("payment handler", &std::collections::BTreeMap::new()), "payment handler");
    }

    #[test]
    fn looks_non_english_votes_stoplists_and_accents() {
        // Mirrors the scan dictionary's is_non_english contract.
        assert!(
            looks_non_english("onde é feita a conciliação do extrato bancário"),
            "pt function words outvote + accent evidence"
        );
        assert!(
            looks_non_english("adicionar validação no handler de criação de contrato"),
            "pt vote wins despite EN loanwords"
        );
        assert!(
            !looks_non_english("add validation to the contract creation handler"),
            "english stays english"
        );
        assert!(
            !looks_non_english("maps the naïve café names into the user profile"),
            "english wins its vote despite an accented word"
        );
    }

    #[test]
    fn insumos_field_always_attaches_and_fails_open_without_sidecars() {
        // A root with NO dictionary sidecar → the ranker is never spawned and
        // the digest audit alone carries the field (source: digest).
        let detail: Vec<FileDetail> = serde_json::from_str(
            r#"[{"file":"src/a.cs","score_x1024":90,"terms":["x"]},
                {"file":"src/b.cs","score_x1024":10,"terms":[]}]"#,
        )
        .expect("detail rows");
        let mut v = json!({ "intent": "x" });
        attach_retrieval(&mut v, "x", None, &detail, &[], &std::collections::BTreeMap::new());
        assert_eq!(
            v["insumos"],
            json!([
                { "file": "src/a.cs", "source": "digest" },
                { "file": "src/b.cs", "source": "digest" }
            ]),
            "ranker unavailable → digest top-10 alone: {v}"
        );
        assert!(v.get("gloss").is_none(), "no gloss key when the gloss did not fire: {v}");
        assert_eq!(v["intent"], json!("x"), "existing fields untouched: {v}");

        // Empty digest too → the fields STILL render, as empty arrays; a
        // fired gloss rides along as the additive `gloss` key.
        let mut v = json!({});
        attach_retrieval(&mut v, "x", Some("where is it done"), &[], &[], &std::collections::BTreeMap::new());
        assert_eq!(v["insumos"], json!([]), "insumos always present: {v}");
        assert_eq!(v["candidates"], json!([]), "candidates always present: {v}");
        assert_eq!(v["gloss"], json!("where is it done"));
    }

    #[test]
    fn stdout_never_carries_subprocess_keys() {
        // The removed `claude -p` selection hop must leave NO residue:
        // `attach_retrieval` (the only attach) adds exactly `insumos` +
        // `candidates` + `uncovered` (+ `gloss` when fired) — never
        // `insumosMode`, never a `hop` audit. Regression guard for the
        // subprocess removal.
        let detail: Vec<FileDetail> =
            serde_json::from_str(r#"[{"file":"src/a.cs","score_x1024":90,"terms":["x"]}]"#).expect("detail rows");
        let mut v = json!({ "intent": "x" });
        attach_retrieval(&mut v, "x", None, &detail, &[], &std::collections::BTreeMap::new());
        assert!(v.get("insumosMode").is_none(), "insumosMode never emitted: {v}");
        assert!(v.get("hop").is_none(), "the hop audit never emitted: {v}");
        let mut keys: Vec<&str> = v.as_object().expect("object").keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec!["candidates", "insumos", "intent", "uncovered"],
            "exactly the additive keys: {v}"
        );
    }

    #[test]
    fn candidates_pool_carries_evidence_and_is_byte_stable() {
        // The `candidates` field is the in-session selection menu: per row
        // {file, source, evidence} where evidence is ONE compact line — the
        // 1-based position per list + up to TERMS_SHOWN matched terms. Built
        // here from the digest side alone (no dictionary sidecar → no spawn),
        // the pool order (RRF score desc, path asc) is preserved verbatim.
        let detail: Vec<FileDetail> = serde_json::from_str(
            r#"[{"file":"src/a.cs","score_x1024":90,"terms":["contrato","parcela"]},
                {"file":"src/b.cs","score_x1024":10,"terms":[]}]"#,
        )
        .expect("detail rows");
        let mut v = json!({ "intent": "x" });
        attach_retrieval(&mut v, "x", None, &detail, &[], &std::collections::BTreeMap::new());
        assert_eq!(
            v["candidates"],
            json!([
                { "file": "src/a.cs", "source": "digest", "evidence": "digest#1 terms=contrato,parcela" },
                { "file": "src/b.cs", "source": "digest", "evidence": "digest#2" }
            ]),
            "one evidence line per pool row: {v}"
        );
        // Byte-stable: two identical attaches serialize to the same bytes.
        let mut v2 = json!({ "intent": "x" });
        attach_retrieval(&mut v2, "x", None, &detail, &[], &std::collections::BTreeMap::new());
        let a = serde_json::to_string(&v).expect("ser");
        let b = serde_json::to_string(&v2).expect("ser");
        assert_eq!(a, b, "candidates payload is byte-stable across runs");

        // The pool caps at POOL_MAX (25) — wider than the insumos ten, so the
        // selector sees past the RRF cut but the payload stays bounded.
        let many: Vec<FileDetail> = (0u64..30)
            .map(|i| FileDetail {
                file: format!("src/f{i:02}.cs"),
                score_x1024: 1024 - i,
                terms: Vec::new(),
            })
            .collect();
        let mut vm = json!({});
        attach_retrieval(&mut vm, "x", None, &many, &[], &std::collections::BTreeMap::new());
        let pool = vm["candidates"].as_array().expect("candidates array");
        assert_eq!(pool.len(), feature_retrieval::POOL_MAX, "pool bounded at POOL_MAX: {}", pool.len());
        assert_eq!(vm["insumos"].as_array().expect("insumos").len(), feature_retrieval::INSUMOS_MAX, "insumos stays top-10");
        // Evidence terms cap at TERMS_SHOWN per line.
        let wide: Vec<FileDetail> = vec![FileDetail {
            file: "src/w.cs".into(),
            score_x1024: 10,
            terms: (0..9).map(|i| format!("t{i}")).collect(),
        }];
        let rows = feature_retrieval::candidates_rows(&feature_retrieval::build_pool(&[], &wide));
        let ev = rows[0]["evidence"].as_str().expect("evidence line");
        assert_eq!(ev, "digest#1 terms=t0,t1,t2,t3,t4,t5", "terms capped at TERMS_SHOWN: {ev}");
    }

    #[test]
    fn uncovered_flags_only_unrepresented_concepts() {
        let pool = vec![feature_retrieval::Candidate {
            file: "src/a.ts".into(),
            source: "both",
            rank_pos: Some(1),
            digest_pos: Some(1),
            terms: vec!["contract".into(), "clientes".into(), "tabs".into(), "validacoes".into()],
        }];
        let mut equiv = std::collections::BTreeMap::new();
        equiv.insert("contrato".to_string(), vec!["contract".to_string()]);
        equiv.insert("abas".to_string(), vec!["tabs".to_string()]);
        let out = uncovered_terms(
            "cadastro do cliente para o contrato com abas e validações the ver cadastro",
            &equiv,
            &pool,
        );
        let terms: Vec<&str> = out.iter().filter_map(|v| v["term"].as_str()).collect();
        assert_eq!(terms, vec!["cadastro"], "only the blind concept flags: {out:?}");
        let tried: Vec<&str> = out[0]["tried"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        assert_eq!(tried, vec!["cadastro"], "probe set = folded term (no expansion configured)");
    }

    /// Empty pool → every content concept flags, term-ascending (the radar
    /// stays honest when retrieval returns nothing).
    #[test]
    fn uncovered_on_empty_pool_flags_content_words_sorted() {
        let out = uncovered_terms("vencimento reajuste", &std::collections::BTreeMap::new(), &[]);
        let terms: Vec<&str> = out.iter().filter_map(|v| v["term"].as_str()).collect();
        assert_eq!(terms, vec!["reajuste", "vencimento"], "sorted ascending");
    }

}
