//! `mustard-rt run digest-validate-render` — materialise a byte-stable VALIDATION
//! prompt that asks an LLM (Sonnet), ONE layer above the deterministic scan, to
//! VALIDATE a digest answer before feature/task/bugfix acts on it.
//!
//! The deterministic scan locates; it cannot judge whether an anchor is a REAL
//! target or an incidental lexical match (a backend "credit-card" file matching a
//! UI request on the bare word "card"), nor whether the work genuinely needs the
//! feature pipeline. Those are semantic judgements — so they live in an LLM step
//! the orchestrator runs, never inside the scan.
//!
//! Superset of [`crate::commands::agent::concern_judge`]: same deterministic
//! assembly (reuses the feature digest's retrieval + the per-anchor project span
//! from `read_projects`, emits a byte-stable prompt, calls no model — the
//! JUDGEMENT is the dispatched LLM's), but the verdict carries `route`
//! (feature vs the lean /task), `scope` (light vs full, for feature), and the
//! `dropped` incidental anchors alongside the concern partition. The model is
//! Sonnet, not Haiku: this is ONE routing-critical call per pipeline entry (not a
//! fan-out), so accuracy outweighs the negligible per-call cost delta.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

use mustard_core::domain::scan::DigestQuery;
use mustard_core::Scan;
use serde::Deserialize;

use crate::commands::agent::concern_judge::{matched_concepts, JudgedConcern};
use crate::commands::feature::domain_terms;

/// A confirmed-on-re-query bridge from a missed USER-SIDE word to the real
/// ENGLISH code identifier(s) it maps to in this codebase. A PAIR (not a flat
/// list) so the orchestrator knows `efetivar -> effectivate`, not a cartesian
/// of every missed word × every requery term. The shape mirrors the
/// `lexicon-enrich --apply` proposal (`{userWord, codeTerms}`), so a confirmed
/// bridge is persisted to the project overlay verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RequeryBridge {
    #[serde(rename = "userWord", default)]
    pub user_word: String,
    #[serde(rename = "codeTerms", default)]
    pub code_terms: Vec<String>,
}

/// The validator's verdict — the parse target of [`parse_digest_verdict`].
/// Every field defaults so a partial LLM reply still deserialises; the caller
/// validates `route` and degrades to the deterministic anchors when it is absent.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DigestVerdict {
    /// Where the request should run: `"task"` (lean — no spec/wave ceremony) or
    /// `"feature"` (the full pipeline). The lean default for single-layer work.
    #[serde(default)]
    pub route: String,
    /// For `route == "feature"`: `"light"` | `"full"`. Empty otherwise.
    #[serde(default)]
    pub scope: String,
    /// Anchor files the validator judged INCIDENTAL — a tangential or far-layer
    /// lexical match, not a real target — so the caller does not read them.
    #[serde(default)]
    pub dropped: Vec<String>,
    /// The concern partition (≥1 unit of work), mirroring the concern-judge shape
    /// so a multi-concern request still splits onto its own anchors.
    #[serde(default)]
    pub concerns: Vec<JudgedConcern>,
    /// Whether the CENTRAL (most-discriminative) concept of the intent was found
    /// at a real tier. `false` means the kept anchors likely matched only common
    /// vocabulary and point at the WRONG flow → the orchestrator must re-query.
    /// Absence MUST default to `true`: an older reply without the field carries no
    /// retrieval concern, so it never triggers a re-query.
    #[serde(default = "default_central_found", rename = "centralFound")]
    pub central_found: bool,
    /// When `central_found` is false, the PAIRED bridge(s) from each missed
    /// USER-SIDE word to the real ENGLISH code identifiers it maps to — what the
    /// orchestrator re-queries the digest with (the flattened `codeTerms`) to
    /// locate the right files, and what it PERSISTS to the project lexicon when
    /// that re-query confirms (comes back `strong`). Paired so a confirmed bridge
    /// persists as `{userWord, codeTerms}`, not a cartesian. Empty when found.
    #[serde(default, rename = "requeryBridges")]
    pub requery_bridges: Vec<RequeryBridge>,
}

/// Default for [`DigestVerdict::central_found`]: an LLM reply (or an older one)
/// that omits the field carries NO retrieval concern, so it must not trigger a
/// re-query — absence means "the central concept was found".
fn default_central_found() -> bool {
    true
}

