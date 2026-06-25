//! `mustard-rt run purpose-judge-render` — the EXPLICIT judge step over the
//! purpose-search candidates.
//!
//! `purpose-search` is the deterministic RECALL layer: it surfaces the handful of
//! files whose `purpose` summaries answer an intent, ranked by IDF so the file
//! that bridges the rare/discriminative term survives the cap. It does NOT decide
//! which candidate is THE target — many sibling/UI files of a business feature
//! legitimately share the same domain words, and telling the implementer apart
//! from the page that merely displays it is a SEMANTIC judgement, not a lexical
//! one. That judgement was, until now, the orchestrator reading the candidates by
//! hand. This command makes it a FIRST-CLASS step: it renders a byte-stable prompt
//! that hands the candidates (each with its purpose summary) to an LLM (Sonnet —
//! the same single routing-critical judge tier as `digest-validate-render`) and
//! asks it to PICK the file(s) that actually IMPLEMENT the intent's action.
//!
//! Determinism: PURE assembly — runs the deterministic `purpose-search`, reads the
//! purposes already in the model, emits the prompt. NO model call here (the
//! JUDGEMENT is the dispatched LLM's, run by the orchestrator). Fail-open: an
//! unavailable scan / model / empty candidate set prints nothing and exits 0 — a
//! miss-recovery aid must never become a hard error.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use mustard_core::Scan;
use serde::Deserialize;

use crate::commands::feature::domain_terms;

/// Cap on purpose lines shown per candidate file — a focused service file has a
/// few logic methods; bounding keeps the prompt byte-stable and small.
const MAX_PURPOSES_PER_FILE: usize = 6;

/// One purpose-search candidate, enriched with the purpose summaries found in the
/// model for that file (sorted + deduped → byte-stable).
#[derive(Debug, Clone, PartialEq)]
struct Candidate {
    file: String,
    matched: Vec<String>,
    purposes: Vec<String>,
}

/// The shape `scan purpose-search` emits — file + the query tokens it bridged.
#[derive(Debug, Deserialize)]
struct PurposeOut {
    #[serde(default)]
    files: Vec<PurposeFile>,
}

#[derive(Debug, Deserialize)]
struct PurposeFile {
    file: String,
    #[serde(rename = "matchedTerms", default)]
    matched_terms: Vec<String>,
}

/// The contract the judge must honour — prepended so the verdict is parseable by
/// [`parse_judge_pick`]. EN/technical by policy (agent prompts stay English).
const JUDGE_CONTRACT: &str = "You are picking which file(s) IMPLEMENT a user's intent, from candidate files that a \
     deterministic search surfaced because their one-sentence purpose summaries share the intent's \
     domain vocabulary. The catch: in any codebase the same domain words recur across many files of a \
     feature — the backend service that PERFORMS the action, the page/component that DISPLAYS it, the \
     repository that stores it, the endpoint that exposes it. Your job is to pick the file(s) whose \
     purpose DOES the action the intent asks for, not the ones that merely show, route, or store it. \
     Reply with ONLY a JSON object: {\"picks\":[\"<file>\"],\"reason\":\"<short concept that decided it>\"}\n\
     RULES:\n\
     - Pick the file(s) whose purpose PERFORMS the intent's action — the file that DOES the action, not one \
     that merely displays, routes, stores, or exposes it. Usually ONE file; pick more only when several \
     genuinely co-implement it.\n\
     - Match on MEANING, not exact words: a purpose that performs the intent's action counts even when it \
     words the concept with a synonym or paraphrase. Do not reject the right file just because its purpose \
     phrases the action differently than the user did.\n\
     - picks: [] (empty) when NO candidate implements the intent — the concept is net-new to build, OR the \
     user's word is a synonym none of the purposes use (a vocabulary bridge is needed, not a pick).\n\
     - reason: name the concept/verb that decided the pick (or why none fit). No prose outside the JSON.";

