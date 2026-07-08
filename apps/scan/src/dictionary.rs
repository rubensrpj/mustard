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
//!   markers (`//`, `#`, `/* */`, `<!-- -->`, `""" """`, `''' '''`), one
//!   SEGMENT per comment, then NORMALIZED TO ENGLISH (below) and split on
//!   non-alphanumerics with the same 3-char floor.
//!
//! The corpus is the modules (hand-written, non-test — the same eligibility
//! [`crate::purpose`] uses). Per candidate term: total `count`, document
//! frequency `df` (distinct modules), and
//! [`domain_specificity_x1024`](mustard_core::domain::ranking::domain_specificity_x1024)
//! — TF·IDF that PEAKS mid-frequency. A term in more than half the corpus is
//! plumbing (dropped); identifier glue, natural-language glue (en + the request
//! language pt) and the mined role affixes are dropped as glue.
//!
//! ## Comments are normalized to ENGLISH (the EN→EN retrieval contract)
//!
//! Identifiers are English-canonical; a dictionary whose comment terms stay in
//! the author's language forces every consumer to bridge languages at QUERY
//! time (measured loser on the sialia benchmark). So the scan normalizes at
//! MINE time: each harvested comment segment is language-checked by a light
//! deterministic heuristic (vendored en/pt stoplist hits + Latin-accent
//! evidence — mirroring the sidecar's {en,pt,es,fr} scope; es/fr ride on the
//! shared function words and accents), and the non-English segments go — as
//! WHOLE sentences, never word by word, deduped and sorted — in ONE batch to
//! the local `mustard-translate batch` sidecar (spawned once; found via PATH,
//! then `target/release`, then `target/debug`). The sidecar's own lingua
//! verdict wins: a line it calls English passes through untranslated. The
//! TRANSLATION is what gets tokenized into the vocabulary.
//!
//! Fail-open: sidecar absent/failed → the raw segment is tokenized exactly as
//! before, and `non_english_comments` on the sidecar JSON counts the comment
//! occurrences that entered the dictionary untranslated — the "fix this later"
//! signal (0 when normalization fully applied).
//!
//! ## Determinism — no cloud/LLM, byte-stable PER MACHINE, fail-open
//!
//! `BTreeMap`/`BTreeSet` throughout (stable iteration), fixed-point specificity
//! (no float ever enters a comparison), a total-order sort, and NO `unwrap`/
//! `expect` outside tests. Two runs over the same inputs ON THE SAME MACHINE
//! serialize to identical bytes: the sidecar decodes greedily (deterministic)
//! and the batch input is sorted. Cross-platform caveat: the sidecar's MT
//! inference is floating-point, so a DIFFERENT machine/BLAS may translate a
//! sentence differently (each still self-deterministic) — unlike the rest of
//! the scan, which is integer-only, cross-machine byte-equality of the
//! dictionary is not guaranteed when translation runs. Fail-open: empty/absent
//! `content` or no modules yields an empty dictionary, never a panic.

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
    /// Comment occurrences detected as non-English that entered the vocabulary
    /// UNTRANSLATED (sidecar absent/failed) — the "normalize me later" signal;
    /// 0 when the EN normalization fully applied. Additive (serde default).
    #[serde(default)]
    pub non_english_comments: usize,
    /// Distinctive domain terms, ordered by `term` ascending (byte-stable).
    pub terms: Vec<DictEntry>,
}

