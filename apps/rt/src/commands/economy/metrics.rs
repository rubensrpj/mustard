//! `mustard-rt run metrics` — a port of `scripts/metrics.js`.
//!
//! A unified CLI for pipeline + hook metrics:
//!
//! - `collect [--hooks-only]` — render the full pipeline + hook-event report.
//! - `report [--since <ISO>] [--event <type>]` — render the hook-event table.
//!
//! Port note: the JS `report --compare` mode resolves git tags via `git show`
//! and is ported below (`build_compare`); `--since` / `--event` filters are
//! ported. The `_rtk-gain.js` shell-out is a separate `run rtk-gain`
//! subcommand — RTK analytics are advisory.
//!
//! `--format json` (default) prints a structured JSON document; `--format
//! html` additionally writes a standalone HTML report and prints its path on
//! stderr. The JS script printed markdown; the JSON form is the new default
//! contract for the Rust port (markdown is a human concern, JSON is consumable).

use crate::commands::event::event_projections::pipeline::build_pipeline_state;
use crate::commands::event::event_projections::read_workspace_events;
use crate::report::{table, Report};
use mustard_core::io::fs;
use mustard_core::ClaudePaths;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Event → category, mirroring `EVENT_CATEGORY` in `metrics.js`.
fn event_category(event: &str) -> &'static str {
    match event {
        "auto-format" | "checklist-auto-mark" | "skill-size-gate" | "spec-size-gate" => "workflow",
        "bash-safety" | "budget-check" | "close-gate" | "enforce-registry" | "review-gate"
        | "skill-validate-gate" | "tool-use-counter" | "duplication-check" | "convention-check"
        | "file-guard" | "guard-verify" | "followup-cancel-gate" => "prevention",
        "bash-native-redirect" => "redirection",
        "memory-auto-extract" | "pre-compact" | "session-memory" | "context-lazy-load"
        | "skill-filter" | "refs-filter" | "spec-hygiene-move" => "extraction",
        "model-routing-gate" => "routing",
        "delegation" => "isolation",
        "rtk-rewrite" => "rtk",
        "output-budget" | "recommended-skills-audit" => "routing-advisory",
        "qa" | "review" => "verification",
        _ => "other",
    }
}

/// Whether `tokens_saved` should be trusted for an event (JS `ALWAYS_TRUSTED_EVENTS`).
fn token_trusted(event: &str) -> bool {
    matches!(
        event,
        "memory-auto-extract"
            | "pre-compact"
            | "spec-hygiene-move"
            | "budget-check"
            | "session-memory"
            | "context-lazy-load"
            | "skill-filter"
            | "refs-filter"
    )
}

/// One aggregated event bucket.
#[derive(Default)]
struct EventAgg {
    count: i64,
    tokens_affected: i64,
    tokens_saved: i64,
    notes: BTreeMap<String, i64>,
}

/// Aggregate every `.jsonl` line under `.claude/.metrics/` into per-event buckets.
fn aggregate_metrics(
    metrics_dir: &Path,
    since: Option<&str>,
    event_filter: Option<&str>,
) -> BTreeMap<String, EventAgg> {
    let mut agg: BTreeMap<String, EventAgg> = BTreeMap::new();
    let Ok(entries) = fs::read_dir(metrics_dir) else {
        return agg;
    };
    for entry in entries {
        if !entry.file_name.ends_with(".jsonl") {
            continue;
        }
        let Ok(content) = fs::read_to_string(&entry.path) else {
            continue;
        };
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let Some(event) = v.get("event").and_then(Value::as_str) else {
                continue;
            };
            if let Some(f) = event_filter {
                if event != f {
                    continue;
                }
            }
            if let Some(s) = since {
                if let Some(ts) = v.get("ts").and_then(Value::as_str) {
                    if ts < s {
                        continue;
                    }
                }
            }
            let bucket = agg.entry(event.to_string()).or_default();
            bucket.count += 1;
            if let Some(n) = v.get("tokens_affected").and_then(Value::as_i64) {
                bucket.tokens_affected += n;
            }
            if event != "rtk-rewrite" {
                if let Some(n) = v.get("tokens_saved").and_then(Value::as_i64) {
                    bucket.tokens_saved += n;
                }
            }
            if let Some(note) = v.get("note").and_then(Value::as_str) {
                if !note.is_empty() {
                    *bucket.notes.entry(note.to_string()).or_insert(0) += 1;
                }
            }
        }
    }
    agg
}

