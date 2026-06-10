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
//! the anchor files to read, and a `miss` flag + note. `miss=true` means no repo
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

/// Build the insumos payload for a successful digest query. Pure (no spawn, no
/// IO) so the payload shape — including the `stacks` passthrough — is
/// unit-testable without the scan binary.
fn payload(intent: &str, terms: &[String], q: &DigestQuery) -> serde_json::Value {
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
        "matchedTerms": q.matched_terms.iter().map(|t| json!({ "term": t.term, "count": t.count })).collect::<Vec<_>>(),
        "slices": q.slices.iter().map(|s| json!({ "label": s.label, "recurrence": s.recurrence, "entities": s.entities })).collect::<Vec<_>>(),
        // Count of matched recurring slices — the deterministic signal the
        // scope classifier consumes: 1 = "mirrors a matched slice"
        // (light/extended-light); >=2 = "spans multiple slices" (full).
        // Additive: the `slices` array is unchanged for existing consumers.
        "sliceMatchCount": q.slices.len(),
        "contracts": q.contracts.iter().map(|c| json!({ "name": c.name, "implementors": c.implementors })).collect::<Vec<_>>(),
        "hubs": q.hubs.iter().map(|h| json!({ "module": h.module, "degree": h.degree })).collect::<Vec<_>>(),
        "anchors": q.files,
        // The honest per-term match report (scan's tier ladder) — the truth
        // about what matched. Per term: the tier that carried it (exact |
        // fold | stem | lexicon | none), the natural-language evidence and
        // the files where the vocabulary lives; aggregate matched k/n.
        "report": json!({
            "matched": q.report.matched,
            "total": q.report.total,
            "reason": q.report.reason,
            "terms": q.report.terms.iter().map(|t| json!({
                "term": t.term, "tier": t.tier, "lang": t.lang, "files": t.files,
            })).collect::<Vec<_>>(),
        }),
        "note": note(q),
    })
}

/// Compact `feature.query` event payload: the queried terms + the honest
/// match report (matched/total/reason + per-term term/tier/lang). The per-term
/// `files` ride along — they are the evidence `lexicon-suggest` cites when a
/// later re-query confirms a vocabulary bridge. Pure + deterministic; the
/// payload carries no timestamp of its own (the event channel stamps `ts`).
fn query_event_payload(terms: &[String], q: &DigestQuery) -> serde_json::Value {
    json!({
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
    match q.report.reason.as_str() {
        "none" => {
            "no repo precedent matched — treat as net-new; the report names each missed term, so re-query the digest in the code's own vocabulary or dispatch an Explore before concluding 'absent'"
        }
        "weak" => {
            "weak precedent — under half the terms matched or only stem/lexicon-derived hits; re-query the digest in the code's own vocabulary (see report.terms[].files) and Explore before planning on top of this"
        }
        "generated_only" => {
            "matches live only in machine-written modules — regenerate or extend the generator's input; never edit the matched files directly"
        }
        "strong" => {
            "repo precedent found — mirror the matched slices/contracts; read the anchors before planning, then ask `scan spec` per unit"
        }
        _ if q.miss => {
            "no repo precedent matched — treat as net-new; the term index has no synonyms and false negatives, so confirm by reading anchors, do not conclude 'absent' blindly"
        }
        _ => {
            "repo precedent found — mirror the matched slices/contracts; read the anchors before planning, then ask `scan spec` per unit"
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
            emit_query_event(query_event_payload(&terms, &q));
            emit_digest_used_event(digest_used_payload(&terms, &q));
            payload(intent, &terms, &q)
        }
        Err(err) => {
            eprintln!("feature: scan digest unavailable: {err}");
            json!({
                "intent": intent,
                "queryTerms": terms,
                "stacks": [],
                "miss": true,
                "matchedTerms": [],
                "slices": [],
                "sliceMatchCount": 0,
                "contracts": [],
                "hubs": [],
                "anchors": [],
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
    fn feature_query_event_payload_is_compact_and_deterministic() {
        // The recorded event carries ONLY {queryTerms, report} — none of the
        // bulky insumos fields (anchors/slices/hubs/stacks) and no timestamp
        // of its own (the event channel stamps `ts`). Per-term entries keep
        // term/tier/lang + the evidence files lexicon-suggest cites.
        let q: DigestQuery = serde_json::from_str(
            r#"{"query":["hierarquia"],"miss":true,"report":{"matched":0,"total":1,"reason":"none","terms":[{"term":"hierarquia","tier":"none","lang":"","files":[]}]}}"#,
        )
        .expect("digest payload with report");
        let terms = vec!["hierarquia".to_string()];
        let v = query_event_payload(&terms, &q);
        assert_eq!(v["queryTerms"], json!(["hierarquia"]));
        assert_eq!(v["report"]["matched"], 0);
        assert_eq!(v["report"]["total"], 1);
        assert_eq!(v["report"]["reason"], "none");
        assert_eq!(v["report"]["terms"][0]["term"], "hierarquia");
        assert_eq!(v["report"]["terms"][0]["tier"], "none");
        assert_eq!(v["report"]["terms"][0]["lang"], "");
        assert_eq!(v["report"]["terms"][0]["files"], json!([]));
        let obj = v.as_object().expect("object payload");
        assert_eq!(obj.len(), 2, "exactly queryTerms + report: {v}");
        assert!(obj.get("ts").is_none(), "no own timestamp in the payload");
        // Byte-stable: the same inputs serialize to the same bytes.
        let a = serde_json::to_string(&query_event_payload(&terms, &q)).expect("serializes");
        let b = serde_json::to_string(&query_event_payload(&terms, &q)).expect("serializes");
        assert_eq!(a, b);
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
}
