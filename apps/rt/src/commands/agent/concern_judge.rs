//! `mustard-rt run concern-judge-render` — materialise a byte-stable JUDGE
//! prompt that asks an LLM, ONE layer above the deterministic scan, to
//! partition a feature intent's matched concepts into labelled concerns.
//!
//! This is the render half of the concern-split direction recorded in memory:
//! the scan's `connected_components` split (co-occurrence over the import graph)
//! is a deterministic FACT, but a clean per-concern partition is a judgement —
//! so it lives in an LLM step the orchestrator runs, never inside the scan.
//! This command does the deterministic assembly; the judgement is the LLM's.
//!
//! Shape-mirrors [`crate::commands::agent::agent_prompt_render`]: the render is
//! pure + deterministic (sorted, no timestamps/volatile paths — the run-face
//! byte-stability contract); stdout = the raw prompt string (no JSON framing).
//! The retrieval is REUSED verbatim from the feature digest — `domain_terms`
//! tokenisation + `Scan::digest_query` — so the concepts and the per-concept
//! anchors the judge sees are exactly the ones `feature` would surface.
//!
//! The judge's RESPONSE (a JSON array of `{label, concepts, anchors}`) is parsed
//! by [`parse_judge_response`], tolerant of invalid form (returns an `Err`, never
//! panics) so a malformed LLM reply degrades instead of crashing the caller.

use std::fmt::Write as _;
use std::path::Path;

use mustard_core::domain::scan::DigestQuery;
use mustard_core::Scan;
use serde::Deserialize;

use crate::commands::feature::domain_terms;

/// One concept the judge reasons over: a matched query term + the anchor files
/// the digest's per-term report named for it. Pure data carried into the prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ConceptAnchors {
    /// The matched request term (the concept).
    term: String,
    /// The match tier that carried it (`exact` | `fold` | `stem` | `lexicon`).
    tier: String,
    /// The anchor files where this concept's vocabulary lives (sorted, deduped).
    files: Vec<String>,
}

/// One concern the judge returns: a human label, the concepts it groups, and
/// the anchor files to read for it. The parse target of [`parse_judge_response`].
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct JudgedConcern {
    /// The judge's human-readable label for the concern.
    #[serde(default)]
    pub label: String,
    /// The query concepts this concern groups.
    #[serde(default)]
    pub concepts: Vec<String>,
    /// The anchor files to read for this concern.
    #[serde(default)]
    pub anchors: Vec<String>,
}

/// Why a judge response could not be parsed — returned instead of panicking so
/// a malformed LLM reply degrades gracefully (the Guard: a hook/run face never
/// panics on bad input).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JudgeParseError {
    /// The text held no JSON array (no `[` … `]`).
    NoJsonArray,
    /// A `[` … `]` span was found but did not deserialise as the concern shape.
    InvalidShape,
    /// The array parsed but was empty — a judge that partitions into nothing is
    /// a non-answer, not a valid partition.
    EmptyPartition,
}

impl std::fmt::Display for JudgeParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            Self::NoJsonArray => "no JSON array found in judge response",
            Self::InvalidShape => "judge response is not a [{label,concepts,anchors}] array",
            Self::EmptyPartition => "judge response partitioned into zero concerns",
        };
        f.write_str(msg)
    }
}

/// Invert the digest's per-term report (`report.terms[].files`) into the matched
/// concepts the judge reasons over. A concept is kept ONLY when it matched (tier
/// != `none`) — an unmatched term carries no anchors and is not a concept to
/// partition. Files are deduped (first-occurrence) and sorted for byte-stability.
/// Concepts are returned in the report's order (the digest's own ranking).
fn matched_concepts(q: &DigestQuery) -> Vec<ConceptAnchors> {
    q.report
        .terms
        .iter()
        .filter(|t| !t.tier.is_empty() && t.tier != "none")
        .map(|t| {
            let mut files: Vec<String> = Vec::new();
            for f in &t.files {
                if !files.contains(f) {
                    files.push(f.clone());
                }
            }
            files.sort();
            ConceptAnchors {
                term: t.term.clone(),
                tier: t.tier.clone(),
                files,
            }
        })
        .collect()
}