/// Serialize the aggregation into the JSON `byEvent` document.
fn agg_to_json(agg: &BTreeMap<String, EventAgg>) -> Value {
    let mut by_event = Map::new();
    let (mut total_count, mut total_saved, mut total_affected) = (0i64, 0i64, 0i64);
    for (event, b) in agg {
        let trusted = token_trusted(event);
        let saved = if trusted && event != "rtk-rewrite" { b.tokens_saved } else { 0 };
        by_event.insert(
            event.clone(),
            json!({
                "count": b.count,
                "category": event_category(event),
                "tokensAffected": b.tokens_affected,
                "tokensSaved": saved,
                "notes": b.notes.iter().map(|(k, v)| (k.clone(), json!(v))).collect::<Map<_, _>>(),
            }),
        );
        total_count += b.count;
        total_saved += saved;
        total_affected += b.tokens_affected;
    }
    json!({
        "byEvent": by_event,
        "total": { "count": total_count, "tokensSaved": total_saved, "tokensAffected": total_affected },
    })
}

/// Published in place of a pipeline counter that no readable source supports —
/// either the spec history could not be read at all, or the value it would be
/// derived from is absent. "no specs" and "I could not look" are different
/// answers, and a report that renders the second as `0` is trusted precisely
/// because it looks like a measurement.
const UNKNOWN: &str = "unknown";

/// Project every spec that appears in the live event log into
/// `{ name, metrics, isOrphaned }`.
///
/// Source of truth is `.claude/spec/*/.events/*.ndjson`, read through the
/// canonical walker and folded by the same `pipeline-state` projection
/// `event-projections --view pipeline-state` publishes — one reader, so the two
/// commands can never disagree. The predecessor read `.pipeline-states/`, a
/// directory the harness stopped writing, and reported `0` for every project.
///
/// Returns `None` when the log's own root (`.claude/spec/`) cannot be read:
/// that is "cannot look", and the caller must say so rather than publish a
/// zero it never measured.
fn collect_specs(project_root: &Path) -> Option<Vec<Value>> {
    let paths = ClaudePaths::for_project(project_root).ok()?;
    let spec_root = paths.spec_dir();
    // Probe the root before folding: an absent/unreadable spec dir is the one
    // case where an empty result would be a lie rather than a measurement.
    fs::read_dir(&spec_root).ok()?;

    let events = read_workspace_events(project_root);
    let names: BTreeSet<String> = events.iter().filter_map(|e| e.spec.clone()).collect();

    let mut out = Vec::new();
    for name in names {
        let state = build_pipeline_state(&events, Some(&name));
        let metrics = state
            .get("metrics")
            .cloned()
            .unwrap_or_else(|| json!({}));
        out.push(json!({
            "name": name,
            "metrics": metrics,
            "isOrphaned": !spec_root.join(&name).exists(),
        }));
    }
    Some(out)
}

/// The `pipelines` block for a spec list that was actually read.
///
/// `pub`: the acceptance test drives the counter rules directly, and building a
/// spec whose projection omits `retries` is only reachable from here.
///
/// First-pass honesty: a spec counts as pass-1 when its projection reports
/// `retries == 0` — a *read* zero. A spec whose projection carries no `retries`
/// at all was never measured, and one unmeasured spec makes the total a guess,
/// so `pass1` / `pass1Pct` fall back to [`UNKNOWN`]. The predecessor coerced the
/// missing key to `0` with `unwrap_or(0)`, which reported "100% first-pass" on
/// every project in existence because the projection never filled the field.
#[must_use]
pub fn pipelines_from_specs(specs: Vec<Value>) -> Value {
    let active = specs.iter().filter(|s| s["isOrphaned"] == json!(false)).count();
    let orphaned = specs.iter().filter(|s| s["isOrphaned"] == json!(true)).count();
    let total_specs = specs.len();

    let retries: Vec<Option<i64>> = specs
        .iter()
        .map(|s| s["metrics"].get("retries").and_then(Value::as_i64))
        .collect();
    let all_derived = retries.iter().all(Option::is_some);
    let pass1_count = retries.iter().flatten().filter(|n| **n == 0).count();
    // A rate over an empty denominator is undefined, not zero.
    let (pass1, pass1_pct) = match (all_derived, total_specs) {
        (false, _) => (json!(UNKNOWN), json!(UNKNOWN)),
        (true, 0) => (json!(0), json!(UNKNOWN)),
        (true, n) => (json!(pass1_count), json!((pass1_count * 100 / n) as i64)),
    };

    json!({
        "source": "events",
        "tracked": total_specs,
        "active": active,
        "orphaned": orphaned,
        "pass1": pass1,
        "pass1Pct": pass1_pct,
        "specs": specs,
    })
}

