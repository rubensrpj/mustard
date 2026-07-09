//! `scan-equivalences` — project the scan dictionary's non-English terms
//! through the local `mustard-translate` sidecar into
//! `.claude/grain.equivalences.json`: the PT→EN query-expansion table the
//! `feature` retrieval feeds to `scan rank` (the measured C2 winner —
//! query = raw PT + these EN tokens).
//!
//! Direct port of the `equivalences-mt` generator in
//! `benchmarks/sialia/compare-equiv.ps1`: ONE `mustard-translate batch` over
//! every dictionary term (positional 1:1 contract); a term the sidecar
//! detects as already-English gets no alias; the translation is tokenized
//! (non-alphanumeric split, ≥3 chars, lowercased), the term's own folded form
//! is removed, and the first [`TOP_TOKENS`] distinct tokens become the alias
//! list. Keys are the accent-folded terms, emitted sorted (byte-stable).
//!
//! Runs automatically at the end of `run scan` (after model + dictionary) and
//! stands alone as `mustard-rt run scan-equivalences`. FAIL-OPEN everywhere:
//! a missing dictionary or absent translator yields `{ok:false, reason}` on
//! exit 0 and never blocks the scan.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{json, Value};

use crate::shared::translate::{Translate, Translation};

/// Alias cap per term (`compare-equiv.ps1`'s `$TopTokens`).
const TOP_TOKENS: usize = 4;

/// Lowercase + fold Latin diacritics to their ASCII base letter — the exact
/// character table of the scan tool's `matching::fold`, applied over the
/// lowercased input (the PS1 `Fold-Tok` shape). Keys built here MUST match
/// the fold the query-expansion side applies, or lookups silently miss.
pub(crate) fn fold_tok(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'ç' => 'c',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ñ' => 'n',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            'ý' | 'ÿ' => 'y',
            _ => c,
        })
        .collect()
}

/// Tokenize one MT translation into alias tokens: split on non-alphanumeric
/// (ASCII runs, the PS1 splitter), lowercase, keep length ≥3 with at least one
/// letter, drop the term's own folded form, dedupe preserving first-occurrence
/// order, cap at [`TOP_TOKENS`].
fn translation_tokens(en: &str, term_folded: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in en.split(|c: char| !c.is_ascii_alphanumeric()) {
        if raw.len() < 3 {
            continue;
        }
        let t = raw.to_lowercase();
        if !t.chars().any(|c| c.is_ascii_alphabetic()) || t == term_folded || out.contains(&t) {
            continue;
        }
        out.push(t);
        if out.len() >= TOP_TOKENS {
            break;
        }
    }
    out
}

/// Fold the (term, translation) rows into the equivalence map: a term the
/// sidecar detected as English gets no alias (it already IS code vocabulary);
/// an empty token list drops the entry. `BTreeMap` keeps the keys sorted —
/// the byte-stable artifact order.
fn build_equivalences(rows: &[(String, Translation)]) -> BTreeMap<String, Vec<String>> {
    let mut map = BTreeMap::new();
    for (term, tr) in rows {
        if tr.detected == "en" {
            continue;
        }
        let key = fold_tok(term);
        let toks = translation_tokens(&tr.en, &key);
        if !toks.is_empty() {
            map.insert(key, toks);
        }
    }
    map
}

/// Read `.claude/grain.equivalences.json` under `root` into the expansion map.
/// Fail-open: missing/unparseable file → empty map (query expands to itself).
pub(crate) fn load_equivalences(root: &Path) -> BTreeMap<String, Vec<String>> {
    let path = root.join(".claude").join("grain.equivalences.json");
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.get("equivalences").cloned())
        .and_then(|e| serde_json::from_value::<BTreeMap<String, Vec<String>>>(e).ok())
        .unwrap_or_default()
}