/// Why a verdict could not be parsed — returned instead of panicking so a
/// malformed LLM reply degrades to the deterministic fallback (the Guard: a run
/// face never panics on bad input).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerdictParseError {
    /// The text held no JSON object (no `{` … `}`).
    NoJsonObject,
    /// A `{` … `}` span was found but did not deserialise as the verdict shape.
    InvalidShape,
    /// The object parsed but carried no `route` — a non-answer, not a verdict.
    NoRoute,
}

impl std::fmt::Display for VerdictParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            Self::NoJsonObject => "no JSON object found in verdict response",
            Self::InvalidShape => "verdict response is not a {route,scope,dropped,concerns,centralFound,requeryBridges} object",
            Self::NoRoute => "verdict response carried no route",
        };
        f.write_str(msg)
    }
}

/// The contract the validator must honour — prepended to the rendered prompt so
/// the verdict is well-formed and parseable by [`parse_digest_verdict`].
/// EN/technical by policy (agent prompts stay English).
const VALIDATE_CONTRACT: &str = "You are a digest validator for a code pipeline. The deterministic scan matched the \
     concepts below for a request and named the anchor files where each concept's vocabulary lives, \
     each tagged with the PROJECT (layer) it sits in. Your job, ONE layer above the scan, is to \
     validate this answer and decide HOW the request runs. Reply with ONLY a JSON object:\n\
     {\"route\":\"task|feature\",\"scope\":\"light|full|\",\"dropped\":[\"<file>\"],\"concerns\":[{\"label\":\"<short>\",\"concepts\":[\"<concept>\"],\"anchors\":[\"<file>\"]}],\"centralFound\":true|false,\"requeryBridges\":[{\"userWord\":\"<missed user-side word>\",\"codeTerms\":[\"<real code identifier>\"]}]}\n\
     RULES:\n\
     - dropped: an anchor is INCIDENTAL when its concept is tangential to the INTENT or lives in a \
     layer the change will not touch (e.g. a UI request matching a backend credit-card file on the \
     bare word \"card\"). List every such anchor in `dropped`; when the intent makes one sense of an \
     ambiguous concept clear, drop the other sense's anchors. The REAL layers are the distinct \
     projects of the anchors you did NOT drop.\n\
     - centralFound: false when the MOST DISCRIMINATIVE concept of the intent (the specific action or \
     entity that defines the task — NOT generic filler like value/date/status) appears under MISSED / \
     WEAK, meaning the kept anchors likely matched only common vocabulary and point at the WRONG flow. \
     true when the central concept was found at a real tier.\n\
     - requeryBridges: propose a bridge ONLY for a TRUE vocabulary gap — a missed user word that names the \
     SAME concept some KEPT anchor already IMPLEMENTS, just spelled differently (e.g. PT \"efetivar\" IS the \
     \"effectivate\" flow that already exists in the anchors). Return an EMPTY array when ANY of: (a) \
     centralFound is true; (b) the concept is NET-NEW — no kept anchor implements it, so it is a feature to \
     BUILD, not a word to bridge (do NOT guess generic terms like import/upload/user); (c) the only code term \
     that would match is the SAME word in a DIFFERENT sense (the programming keyword \"import\"; \"user\" \
     meaning the auth/login user) — a false match, not a bridge. NEVER propose a term that already appears \
     under MISSED. A wrong bridge poisons EVERY future query, so when unsure, return empty.\n\
     - route: \"task\" when the real work is single-layer and small (one project, mirrors an existing \
     pattern, no new entity) — the lean path, no spec/wave ceremony. \"feature\" only when it genuinely \
     needs the pipeline.\n\
     - scope (feature only; \"\" when route is task): \"light\" = one real layer, enhancing existing code. \
     \"full\" = two or more real layers, OR a net-new entity (the SIGNALS show miss / no precedent), OR \
     clearly large.\n\
     - concerns: partition the KEPT concepts into units of work (one when they collaborate on the same \
     files, separate when they do not). Every kept concept lands in exactly one concern; never invent a \
     concept or an anchor. No prose outside the JSON.";

/// The `projects[].dir` that is the longest path-prefix of `file` (the most
/// specific enclosing project), or `""` when none encloses it. `dirs` MUST be
/// sorted by length descending so the first match is the longest. Mirrors the
/// spec compiler's `project_of` attribution rule.
fn project_of<'a>(file: &str, dirs: &'a [String]) -> &'a str {
    dirs.iter()
        .find(|d| !d.is_empty() && (file == d.as_str() || file.starts_with(&format!("{d}/"))))
        .map(String::as_str)
        .unwrap_or("")
}

