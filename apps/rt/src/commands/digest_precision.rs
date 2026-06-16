//! `mustard-rt run digest-precision` — the METRIC half of the digest-outcome
//! loop: a deterministic projection that folds `feature.query` (the SUGGESTED
//! anchors) × `feature.outcome` (the OBSERVED reads) into a recall / precision
//! report.
//!
//! ## What it answers
//!
//! After the digest pointed at a set of files, did the orchestrator open them?
//!
//! - **recall** — of the anchors the digest SUGGESTED, how many were read?
//!   (`anchors read / anchors suggested`). High recall ⇒ the digest pointed at
//!   the files the orchestrator needed.
//! - **precision** — of the files read IN A RESEARCH WINDOW, how many were
//!   suggested? (`reads-that-were-anchors / reads-total-in-window`).
//!   Precision < 1 ⇒ the orchestrator had to go beyond the digest.
//! - **perTerm** — per query term, how many reads it led to (a term that never
//!   leads to a read is low-value noise — the Onda-3 feedback signal, if ever
//!   built).
//!
//! This is the CRITERION OF STOP for the locator redesign: plot it over time;
//! when it flattens, stop redesigning.
//!
//! ## Determinism (byte-stability)
//!
//! The run-face contract is a byte-stable stdout (snapshots + gates compare
//! it). So: NO floats — ratios are fixed-point per-mille (`*1000`, integer
//! `num*1000/den`); NO timestamps or volatile paths in the output; every list
//! (perTerm, the anchor/read sets feeding the counts) is sorted. The same
//! event tree always folds to the same bytes.
//!
//! ## Reuse
//!
//! The event reader is the SAME merged session+spec walk
//! [`crate::commands::agent::digest_adherence_finalize`] uses
//! (`read_harness_events_from_ndjson_dir` over `.session/<id>/.events/` +
//! `.claude/spec/<spec>/**/.events/`, session-filtered) — the projection reads
//! events, never the repo. Fail-open: no events ⇒ a zero report, exit 0.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use mustard_core::domain::model::event::HarnessEvent;
use mustard_core::io::claude_paths::ClaudePaths;
use mustard_core::io::fs;
use mustard_core::view::projection::read_harness_events_from_ndjson_dir;
use serde_json::{json, Value};

/// The SUGGESTED-set event `feature::run` records per research round.
const EVENT_FEATURE_QUERY: &str = "feature.query";

/// The OBSERVED-set event `feature_outcome_observer` emits per touched file
/// inside a research window.
const EVENT_FEATURE_OUTCOME: &str = "feature.outcome";

/// Fixed-point scale for the ratios — per-mille (3 significant digits) keeps
/// the metric legible while staying integer (byte-stable, no float drift).
const SCALE: i64 = 1000;

/// Integer ratio `num/den` scaled by [`SCALE`], rounded to nearest. `den == 0`
/// ⇒ 0 (an empty denominator is "no signal", not infinity).
fn ratio_x1000(num: usize, den: usize) -> i64 {
    if den == 0 {
        return 0;
    }
    // Round-to-nearest in integer arithmetic: (num*SCALE + den/2) / den.
    let n = num as i64 * SCALE + (den as i64) / 2;
    n / den as i64
}

/// The normalised anchor/read file key — forward-slash, lowercase. Both the
/// `feature.query` per-term files and the `feature.outcome` `file` are mapped
/// through this so an absolute touch path and a repo-relative anchor compare
/// equal by basename / suffix at the matching step.
fn norm(path: &str) -> String {
    path.replace('\\', "/").to_ascii_lowercase()
}

/// The basename of an already-normalised path.
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The folded view of all `feature.query` events: the SUGGESTED anchor set
/// (by basename — the stable identity across absolute/relative forms) and the
/// per-term suggested files, plus the count of queries folded.
#[derive(Default)]
struct Suggested {
    queries: usize,
    /// Every suggested anchor, keyed by basename → its normalised full forms.
    anchors: BTreeSet<String>,
    /// term → the set of anchor basenames it suggested.
    per_term: BTreeMap<String, BTreeSet<String>>,
}

/// Fold the `feature.query` events into the suggested set. Each event's
/// `report.terms[].files` are the anchors; the term that lists a file is the
/// term credited for it.
fn fold_suggested(events: &[HarnessEvent]) -> Suggested {
    let mut s = Suggested::default();
    for e in events.iter().filter(|e| e.event == EVENT_FEATURE_QUERY) {
        s.queries += 1;
        let Some(terms) = e.payload.get("report").and_then(|r| r.get("terms")).and_then(Value::as_array)
        else {
            continue;
        };
        for t in terms {
            let Some(term) = t.get("term").and_then(Value::as_str) else {
                continue;
            };
            let files = t.get("files").and_then(Value::as_array);
            let Some(files) = files else { continue };
            for f in files.iter().filter_map(Value::as_str) {
                let base = basename(&norm(f)).to_string();
                if base.is_empty() {
                    continue;
                }
                s.anchors.insert(base.clone());
                s.per_term.entry(term.to_string()).or_default().insert(base);
            }
        }
    }
    s
}

