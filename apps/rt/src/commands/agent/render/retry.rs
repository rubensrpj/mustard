//! `## RETRY CONTEXT` composition for a re-dispatched wave (granular /
//! fix-loop): fold the WHY a wave was rejected — the last review verdict, the
//! reviewer's persisted findings, and the prior-wave diff + change requests —
//! from what the spec already recorded, so a rejected wave is not re-dispatched
//! blind.

use crate::commands::review::review_result;
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
///   - the reviewer's findings persisted under `<spec>/review/` (written by
///     `review-result --findings-file`);
///   - the prior-wave diff and mid-pipeline change requests already computed in
///     [`super::render_prompt_at`] and passed in here — the retry template omits
///     both, and re-passing them avoids reading anything twice.
///
/// Every part is optional; when all resolve empty the result is `""` and the
/// `## RETRY CONTEXT` heading collapses (`collapse_empty_sections`). Sub-headings
/// are `### ` so they are never mistaken for `## ` section boundaries.
/// Deterministic: the event reader sorts by `ts`, so "last" is stable, and the
/// findings/diff/change-log inputs are byte-stable on disk.
///
/// ## Why `subproject`
///
/// A review round emits one verdict and one findings file PER SUBPROJECT, but
/// this context was assembled per SPEC. Two fix-loop prompts rendered for
/// different subprojects therefore came out with identical task text and sent
/// two writers into one set of files; nothing but the agents' own care kept the
/// work from being written twice. Both the verdict lookup and the findings read
/// are now scoped, and `None` keeps the spec-wide reading for a caller with no
/// subproject to name.
pub(crate) fn compose_retry_context(
    project: &Path,
    spec: Option<&str>,
    spec_dir: &Path,
    subproject: Option<&str>,
    prior_wave_diff: &str,
    change_log: &str,
) -> String {
    let mut sections: Vec<String> = Vec::new();

    // 1. Event summary — the last review verdict + wave-failure signal. Only a
    //    spec has an event sink; a spec-less retry skips straight to the files.
    if let Some(s) = spec {
        let events = read_spec_events(project, s);
        let mut lines: Vec<String> = Vec::new();
        // Scoped to THIS subproject: the review round emits one `review.result`
        // per touched subproject, so "the last verdict" spec-wide is whichever
        // reviewer happened to finish last — a verdict about someone else's
        // files. A verdict that named no subproject still speaks for every one.
        let verdict_is_ours = |e: &&mustard_core::domain::model::event::HarnessEvent| {
            let named = e.payload.get("subproject").and_then(|v| v.as_str());
            match (subproject, named) {
                (Some(want), Some(got)) => same_subproject(want, got),
                (_, None) => true,
                (None, Some(_)) => false,
            }
        };
        if let Some(ev) = events
            .iter()
            .rev()
            .find(|e| e.event == "review.result" && verdict_is_ours(e))
        {
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

    // 2. The reviewer's persisted findings (`review-result --findings-file`),
    //    scoped to this subproject.
    let findings = read_scoped_findings(&spec_dir.join("review"), subproject);
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

/// `true` when two subproject names denote the same subproject. Compared on the
/// normalised path (either separator, no surrounding slashes, case-insensitive)
/// because a dispatch item carries `apps/rt` while a hand-run `review-result`
/// may have recorded `apps\rt` or a trailing slash.
fn same_subproject(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        s.trim()
            .replace('\\', "/")
            .trim_matches('/')
            .to_ascii_lowercase()
    };
    norm(a) == norm(b)
}

/// The findings this subproject's retry may read.
///
/// Order, and why it is not a plain fallback chain:
///
/// 1. `findings-{slug}.md` — written by the review that named this subproject.
/// 2. `findings.md` — the spec-wide file, but ONLY when the review directory
///    holds no scoped file at all. Once ANY review has recorded per-subproject
///    findings, the unscoped file is the last reviewer's, whoever that was, and
///    handing it to a different subproject is exactly the defect this scoping
///    removes: two prompts, identical task text, two writers in one set of
///    files.
/// 3. Nothing. A subproject whose review left no findings gets no findings —
///    which is the fact, and the retry still carries its diff and the change
///    requests.
///
/// Fail-open: an unreadable directory or file yields `""`.
fn read_scoped_findings(review_dir: &Path, subproject: Option<&str>) -> String {
    if let Some(name) = review_result::scoped_findings_name(subproject) {
        let scoped = mfs::read_to_string(review_dir.join(name)).unwrap_or_default();
        if !scoped.trim().is_empty() {
            return scoped;
        }
    }
    if has_scoped_findings(review_dir) {
        return String::new();
    }
    mfs::read_to_string(review_dir.join(review_result::FINDINGS_FILE)).unwrap_or_default()
}

/// `true` when `review_dir` holds at least one subproject-scoped findings file.
/// Fail-open: an unreadable directory reads as "none", which at worst restores
/// the spec-wide fallback.
fn has_scoped_findings(review_dir: &Path) -> bool {
    let Ok(entries) = mfs::read_dir(review_dir) else {
        return false;
    };
    entries.iter().any(|e| {
        !e.is_dir
            && e.file_name.starts_with(review_result::FINDINGS_SCOPED_PREFIX)
            && e.file_name.ends_with(".md")
    })
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
            None,
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
            compose_retry_context(dir.path(), Some("bare"), bare.dir(), None, "", "").is_empty(),
            "no signals must yield empty retry context"
        );
    }

    /// AC-4: two fix-loop prompts rendered for two subprojects of ONE spec carry
    /// different findings.
    ///
    /// The measured defect: the review round records one verdict and one
    /// findings file per subproject, but this context read a single spec-wide
    /// `findings.md`. Both prompts came out with identical task text and sent two
    /// writers into one set of files; only the agents' own care kept the work
    /// from being written twice.
    ///
    /// Written through `record_review`, the real writer, so the test cannot pass
    /// over a fixture layout the producer never creates.
    #[test]
    fn retry_context_is_scoped_to_its_subproject() {
        use crate::commands::review::review_result::record_review;

        let dir = tempdir().unwrap();
        anchor(dir.path());
        let sp = ClaudePaths::for_project(dir.path()).unwrap().for_spec("demo").unwrap();

        for (sub, finding) in [("apps/rt", "null deref in parse()"), ("packages/core", "unclosed reader in load()")] {
            let src = dir.path().join(format!("findings-{}.md", sub.replace('/', "-")));
            std::fs::write(&src, format!("- critical: {finding}\n")).unwrap();
            record_review(dir.path(), "demo", "rejected", 1, Some(sub), Some(&src));
        }

        let rt = compose_retry_context(dir.path(), Some("demo"), sp.dir(), Some("apps/rt"), "", "");
        let core =
            compose_retry_context(dir.path(), Some("demo"), sp.dir(), Some("packages/core"), "", "");

        assert!(rt.contains("null deref in parse()"), "rt lost its own findings: {rt}");
        assert!(
            !rt.contains("unclosed reader in load()"),
            "the other subproject's findings leaked into rt: {rt}"
        );
        assert!(
            core.contains("unclosed reader in load()"),
            "core lost its own findings: {core}"
        );
        assert!(
            !core.contains("null deref in parse()"),
            "the other subproject's findings leaked into core: {core}"
        );
        assert_ne!(rt, core, "two subprojects must not be handed the same task text");

        // A subproject this round never reviewed gets no findings at all —
        // rather than the last reviewer's, which is the leak in another shape.
        let untouched =
            compose_retry_context(dir.path(), Some("demo"), sp.dir(), Some("apps/cli"), "", "");
        assert!(
            !untouched.contains("### Review findings"),
            "an unreviewed subproject inherited someone else's findings: {untouched}"
        );
    }
}