/// The `pipelines` block when no spec history was read — `source` names why
/// (`unreadable` = the log root could not be opened, `skipped` = the caller
/// asked for `--hooks-only`). Every counter carries the [`UNKNOWN`] marker so a
/// consumer cannot mistake "not measured" for "measured zero".
fn pipelines_unknown(source: &str) -> Value {
    json!({
        "source": source,
        "tracked": UNKNOWN,
        "active": UNKNOWN,
        "orphaned": UNKNOWN,
        "pass1": UNKNOWN,
        "pass1Pct": UNKNOWN,
        "specs": [],
    })
}

/// Build the `collect` JSON document.
#[must_use]
pub fn build_collect(cwd: &Path, hooks_only: bool) -> Value {
    let paths = ClaudePaths::for_project(cwd).ok();
    let claude_dir = paths
        .as_ref()
        .map(ClaudePaths::claude_dir)
        .unwrap_or_else(|| cwd.to_path_buf());
    let metrics_dir = paths
        .as_ref()
        .map(ClaudePaths::metrics_dir)
        .unwrap_or_else(|| claude_dir.clone());
    let hook_events = aggregate_metrics(&metrics_dir, None, None);

    let pipelines = if hooks_only {
        pipelines_unknown("skipped")
    } else {
        collect_specs(cwd).map_or_else(|| pipelines_unknown("unreadable"), pipelines_from_specs)
    };

    json!({
        "hookEvents": agg_to_json(&hook_events),
        "pipelines": pipelines,
    })
}

/// Build the `report` JSON document.
fn build_report(cwd: &Path, since: Option<&str>, event_filter: Option<&str>) -> Value {
    let metrics_dir = ClaudePaths::for_project(cwd)
        .map(|p| p.metrics_dir())
        .unwrap_or_else(|_| cwd.to_path_buf());
    let agg = aggregate_metrics(&metrics_dir, since, event_filter);
    agg_to_json(&agg)
}

/// A resolved compare endpoint — an ISO timestamp plus how it was obtained.
struct Endpoint {
    /// ISO-8601 timestamp string (lexicographically comparable).
    iso: String,
    /// `"tag"` (resolved via `git show`) or `"iso"` (literal date).
    source: &'static str,
    /// The raw `--compare` argument.
    raw: String,
}

