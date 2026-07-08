//! dictionary — the project's DISTINCTIVE DOMAIN VOCABULARY, mined
//! deterministically (no LLM) from BOTH declaration names AND comments/
//! docstrings, ranked by TF·IDF specificity so the ubiquitous plumbing (the
//! repo/namespace name, framework glue) and the natural-language glue fall away
//! and the discriminative mid-frequency domain terms rise. Written once per
//! scan as the byte-stable sidecar `.claude/grain.dictionary.json` — the ENGLISH
//! side of a dictionary a later step aliases to the request language to anchor a
//! query translation.
//!
//! ## Why this is a SCAN STAGE, not a projection from the finished model
//!
//! Comments live ONLY in the in-memory `content` map during a scan; the model's
//! [`Decl`](crate::model::Decl) keeps `kind/name/line/supertypes/purpose/
//! body_hash` and NEVER the comment text. So the "everything, including
//! comments" requirement forces this to run INSIDE `analyze` — alongside `mine`,
//! over `modules` + `content` — not as a `digest`/`purpose`-style projection
//! that reads only the finished `ProjectModel`.
//!
//! ## Two sources, one corpus
//!
//! - Identifiers: every declaration `name`, split by [`crate::digest::tokenize`]
//!   (case boundaries + acronyms, lowercased, <3-char glue dropped).
//! - Comments/docstrings: harvested agnostically from the source by the common
//!   markers (`//`, `#`, `/* */`, `<!-- -->`, `""" """`, `''' '''`), then split
//!   on non-alphanumerics with the same 3-char floor.
//!
//! The corpus is the modules (hand-written, non-test — the same eligibility
//! [`crate::purpose`] uses). Per candidate term: total `count`, document
//! frequency `df` (distinct modules), and
//! [`domain_specificity_x1024`](mustard_core::domain::ranking::domain_specificity_x1024)
//! — TF·IDF that PEAKS mid-frequency. A term in more than half the corpus is
//! plumbing (dropped); identifier glue, natural-language glue (en + the request
//! language pt) and the mined role affixes are dropped as glue.
//!
//! ## Determinism — no LLM, byte-stable, fail-open
//!
//! `BTreeMap`/`BTreeSet` throughout (stable iteration), fixed-point specificity
//! (no float ever enters a comparison), a total-order sort, and NO `unwrap`/
//! `expect` outside tests. Two runs over the same inputs serialize to identical
//! bytes. Fail-open: empty/absent `content` or no modules yields an empty
//! dictionary, never a panic.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use crate::matching::{fold, Ladder};
use crate::model::{Module, RoleStat};

/// A term must occur at least this many times (across both sources) to enter the
/// dictionary — a lone mention is noise, not vocabulary. Mirrors the scan's
/// "≥2 witnesses" bar for a real convention.
const MIN_COUNT: usize = 2;

/// Sample files per term: the modules where the term is most central. Kept
/// wider than a human "confirm the meaning" handful because these anchors are
/// also the SEED set the personalized-PageRank ranker (`pagerank`) resolves a
/// PT comment-term to — a term whose identifiers are English never resolves to
/// a model declaration, so its comment anchors are the only seeds it has, and a
/// sparse seed set gives the graph nothing to propagate. Still bounded (a few
/// hundred bytes/term on the sidecar).
const MAX_ANCHORS: usize = 15;

/// Upper bound on the dictionary size: the most distinctive terms are kept, the
/// long tail dropped, so the sidecar stays AI-sized on a large repo. The top of
/// the distribution — what anchors a query — is unaffected.
const MAX_TERMS: usize = 500;

/// Sidecar schema version — bumped when the shape changes.
const VERSION: u32 = 1;

/// The byte-stable dictionary sidecar (`grain.dictionary.json`). `Deserialize`
/// too, so a consumer (the `pagerank` ranker) reads the sidecar back without a
/// second schema.
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Dictionary {
    pub version: u32,
    /// Distinctive domain terms, ordered by `term` ascending (byte-stable).
    pub terms: Vec<DictEntry>,
}

impl Default for Dictionary {
    fn default() -> Self {
        Dictionary { version: VERSION, terms: Vec::new() }
    }
}

/// One distinctive term: its TF·IDF specificity, total occurrences, document
/// frequency, up to [`MAX_ANCHORS`] sample files where it is most central, and
/// which source(s) witnessed it.
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DictEntry {
    pub term: String,
    pub specificity_x1024: u64,
    pub count: usize,
    /// Document frequency — distinct modules the term occurs in. `idf` is
    /// recoverable as `specificity_x1024 / count`, but `df` is published
    /// directly so a consumer weights a seed by rarity without re-deriving it.
    /// Additive (serde default = 0 for an older sidecar).
    #[serde(default)]
    pub df: usize,
    pub anchors: Vec<String>,
    /// "ident" | "comment" | "both".
    pub source: String,
}