/// The contract the judge must honour — prepended to the rendered prompt so the
/// partition is well-formed and the response shape is parseable by
/// [`parse_judge_response`]. EN/technical by policy (agent prompts stay English).
const JUDGE_CONTRACT: &str = "You are a concern-split judge. The deterministic scan matched the concepts below \
     for a feature intent and named the anchor files where each concept's vocabulary lives. \
     Partition the concepts into one or more CONCERNS: a concern is a group of concepts that \
     belong to the SAME unit of work (they touch the same files / collaborate). Concepts that do \
     not collaborate go into SEPARATE concerns. Every matched concept MUST land in exactly one \
     concern; never invent a concept or an anchor that is not listed. Reply with ONLY a JSON array, \
     one object per concern: [{\"label\":\"<short label>\",\"concepts\":[\"<concept>\"],\"anchors\":[\"<file>\"]}]. \
     The `anchors` of a concern are the union of its concepts' anchor files. No prose outside the JSON.";

/// Render the byte-stable judge prompt for `intent` against the digest answer
/// `q`. Pure + deterministic (no IO, no clock): the contract, then the intent,
/// then each matched concept with its tier and its sorted anchor files. When the
/// scan already split the query into ≥2 concerns ([`DigestQuery::concerns`]),
/// the scan's split is shown as the deterministic STARTING point the judge
/// refines; otherwise the flat concept list is the judge's input. Empty (the
/// caller then prints nothing) when no concept matched — there is nothing to
/// partition.
fn render_judge_prompt(intent: &str, q: &DigestQuery) -> String {
    let concepts = matched_concepts(q);
    if concepts.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    out.push_str(JUDGE_CONTRACT);
    out.push_str("\n\n## INTENT\n");
    out.push_str(intent.trim());
    out.push_str("\n\n## CONCEPTS\n");
    for c in &concepts {
        // `- {term} [{tier}]` then each anchor indented; deterministic order.
        let _ = writeln!(out, "- {} [{}]", c.term, c.tier);
        for f in &c.files {
            let _ = writeln!(out, "    - {f}");
        }
    }
    // Deterministic starting partition: scan's own connected-components split,
    // shown sorted by label so the judge refines a stable baseline rather than
    // starting cold. Omitted when the scan saw a single concern.
    if q.concerns.len() >= 2 {
        let mut split: Vec<&mustard_core::domain::scan::ConcernHit> = q.concerns.iter().collect();
        split.sort_by(|a, b| a.label.cmp(&b.label));
        out.push_str("\n## SCAN SPLIT (deterministic starting point — refine, do not just echo)\n");
        for c in split {
            let _ = writeln!(out, "- {} ({}): {}", c.label, c.reason, c.concepts.join(", "));
        }
    }
    out
}