/// Render the byte-stable validation prompt for `intent` against the digest
/// answer `q` and the model's project dirs (`project_dirs`, sorted length-desc).
/// Pure + deterministic (no IO, no clock): contract, intent, each matched concept
/// with its tier and per-anchor project, then the SIGNALS the validator weighs
/// (reason, miss, slice matches, distinct anchor projects). Empty when no concept
/// matched — there is nothing to validate.
fn render_validate_prompt(intent: &str, q: &DigestQuery, project_dirs: &[String]) -> String {
    let concepts = matched_concepts(q);
    if concepts.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    out.push_str(VALIDATE_CONTRACT);
    out.push_str("\n\n## INTENT\n");
    out.push_str(intent.trim());
    out.push_str("\n\n## CONCEPTS (term [tier]; each anchor with its [project])\n");
    let mut layers: BTreeSet<&str> = BTreeSet::new();
    for c in &concepts {
        let _ = writeln!(out, "- {} [{}]", c.term, c.tier);
        for f in &c.files {
            let p = project_of(f, project_dirs);
            if !p.is_empty() {
                layers.insert(p);
            }
            let _ = writeln!(out, "    - {f}  [{}]", if p.is_empty() { "-" } else { p });
        }
    }
    // MISSED / WEAK concepts: every REQUEST term the digest named but did NOT
    // find at a real tier (tier ∈ {none, trigram} or empty). Surfacing these is
    // the point — `matched_concepts` filters them out, so without this section
    // the judge is blind to a missed CENTRAL concept (a request whose specific
    // action/entity hit `none` while only common vocabulary matched). Iterate
    // `report.terms` directly (do NOT touch the shared `matched_concepts`).
    // OMITTED entirely when there are none, so a clean strong query renders
    // byte-identically to before.
    let missed: Vec<&mustard_core::domain::scan::TermReport> = q
        .report
        .terms
        .iter()
        .filter(|t| t.tier.is_empty() || t.tier == "none" || t.tier == "trigram")
        .collect();
    if !missed.is_empty() {
        out.push_str(
            "\n## MISSED / WEAK CONCEPTS (the request named these but the scan did NOT find them at a real tier — judge whether one is the CENTRAL concept the anchors are missing)\n",
        );
        for t in &missed {
            let tier = if t.tier.is_empty() { "none" } else { t.tier.as_str() };
            let _ = writeln!(out, "- {} [{}]", t.term, tier);
        }
    }
    let _ = write!(
        out,
        "\n## SIGNALS\n- reason: {}\n- miss: {}\n- sliceMatches: {}\n- distinctProjects(anchors): {}\n",
        q.report.reason,
        q.miss,
        q.slices.len(),
        layers.len()
    );
    out
}

/// Locate the first balanced `{` … `}` span in `text` (the validator may wrap the
/// JSON in prose or a ``` fence despite the contract). Coarse — the real
/// validation is the serde parse in [`parse_digest_verdict`].
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

/// Parse the validator's response into a [`DigestVerdict`].
///
/// Tolerant by contract (the Guard: a run face never panics on bad input):
/// extracts the first balanced JSON object (the validator may fence or wrap it),
/// deserialises it, and returns a typed [`VerdictParseError`] on every failure
/// mode instead of unwrapping. A verdict with no `route` is rejected
/// ([`VerdictParseError::NoRoute`]) — a non-answer the caller treats as fallback.
pub fn parse_digest_verdict(text: &str) -> Result<DigestVerdict, VerdictParseError> {
    let span = extract_json_object(text).ok_or(VerdictParseError::NoJsonObject)?;
    let verdict: DigestVerdict =
        serde_json::from_str(span).map_err(|_| VerdictParseError::InvalidShape)?;
    if verdict.route.trim().is_empty() {
        return Err(VerdictParseError::NoRoute);
    }
    Ok(verdict)
}