/// Per-term accumulator over the corpus. `tf` (module path -> occurrences in
/// that module) yields BOTH the document frequency (`tf.len()`) and the anchors
/// (top modules by occurrence), so one map carries every signal.
#[derive(Default)]
struct TermAgg {
    count: usize,
    from_ident: bool,
    from_comment: bool,
    tf: BTreeMap<String, usize>,
}

/// Build the distinctive-vocabulary dictionary from the mined `modules`, their
/// in-memory `content` (the only place comments survive), and the mined role
/// affixes (structural glue to demote). Pure given its inputs; see the module
/// note for the determinism + fail-open contract.
pub fn build(modules: &[Module], content: &HashMap<String, String>, roles: &[RoleStat]) -> Dictionary {
    let ident_glue = crate::digest::stopwords(); // identifier glue (stopwords.toml)
    let ladder = Ladder::new(); // en natural-language glue via query_stopword
    let nl_glue = natural_language_glue(); // en + pt stoplists, accent-folded
    let role_glue = role_glue(roles); // mined structural affixes (Repository, …)

    // One pass over the eligible corpus: accumulate per-term signals and count
    // the documents (for IDF). BTreeMap → deterministic term iteration.
    let mut agg: BTreeMap<String, TermAgg> = BTreeMap::new();
    let mut n_docs = 0usize;
    for m in modules {
        // Same gate as the purpose index: a machine-written or test module is
        // never domain vocabulary you would anchor a query on.
        if mustard_core::domain::ast::is_test_path(&m.path) || !crate::classify::anchor_eligible(&m.file_class) {
            continue;
        }
        n_docs += 1;
        for d in &m.declarations {
            for tok in crate::digest::tokenize(&d.name) {
                bump(&mut agg, tok, &m.path, true);
            }
        }
        if let Some(src) = content.get(&m.path) {
            for tok in comment_tokens(src) {
                bump(&mut agg, tok, &m.path, false);
            }
        }
    }

    // Filter + score. Drop glue, the ubiquitous (in > half the corpus — the
    // repo/namespace name, top framework), the sub-threshold and the
    // zero-specificity (degenerate). What survives is discriminative vocabulary.
    let mut entries: Vec<DictEntry> = Vec::new();
    for (term, a) in agg {
        if a.count < MIN_COUNT {
            continue;
        }
        if ident_glue.contains(&term)
            || nl_glue.contains(&term)
            || nl_glue.contains(&fold(&term))
            || ladder.query_stopword(&term)
            || role_glue.contains(&term)
        {
            continue;
        }
        let df = a.tf.len();
        // Ubiquity ceiling: a term in more than half the modules is plumbing,
        // not distinctive — a corpus-derived cutoff, never a hand-curated list.
        if df * 2 > n_docs {
            continue;
        }
        let specificity_x1024 = mustard_core::domain::ranking::domain_specificity_x1024(a.count, df, n_docs);
        if specificity_x1024 == 0 {
            continue;
        }
        let source = match (a.from_ident, a.from_comment) {
            (true, true) => "both",
            (false, true) => "comment",
            _ => "ident",
        }
        .to_string();
        entries.push(DictEntry { term, specificity_x1024, count: a.count, df, anchors: top_anchors(&a.tf), source });
    }

    // Keep the MOST DISTINCTIVE up to the cap (specificity desc, term asc), then
    // present them ordered BY TERM (the sidecar's stated order). Both sorts are
    // total orders, so the output is byte-stable across runs.
    entries.sort_by(|a, b| b.specificity_x1024.cmp(&a.specificity_x1024).then(a.term.cmp(&b.term)));
    entries.truncate(MAX_TERMS);
    entries.sort_by(|a, b| a.term.cmp(&b.term));

    Dictionary { version: VERSION, terms: entries }
}

/// Record one occurrence of `term` in module `path` from identifiers (`ident`)
/// or comments (`!ident`).
fn bump(agg: &mut BTreeMap<String, TermAgg>, term: String, path: &str, ident: bool) {
    let e = agg.entry(term).or_default();
    e.count += 1;
    *e.tf.entry(path.to_string()).or_insert(0) += 1;
    if ident {
        e.from_ident = true;
    } else {
        e.from_comment = true;
    }
}

