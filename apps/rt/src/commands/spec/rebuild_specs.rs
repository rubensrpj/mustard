//! `mustard-rt run rebuild-specs` — generate `.summary.json` for every spec
//! found via filesystem walk of `.claude/spec/*/spec.md`.
//!
//! Why a dedicated subcommand
//! --------------------------
//!
//! The canonical `.summary.json` sidecar (schema: [`mustard_core::SpecSummaryDoc`])
//! is committed to git alongside each spec so teammates who clone later can read
//! spec history without the local `.events/*.ndjson` streams (which are not
//! versioned). `rebuild-specs` rematerialises every sidecar from the spec headers
//! and whatever NDJSON events exist locally — it is safe to run at any time;
//! the output is idempotent.
//!
//! Design
//! ------
//!
//! - **Source of truth:** `spec.md` header (`### Stage:`, `### Outcome:`, `### Lang:`,
//!   `### Scope:`) + NDJSON event log (timeline, AC results, waves).
//! - **Output:** `{spec_dir}/.summary.json` via [`mustard_core::summary::writer`].
//! - **Failure model:** fail-open per spec. A spec that fails to project is
//!   recorded in `errors[]` and skipped; the rest still materialise.
//!
//! Output (JSON, written to stdout):
//!
//! ```json
//! {
//!   "specs_count": 17,
//!   "duration_ms": 42,
//!   "errors": []
//! }
//! ```

use crate::shared::context::project_dir;
use mustard_core::claude_paths::ClaudePaths;
use mustard_core::fs as mfs;
use mustard_core::summary::{writer, AcResult, SpecSummaryDoc, SummaryTimeline, WaveSummary};
use mustard_core::EventReader;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Instant;

// `rebuild_one` is `pub` because `complete_spec::run` calls it.

/// Subcommand entry point — full re-materialisation across every spec.
///
/// Always exits `0`: the JSON report carries the count and any per-spec
/// errors so a caller (e.g. an integration test) can read both.
pub fn run() {
    let started = Instant::now();
    let project = PathBuf::from(project_dir());

    let (count, errors) = rematerialize_all(&project);

    print_json(&json!({
        "specs_count": count,
        "duration_ms": started.elapsed().as_millis() as u64,
        "errors": errors,
    }));
}

/// Re-materialise `.summary.json` for a single spec after a pipeline closes.
///
/// Fail-open: returns `Ok(())` even when the spec dir is absent or produces
/// no events — a minimal summary is still written from the header alone.
///
/// # Errors
/// Returns an error only when `ClaudePaths` rejects the project root (I1
/// guard violation). In practice this is always the caller's problem, not
/// the spec's.
pub fn rebuild_one(project_dir_str: &str, spec: &str) -> mustard_core::error::Result<()> {
    if spec.is_empty() {
        return Ok(());
    }
    let project = PathBuf::from(project_dir_str);
    let Ok(cp) = ClaudePaths::for_project(&project) else {
        return Ok(());
    };
    let Ok(sp) = cp.for_spec(spec) else {
        return Ok(());
    };
    let spec_dir = sp.dir().to_path_buf();
    if !spec_dir.exists() {
        return Ok(());
    }
    let spec_md = spec_dir.join("spec.md");
    let doc = build_summary_doc(spec, &spec_dir, &spec_md);
    let _ = writer::write(&spec_dir, &doc);
    Ok(())
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// Walk every `spec.md` under `.claude/spec/*/` and write `.summary.json`.
/// Returns `(count_ok, errors)`.
fn rematerialize_all(project: &Path) -> (usize, Vec<String>) {
    let Ok(cp) = ClaudePaths::for_project(project) else {
        return (0, vec!["invalid project root (I1 guard)".to_string()]);
    };
    let spec_root = cp.spec_dir();

    let entries = match std::fs::read_dir(&spec_root) {
        Ok(e) => e,
        Err(e) => return (0, vec![format!("read_dir {}: {e}", spec_root.display())]),
    };

    let mut count = 0usize;
    let mut errors: Vec<String> = Vec::new();

    for entry in entries.flatten() {
        let spec_dir = entry.path();
        if !spec_dir.is_dir() {
            continue;
        }
        let spec_md = spec_dir.join("spec.md");
        if !spec_md.exists() {
            continue;
        }
        let Some(spec_name) = spec_dir.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
            continue;
        };

        let doc = build_summary_doc(&spec_name, &spec_dir, &spec_md);
        match writer::write(&spec_dir, &doc) {
            Ok(()) => count += 1,
            Err(e) => errors.push(format!("{spec_name}: write failed: {e}")),
        }
    }

    (count, errors)
}

