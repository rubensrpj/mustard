//! `mustard-rt run spec-children-tree --spec NAME` — a single round-trip
//! projection of a parent spec's **waves**, **acceptance criteria** and
//! **sub-specs**, consumed by the dashboard's `spec_children_tree` Tauri
//! command (Wave 3 of `spec-lifecycle-unification`).
//!
//! Why one subcommand
//! ------------------
//!
//! The dashboard previously fanned out three separate calls (`waves`,
//! `quality`, `spec-children`) to render a single spec drill-down. This
//! subcommand folds the three projections into one document so the UI pays a
//! single IPC round-trip:
//!
//! ```json
//! {
//!   "spec": "<parent>",
//!   "waves":    [ WaveChild,  … ],
//!   "acs":      [ AcChild,    … ],
//!   "subspecs": [ SpecChild,  … ]
//! }
//! ```
//!
//! All three sub-projections come straight from the canonical
//! [`mustard_core`] reader layer ([`SpecReader::waves`], [`SpecReader::quality`])
//! plus the cross-developer UNION used by [`crate::run::spec_children`] for
//! sub-spec discovery (events + filesystem `### Parent:` headers). Reusing the
//! reader keeps this byte-stable with every other dashboard surface — no SQL
//! drift.
//!
//! Fail-open: a missing event store, missing parent dir, or unreadable spec all
//! degrade to empty arrays — never a non-zero exit.

use crate::run::env::project_dir;
use crate::run::spec_children::{list_children, ChildEntry};
use mustard_core::{
    AcStatus, Outcome, SpecChild, SpecReader, SpecState, SpecStatus, SqliteSpecReader, Stage,
    WaveStatus, WaveView,
};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// One wave row in the children tree. Field shapes per Wave 2 task #2.
///
/// `idx` is the 1-based wave number, `role` the wave-plan role tag (empty when
/// the plan declared none), `status` the canonical [`WaveStatus`].
#[derive(Debug, Clone, Serialize)]
pub struct WaveChild {
    /// 1-based wave number (matches the spec's `### Wave N` headings).
    pub idx: u32,
    /// Role tag from the wave plan (`api`, `ui`, `rt`, …). Empty string when
    /// the wave plan did not declare one.
    pub role: String,
    /// Canonical lifecycle status of the wave.
    pub status: WaveStatus,
    /// ISO-8601 of the wave's first `pipeline.task.dispatch`.
    pub started_at: Option<String>,
    /// ISO-8601 of `pipeline.wave.complete` (or `pipeline.wave.failed`).
    pub completed_at: Option<String>,
    /// `completed_at - started_at` in milliseconds. `None` until both are set.
    pub duration_ms: Option<i64>,
}

impl From<WaveView> for WaveChild {
    fn from(w: WaveView) -> Self {
        Self {
            idx: w.wave,
            role: w.role.unwrap_or_default(),
            status: w.status,
            started_at: w.started_at,
            completed_at: w.completed_at,
            duration_ms: w.duration_ms,
        }
    }
}

/// One acceptance-criterion row. Field shapes per Wave 2 task #3.
///
/// `evidence` is a summarised stdout/stderr excerpt of the AC's pass/fail run
/// — the `fail_reason` the core quality projection captured from the latest
/// `qa.result` event (capped by the projection, never displayed inline).
#[derive(Debug, Clone, Serialize)]
pub struct AcChild {
    /// Identifier from the spec (`AC-1`, `AC-W2-4`, …).
    pub id: String,
    /// Human-readable label (the text after `AC-N:` in the spec).
    pub label: String,
    /// Latest known status.
    pub status: AcStatus,
    /// ISO-8601 of the most recent `qa.result` event that touched this AC.
    pub last_run_at: Option<String>,
    /// Summarised stdout/stderr of the AC's pass/fail run, when one is known.
    pub evidence: Option<String>,
}

/// The full projection returned by [`build_tree`] and serialised by [`run`].
#[derive(Debug, Clone, Serialize)]
pub struct ChildrenTree {
    /// Parent spec slug this tree is rooted at.
    pub spec: String,
    /// Per-wave rows, ordered by wave number.
    pub waves: Vec<WaveChild>,
    /// Per-AC rows, ordered by spec id.
    pub acs: Vec<AcChild>,
    /// Linked sub-specs (UNION of `spec.link` events + `### Parent:` headers).
    pub subspecs: Vec<SpecChild>,
}

/// Derive a [`SpecState`] from a kebab-case status token (the spelling carried
/// on [`ChildEntry::status`] and the on-disk `### Status:` / `### Stage:`
/// header). The token may be a [`Stage`] spelling (new format) or a legacy
/// flat status; both resolve via the tolerant parsers in `mustard_core`.
///
/// Fail-open: an unrecognised token degrades to `Plan` + `Active`, the same
/// earliest-meaningful state the core projection uses for an empty stream.
fn state_from_kebab(status: &str) -> SpecState {
    // A terminal outcome (`completed`/`cancelled`/`abandoned`) pins the stage
    // to CLOSE; otherwise the token is a stage spelling and the spec is active.
    if let Some(outcome) = Outcome::parse(status) {
        if outcome != Outcome::Active {
            // SpecState::new enforces terminal-outcome ⇒ Stage::Close.
            return SpecState::new(Stage::Close, outcome, Default::default())
                .unwrap_or_else(|_| fallback_state());
        }
    }
    let stage = Stage::parse(status).unwrap_or(Stage::Plan);
    SpecState::new(stage, Outcome::Active, Default::default())
        .unwrap_or_else(|_| fallback_state())
}

