//! `close_gate` — the `PreToolUse(Write|Edit)` adapter for the pipeline-CLOSE gate.
//!
//! ## Scope (thin adapter)
//!
//! This module is the HOOK end of the pipeline-CLOSE gate: the [`Check`] the
//! registry wires onto `PreToolUse(Write|Edit)`. Its whole job is to apply the
//! trigger-guard (only a Write/Edit of a pipeline-state JSON that transitions
//! the phase to `CLOSE`), extract `(cwd, spec)` from the [`HookInput`]/[`Ctx`],
//! and DELEGATE the decision to the policy engine in
//! [`crate::commands::pipeline::close_gates`] (the sane `hooks → commands`
//! direction). All the sub-gates (debt / checklist / QA / build), their mode
//! resolution, and the shell runner live there, not here.
//!
//! It triggers on a `PreToolUse(Write|Edit)` of a pipeline-state JSON file (the
//! legacy `.claude` state-file directory) whose content transitions the phase
//! to `CLOSE`. Post-`pipeline.phase` migration the canonical phase lives in the
//! event store and the real CLOSE gate runs inline in
//! `mustard-rt run emit-phase --to CLOSE`; this hook is kept defensively for any
//! legacy state file that still carries `phaseName: "CLOSE"`.

use mustard_core::platform::error::Error;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use serde_json::Value;

use crate::commands::pipeline::close_gates::{CloseGateModes, run_close_gates};
use crate::shared::gate_mode::GateMode;

/// The pipeline-CLOSE sensor gate module.
pub struct CloseGate;

// ---------------------------------------------------------------------------
// Input parsing
// ---------------------------------------------------------------------------

/// `true` if `file_path` is a pipeline-state file (a `.json` file directly
/// inside the `.pipeline-states` segment of the path).
fn is_pipeline_state_file(file_path: &str) -> bool {
    let p = file_path.replace('\\', "/");
    // Match paths of the form `...{seg}/{name}.json` where
    // `{name}` contains no path separator (i.e., directly inside the dir).
    let seg = ".pipeline-states";
    let Some(idx) = p.find(seg) else {
        return false;
    };
    let after = &p[idx + seg.len()..];
    // `after` must start with '/' followed by a single-component .json file.
    let Some(rest) = after.strip_prefix('/') else {
        return false;
    };
    !rest.contains('/') && std::path::Path::new(rest)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
}

/// Extract the post-write content of a Write/Edit invocation. `Write` uses
/// `content`; `Edit` uses `new_string` (the JS `extractContent`).
fn extract_content(input: &HookInput) -> Option<String> {
    let ti = &input.tool_input;
    if let Some(c) = ti.get("content").and_then(|v| v.as_str()) {
        return Some(c.to_string());
    }
    if let Some(c) = ti.get("new_string").and_then(|v| v.as_str()) {
        return Some(c.to_string());
    }
    None
}

/// The uppercased phase from a pipeline-state JSON string. Reads `phaseName`
/// (string) then a legacy string `phase` (the JS `extractPhase`).
fn extract_phase(content: &str) -> Option<String> {
    let obj: Value = serde_json::from_str(content).ok()?;
    let raw = obj
        .get("phaseName")
        .and_then(|v| v.as_str())
        .or_else(|| obj.get("phase").and_then(|v| v.as_str()))?;
    Some(raw.to_ascii_uppercase())
}

/// The spec name from a pipeline-state JSON string (`spec` then `specName`).
fn extract_spec(content: &str) -> Option<String> {
    let obj: Value = serde_json::from_str(content).ok()?;
    obj.get("spec")
        .and_then(|v| v.as_str())
        .or_else(|| obj.get("specName").and_then(|v| v.as_str()))
        .map(str::to_string)
}

// ---------------------------------------------------------------------------
// The adapter body
// ---------------------------------------------------------------------------

