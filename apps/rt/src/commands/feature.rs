//! `feature` — the research / "insumos" step of a feature request.
//!
//! Given a free-text client intent, this researches the repo through the
//! `scan` tool's DIGEST (never reading project source) and emits the structured
//! inputs an AI then uses to: decompose the request into units, identify
//! cross-cutting invariants, flag net-new gaps, and ask `scan spec` for each
//! unit. It is the deterministic grounding for the elicitation loop — the
//! "pesquisa no scan" that replaces reading files by hand.
//!
//! Output (stdout, pretty JSON): the intent, the domain terms queried, the
//! digest findings (matched terms, recurring slices, shared contracts, hubs),
//! the anchor files to read (plus the per-anchor `anchorsDetail` audit —
//! score/terms — and the `report.reason` strength, so the orchestrator never
//! opens the scan JSON), and a `miss` flag + note. `miss=true` means no repo
//! precedent matched — the AI must treat it as net-new (do NOT conclude "absent"
//! blindly: the term index has false negatives and no synonyms; confirm by
//! reading). Fail-open: a missing model / unavailable tool yields a miss result.

use std::path::Path;

use mustard_core::domain::scan::DigestQuery;
use mustard_core::Scan;
use serde_json::json;

/// Extract domain terms from a free-text intent: lowercased alphanumeric runs
/// >=3 chars, deduped, capped. The digest matches by token, so over-querying is
/// harmless (it ORs); the AI refines. No language/framework knowledge.
pub(crate) fn domain_terms(intent: &str) -> Vec<String> {
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
/// plus `planningWithheld: true`); the honest counts (`sliceMatchCount`,
/// `slicesOmitted`) still report what existed, and the successful re-query
/// returns the fields. The recorded `feature.query` event keeps the full
/// report either way — it goes to NDJSON, not to context.
///
/// Exception: a `bridged` weak answer is NOT withheld. There the weakness is
/// only "no literal hit", and a CURATED lexicon bridge already translated the
/// user's vocabulary onto the code's — a re-query in the repo's own words would
/// merely re-find what the supervised lexicon bridged. So the planning fields
/// ride along (the note flags the translated hit and how to promote it), which
/// is the whole point of teaching the bridge via `lexicon-suggest --accept`.
fn withhold_planning(reason: &str, bridged: bool) -> bool {
    matches!(reason, "weak" | "none") && !bridged
}

/// Max evidence files rendered per report term on stdout. The per-term files
/// are re-query evidence ("where does this vocabulary live"), not an
/// exhaustive index — three are plenty, and a 32-term prose intent times an
/// uncapped list is exactly the "gigantic weak JSON" the field run paid for.
/// The `feature.query` event keeps the full list for `lexicon-suggest`.
const REPORT_TERM_FILES_MAX: usize = 3;

/// Build the insumos payload for a successful digest query. Pure (no spawn, no
/// IO) so the payload shape — including the `stacks` passthrough — is
/// unit-testable without the scan binary.
fn payload(intent: &str, terms: &[String], q: &DigestQuery) -> serde_json::Value {
    let withhold = withhold_planning(q.report.reason.as_str(), q.report.bridged);
    json!({
        "intent": intent,
        "queryTerms": terms,
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
        "slices": if withhold { Vec::new() } else { q.slices.iter().map(|s| json!({ "label": s.label, "recurrence": s.recurrence, "entities": s.entities })).collect::<Vec<_>>() },
        // Count of matched recurring slices — the deterministic signal the
        // scope classifier consumes: 1 = "mirrors a matched slice"
        // (light/extended-light); >=2 = multi-slice vocabulary overlap, which
        // counts toward "full" only alongside layer spread (layerCount >= 2 in
        // scope-classify) — alone it is precedent, not layer spanning.
        // Additive: the `slices` array is unchanged for existing consumers.
        "sliceMatchCount": q.slices.len(),
        // Slices the per-query cap trimmed (additive; 0 from an older scan
        // binary) — honest "there was more" signal next to the count above.
        "slicesOmitted": q.slices_omitted,
        "contracts": if withhold { Vec::new() } else { q.contracts.iter().map(|c| json!({ "name": c.name, "implementors": c.implementors })).collect::<Vec<_>>() },
        "hubs": if withhold { Vec::new() } else { q.hubs.iter().map(|h| json!({ "module": h.module, "degree": h.degree })).collect::<Vec<_>>() },
        "anchors": if withhold { &[] as &[String] } else { &q.files[..] },
        // Per-anchor provenance (same order as `anchors`): which matched terms
        // declare each file — the orchestrator sees WHY each anchor is in the
        // set (file→terms) to pick what to read, without opening the scan JSON.
        // The `scoreX1024` was dropped: since the ranking became an insumo
        // union (no relevance scoring), it was always 0 — dead weight × N rows.
        "anchorsDetail": if withhold { Vec::new() } else { q.files_detail.iter().map(|d| json!({
            "file": d.file, "terms": d.terms,
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
            "terms": q.report.terms.iter().map(|t| json!({
                "term": t.term, "tier": t.tier, "lang": t.lang,
                // Evidence cap — see `REPORT_TERM_FILES_MAX`.
                "files": t.files.iter().take(REPORT_TERM_FILES_MAX).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        }),
        "note": note(q),
    })
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

/// Build the `active-research.json` marker body the digest-outcome observer
/// reads: `{terms, anchors:[{file, terms}], ts}`. The anchors invert the
/// digest's per-term report (`report.terms[].files`) into file→terms, so the
/// observer can mark a touched file `wasAnchor` and name the query terms that
/// declared it WITHOUT re-deriving the mapping. Pure + deterministic (sorted
/// files; each file's terms in report order, deduped) so the body is testable
/// without IO; `ts` is the event channel's wall clock (stamped by the caller).
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
    json!({ "terms": terms, "anchors": anchors, "ts": ts })
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

/// The guidance note for the AI consuming the payload, keyed on the report's
/// reason (the truth); an empty reason means the payload came from an older
/// scan binary, so it falls back to the legacy `miss` flag.
fn note(q: &DigestQuery) -> &'static str {
    // A curated lexicon bridge carried this answer — translated, not literal.
    // The planning fields are RETURNED (not withheld): the supervised glossary
    // already mapped the request vocabulary onto the code's, so a re-query in
    // the repo's words would only re-find the same files. Read the anchors as
    // evidence; promote the bridge so future queries land on exact/fold.
    if q.report.bridged {
        return "repo precedent found via a CURATED lexicon bridge — your request vocabulary translated onto the code's own (report.terms[].lang names the pair). `anchors` are EVIDENCE, returned not withheld: pick the files that fit and read them, and read the `hubs` (the computing logic often lives in a generically-named central service). The hit is translated, not literal — to make future queries land directly without this bridge, promote it with `mustard-rt run lexicon-suggest --accept`";
    }
    match q.report.reason.as_str() {
        "none" => {
            "no repo precedent matched — treat as net-new; the report names each missed term, so re-query the digest in the code's own vocabulary or dispatch an Explore before concluding 'absent'"
        }
        "weak" => {
            "weak precedent — under half the terms matched or only stem/lexicon-derived hits; re-query the digest in the code's own vocabulary (see report.terms[].files) and Explore before planning on top of this. Planning fields (anchors/slices/contracts/hubs) are withheld on weak precedent — the re-query returns them"
        }
        "generated_only" => {
            "matches live only in machine-written modules — regenerate or extend the generator's input; never edit the matched files directly"
        }
        "strong" => {
            "repo precedent found — `anchors` is EVIDENCE, not a ranked verdict: the deduped union of the files where your matched vocabulary is DECLARED (`report.terms[].files` is the same evidence grouped per term — on a wide query read THAT, the flat union is capped and may not list every term's file). Pick the files that fit the request and read them; also read the `hubs` — the logic that COMPUTES a behavior often lives in a generically-named central service, not the module named after the entity. Mirror the matched slices/contracts, then ask `scan spec` per unit"
        }
        _ if q.miss => {
            "no repo precedent matched — treat as net-new; the term index has no synonyms and false negatives, so confirm by reading the matched files, do not conclude 'absent' blindly"
        }
        _ => {
            "repo precedent found — `anchors` is EVIDENCE, not a ranked verdict: the deduped union of the files where your matched vocabulary is DECLARED (`report.terms[].files` is the same evidence grouped per term — on a wide query read THAT). Pick the files that fit the request and read them; also read the `hubs` (the computing logic often lives in a generically-named central service). Mirror the matched slices/contracts, then ask `scan spec` per unit"
        }
    }
}

/// Run the research step: print the feature insumos JSON for `intent`.
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
            payload(intent, &terms, &q)
        }
        Err(err) => {
            eprintln!("feature: scan digest unavailable: {err}");
            json!({
                "intent": intent,
                "queryTerms": terms,
                "stacks": [],
                "miss": true,
                "planningWithheld": true,
                "slices": [],
                "sliceMatchCount": 0,
                "slicesOmitted": 0,
                "contracts": [],
                "hubs": [],
                "anchors": [],
                "anchorsDetail": [],
                "report": { "matched": 0, "total": 0, "reason": "none", "terms": [] },
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
        let v = payload("add a page", &["page".to_string()], &q);
        let stacks = v["stacks"].as_array().expect("stacks array");
        assert_eq!(stacks.len(), 2, "both detections carried: {v}");
        assert_eq!(stacks[0]["name"], "nextjs");
        assert_eq!(stacks[0]["confidence"], 0.65);
        assert_eq!(stacks[0]["signals"], json!(["dep:next", "path:next.config.js"]));
        assert_eq!(stacks[1]["name"], "laravel");
        assert_eq!(stacks[1]["confidence"], 0.95);
        // Byte-stability: the serialized payload carries the clean decimals.
        let s = serde_json::to_string(&v).expect("payload serializes");
        assert!(s.contains("0.65"), "clean confidence missing: {s}");
        assert!(!s.contains("0.649999"), "f32 widening noise leaked: {s}");

        // No detections → an empty array, same shape as the fallback payload.
        let bare: DigestQuery = serde_json::from_str(r#"{"miss":true}"#).expect("bare digest");
        let v = payload("anything", &[], &bare);
        assert_eq!(v["stacks"], json!([]), "empty stacks must stay an empty array: {v}");
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
        // matched terms that declare it (file→terms provenance), so the
        // orchestrator sees WHY each anchor is in the set and picks what to
        // read without opening the scan JSON. NO score: the ranking became an
        // insumo union, so the per-anchor score was always 0 and was dropped.
        let q: DigestQuery = serde_json::from_str(
            r#"{"query":["refund"],"files":["src/refund.cs","src/tail.cs"],"files_detail":[{"file":"src/refund.cs","score_x1024":2048,"terms":["refund"]},{"file":"src/tail.cs","score_x1024":0,"terms":[]}],"miss":false,"report":{"matched":1,"total":1,"reason":"strong","terms":[]}}"#,
        )
        .expect("digest payload with files_detail");
        let v = payload("refund", &["refund".to_string()], &q);
        let detail = v["anchorsDetail"].as_array().expect("anchorsDetail array");
        assert_eq!(detail.len(), 2, "one provenance row per anchor: {v}");
        assert_eq!(detail[0]["file"], "src/refund.cs");
        assert_eq!(detail[0]["terms"], json!(["refund"]));
        assert!(detail[0].get("scoreX1024").is_none(), "dead score field dropped: {v}");
        assert_eq!(detail[1]["terms"], json!([]), "tail anchor shows no carrying terms: {v}");
        // The reason rides in the same payload.
        assert_eq!(v["report"]["reason"], "strong");

        // Old scan binary (no files_detail): the field degrades to an empty
        // array, mirroring the miss-fallback payload's shape.
        let old: DigestQuery = serde_json::from_str(r#"{"miss":true}"#).expect("old digest");
        let v = payload("anything", &[], &old);
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
        let v = payload("cancelar titulo", &["cancelado".to_string(), "hierarquia".to_string()], &weak);
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
        let v = payload("zzz", &["zzz".to_string()], &none);
        let note = v["note"].as_str().expect("note");
        assert!(note.contains("net-new") && note.contains("Explore"), "none note: {note}");

        // Old binary (empty reason): the legacy miss flag still drives the note.
        let old: DigestQuery = serde_json::from_str(r#"{"miss":true}"#).expect("old digest payload");
        let v = payload("anything", &[], &old);
        assert_eq!(v["report"]["reason"], "", "old payload exposes the defaulted report honestly: {v}");
        assert!(v["note"].as_str().expect("note").contains("net-new"), "miss fallback note: {v}");
    }

    #[test]
    fn weak_report_withholds_planning_fields_and_caps_term_evidence() {
        // On `weak` the note forbids planning on top of the payload, so the
        // planning fields are withheld from stdout (the orchestrator would
        // pay for them and then discard them by contract): empty arrays +
        // `planningWithheld: true`. The honest counts keep the true sizes
        // and the per-term evidence files cap at REPORT_TERM_FILES_MAX (the
        // recorded `feature.query` event keeps the full report).
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
        let v = payload("cancelar titulo", &["cancelado".to_string()], &weak);
        assert_eq!(v["planningWithheld"], json!(true));
        for field in ["slices", "contracts", "hubs", "anchors", "anchorsDetail"] {
            assert_eq!(v[field], json!([]), "{field} must be withheld on weak: {v}");
        }
        // Honest counts survive the withholding.
        assert_eq!(v["sliceMatchCount"], 2, "true slice count stays visible: {v}");
        // Re-query evidence stays, capped at REPORT_TERM_FILES_MAX.
        let files = v["report"]["terms"][0]["files"].as_array().expect("files");
        assert_eq!(files.len(), REPORT_TERM_FILES_MAX, "evidence cap: {v}");
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
        let v = payload("cancel title", &["cancel".to_string()], &strong);
        assert_eq!(v["planningWithheld"], json!(false));
        assert_eq!(v["anchors"], json!(["src/cancel.cs"]), "strong keeps anchors: {v}");
    }

    #[test]
    fn bridged_weak_returns_planning_with_a_promote_note() {
        // A `weak` answer the scan flagged `bridged: true`: the only missing
        // strength is the absence of an exact/fold hit, and a CURATED lexicon
        // bridge carried a non-thin query (the user's vocabulary translated
        // onto the code's). Unlike a plain weak, the planning fields are NOT
        // withheld — re-querying in the repo's words would just re-find what
        // the supervised lexicon already bridged. The note explains the
        // translated hit and how to promote it (`lexicon-suggest --accept`).
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
                    {"term":"cancelado","tier":"lexicon","lang":"pt-en","files":["src/cancel.cs"]}]}}"#,
        )
        .expect("bridged digest payload");
        let v = payload("cancelar titulo", &["cancelado".to_string()], &bridged);
        assert_eq!(v["planningWithheld"], json!(false), "a curated bridge is not withheld: {v}");
        assert_eq!(v["anchors"], json!(["src/cancel.cs"]), "anchors returned for the bridged hit: {v}");
        assert_eq!(v["report"]["bridged"], json!(true), "the marker rides along: {v}");
        assert_eq!(v["sliceMatchCount"], 1, "honest counts stay: {v}");
        assert_ne!(v["slices"], json!([]), "planning fields survive the bridge: {v}");
        let note = v["note"].as_str().expect("note");
        assert!(note.contains("lexicon-suggest --accept"), "note explains how to promote the bridge: {v}");
        // The bridged note must NOT steer a re-query (the plain-weak note does):
        // the supervised lexicon already bridged the vocabulary.
        assert!(!note.contains("re-query"), "bridged note does not steer a re-query: {v}");
    }
}