/// Generate `<dict dir>/grain.equivalences.json` from the dictionary at
/// `dict_path`. Returns the JSON summary (never panics, never exits non-zero):
/// `{ok:true, terms, aliased, out}` or `{ok:false, reason}`.
pub(crate) fn generate_at(dict_path: &Path) -> Value {
    let Ok(raw) = std::fs::read_to_string(dict_path) else {
        return json!({ "ok": false, "reason": "no-dictionary" });
    };
    let Ok(dict) = serde_json::from_str::<Value>(&raw) else {
        return json!({ "ok": false, "reason": "bad-dictionary" });
    };
    let terms: Vec<String> = dict
        .get("terms")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|e| e.get("term").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if terms.is_empty() {
        return json!({ "ok": false, "reason": "no-terms" });
    }
    let Some(translator) = Translate::locate() else {
        return json!({ "ok": false, "reason": "translator-unavailable" });
    };
    let Some(translations) = translator.batch(&terms) else {
        return json!({ "ok": false, "reason": "batch-failed" });
    };
    let rows: Vec<(String, Translation)> = terms.into_iter().zip(translations).collect();
    let map = build_equivalences(&rows);
    let body = json!({ "version": 1, "equivalences": map });
    let Ok(pretty) = serde_json::to_string_pretty(&body) else {
        return json!({ "ok": false, "reason": "serialize-failed" });
    };
    let out_path = dict_path.with_file_name("grain.equivalences.json");
    if let Err(e) = mustard_core::io::fs::write_atomic(&out_path, format!("{pretty}\n").as_bytes()) {
        eprintln!("scan-equivalences: cannot write {}: {e}", out_path.display());
        return json!({ "ok": false, "reason": "write-failed" });
    }
    json!({
        "ok": true,
        "terms": rows.len(),
        "aliased": map.len(),
        "out": out_path.to_string_lossy(),
    })
}

/// Run the subcommand: (re)generate the equivalences artifact for the
/// dictionary under `<root>/.claude/` and print the JSON summary.
pub fn run(root: &Path) {
    let dict_path = root.join(".claude").join("grain.dictionary.json");
    let result = generate_at(&dict_path);
    println!("{}", serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".into()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tr(en: &str, detected: &str) -> Translation {
        Translation { en: en.to_string(), detected: detected.to_string() }
    }

    #[test]
    fn fold_tok_lowercases_and_strips_diacritics() {
        assert_eq!(fold_tok("Conciliação"), "conciliacao");
        assert_eq!(fold_tok("Título"), "titulo");
        assert_eq!(fold_tok("supplier"), "supplier", "plain ASCII passes through");
    }

    #[test]
    fn translation_tokens_ports_the_ps1_rules() {
        // Split non-alphanumeric, ≥3 chars, lowercase, dedupe, cap 4 — and the
        // term's own folded form never aliases itself.
        let t = translation_tokens("Bank statement reconciliation, of the bank account", "conciliacao");
        assert_eq!(t, vec!["bank", "statement", "reconciliation", "the"], "of (<3) dropped; bank deduped; cap 4");
        assert_eq!(
            translation_tokens("Supplier", "supplier"),
            Vec::<String>::new(),
            "token == folded term removed"
        );
        assert_eq!(translation_tokens("a of 42", "x"), Vec::<String>::new(), "short + letterless dropped");
    }

    #[test]
    fn build_equivalences_skips_english_and_sorts_keys_byte_stably() {
        let rows = vec![
            ("título".to_string(), tr("Title of the bill", "pt")),
            ("handler".to_string(), tr("handler", "en")), // detected en → no alias
            ("conciliação".to_string(), tr("Reconciliation", "pt")),
            ("vazio".to_string(), tr("", "pt")), // empty translation → no entry
        ];
        let map = build_equivalences(&rows);
        assert_eq!(
            map.keys().collect::<Vec<_>>(),
            vec!["conciliacao", "titulo"],
            "folded keys, sorted; en + empty skipped"
        );
        assert_eq!(map["titulo"], vec!["title", "the", "bill"]);
        assert_eq!(map["conciliacao"], vec!["reconciliation"]);
        // Byte-stable: same rows → same serialized artifact body.
        let a = serde_json::to_string(&json!({"version": 1, "equivalences": build_equivalences(&rows)})).expect("ser");
        let b = serde_json::to_string(&json!({"version": 1, "equivalences": build_equivalences(&rows)})).expect("ser");
        assert_eq!(a, b);
    }

    #[test]
    fn generate_at_fails_open_without_a_dictionary() {
        let missing = std::env::temp_dir().join("mustard-no-such-dir-e2e").join("grain.dictionary.json");
        let v = generate_at(&missing);
        assert_eq!(v["ok"], json!(false));
        assert_eq!(v["reason"], json!("no-dictionary"));
    }

    #[test]
    fn load_equivalences_fails_open_to_an_empty_map() {
        let root = std::env::temp_dir().join("mustard-no-such-root-e2e");
        assert!(load_equivalences(&root).is_empty());
    }
}
