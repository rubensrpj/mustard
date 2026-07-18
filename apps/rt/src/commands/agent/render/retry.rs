//! `## RETRY CONTEXT` composition for a re-dispatched wave (granular /
//! fix-loop): fold the WHY a wave was rejected — the last review verdict, the
//! reviewer's persisted findings, and the prior-wave diff + change requests —
//! from what the spec already recorded, so a rejected wave is not re-dispatched
//! blind.

use mustard_core::io::fs as mfs;
use mustard_core::ClaudePaths;
use std::path::Path;

/// Read every harness event recorded for `spec` from its per-spec NDJSON sink
/// (`.claude/spec/{spec}/.events`), chronologically sorted by the core reader.
/// Empty on any path/IO error — fail-open like every reader here.
fn read_spec_events(
    project: &Path,
    spec: &str,
) -> Vec<mustard_core::domain::model::event::HarnessEvent> {
    let Ok(sp) = ClaudePaths::for_project(project).and_then(|p| p.for_spec(spec)) else {
        return Vec::new();
    };
    mustard_core::view::projection::read_harness_events_from_ndjson_dir(&sp.events_dir())
}

/// Compose the `## RETRY CONTEXT` body for a re-dispatched wave (granular /
/// fix-loop) when the caller passed no explicit `--retry-context-file`. A
/// rejected wave otherwise went back to the implementer with a blank prompt;
/// this folds in the WHY from what the spec already recorded:
///   - the last `review.result` verdict + critical count and the last
///     `pipeline.wave.failed` signal (the event summary);
///   - the reviewer's findings persisted at `<spec>/review/findings.md`
///     (written by `review-result --findings-file`);
///   - the prior-wave diff and mid-pipeline change requests already computed in
///     [`super::render_prompt_at`] and passed in here — the retry template omits
///     both, and re-passing them avoids reading anything twice.
///
/// Every part is optional; when all resolve empty the result is `""` and the
/// `## RETRY CONTEXT` heading collapses (`collapse_empty_sections`). Sub-headings
/// are `### ` so they are never mistaken for `## ` section boundaries.
/// Deterministic: the event reader sorts by `ts`, so "last" is stable, and the
/// findings/diff/change-log inputs are byte-stable on disk.
pub(crate) fn compose_retry_context(
    project: &Path,
    spec: Option<&str>,
    spec_dir: &Path,
    prior_wave_diff: &str,
    change_log: &str,
) -> String {
    let mut sections: Vec<String> = Vec::new();

    // 1. Event summary — the last review verdict + wave-failure signal. Only a
    //    spec has an event sink; a spec-less retry skips straight to the files.
    if let Some(s) = spec {
        let events = read_spec_events(project, s);
        let mut lines: Vec<String> = Vec::new();
        if let Some(ev) = events.iter().rev().find(|e| e.event == "review.result") {
            let verdict = ev.payload.get("verdict").and_then(|v| v.as_str()).unwrap_or("");
            let critical = ev.payload.get("criticalCount").and_then(|v| v.as_i64()).unwrap_or(0);
            if !verdict.is_empty() {
                lines.push(format!(
                    "Prior review verdict: {} — {critical} critical finding(s).",
                    verdict.to_uppercase()
                ));
            }
        }
        if let Some(ev) = events.iter().rev().find(|e| e.event == "pipeline.wave.failed") {
            // The twin payload today is `{spec, wave}`; a richer emitter may add
            // a `reason`. Prefer the reason, else name the wave that failed.
            let reason = ev.payload.get("reason").and_then(|v| v.as_str()).unwrap_or("");
            if !reason.is_empty() {
                lines.push(format!("Last wave failure: {reason}"));
            } else if let Some(w) = ev.payload.get("wave").and_then(|v| v.as_u64()) {
                lines.push(format!("Last wave failure recorded (wave {w})."));
            }
        }
        if !lines.is_empty() {
            sections.push(format!("### Prior verdict\n{}", lines.join("\n")));
        }
    }

    // 2. The reviewer's persisted findings (`review-result --findings-file`).
    let findings =
        mfs::read_to_string(spec_dir.join("review").join("findings.md")).unwrap_or_default();
    if !findings.trim().is_empty() {
        sections.push(format!("### Review findings\n{}", findings.trim()));
    }

    // 3. Reuse the already-built prior-wave diff + change requests so the retry
    //    carries what the retry template otherwise drops.
    if !prior_wave_diff.trim().is_empty() {
        sections.push(format!("### Prior wave diff\n{}", prior_wave_diff.trim()));
    }
    if !change_log.trim().is_empty() {
        sections.push(format!("### Change requests\n{}", change_log.trim()));
    }

    sections.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Plant a workspace anchor so `ClaudePaths::for_project` accepts the temp dir.
    fn anchor(dir: &Path) {
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(dir.join("mustard.json"), b"{}").unwrap();
    }

    /// B1: `compose_retry_context` folds the persisted findings, the prior-wave
    /// diff and the mid-pipeline change requests into the retry body under `### `
    /// sub-headings; nothing recorded → `""` (so `## RETRY CONTEXT` collapses).
    #[test]
    fn compose_retry_context_folds_findings_diff_and_changelog() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        // Findings persisted by `review-result --findings-file`.
        let sp = ClaudePaths::for_project(dir.path()).unwrap().for_spec("demo").unwrap();
        let review_dir = sp.dir().join("review");
        std::fs::create_dir_all(&review_dir).unwrap();
        std::fs::write(review_dir.join("findings.md"), "- critical: null deref in parse()\n").unwrap();

        let body = compose_retry_context(
            dir.path(),
            Some("demo"),
            sp.dir(),
            "diff --git a/x b/x\n+added line",
            "- **ts1** — tighten the guard",
        );
        assert!(body.contains("### Review findings"), "findings heading: {body}");
        assert!(body.contains("null deref in parse()"), "findings body: {body}");
        assert!(body.contains("### Prior wave diff"), "diff heading: {body}");
        assert!(body.contains("+added line"), "diff body: {body}");
        assert!(body.contains("### Change requests"), "changelog heading: {body}");
        assert!(body.contains("tighten the guard"), "changelog body: {body}");

        // Nothing recorded → empty (the `## RETRY CONTEXT` heading collapses).
        let bare = ClaudePaths::for_project(dir.path()).unwrap().for_spec("bare").unwrap();
        assert!(
            compose_retry_context(dir.path(), Some("bare"), bare.dir(), "", "").is_empty(),
            "no signals must yield empty retry context"
        );
    }
}