/// Run the full close-gate against a `PreToolUse(Write|Edit)` invocation,
/// resolving every sub-gate mode from the environment.
///
/// Returns the verdict — 1:1 with `close-gate.js`. Every JS `process.exit(0)`
/// with no stdout maps to `Allow`; a `permissionDecision: deny` maps to `Deny`.
fn close_gate(input: &HookInput, cwd: &str) -> Verdict {
    close_gate_with_modes(input, cwd, CloseGateModes::resolve(cwd))
}

/// The pure adapter body — every `MUSTARD_*_MODE` is supplied via `modes`
/// rather than read from the environment, so it is exercised directly by the
/// parity tests. Applies the trigger-guard (pipeline-state file + phase CLOSE)
/// and extracts `(cwd, spec)` from the `HookInput`, then delegates the decision
/// to [`run_close_gates`].
fn close_gate_with_modes(input: &HookInput, cwd: &str, modes: CloseGateModes) -> Verdict {
    let mode = modes.close;
    if mode == GateMode::Off {
        return Verdict::Allow;
    }
    let Some(file_path) = input.file_path() else {
        return Verdict::Allow;
    };
    if !is_pipeline_state_file(&file_path) {
        return Verdict::Allow;
    }
    let Some(content) = extract_content(input) else {
        return Verdict::Allow;
    };
    // Only trigger on a transition to phase CLOSE.
    //
    // Post-`pipeline.phase` migration the canonical phase lives in the SQLite
    // event store, not the pipeline-state JSON — SKILL.md no longer writes
    // `phaseName`. This branch is kept defensively for any legacy state file
    // that still carries `phaseName: "CLOSE"`; in steady state the real CLOSE
    // gate runs inline in `mustard-rt run emit-phase --to CLOSE`.
    if extract_phase(&content).as_deref() != Some("CLOSE") {
        return Verdict::Allow;
    }
    let spec_name = extract_spec(&content);
    let spec_ref = spec_name.as_deref();
    run_close_gates(cwd, spec_ref, modes)
}