/// One observed read, reduced to what the metric needs: the touched file's
/// basename, whether it was an anchor, and the terms that named it.
struct Read {
    base: String,
    was_anchor: bool,
    terms: Vec<String>,
}

/// Fold the `feature.outcome` events into the observed read list (insertion
/// order is the event order; the metric counts are order-insensitive, and the
/// per-term tally is materialised through a BTreeMap so the output stays
/// sorted).
fn fold_reads(events: &[HarnessEvent]) -> Vec<Read> {
    events
        .iter()
        .filter(|e| e.event == EVENT_FEATURE_OUTCOME)
        .filter_map(|e| {
            let file = e.payload.get("file").and_then(Value::as_str)?;
            let base = basename(&norm(file)).to_string();
            if base.is_empty() {
                return None;
            }
            let was_anchor = e.payload.get("wasAnchor").and_then(Value::as_bool).unwrap_or(false);
            let terms = e
                .payload
                .get("terms")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default();
            Some(Read { base, was_anchor, terms })
        })
        .collect()
}

/// Build the byte-stable precision report from the suggested + observed folds.
/// Pure — unit-testable from synthetic event slices. Shape:
/// `{queries, recall_x1000, precision_x1000, anchorsSuggested, anchorsRead,
///   readsTotal, perTerm:[{term, reads, queries, precision_x1000}]}`.
fn build_report(events: &[HarnessEvent]) -> Value {
    let suggested = fold_suggested(events);
    let reads = fold_reads(events);

    // recall: distinct suggested anchors that were READ / suggested anchors.
    // An anchor is "read" iff some outcome with wasAnchor named that basename.
    let read_anchor_bases: BTreeSet<&str> = reads
        .iter()
        .filter(|r| r.was_anchor)
        .map(|r| r.base.as_str())
        .filter(|b| suggested.anchors.contains(*b))
        .collect();
    let anchors_suggested = suggested.anchors.len();
    let anchors_read = read_anchor_bases.len();
    let recall = ratio_x1000(anchors_read, anchors_suggested);

    // precision: reads-in-window that were anchors / reads-in-window total.
    let reads_total = reads.len();
    let reads_were_anchor = reads.iter().filter(|r| r.was_anchor).count();
    let precision = ratio_x1000(reads_were_anchor, reads_total);

    // perTerm: for each query term, how many reads it led to (reads whose
    // outcome terms include it), how many queries suggested it, and the
    // per-term precision (anchor-reads-for-term / reads-naming-term). Union
    // the term universe from BOTH sides so a term that suggested files but led
    // to no read still shows reads=0 (the low-value signal).
    let mut term_universe: BTreeSet<String> = suggested.per_term.keys().cloned().collect();
    for r in &reads {
        for t in &r.terms {
            term_universe.insert(t.clone());
        }
    }
    let per_term: Vec<Value> = term_universe
        .into_iter()
        .map(|term| {
            let reads_naming: Vec<&Read> =
                reads.iter().filter(|r| r.terms.iter().any(|t| t == &term)).collect();
            let reads_for_term = reads_naming.len();
            let anchor_reads_for_term = reads_naming.iter().filter(|r| r.was_anchor).count();
            let queries_for_term = usize::from(suggested.per_term.contains_key(&term));
            json!({
                "term": term,
                "reads": reads_for_term,
                "queries": queries_for_term,
                "precision_x1000": ratio_x1000(anchor_reads_for_term, reads_for_term),
            })
        })
        .collect();

    json!({
        "queries": suggested.queries,
        "recall_x1000": recall,
        "precision_x1000": precision,
        "anchorsSuggested": anchors_suggested,
        "anchorsRead": anchors_read,
        "readsTotal": reads_total,
        "perTerm": per_term,
    })
}

// --- event reader (mirrors digest_adherence_finalize's merged walk) ----------

/// Read the session's own event log. Empty on an unresolved session id or a
/// missing `.events/` dir — fail-open.
fn session_events(project_dir: &str, session: &str) -> Vec<HarnessEvent> {
    if session.is_empty() || session == "unknown" {
        return Vec::new();
    }
    let root = Path::new(project_dir);
    let claude_dir = ClaudePaths::for_project(root)
        .map(|p| p.claude_dir().clone())
        .unwrap_or_else(|_| ClaudePaths::compose_unchecked(root).claude_dir().clone());
    let dir = claude_dir.join(".session").join(session).join(".events");
    read_harness_events_from_ndjson_dir(&dir)
}