/// The earliest-meaningful state — used when a triple cannot be constructed.
fn fallback_state() -> SpecState {
    SpecState::new(Stage::Plan, Outcome::Active, Default::default())
        .unwrap_or(SpecState {
            stage: Stage::Plan,
            outcome: Outcome::Active,
            flags: Default::default(),
        })
}

/// Convert a UNION [`ChildEntry`] into the core [`SpecChild`] shape. The
/// `status` is derived as the kebab-case [`Stage`] of the resolved
/// [`SpecState`], matching the contract: "`status` inside `subspecs` is the
/// `Stage` of the sub-spec (kebab-case)".
#[allow(deprecated)] // populates the derived legacy `status` field on SpecChild.
fn child_from_entry(entry: ChildEntry) -> SpecChild {
    let state = state_from_kebab(&entry.status);
    SpecChild {
        spec: entry.spec,
        // Legacy flat status, derived (lossy) from the canonical state. Kept
        // populated during the W1→W7 back-compat window.
        status: SpecStatus::try_from(state.clone()).unwrap_or(SpecStatus::NoEvents),
        state,
        started_at: entry.started_at,
        completed_at: entry.completed_at,
        reason: entry.reason,
    }
}

/// Build the children tree for `spec` under `project`. Pure projection — no
/// stdout, no process exit. Fail-open at every source: a reader that cannot
/// open contributes empty waves/acs, and sub-spec discovery degrades to `[]`.
#[must_use]
pub fn build_tree(project: &Path, spec: &str) -> ChildrenTree {
    let (waves, acs) = match SqliteSpecReader::for_project(project) {
        Ok(reader) => {
            let waves: Vec<WaveChild> = reader
                .waves(spec)
                .unwrap_or_default()
                .into_iter()
                .map(WaveChild::from)
                .collect();
            let acs: Vec<AcChild> = reader
                .quality(spec)
                .map(|q| {
                    q.criteria
                        .into_iter()
                        .map(|c| AcChild {
                            id: c.id,
                            label: c.label,
                            status: c.status,
                            last_run_at: c.last_run_at,
                            evidence: c.fail_reason,
                        })
                        .collect()
                })
                .unwrap_or_default();
            (waves, acs)
        }
        Err(_) => (Vec::new(), Vec::new()),
    };

    // Sub-specs: reuse the cross-developer UNION (events + `### Parent:`
    // headers) so a tactical-fix linked only via its filesystem header still
    // surfaces. `list_children` is already sorted by slug.
    let subspecs: Vec<SpecChild> = list_children(project, spec)
        .into_iter()
        .map(child_from_entry)
        .collect();

    ChildrenTree {
        spec: spec.to_string(),
        waves,
        acs,
        subspecs,
    }
}

/// Dispatch `mustard-rt run spec-children-tree --spec NAME`. Emits the
/// [`ChildrenTree`] as pretty JSON to stdout. Fail-open: a missing `--spec`
/// prints an empty tree and exit `0`.
pub fn run(spec: Option<&str>) {
    let project = PathBuf::from(project_dir());
    let tree = match spec {
        Some(s) if !s.is_empty() => build_tree(&project, s),
        _ => {
            eprintln!("Usage: mustard-rt run spec-children-tree --spec <name>");
            ChildrenTree {
                spec: String::new(),
                waves: Vec::new(),
                acs: Vec::new(),
                subspecs: Vec::new(),
            }
        }
    };
    match serde_json::to_string_pretty(&tree) {
        Ok(text) => println!("{text}"),
        Err(_) => println!("{{}}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_from_kebab_maps_stage_spellings() {
        assert_eq!(state_from_kebab("execute").stage, Stage::Execute);
        assert_eq!(state_from_kebab("qa-review").stage, Stage::QaReview);
        assert_eq!(state_from_kebab("plan").stage, Stage::Plan);
        // Legacy synonyms resolve too.
        assert_eq!(state_from_kebab("implementing").stage, Stage::Execute);
        assert_eq!(state_from_kebab("reviewing").stage, Stage::QaReview);
    }

    #[test]
    fn state_from_kebab_maps_terminal_outcomes_to_close() {
        let s = state_from_kebab("completed");
        assert_eq!(s.stage, Stage::Close);
        assert_eq!(s.outcome, Outcome::Completed);
        assert_eq!(state_from_kebab("cancelled").outcome, Outcome::Cancelled);
    }

    #[test]
    fn state_from_kebab_unknown_falls_back_to_plan_active() {
        let s = state_from_kebab("garbage-token");
        assert_eq!(s.stage, Stage::Plan);
        assert_eq!(s.outcome, Outcome::Active);
    }

    #[test]
    fn wave_child_from_view_carries_idx_and_role() {
        let mut wv = WaveView::queued(2);
        wv.role = Some("rt".to_string());
        wv.status = WaveStatus::InProgress;
        let wc = WaveChild::from(wv);
        assert_eq!(wc.idx, 2);
        assert_eq!(wc.role, "rt");
        assert_eq!(wc.status, WaveStatus::InProgress);
    }

    #[test]
    fn wave_child_empty_role_when_none() {
        let wc = WaveChild::from(WaveView::queued(1));
        assert_eq!(wc.role, "");
    }
}