/// Build a [`SpecSummaryDoc`] from the spec's header + local NDJSON events.
///
/// Every field is derived from what's available on disk — no field is
/// mandatory; missing data degrades gracefully to `None` / empty `Vec`.
fn build_summary_doc(spec: &str, spec_dir: &Path, spec_md: &Path) -> SpecSummaryDoc {
    // --- Header parse ---
    let head_text = mfs::read_to_string(spec_md)
        .unwrap_or_default();
    let head_lines: String = head_text.lines().take(30).collect::<Vec<_>>().join("\n");

    let stage = header_value(&head_lines, "stage");
    let outcome = header_value(&head_lines, "outcome");
    let lang = header_value(&head_lines, "lang");
    let scope = header_value(&head_lines, "scope");

    // Title: first `# ` line in the file.
    let title = head_text
        .lines()
        .find(|l| l.starts_with("# "))
        .map(|l| l.trim_start_matches('#').trim().to_string())
        .unwrap_or_else(|| spec.to_string());

    // --- NDJSON timeline from per-spec events dir ---
    let events_dir = spec_dir.join(".events");
    let timeline = build_timeline_from_events(&events_dir);

    // --- Wave summaries (each wave-N-*/spec.md header) ---
    let waves = build_wave_summaries(spec_dir);

    SpecSummaryDoc {
        version: 1,
        spec: spec.to_string(),
        title,
        lang,
        scope,
        stage,
        outcome,
        timeline,
        waves,
        ..Default::default()
    }
}

/// Build a [`SummaryTimeline`] from the NDJSON events in `events_dir`.
///
/// Scans for `pipeline.status` events and maps known status values to
/// timeline slots. Fail-open: an unreadable dir produces an empty timeline.
fn build_timeline_from_events(events_dir: &Path) -> SummaryTimeline {
    let Ok(dir_entries) = std::fs::read_dir(events_dir) else {
        return SummaryTimeline::default();
    };

    let mut draft_at: Option<String> = None;
    let mut approved_at: Option<String> = None;
    let mut execute_started_at: Option<String> = None;
    let mut review_at: Option<String> = None;
    let mut qa_at: Option<String> = None;
    let mut closed_at: Option<String> = None;

    for entry in dir_entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("ndjson") {
            continue;
        }
        for ev in EventReader::stream(&path) {
            // Only `pipeline.status` events carry lifecycle milestones.
            if ev.kind != "pipeline.status" {
                continue;
            }
            let ts = ev
                .raw
                .get("ts")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let to = ev.payload.get("to").and_then(|v| v.as_str()).map(str::to_string);
            match to.as_deref() {
                Some("planning") | Some("plan") => {
                    if draft_at.is_none() {
                        draft_at = ts;
                    }
                }
                Some("approved") => approved_at = ts,
                Some("implementing") | Some("execute") => {
                    if execute_started_at.is_none() {
                        execute_started_at = ts;
                    }
                }
                Some("reviewing") | Some("review") => review_at = ts,
                Some("qa") | Some("qa-review") => qa_at = ts,
                Some("closed") | Some("closed-followup") | Some("close") => closed_at = ts,
                _ => {}
            }
        }
    }

    SummaryTimeline {
        draft_at,
        approved_at,
        execute_started_at,
        review_at,
        qa_at,
        closed_at,
    }
}

/// Build wave summaries by scanning `wave-N-*/spec.md` subdirectories.
///
/// Each wave's `### Stage:` + `### Outcome:` header determines `status`.
/// The role is derived from the directory suffix (`wave-N-{role}`).
fn build_wave_summaries(spec_dir: &Path) -> Vec<WaveSummary> {
    let Ok(entries) = std::fs::read_dir(spec_dir) else {
        return Vec::new();
    };
    let mut waves: Vec<(u32, WaveSummary)> = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let dir_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        // Must match `wave-<digits>-<role>`.
        let Some(after_wave) = dir_name.strip_prefix("wave-") else {
            continue;
        };
        let digit_end = after_wave
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(0);
        if digit_end == 0 || !after_wave[digit_end..].starts_with('-') {
            continue;
        }
        let Ok(wave_n) = after_wave[..digit_end].parse::<u32>() else {
            continue;
        };
        let role = after_wave[digit_end + 1..].to_string();

        let spec_md = path.join("spec.md");
        if !spec_md.exists() {
            continue;
        }
        let head_text = mfs::read_to_string(&spec_md).unwrap_or_default();
        let head: String = head_text.lines().take(30).collect::<Vec<_>>().join("\n");

        let stage = header_value(&head, "stage").unwrap_or_default();
        let outcome = header_value(&head, "outcome").unwrap_or_default();

        let status = wave_status_from_header(&stage, &outcome);

        // Read qa-report.json for AC results if it exists.
        let ac_results = read_ac_results_from_sidecar(&path);

        let summary_line = extract_summary_line(&head_text);

        waves.push((
            wave_n,
            WaveSummary {
                n: wave_n,
                role,
                summary: summary_line,
                status,
                ac_results,
                review: None,
                qa: None,
                concerns: Vec::new(),
            },
        ));
    }

    waves.sort_by_key(|(n, _)| *n);
    waves.into_iter().map(|(_, w)| w).collect()
}

