//! `mustard-rt run change-request` — record a DELIBERATE mid-pipeline change
//! request, carrying the INSTRUCTION the conversation produced.
//!
//! ## Why this exists
//!
//! [`crate::hooks::observe::change_request_log`] captures every user prompt of
//! an active spec into `change-log.md`, and the per-wave renderer feeds those
//! bullets to the agents (`## CHANGE REQUESTS`). That capture is deliberately
//! BLIND: it fires on the prompt event and can only store the sentence that was
//! typed. A reply carries its meaning from the conversation, not from its own
//! words — "pode ir" recorded alone is an approval with no trace of what was
//! approved, and an agent reading it learns nothing it can act on.
//!
//! The one who knows what the sentence meant is the orchestrator. So the blind
//! capture stays exactly as it is (the raw, greppable trail) and this command is
//! the deliberate record beside it: the orchestrator states the instruction, and
//! it lands in the SAME two files the observer writes, in the SAME shape the
//! renderer reads.
//!
//! ## Single source for the record shape
//!
//! Both writers are reused from the observer module rather than re-implemented
//! here. Hand-formatting the bullet (what this command replaces) is fragile
//! precisely because the shape is a contract the renderer filters on — one wrong
//! prefix and the instruction silently never arrives. A second copy of that
//! format inside this file would reintroduce the same drift with extra steps.
//!
//! ## Refusal, not silence
//!
//! An empty / whitespace-only instruction is refused and NOTHING is written, so
//! the log cannot fill with entries that say nothing. And because the underlying
//! writers are fail-open (telemetry must never block a turn), this command
//! RE-READS the log after appending and reports `recorded: false` when the
//! instruction did not land — a loss is reported instead of being assumed away.

use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::hooks::observe::change_request_log::{
    append_change_log_md, append_change_request, CHANGE_LOG_MD,
};
use crate::shared::context::{current_spec, session_id, spec_for_session};
use mustard_core::io::claude_paths::ClaudePaths;
use mustard_core::io::fs as mfs;

/// Count the change-log's bullet lines — the same `- ` shape the per-wave
/// renderer filters on, so this counts exactly what a dispatched agent will
/// read. A missing file counts as zero, which makes the first append a growth.
fn count_bullets(log_path: &Path) -> usize {
    mfs::read_to_string(log_path)
        .map(|body| {
            body.lines()
                .filter(|l| l.trim_start().starts_with("- "))
                .count()
        })
        .unwrap_or(0)
}

/// Options for `mustard-rt run change-request`.
#[derive(Debug, Clone)]
pub struct ChangeRequestOpts {
    /// Spec slug under `.claude/spec/`. Omitted: the session→spec marker, then
    /// the active-spec fallback — the same resolution the observer uses.
    pub spec: Option<String>,
    /// What the orchestrator instructs, in full — the meaning the conversation
    /// produced, not the sentence the user typed.
    pub instruction: String,
}

/// JSON report printed on stdout.
#[derive(Debug, Serialize)]
pub(crate) struct ChangeRequestReport {
    /// `true` only when the instruction is proven to be in the change log.
    pub ok: bool,
    /// The spec the request was attributed to (`None` when none resolved).
    pub spec: Option<String>,
    /// Re-read confirmation that the bullet landed.
    pub recorded: bool,
    /// Path of the change log the bullet was appended to.
    pub change_log: Option<String>,
    /// Refusal / failure reason: `empty_instruction`, `no_spec`,
    /// `unknown_spec`, `write_failed`.
    pub error: Option<String>,
}

/// Resolve the spec to attribute the request to: the explicit `--spec` first,
/// then the session→spec marker, then the active-spec fallback (the observer's
/// order — the two records must never land on different specs).
fn resolve_spec(project_dir: &str, explicit: Option<&str>) -> Option<String> {
    if let Some(s) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        return Some(s.to_string());
    }
    let sid = session_id();
    if !sid.is_empty() && sid != "unknown" {
        if let Some(spec) = spec_for_session(project_dir, &sid) {
            return Some(spec);
        }
    }
    current_spec(project_dir)
}

/// The spec's current `meta.json#stage`, so the bullet carries the same stage
/// tag the blind capture would have carried. `None` on any read error.
fn read_stage(project: &Path, spec: &str) -> Option<String> {
    let sp = ClaudePaths::for_project(project).ok()?.for_spec(spec).ok()?;
    mustard_core::domain::meta::read_meta_beside(&sp.spec_md_path())?.stage
}

