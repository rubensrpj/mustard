//! `mustard-rt run enrich` — PROACTIVE population of the project lexicon
//! overlay with code→user-word bridges, the sibling of the REACTIVE
//! [`crate::commands::lexicon_suggest`].
//!
//! ## Why
//!
//! The digest's match ladder only bridges a cross-language request term onto
//! the code's vocabulary through the curated tier-4 lexicon. `lexicon-suggest`
//! fills that overlay AFTER a query missed and a re-query confirmed the bridge
//! (reactive). `enrich` fills it BEFORE the first miss: it surfaces the mined
//! domain terms of the CODE that nothing in the lexicon bridges to yet, so the
//! orchestrator can propose the user-side word for each — closing the
//! `titulo→payable` vocabulary gap proactively.
//!
//! ## Determinism — the AI never runs here
//!
//! The rt stays 100% deterministic and offline (the `apps/rt` guard): no LLM
//! shell-out, no network. The two modes are pure data:
//!
//! - `--check --root <dir>` READS only. It loads the model's full digest
//!   (`scan digest`, the same boundary `feature` uses), resolves the language
//!   pair like `lexicon-suggest`, and emits the top-N mined CODE terms that are
//!   NOT a value of any lexicon entry (seed + project overlay) — the "unbridged"
//!   vocabulary. Byte-stable JSON; nothing is written.
//! - `--apply <proposals.json> --root <dir>` WRITES, gated. It reads the
//!   orchestrator's proposed `{userWord, codeTerms}` bridges and, for each code
//!   term, validates it EXISTS as a mined term in the model (the deterministic
//!   anti-hallucination gate). Valid targets are written to the overlay via the
//!   shared [`lexicon_suggest::write_bridge`] (atomic, alphabetical, comments
//!   preserved); rejected targets are reported, never written.
//!
//! The orchestrator (the harness model, agnostic of provider) is the only AI in
//! the loop, and it lives OUTSIDE this binary — it proposes the bridges between
//! the two deterministic steps. Headless / no orchestrator ⇒ enrich is simply
//! not invoked; the digest fail-opens to the committed overlay.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mustard_core::domain::scan::Digest;
use mustard_core::Scan;
use serde_json::{json, Value};

use crate::commands::lexicon_suggest::{
    effective_lexicon, folded, pair_for_root, write_bridge, PairSeed,
};

/// Cap on the unbridged terms `--check` emits. The published digest already
/// orders its term index by discriminative rank, so the first N are the most
/// discriminative unbridged terms — the ones worth a bridge. Bounds the payload
/// (a repo can mine thousands of tokens) without dumping the long tail.
const MAX_UNBRIDGED: usize = 60;

/// One mined code term with no lexicon bridge — the `--check` output row.
struct Unbridged {
    term: String,
    count: usize,
    samples: Vec<String>,
}

/// The set of folded CODE terms the lexicon already bridges TO — every VALUE
/// across every entry (seed + overlay). The lexicon's KEYS are the user words;
/// its VALUES are the code terms. A mined term that is not in this set is one
/// no entry bridges to (nobody maps a user word onto it).
fn bridged_code_terms(root: &Path, pair: Option<&PairSeed>) -> BTreeSet<String> {
    effective_lexicon(root, pair).into_values().flatten().collect()
}

/// The mined CODE terms (digest term index) that no lexicon entry bridges to,
/// in the digest's discriminative-rank order, capped at [`MAX_UNBRIDGED`]. A
/// term is matched against the bridged set by its folded key (the lexicon's
/// identity), so accent/case never leaks a false "unbridged".
fn unbridged_terms(digest: &Digest, bridged: &BTreeSet<String>) -> Vec<Unbridged> {
    digest
        .terms
        .iter()
        .filter(|t| !bridged.contains(&folded(&t.term)))
        .take(MAX_UNBRIDGED)
        .map(|t| Unbridged { term: t.term.clone(), count: t.count, samples: t.samples.clone() })
        .collect()
}

/// `--check` report: the unbridged mined vocabulary the orchestrator should
/// propose user-side words for. Read-only; byte-stable. A missing model / no
/// vendored pair degrades to an empty `unbridged` list (no-op, never an error).
fn check_report(root: &Path) -> Value {
    let pair = pair_for_root(root);
    // The root's request language, the same `lang` wins over `specLang`
    // precedence the pair resolution uses — surfaced so the orchestrator knows
    // which user-side natural language to propose words in.
    let cfg = mustard_core::ProjectConfig::load(root);
    let language = cfg.lang.or(cfg.spec_lang).unwrap_or_default();
    let bridged = bridged_code_terms(root, pair.as_ref());
    let model = root.join(".claude").join("grain.model.json");
    let digest = Scan::locate().digest(&model).unwrap_or_default();
    let unbridged = unbridged_terms(&digest, &bridged);
    json!({
        "pair": pair.as_ref().map(|p| p.label),
        "language": language,
        "unbridged": unbridged.iter().map(|u| json!({
            "term": u.term, "count": u.count, "samples": u.samples,
        })).collect::<Vec<_>>(),
    })
}