/// Whether `value` looks like a semver git tag — `vX.Y.Z` or `X.Y.Z`.
fn is_tag(value: &str) -> bool {
    let body = value.strip_prefix('v').unwrap_or(value);
    let parts: Vec<&str> = body.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// Resolve a `--compare` endpoint: a semver tag is resolved to its commit date
/// via `git show -s --format=%cI`; anything else is parsed as an ISO date.
/// Returns `Err(message)` on a resolution failure (the JS exited `1`).
fn resolve_endpoint(value: &str) -> Result<Endpoint, String> {
    if is_tag(value) {
        let output = std::process::Command::new("git")
            .args(["show", "-s", "--format=%cI", value])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .map_err(|_| format!("could not resolve git tag \"{value}\" (is git available?)"))?;
        if !output.status.success() {
            return Err(format!(
                "could not resolve git tag \"{value}\" (is git available and the tag present?)"
            ));
        }
        let iso = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if mustard_core::time::parse_iso_millis(&iso).is_none() {
            return Err(format!("git returned unparseable date for \"{value}\": {iso}"));
        }
        return Ok(Endpoint { iso, source: "tag", raw: value.to_string() });
    }
    if mustard_core::time::parse_iso_millis(value).is_none() {
        return Err(format!(
            "\"{value}\" is not a valid git tag (expected vX.Y.Z) or ISO date"
        ));
    }
    Ok(Endpoint { iso: value.to_string(), source: "iso", raw: value.to_string() })
}

/// One windowed event bucket for the compare table.
#[derive(Default, Clone)]
struct CompareAgg {
    count: i64,
    tokens_affected: i64,
    tokens_saved: i64,
}

/// Aggregate `*.jsonl` lines whose `ts` falls in `[start, end)`.
fn aggregate_window(
    metrics_dir: &Path,
    start_ms: i64,
    end_ms: i64,
    event_filter: Option<&str>,
) -> BTreeMap<String, CompareAgg> {
    let mut agg: BTreeMap<String, CompareAgg> = BTreeMap::new();
    let Ok(entries) = fs::read_dir(metrics_dir) else {
        return agg;
    };
    for entry in entries {
        if !entry.file_name.ends_with(".jsonl") {
            continue;
        }
        let Ok(content) = fs::read_to_string(&entry.path) else {
            continue;
        };
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let Some(event) = v.get("event").and_then(Value::as_str) else {
                continue;
            };
            if let Some(f) = event_filter {
                if event != f {
                    continue;
                }
            }
            let Some(ts) = v.get("ts").and_then(Value::as_str) else {
                continue;
            };
            let Some(ts_ms) = mustard_core::time::parse_iso_millis(ts) else {
                continue;
            };
            if ts_ms < start_ms || ts_ms >= end_ms {
                continue;
            }
            let bucket = agg.entry(event.to_string()).or_default();
            bucket.count += 1;
            if let Some(n) = v.get("tokens_affected").and_then(Value::as_i64) {
                bucket.tokens_affected += n;
            }
            if event != "rtk-rewrite" {
                if let Some(n) = v.get("tokens_saved").and_then(Value::as_i64) {
                    bucket.tokens_saved += n;
                }
            }
        }
    }
    agg
}

/// Build the `report --compare <from> <to>` JSON document.
///
/// The new window is `[from, to)`; the reference window is the equal-length
/// span immediately before it (`[from-duration, from)`). Each per-event row
/// carries the reference and new counts so the consumer can render the delta.
fn build_compare(
    cwd: &Path,
    from: &str,
    to: &str,
    event_filter: Option<&str>,
) -> Result<Value, String> {
    let from_ep = resolve_endpoint(from)?;
    let to_ep = resolve_endpoint(to)?;
    let (Some(from_ms), Some(to_ms)) = (
        mustard_core::time::parse_iso_millis(&from_ep.iso),
        mustard_core::time::parse_iso_millis(&to_ep.iso),
    ) else {
        return Err("could not parse resolved endpoints".to_string());
    };
    if from_ms >= to_ms {
        return Err(format!(
            "--compare <from> must be earlier than <to> (got {} >= {})",
            from_ep.iso, to_ep.iso
        ));
    }
    let duration = to_ms - from_ms;
    let ref_start = from_ms - duration;
    let metrics_dir = ClaudePaths::for_project(cwd)
        .map(|p| p.metrics_dir())
        .unwrap_or_else(|_| cwd.to_path_buf());
    let new_agg = aggregate_window(&metrics_dir, from_ms, to_ms, event_filter);
    let ref_agg = aggregate_window(&metrics_dir, ref_start, from_ms, event_filter);

    let new_total: i64 = new_agg.values().map(|a| a.count).sum();
    let ref_total: i64 = ref_agg.values().map(|a| a.count).sum();
    let ref_sparse = ref_total < 5;

    let mut keys: BTreeSet<String> = BTreeSet::new();
    keys.extend(new_agg.keys().cloned());
    keys.extend(ref_agg.keys().cloned());
    let mut by_event = Map::new();
    for key in &keys {
        let r = ref_agg.get(key).cloned().unwrap_or_default();
        let n = new_agg.get(key).cloned().unwrap_or_default();
        by_event.insert(
            key.clone(),
            json!({
                "category": event_category(key),
                "ref": { "count": r.count, "tokensAffected": r.tokens_affected, "tokensSaved": r.tokens_saved },
                "new": { "count": n.count, "tokensAffected": n.tokens_affected, "tokensSaved": n.tokens_saved },
            }),
        );
    }
    Ok(json!({
        "compare": {
            "from": { "raw": from_ep.raw, "source": from_ep.source, "iso": from_ep.iso },
            "to": { "raw": to_ep.raw, "source": to_ep.source, "iso": to_ep.iso },
            "referenceWindow": { "events": ref_total, "sparse": ref_sparse },
            "newWindow": { "events": new_total },
        },
        "byEvent": by_event,
    }))
}