/// Core routine — writes the two records, returns the report.
fn record(project: &Path, opts: &ChangeRequestOpts) -> ChangeRequestReport {
    let mut report = ChangeRequestReport {
        ok: false,
        spec: None,
        recorded: false,
        change_log: None,
        error: None,
    };
    // Collapse whitespace up front: the bullet is one line, and the collapsed
    // form is also what the re-read below looks for.
    let instruction = opts.instruction.split_whitespace().collect::<Vec<_>>().join(" ");
    if instruction.is_empty() {
        report.error = Some("empty_instruction".to_string());
        return report;
    }
    let project_dir = project.to_string_lossy().into_owned();
    let Some(spec) = resolve_spec(&project_dir, opts.spec.as_deref()) else {
        report.error = Some("no_spec".to_string());
        return report;
    };
    report.spec = Some(spec.clone());
    let spec_dir = match ClaudePaths::for_project(project).and_then(|p| p.for_spec(&spec)) {
        Ok(sp) => sp.dir().to_path_buf(),
        Err(e) => {
            report.error = Some(format!("spec path unresolved: {e}"));
            return report;
        }
    };
    // A typo'd slug must NOT materialise a phantom spec folder whose log nobody
    // ever renders — that is the silent loss this command exists to remove.
    if !spec_dir.is_dir() {
        report.error = Some("unknown_spec".to_string());
        return report;
    }
    let stage = read_stage(project, &spec);
    // A terminal spec belongs to the post-close amendment window, not here — the
    // observer twin is fail-CLOSED on non-`Active` for exactly that reason, and
    // this deliberate half must not diverge from its own sibling: two writers of
    // one record with different admission rules is how the two drift.
    if let Some(outcome) = mustard_core::domain::meta::read_meta(&spec_dir.join("meta.json"))
        .and_then(|m| m.outcome)
        .as_deref()
        .and_then(mustard_core::Outcome::parse)
    {
        if outcome != mustard_core::Outcome::Active {
            report.error = Some("spec_not_active".to_string());
            return report;
        }
    }
    let log_path = spec_dir.join(CHANGE_LOG_MD);
    // Count the bullets BEFORE writing. The landed-proof used to be substring
    // containment, which cannot tell "appended" from "was already there": the
    // same instruction sent twice reported success even if the second append
    // silently failed. A growth check answers the question actually being asked.
    let bullets_before = count_bullets(&log_path);

    // The bullet body marks the record as deliberate, so a reader (human or
    // agent) can tell the orchestrator's instruction from the raw prompt trail.
    let bullet_body = format!("**Instruction:** {instruction}");
    append_change_log_md(&project_dir, &spec, stage.as_deref(), &bullet_body);
    append_change_request(&project_dir, &spec, &session_id(), stage.as_deref(), &instruction);

    report.change_log = Some(log_path.display().to_string());
    // The writers are fail-open by contract; confirm rather than assume. Both
    // clauses matter: the log must have grown AND the instruction must be in it.
    let body = mfs::read_to_string(&log_path).unwrap_or_default();
    let landed = count_bullets(&log_path) > bullets_before && body.contains(&instruction);
    report.recorded = landed;
    report.ok = landed;
    if !landed {
        report.error = Some("write_failed".to_string());
    }
    report
}