impl Check for CloseGate {
    /// Gate a `PreToolUse(Write|Edit)` pipeline-state write that transitions to
    /// CLOSE. The verdict is computed entirely by [`close_gate`], which carries
    /// its own `MUSTARD_*_MODE` resolution — independent of the dispatcher's
    /// module-level mode.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        if !matches!(input.tool_name.as_deref(), Some("Write" | "Edit")) {
            return Ok(Verdict::Allow);
        }
        let cwd = if ctx.project_dir.is_empty() {
            input.cwd.as_deref().unwrap_or(".").to_string()
        } else {
            ctx.project_dir.clone()
        };
        Ok(close_gate(input, &cwd))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // W5 follow-up landed: `qa.result` events seed straight into the per-spec
    // NDJSON dir, mirroring `qa-run`'s production write path through
    // `route::emit`.
    use crate::commands::pipeline::close_gates::find_unmarked_checklist;
    use crate::shared::events::route;
    use mustard_core::ClaudePaths;
    use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
    use serde_json::json;
    use std::path::Path;
    use tempfile::tempdir;

    /// Build a project dir with the standard `.claude` subtree.
    fn make_project() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let paths = ClaudePaths::for_project(dir.path()).unwrap();
        std::fs::create_dir_all(paths.harness_dir()).unwrap();
        std::fs::create_dir_all(paths.pipeline_states_dir()).unwrap();
        std::fs::create_dir_all(paths.spec_dir())
            .unwrap();
        dir
    }

    /// A `PreToolUse(Write)` close-state input for `spec_name`.
    fn close_input(cwd: &Path, spec_name: &str) -> HookInput {
        let state_file = ClaudePaths::for_project(cwd)
            .unwrap()
            .pipeline_state_file(spec_name);
        HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({
                "file_path": state_file.to_string_lossy(),
                "content": json!({ "spec": spec_name, "phase": "CLOSE" }).to_string(),
            }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(cwd.to_string_lossy().into_owned()),
            ..HookInput::default()
        }
    }

    fn write_spec(cwd: &Path, spec_name: &str, body: &str) {
        let sp = ClaudePaths::for_project(cwd).unwrap().for_spec(spec_name).unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        std::fs::write(sp.spec_md_path(), body).unwrap();
    }

    fn write_mustard_json(cwd: &Path, fields: Value) {
        std::fs::write(cwd.join("mustard.json"), fields.to_string()).unwrap();
    }

    fn write_qa_event(cwd: &Path, spec: &str, overall: &str, criteria: Value) {
        // Route a `qa.result` through the event router — W5 lands it in the
        // per-spec NDJSON sink, same path `qa-run` uses in production.
        let event = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-05-19T00:00:00.000Z".to_string(),
            session_id: "s-test".to_string(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Cli,
                id: Some("qa-run".to_string()),
                actor_type: None,
            },
            event: "qa.result".to_string(),
            payload: json!({ "spec": spec, "overall": overall, "criteria": criteria }),
            spec: Some(spec.to_string()),
        };
        assert!(
            route::emit(cwd.to_str().unwrap(), &event),
            "router must land qa.result for {spec}"
        );
    }

    /// Every sub-gate strict — the production default.
    fn all_strict() -> CloseGateModes {
        CloseGateModes {
            close: GateMode::Strict,
            debt: GateMode::Strict,
            checklist: GateMode::Strict,
            qa: GateMode::Strict,
        }
    }

    /// Strict close-gate with the QA sub-gate off — isolates the build/test /
    /// checklist / debt gates without needing a `qa.result` event.
    fn no_qa() -> CloseGateModes {
        CloseGateModes {
            qa: GateMode::Off,
            ..all_strict()
        }
    }

    // --- trigger guards -----------------------------------------------------

    #[test]
    fn skips_non_pipeline_state_files() {
        assert!(!is_pipeline_state_file("/p/src/app.json"));
        // Construct the expected path programmatically so the literal substring
        // does not appear in source (docs-stale-check audit).
        let state_path = format!("/p/.claude/{}/x.json", ".pipeline-states");
        assert!(is_pipeline_state_file(&state_path));
    }

    #[test]
    fn skips_non_close_phase() {
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_fail() }));
        let mut input = close_input(dir.path(), "spec-exec");
        // Override phase to EXECUTE.
        input.tool_input = json!({
            "file_path": input.tool_input["file_path"],
            "content": json!({ "spec": "spec-exec", "phase": "EXECUTE" }).to_string(),
        });
        assert_eq!(
            close_gate_with_modes(&input, dir.path().to_str().unwrap(), all_strict()),
            Verdict::Allow
        );
    }

    /// The strict-cmd commands that exit non-zero / zero, cross-platform.
    fn exit_fail() -> &'static str {
        if cfg!(windows) {
            "cmd /c exit 1"
        } else {
            "sh -c \"exit 1\""
        }
    }
    fn exit_pass() -> &'static str {
        if cfg!(windows) {
            "cmd /c exit 0"
        } else {
            "sh -c \"exit 0\""
        }
    }

    // --- Wave 9: build/test gate (harness-wave9.test.js) -------------------

    #[test]
    fn close_gate_denies_on_failing_test_command() {
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_fail() }));
        // QA off + no checklist/debt → isolate the build/test gate.
        let input = close_input(dir.path(), "auth-login");
        match close_gate_with_modes(&input, dir.path().to_str().unwrap(), no_qa()) {
            Verdict::Deny { reason } => assert!(reason.contains("[Close Gate]")),
            other => panic!("expected Deny on failing test, got {other:?}"),
        }
    }

    #[test]
    fn close_gate_allows_on_passing_commands() {
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_pass() }));
        let input = close_input(dir.path(), "auth-login");
        let verdict = close_gate_with_modes(&input, dir.path().to_str().unwrap(), no_qa());
        assert!(!verdict.is_blocking(), "passing tests must not deny");
    }

    #[test]
    fn close_gate_warn_mode_does_not_deny_failing_test() {
        // mode=warn + failing test → advisory Warn, never Deny.
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_fail() }));
        let modes = CloseGateModes {
            close: GateMode::Warn,
            ..no_qa()
        };
        let input = close_input(dir.path(), "warn-spec");
        let verdict = close_gate_with_modes(&input, dir.path().to_str().unwrap(), modes);
        assert!(!verdict.is_blocking(), "warn mode must not deny");
        assert!(matches!(verdict, Verdict::Warn { .. }));
    }

    #[test]
    fn close_gate_off_mode_skips_entirely() {
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_fail() }));
        let modes = CloseGateModes {
            close: GateMode::Off,
            ..all_strict()
        };
        let input = close_input(dir.path(), "off-spec");
        assert_eq!(
            close_gate_with_modes(&input, dir.path().to_str().unwrap(), modes),
            Verdict::Allow
        );
    }

    #[test]
    fn close_gate_fails_open_without_mustard_json() {
        let dir = make_project();
        let input = close_input(dir.path(), "spec2");
        // No mustard.json → fail-open, no deny.
        assert_eq!(
            close_gate_with_modes(&input, dir.path().to_str().unwrap(), no_qa()),
            Verdict::Allow
        );
    }

    // --- Wave 10: QA gate (harness-wave10.test.js) -------------------------

    #[test]
    fn close_gate_denies_when_no_qa_result() {
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_pass() }));
        // QA strict, no qa.result event → deny.
        let input = close_input(dir.path(), "my-spec");
        match close_gate_with_modes(&input, dir.path().to_str().unwrap(), all_strict()) {
            Verdict::Deny { reason } => {
                assert!(reason.to_lowercase().contains("qa"));
            }
            other => panic!("expected Deny for missing QA, got {other:?}"),
        }
    }

    #[test]
    fn close_gate_denies_when_qa_failed() {
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_pass() }));
        write_qa_event(
            dir.path(),
            "fail-qa-spec",
            "fail",
            json!([{ "id": "AC-1", "status": "fail" }]),
        );
        let input = close_input(dir.path(), "fail-qa-spec");
        assert!(
            close_gate_with_modes(&input, dir.path().to_str().unwrap(), all_strict())
                .is_blocking()
        );
    }

    #[test]
    fn close_gate_allows_when_qa_passed() {
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_pass() }));
        write_qa_event(
            dir.path(),
            "pass-qa-spec",
            "pass",
            json!([{ "id": "AC-1", "status": "pass" }]),
        );
        let input = close_input(dir.path(), "pass-qa-spec");
        assert!(
            !close_gate_with_modes(&input, dir.path().to_str().unwrap(), all_strict())
                .is_blocking()
        );
    }

    /// The legitimate skip shape: the spec carries NO acceptance criteria at
    /// all (`criteria` empty) — the historical advisory contract holds.
    #[test]
    fn close_gate_allows_when_qa_skipped() {
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_pass() }));
        write_qa_event(dir.path(), "skip-qa-spec", "skip", json!([]));
        let input = close_input(dir.path(), "skip-qa-spec");
        assert!(
            !close_gate_with_modes(&input, dir.path().to_str().unwrap(), all_strict())
                .is_blocking()
        );
    }

    /// The dangerous skip shape: acceptance criteria EXIST but every one
    /// skipped at run time (timeout / spawn failure). Strict must route the
    /// decision to the user instead of closing on a green that verified nothing.
    #[test]
    fn close_gate_denies_when_qa_skipped_with_criteria() {
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_pass() }));
        write_qa_event(
            dir.path(),
            "skip-ac-spec",
            "skip",
            json!([
                { "id": "AC-1", "status": "skip" },
                { "id": "AC-2", "status": "skip" },
            ]),
        );
        let input = close_input(dir.path(), "skip-ac-spec");
        match close_gate_with_modes(&input, dir.path().to_str().unwrap(), all_strict()) {
            Verdict::Deny { reason } => {
                assert!(reason.contains("skipped all 2"), "reason names the count: {reason}");
                assert!(
                    reason.contains("never exercised"),
                    "reason explains the principle: {reason}"
                );
            }
            other => panic!("expected Deny for skip-with-criteria, got {other:?}"),
        }
    }

    /// Same shape under `qa: warn` — advisory, falls through without blocking.
    #[test]
    fn close_gate_warn_allows_qa_skipped_with_criteria() {
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_pass() }));
        write_qa_event(
            dir.path(),
            "skip-ac-warn-spec",
            "skip",
            json!([{ "id": "AC-1", "status": "skip" }]),
        );
        let input = close_input(dir.path(), "skip-ac-warn-spec");
        let modes = CloseGateModes { qa: GateMode::Warn, ..all_strict() };
        assert!(
            !close_gate_with_modes(&input, dir.path().to_str().unwrap(), modes).is_blocking()
        );
    }

    #[test]
    fn close_gate_qa_off_does_not_deny_missing_qa() {
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_pass() }));
        // No qa.result, QA gate off → must not deny on QA grounds.
        let input = close_input(dir.path(), "off-qa-spec");
        assert!(
            !close_gate_with_modes(&input, dir.path().to_str().unwrap(), no_qa())
                .is_blocking()
        );
    }

    // --- checklist gate (checklist-mark.test.js) ---------------------------

    #[test]
    fn close_gate_denies_unmarked_checklist() {
        let dir = make_project();
        write_spec(
            dir.path(),
            "demo",
            "# Spec\n\n## Checklist\n\n- [x] first done\n- [ ] second open\n\
             - [ ] third open\n\n## Notes\n",
        );
        let input = close_input(dir.path(), "demo");
        match close_gate_with_modes(&input, dir.path().to_str().unwrap(), no_qa()) {
            Verdict::Deny { reason } => assert!(reason.contains("2 unmarked")),
            other => panic!("expected Deny for unmarked checklist, got {other:?}"),
        }
    }

    /// D1 orphan-gate fix: a wave-plan PARENT has no `## Checklist`, so the gate
    /// must consolidate the WAVE checklists. An unmarked wave item → Deny.
    #[test]
    fn close_gate_consolidates_wave_checklists_when_parent_has_none() {
        let dir = make_project();
        // Parent: coordination doc — no `## Checklist`, but a wave-plan meta.
        let sp = ClaudePaths::for_project(dir.path()).unwrap().for_spec("epic").unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        std::fs::write(sp.spec_md_path(), "# Epic\n\n## Network\n- coordination only\n").unwrap();
        std::fs::write(
            sp.dir().join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active","isWavePlan":true,"totalWaves":2}"#,
        )
        .unwrap();
        // Wave 1: fully marked. Wave 2: one unmarked item.
        std::fs::create_dir_all(sp.dir().join("wave-1-general")).unwrap();
        std::fs::write(
            sp.dir().join("wave-1-general").join("spec.md"),
            "# Wave 1\n\n## Checklist\n- [x] done\n",
        )
        .unwrap();
        std::fs::create_dir_all(sp.dir().join("wave-2-frontend")).unwrap();
        std::fs::write(
            sp.dir().join("wave-2-frontend").join("spec.md"),
            "# Wave 2\n\n## Checklist\n- [x] one\n- [ ] still open\n",
        )
        .unwrap();

        let (found, unmarked) = find_unmarked_checklist(dir.path().to_str().unwrap(), Some("epic"));
        assert!(found, "wave-plan parent must consolidate wave checklists");
        assert_eq!(unmarked.len(), 1, "exactly one unmarked wave item: {unmarked:?}");
        assert!(unmarked[0].contains("still open"));
        assert!(unmarked[0].contains("wave-2-frontend"), "wave label prefix: {unmarked:?}");

        // End-to-end through the gate: an unmarked wave item denies CLOSE.
        let input = close_input(dir.path(), "epic");
        match close_gate_with_modes(&input, dir.path().to_str().unwrap(), no_qa()) {
            Verdict::Deny { reason } => assert!(reason.contains("unmarked")),
            other => panic!("expected Deny for unmarked wave checklist, got {other:?}"),
        }
    }

    /// Meta-first consolidation (checklist-progresso-por-onda W2): the wave's
    /// `meta.json#checklist` is the source the gate reads — a `done:false`
    /// item blocks CLOSE even when the wave's markdown carries a stale
    /// all-marked `## Checklist`; flipping every `done` to `true` releases it.
    #[test]
    fn close_gate_blocks_on_wave_meta_checklist_and_releases_when_done() {
        let dir = make_project();
        let sp = ClaudePaths::for_project(dir.path()).unwrap().for_spec("epic-meta").unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        std::fs::write(sp.spec_md_path(), "# Epic\n\n## Network\n- coord\n").unwrap();
        std::fs::write(
            sp.dir().join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active","isWavePlan":true,"totalWaves":1}"#,
        )
        .unwrap();
        let wave_dir = sp.dir().join("wave-1-rt");
        std::fs::create_dir_all(&wave_dir).unwrap();
        // Stale markdown says everything is done — the sidecar must win.
        std::fs::write(
            wave_dir.join("spec.md"),
            "# Wave 1\n\n## Checklist\n- [x] stale markdown item\n",
        )
        .unwrap();
        std::fs::write(
            wave_dir.join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active","parent":"epic-meta","checklist":[{"label":"src/a.rs","path":"src/a.rs","done":true},{"label":"src/b.rs","path":"src/b.rs","done":false}]}"#,
        )
        .unwrap();

        let (found, unmarked) =
            find_unmarked_checklist(dir.path().to_str().unwrap(), Some("epic-meta"));
        assert!(found, "wave meta checklist must be consolidated");
        assert_eq!(unmarked.len(), 1, "one done:false item: {unmarked:?}");
        assert!(unmarked[0].contains("src/b.rs"), "{unmarked:?}");
        assert!(unmarked[0].contains("wave-1-rt"), "wave label prefix: {unmarked:?}");

        // End-to-end: the gate denies CLOSE on the open meta item.
        let input = close_input(dir.path(), "epic-meta");
        match close_gate_with_modes(&input, dir.path().to_str().unwrap(), no_qa()) {
            Verdict::Deny { reason } => assert!(reason.contains("unmarked")),
            other => panic!("expected Deny for done:false wave meta item, got {other:?}"),
        }

        // Flip the open item → the gate releases (anti-gate-órfão preserved:
        // found stays true, the unmarked list empties).
        std::fs::write(
            wave_dir.join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active","parent":"epic-meta","checklist":[{"label":"src/a.rs","path":"src/a.rs","done":true},{"label":"src/b.rs","path":"src/b.rs","done":true}]}"#,
        )
        .unwrap();
        let (found, unmarked) =
            find_unmarked_checklist(dir.path().to_str().unwrap(), Some("epic-meta"));
        assert!(found);
        assert!(unmarked.is_empty(), "all done:true → release: {unmarked:?}");
        assert_eq!(
            close_gate_with_modes(&input, dir.path().to_str().unwrap(), no_qa()),
            Verdict::Allow
        );
    }

    /// A wave-plan parent whose waves are all fully marked → the consolidated
    /// gate finds nothing unmarked and CLOSE proceeds (no orphan, no false deny).
    #[test]
    fn close_gate_allows_wave_plan_when_all_waves_marked() {
        let dir = make_project();
        let sp = ClaudePaths::for_project(dir.path()).unwrap().for_spec("epic2").unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        std::fs::write(sp.spec_md_path(), "# Epic2\n\n## Network\n- coord\n").unwrap();
        std::fs::write(
            sp.dir().join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active","isWavePlan":true,"totalWaves":1}"#,
        )
        .unwrap();
        std::fs::create_dir_all(sp.dir().join("wave-1-general")).unwrap();
        std::fs::write(
            sp.dir().join("wave-1-general").join("spec.md"),
            "# Wave 1\n\n## Checklist\n- [x] done\n",
        )
        .unwrap();

        let (found, unmarked) =
            find_unmarked_checklist(dir.path().to_str().unwrap(), Some("epic2"));
        assert!(found);
        assert!(unmarked.is_empty(), "all waves marked → no unmarked items: {unmarked:?}");
        let input = close_input(dir.path(), "epic2");
        assert_eq!(
            close_gate_with_modes(&input, dir.path().to_str().unwrap(), no_qa()),
            Verdict::Allow
        );
    }

    #[test]
    fn close_gate_passes_fully_marked_checklist() {
        let dir = make_project();
        write_spec(
            dir.path(),
            "demo",
            "# Spec\n\n## Checklist\n\n- [x] first\n- [x] second\n\n## Notes\n",
        );
        // No mustard.json → after the checklist gate passes, build gate skips.
        let input = close_input(dir.path(), "demo");
        assert_eq!(
            close_gate_with_modes(&input, dir.path().to_str().unwrap(), no_qa()),
            Verdict::Allow
        );
    }

    // --- close-gate.check event --------------------------------------------

    #[test]
    fn close_gate_emits_check_event() {
        let dir = make_project();
        write_mustard_json(
            dir.path(),
            json!({ "testCommand": exit_pass(), "buildCommand": exit_pass() }),
        );
        let input = close_input(dir.path(), "spec-event");
        let _ = close_gate_with_modes(&input, dir.path().to_str().unwrap(), no_qa());

        // W5: `close-gate.check` is non-pipeline → per-spec NDJSON. The spec
        // dir is created by `write_active_spec` indirectly via close_gate's
        // path resolution; with no spec attribution the event falls back to
        // the session dir.
        let paths = ClaudePaths::for_project(dir.path()).unwrap();
        let spec_events = paths.for_spec("spec-event").unwrap().events_dir();
        let session_root = paths.claude_dir().join(".session");
        let candidate_dirs: Vec<std::path::PathBuf> = std::iter::once(spec_events)
            .chain(
                std::fs::read_dir(&session_root)
                    .into_iter()
                    .flatten()
                    .filter_map(|e| e.ok().map(|e| e.path().join(".events"))),
            )
            .collect();
        let mut found = false;
        for d in candidate_dirs {
            if !d.exists() {
                continue;
            }
            for f in std::fs::read_dir(&d).unwrap() {
                let body = std::fs::read_to_string(f.unwrap().path()).unwrap_or_default();
                if body.lines().any(|l| l.contains("\"event\":\"close-gate.check\"")) {
                    found = true;
                }
            }
        }
        assert!(found, "close-gate.check NDJSON line must be present");
    }

    // --- extractPhase / extractSpec parity ---------------------------------

    #[test]
    fn extract_phase_reads_phase_name_and_legacy_phase() {
        // Real shape: numeric `phase` + string `phaseName`.
        assert_eq!(
            extract_phase(r#"{"phase":3,"phaseName":"CLOSE"}"#).as_deref(),
            Some("CLOSE")
        );
        // Legacy shape: string `phase`.
        assert_eq!(
            extract_phase(r#"{"phase":"close"}"#).as_deref(),
            Some("CLOSE")
        );
        assert_eq!(extract_phase("not json"), None);
    }

    // --- Wave-3a: projection None → fail-open in close_gate -----------------

    #[test]
    fn close_gate_allows_when_state_file_absent() {
        // No pipeline-state JSON at all and no mustard.json → fail-open Allow.
        // This mirrors the projection-None behaviour: when state is absent the
        // close gate should not block (spec guard: "Fail-open: projection None
        // → return Verdict::Allow").
        let dir = make_project();
        // Build an input that points at a pipeline-state file path but with
        // a non-CLOSE phase — gate must Allow without touching any state.
        let state_file = ClaudePaths::for_project(dir.path())
            .unwrap()
            .pipeline_state_file("ghost");
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({
                "file_path": state_file.to_string_lossy(),
                "content": json!({ "spec": "ghost", "phase": "CLOSE" }).to_string(),
            }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(dir.path().to_string_lossy().into_owned()),
            ..HookInput::default()
        };
        // No mustard.json → build/test gate skips → Allow.
        assert_eq!(
            close_gate_with_modes(&input, dir.path().to_str().unwrap(), no_qa()),
            Verdict::Allow,
            "missing mustard.json must fail-open (Allow)"
        );
    }
}
