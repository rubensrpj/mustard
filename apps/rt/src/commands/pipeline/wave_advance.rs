//! `mustard-rt run wave-advance` — composite dispatch face: the next pending
//! wave level, prompts already rendered inline.
//!
//! Composes, **in-process** (module-qualified, no subprocess):
//!
//! 1. `dispatch-plan` — [`crate::commands::pipeline::dispatch_plan::build_plan`]
//!    (the wave DAG + ordering, including the single-spec one-item fallback).
//! 2. `agent-prompt-render` — for each item of the next pending level, the
//!    prompt is rendered via
//!    [`crate::commands::agent::agent_prompt_render::render_prompt_ref_at`]:
//!    the full text is written to the spec's `.dispatch/` file and the item
//!    carries the 2-line `MUSTARD-PROMPT-REF` stub. The orchestrator passes
//!    the stub verbatim to `Task`; the `subagent_inject` PreToolUse hook
//!    expands it — the full prompt never transits the orchestrator's context
//!    (it used to be paid twice: once in this command's JSON, once again in
//!    the dispatch).
//!
//! ## "Next pending level" semantics
//!
//! A wave counts as **completed** when a `pipeline.wave.complete` event with
//! its wave number exists in the spec's per-spec NDJSON `.events/` log (the
//! same signal `emit-pipeline` writes and the resume projections fold). There
//! is **no reliable persisted "dispatched" signal** — `pipeline.task.dispatch`
//! is emitted by the orchestrator relay, not enforced — so this command
//! returns the items of the FIRST dependency level (ascending) that still has
//! a non-completed wave, filtered to its non-completed waves. Re-invoking
//! after dispatch but before the waves complete returns the same level again;
//! the caller owns not double-dispatching within a session. All impl waves
//! completed → the review round (below); no plan at all → empty array.
//!
//! ## Review round (post-impl)
//!
//! Once EVERY impl wave carries a `pipeline.wave.complete`, the advance does
//! not terminate at `[]` yet: it emits one `role: review` item (subagent
//! `mustard-review`) per **distinct subproject touched by the plan's waves**,
//! in alphabetical order, each with its prompt rendered inline (role `review`,
//! root `spec.md` — wave-less, so `wave: 0`). The "already reviewed" signal is
//! a `review.result` event of the spec whose payload names that subproject
//! (recorded by `mustard-rt run review-result --subproject <sub>`); an
//! absent/null payload `subproject` counts as `"."` — a whole-project review.
//!
//! Re-invocation semantics mirror the impl waves: calling `wave-advance` again
//! after dispatching the review round but BEFORE the verdicts are recorded
//! returns the same pending review items — the caller owns not
//! double-dispatching within a session. Each recorded `review.result` removes
//! its subproject from the round; once every touched subproject carries one,
//! the advance returns `[]` (terminal).
//!
//! ## Output
//!
//! A deterministic JSON array, one item per agent of the pending level:
//! `[{wave, role, subproject, subagent_type, prompt}]` — `prompt` is the
//! dispatch stub (`MUSTARD-PROMPT-REF` + fallback line), ready for the `Task`
//! tool verbatim; the full rendered text sits in the spec's `.dispatch/` file
//! the stub names. Fail-open: an unknown spec degrades to `[]` and a failed
//! stub write degrades to the full inline prompt; exit 0 always.

use crate::commands::agent::agent_prompt_render::{self, RenderMode};
use crate::commands::pipeline::dispatch_plan;
use mustard_core::domain::model::event::{HarnessEvent, EVENT_PIPELINE_WAVE_COMPLETE};
use mustard_core::io::claude_paths::ClaudePaths;
use mustard_core::view::projection::read_harness_events_from_ndjson_dir;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// One ready-to-dispatch agent of the pending level.
#[derive(Debug, Serialize)]
pub struct AdvanceItem {
    /// 1-based wave number (`0` marks the wave-less single-spec fallback).
    pub wave: u32,
    /// Role token (the `{role}` suffix of `wave-N-{role}`).
    pub role: String,
    /// Subproject path relative to the project root, or `"."`.
    pub subproject: String,
    /// The `subagent_type` to pass to `Task` (picked by the tool, never by hand).
    #[serde(rename = "subagent_type")]
    pub subagent_type: String,
    /// The dispatch stub (`MUSTARD-PROMPT-REF` line + fallback) — the
    /// orchestrator relays it straight to `Task`; the PreToolUse hook expands
    /// it to the full rendered prompt. Falls back to the full inline text
    /// when the stub file could not be written.
    pub prompt: String,
}