/// CLI entry.
pub fn run(opts: ChangeRequestOpts) {
    let project = PathBuf::from(crate::shared::context::project_dir());
    let report = record(&project, &opts);
    let body = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
    println!("{body}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::agent::render::{render_prompt_at, RenderMode};
    use serde_json::json;
    use tempfile::tempdir;

    /// Plant a workspace anchor plus a spec directory with its `meta.json`.
    fn seed(dir: &Path, spec: &str) -> PathBuf {
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(dir.join("mustard.json"), b"{}").unwrap();
        let sp = ClaudePaths::for_project(dir).unwrap().for_spec(spec).unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        std::fs::write(
            sp.dir().join("meta.json"),
            json!({ "scope": "light", "stage": "Execute", "outcome": "Active" }).to_string(),
        )
        .unwrap();
        std::fs::write(sp.dir().join("spec.md"), "# Spec\n\n## Tasks\n\n- do the thing\n").unwrap();
        sp.dir().to_path_buf()
    }

    /// AC-1 — a registered instruction lands in the change log in the shape the
    /// per-wave renderer reads, and comes back out of the NEXT rendered prompt:
    /// no hand-formatted bullet anywhere in the path.
    #[test]
    fn change_request_instruction_reaches_the_next_prompt() {
        let dir = tempdir().unwrap();
        let spec_dir = seed(dir.path(), "my-feature");
        let instruction = "keep the observer blind and record the meaning beside it";

        let report = record(
            dir.path(),
            &ChangeRequestOpts {
                spec: Some("my-feature".to_string()),
                instruction: instruction.to_string(),
            },
        );
        assert!(report.ok, "unexpected error: {:?}", report.error);
        assert!(report.recorded, "the command must confirm the record landed");

        // The on-disk shape: a `- ` bullet, which is the ONLY thing the renderer
        // keeps out of the change log.
        let md = std::fs::read_to_string(spec_dir.join(CHANGE_LOG_MD)).unwrap();
        let bullet = md
            .lines()
            .find(|l| l.trim_start().starts_with("- ") && l.contains(instruction))
            .unwrap_or_else(|| panic!("no bullet carrying the instruction: {md}"));
        assert!(bullet.contains("_(Execute)_"), "stage tag carried: {bullet}");

        // The machine twin records it too, so the greppable trail stays complete.
        let ndjson = std::fs::read_to_string(spec_dir.join("change-requests.ndjson")).unwrap();
        assert!(ndjson.contains(instruction), "ndjson twin: {ndjson}");

        // And it reaches the next rendered prompt.
        let prompt = render_prompt_at(
            dir.path(),
            Some("my-feature"),
            None,
            "impl",
            Path::new("."),
            RenderMode::First,
            None,
            None,
            None,
        );
        assert!(prompt.contains("## CHANGE REQUESTS"), "section rendered: {prompt}");
        assert!(prompt.contains(instruction), "instruction reached the prompt: {prompt}");
    }

    /// AC-2 — an empty / whitespace-only instruction is refused and NOTHING is
    /// written, so the log cannot fill with entries that say nothing.
    #[test]
    fn change_request_refuses_an_empty_instruction() {
        for blank in ["", "   ", "\n\t "] {
            let dir = tempdir().unwrap();
            let spec_dir = seed(dir.path(), "my-feature");

            let report = record(
                dir.path(),
                &ChangeRequestOpts {
                    spec: Some("my-feature".to_string()),
                    instruction: blank.to_string(),
                },
            );
            assert!(!report.ok, "a blank instruction must be refused ({blank:?})");
            assert!(!report.recorded, "nothing may be recorded ({blank:?})");
            assert_eq!(report.error.as_deref(), Some("empty_instruction"));
            assert!(
                !spec_dir.join(CHANGE_LOG_MD).exists(),
                "the refusal must not even create the log ({blank:?})"
            );
            assert!(!spec_dir.join("change-requests.ndjson").exists());
        }

        // Two-sided: a real instruction against the same seed still records, so
        // the assertions above cannot pass by the command being inert.
        let dir = tempdir().unwrap();
        let spec_dir = seed(dir.path(), "my-feature");
        let report = record(
            dir.path(),
            &ChangeRequestOpts {
                spec: Some("my-feature".to_string()),
                instruction: "tighten the gate".to_string(),
            },
        );
        assert!(report.ok, "a real instruction still records: {:?}", report.error);
        assert!(std::fs::read_to_string(spec_dir.join(CHANGE_LOG_MD))
            .unwrap()
            .contains("tighten the gate"));
    }

    /// A slug with no directory is a typo, not a new spec: refuse instead of
    /// materialising a phantom log nobody renders.
    #[test]
    fn change_request_refuses_an_unknown_spec() {
        let dir = tempdir().unwrap();
        seed(dir.path(), "my-feature");
        let report = record(
            dir.path(),
            &ChangeRequestOpts {
                spec: Some("no-such-spec".to_string()),
                instruction: "do something".to_string(),
            },
        );
        assert_eq!(report.error.as_deref(), Some("unknown_spec"));
        assert!(!report.recorded);
    }
}
