//! `mustard-rt run lexicon-suggest` — supervised promotion of confirmed
//! vocabulary bridges into the project lexicon overlay.
//!
//! ## Why
//!
//! The digest's match ladder (scan's `matching.rs`) is deliberately strict: a
//! cross-language domain equivalence only matches through the curated tier-4
//! lexicon. When a `feature` query reports a term as `none` and the
//! orchestrator re-queries in the code's own vocabulary (the documented
//! weak/none flow), that successful re-query IS empirical evidence of a
//! missing lexicon entry. This command folds the `feature.query` events of
//! the WHOLE workspace telemetry — every `.claude/.session/<id>/.events/`
//! and `.claude/spec/<name>/.events/` scope (emitted by
//! [`crate::commands::feature`]) — windowed to the [`QUERY_WINDOW`] most
//! recent rounds by `ts`, so the suggestion run does not have to share a
//! session with the research that produced the evidence (the typical flow:
//! the user asks for suggestions in a LATER session). Correlation stays
//! consecutive PER ORIGIN — the emitting session when the event carries one,
//! the telemetry directory otherwise — and, for each consecutive pair
//! (q1, q2) of one origin, turns every `none`-tier term of q1 crossed with
//! every NEW exact/fold/stem term of q2 into a candidate
//! `{missed, bridged, files}` — the files being the re-query's evidence.
//! Rounds of different origins never pair: two sessions researching the same
//! spec (or two unrelated specs) cannot mint a false bridge, while one
//! session's rounds still correlate across its own session scope and the
//! spec it later bound.
//!
//! ## Never auto-apply
//!
//! Mirrors [`crate::commands::spec::tactical_fix_detect`]'s
//! suggestion-without-apply contract: without `--accept` the command only
//! LISTS — no file is created or touched, even with candidates pending.
//! `--accept <missed>=<bridged>` writes ONE entry to the project overlay
//! `<root>/.claude/lexicons/<pair>.toml` (created from the template shape
//! when absent; inserted into the `[terms]` section in alphabetical order,
//! preserving existing comments). The embedded seed is NEVER written — it is
//! compiled into the scan tool; the overlay is the project's extension point.
//! Accepting an already-covered candidate is an idempotent no-op.
//!
//! ## Language pair + data single-source
//!
//! The pair is resolved exactly like the digest resolves its ladder: the root
//! `mustard.json` request language (`lang` wins over `specLang`, the same
//! precedence as scan's `request_lang`) reduced to its primary subtag, plus
//! the always-on `en` fallback. Only pairs with a vendored seed exist
//! (`pt-en` today — one data row, mirroring scan's `stemmers::lexicon`). The
//! seed and the overlay template are embedded from their single sources of
//! truth (`apps/scan/lexicons/`, `apps/cli/templates/lexicons/`) so the data
//! never forks; candidates are deduped by folded key against that effective
//! lexicon (seed + overlay, project wins per key).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use mustard_core::io::claude_paths::ClaudePaths;
use mustard_core::io::fs;
use mustard_core::view::projection::read_harness_events_from_ndjson_dir;
use serde_json::{json, Value};

/// The embedded seed for the `pt-en` pair — the SAME file the scan tool
/// embeds (single source of truth; never written by this command).
const SEED_PT_EN: &str = include_str!("../../../scan/lexicons/pt-en.toml");

/// The project-overlay template shape for the `pt-en` pair — the SAME file
/// `mustard init` ships under `apps/cli/templates/lexicons/`.
const OVERLAY_TEMPLATE_PT_EN: &str = include_str!("../../../cli/templates/lexicons/pt-en.toml");

/// The event `feature::run` records per answered digest query.
const EVENT_FEATURE_QUERY: &str = "feature.query";

/// Correlation window: only the most recent `QUERY_WINDOW` `feature.query`
/// events of the whole workspace telemetry — ordered by `ts`, across every
/// session and spec scope — are folded per run. The constant bounds cost and
/// drift deterministically (a fixed telemetry tree always folds the same
/// rows): stale research rounds age out as new ones land, with no wall-clock
/// cutoff that would make the listing time-dependent. 64 comfortably covers
/// several days of research (production showed ~5 rounds/day) while keeping
/// the fold cheap.
const QUERY_WINDOW: usize = 64;

/// Tiers that count as a CONFIRMED bridge in the re-query: real vocabulary
/// hits. `lexicon` is excluded on purpose — a lexicon-carried match means the
/// bridge already exists.
const BRIDGE_TIERS: [&str; 3] = ["exact", "fold", "stem"];

/// A vendored language pair: its label (the overlay file stem) plus the
/// embedded seed and template texts.
struct PairSeed {
    label: &'static str,
    seed: &'static str,
    template: &'static str,
}

/// One data row per vendored pair, mirroring scan's `stemmers::lexicon`
/// (order-insensitive). `None` = no curated pair: candidates cannot be deduped
/// against a seed and `--accept` has no overlay the digest would ever read.
fn seed_pair(a: &str, b: &str) -> Option<PairSeed> {
    match (a, b) {
        ("pt", "en") | ("en", "pt") => Some(PairSeed {
            label: "pt-en",
            seed: SEED_PT_EN,
            template: OVERLAY_TEMPLATE_PT_EN,
        }),
        _ => None,
    }
}