/// CLI face: `mustard-rt run digest-validate-render --intent <text> --model <path>`.
///
/// PURE DETERMINISTIC — no `claude` subprocess (the JUDGEMENT is the LLM's, run by
/// the orchestrator on this prompt). Reuses the feature digest's retrieval +
/// `read_projects` to tag each anchor with its project, renders the byte-stable
/// validation prompt, and prints it to stdout (raw, no JSON framing). Fail-open:
/// an unavailable scan / model prints nothing and always exits 0.
pub fn run(intent: &str, model: &Path) {
    let terms = domain_terms(intent);
    let prompt = match Scan::locate().digest_query(model, &terms) {
        Ok(q) => {
            // Project dirs, longest first, so `project_of` picks the most specific
            // enclosing project. Empty dirs are dropped (the root is not a layer).
            let mut dirs: Vec<String> = mustard_core::read_projects(model)
                .into_iter()
                .map(|p| p.dir)
                .filter(|d| !d.is_empty())
                .collect();
            dirs.sort_by(|a, b| b.len().cmp(&a.len()).then(a.cmp(b)));
            render_validate_prompt(intent, &q, &dirs)
        }
        Err(err) => {
            eprintln!("digest-validate-render: scan digest unavailable: {err}");
            String::new()
        }
    };
    print!("{prompt}");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixture mirroring the sialia cross-layer case: a UI-shaped intent whose
    /// `card` concept matches BOTH a UI component and a backend credit-card file.
    fn ui_query() -> DigestQuery {
        serde_json::from_str(
            r#"{"query":["card","chevron"],"files":["packages/ui/card.tsx","backend/Pay/CardCharge.cs"],"miss":false,
                "slices":[{"label":"List","recurrence":3}],
                "report":{"matched":2,"total":2,"reason":"strong","terms":[
                    {"term":"card","tier":"exact","lang":"","files":["packages/ui/card.tsx","backend/Pay/CardCharge.cs"]},
                    {"term":"chevron","tier":"exact","lang":"","files":["packages/ui/card.tsx"]}]}}"#,
        )
        .expect("ui digest fixture")
    }

    fn dirs() -> Vec<String> {
        // Sorted length-desc, as `run` supplies them.
        vec!["backend/Pay".to_string(), "packages/ui".to_string()]
    }

    #[test]
    fn project_of_picks_the_longest_enclosing_dir() {
        let d = vec!["apps/web/admin".to_string(), "apps/web".to_string()];
        assert_eq!(project_of("apps/web/admin/page.tsx", &d), "apps/web/admin");
        assert_eq!(project_of("apps/web/home.tsx", &d), "apps/web");
        assert_eq!(project_of("scripts/build.sh", &d), "");
        // A path equal to the dir itself, and a near-miss prefix (no slash).
        assert_eq!(project_of("apps/web", &d), "apps/web");
        assert_eq!(project_of("apps/website/x", &d), "");
    }

    #[test]
    fn render_is_byte_stable_and_tags_anchors_with_their_project() {
        let q = ui_query();
        let a = render_validate_prompt("make the chevron clickable on the card", &q, &dirs());
        let b = render_validate_prompt("make the chevron clickable on the card", &q, &dirs());
        assert_eq!(a, b, "render must be byte-stable for the same inputs");

        assert!(a.contains("digest validator"), "contract present: {a}");
        assert!(a.contains("## INTENT\nmake the chevron clickable on the card"), "intent: {a}");
        // The backend anchor is TAGGED with its project so the validator can judge it incidental.
        assert!(a.contains("backend/Pay/CardCharge.cs  [backend/Pay]"), "anchor tagged with project: {a}");
        assert!(a.contains("packages/ui/card.tsx  [packages/ui]"), "ui anchor tagged: {a}");
        // SIGNALS carry the scope inputs.
        assert!(a.contains("reason: strong"), "reason signal: {a}");
        assert!(a.contains("sliceMatches: 1"), "slice-match signal: {a}");
        assert!(a.contains("distinctProjects(anchors): 2"), "layer-span signal: {a}");
    }

    #[test]
    fn render_empty_when_no_concept_matched() {
        let none: DigestQuery = serde_json::from_str(
            r#"{"query":["zzz"],"miss":true,
                "report":{"matched":0,"total":1,"reason":"none","terms":[
                    {"term":"zzz","tier":"none","lang":"","files":[]}]}}"#,
        )
        .expect("none-reason digest");
        assert_eq!(render_validate_prompt("zzz", &none, &dirs()), "");
    }

    /// The proven "efetivar previsão" failure: a CENTRAL concept (`effectivate`)
    /// hit `none` while common terms matched, so the digest pointed at the WRONG
    /// file and the validator — blind to the miss — blindly confirmed it. The
    /// MISSED / WEAK section surfaces it so the judge can flag `centralFound`.
    fn missed_central_query() -> DigestQuery {
        serde_json::from_str(
            r#"{"query":["payable","effectivate"],"files":["backend/Pay/Payable.cs"],"miss":false,
                "slices":[],
                "report":{"matched":1,"total":2,"reason":"strong","terms":[
                    {"term":"payable","tier":"exact","lang":"","files":["backend/Pay/Payable.cs"]},
                    {"term":"effectivate","tier":"none","lang":"","files":[]}]}}"#,
        )
        .expect("missed-central digest fixture")
    }

    #[test]
    fn render_surfaces_a_missed_central_concept() {
        let q = missed_central_query();
        let p = render_validate_prompt("efetivar a previsão", &q, &dirs());
        // The matched concept renders as before…
        assert!(p.contains("- payable [exact]"), "matched concept present: {p}");
        // …and the missed CENTRAL concept is surfaced under its own header.
        assert!(p.contains("## MISSED / WEAK CONCEPTS"), "missed section present: {p}");
        assert!(p.contains("- effectivate [none]"), "missed term rendered with tier: {p}");
    }

    #[test]
    fn render_omits_missed_section_when_clean() {
        // The all-matched fixture has NO missed term → the section is omitted
        // entirely, so a clean strong query renders byte-identically to before.
        let q = ui_query();
        let p = render_validate_prompt("make the chevron clickable on the card", &q, &dirs());
        assert!(!p.contains("## MISSED / WEAK CONCEPTS"), "no missed section when clean: {p}");
    }

    #[test]
    fn parse_accepts_a_full_verdict() {
        let resp = r#"{"route":"task","scope":"","dropped":["backend/Pay/CardCharge.cs"],
            "concerns":[{"label":"card ui","concepts":["card","chevron"],"anchors":["packages/ui/card.tsx"]}]}"#;
        let v = parse_digest_verdict(resp).expect("valid verdict parses");
        assert_eq!(v.route, "task");
        assert_eq!(v.scope, "");
        assert_eq!(v.dropped, vec!["backend/Pay/CardCharge.cs".to_string()]);
        assert_eq!(v.concerns.len(), 1);
        assert_eq!(v.concerns[0].anchors, vec!["packages/ui/card.tsx".to_string()]);
        // A verdict WITHOUT the retrieval fields defaults `central_found` to TRUE
        // (no re-query) and `requery_bridges` to empty — an old reply must not
        // trigger a re-query.
        assert!(v.central_found, "absent centralFound defaults to true");
        assert!(v.requery_bridges.is_empty(), "absent requeryBridges defaults to empty");
    }

    #[test]
    fn parse_reads_central_found_false_with_requery_bridges() {
        let resp = r#"{"route":"feature","scope":"full","centralFound":false,"requeryBridges":[{"userWord":"efetivar","codeTerms":["effectivate"]}]}"#;
        let v = parse_digest_verdict(resp).expect("retrieval-concern verdict parses");
        assert!(!v.central_found, "centralFound:false read");
        assert_eq!(
            v.requery_bridges,
            vec![RequeryBridge { user_word: "efetivar".to_string(), code_terms: vec!["effectivate".to_string()] }],
            "paired bridge parsed: userWord -> codeTerms"
        );
    }

    #[test]
    fn parse_tolerates_prose_and_fences_around_the_object() {
        let resp = "Here is the verdict:\n```json\n{\"route\":\"feature\",\"scope\":\"full\"}\n```\nDone.";
        let v = parse_digest_verdict(resp).expect("object extracted from fenced prose");
        assert_eq!(v.route, "feature");
        assert_eq!(v.scope, "full");
    }

    #[test]
    fn parse_rejects_invalid_forms_without_panic() {
        assert_eq!(parse_digest_verdict("not json"), Err(VerdictParseError::NoJsonObject));
        assert_eq!(parse_digest_verdict(""), Err(VerdictParseError::NoJsonObject));
        // Parseable object but no route → a non-answer the caller treats as fallback.
        assert_eq!(parse_digest_verdict("{\"scope\":\"light\"}"), Err(VerdictParseError::NoRoute));
        // A scalar where the object's fields expect arrays/strings.
        assert_eq!(parse_digest_verdict("{\"route\":[1,2]}"), Err(VerdictParseError::InvalidShape));
    }
}