/// The up-to-[`MAX_ANCHORS`] modules where the term is most central: most
/// occurrences first, ties on path ascending (byte-stable).
fn top_anchors(tf: &BTreeMap<String, usize>) -> Vec<String> {
    let mut v: Vec<(&String, usize)> = tf.iter().map(|(p, &c)| (p, c)).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    v.into_iter().take(MAX_ANCHORS).map(|(p, _)| p.clone()).collect()
}

/// Natural-language glue for en + the request language (pt): every stop word
/// from the vendored Snowball stoplists, stored raw AND accent-folded — the same
/// parse [`Ladder::new`] applies to the en list, extended to pt so a PT comment's
/// glue ("de", "para", "não") never becomes a dictionary term. Data, not logic.
pub(crate) fn natural_language_glue() -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for code in ["en", "pt"] {
        for line in crate::stemmers::stoplist(code).lines() {
            let w = line.trim().to_lowercase();
            if w.is_empty() || w.starts_with('#') {
                continue;
            }
            set.insert(fold(&w));
            set.insert(w);
        }
    }
    set
}

/// Structural role affixes, tokenized to lowercase glue — the mined convention
/// suffixes/prefixes (Repository, Service, Handler, …) are type plumbing, not
/// domain vocabulary, so a term equal to one is dropped.
fn role_glue(roles: &[RoleStat]) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for r in roles {
        for tok in crate::digest::tokenize(&r.affix) {
            set.insert(tok);
        }
    }
    set
}

/// Split harvested comment text into lowercased domain tokens: non-alphanumeric
/// boundaries, a 3-char floor, and at least one alphabetic char (drops pure
/// numbers) — the same shape [`crate::digest::tokenize`] lands identifiers in.
fn comment_tokens(src: &str) -> Vec<String> {
    harvest_comments(src)
        .split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|w| w.len() >= 3 && w.chars().any(char::is_alphabetic))
        .collect()
}

/// Extract the text of every comment/docstring from `src`, agnostic of language,
/// by the common markers. A linear byte scan (all markers are ASCII, so every
/// cut is on a char boundary — the sliced text may hold any UTF-8). Unclosed
/// blocks run to end-of-input; nothing panics.
fn harvest_comments(src: &str) -> String {
    const LINE2: &[u8] = b"//";
    const BLOCK_OPEN: &[u8] = b"/*";
    const BLOCK_CLOSE: &[u8] = b"*/";
    const HTML_OPEN: &[u8] = b"<!--";
    const HTML_CLOSE: &[u8] = b"-->";
    const DQ: &[u8] = b"\"\"\"";
    const SQ: &[u8] = b"'''";

    let bytes = src.as_bytes();
    let n = bytes.len();
    let mut out = String::new();
    let mut i = 0;
    while i < n {
        // Line comments (`//`, `#`): to end of line.
        if marker_at(bytes, i, LINE2) || bytes[i] == b'#' {
            let start = i + if bytes[i] == b'#' { 1 } else { LINE2.len() };
            let mut j = start;
            while j < n && bytes[j] != b'\n' {
                j += 1;
            }
            push_slice(&mut out, src, start, j);
            i = j;
            continue;
        }
        // Block comments (`/* */`, `<!-- -->`) and docstrings (`""" """`,
        // `''' '''`): to the matching close marker (or end of input).
        if let Some((open, close)) = block_markers(bytes, i, BLOCK_OPEN, BLOCK_CLOSE, HTML_OPEN, HTML_CLOSE, DQ, SQ) {
            let start = i + open.len();
            let mut j = start;
            while j < n && !marker_at(bytes, j, close) {
                j += 1;
            }
            push_slice(&mut out, src, start, j.min(n));
            i = if j < n { j + close.len() } else { n };
            continue;
        }
        i += 1;
    }
    out
}

/// The (open, close) marker pair whose OPEN begins at byte `i`, if any — checked
/// longest-first so `<!--` beats a stray `<`. `None` = no block opens here.
#[allow(clippy::too_many_arguments)]
fn block_markers<'a>(
    bytes: &[u8],
    i: usize,
    block_open: &'a [u8],
    block_close: &'a [u8],
    html_open: &'a [u8],
    html_close: &'a [u8],
    dq: &'a [u8],
    sq: &'a [u8],
) -> Option<(&'a [u8], &'a [u8])> {
    if marker_at(bytes, i, html_open) {
        Some((html_open, html_close))
    } else if marker_at(bytes, i, dq) {
        Some((dq, dq))
    } else if marker_at(bytes, i, sq) {
        Some((sq, sq))
    } else if marker_at(bytes, i, block_open) {
        Some((block_open, block_close))
    } else {
        None
    }
}