/// CLI entry — `mustard-rt run wave-advance --spec <slug>`.
pub fn run(spec: &str) {
    let project = PathBuf::from(crate::shared::context::project_dir());
    let items = advance(&project, spec);
    println!(
        "{}",
        serde_json::to_string_pretty(&items).unwrap_or_else(|_| "[]".to_string())
    );
}

/// The composite miolo against an explicit `project` root (testable without
/// mutating the process cwd). See the module docs for the pending-level
/// semantics.
pub(crate) fn advance(project: &Path, spec: &str) -> Vec<AdvanceItem> {
    let spec_dir = dispatch_plan::resolve_spec_dir(project, spec);
    let plan = dispatch_plan::build_plan(project, &spec_dir, spec, None);
    if plan.is_empty() {
        return Vec::new();
    }

    let events = spec_events(project, spec);
    let completed = completed_waves(&events, spec);
    let pending_level = plan
        .iter()
        .filter(|it| !completed.contains(&it.wave))
        .map(|it| it.level)
        .min();
    let Some(level) = pending_level else {
        // Every impl wave already carries a pipeline.wave.complete — emit the
        // review round before the terminal `[]` (see module docs).
        return review_round(project, spec, &plan, &events);
    };

    plan.into_iter()
        .filter(|it| it.level == level && !completed.contains(&it.wave))
        .map(|it| {
            // Wave 0 is the single-spec fallback: render the root spec.md
            // (no `--wave`), exactly like the prompt_cmd dispatch-plan emits.
            let wave_arg = (it.wave > 0).then_some(it.wave);
            let prompt = agent_prompt_render::render_prompt_ref_at(
                project,
                Some(spec),
                wave_arg,
                &it.role,
                Path::new(&it.subproject),
                RenderMode::First,
            );
            AdvanceItem {
                wave: it.wave,
                role: it.role,
                subproject: it.subproject,
                subagent_type: it.subagent_type,
                prompt,
            }
        })
        .collect()
}

/// Read the spec's per-spec NDJSON event log. Fail-open: a missing/unreadable
/// events dir yields the empty vec (every wave pending, nothing reviewed —
/// the conservative read). Single resolution shared by [`completed_waves`]
/// and [`reviewed_subprojects`].
fn spec_events(project: &Path, spec: &str) -> Vec<HarnessEvent> {
    let events_dir = ClaudePaths::for_project(project)
        .and_then(|p| p.for_spec(spec))
        .ok()
        .map_or_else(
            || {
                ClaudePaths::compose_unchecked(project)
                    .spec_dir()
                    .join(spec)
                    .join(".events")
            },
            |sp| sp.events_dir(),
        );
    read_harness_events_from_ndjson_dir(&events_dir)
}

/// The set of wave numbers carrying a `pipeline.wave.complete` event in the
/// spec's event log.
fn completed_waves(events: &[HarnessEvent], spec: &str) -> BTreeSet<u32> {
    events
        .iter()
        .filter(|e| e.event == EVENT_PIPELINE_WAVE_COMPLETE && e.spec.as_deref() == Some(spec))
        .filter_map(|e| {
            e.payload
                .get("wave")
                .and_then(Value::as_u64)
                .and_then(|w| u32::try_from(w).ok())
        })
        .collect()
}

/// Subprojects already covered by a `review.result` event of `spec`. The
/// payload's `subproject` field is the key; an absent/null/empty subproject
/// counts as `"."` (a whole-project review covers the root-level round item).
fn reviewed_subprojects(events: &[HarnessEvent], spec: &str) -> BTreeSet<String> {
    events
        .iter()
        .filter(|e| e.event == "review.result" && e.spec.as_deref() == Some(spec))
        .map(|e| {
            e.payload
                .get("subproject")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(".")
                .to_string()
        })
        .collect()
}