/// Resolve the active pair for `root` exactly like the digest resolves its
/// ladder: request language from the root config (`lang` wins over
/// `specLang`, scan's `request_lang` precedence), primary subtag + the `en`
/// fallback. An `en`/empty/unknown request language leaves no second
/// language, hence no pair.
fn pair_for_root(root: &Path) -> Option<PairSeed> {
    let cfg = mustard_core::ProjectConfig::load(root);
    let lang = cfg.lang.or(cfg.spec_lang).unwrap_or_default();
    let primary = primary_subtag(&lang);
    if primary.is_empty() || primary == "en" {
        return None;
    }
    seed_pair(&primary, "en")
}

/// Primary BCP-47 subtag, lowercased — same degradation as scan's matcher.
fn primary_subtag(raw: &str) -> String {
    raw.trim().to_lowercase().chars().take_while(|c| c.is_ascii_alphabetic()).collect()
}

/// Fold Latin diacritics to their ASCII base letter (input already
/// lowercased) — the folded-key convention every lexicon file uses. Pure
/// character table, mirroring scan's `matching::fold`.
fn fold(s: &str) -> String {
    s.chars()
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

/// Folded key for a raw term — the dedup identity everywhere in this module.
fn folded(term: &str) -> String {
    fold(&term.to_lowercase())
}

/// The `[terms]` table of a lexicon TOML, folded lowercase. Tolerant like
/// scan's `overlay_terms`: invalid TOML or a missing `[terms]` table yields
/// nothing, malformed entries are skipped individually — never an error.
fn terms_table(src: &str) -> BTreeMap<String, Vec<String>> {
    let Ok(v) = toml::from_str::<toml::Value>(src) else {
        return BTreeMap::new();
    };
    let Some(table) = v.get("terms").and_then(|t| t.as_table()) else {
        return BTreeMap::new();
    };
    table
        .iter()
        .filter_map(|(k, val)| {
            let syns: Vec<String> =
                val.as_array()?.iter().filter_map(|s| s.as_str()).map(folded).collect();
            (!syns.is_empty()).then(|| (folded(k), syns))
        })
        .collect()
}

/// The overlay file path: `<root>/.claude/lexicons/<label>.toml`.
fn overlay_path(root: &Path, label: &str) -> PathBuf {
    root.join(".claude").join("lexicons").join(format!("{label}.toml"))
}

/// The lexicon in force for `root`: the embedded seed merged with the
/// project overlay (project wins per key — the same merge `parse_lexicon`
/// applies in scan). Empty when no pair is vendored.
fn effective_lexicon(root: &Path, pair: Option<&PairSeed>) -> BTreeMap<String, Vec<String>> {
    let Some(p) = pair else {
        return BTreeMap::new();
    };
    let mut terms = terms_table(p.seed);
    if let Ok(raw) = fs::read_to_string(&overlay_path(root, p.label)) {
        for (k, v) in terms_table(&raw) {
            terms.insert(k, v);
        }
    }
    terms
}

/// Does the lexicon already bridge `a` and `b` (both folded)? An entry's key
/// and its synonyms are OR-equivalent (Pirkola semantics), so coverage means
/// one entry whose `{key} ∪ synonyms` contains both surfaces.
fn covers(lexicon: &BTreeMap<String, Vec<String>>, a: &str, b: &str) -> bool {
    lexicon.iter().any(|(key, syns)| {
        let in_entry = |w: &str| key == w || syns.iter().any(|s| s == w);
        in_entry(a) && in_entry(b)
    })
}

// --- feature.query fold ------------------------------------------------------

/// One per-term row of a recorded `feature.query` report.
#[derive(Debug, Clone)]
pub(crate) struct TermEntry {
    term: String,
    tier: String,
    files: Vec<String>,
}

/// One recorded `feature.query` round: the raw query terms + the report rows.
#[derive(Debug, Clone)]
pub(crate) struct QueryRecord {
    query_terms: Vec<String>,
    terms: Vec<TermEntry>,
}

/// A confirmed-bridge candidate: the term that missed in q1, the NEW term
/// that matched in q2, and the re-query's evidence files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Candidate {
    missed: String,
    bridged: String,
    files: Vec<String>,
}

/// Parse one `feature.query` payload into a [`QueryRecord`]. Tolerant: a
/// payload without a `report.terms` array is skipped (`None`).
fn query_record(payload: &Value) -> Option<QueryRecord> {
    let rows = payload.get("report")?.get("terms")?.as_array()?;
    let terms = rows
        .iter()
        .filter_map(|t| {
            let term = t.get("term").and_then(Value::as_str)?.to_string();
            let tier = t.get("tier").and_then(Value::as_str).unwrap_or("").to_string();
            let files = t
                .get("files")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default();
            Some(TermEntry { term, tier, files })
        })
        .collect();
    let query_terms = payload
        .get("queryTerms")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    Some(QueryRecord { query_terms, terms })
}