/// Render the byte-stable judge prompt for `intent` over `candidates`. Pure +
/// deterministic: contract, intent, then each candidate (1-based) with its matched
/// terms and its purpose summaries. Empty when there are no candidates — there is
/// nothing to judge.
fn render_judge_prompt(intent: &str, candidates: &[Candidate]) -> String {
    if candidates.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    out.push_str(JUDGE_CONTRACT);
    out.push_str("\n\n## INTENT\n");
    out.push_str(intent.trim());
    out.push_str("\n\n## CANDIDATES (file — matched terms; purpose summaries of its methods)\n");
    for (i, c) in candidates.iter().enumerate() {
        let _ = writeln!(out, "{}. {}  [matched: {}]", i + 1, c.file, c.matched.join(", "));
        for p in &c.purposes {
            let _ = writeln!(out, "    - {p}");
        }
    }
    out
}

/// Look up, for each candidate file, the purpose summaries of its logic
/// declarations from the model. Returns a `file -> purposes` map, each list
/// capped at [`MAX_PURPOSES_PER_FILE`] with the purposes that ANSWER the query
/// (contain a query term) kept FIRST — so a big multi-method service does not lose
/// the one purpose that matched to an alphabetical truncation (field-found: a
/// write-off method's purpose fell past the cap behind unrelated "Buscar…" lines,
/// and the judge, blind to it, declined). `query_terms` is the tokenised intent;
/// matching is a case-insensitive substring (agnostic — no domain words). Reads
/// the model as a generic JSON value (never panics; a missing model → empty map).
fn purposes_by_file(
    model: &Path,
    wanted: &[String],
    query_terms: &[String],
) -> BTreeMap<String, Vec<String>> {
    let ql: Vec<String> = query_terms.iter().map(|t| t.to_lowercase()).collect();
    // How MANY distinct query terms a purpose contains — not just whether ANY does.
    // A common token ("recebível") sits in EVERY purpose of a receivables service,
    // so a boolean "answers?" cannot tell the matcher from the rest; the COUNT can
    // (the write-off purpose "dar baixa … recebível" scores 3, a bare "buscar
    // recebível" scores 1), so the purpose that actually answered the query wins
    // the cap. Case-insensitive substring — agnostic, no domain words.
    let query_hits = |p: &str| -> usize {
        let lp = p.to_lowercase();
        ql.iter().filter(|q| lp.contains(q.as_str())).count()
    };
    let want: std::collections::BTreeSet<&str> = wanted.iter().map(String::as_str).collect();
    let mut by_file: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let raw = std::fs::read_to_string(model).unwrap_or_default();
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return by_file;
    };
    let Some(modules) = value.get("modules").and_then(|v| v.as_array()) else {
        return by_file;
    };
    for module in modules {
        let Some(path) = module.get("path").and_then(|v| v.as_str()) else { continue };
        if !want.contains(path) {
            continue;
        }
        let entry = by_file.entry(path.to_string()).or_default();
        if let Some(decls) = module.get("declarations").and_then(|v| v.as_array()) {
            for decl in decls {
                if let Some(p) = decl.get("purpose").and_then(|v| v.as_str()) {
                    if !p.is_empty() {
                        entry.push(p.to_string());
                    }
                }
            }
        }
        entry.sort();
        entry.dedup();
        // Most-query-terms first (stable, so alpha order holds within a tie) BEFORE
        // the cap — the purpose that actually answered must survive the truncation.
        entry.sort_by(|a, b| query_hits(b).cmp(&query_hits(a)));
        entry.truncate(MAX_PURPOSES_PER_FILE);
    }
    by_file
}