/// The post-impl review round: one `role: review` item per distinct subproject
/// touched by the plan's waves, alphabetical (`BTreeSet` order), minus the
/// subprojects already carrying a `review.result` (see module docs for the
/// re-invocation semantics). The prompt stub is rendered by the same
/// `agent-prompt-render` ref miolo the impl waves use — role `review`, wave-less
/// (the root `spec.md`), so the item carries `wave: 0` like the single-spec
/// fallback. `subagent_type` resolves through [`recommended_subagent_type`]
/// (`review` → `mustard-review`), never picked by hand.
///
/// [`recommended_subagent_type`]: agent_prompt_render::recommended_subagent_type
fn review_round(
    project: &Path,
    spec: &str,
    plan: &[dispatch_plan::DispatchItem],
    events: &[HarnessEvent],
) -> Vec<AdvanceItem> {
    let reviewed = reviewed_subprojects(events, spec);
    let touched: BTreeSet<String> = plan.iter().map(|it| it.subproject.clone()).collect();
    touched
        .into_iter()
        .filter(|sub| !reviewed.contains(sub))
        .map(|sub| {
            let prompt = agent_prompt_render::render_prompt_ref_at(
                project,
                Some(spec),
                None,
                "review",
                Path::new(&sub),
                RenderMode::First,
            );
            AdvanceItem {
                wave: 0,
                role: "review".to_string(),
                subproject: sub,
                subagent_type: agent_prompt_render::recommended_subagent_type("review")
                    .to_string(),
                prompt,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    /// Anchor a project root so `ClaudePaths::for_project` resolves (mirrors
    /// the dispatch_plan test helper).
    fn anchor(dir: &Path) {
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(dir.join("mustard.json"), b"{}").unwrap();
    }

    /// Seed a 3-wave spec: waves 1 and 2 are independent (level 0), wave 3
    /// depends on both (level 1). Each wave dir carries a spec.md with Tasks.
    fn seed_three_waves(project: &Path, slug: &str) -> PathBuf {
        let spec_dir = project.join(".claude").join("spec").join(slug);
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("wave-plan.md"),
            "\
| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave-1-rt]] | rt | — | base |
| 2 | [[wave-2-cli]] | cli | — | parallel base |
| 3 | [[wave-3-core]] | core | [[wave-1-rt]], [[wave-2-cli]] | joins both |
",
        )
        .unwrap();
        for (n, role) in [(1, "rt"), (2, "cli"), (3, "core")] {
            let dir = spec_dir.join(format!("wave-{n}-{role}"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("spec.md"),
                format!("# wave-{n}-{role}\n\n## Tasks\n\n- [ ] task for {role}\n"),
            )
            .unwrap();
        }
        spec_dir
    }

    /// Emit a `pipeline.wave.complete` for `wave` into the spec's events log.
    fn complete_wave(project: &Path, spec: &str, wave: u32) {
        use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
        let event = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: format!("2026-06-09T00:00:0{wave}.000Z"),
            session_id: "test-session".to_string(),
            wave,
            actor: Actor {
                kind: ActorKind::Orchestrator,
                id: Some("emit-pipeline".to_string()),
                actor_type: None,
            },
            event: EVENT_PIPELINE_WAVE_COMPLETE.to_string(),
            payload: json!({ "wave": wave }),
            spec: Some(spec.to_string()),
        };
        crate::shared::events::route::emit(project.to_str().unwrap(), &event);
    }

    /// Resolve a dispatch stub to the rendered body it references: extract
    /// the `MUSTARD-PROMPT-REF` line and read the file under `project`.
    fn stub_body(project: &Path, prompt: &str) -> String {
        let rel = prompt
            .lines()
            .find_map(|l| l.trim().strip_prefix("MUSTARD-PROMPT-REF:"))
            .unwrap_or_else(|| panic!("prompt is not a dispatch stub: {prompt}"))
            .trim()
            .to_string();
        std::fs::read_to_string(project.join(rel)).expect("stub file readable")
    }

    /// Happy path: with no wave completed, the first level (the two parallel
    /// waves 1 and 2) comes back, each carrying a dispatch stub whose file
    /// holds the full rendered prompt.
    #[test]
    fn composite_wave_advance_returns_first_level_with_prompt_stubs() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let project = dir.path();
        seed_three_waves(project, "adv");

        let items = advance(project, "adv");
        assert_eq!(items.len(), 2, "level 0 carries the two parallel waves");
        assert_eq!(items[0].wave, 1);
        assert_eq!(items[0].role, "rt");
        assert_eq!(items[1].wave, 2);
        assert_eq!(items[1].role, "cli");
        for item in &items {
            assert_eq!(item.subagent_type, "general-purpose");
            assert!(
                item.prompt.starts_with("MUSTARD-PROMPT-REF:"),
                "prompt must be the dispatch stub: {}",
                item.prompt
            );
            let body = stub_body(project, &item.prompt);
            assert!(
                body.contains(&format!("task for {}", item.role)),
                "stub file must hold the wave's task body: {body}"
            );
            assert!(
                !item.prompt.contains("agent-prompt-render"),
                "prompt must not be a prompt_cmd shell line"
            );
        }
    }

    /// Emit a `review.result` for `spec` into the spec's events log, optionally
    /// naming a subproject (mirrors `review-result --subproject`).
    fn record_review(project: &Path, spec: &str, subproject: Option<&str>, ts_suffix: u32) {
        use mustard_core::domain::model::event::{Actor, ActorKind, SCHEMA_VERSION};
        let event = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: format!("2026-06-09T01:00:0{ts_suffix}.000Z"),
            session_id: "test-session".to_string(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Cli,
                id: Some("review-result".to_string()),
                actor_type: None,
            },
            event: "review.result".to_string(),
            payload: json!({
                "spec": spec,
                "verdict": "approved",
                "criticalCount": 0,
                "subproject": subproject,
            }),
            spec: Some(spec.to_string()),
        };
        crate::shared::events::route::emit(project.to_str().unwrap(), &event);
    }

    /// Dependency progression: completing waves 1 and 2 advances the pending
    /// level to wave 3; completing everything yields the review round, and a
    /// recorded `review.result` drains the advance to the terminal empty array.
    #[test]
    fn composite_wave_advance_progresses_levels_and_drains() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let project = dir.path();
        seed_three_waves(project, "adv2");

        // Wave 1 done, wave 2 still pending → level 0 again, only wave 2.
        complete_wave(project, "adv2", 1);
        let items = advance(project, "adv2");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].wave, 2, "non-completed level-0 wave still pending");

        // Both level-0 waves done → level 1 (wave 3).
        complete_wave(project, "adv2", 2);
        let items = advance(project, "adv2");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].wave, 3);
        assert_eq!(items[0].role, "core");

        // Everything done → the review round (one item: the seeded waves all
        // converge on subproject ".").
        complete_wave(project, "adv2", 3);
        let items = advance(project, "adv2");
        assert_eq!(items.len(), 1, "review round expected after all impl waves");
        assert_eq!(items[0].role, "review");

        // A recorded review.result (no subproject → covers ".") drains it.
        record_review(project, "adv2", None, 1);
        let items = advance(project, "adv2");
        assert!(items.is_empty(), "reviewed spec returns the empty list");
    }

    /// Degraded: an unknown spec (no dir, no spec.md) degrades to `[]`.
    #[test]
    fn composite_wave_advance_unknown_spec_degrades_empty() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        assert!(advance(dir.path(), "ghost").is_empty());
    }

    /// Single-spec fallback (no wave plan): one wave-0 `impl` item whose
    /// prompt renders the root spec.md (no `--wave` semantics).
    #[test]
    fn composite_wave_advance_single_spec_renders_root() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let project = dir.path();
        let spec_dir = project.join(".claude").join("spec").join("flat");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("spec.md"),
            "# Flat\n\n## Tasks\n\n- [ ] the only task\n",
        )
        .unwrap();

        let items = advance(project, "flat");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].wave, 0);
        assert_eq!(items[0].role, "impl");
        let body = stub_body(project, &items[0].prompt);
        assert!(
            body.contains("the only task"),
            "root spec.md tasks must reach the rendered prompt: {body}"
        );
    }

    /// Seed a 3-wave spec whose waves declare `## Files` in DISTINCT
    /// subprojects, deliberately out of alphabetical order (rt, core, cli) so
    /// the review-round ordering assertion is meaningful.
    fn seed_three_waves_with_subprojects(project: &Path, slug: &str) {
        let spec_dir = project.join(".claude").join("spec").join(slug);
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("wave-plan.md"),
            "\
| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave-1-rt]] | rt | — | base |
| 2 | [[wave-2-core]] | core | [[wave-1-rt]] | uses base |
| 3 | [[wave-3-cli]] | cli | [[wave-2-core]] | wires cli |
",
        )
        .unwrap();
        for (n, role, file) in [
            (1, "rt", "apps/rt/src/foo.rs"),
            (2, "core", "packages/core/src/lib.rs"),
            (3, "cli", "apps/cli/src/main.rs"),
        ] {
            let dir = spec_dir.join(format!("wave-{n}-{role}"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("spec.md"),
                format!("# wave-{n}-{role}\n\n## Files\n- {file}\n\n## Tasks\n\n- [ ] task for {role}\n"),
            )
            .unwrap();
        }
    }

    /// Post-impl review round: once every impl wave is complete, the advance
    /// emits one `role: review` item per distinct subproject, alphabetically,
    /// each locked to `mustard-review` with a stub-referenced review prompt.
    #[test]
    fn wave_advance_review_round_emitted_after_all_impl_complete() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let project = dir.path();
        seed_three_waves_with_subprojects(project, "rev");
        for w in 1..=3 {
            complete_wave(project, "rev", w);
        }

        let items = advance(project, "rev");
        assert_eq!(items.len(), 3, "one review item per distinct subproject");
        // Alphabetical, regardless of the wave order that touched them.
        let subs: Vec<&str> = items.iter().map(|i| i.subproject.as_str()).collect();
        assert_eq!(subs, vec!["apps/cli", "apps/rt", "packages/core"]);
        let mut stub_paths = std::collections::BTreeSet::new();
        for item in &items {
            assert_eq!(item.role, "review");
            assert_eq!(item.subagent_type, "mustard-review");
            assert_eq!(item.wave, 0, "review round is wave-less (root spec render)");
            let body = stub_body(project, &item.prompt);
            assert!(
                body.contains("ROLE: review"),
                "stub file must carry the review role contract: {body}"
            );
            // Same spec/wave/role across subprojects — the subproject slug in
            // the filename must keep the three stub files distinct.
            let rel = item.prompt.lines().next().unwrap_or("").to_string();
            assert!(stub_paths.insert(rel), "review stub files must not collide: {items:?}");
        }
    }

    /// Re-invocation: a `review.result` naming a subproject removes it from
    /// the round; once every touched subproject carries one, the advance is
    /// terminal (`[]`). Until then, re-invoking returns the same pending items.
    #[test]
    fn wave_advance_review_round_not_reemitted_once_reviewed() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let project = dir.path();
        seed_three_waves_with_subprojects(project, "rev2");
        for w in 1..=3 {
            complete_wave(project, "rev2", w);
        }

        // Partial coverage: apps/rt reviewed → only the other two remain.
        record_review(project, "rev2", Some("apps/rt"), 1);
        let items = advance(project, "rev2");
        let subs: Vec<&str> = items.iter().map(|i| i.subproject.as_str()).collect();
        assert_eq!(subs, vec!["apps/cli", "packages/core"]);

        // Re-invoking without new verdicts returns the same pending round.
        let again = advance(project, "rev2");
        let subs_again: Vec<&str> = again.iter().map(|i| i.subproject.as_str()).collect();
        assert_eq!(subs_again, subs, "pending review items must be stable");

        // Full coverage → terminal empty array.
        record_review(project, "rev2", Some("apps/cli"), 2);
        record_review(project, "rev2", Some("packages/core"), 3);
        assert!(advance(project, "rev2").is_empty());
    }

    /// The single-spec fallback (no wave plan, wave 0) also gets the review
    /// round once its impl item completes — TF/Light specs are not exempt.
    #[test]
    fn wave_advance_review_round_covers_single_spec() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let project = dir.path();
        let spec_dir = project.join(".claude").join("spec").join("rev3");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("spec.md"),
            "# Flat\n\n## Files\n- apps/rt/src/foo.rs\n\n## Tasks\n\n- [ ] the only task\n",
        )
        .unwrap();

        complete_wave(project, "rev3", 0);
        let items = advance(project, "rev3");
        assert_eq!(items.len(), 1, "single spec gets exactly one review item");
        assert_eq!(items[0].role, "review");
        assert_eq!(items[0].subagent_type, "mustard-review");
        assert_eq!(items[0].subproject, "apps/rt");

        record_review(project, "rev3", Some("apps/rt"), 1);
        assert!(advance(project, "rev3").is_empty());
    }
}
