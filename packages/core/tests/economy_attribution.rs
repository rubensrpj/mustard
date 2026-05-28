#![allow(clippy::unwrap_used)]
//! Integration tests for the economy attribution roll-ups (post-W7A NDJSON).
//!
//! W7C of [[2026-05-26-no-sqlite-git-source-of-truth]] rewrote these tests
//! after the SQLite reader was retired. Each test plants NDJSON
//! `pipeline.economy.run` events under a fresh tempdir and asserts the new
//! path-based readers (`per_agent_costs`, `per_spec_costs`,
//! `per_wave_costs`) group them as the dashboard expects.

use mustard_core::domain::economy::{
    per_agent_costs, per_spec_costs, per_wave_costs, EconomyScope,
};
use mustard_core::domain::economy::scope::{ProjectPath, SpecId, WaveId};
use serde_json::json;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

/// Plant NDJSON `lines` at `<root>/.claude/spec/{spec}/.events/seed.ndjson`.
fn plant_events(root: &Path, spec: &str, lines: &[String]) {
    let dir = root.join(".claude").join("spec").join(spec).join(".events");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("seed.ndjson"), lines.join("\n")).unwrap();
}

/// Build one `pipeline.economy.run` NDJSON line carrying full attribution.
fn run_line(
    spec: &str,
    wave: Option<&str>,
    agent: Option<&str>,
    cost: i64,
    tokens: i64,
) -> String {
    let payload = json!({
        "spec": spec,
        "wave_id": wave,
        "agent_id": agent,
        "cost_usd_micros": cost,
        "input_tokens": tokens,
        "output_tokens": 0,
        "session_id": "sess-1",
        "started_at": 0i64,
    });
    json!({
        "kind": "pipeline.economy.run",
        "event": "pipeline.economy.run",
        "payload": payload,
    })
    .to_string()
}

#[test]
fn agent_rollup_single() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    plant_events(
        root,
        "spec-A",
        &[run_line("spec-A", Some("wave-1"), Some("core-impl"), 10_000, 500)],
    );

    let rows = per_agent_costs(root, EconomyScope::Project(ProjectPath::new(root))).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].agent_id.0, "core-impl");
    assert_eq!(rows[0].cost_usd_micros, 10_000);
    assert_eq!(rows[0].tokens, 500);
    assert_eq!(rows[0].span_count, 1);
}

#[test]
fn agent_rollup_excludes_unattributed() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    plant_events(
        root,
        "spec-F",
        &[
            // No agent_id — must be excluded.
            run_line("spec-F", Some("wave-2"), None, 7_500, 300),
            // Attributed.
            run_line("spec-F", Some("wave-2"), Some("core-explore"), 7_500, 300),
        ],
    );

    let rows = per_agent_costs(root, EconomyScope::Project(ProjectPath::new(root))).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].agent_id.0, "core-explore");
    assert_eq!(rows[0].cost_usd_micros, 7_500);
}

#[test]
fn empty_all_projects() {
    // AC-4: AllProjects scope with zero entries returns Vec empty, no error.
    let dir = tempdir().unwrap();
    let scope = EconomyScope::AllProjects(vec![]);
    let rows = per_agent_costs(dir.path(), scope).unwrap();
    assert!(rows.is_empty());

    let rows = per_spec_costs(dir.path(), EconomyScope::AllProjects(vec![])).unwrap();
    assert!(rows.is_empty());

    let rows = per_wave_costs(dir.path(), EconomyScope::AllProjects(vec![])).unwrap();
    assert!(rows.is_empty());
}

#[test]
fn per_spec_aggregation() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    plant_events(
        root,
        "all",
        &[
            run_line("spec-X", Some("w1"), Some("agent-x"), 1_000, 100),
            run_line("spec-X", Some("w1"), Some("agent-x"), 2_000, 200),
            run_line("spec-Y", Some("w1"), Some("agent-y"), 5_000, 500),
        ],
    );

    let by_spec = per_spec_costs(root, EconomyScope::Project(ProjectPath::new(root))).unwrap();
    assert_eq!(by_spec.len(), 2);
    // Both have started_at=0 so cost desc tiebreaks — spec-Y first.
    assert_eq!(by_spec[0].spec_id.0, "spec-Y");
    assert_eq!(by_spec[0].cost_usd_micros, 5_000);
    assert_eq!(by_spec[0].span_count, 1);
    assert_eq!(by_spec[1].spec_id.0, "spec-X");
    assert_eq!(by_spec[1].cost_usd_micros, 3_000);
    assert_eq!(by_spec[1].span_count, 2);
}

#[test]
fn per_wave_aggregation() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    plant_events(
        root,
        "spec-W",
        &[
            run_line("spec-W", Some("wave-alpha"), Some("core-impl"), 1_200, 120),
            run_line("spec-W", Some("wave-beta"), Some("core-impl"), 3_400, 340),
        ],
    );

    let by_wave =
        per_wave_costs(root, EconomyScope::Project(ProjectPath::new(root))).unwrap();
    assert_eq!(by_wave.len(), 2);
    // Sorted by cost desc: wave-beta first.
    assert_eq!(by_wave[0].wave_id.0, "wave-beta");
    assert_eq!(by_wave[0].cost_usd_micros, 3_400);
    assert_eq!(by_wave[1].wave_id.0, "wave-alpha");
    assert_eq!(by_wave[1].cost_usd_micros, 1_200);

    // Wave-scoped filter narrows to one wave.
    let wave_scope = EconomyScope::Wave {
        project: ProjectPath::new(root),
        spec: SpecId::new("spec-W"),
        wave: WaveId::new("wave-alpha"),
    };
    let scoped = per_wave_costs(root, wave_scope).unwrap();
    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0].wave_id.0, "wave-alpha");
    assert_eq!(scoped[0].cost_usd_micros, 1_200);
}

/// Regression for AC-6: a run dispatched on a parent spec's session that
/// carries the child wave it was launched against must roll up against the
/// child wave (the run payload's own `wave_id`), not the parent.
#[test]
fn parent_spec_child_wave_attribution() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    plant_events(
        root,
        "parent-spec",
        &[run_line("parent-spec", Some("child-wave"), Some("core-impl"), 8_888, 444)],
    );

    let scope = EconomyScope::Project(ProjectPath::new(root));
    let by_wave = per_wave_costs(root, scope.clone()).unwrap();
    assert_eq!(by_wave.len(), 1);
    assert_eq!(by_wave[0].spec_id.0, "parent-spec");
    assert_eq!(
        by_wave[0].wave_id.0, "child-wave",
        "the run row's own wave_id must drive the wave attribution"
    );

    let by_spec = per_spec_costs(root, scope.clone()).unwrap();
    assert_eq!(by_spec.len(), 1);
    assert_eq!(by_spec[0].spec_id.0, "parent-spec");
    assert_eq!(by_spec[0].cost_usd_micros, 8_888);

    let by_agent = per_agent_costs(root, scope).unwrap();
    assert_eq!(by_agent.len(), 1);
    assert_eq!(by_agent[0].agent_id.0, "core-impl");
    assert_eq!(by_agent[0].cost_usd_micros, 8_888);
}
