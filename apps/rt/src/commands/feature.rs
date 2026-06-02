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
        if out.len() >= 16 {
            break;
        }
    }
    out
}

/// Run the research step: print the feature insumos JSON for `intent`.
pub fn run(intent: &str, root: &Path) {
    let terms = domain_terms(intent);
    let model = root.join(".claude").join("grain.model.json");

    let payload = match Scan::locate().digest_query(&model, &terms) {
        Ok(q) => json!({
            "intent": intent,
            "queryTerms": terms,
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
            "note": if q.miss {
                "no repo precedent matched — treat as net-new; the term index has no synonyms and false negatives, so confirm by reading anchors, do not conclude 'absent' blindly"
            } else {
                "repo precedent found — mirror the matched slices/contracts; read the anchors before planning, then ask `scan spec` per unit"
            },
        }),
        Err(err) => {
            eprintln!("feature: scan digest unavailable: {err}");
            json!({
                "intent": intent,
                "queryTerms": terms,
                "miss": true,
                "matchedTerms": [],
                "slices": [],
                "sliceMatchCount": 0,
                "contracts": [],
                "hubs": [],
                "anchors": [],
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
        assert!(domain_terms(&many).len() <= 16);
    }
}