/// Locate the first balanced `[` … `]` span in `text` (the judge may wrap the
/// JSON in prose or a ``` fence despite the contract). Returns the slice
/// including the brackets, or `None` when no array delimiters are present.
/// Bracket counting ignores nesting depth strings — it is a coarse extractor;
/// the real validation is the serde parse in [`parse_judge_response`].
fn extract_json_array(text: &str) -> Option<&str> {
    let start = text.find('[')?;
    let mut depth = 0i32;
    for (i, ch) in text[start..].char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..=start + i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Parse the judge's response into the partition of [`JudgedConcern`]s.
///
/// Tolerant by contract (the Guard: a run face never panics on bad input):
/// extracts the first balanced JSON array (the judge may fence or wrap it),
/// deserialises it, and returns a typed [`JudgeParseError`] on every failure
/// mode instead of unwrapping. A parsed-but-empty array is rejected
/// ([`JudgeParseError::EmptyPartition`]) — a non-answer, not a valid partition.
pub fn parse_judge_response(text: &str) -> Result<Vec<JudgedConcern>, JudgeParseError> {
    let span = extract_json_array(text).ok_or(JudgeParseError::NoJsonArray)?;
    let parsed: Vec<JudgedConcern> =
        serde_json::from_str(span).map_err(|_| JudgeParseError::InvalidShape)?;
    if parsed.is_empty() {
        return Err(JudgeParseError::EmptyPartition);
    }
    Ok(parsed)
}

/// CLI face: `mustard-rt run concern-judge-render --intent <text> --model <path>`.
///
/// PURE DETERMINISTIC — no `claude` subprocess (the JUDGEMENT is the LLM's, run
/// by the orchestrator on this prompt). Reuses the feature digest's retrieval
/// (`domain_terms` + `digest_query`) to obtain the matched concepts + per-concept
/// anchors, renders the byte-stable judge prompt, and prints it to stdout (raw,
/// no JSON framing). Fail-open: an unavailable scan / model prints nothing and
/// always exits 0 (there is no precedent to partition).
pub fn run(intent: &str, model: &Path) {
    let terms = domain_terms(intent);
    let prompt = match Scan::locate().digest_query(model, &terms) {
        Ok(q) => render_judge_prompt(intent, &q),
        Err(err) => {
            eprintln!("concern-judge-render: scan digest unavailable: {err}");
            String::new()
        }
    };
    // stdout = the prompt string (raw). Empty render prints nothing (the
    // historical print-nothing-on-no-content behaviour of the render faces).
    print!("{prompt}");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixture mirroring the sialia case: a query whose matched concepts split
    /// into two disconnected concerns (a tenant/export pair) the judge groups.
    fn sialia_query() -> DigestQuery {
        serde_json::from_str(
            r#"{"query":["tenant","export","outcome"],"files":["t.cs","e.cs"],"miss":false,
                "report":{"matched":3,"total":3,"reason":"strong","terms":[
                    {"term":"tenant","tier":"exact","lang":"","files":["src/tenant.cs","src/shared.cs"]},
                    {"term":"export","tier":"exact","lang":"","files":["src/export.cs"]},
                    {"term":"outcome","tier":"none","lang":"","files":[]}]},
                "concerns":[
                    {"label":"tenant","concepts":["tenant"],"files":["src/tenant.cs"],
                     "files_detail":[{"file":"src/tenant.cs","score_x1024":2048,"terms":["tenant"]}],"reason":"strong"},
                    {"label":"export","concepts":["export"],"files":["src/export.cs"],
                     "files_detail":[{"file":"src/export.cs","score_x1024":1024,"terms":["export"]}],"reason":"weak"}]}"#,
        )
        .expect("sialia digest fixture")
    }

    #[test]
    fn concern_judge_render_is_byte_stable_from_the_sialia_fixture() {
        let q = sialia_query();
        let a = render_judge_prompt("export per tenant", &q);
        let b = render_judge_prompt("export per tenant", &q);
        assert_eq!(a, b, "the render must be byte-stable for the same inputs");

        // The contract, the intent and each MATCHED concept (with its tier and
        // sorted anchors) are present; the unmatched `outcome` concept is not.
        assert!(a.contains("concern-split judge"), "contract present: {a}");
        assert!(a.contains("## INTENT\nexport per tenant"), "intent rendered: {a}");
        assert!(a.contains("- tenant [exact]"), "matched concept + tier: {a}");
        assert!(a.contains("- export [exact]"), "matched concept + tier: {a}");
        assert!(!a.contains("outcome"), "unmatched (tier=none) concept dropped: {a}");
        // Anchors are sorted asc (shared.cs < tenant.cs) under the tenant concept.
        let t_idx = a.find("src/tenant.cs").expect("tenant anchor");
        let s_idx = a.find("src/shared.cs").expect("shared anchor");
        assert!(s_idx < t_idx, "anchors sorted asc: {a}");
        // The deterministic scan split rides along as the starting point.
        assert!(a.contains("SCAN SPLIT"), "≥2 concerns shows the scan split: {a}");
        assert!(a.contains("export (weak)"), "scan split carries the per-concern reason: {a}");
    }

    #[test]
    fn concern_judge_render_empty_when_no_concept_matched() {
        // Every term missed (tier=none) → nothing to partition → empty render.
        let none: DigestQuery = serde_json::from_str(
            r#"{"query":["zzz"],"miss":true,
                "report":{"matched":0,"total":1,"reason":"none","terms":[
                    {"term":"zzz","tier":"none","lang":"","files":[]}]}}"#,
        )
        .expect("none-reason digest");
        assert_eq!(render_judge_prompt("zzz", &none), "");
    }

    #[test]
    fn concern_judge_render_single_concern_omits_scan_split() {
        // A single-concern answer (no ≥2 split) renders the concepts but no
        // SCAN SPLIT section — the flat concept list is the judge's input.
        let single: DigestQuery = serde_json::from_str(
            r#"{"query":["refund"],"files":["src/refund.cs"],"miss":false,
                "report":{"matched":1,"total":1,"reason":"strong","terms":[
                    {"term":"refund","tier":"exact","lang":"","files":["src/refund.cs"]}]}}"#,
        )
        .expect("single-concern digest");
        let p = render_judge_prompt("refund", &single);
        assert!(p.contains("- refund [exact]"), "concept rendered: {p}");
        assert!(!p.contains("SCAN SPLIT"), "no split section on a single concern: {p}");
    }

    #[test]
    fn concern_judge_parse_accepts_a_valid_partition() {
        let resp = r#"[
            {"label":"tenancy","concepts":["tenant"],"anchors":["src/tenant.cs","src/shared.cs"]},
            {"label":"export","concepts":["export"],"anchors":["src/export.cs"]}
        ]"#;
        let parsed = parse_judge_response(resp).expect("valid partition parses");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].label, "tenancy");
        assert_eq!(parsed[0].concepts, vec!["tenant".to_string()]);
        assert_eq!(parsed[0].anchors, vec!["src/tenant.cs".to_string(), "src/shared.cs".to_string()]);
        assert_eq!(parsed[1].label, "export");
    }

    #[test]
    fn concern_judge_parse_tolerates_prose_and_fences_around_the_array() {
        // The judge wrapped the JSON in a ``` fence + prose despite the
        // contract — the balanced-bracket extractor still finds the array.
        let resp = "Here is the partition:\n```json\n[{\"label\":\"a\",\"concepts\":[\"x\"],\"anchors\":[]}]\n```\nDone.";
        let parsed = parse_judge_response(resp).expect("array extracted from fenced prose");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].label, "a");
    }

    #[test]
    fn concern_judge_parse_rejects_invalid_forms_without_panic() {
        // No array at all.
        assert_eq!(parse_judge_response("not json"), Err(JudgeParseError::NoJsonArray));
        assert_eq!(parse_judge_response(""), Err(JudgeParseError::NoJsonArray));
        // A bracket span that is not the concern shape (array of scalars).
        assert_eq!(parse_judge_response("[1, 2, 3]"), Err(JudgeParseError::InvalidShape));
        // An object, not an array.
        assert_eq!(parse_judge_response("{\"label\":\"a\"}"), Err(JudgeParseError::NoJsonArray));
        // A well-formed but empty partition is a non-answer.
        assert_eq!(parse_judge_response("[]"), Err(JudgeParseError::EmptyPartition));
        // Unbalanced bracket → no complete span found.
        assert_eq!(parse_judge_response("[{\"label\":\"a\"}"), Err(JudgeParseError::NoJsonArray));
    }
}