/// The purpose-search candidates as `(file, purpose summaries)` pairs — the
/// PURPOSE-LAYER grounding the `digest-validate` auto-heal reuses to propose a
/// bridge when the NAME index missed the central concept (the target is invisible
/// to name-match but its purpose answers the request). Same deterministic search +
/// model purpose lookup as the judge render. Fail-open: empty on any failure.
pub(crate) fn candidate_purposes(model: &Path, terms: &[String]) -> Vec<(String, Vec<String>)> {
    Scan::locate()
        .purpose_search(model, terms)
        .ok()
        .and_then(|out| serde_json::from_str::<PurposeOut>(&out).ok())
        .map(|out| {
            let files: Vec<String> = out.files.iter().map(|f| f.file.clone()).collect();
            let purposes = purposes_by_file(model, &files, terms);
            out.files
                .into_iter()
                .map(|f| {
                    let p = purposes.get(&f.file).cloned().unwrap_or_default();
                    (f.file, p)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// CLI face: `mustard-rt run purpose-judge-render --intent <text> --model <path>`.
///
/// PURE assembly — runs the deterministic `purpose-search`, attaches each
/// candidate's purpose summaries from the model, renders the byte-stable judge
/// prompt, prints it raw (the JUDGEMENT is the dispatched LLM's). Fail-open: an
/// unavailable scan / no candidates prints nothing, exits 0.
pub fn run(intent: &str, model: &Path) {
    let terms = domain_terms(intent);
    let candidates = Scan::locate()
        .purpose_search(model, &terms)
        .ok()
        .and_then(|out| serde_json::from_str::<PurposeOut>(&out).ok())
        .map(|out| {
            let files: Vec<String> = out.files.iter().map(|f| f.file.clone()).collect();
            let purposes = purposes_by_file(model, &files, &terms);
            out.files
                .into_iter()
                .map(|f| Candidate {
                    purposes: purposes.get(&f.file).cloned().unwrap_or_default(),
                    file: f.file,
                    matched: f.matched_terms,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    print!("{}", render_judge_prompt(intent, &candidates));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(file: &str, matched: &[&str], purposes: &[&str]) -> Candidate {
        Candidate {
            file: file.to_string(),
            matched: matched.iter().map(|s| s.to_string()).collect(),
            purposes: purposes.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn render_is_byte_stable_and_lists_candidates_with_purposes() {
        let cands = vec![
            cand("src/Reconciliation/ReconciliationService.cs", &["conciliar", "bancário"], &["Concilia manualmente uma linha de extrato bancário a uma movimentação."]),
            cand("app/reconciliation/page.tsx", &["conciliar"], &["Exibe a página de conciliação bancária."]),
        ];
        let a = render_judge_prompt("conciliar lançamento bancário", &cands);
        let b = render_judge_prompt("conciliar lançamento bancário", &cands);
        assert_eq!(a, b, "render must be byte-stable for the same inputs");
        assert!(a.contains("picking which file"), "contract present");
        assert!(a.contains("## INTENT\nconciliar lançamento bancário"), "intent present");
        assert!(a.contains("1. src/Reconciliation/ReconciliationService.cs  [matched: conciliar, bancário]"), "candidate 1 with matched terms: {a}");
        assert!(a.contains("Concilia manualmente uma linha de extrato bancário"), "purpose summary shown: {a}");
        assert!(a.contains("2. app/reconciliation/page.tsx"), "candidate 2 present");
    }

    #[test]
    fn render_empty_when_no_candidates() {
        assert_eq!(render_judge_prompt("anything", &[]), "");
    }

    #[test]
    fn render_states_the_pick_implements_not_displays_contract() {
        // The judge's discriminating instruction — pick what DOES the action, not
        // what shows/stores it — must be in the prompt, plus the empty-picks escape
        // for a net-new concept / synonym gap.
        let c = vec![cand("a.cs", &["x"], &["faz algo"])];
        let p = render_judge_prompt("x", &c);
        assert!(p.contains("DOES the action"), "implements-not-displays rule present: {p}");
        assert!(p.contains("net-new to build"), "empty-picks escape present: {p}");
        assert!(p.contains("\"picks\":[\"<file>\"]"), "JSON reply shape present: {p}");
    }
}