/// Deterministic fold over the ordered query rounds: for each consecutive
/// pair (q1, q2), terms X of q1 with tier `none` × terms Y of q2 that are NEW
/// (not queried in q1, by folded key) and matched at a real-vocabulary tier
/// become candidates. Pure; chronological discovery order is preserved.
fn correlate(queries: &[QueryRecord]) -> Vec<Candidate> {
    let mut out = Vec::new();
    for w in queries.windows(2) {
        let (q1, q2) = (&w[0], &w[1]);
        let known: BTreeSet<String> = q1
            .terms
            .iter()
            .map(|t| folded(&t.term))
            .chain(q1.query_terms.iter().map(|t| folded(t)))
            .collect();
        let missed: Vec<&TermEntry> = q1.terms.iter().filter(|t| t.tier == "none").collect();
        let bridged: Vec<&TermEntry> = q2
            .terms
            .iter()
            .filter(|t| BRIDGE_TIERS.contains(&t.tier.as_str()) && !known.contains(&folded(&t.term)))
            .collect();
        for x in &missed {
            for y in &bridged {
                out.push(Candidate {
                    missed: folded(&x.term),
                    bridged: folded(&y.term),
                    files: y.files.clone(),
                });
            }
        }
    }
    out
}

/// Dedup candidates by folded `(missed, bridged)` key: identical earlier
/// candidates win (first occurrence kept) and pairs the lexicon in force
/// already bridges are dropped.
fn dedup(candidates: Vec<Candidate>, lexicon: &BTreeMap<String, Vec<String>>) -> Vec<Candidate> {
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    candidates
        .into_iter()
        .filter(|c| {
            seen.insert((c.missed.clone(), c.bridged.clone()))
                && !covers(lexicon, &c.missed, &c.bridged)
        })
        .collect()
}

/// Every telemetry `.events/` directory of the workspace, labelled by scope:
/// the per-session ones (`.claude/.session/<id>/.events`) and the per-spec
/// ones (`.claude/spec/<name>/.events`). Enumerated in sorted name order so
/// the fold stays byte-stable regardless of OS readdir order. Fail-open: a
/// missing base directory contributes nothing.
fn telemetry_event_dirs(claude_dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for (label, base) in
        [("session", claude_dir.join(".session")), ("spec", claude_dir.join("spec"))]
    {
        let Ok(mut entries) = fs::read_dir(&base) else {
            continue;
        };
        entries.sort_by(|a, b| a.file_name.cmp(&b.file_name));
        for e in entries.into_iter().filter(|e| e.is_dir) {
            out.push((format!("{label}:{}", e.file_name), e.path.join(".events")));
        }
    }
    out
}