impl Default for Dictionary {
    fn default() -> Self {
        Dictionary { version: VERSION, non_english_comments: 0, terms: Vec::new() }
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

/// One translated line of the sidecar's batch protocol
/// (`{"en":"...","detected":"pt"}` — same order as the input lines).
#[derive(Deserialize)]
struct SidecarLine {
    en: String,
    #[serde(default)]
    detected: String,
}

/// A batch translator: all unique non-English segments in, one translated line
/// per segment out (same order), or `None` when the whole batch failed —
/// injectable so tests never spawn a process.
type BatchTranslate<'a> = &'a mut dyn FnMut(&[String]) -> Option<Vec<SidecarLine>>;

/// Build the dictionary WITHOUT translation (the fail-open shape): non-English
/// comment segments are tokenized raw, exactly as before, and counted into
/// `non_english_comments`. Pure given its inputs — the hermetic path tests use.
pub fn build(modules: &[Module], content: &HashMap<String, String>, roles: &[RoleStat]) -> Dictionary {
    build_with(modules, content, roles, None)
}

/// Build the dictionary WITH the EN normalization: non-English comment
/// segments go in one batch to the local `mustard-translate` sidecar (PATH →
/// `target/release` → `target/debug`; spawned once). Sidecar absent/failed →
/// identical to [`build`]. Deterministic per machine; see the module note.
pub fn build_normalized(modules: &[Module], content: &HashMap<String, String>, roles: &[RoleStat]) -> Dictionary {
    let mut sidecar = |lines: &[String]| sidecar_translate_batch(lines);
    build_with(modules, content, roles, Some(&mut sidecar))
}

/// The shared core: harvest comment segments per eligible module, detect the
/// non-English ones, translate them (when a translator is given) in ONE
/// deduped+sorted batch, then accumulate identifiers + (normalized) comment
/// tokens into the ranked dictionary.
fn build_with(
    modules: &[Module],
    content: &HashMap<String, String>,
    roles: &[RoleStat],
    translate: Option<BatchTranslate>,
) -> Dictionary {
    let ident_glue = crate::digest::stopwords(); // identifier glue (stopwords.toml)
    let ladder = Ladder::new(); // en natural-language glue via query_stopword
    let nl_glue = natural_language_glue(); // en + pt stoplists, accent-folded
    let role_glue = role_glue(roles); // mined structural affixes (Repository, …)
    let en_stop = stoplist_words("en");
    let pt_stop = stoplist_words("pt");

    // Pass 1 — per eligible module (the same gate as the purpose index: a
    // machine-written or test module is never domain vocabulary you would
    // anchor a query on), harvest the comment segments and flag the
    // non-English ones; collect the unique foreign segments for ONE batch.
    let mut per_module: Vec<(&Module, Vec<(String, bool)>)> = Vec::new();
    let mut unique_foreign: BTreeSet<String> = BTreeSet::new();
    for m in modules {
        if mustard_core::domain::ast::is_test_path(&m.path) || !crate::classify::anchor_eligible(&m.file_class) {
            continue;
        }
        let flagged: Vec<(String, bool)> = content
            .get(&m.path)
            .map(|src| comment_segments(src))
            .unwrap_or_default()
            .into_iter()
            .map(|seg| {
                let foreign = is_non_english(&seg, &en_stop, &pt_stop);
                if foreign {
                    unique_foreign.insert(seg.clone());
                }
                (seg, foreign)
            })
            .collect();
        per_module.push((m, flagged));
    }

    // Pass 2 — one sidecar batch over the sorted unique foreign segments
    // (BTreeSet order → the batch input is byte-stable across runs), mapped
    // back segment → translated line. `None` = no translator / batch failed.
    let foreign: Vec<String> = unique_foreign.into_iter().collect();
    let translated: Option<BTreeMap<&str, SidecarLine>> = match translate {
        Some(t) if !foreign.is_empty() => t(&foreign)
            .filter(|out| out.len() == foreign.len())
            .map(|out| foreign.iter().map(String::as_str).zip(out).collect()),
        _ => None,
    };

    // Pass 3 — accumulate per-term signals and count the documents (for IDF).
    // BTreeMap → deterministic term iteration. Foreign segments tokenize their
    // TRANSLATION; without one they tokenize raw and are counted.
    let mut agg: BTreeMap<String, TermAgg> = BTreeMap::new();
    let mut non_english_comments = 0usize;
    let n_docs = per_module.len();
    for (m, segments) in &per_module {
        for d in &m.declarations {
            for tok in crate::digest::tokenize(&d.name) {
                bump(&mut agg, tok, &m.path, true);
            }
        }
        for (seg, foreign) in segments {
            let text: &str = if *foreign {
                match translated.as_ref().and_then(|map| map.get(seg.as_str())) {
                    // The sidecar's lingua verdict wins: a line it calls
                    // English (or undetectable) passed through — not foreign.
                    Some(line) if line.detected == "en" || line.detected == "unknown" => &line.en,
                    // Genuinely translated: tokenize the English sentence.
                    Some(line) if line.en != *seg => &line.en,
                    // Sidecar passed a non-English line through (its own
                    // fail-open) — raw, and counted as untranslated.
                    Some(_) => {
                        non_english_comments += 1;
                        seg
                    }
                    // No sidecar / whole batch failed — raw + counted.
                    None => {
                        non_english_comments += 1;
                        seg
                    }
                }
            } else {
                seg
            };
            for tok in segment_tokens(text) {
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

    Dictionary { version: VERSION, non_english_comments, terms: entries }
}

// ---------------------------------------------------------------------------
// Language heuristic — deterministic, dependency-free. The sidecar's lingua
// makes the FINAL call; this only routes segments, so it is biased to send
// (a falsely-sent English line passes through the sidecar unchanged, while a
// falsely-kept foreign line merely keeps today's raw behavior).
// ---------------------------------------------------------------------------

/// The vendored Snowball stoplist for `code` as a raw lowercase word set —
/// kept SEPARATE per language (unlike [`natural_language_glue`], which merges
/// them) because detection needs the two sides to vote against each other.
fn stoplist_words(code: &str) -> BTreeSet<String> {
    crate::stemmers::stoplist(code)
        .lines()
        .map(|l| l.trim().to_lowercase())
        .filter(|w| !w.is_empty() && !w.starts_with('#'))
        .collect()
}

/// True when a comment segment reads as NON-English: its pt-stoplist function
/// words outvote the English ones, or the vote ties with Latin-accent evidence
/// present (ã/ç/é… — covers es/fr too, which share the accents and much of the
/// pt function-word inventory; the vendored romance stoplist today is pt).
fn is_non_english(seg: &str, en_stop: &BTreeSet<String>, pt_stop: &BTreeSet<String>) -> bool {
    let mut en_hits = 0usize;
    let mut pt_hits = 0usize;
    for word in seg.split(|c: char| !c.is_alphanumeric()) {
        if word.is_empty() {
            continue;
        }
        let w = word.to_lowercase();
        if en_stop.contains(&w) {
            en_hits += 1;
        }
        if pt_stop.contains(&w) {
            pt_hits += 1;
        }
    }
    let lower = seg.to_lowercase();
    let has_accent = fold(&lower) != lower;
    pt_hits > en_hits || (has_accent && pt_hits >= en_hits)
}

// ---------------------------------------------------------------------------
// The sidecar client — locate `mustard-translate`, spawn ONCE, stream the
// batch through stdin/stdout. Never panics; every failure returns None (the
// caller falls back to raw tokenization + counting).
// ---------------------------------------------------------------------------

/// Sidecar candidates, tried in order: bare name (the OS resolves it on PATH),
/// then the conventional cargo output dirs relative to the working directory.
fn sidecar_candidates() -> Vec<std::path::PathBuf> {
    let exe = format!("mustard-translate{}", std::env::consts::EXE_SUFFIX);
    vec![
        std::path::PathBuf::from(&exe),
        std::path::Path::new("target").join("release").join(&exe),
        std::path::Path::new("target").join("debug").join(&exe),
    ]
}

/// Translate `lines` in one `mustard-translate batch` run — first candidate
/// that spawns AND honours the 1-JSON-line-per-input-line contract wins.
fn sidecar_translate_batch(lines: &[String]) -> Option<Vec<SidecarLine>> {
    sidecar_candidates().iter().find_map(|cand| run_sidecar(cand, lines))
}

/// One spawn attempt: pipe every line to the child's stdin (from a writer
/// thread, so a full stdout pipe can never deadlock the exchange), read one
/// JSON line back per input line. Any mismatch/failure → None.
fn run_sidecar(program: &std::path::Path, lines: &[String]) -> Option<Vec<SidecarLine>> {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};
    let mut child = Command::new(program)
        .arg("batch")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdin = child.stdin.take()?;
    let mut payload = String::new();
    for l in lines {
        payload.push_str(l);
        payload.push('\n');
    }
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(payload.as_bytes());
        // stdin drops here — EOF tells the sidecar the batch is complete.
    });
    let stdout = child.stdout.take()?;
    let mut out: Vec<SidecarLine> = Vec::new();
    for line in BufReader::new(stdout).lines() {
        let Ok(l) = line else { break };
        match serde_json::from_str::<SidecarLine>(&l) {
            Ok(v) => out.push(v),
            Err(_) => break, // off-contract output → let the length check fail
        }
    }
    let _ = writer.join();
    let ok = child.wait().map(|s| s.success()).unwrap_or(false);
    (ok && out.len() == lines.len()).then_some(out)
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

/// Split ONE (already whitespace-normalized) comment segment into lowercased
/// domain tokens: non-alphanumeric boundaries, a 3-char floor, and at least one
/// alphabetic char (drops pure numbers) — the same shape
/// [`crate::digest::tokenize`] lands identifiers in.
fn segment_tokens(seg: &str) -> Vec<String> {
    seg.split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|w| w.len() >= 3 && w.chars().any(char::is_alphabetic))
        .collect()
}

/// Tokenize every comment of `src` in one flat list — a test convenience over
/// [`comment_segments`] + [`segment_tokens`].
#[cfg(test)]
fn comment_tokens(src: &str) -> Vec<String> {
    comment_segments(src).iter().flat_map(|s| segment_tokens(s)).collect()
}

/// Extract every comment/docstring of `src` as ONE SEGMENT PER COMMENT,
/// whitespace-normalized (inner newline/space runs collapse to single spaces —
/// the sidecar batch protocol is line-based, and MT wants the sentence whole),
/// empty segments dropped. Agnostic of language, by the common markers. A
/// linear byte scan (all markers are ASCII, so every cut is on a char
/// boundary — the sliced text may hold any UTF-8). Unclosed blocks run to
/// end-of-input; nothing panics.
fn comment_segments(src: &str) -> Vec<String> {
    const LINE2: &[u8] = b"//";
    const BLOCK_OPEN: &[u8] = b"/*";
    const BLOCK_CLOSE: &[u8] = b"*/";
    const HTML_OPEN: &[u8] = b"<!--";
    const HTML_CLOSE: &[u8] = b"-->";
    const DQ: &[u8] = b"\"\"\"";
    const SQ: &[u8] = b"'''";

    let bytes = src.as_bytes();
    let n = bytes.len();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < n {
        // Line comments (`//`, `#`): to end of line.
        if marker_at(bytes, i, LINE2) || bytes[i] == b'#' {
            let start = i + if bytes[i] == b'#' { 1 } else { LINE2.len() };
            let mut j = start;
            while j < n && bytes[j] != b'\n' {
                j += 1;
            }
            push_segment(&mut out, src, start, j);
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
            push_segment(&mut out, src, start, j.min(n));
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

/// Append `src[start..end]` as one whitespace-normalized segment (both offsets
/// are on char boundaries — they sit at ASCII markers/newline/end); a
/// blank-only comment contributes nothing.
fn push_segment(out: &mut Vec<String>, src: &str, start: usize, end: usize) {
    if let Some(slice) = src.get(start..end) {
        let norm = slice.split_whitespace().collect::<Vec<_>>().join(" ");
        if !norm.is_empty() {
            out.push(norm);
        }
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

    /// A deterministic stand-in for the sidecar: the PT fixtures translate to
    /// fixed English; anything else passes through (like `detected:"en"` would).
    fn fake_tr(lines: &[String]) -> Option<Vec<SidecarLine>> {
        Some(
            lines
                .iter()
                .map(|l| SidecarLine {
                    en: match l.as_str() {
                        "valida o contrato do parceiro" => "validates the partner contract".to_string(),
                        "atualiza o contrato existente" => "updates the existing contract".to_string(),
                        other => other.to_string(),
                    },
                    detected: "pt".to_string(),
                })
                .collect(),
        )
    }

    /// (f) EN NORMALIZATION — the non-English comment segments go to the batch
    /// translator ONCE, deduped + sorted; the TRANSLATION is what enters the
    /// vocabulary (the PT surface never does); nothing is counted untranslated.
    #[test]
    fn normalizes_non_english_comments_via_batch_translation() {
        let mut modules = vec![module("src/contracts/create.rs", &[]), module("src/contracts/update.rs", &[])];
        modules.extend(fillers(6));
        let mut content = HashMap::new();
        content.insert("src/contracts/create.rs".to_string(), "// valida o contrato do parceiro".to_string());
        content
            .insert("src/contracts/update.rs".to_string(), "// atualiza o contrato existente\n// keeps the audit note in english".to_string());

        let mut batches: Vec<Vec<String>> = Vec::new();
        let mut fake = |lines: &[String]| -> Option<Vec<SidecarLine>> {
            batches.push(lines.to_vec());
            fake_tr(lines)
        };
        let dict = build_with(&modules, &content, &[], Some(&mut fake));
        drop(fake);

        assert_eq!(batches.len(), 1, "the sidecar is spawned exactly once");
        assert_eq!(
            batches[0],
            vec!["atualiza o contrato existente".to_string(), "valida o contrato do parceiro".to_string()],
            "only the non-English segments, deduped and sorted (the English note stays home)"
        );
        let contract = find(&dict, "contract").expect("translated token mined");
        assert_eq!(contract.source, "comment");
        assert_eq!(contract.count, 2, "one occurrence per translated segment");
        assert!(find(&dict, "contrato").is_none(), "the raw PT token never enters: {:?}", terms(&dict));
        assert_eq!(dict.non_english_comments, 0, "fully normalized -> nothing left raw");
    }

    /// (g) FAIL-OPEN — no sidecar (`build`) and a failing batch behave
    /// IDENTICALLY: raw tokenization exactly as before, every non-English
    /// occurrence counted into `non_english_comments`.
    #[test]
    fn counts_untranslated_non_english_comments_when_sidecar_unavailable() {
        let mut modules = vec![module("src/a.rs", &[]), module("src/b.rs", &[])];
        modules.extend(fillers(6));
        let mut content = HashMap::new();
        content.insert("src/a.rs".to_string(), "// valida o contrato do parceiro".to_string());
        content.insert("src/b.rs".to_string(), "// atualiza o contrato existente\n// plain english comment of the module".to_string());

        let dict = build(&modules, &content, &[]);
        assert_eq!(dict.non_english_comments, 2, "both PT occurrences counted, the English one not");
        assert!(find(&dict, "contrato").is_some(), "raw fallback keeps today's vocabulary: {:?}", terms(&dict));

        let mut failing = |_lines: &[String]| -> Option<Vec<SidecarLine>> { None };
        let failed = build_with(&modules, &content, &[], Some(&mut failing));
        let (a, b) = (serde_json::to_string(&dict).expect("serialize"), serde_json::to_string(&failed).expect("serialize"));
        assert_eq!(a, b, "failed batch == no sidecar, byte for byte");
    }

    /// (h) BYTE-STABILITY under translation — two runs with the same
    /// (deterministic) translator serialize identically.
    #[test]
    fn normalized_build_is_byte_stable() {
        let mut modules = vec![module("src/contracts/create.rs", &[]), module("src/contracts/update.rs", &[])];
        modules.extend(fillers(6));
        let mut content = HashMap::new();
        content.insert("src/contracts/create.rs".to_string(), "// valida o contrato do parceiro".to_string());
        content.insert("src/contracts/update.rs".to_string(), "// atualiza o contrato existente".to_string());
        let (mut t1, mut t2) = (fake_tr, fake_tr);
        let one = serde_json::to_string(&build_with(&modules, &content, &[], Some(&mut t1))).expect("serialize");
        let two = serde_json::to_string(&build_with(&modules, &content, &[], Some(&mut t2))).expect("serialize");
        assert_eq!(one, two, "two normalized runs are byte-identical");
    }

    /// The routing heuristic: pt function words / accent evidence send a
    /// segment to translation; English stays home, even with a quoted accent.
    #[test]
    fn non_english_detection_votes_stoplists_and_accents() {
        let en = stoplist_words("en");
        let pt = stoplist_words("pt");
        assert!(is_non_english("valida o cadastro do parceiro antes de salvar", &en, &pt), "pt function words outvote");
        assert!(is_non_english("conciliação bancária", &en, &pt), "accent evidence breaks the 0-0 tie");
        assert!(!is_non_english("validates the partner registration before saving", &en, &pt));
        assert!(!is_non_english("maps the naïve café names into the user profile", &en, &pt), "English wins its vote despite an accented word");
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
