//! `mustard-rt run lexicon-judge-render` — materialise a byte-stable JUDGE
//! prompt that asks an LLM, ONE layer above the deterministic enrich, to score
//! each mined CODE candidate term as BUSINESS-DOMAIN vs GENERIC plumbing.
//!
//! This is the LLM half of the lexicon-enrich redesign recorded in memory: the
//! deterministic binary NARROWS the candidates (`lexicon-enrich --check` runs
//! the provenance rank that demotes recurring structural role affixes), and the
//! domain-vs-generic CALL — which `count×idf` specificity could NOT make (44%
//! accuracy, empirically refuted) — lives in this LLM step the orchestrator runs
//! on the rendered prompt. The binary never shells out to a model; the JUDGEMENT
//! is the LLM's.
//!
//! Shape-mirrors [`crate::commands::agent::concern_judge`]: the render is pure +
//! deterministic (no IO, no clock — the run-face byte-stability contract);
//! stdout = the raw prompt string (no JSON framing). The candidate retrieval is
//! REUSED verbatim from `lexicon-enrich`
//! ([`crate::commands::lexicon_enrich::unbridged_candidate_terms`]) so the judge
//! scores EXACTLY the candidates `--check` surfaces.
//!
//! The judge's RESPONSE (a single-line JSON object mapping each term to a 0-100
//! score) is parsed by [`parse_lexicon_scores`], tolerant of invalid form
//! (returns an `Err`, never panics) so a malformed LLM reply degrades instead of
//! crashing the caller.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::commands::lexicon_enrich::unbridged_candidate_terms;

/// The contract the judge must honour — prepended to the rendered prompt so the
/// scoring scale is fixed and the response is parseable by
/// [`parse_lexicon_scores`]. EN/technical by policy (agent prompts stay
/// English). The validated text — the scoring scale and the JSON instruction are
/// verbatim; do not reword.
const JUDGE_CONTRACT: &str = "You are scoring code identifier tokens mined from a software project, to decide which are worth adding to a bilingual domain glossary. For EACH term below, output a score 0-100 for how much it is BUSINESS-DOMAIN vocabulary versus GENERIC programming/framework vocabulary.\n\n100 = a BUSINESS/DOMAIN concept specific to what this software does (e.g. invoice, tenant, voucher, partner).\n0 = GENERIC programming/framework plumbing (e.g. handler, response, config, dto, repository).\n50 = genuinely ambiguous.\n\nSome identifiers may be in a language other than English (e.g. Portuguese \"venda\" = sale, \"cobranca\" = charge) — judge by MEANING, not language.\n\nReturn ONLY a single-line JSON object mapping each term to its integer score, nothing else.";

/// Render the byte-stable judge prompt for `terms`: the contract, then
/// `\n\nTerms: ` + the terms joined by `, `. Pure + deterministic (no IO, no
/// clock). Returns the EMPTY string when `terms` is empty (the caller then
/// prints nothing — there is nothing to score).
fn render_judge_prompt(terms: &[String]) -> String {
    if terms.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(JUDGE_CONTRACT.len() + 16 + terms.len() * 8);
    out.push_str(JUDGE_CONTRACT);
    out.push_str("\n\nTerms: ");
    out.push_str(&terms.join(", "));
    out
}

/// Why a judge response could not be parsed — returned instead of panicking so a
/// malformed LLM reply degrades gracefully (the Guard: a run face never panics
/// on bad input).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexiconJudgeParseError {
    /// The text held no JSON object (no balanced `{` … `}`).
    NoJsonObject,
    /// A `{` … `}` span was found but did not deserialise as an object.
    InvalidShape,
    /// The object parsed but held no usable numeric score — a non-answer.
    Empty,
}

impl std::fmt::Display for LexiconJudgeParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            Self::NoJsonObject => "no JSON object found in judge response",
            Self::InvalidShape => "judge response is not a {term: score} object",
            Self::Empty => "judge response carried no usable score",
        };
        f.write_str(msg)
    }
}