/// Write a standalone HTML report wrapping the metrics document.
///
/// Metrics reports are not per-spec QA — they are workspace-wide diagnostic
/// rollups, so the W2 cache reorg keeps them under
/// `<root>/.claude/.metrics/reports/`.
fn write_html_report(cwd: &Path, subcommand: &str, doc: &Value) -> Option<PathBuf> {
    let paths = ClaudePaths::for_project(cwd).ok()?;
    let dir = paths.metrics_dir().join("reports");
    fs::create_dir_all(&dir).ok()?;
    let mut report = Report::new(format!("Metrics — {subcommand}"), "pipeline + hook telemetry");

    // Render the hook-event table when present.
    let by_event = doc
        .get("hookEvents")
        .and_then(|h| h.get("byEvent"))
        .or_else(|| doc.get("byEvent"))
        .and_then(Value::as_object);
    if let Some(by_event) = by_event {
        let mut rows: Vec<Vec<String>> = by_event
            .iter()
            .map(|(event, e)| {
                vec![
                    event.clone(),
                    e.get("count").and_then(Value::as_i64).unwrap_or(0).to_string(),
                    e.get("category").and_then(Value::as_str).unwrap_or("").to_string(),
                    e.get("tokensSaved").and_then(Value::as_i64).unwrap_or(0).to_string(),
                ]
            })
            .collect();
        rows.sort_by(|a, b| a[0].cmp(&b[0]));
        report.section(
            "Hook Events",
            &table(&["Event", "Count", "Category", "Tokens Saved"], &rows),
        );
    }
    report.pre_section("Raw", &serde_json::to_string_pretty(doc).unwrap_or_default());
    let path = dir.join(format!("metrics-{subcommand}.html"));
    fs::write_atomic(&path, report.render().as_bytes()).ok()?;
    Some(path)
}