/// Collect the `feature.query` rounds of the WHOLE workspace telemetry —
/// every session and spec scope — windowed to the [`QUERY_WINDOW`] most
/// recent by `ts`, each labelled with the ORIGIN that groups it for
/// correlation: the emitting `session_id` when the event carries one, the
/// telemetry directory it was read from otherwise. No "active session/spec"
/// filter: the suggestion run is typically a LATER session than the research
/// that produced the evidence, and a strict filter made it see zero rounds
/// (the confirmed "filtro de sessão tolerante" review concern).
fn collect_queries(root: &Path) -> Vec<(String, QueryRecord)> {
    let claude_dir = ClaudePaths::for_project(root)
        .map(|p| p.claude_dir().clone())
        .unwrap_or_else(|_| ClaudePaths::compose_unchecked(root).claude_dir().clone());
    // (ts, origin, record) rows from every scope, then ONE global ts sort
    // (origin as tiebreak) and the trailing window. Sorted dir enumeration +
    // the explicit tiebreak keep the result deterministic.
    let mut rows: Vec<(String, String, QueryRecord)> = Vec::new();
    for (dir_origin, dir) in telemetry_event_dirs(&claude_dir) {
        for e in read_harness_events_from_ndjson_dir(&dir) {
            if e.event != EVENT_FEATURE_QUERY {
                continue;
            }
            let Some(rec) = query_record(&e.payload) else {
                continue;
            };
            let origin = if e.session_id.is_empty() || e.session_id == "unknown" {
                dir_origin.clone()
            } else {
                // Keyed by the emitting session, NOT the directory: one
                // session's ANALYZE-time rounds (session scope) and its
                // post-PLAN rounds (bound-spec scope) stay one origin.
                format!("session:{}", e.session_id)
            };
            rows.push((e.ts, origin, rec));
        }
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    let cut = rows.len().saturating_sub(QUERY_WINDOW);
    rows.drain(..cut);
    rows.into_iter().map(|(_, origin, rec)| (origin, rec)).collect()
}

/// Correlate the windowed rounds origin by origin: group by origin label
/// (`BTreeMap` — deterministic order), then run the consecutive-pair fold
/// within each group. Rounds of different origins never pair, so contexts
/// cannot mint a bridge across each other.
fn correlate_by_origin(rows: &[(String, QueryRecord)]) -> Vec<Candidate> {
    let mut groups: BTreeMap<&str, Vec<QueryRecord>> = BTreeMap::new();
    for (origin, rec) in rows {
        groups.entry(origin.as_str()).or_default().push(rec.clone());
    }
    groups.values().flat_map(|qs| correlate(qs)).collect()
}

// --- reports ------------------------------------------------------------------

/// List mode: candidates only, NEVER a write — even with candidates pending
/// (the tactical-fix-detect "suggest, do not apply" contract).
fn list_report(root: &Path) -> Value {
    let pair = pair_for_root(root);
    let lexicon = effective_lexicon(root, pair.as_ref());
    let rows = collect_queries(root);
    let candidates = dedup(correlate_by_origin(&rows), &lexicon);
    json!({
        "pair": pair.as_ref().map(|p| p.label),
        // Rounds actually folded (workspace-wide, already windowed).
        "queries": rows.len(),
        // The documented determinism bound: at most this many of the
        // workspace's most recent feature.query events are eligible per run.
        "window": QUERY_WINDOW,
        "candidates": candidates.iter().map(|c| json!({
            "missed": c.missed, "bridged": c.bridged, "files": c.files,
        })).collect::<Vec<_>>(),
        // Explicit: listing applies nothing — acceptance is a separate,
        // user-confirmed `--accept` invocation.
        "applied": false,
    })
}

/// Accept mode: record `<missed>=<bridged>` in the project overlay. Already
/// covered (seed or overlay) → idempotent no-op, nothing touched. The
/// embedded seed is never a write target — only
/// `<root>/.claude/lexicons/<pair>.toml`.
fn accept_report(root: &Path, arg: &str) -> Value {
    let parsed = arg.split_once('=').map(|(m, b)| (folded(m.trim()), folded(b.trim())));
    let Some((missed, bridged)) = parsed.filter(|(m, b)| !m.is_empty() && !b.is_empty()) else {
        return json!({ "accepted": false, "reason": "expected --accept <missed>=<bridged>" });
    };
    let Some(pair) = pair_for_root(root) else {
        // No vendored pair for the root's request language: an overlay file
        // would never be read by the digest's ladder, so refuse honestly.
        return json!({ "accepted": false, "reason": "no-lexicon-pair" });
    };
    let rel_path = format!(".claude/lexicons/{}.toml", pair.label);
    let lexicon = effective_lexicon(root, Some(&pair));
    if covers(&lexicon, &missed, &bridged) {
        return json!({
            "accepted": true, "changed": false, "reason": "already-covered",
            "missed": missed, "bridged": bridged, "pair": pair.label, "path": rel_path,
        });
    }
    let path = overlay_path(root, pair.label);
    let text = fs::read_to_string(&path).unwrap_or_else(|_| pair.template.to_string());
    // Overlay entries REPLACE the seed's synonyms per key, so an entry being
    // promoted for a seed key must carry the seed's synonyms forward too —
    // otherwise accepting one bridge would silently drop the curated ones.
    let mut syns: Vec<String> = terms_table(&text)
        .remove(&missed)
        .or_else(|| terms_table(pair.seed).remove(&missed))
        .unwrap_or_default();
    if !syns.contains(&bridged) {
        syns.push(bridged.clone());
    }
    let new_text = upsert_term(&text, &missed, &syns);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if fs::write_atomic(&path, new_text.as_bytes()).is_err() {
        return json!({ "accepted": false, "reason": "overlay-write-failed", "path": rel_path });
    }
    json!({
        "accepted": true, "changed": true,
        "missed": missed, "bridged": bridged, "pair": pair.label, "path": rel_path,
    })
}

/// Bare TOML key of an entry line inside `[terms]`; `None` for comments,
/// blanks, table headers and anything not shaped like `key = ...`.
fn entry_key(line: &str) -> Option<String> {
    let t = line.trim_start();
    if t.is_empty() || t.starts_with('#') || t.starts_with('[') {
        return None;
    }
    let (k, _) = t.split_once('=')?;
    let k = k.trim();
    (!k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .then(|| k.to_string())
}

/// Insert (or rewrite in place) `key = [syns…]` inside the `[terms]` section,
/// keeping every existing line — comments included — untouched. A new entry
/// lands at the alphabetically-correct position among the existing entry
/// lines; with no entries yet it appends at the end of the section (after the
/// template's commented examples). Output always ends with a newline.
fn upsert_term(text: &str, key: &str, syns: &[String]) -> String {
    let rendered = format!(
        "{key} = [{}]",
        syns.iter().map(|s| format!("\"{s}\"")).collect::<Vec<_>>().join(", ")
    );
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let Some(header) = lines.iter().position(|l| l.trim() == "[terms]") else {
        // Degenerate hand-edited overlay without the table: append one so the
        // accept still lands somewhere the digest can read (fail-open).
        lines.push(String::new());
        lines.push("[terms]".to_string());
        lines.push(rendered);
        return lines.join("\n") + "\n";
    };
    let end = lines
        .iter()
        .enumerate()
        .skip(header + 1)
        .find(|(_, l)| l.trim_start().starts_with('['))
        .map_or(lines.len(), |(i, _)| i);
    for line in lines.iter_mut().take(end).skip(header + 1) {
        if entry_key(line).is_some_and(|k| k == key) {
            *line = rendered;
            return lines.join("\n") + "\n";
        }
    }
    let insert_at = (header + 1..end)
        .find(|&i| entry_key(&lines[i]).is_some_and(|k| k.as_str() > key))
        .unwrap_or(end);
    lines.insert(insert_at, rendered);
    lines.join("\n") + "\n"
}

/// Dispatch `mustard-rt run lexicon-suggest [--accept <missed>=<bridged>] [--root <dir>]`.
///
/// Without `--accept`: byte-stable JSON listing of the candidates (read-only —
/// nothing on disk is created or modified). With `--accept`: records one entry
/// in the project lexicon overlay, never the embedded seed.
pub fn run(accept: Option<&str>, root: &Path) {
    let root = if root == Path::new(".") {
        PathBuf::from(crate::shared::context::project_dir())
    } else {
        root.to_path_buf()
    };
    let report = match accept {
        Some(arg) => accept_report(&root, arg),
        // No session/spec resolution: the listing folds the whole workspace
        // telemetry (windowed), so it works from ANY session — including
        // none at all (the typical "suggest later" invocation).
        None => list_report(&root),
    };
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::events::writer_ndjson::{write_event, write_event_with_ts};
    use tempfile::tempdir;

    /// Build a `feature.query` payload the way `feature::query_event_payload`
    /// shapes it: queryTerms + report rows of (term, tier, files).
    fn query_payload(query_terms: &[&str], rows: &[(&str, &str, &[&str])]) -> Value {
        json!({
            "queryTerms": query_terms,
            "report": {
                "matched": rows.iter().filter(|(_, tier, _)| *tier != "none").count(),
                "total": rows.len(),
                "reason": "weak",
                "terms": rows.iter().map(|(term, tier, files)| json!({
                    "term": term, "tier": tier, "lang": "", "files": files,
                })).collect::<Vec<_>>(),
            },
        })
    }

    fn record(payload: Value) -> QueryRecord {
        query_record(&payload).expect("valid feature.query payload")
    }

    fn write_root_config(root: &Path) {
        std::fs::write(root.join("mustard.json"), br#"{"specLang":"pt-BR"}"#).unwrap();
    }

    // -- correlation ---------------------------------------------------------

    #[test]
    fn lexicon_correlation_requery_bridges_none_to_new_term() {
        // q1: "hierarquia" missed (tier none); q2 re-queries in the code's
        // vocabulary and the NEW term "parent" matches exact — the pair plus
        // the re-query's evidence files become the candidate.
        let q1 = record(query_payload(
            &["hierarquia", "titulo"],
            &[("hierarquia", "none", &[]), ("titulo", "exact", &["src/title.cs"])],
        ));
        let q2 = record(query_payload(
            &["parent", "titulo"],
            &[
                ("parent", "exact", &["src/tree.cs", "src/node.cs"]),
                // Already queried in q1 → NOT new → never a bridge.
                ("titulo", "exact", &["src/title.cs"]),
            ],
        ));
        let got = correlate(&[q1, q2]);
        assert_eq!(got.len(), 1, "one missed × one NEW bridged term: {got:?}");
        assert_eq!(got[0].missed, "hierarquia");
        assert_eq!(got[0].bridged, "parent");
        assert_eq!(got[0].files, vec!["src/tree.cs", "src/node.cs"], "evidence files carried");
    }

    #[test]
    fn lexicon_correlation_without_requery_yields_no_candidates() {
        // A single query — nothing to correlate against.
        let only = record(query_payload(&["hierarquia"], &[("hierarquia", "none", &[])]));
        assert!(correlate(&[only.clone()]).is_empty(), "no consecutive pair, no candidates");

        // A re-query whose new term matched only through the LEXICON tier is
        // not a confirmed raw-vocabulary bridge either.
        let q2 = record(query_payload(&["cancelado"], &[("cancelado", "lexicon", &["src/c.cs"])]));
        assert!(correlate(&[only, q2]).is_empty(), "lexicon-tier hits are not new bridges");
    }

    #[test]
    fn lexicon_correlation_dedups_against_current_lexicon_and_self() {
        let dir = tempdir().unwrap();
        write_root_config(dir.path());
        // Overlay already bridges titulo→payable (project entry).
        let lexdir = dir.path().join(".claude").join("lexicons");
        std::fs::create_dir_all(&lexdir).unwrap();
        std::fs::write(lexdir.join("pt-en.toml"), "[terms]\ntitulo = [\"payable\"]\n").unwrap();
        let pair = pair_for_root(dir.path()).expect("pt-BR root resolves the pt-en pair");
        let lexicon = effective_lexicon(dir.path(), Some(&pair));

        let candidates = vec![
            // Seed-covered: cancelar = ["cancel"] ships in the embedded seed.
            Candidate { missed: "cancelar".into(), bridged: "cancel".into(), files: vec![] },
            // Overlay-covered.
            Candidate { missed: "titulo".into(), bridged: "payable".into(), files: vec![] },
            // Genuinely new — survives.
            Candidate { missed: "apolice".into(), bridged: "policy".into(), files: vec![] },
            // Identical to an earlier candidate — dropped by the seen-set.
            Candidate { missed: "apolice".into(), bridged: "policy".into(), files: vec![] },
        ];
        let got = dedup(candidates, &lexicon);
        assert_eq!(got.len(), 1, "covered + duplicate candidates are dropped: {got:?}");
        assert_eq!(got[0].missed, "apolice");
        assert_eq!(got[0].bridged, "policy");
    }

    // -- accept ----------------------------------------------------------------

    #[test]
    fn lexicon_accept_writes_overlay_alphabetically_never_seed() {
        let dir = tempdir().unwrap();
        write_root_config(dir.path());

        // Out-of-order accepts; the file must stay alphabetical.
        let first = accept_report(dir.path(), "sinistro=claim");
        assert_eq!(first["accepted"], true);
        assert_eq!(first["changed"], true);
        assert_eq!(first["pair"], "pt-en");
        assert_eq!(first["path"], ".claude/lexicons/pt-en.toml");
        let second = accept_report(dir.path(), "apolice=policy");
        assert_eq!(second["changed"], true);

        let overlay = overlay_path(dir.path(), "pt-en");
        let text = std::fs::read_to_string(&overlay).expect("overlay created from template");
        // Template shape preserved: header comment + [terms] + commented examples.
        assert!(text.starts_with("# PROJECT domain lexicon"), "template header kept: {text}");
        assert!(text.contains("# titulo = [\"payable\", \"receivable\"]"), "template comments kept");
        let keys: Vec<String> = text.lines().filter_map(entry_key).collect();
        assert_eq!(keys, vec!["apolice", "sinistro"], "entries in alphabetical order");
        assert!(text.contains("apolice = [\"policy\"]"));
        assert!(text.contains("sinistro = [\"claim\"]"));
        assert!(text.ends_with('\n'));

        // The ONLY file ever written is the project overlay — nothing else
        // appears under `.claude/lexicons/` (the seed is embedded, not a path).
        let entries: Vec<_> = std::fs::read_dir(overlay.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(entries, vec!["pt-en.toml"]);
    }

    #[test]
    fn lexicon_accept_preserves_seed_synonyms_when_overriding_a_seed_key() {
        // Overlay entries REPLACE the seed per key, so promoting a new bridge
        // for a seed key must carry the seed's synonyms forward.
        let dir = tempdir().unwrap();
        write_root_config(dir.path());
        let report = accept_report(dir.path(), "cancelar=abort");
        assert_eq!(report["changed"], true);
        let text = std::fs::read_to_string(overlay_path(dir.path(), "pt-en")).unwrap();
        assert!(
            text.contains("cancelar = [\"cancel\", \"abort\"]"),
            "seed synonym kept + new bridge appended: {text}"
        );
    }

    #[test]
    fn lexicon_accept_already_covered_is_idempotent_noop() {
        let dir = tempdir().unwrap();
        write_root_config(dir.path());

        // Seed-covered pair: no overlay file is even created.
        let noop = accept_report(dir.path(), "estorno=refund");
        assert_eq!(noop["accepted"], true);
        assert_eq!(noop["changed"], false);
        assert_eq!(noop["reason"], "already-covered");
        assert!(!overlay_path(dir.path(), "pt-en").exists(), "no-op must not create the overlay");

        // A real accept, then the same accept again: bytes are identical.
        assert_eq!(accept_report(dir.path(), "apolice=policy")["changed"], true);
        let before = std::fs::read(overlay_path(dir.path(), "pt-en")).unwrap();
        let again = accept_report(dir.path(), "apolice=policy");
        assert_eq!(again["changed"], false, "re-accept is a no-op: {again}");
        let after = std::fs::read(overlay_path(dir.path(), "pt-en")).unwrap();
        assert_eq!(before, after, "idempotent: file bytes unchanged");
    }

    #[test]
    fn lexicon_accept_refuses_without_a_vendored_pair() {
        // An `en` root has no second language → no pair → honest refusal,
        // and nothing is written anywhere.
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"), br#"{"specLang":"en-US"}"#).unwrap();
        let report = accept_report(dir.path(), "apolice=policy");
        assert_eq!(report["accepted"], false);
        assert_eq!(report["reason"], "no-lexicon-pair");
        assert!(!dir.path().join(".claude").join("lexicons").exists());
    }

    // -- never auto-apply --------------------------------------------------------

    #[test]
    fn lexicon_no_auto_listing_writes_nothing_even_with_candidates_pending() {
        let dir = tempdir().unwrap();
        write_root_config(dir.path());
        let spec = "lex-spec";
        // Two recorded feature.query rounds that DO yield a candidate — a
        // pair NOT covered by the embedded seed (e.g. `hierarquia=parent`
        // would be seed-covered and rightly deduped away).
        let q1 = query_payload(&["apolice"], &[("apolice", "none", &[])]);
        let q2 = query_payload(&["policy"], &[("policy", "exact", &["src/policy.cs"])]);
        for payload in [&q1, &q2] {
            let _ = write_event(
                dir.path(), Some(spec), None, "s", EVENT_FEATURE_QUERY, "other",
                Some(0), Some("s"), Some("feature"), None, payload,
            );
        }

        let report = list_report(dir.path());
        assert_eq!(report["applied"], false);
        assert_eq!(report["queries"], 2);
        let candidates = report["candidates"].as_array().expect("candidates array");
        assert_eq!(candidates.len(), 1, "the pending candidate is listed: {report}");
        assert_eq!(candidates[0]["missed"], "apolice");
        assert_eq!(candidates[0]["bridged"], "policy");
        assert_eq!(candidates[0]["files"], json!(["src/policy.cs"]));

        // The invariant: listing NEVER touches the filesystem — no overlay,
        // no lexicons dir, even with candidates pending.
        assert!(
            !dir.path().join(".claude").join("lexicons").exists(),
            "list mode must not create .claude/lexicons/"
        );

        // Byte-stable across runs.
        let a = serde_json::to_string(&report).unwrap();
        let b = serde_json::to_string(&list_report(dir.path())).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn lexicon_suggest_never_cross_correlates_distinct_origins() {
        // Two DIFFERENT sessions researching the same spec: both rounds are
        // visible to the workspace-wide fold (queries=2 — no strict session
        // filter), but they are distinct origins, so the none-tier round of
        // s1 never pairs with s2's exact hit — no false bridge.
        let dir = tempdir().unwrap();
        write_root_config(dir.path());
        let spec = "lex-cross";
        let q1 = query_payload(&["hierarquia"], &[("hierarquia", "none", &[])]);
        let q2 = query_payload(&["parent"], &[("parent", "exact", &["src/tree.cs"])]);
        let _ = write_event_with_ts(
            dir.path(), Some(spec), None, "s1", EVENT_FEATURE_QUERY, "other",
            Some(0), Some("s1"), Some("feature"), None, &q1,
            Some("2026-06-10T10:00:00.000Z"),
        );
        let _ = write_event_with_ts(
            dir.path(), Some(spec), None, "s2", EVENT_FEATURE_QUERY, "other",
            Some(0), Some("s2"), Some("feature"), None, &q2,
            Some("2026-06-10T10:01:00.000Z"),
        );
        let report = list_report(dir.path());
        assert_eq!(report["queries"], 2, "both origins' rounds are folded: {report}");
        assert_eq!(report["candidates"], json!([]), "origins never cross-pair: {report}");
    }

    #[test]
    fn lexicon_suggest_correlates_rounds_from_other_sessions_and_specs() {
        // The production regression: the research happened in OTHER sessions
        // — one pair in a session scope (`.session/sA/.events`, ANALYZE-time)
        // and one pair in a spec scope from a different session — and the
        // listing runs with NO session/spec context of its own. The
        // workspace-wide fold must surface BOTH bridges (the old
        // active-session filter saw zero).
        let dir = tempdir().unwrap();
        write_root_config(dir.path());
        // Session-scope pair from sA: apolice → policy.
        let a1 = query_payload(&["apolice"], &[("apolice", "none", &[])]);
        let a2 = query_payload(&["policy"], &[("policy", "exact", &["src/policy.cs"])]);
        let _ = write_event_with_ts(
            dir.path(), None, None, "sA", EVENT_FEATURE_QUERY, "other",
            Some(0), Some("sA"), Some("feature"), None, &a1,
            Some("2026-06-10T08:00:00.000Z"),
        );
        let _ = write_event_with_ts(
            dir.path(), None, None, "sA", EVENT_FEATURE_QUERY, "other",
            Some(0), Some("sA"), Some("feature"), None, &a2,
            Some("2026-06-10T08:05:00.000Z"),
        );
        // Spec-scope pair from sB: sinistro → claim.
        let b1 = query_payload(&["sinistro"], &[("sinistro", "none", &[])]);
        let b2 = query_payload(&["claim"], &[("claim", "exact", &["src/claim.cs"])]);
        let _ = write_event_with_ts(
            dir.path(), Some("lex-other-spec"), None, "sB", EVENT_FEATURE_QUERY, "other",
            Some(0), Some("sB"), Some("feature"), None, &b1,
            Some("2026-06-10T09:00:00.000Z"),
        );
        let _ = write_event_with_ts(
            dir.path(), Some("lex-other-spec"), None, "sB", EVENT_FEATURE_QUERY, "other",
            Some(0), Some("sB"), Some("feature"), None, &b2,
            Some("2026-06-10T09:05:00.000Z"),
        );

        let report = list_report(dir.path());
        assert_eq!(report["queries"], 4, "all four workspace rounds folded: {report}");
        let candidates = report["candidates"].as_array().expect("candidates array");
        assert_eq!(candidates.len(), 2, "one bridge per origin: {report}");
        // Deterministic origin order (BTreeMap: session:sA < session:sB).
        assert_eq!(candidates[0]["missed"], "apolice");
        assert_eq!(candidates[0]["bridged"], "policy");
        assert_eq!(candidates[1]["missed"], "sinistro");
        assert_eq!(candidates[1]["bridged"], "claim");
        assert_eq!(candidates[1]["files"], json!(["src/claim.cs"]), "evidence carried");
        // Byte-stable across runs.
        let a = serde_json::to_string(&report).unwrap();
        let b = serde_json::to_string(&list_report(dir.path())).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn lexicon_suggest_same_session_still_correlates_across_scopes() {
        // One session's ANALYZE-time miss (session scope, no spec bound yet)
        // followed by its post-PLAN re-query (bound-spec scope) is ONE origin
        // — keyed by the emitting session, not the directory — so the bridge
        // is still confirmed across the scope move.
        let dir = tempdir().unwrap();
        write_root_config(dir.path());
        let q1 = query_payload(&["apolice"], &[("apolice", "none", &[])]);
        let q2 = query_payload(&["policy"], &[("policy", "exact", &["src/policy.cs"])]);
        let _ = write_event_with_ts(
            dir.path(), None, None, "sC", EVENT_FEATURE_QUERY, "other",
            Some(0), Some("sC"), Some("feature"), None, &q1,
            Some("2026-06-10T11:00:00.000Z"),
        );
        let _ = write_event_with_ts(
            dir.path(), Some("lex-bind"), None, "sC", EVENT_FEATURE_QUERY, "other",
            Some(0), Some("sC"), Some("feature"), None, &q2,
            Some("2026-06-10T11:10:00.000Z"),
        );
        let report = list_report(dir.path());
        assert_eq!(report["queries"], 2, "{report}");
        let candidates = report["candidates"].as_array().expect("candidates array");
        assert_eq!(candidates.len(), 1, "scope move does not break the origin: {report}");
        assert_eq!(candidates[0]["missed"], "apolice");
        assert_eq!(candidates[0]["bridged"], "policy");
    }

    #[test]
    fn lexicon_suggest_window_caps_the_fold_to_recent_events() {
        // An old confirmed pair is a candidate while inside the window…
        let dir = tempdir().unwrap();
        write_root_config(dir.path());
        let q1 = query_payload(&["apolice"], &[("apolice", "none", &[])]);
        let q2 = query_payload(&["policy"], &[("policy", "exact", &["src/policy.cs"])]);
        let _ = write_event_with_ts(
            dir.path(), None, None, "sOld", EVENT_FEATURE_QUERY, "other",
            Some(0), Some("sOld"), Some("feature"), None, &q1,
            Some("2026-06-01T00:00:00.000Z"),
        );
        let _ = write_event_with_ts(
            dir.path(), None, None, "sOld", EVENT_FEATURE_QUERY, "other",
            Some(0), Some("sOld"), Some("feature"), None, &q2,
            Some("2026-06-01T00:01:00.000Z"),
        );
        let before = list_report(dir.path());
        assert_eq!(before["queries"], 2);
        assert_eq!(before["window"], QUERY_WINDOW as u64);
        assert_eq!(
            before["candidates"].as_array().map(Vec::len),
            Some(1),
            "inside the window the old pair is a candidate: {before}"
        );

        // …and ages out once QUERY_WINDOW newer rounds land: only the most
        // recent QUERY_WINDOW events stay eligible, deterministically by ts.
        let filler = query_payload(&["titulo"], &[("titulo", "exact", &["src/title.cs"])]);
        for i in 0..QUERY_WINDOW {
            let ts = format!("2026-06-02T01:{:02}:{:02}.000Z", i / 60, i % 60);
            let _ = write_event_with_ts(
                dir.path(), None, None, "sNew", EVENT_FEATURE_QUERY, "other",
                Some(0), Some("sNew"), Some("feature"), None, &filler,
                Some(&ts),
            );
        }
        let after = list_report(dir.path());
        assert_eq!(
            after["queries"],
            QUERY_WINDOW as u64,
            "the fold is capped at the window: {after}"
        );
        assert_eq!(
            after["candidates"],
            json!([]),
            "the aged-out pair stops producing candidates: {after}"
        );
    }

    // -- upsert mechanics ----------------------------------------------------------

    #[test]
    fn upsert_term_inserts_alphabetically_and_preserves_comments() {
        let base = "# header comment\n\n[terms]\n# example = [\"kept\"]\nbanana = [\"yellow\"]\n";
        let one = upsert_term(base, "abacate", &["avocado".to_string()]);
        let keys: Vec<String> = one.lines().filter_map(entry_key).collect();
        assert_eq!(keys, vec!["abacate", "banana"]);
        assert!(one.contains("# header comment"));
        assert!(one.contains("# example = [\"kept\"]"));

        // Rewrite-in-place for an existing key keeps position + neighbours.
        let two = upsert_term(&one, "banana", &["yellow".to_string(), "plantain".to_string()]);
        assert!(two.contains("banana = [\"yellow\", \"plantain\"]"));
        let keys: Vec<String> = two.lines().filter_map(entry_key).collect();
        assert_eq!(keys, vec!["abacate", "banana"]);
    }
}