/// Locate the first balanced `{` … `}` span in `text` (the judge may wrap the
/// JSON in prose or a ``` fence despite the contract). Returns the slice
/// including the braces, or `None` when no object delimiters are present.
/// Brace counting is depth-only — a coarse extractor; the real validation is the
/// serde parse in [`parse_lexicon_scores`]. Mirrors concern_judge's
/// `extract_json_array` for `{`/`}`.
fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let mut depth = 0i32;
    for (i, ch) in text[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
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

/// Coerce a judge score value to a clamped 0-100 `u8`. Accepts an integer, a
/// float (rounded toward zero by the cast after clamping), or a numeric string
/// (the LLM occasionally quotes a number). Non-numeric values yield `None` (the
/// term is skipped, not defaulted). Always clamped to `[0, 100]`.
fn coerce_score(v: &serde_json::Value) -> Option<u8> {
    let n = if let Some(i) = v.as_i64() {
        i as f64
    } else if let Some(f) = v.as_f64() {
        f
    } else if let Some(s) = v.as_str() {
        s.trim().parse::<f64>().ok()?
    } else {
        return None;
    };
    // Clamp into the score band, then narrow to u8 (the clamp guarantees range).
    Some(n.clamp(0.0, 100.0) as u8)
}

/// Parse the judge's response into a `term -> score` map.
///
/// Tolerant by contract (the Guard: a run face never panics on bad input):
/// extracts the first balanced JSON object (the judge may fence or wrap it),
/// deserialises it as `{term: Value}`, coerces each value to a clamped 0-100
/// `u8` (integer / float / numeric string; non-numeric values are skipped), and
/// returns a typed [`LexiconJudgeParseError`] on every failure mode instead of
/// unwrapping. A parsed object with no usable score is rejected
/// ([`LexiconJudgeParseError::Empty`]) — a non-answer. Scores are clamped to
/// `[0, 100]`.
pub fn parse_lexicon_scores(text: &str) -> Result<BTreeMap<String, u8>, LexiconJudgeParseError> {
    let span = extract_json_object(text).ok_or(LexiconJudgeParseError::NoJsonObject)?;
    let raw: BTreeMap<String, serde_json::Value> =
        serde_json::from_str(span).map_err(|_| LexiconJudgeParseError::InvalidShape)?;
    let scores: BTreeMap<String, u8> = raw
        .iter()
        .filter_map(|(k, v)| coerce_score(v).map(|s| (k.clone(), s)))
        .collect();
    if scores.is_empty() {
        return Err(LexiconJudgeParseError::Empty);
    }
    Ok(scores)
}

/// CLI face: `mustard-rt run lexicon-judge-render --root <dir>`.
///
/// PURE DETERMINISTIC — no `claude` subprocess (the JUDGEMENT is the LLM's, run
/// by the orchestrator on this prompt). Re-derives the SAME unbridged candidates
/// `lexicon-enrich --check` produces
/// ([`unbridged_candidate_terms`]), renders the byte-stable judge prompt, and
/// prints it to stdout (raw, no JSON framing). Fail-open: an unavailable model /
/// no vendored pair → empty candidates → prints nothing → exit 0.
pub fn run(root: &Path) {
    let root = if root == Path::new(".") {
        PathBuf::from(crate::shared::context::project_dir())
    } else {
        root.to_path_buf()
    };
    let terms = unbridged_candidate_terms(&root);
    // stdout = the prompt string (raw). Empty render prints nothing (the
    // historical print-nothing-on-no-content behaviour of the render faces).
    print!("{}", render_judge_prompt(&terms));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_is_byte_stable_and_verbatim() {
        let terms = vec!["payable".to_string(), "handler".to_string(), "cobranca".to_string()];
        let a = render_judge_prompt(&terms);
        let b = render_judge_prompt(&terms);
        assert_eq!(a, b, "the render must be byte-stable for the same inputs");
        // The contract and the terms line are present, terms joined by ", ".
        assert!(a.starts_with("You are scoring code identifier tokens"), "contract head: {a}");
        assert!(a.contains("Return ONLY a single-line JSON object"), "JSON instruction present: {a}");
        assert!(a.ends_with("\n\nTerms: payable, handler, cobranca"), "terms line: {a}");
    }

    #[test]
    fn render_empty_when_no_terms() {
        assert_eq!(render_judge_prompt(&[]), "", "no candidates → empty render");
    }

    #[test]
    fn parse_accepts_a_clean_object() {
        let resp = r#"{"payable": 90, "handler": 5, "cobranca": 85}"#;
        let scores = parse_lexicon_scores(resp).expect("clean object parses");
        assert_eq!(scores.get("payable"), Some(&90));
        assert_eq!(scores.get("handler"), Some(&5));
        assert_eq!(scores.get("cobranca"), Some(&85));
    }

    #[test]
    fn parse_tolerates_fenced_and_prose_wrapped_object() {
        // The judge wrapped the JSON in a ```json fence + prose despite the
        // contract — the balanced-brace extractor still finds the object.
        let resp = "Here are the scores:\n```json\n{\"payable\": 88, \"dto\": 0}\n```\nDone.";
        let scores = parse_lexicon_scores(resp).expect("object extracted from fenced prose");
        assert_eq!(scores.get("payable"), Some(&88));
        assert_eq!(scores.get("dto"), Some(&0));
    }

    #[test]
    fn parse_coerces_float_and_stringified_scores_clamped() {
        // A float rounds into the band; a quoted number coerces; an over-range
        // value clamps to 100; a non-numeric value is skipped (not defaulted).
        let resp = r#"{"a": 72.9, "b": "63", "c": 250, "d": "not-a-number"}"#;
        let scores = parse_lexicon_scores(resp).expect("mixed numeric forms coerce");
        assert_eq!(scores.get("a"), Some(&72), "float coerced (toward zero after clamp)");
        assert_eq!(scores.get("b"), Some(&63), "stringified number coerced");
        assert_eq!(scores.get("c"), Some(&100), "over-range clamped to 100");
        assert!(!scores.contains_key("d"), "non-numeric value skipped");
    }

    #[test]
    fn parse_rejects_invalid_forms_without_panic() {
        // No object at all.
        assert_eq!(parse_lexicon_scores("no json"), Err(LexiconJudgeParseError::NoJsonObject));
        assert_eq!(parse_lexicon_scores(""), Err(LexiconJudgeParseError::NoJsonObject));
        // An array, not an object — the brace extractor finds nothing.
        assert_eq!(parse_lexicon_scores("[1, 2, 3]"), Err(LexiconJudgeParseError::NoJsonObject));
        // An empty object is a non-answer.
        assert_eq!(parse_lexicon_scores("{}"), Err(LexiconJudgeParseError::Empty));
        // An object whose every value is non-numeric → no usable score.
        assert_eq!(parse_lexicon_scores(r#"{"a": "x", "b": null}"#), Err(LexiconJudgeParseError::Empty));
    }
}