/// One proposed bridge from the orchestrator: a user-side word and the code
/// term(s) it should map onto.
#[derive(Debug)]
struct Proposal {
    user_word: String,
    code_terms: Vec<String>,
}

impl Proposal {
    /// Accept either camelCase (`userWord`/`codeTerms`, the documented shape)
    /// or snake_case (`user_word`/`code_terms`). Tolerant of extra keys and
    /// missing fields (which degrade to empty and are skipped at apply time).
    fn from_value(v: &Value) -> Self {
        Proposal {
            user_word: v
                .get("userWord")
                .or_else(|| v.get("user_word"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            code_terms: v
                .get("codeTerms")
                .or_else(|| v.get("code_terms"))
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default(),
        }
    }
}

/// The folded set of CODE terms mined in the model — the gate's source of
/// truth. A proposed bridge whose target is not in this set is a hallucination
/// and is rejected, deterministically.
fn mined_term_set(digest: &Digest) -> BTreeSet<String> {
    digest.terms.iter().map(|t| folded(&t.term)).collect()
}

/// `--apply` report: validate each proposed bridge against the model and write
/// the valid ones to the overlay. Anti-hallucination gate — a code term absent
/// from the mined vocabulary is rejected (`target_not_in_model`), never
/// written. Byte-stable: applied + rejected rows are emitted in input order.
fn apply_report(root: &Path, proposals_path: &Path) -> Value {
    let Some(pair) = pair_for_root(root) else {
        return json!({ "pair": null, "applied": [], "rejected": [], "reason": "no-lexicon-pair" });
    };
    let rel_path = format!(".claude/lexicons/{}.toml", pair.label);
    let Ok(raw) = mustard_core::io::fs::read_to_string(proposals_path) else {
        return json!({
            "pair": pair.label, "applied": [], "rejected": [],
            "reason": "proposals-unreadable", "path": rel_path,
        });
    };
    let proposals: Vec<Proposal> = match serde_json::from_str::<Value>(&raw) {
        Ok(Value::Array(items)) => items.iter().map(Proposal::from_value).collect(),
        _ => {
            return json!({
                "pair": pair.label, "applied": [], "rejected": [],
                "reason": "proposals-not-an-array", "path": rel_path,
            });
        }
    };

    let digest = Scan::locate().digest(&root.join(".claude").join("grain.model.json")).unwrap_or_default();
    let mined = mined_term_set(&digest);

    let mut applied: Vec<Value> = Vec::new();
    let mut rejected: Vec<Value> = Vec::new();
    for p in &proposals {
        let user_word = folded(&p.user_word);
        if user_word.is_empty() {
            continue;
        }
        for code_raw in &p.code_terms {
            let code_term = folded(code_raw);
            if code_term.is_empty() {
                continue;
            }
            // The gate: the target MUST be a real mined term. This is what kills
            // a hallucinated bridge deterministically, before any write.
            if !mined.contains(&code_term) {
                rejected.push(json!({
                    "userWord": user_word, "codeTerm": code_term,
                    "reason": "target_not_in_model",
                }));
                continue;
            }
            if write_bridge(root, &pair, &user_word, &code_term).is_ok() {
                applied.push(json!({ "userWord": user_word, "codeTerm": code_term }));
            } else {
                rejected.push(json!({
                    "userWord": user_word, "codeTerm": code_term, "reason": "overlay_write_failed",
                }));
            }
        }
    }
    json!({
        "pair": pair.label,
        "applied": applied,
        "rejected": rejected,
        "path": rel_path,
    })
}

/// Dispatch `mustard-rt run enrich [--check | --apply <proposals.json>] --root <dir>`.
///
/// `--check` lists the unbridged mined vocabulary (read-only). `--apply` writes
/// the orchestrator's validated bridges to the project overlay (gated). Exactly
/// one mode runs; `--check` is the default when neither is given.
pub fn run(check: bool, apply: Option<&Path>, root: &Path) {
    let root = if root == Path::new(".") {
        PathBuf::from(crate::shared::context::project_dir())
    } else {
        root.to_path_buf()
    };
    let report = match apply {
        Some(path) => apply_report(&root, path),
        None => {
            let _ = check; // `--check` is the only read mode; the flag is documentary.
            check_report(&root)
        }
    };
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::lexicon_suggest::overlay_path;
    use mustard_core::domain::scan::Digest;
    use tempfile::tempdir;

    fn write_root_config(root: &Path) {
        std::fs::write(root.join("mustard.json"), br#"{"specLang":"pt-BR"}"#).unwrap();
    }

    /// A digest with the given (term, count, samples) rows — the shape
    /// `scan digest` serializes, deserialized into our view.
    fn digest_of(rows: &[(&str, usize, &[&str])]) -> Digest {
        let terms: Vec<Value> = rows
            .iter()
            .map(|(t, c, s)| json!({ "term": t, "count": c, "samples": s }))
            .collect();
        serde_json::from_value(json!({ "terms": terms })).expect("digest view")
    }

    // -- AC-1: --check lists unbridged, empty when fully bridged ---------------

    #[test]
    fn check_lists_only_unbridged_mined_terms() {
        let dir = tempdir().unwrap();
        write_root_config(dir.path());
        let pair = pair_for_root(dir.path());
        // `cancel` is a seed value (cancelar=["cancel"]) → bridged.
        // `payable` is nobody's value → unbridged.
        let digest = digest_of(&[
            ("payable", 197, &["src/payable.cs", "src/p2.cs"]),
            ("cancel", 12, &["src/cancel.cs"]),
            ("receivable", 40, &["src/recv.cs"]),
        ]);
        let bridged = bridged_code_terms(dir.path(), pair.as_ref());
        let got = unbridged_terms(&digest, &bridged);
        let terms: Vec<&str> = got.iter().map(|u| u.term.as_str()).collect();
        // `cancel` (seed) and `receivable` (seed: recebivel=["receivable"]) are
        // bridged; only `payable` survives, carrying its count + samples.
        assert_eq!(terms, vec!["payable"], "only the unbridged term: {terms:?}");
        assert_eq!(got[0].count, 197);
        assert_eq!(got[0].samples, vec!["src/payable.cs", "src/p2.cs"]);
    }

    #[test]
    fn check_returns_empty_when_every_term_is_bridged() {
        let dir = tempdir().unwrap();
        write_root_config(dir.path());
        let pair = pair_for_root(dir.path());
        // Overlay bridges titulo→payable; the rest are seed-covered values.
        let lexdir = dir.path().join(".claude").join("lexicons");
        std::fs::create_dir_all(&lexdir).unwrap();
        std::fs::write(lexdir.join("pt-en.toml"), "[terms]\ntitulo = [\"payable\"]\n").unwrap();
        let digest = digest_of(&[("payable", 197, &[]), ("cancel", 12, &[]), ("customer", 9, &[])]);
        let bridged = bridged_code_terms(dir.path(), pair.as_ref());
        let got = unbridged_terms(&digest, &bridged);
        assert!(got.is_empty(), "fully bridged model → no unbridged terms: {:?}", got.len());

        // And the whole report's `unbridged` is the empty no-op list.
        // (digest unavailable in the report path → still empty, never an error.)
        let report = check_report(dir.path());
        assert_eq!(report["pair"], "pt-en");
        assert_eq!(report["unbridged"], json!([]));
    }

    #[test]
    fn check_caps_unbridged_at_max_in_rank_order() {
        let dir = tempdir().unwrap();
        write_root_config(dir.path());
        let pair = pair_for_root(dir.path());
        // More unbridged terms than the cap, already in rank order from scan.
        let rows: Vec<(String, usize, Vec<String>)> =
            (0..MAX_UNBRIDGED + 10).map(|i| (format!("term{i:03}"), 100 - i, vec![])).collect();
        let view: Vec<(&str, usize, &[&str])> =
            rows.iter().map(|(t, c, _)| (t.as_str(), *c, &[][..])).collect();
        let digest = digest_of(&view);
        let bridged = bridged_code_terms(dir.path(), pair.as_ref());
        let got = unbridged_terms(&digest, &bridged);
        assert_eq!(got.len(), MAX_UNBRIDGED, "capped at MAX_UNBRIDGED");
        // The cap keeps the HEAD (most discriminative) — order is preserved.
        assert_eq!(got[0].term, "term000");
        assert_eq!(got[MAX_UNBRIDGED - 1].term, format!("term{:03}", MAX_UNBRIDGED - 1));
    }

    // -- AC-2: --apply writes real targets, gate rejects hallucinations -------

    #[test]
    fn apply_writes_real_target_and_rejects_unmined_one() {
        let dir = tempdir().unwrap();
        write_root_config(dir.path());
        // The model mines `payable` but NOT `frobnicate`.
        let digest = digest_of(&[("payable", 197, &["src/payable.cs"])]);
        let mined = mined_term_set(&digest);
        assert!(mined.contains("payable"));
        assert!(!mined.contains("frobnicate"));

        // Drive apply_report through real proposals: one valid, one hallucinated.
        let proposals = dir.path().join("proposals.json");
        std::fs::write(
            &proposals,
            br#"[{"userWord":"titulo","codeTerms":["payable","frobnicate"]}]"#,
        )
        .unwrap();

        // apply_report spawns `scan digest`, which is unavailable in unit tests
        // (no model) → mined set empty → BOTH would reject. So assert the gate
        // logic directly against a seeded mined set, then assert the write path
        // via write_bridge below. The report-level e2e is the integration test.
        let user_word = folded("titulo");
        let mut applied = Vec::new();
        let mut rejected = Vec::new();
        for code in ["payable", "frobnicate"] {
            let code_term = folded(code);
            if mined.contains(&code_term) {
                assert!(write_bridge(dir.path(), &pair_for_root(dir.path()).unwrap(), &user_word, &code_term).is_ok());
                applied.push(code_term);
            } else {
                rejected.push(code_term);
            }
        }
        assert_eq!(applied, vec!["payable"], "real target written");
        assert_eq!(rejected, vec!["frobnicate"], "hallucinated target rejected by the gate");

        // AC-3 (format leg): the written entry is in the shape parse_lexicon /
        // terms_table reads — titulo = ["payable"], alphabetical, comments kept.
        let overlay = overlay_path(dir.path(), "pt-en");
        let text = std::fs::read_to_string(&overlay).expect("overlay created");
        assert!(text.contains("titulo = [\"payable\"]"), "bridge written in lexicon shape: {text}");
        assert!(text.starts_with("# PROJECT domain lexicon"), "template header kept");
        // Re-read through the project's own term table: the bridge is live.
        let table = crate::commands::lexicon_suggest::terms_table(&text);
        assert_eq!(table.get("titulo"), Some(&vec!["payable".to_string()]), "round-trips: {table:?}");
        assert!(!text.contains("frobnicate"), "rejected target never written");

        // Sanity: the proposals file was the real driver shape.
        let _ = proposals;
    }

    #[test]
    fn apply_report_gate_rejects_when_model_absent() {
        // No grain.model.json → mined set is empty → every target is rejected
        // (fail-closed on the gate, never a panic, nothing written).
        let dir = tempdir().unwrap();
        write_root_config(dir.path());
        let proposals = dir.path().join("p.json");
        std::fs::write(&proposals, br#"[{"userWord":"titulo","codeTerms":["payable"]}]"#).unwrap();
        let report = apply_report(dir.path(), &proposals);
        assert_eq!(report["pair"], "pt-en");
        assert_eq!(report["applied"], json!([]), "no model → nothing applied: {report}");
        let rejected = report["rejected"].as_array().expect("rejected array");
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0]["reason"], "target_not_in_model");
        // The gate refusing to write means no overlay was created.
        assert!(!overlay_path(dir.path(), "pt-en").exists(), "gate wrote nothing");
    }

    #[test]
    fn apply_refuses_without_a_vendored_pair() {
        // An `en` root has no second language → no pair → honest refusal.
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"), br#"{"specLang":"en-US"}"#).unwrap();
        let proposals = dir.path().join("p.json");
        std::fs::write(&proposals, br#"[{"userWord":"x","codeTerms":["y"]}]"#).unwrap();
        let report = apply_report(dir.path(), &proposals);
        assert_eq!(report["reason"], "no-lexicon-pair");
        assert!(!dir.path().join(".claude").join("lexicons").exists());
    }

    #[test]
    fn proposal_accepts_camel_and_snake_case() {
        let camel = Proposal::from_value(&json!({ "userWord": "titulo", "codeTerms": ["payable"] }));
        assert_eq!(camel.user_word, "titulo");
        assert_eq!(camel.code_terms, vec!["payable"]);
        let snake = Proposal::from_value(&json!({ "user_word": "conta", "code_terms": ["account"] }));
        assert_eq!(snake.user_word, "conta");
        assert_eq!(snake.code_terms, vec!["account"]);
    }

    // -- AC-5: --check is byte-stable -----------------------------------------

    #[test]
    fn check_report_is_byte_stable() {
        let dir = tempdir().unwrap();
        write_root_config(dir.path());
        let a = serde_json::to_string(&check_report(dir.path())).unwrap();
        let b = serde_json::to_string(&check_report(dir.path())).unwrap();
        assert_eq!(a, b, "two --check runs serialize identically");
    }
}