/// Read every spec sink — `.claude/spec/<name>/.events/` + each
/// `wave-N-*/.events/` subdir — keeping ONLY events of `session`. A research
/// query emitted at ANALYZE time lands in the session sink; once the session
/// binds a spec, later rounds (and the `feature.outcome` reads) route to the
/// spec sink, so both must be read and re-merged. Fail-open.
fn spec_events_for_session(project_dir: &str, session: &str) -> Vec<HarnessEvent> {
    if session.is_empty() || session == "unknown" {
        return Vec::new();
    }
    let root = Path::new(project_dir);
    let spec_root = ClaudePaths::for_project(root)
        .map(|p| p.spec_dir())
        .unwrap_or_else(|_| ClaudePaths::compose_unchecked(root).spec_dir());
    let Ok(spec_dirs) = fs::read_dir(&spec_root) else {
        return Vec::new();
    };
    let mut events: Vec<HarnessEvent> = Vec::new();
    for spec in spec_dirs.into_iter().filter(|e| e.is_dir) {
        let mut dirs: Vec<PathBuf> = vec![spec.path.join(".events")];
        if let Ok(waves) = fs::read_dir(&spec.path) {
            for w in waves.into_iter().filter(|e| e.is_dir && e.file_name.starts_with("wave-")) {
                dirs.push(w.path.join(".events"));
            }
        }
        for dir in dirs {
            events.extend(
                read_harness_events_from_ndjson_dir(&dir)
                    .into_iter()
                    .filter(|e| e.session_id == session),
            );
        }
    }
    events
}

/// Merge the session sink with every spec sink (session-filtered) into one
/// ts-sorted list — the same disjoint-sink merge `digest_adherence_finalize`
/// performs, generalised to all specs (the precision metric is not scoped to
/// one spec; a session's research may span several).
fn merged_events(project_dir: &str, session: &str) -> Vec<HarnessEvent> {
    let mut events = session_events(project_dir, session);
    events.extend(spec_events_for_session(project_dir, session));
    events.sort_by(|a, b| a.ts.cmp(&b.ts));
    events
}