/// Map `(stage, outcome)` header values to a `WaveSummary.status` string.
fn wave_status_from_header(stage: &str, outcome: &str) -> String {
    if outcome.eq_ignore_ascii_case("completed") || stage.eq_ignore_ascii_case("close") {
        "completed".to_string()
    } else if outcome.eq_ignore_ascii_case("cancelled") {
        "cancelled".to_string()
    } else if stage.is_empty() {
        "in_progress".to_string()
    } else {
        "in_progress".to_string()
    }
}

/// Read AC results from the wave's `qa-report.json` sidecar. Returns empty
/// vec when the file is absent or malformed.
fn read_ac_results_from_sidecar(wave_dir: &Path) -> Vec<AcResult> {
    let path = wave_dir.join("qa-report.json");
    let text = match mfs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let v: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let Some(criteria) = v.get("criteria").and_then(|c| c.as_array()) else {
        return Vec::new();
    };
    criteria
        .iter()
        .filter_map(|c| {
            let id = c.get("id")?.as_str()?;
            let status = c.get("status")?.as_str()?;
            Some(AcResult {
                id: id.to_string(),
                pass: status == "pass",
                command: None,
                note: None,
            })
        })
        .collect()
}

/// Extract the first non-empty line under `## Resumo` / `## Summary`.
fn extract_summary_line(body: &str) -> String {
    let mut in_section = false;
    for line in body.lines() {
        let t = line.trim();
        if !in_section {
            if t.starts_with("## ") {
                let after = t.trim_start_matches('#').trim().to_lowercase();
                if after == "resumo" || after == "summary" {
                    in_section = true;
                }
            }
            continue;
        }
        if t.is_empty() {
            continue;
        }
        if t.starts_with("## ") {
            break;
        }
        return t.chars().take(200).collect();
    }
    String::new()
}

/// Parse `### Key: value` from a header block (case-insensitive key match).
fn header_value(head: &str, key_lower: &str) -> Option<String> {
    for line in head.lines() {
        let t = line.trim_start();
        let Some(rest) = t.strip_prefix("### ") else {
            continue;
        };
        let Some(colon) = rest.find(':') else { continue };
        if rest[..colon].trim().eq_ignore_ascii_case(key_lower) {
            let v = rest[colon + 1..].trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Pretty-print a JSON value with two-space indentation.
fn print_json(value: &serde_json::Value) {
    let text = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    println!("{text}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn rematerialize_all_writes_summary_json() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        // Create .claude/spec/my-spec/spec.md
        let spec_dir = project.join(".claude").join("spec").join("my-spec");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("spec.md"),
            "# My Spec\n### Stage: Close\n### Outcome: Completed\n### Lang: en-US\n\n## Summary\nDid the thing.\n",
        )
        .unwrap();
        // Write mustard.json so ClaudePaths resolves the root.
        std::fs::write(project.join("mustard.json"), "{}").unwrap();

        let (count, errors) = rematerialize_all(project);
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(count, 1);
        let summary_path = spec_dir.join(".summary.json");
        assert!(summary_path.exists());
        let raw = std::fs::read_to_string(&summary_path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["spec"].as_str(), Some("my-spec"));
        assert_eq!(v["stage"].as_str(), Some("Close"));
        assert_eq!(v["outcome"].as_str(), Some("Completed"));
        assert_eq!(v["version"].as_u64(), Some(1));
    }

    #[test]
    fn rematerialize_is_idempotent() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let spec_dir = project.join(".claude").join("spec").join("alpha");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), "# Alpha\n### Stage: Execute\n").unwrap();
        std::fs::write(project.join("mustard.json"), "{}").unwrap();

        let (c1, _) = rematerialize_all(project);
        let (c2, _) = rematerialize_all(project);
        assert_eq!(c1, c2);
        let count = project
            .join(".claude")
            .join("spec")
            .join("alpha")
            .join(".summary.json")
            .exists() as usize;
        assert_eq!(count, 1);
    }

    #[test]
    fn empty_spec_dir_yields_zero_count() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        std::fs::create_dir_all(project.join(".claude").join("spec")).unwrap();
        std::fs::write(project.join("mustard.json"), "{}").unwrap();
        let (count, errors) = rematerialize_all(project);
        assert_eq!(count, 0);
        assert!(errors.is_empty());
    }

    #[test]
    fn build_wave_summaries_parses_role_and_status() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path();
        let wave_dir = spec_dir.join("wave-0-rt");
        std::fs::create_dir_all(&wave_dir).unwrap();
        std::fs::write(
            wave_dir.join("spec.md"),
            "### Stage: Close\n### Outcome: Completed\n",
        )
        .unwrap();

        let waves = build_wave_summaries(spec_dir);
        assert_eq!(waves.len(), 1);
        assert_eq!(waves[0].n, 0);
        assert_eq!(waves[0].role, "rt");
        assert_eq!(waves[0].status, "completed");
    }
}