/// True when ASCII marker `m` begins at byte `i` of `bytes`.
fn marker_at(bytes: &[u8], i: usize, m: &[u8]) -> bool {
    bytes.len() >= i + m.len() && &bytes[i..i + m.len()] == m
}

/// Append `src[start..end]` and a space separator (both offsets are on char
/// boundaries — they sit at ASCII markers/newline/end).
fn push_slice(out: &mut String, src: &str, start: usize, end: usize) {
    if let Some(slice) = src.get(start..end) {
        out.push_str(slice);
        out.push(' ');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Decl;

    /// A hand-written module (empty class → anchor-eligible) with the given
    /// declaration names.
    fn module(path: &str, names: &[&str]) -> Module {
        Module {
            path: path.to_string(),
            declarations: names
                .iter()
                .map(|n| Decl { kind: "function".to_string(), name: (*n).to_string(), ..Default::default() })
                .collect(),
            ..Default::default()
        }
    }

    fn find<'a>(dict: &'a Dictionary, term: &str) -> Option<&'a DictEntry> {
        dict.terms.iter().find(|e| e.term == term)
    }

    /// `n` padding modules, each a unique single-count name — they never survive
    /// the count floor themselves; they exist only to enlarge the corpus so
    /// document frequencies (and thus IDF) are realistic. A one-or-two module
    /// corpus has NO discriminative power (every term is in every/both docs → idf
    /// 0), which is correct but useless for exercising the ranking.
    fn fillers(n: usize) -> Vec<Module> {
        (0..n).map(|i| module(&format!("src/filler/f{i}.rs"), &[&format!("FillerNode{i}")])).collect()
    }

    /// (a) EXTRACTION — a term from a declaration name and a term from a comment
    /// both surface, with the right `source`; a term seen in BOTH is "both".
    #[test]
    fn extracts_from_identifiers_and_comments() {
        let mut modules = vec![
            module("src/payments/payable_service.rs", &["PayableService", "approvePayable"]),
            module("src/ledger/reconcile.rs", &[]),
            module("src/ledger/report.rs", &[]),
        ];
        modules.extend(fillers(5)); // corpus of 8 → payable's df=2 stays distinctive
        let mut content = HashMap::new();
        // "payable" also appears in a COMMENT (it is an identifier in another
        // module) → source "both". "statement" is comment-only, in two modules.
        content.insert("src/ledger/reconcile.rs".to_string(), "// the statement covers each payable".to_string());
        content.insert("src/ledger/report.rs".to_string(), "// export the statement to the ledger".to_string());
        let dict = build(&modules, &content, &[]);

        let payable = find(&dict, "payable").expect("`payable` mined from identifiers");
        assert_eq!(payable.source, "both", "payable is in an identifier AND a comment");
        assert_eq!(payable.count, 3, "two identifier tokens + one comment mention");
        assert_eq!(payable.anchors.first().map(String::as_str), Some("src/payments/payable_service.rs"), "the module with the most occurrences anchors first");

        let statement = find(&dict, "statement").expect("`statement` mined from a comment");
        assert_eq!(statement.source, "comment");
    }

    /// (b) RANKING — a mid-frequency term is more specific than a rarer one, and
    /// a term in EVERY module (the repo-name pattern) is dropped as ubiquitous.
    #[test]
    fn ranks_by_specificity_and_drops_ubiquitous() {
        // "widget" in ALL eight modules (the repo/namespace pattern) → dropped as
        // ubiquitous. "payable" (df 3, count 5) and "escrow" (df 2, count 2) are
        // both distinctive; payable is more so.
        let modules = vec![
            module("src/a.rs", &["WidgetOne", "PayableService", "ApprovePayable"]),
            module("src/b.rs", &["WidgetTwo", "PayableList", "PayableTotal"]),
            module("src/c.rs", &["WidgetThree", "PayableView"]),
            module("src/d.rs", &["WidgetFour", "EscrowAccount"]),
            module("src/e.rs", &["WidgetFive", "EscrowHold"]),
            module("src/f.rs", &["WidgetSix"]),
            module("src/g.rs", &["WidgetSeven"]),
            module("src/h.rs", &["WidgetEight"]),
        ];
        let dict = build(&modules, &HashMap::new(), &[]);
        assert!(find(&dict, "widget").is_none(), "ubiquitous term dropped: {:?}", terms(&dict));
        let payable = find(&dict, "payable").expect("payable kept");
        let escrow = find(&dict, "escrow").expect("escrow kept");
        assert!(
            payable.specificity_x1024 > escrow.specificity_x1024,
            "mid-frequency payable ({}) outranks rarer escrow ({})",
            payable.specificity_x1024,
            escrow.specificity_x1024,
        );
    }

    /// (c) GLUE — identifier glue, natural-language glue (en + pt) and mined role
    /// affixes are all excluded even when frequent.
    #[test]
    fn drops_identifier_natural_language_and_role_glue() {
        let mut modules = vec![
            module("src/a.rs", &["OrderRepository", "OrderService"]),
            module("src/b.rs", &["OrderController"]),
        ];
        modules.extend(fillers(4)); // corpus of 6 → "order" (df 2) survives
        let mut content = HashMap::new();
        // "para"/"com" are pt glue (vendored Snowball list); "from"/"the" are en
        // glue — they recur across the comments but must never become terms; the
        // mined role affix "Repository" (an identifier token here) is structural,
        // also dropped.
        content.insert("src/a.rs".to_string(), "// para o order com o repository".to_string());
        content.insert("src/b.rs".to_string(), "// para o order com o repository from the list".to_string());
        let roles = vec![RoleStat { affix: "Repository".to_string(), ..Default::default() }];

        let dict = build(&modules, &content, &roles);
        assert!(find(&dict, "order").is_some(), "the domain term survives alongside the glue: {:?}", terms(&dict));
        assert!(find(&dict, "repository").is_none(), "mined role affix dropped");
        assert!(find(&dict, "para").is_none() && find(&dict, "com").is_none(), "pt glue dropped");
        assert!(find(&dict, "from").is_none() && find(&dict, "the").is_none(), "en glue dropped");
    }

    /// (d) BYTE-STABILITY — two builds over the same inputs serialize identically.
    #[test]
    fn is_byte_stable() {
        let modules = vec![
            module("src/z.rs", &["ContractRenewal", "ContractTerm"]),
            module("src/a.rs", &["ContractRenewal", "InvoiceContract"]),
        ];
        let mut content = HashMap::new();
        content.insert("src/a.rs".to_string(), "/* renew the contract each period */".to_string());
        let one = serde_json::to_string(&build(&modules, &content, &[])).expect("serialize");
        let two = serde_json::to_string(&build(&modules, &content, &[])).expect("serialize");
        assert_eq!(one, two, "two runs are byte-identical");
    }

    /// (e) FAIL-OPEN — empty/absent content and no modules yield an empty
    /// dictionary, never a panic.
    #[test]
    fn fails_open_on_empty_inputs() {
        let empty = build(&[], &HashMap::new(), &[]);
        assert_eq!(empty.version, VERSION);
        assert!(empty.terms.is_empty(), "no modules → empty dictionary");

        // Modules present but content absent for their paths → identifiers still
        // mine, comments simply contribute nothing (no panic on the missing key).
        let modules = vec![
            module("src/m1.rs", &["ReconcilePayable", "ReconcileInvoice"]),
            module("src/m2.rs", &["ReconcileLedger"]),
            module("src/m3.rs", &["GammaThing"]),
            module("src/m4.rs", &["DeltaBox"]),
        ];
        let dict = build(&modules, &HashMap::new(), &[]);
        assert!(find(&dict, "reconcile").is_some(), "identifiers mine without any content");
        assert!(dict.terms.iter().all(|e| e.source == "ident"), "no comment source when content is absent");
    }

    /// The comment harvester pulls text out of every supported marker and leaves
    /// code outside comments untouched.
    #[test]
    fn harvest_comments_covers_every_marker() {
        let src = "let x = 1; // line-en\n# hash-line\n/* block-c */\n<!-- html-x -->\n\"\"\"doc-py\"\"\"\n'''doc-sq'''";
        let toks: BTreeSet<String> = comment_tokens(src).into_iter().collect();
        for expected in ["line", "hash", "line", "block", "html", "doc"] {
            assert!(toks.contains(expected), "marker text harvested: {expected} in {toks:?}");
        }
        // Code outside a comment (the `let x` identifiers) is NOT harvested here
        // (identifiers come from decl names, not the raw source).
        assert!(!toks.contains("let"), "non-comment code is not harvested by the comment pass");
    }

    fn terms(dict: &Dictionary) -> Vec<&str> {
        dict.terms.iter().map(|e| e.term.as_str()).collect()
    }
}