/// CLI face: `mustard-rt run digest-precision [--root <dir>]`. Folds the active
/// session's `feature.query` × `feature.outcome` events into the byte-stable
/// precision JSON and prints it. Always exits 0 (fail-open telemetry).
pub fn run(root: &Path) {
    let project_dir = if root == Path::new(".") {
        crate::shared::context::project_dir()
    } else {
        root.to_string_lossy().into_owned()
    };
    let session = crate::shared::context::session_id();
    let events = merged_events(&project_dir, &session);
    let report = build_report(&events);
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
    use serde_json::json;

    fn ev(name: &str, ts: &str, payload: Value) -> HarnessEvent {
        HarnessEvent {
            v: SCHEMA_VERSION,
            ts: ts.to_string(),
            session_id: "s-1".to_string(),
            wave: 0,
            actor: Actor { kind: ActorKind::Cli, id: None, actor_type: None },
            event: name.to_string(),
            payload,
            spec: None,
        }
    }

    /// A `feature.query` event with the given per-term (term, files) report.
    fn query_ev(ts: &str, rows: &[(&str, &[&str])]) -> HarnessEvent {
        ev(
            EVENT_FEATURE_QUERY,
            ts,
            json!({
                "queryTerms": rows.iter().map(|(t, _)| *t).collect::<Vec<_>>(),
                "report": {
                    "matched": rows.len(), "total": rows.len(), "reason": "strong",
                    "terms": rows.iter().map(|(term, files)| json!({
                        "term": term, "tier": "exact", "lang": "", "files": files,
                    })).collect::<Vec<_>>(),
                },
            }),
        )
    }

    /// A `feature.outcome` event for a touched file.
    fn outcome_ev(ts: &str, file: &str, was_anchor: bool, terms: &[&str]) -> HarnessEvent {
        ev(
            EVENT_FEATURE_OUTCOME,
            ts,
            json!({ "file": file, "wasAnchor": was_anchor, "terms": terms }),
        )
    }

    #[test]
    fn ratio_x1000_is_integer_fixed_point_rounded() {
        assert_eq!(ratio_x1000(0, 0), 0, "empty denominator is zero, not NaN");
        assert_eq!(ratio_x1000(0, 4), 0);
        assert_eq!(ratio_x1000(4, 4), 1000, "full = 1000 per-mille");
        assert_eq!(ratio_x1000(1, 2), 500);
        assert_eq!(ratio_x1000(1, 3), 333, "rounds to nearest");
        assert_eq!(ratio_x1000(2, 3), 667, "rounds to nearest");
    }

    #[test]
    fn ac1_query_plus_anchor_read_lifts_recall_to_full() {
        // AC-1: a feature.query suggesting one anchor, then a Read of that
        // anchor → the projection marks it read (recall = 1000).
        let events = vec![
            query_ev("2026-06-16T00:00:01.000Z", &[("refund", &["src/refund.cs"])]),
            outcome_ev("2026-06-16T00:00:02.000Z", "/proj/src/refund.cs", true, &["refund"]),
        ];
        let r = build_report(&events);
        assert_eq!(r["queries"], json!(1));
        assert_eq!(r["anchorsSuggested"], json!(1));
        assert_eq!(r["anchorsRead"], json!(1), "the anchor was read: {r}");
        assert_eq!(r["recall_x1000"], json!(1000), "recall is full: {r}");
        assert_eq!(r["precision_x1000"], json!(1000), "the only read was an anchor");
    }

    #[test]
    fn ac2_projection_is_byte_stable_for_identical_events() {
        // AC-2: same events → identical bytes (the fold is deterministic).
        let events = vec![
            query_ev("2026-06-16T00:00:01.000Z", &[("refund", &["src/refund.cs"]), ("order", &["src/order.cs"])]),
            outcome_ev("2026-06-16T00:00:02.000Z", "/proj/src/refund.cs", true, &["refund"]),
            outcome_ev("2026-06-16T00:00:03.000Z", "/proj/src/extra.cs", false, &[]),
        ];
        let a = serde_json::to_string(&build_report(&events)).expect("ser");
        let b = serde_json::to_string(&build_report(&events)).expect("ser");
        assert_eq!(a, b, "byte-stable");
        // perTerm is sorted (order < refund) regardless of report order.
        let r = build_report(&events);
        let per = r["perTerm"].as_array().expect("perTerm");
        assert_eq!(per[0]["term"], json!("order"));
        assert_eq!(per[1]["term"], json!("refund"));
    }

    #[test]
    fn ac3_non_anchor_read_drops_precision_below_one() {
        // AC-3: a Read of a NON-anchor file is a read-not-suggested →
        // precision < 1 (one of two reads was an anchor → 500 per-mille).
        let events = vec![
            query_ev("2026-06-16T00:00:01.000Z", &[("refund", &["src/refund.cs"])]),
            outcome_ev("2026-06-16T00:00:02.000Z", "/proj/src/refund.cs", true, &["refund"]),
            outcome_ev("2026-06-16T00:00:03.000Z", "/proj/src/unrelated.cs", false, &[]),
        ];
        let r = build_report(&events);
        assert_eq!(r["readsTotal"], json!(2));
        assert_eq!(r["precision_x1000"], json!(500), "half the reads were anchors: {r}");
        // recall is still full — the one suggested anchor was read.
        assert_eq!(r["recall_x1000"], json!(1000));
    }

    #[test]
    fn per_term_counts_reads_and_flags_never_read_terms() {
        // A term that suggested an anchor but led to NO read shows reads=0,
        // queries=1 — the low-value signal Onda 3 would consume.
        let events = vec![
            query_ev(
                "2026-06-16T00:00:01.000Z",
                &[("refund", &["src/refund.cs"]), ("janela", &["src/window.cs"])],
            ),
            outcome_ev("2026-06-16T00:00:02.000Z", "/proj/src/refund.cs", true, &["refund"]),
        ];
        let r = build_report(&events);
        let per = r["perTerm"].as_array().expect("perTerm");
        // janela (sorted first) suggested but never led to a read.
        assert_eq!(per[0]["term"], json!("janela"));
        assert_eq!(per[0]["reads"], json!(0), "never-read term: {r}");
        assert_eq!(per[0]["queries"], json!(1));
        assert_eq!(per[0]["precision_x1000"], json!(0));
        // refund led to one anchor read.
        assert_eq!(per[1]["term"], json!("refund"));
        assert_eq!(per[1]["reads"], json!(1));
        assert_eq!(per[1]["precision_x1000"], json!(1000));
    }

    #[test]
    fn empty_events_fold_to_a_zero_report() {
        // Fail-open: no events → a well-formed zero report (no panic, no NaN).
        let r = build_report(&[]);
        assert_eq!(r["queries"], json!(0));
        assert_eq!(r["recall_x1000"], json!(0));
        assert_eq!(r["precision_x1000"], json!(0));
        assert_eq!(r["anchorsSuggested"], json!(0));
        assert_eq!(r["readsTotal"], json!(0));
        assert_eq!(r["perTerm"], json!([]));
    }

    #[test]
    fn read_events_fail_open_on_missing_dirs() {
        // Unknown/absent session + project degrade to an empty list, no panic.
        assert!(session_events("/nonexistent-mustard-xyzzy", "unknown").is_empty());
        assert!(session_events("/nonexistent-mustard-xyzzy", "s-1").is_empty());
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().to_str().expect("utf8");
        assert!(spec_events_for_session(project, "s-never").is_empty());
        assert!(merged_events(project, "s-never").is_empty());
    }
}