/// Dispatch `mustard-rt run metrics`.
pub fn run(subcommand: Option<&str>, args: &[String], format: &str) {
    // `metrics wave-status` is a sibling subcommand owned by the wave-network
    // spec; delegate to its module rather than threading it through the
    // collect/report data path.
    if subcommand == Some("wave-status") {
        crate::commands::economy::metrics_wave_status::run(args);
        return;
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (doc, sub) = match subcommand {
        Some("collect") => {
            let hooks_only = args.iter().any(|a| a == "--hooks-only");
            (build_collect(&cwd, hooks_only), "collect")
        }
        Some("report") => {
            let mut since = None;
            let mut event = None;
            let mut compare: Option<(String, String)> = None;
            let mut i = 0;
            while i < args.len() {
                match args[i].as_str() {
                    "--since" => {
                        since = args.get(i + 1).cloned();
                        i += 1;
                    }
                    "--event" => {
                        event = args.get(i + 1).cloned();
                        i += 1;
                    }
                    "--compare" => {
                        match (args.get(i + 1).cloned(), args.get(i + 2).cloned()) {
                            (Some(f), Some(t)) => {
                                compare = Some((f, t));
                                i += 2;
                            }
                            _ => {
                                eprintln!(
                                    "Error: --compare requires two arguments: --compare <from> <to>"
                                );
                                std::process::exit(1);
                            }
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            if let Some((from, to)) = compare {
                match build_compare(&cwd, &from, &to, event.as_deref()) {
                    Ok(doc) => (doc, "compare"),
                    Err(msg) => {
                        eprintln!("Error: {msg}");
                        std::process::exit(1);
                    }
                }
            } else {
                (build_report(&cwd, since.as_deref(), event.as_deref()), "report")
            }
        }
        _ => {
            eprintln!("Usage:");
            eprintln!("  metrics collect [--hooks-only] [--format json|html]");
            eprintln!("  metrics report [--since <ISO>] [--event <type>] [--compare <from> <to>] [--format json|html]");
            eprintln!("  metrics wave-status --spec <parent>");
            return;
        }
    };

    if format == "html" {
        match write_html_report(&cwd, sub, &doc) {
            Some(path) => eprintln!("[metrics] HTML report: {}", path.display()),
            None => eprintln!("[metrics] WARN: could not write HTML report"),
        }
    }
    println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".to_string()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_metric(dir: &Path, event: &str, line: &str) {
        let m = ClaudePaths::for_project(dir).unwrap().metrics_dir();
        std::fs::create_dir_all(&m).unwrap();
        let path = m.join(format!("{event}.jsonl"));
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        std::fs::write(&path, format!("{existing}{line}\n")).unwrap();
    }

    #[test]
    fn report_aggregates_events() {
        let dir = tempdir().unwrap();
        write_metric(dir.path(), "qa", r#"{"event":"qa","note":"pass","ts":"2026-05-19T00:00:00Z"}"#);
        write_metric(dir.path(), "qa", r#"{"event":"qa","note":"fail","ts":"2026-05-19T01:00:00Z"}"#);
        let doc = build_report(dir.path(), None, None);
        assert_eq!(doc["byEvent"]["qa"]["count"], json!(2));
        assert_eq!(doc["byEvent"]["qa"]["category"], json!("verification"));
    }

    #[test]
    fn report_since_filter_excludes_old() {
        let dir = tempdir().unwrap();
        write_metric(dir.path(), "qa", r#"{"event":"qa","ts":"2026-05-01T00:00:00Z"}"#);
        write_metric(dir.path(), "qa", r#"{"event":"qa","ts":"2026-05-19T00:00:00Z"}"#);
        let doc = build_report(dir.path(), Some("2026-05-10T00:00:00Z"), None);
        assert_eq!(doc["byEvent"]["qa"]["count"], json!(1));
    }

    #[test]
    fn is_tag_recognises_semver() {
        assert!(is_tag("v1.2.3"));
        assert!(is_tag("0.10.4"));
        assert!(!is_tag("2026-05-19T00:00:00Z"));
        assert!(!is_tag("v1.2"));
    }

    #[test]
    fn compare_splits_windows() {
        let dir = tempdir().unwrap();
        // Reference window event (earlier) + new window event (later).
        write_metric(dir.path(), "qa", r#"{"event":"qa","ts":"2026-05-01T00:00:00Z"}"#);
        write_metric(dir.path(), "qa", r#"{"event":"qa","ts":"2026-05-15T00:00:00Z"}"#);
        // from=05-10, to=05-20 → new window catches 05-15; ref window
        // [04-30, 05-10) catches 05-01.
        let doc = build_compare(
            dir.path(),
            "2026-05-10T00:00:00Z",
            "2026-05-20T00:00:00Z",
            None,
        )
        .unwrap();
        assert_eq!(doc["byEvent"]["qa"]["new"]["count"], json!(1));
        assert_eq!(doc["byEvent"]["qa"]["ref"]["count"], json!(1));
        assert_eq!(doc["compare"]["newWindow"]["events"], json!(1));
    }

    #[test]
    fn compare_rejects_inverted_window() {
        let dir = tempdir().unwrap();
        let err = build_compare(
            dir.path(),
            "2026-05-20T00:00:00Z",
            "2026-05-10T00:00:00Z",
            None,
        );
        assert!(err.is_err());
    }

    #[test]
    fn html_report_is_standalone() {
        let dir = tempdir().unwrap();
        write_metric(dir.path(), "budget-check", r#"{"event":"budget-check","ts":"2026-05-19T00:00:00Z"}"#);
        let doc = build_report(dir.path(), None, None);
        let path = write_html_report(dir.path(), "report", &doc).unwrap();
        let html = std::fs::read_to_string(path).unwrap();
        assert!(html.starts_with("<!doctype html>"));
        assert!(!html.contains("href=") && !html.contains("src="));
        assert!(html.contains("budget-check"));
    }
}
