//! `crystallise_nudge` — the `Stop`-event reminder that a settled conversation
//! is not yet on disk.
//!
//! ## The window is the only place the conversation lives
//!
//! Everything decided before a unit opens — definitions the conversation
//! settled, choices and the reasons behind them, findings checked at a
//! `file:line` — lives in the model's context window and nowhere else. The
//! channel that carries it into the spec (`spec-draft --material`) is written
//! at draft time, from memory, and a compaction between the decision and the
//! draft silently dilutes what reaches the artefact. Measured in the field: the
//! operator reported specs and waves that "do not reflect what was defined".
//!
//! Writing each decision down WHEN IT IS SETTLED fixes that, and the router
//! says so. But a rule the model is asked to remember is not a guarantee — it
//! is the same class of promise this whole unit exists to replace with a
//! measurement. So this gate measures the one thing that is measurable: an
//! active unit whose material file has not changed while the turns went by.
//!
//! ## It nudges ONCE, and never twice for the same drift
//!
//! Blocking a stop is expensive: it costs the operator a turn. The gate
//! therefore fires at most once per drift — the nudge writes a marker holding
//! the material's state at that moment, and it stays quiet until the material
//! actually changes. A conversation that legitimately has nothing new to record
//! is not asked twice.
//!
//! There is no auto-retry loop here and so no counter to bound: unlike the QA
//! gate, one block is the whole intervention.
//!
//! ## Fail-open
//!
//! Every step degrades to `Allow`: no unit, no config, an unreadable material
//! file or an unwritable marker all release the stop. A gate that blocked on
//! its own IO failure would be worse than the drift it reports.

use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::io::fs;
use mustard_core::platform::error::Error;
use mustard_core::ClaudePaths;
use std::path::{Path, PathBuf};

/// Turns an active unit may run without its material changing before the gate
/// speaks up.
///
/// Not zero: the first turns of a unit are research, and there is genuinely
/// nothing settled to record yet. Not large either — the cost of waiting is
/// that a compaction lands first and the material is reconstructed from a
/// summary, which is the defect.
const QUIET_TURNS_BEFORE_NUDGE: u32 = 6;

/// Where the conversation material is assembled, project-relative.
const MATERIAL_FILE: &str = ".claude/.cache/spec-material.json";

/// The `Stop`-event crystallisation reminder.
pub struct CrystalliseNudge;

impl Check for CrystalliseNudge {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::Stop) || input.is_subagent() {
            return Ok(Verdict::Allow);
        }
        // The platform's own repeat signal: never stack on a stop already being
        // blocked by a sibling gate.
        if input.raw.get("stop_hook_active").and_then(serde_json::Value::as_bool) == Some(true) {
            return Ok(Verdict::Allow);
        }
        let project_dir = ctx.project_dir_or_cwd(input);
        let root = Path::new(&project_dir);

        // Self-restriction: only a unit that is actually open. Without one there
        // is no material file to write and nothing to nudge about.
        let Some(spec) = crate::shared::context::current_spec(&project_dir).filter(|s| !s.is_empty())
        else {
            return Ok(Verdict::Allow);
        };
        // …and a unit that is actually OPEN. `current_spec` reads the newest
        // pipeline-state file, and those are swept only at SessionEnd — so for
        // the rest of a session after a unit closes, a finished one still reads
        // as active. The banner shares that signal and merely displays it; this
        // gate BLOCKS, so the staleness would cost the operator a turn demanding
        // they crystallise material for a unit that no longer exists (found in
        // review). `meta.json` is the lifecycle authority.
        if spec_is_closed(root, &spec) {
            return Ok(Verdict::Allow);
        }

        let material = root.join(MATERIAL_FILE);
        let state = material_state(&material);
        let Some(marker) = marker_path(root, &spec) else {
            return Ok(Verdict::Allow);
        };

        let seen: Option<String> = fs::read_to_string(&marker).ok().map(|s| s.trim().to_string());
        // The material moved since the last nudge: the operator acted, so the
        // count starts over and the gate goes quiet.
        if seen.as_deref() == Some(state.as_str()) {
            return Ok(Verdict::Allow);
        }

        let turns = bump_quiet_turns(root, &spec);
        if turns < QUIET_TURNS_BEFORE_NUDGE {
            return Ok(Verdict::Allow);
        }

        // Record the state we are nudging ABOUT, so the same standing drift
        // never costs a second turn. Written before the block so an unwritable
        // marker cannot produce a nudge on every turn.
        if write_marker(&marker, &state).is_err() {
            return Ok(Verdict::Allow);
        }
        reset_quiet_turns(root, &spec);

        let lang = mustard_core::ProjectConfig::load(root).i18n().lang;
        let reason = mustard_core::translate("crystallise.nudge", lang).replace("{spec}", &spec);
        Ok(Verdict::Deny { reason })
    }
}

/// Has this unit already reached a terminal outcome?
///
/// Read from `meta.json`, the single lifecycle source. Fail-open: an absent or
/// unreadable sidecar answers "not closed", so the gate still applies to a unit
/// whose state cannot be read — the direction that keeps the reminder working
/// rather than silently disabling it.
pub(crate) fn spec_is_closed(root: &Path, spec: &str) -> bool {
    let spec_md = root.join(".claude").join("spec").join(spec).join("spec.md");
    mustard_core::domain::meta::read_meta_beside(&spec_md)
        .and_then(|m| m.outcome)
        .and_then(|o| mustard_core::Outcome::parse(&o))
        .is_some_and(|o| o == mustard_core::Outcome::Completed)
}

/// A cheap fingerprint of the material file: byte length plus modification
/// time, or `"absent"` when it does not exist.
///
/// Length+mtime answers the only question asked — did this change since the
/// last nudge? Hashing the body would buy nothing the gate can act on.
fn material_state(material: &Path) -> String {
    let Ok(body) = fs::read_to_string(material) else {
        return "absent".to_string();
    };
    // The CONTENT decides, not its length and mtime. Those two miss a rewrite
    // of identical length within the same wall-clock second — the gate then
    // reads a changed file as unchanged and stays quiet (found in review). The
    // body is already in hand, so hashing it costs nothing and cannot miss.
    //
    // Written out here rather than through `DefaultHasher`, whose output the
    // standard library does not promise to keep stable across releases: the
    // marker outlives a toolchain upgrade, and a changed hash would read as a
    // changed file and spend one spurious nudge per spec. FNV-1a is four lines
    // and fixed forever.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in body.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{}:{hash:016x}", body.len())
}

/// `<root>/.claude/.harness/crystallise-<spec>` — the state this gate last
/// nudged about.
fn marker_path(root: &Path, spec: &str) -> Option<PathBuf> {
    Some(
        ClaudePaths::for_project(root)
            .ok()?
            .harness_dir()
            .join(format!("crystallise-{}", spec.replace(['/', '\\'], "-"))),
    )
}

/// `<root>/.claude/.harness/crystallise-turns-<spec>` — consecutive turns the
/// material stayed put.
fn turns_path(root: &Path, spec: &str) -> Option<PathBuf> {
    marker_path(root, spec).map(|p| {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        p.with_file_name(name.replacen("crystallise-", "crystallise-turns-", 1))
    })
}

/// Increment and return the quiet-turn count. Fail-open: an unwritable counter
/// reads as the turn that just happened, so the gate simply never reaches the
/// threshold instead of nudging on every turn.
fn bump_quiet_turns(root: &Path, spec: &str) -> u32 {
    let Some(path) = turns_path(root, spec) else {
        return 0;
    };
    let next = fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(0)
        .saturating_add(1);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if fs::write_atomic(&path, next.to_string().as_bytes()).is_err() {
        return 0;
    }
    next
}

fn reset_quiet_turns(root: &Path, spec: &str) {
    if let Some(path) = turns_path(root, spec) {
        let _ = fs::remove_file(&path);
    }
}

fn write_marker(marker: &Path, state: &str) -> Result<(), Error> {
    if let Some(parent) = marker.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write_atomic(marker, state.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn ctx(dir: &str) -> Ctx {
        Ctx {
            project_dir: dir.to_string(),
            trigger: Some(Trigger::Stop),
            workspace_root: None,
            inject_only: None,
        }
    }

    fn stop_input() -> HookInput {
        HookInput {
            hook_event_name: Some("Stop".to_string()),
            session_id: Some("s1".to_string()),
            ..HookInput::default()
        }
    }

    /// Seed a project with an ACTIVE unit — the pipeline-state marker is what
    /// `current_spec` reads.
    fn seed_unit(root: &Path, spec: &str) {
        std::fs::write(root.join("mustard.json"), r#"{"version":"1.0.0"}"#).unwrap();
        let states = root.join(".claude/.pipeline-states");
        std::fs::create_dir_all(&states).unwrap();
        std::fs::write(states.join(format!("{spec}.json")), "{}").unwrap();
    }

    /// Run `n` turn-ends and report whether ANY of them blocked.
    ///
    /// Any, not the last: the nudge fires on one turn and goes quiet again, so
    /// a helper that returned only the final verdict would miss it whenever the
    /// block lands mid-run.
    fn any_blocked(root: &Path, n: u32) -> bool {
        let project = root.to_str().unwrap();
        (0..n).any(|_| {
            CrystalliseNudge
                .evaluate(&stop_input(), &ctx(project))
                .expect("no error")
                .is_blocking()
        })
    }

    /// The verdict of ONE turn-end.
    fn one_turn(root: &Path) -> Verdict {
        CrystalliseNudge
            .evaluate(&stop_input(), &ctx(root.to_str().unwrap()))
            .expect("no error")
    }

    /// AC-9 — a unit whose material sits still is nudged ONCE, and the ceiling
    /// keeps the same standing drift from costing a second turn.
    #[test]
    fn stop_nudges_stale_material_once_with_a_ceiling() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_unit(root, "uma-unidade");

        // Below the threshold the gate is silent: the first turns are research.
        assert!(
            !any_blocked(root, QUIET_TURNS_BEFORE_NUDGE - 1),
            "the gate must not fire during research turns",
        );

        // At the threshold it blocks once, naming the unit.
        let nudge = one_turn(root);
        let Verdict::Deny { ref reason } = nudge else {
            panic!("stale material must be nudged: {nudge:?}");
        };
        assert!(reason.contains("uma-unidade"), "the nudge must name the unit: {reason}");

        // The SAME standing drift never blocks again — no second turn is spent.
        assert!(
            !any_blocked(root, QUIET_TURNS_BEFORE_NUDGE * 2),
            "the same drift must not be nudged twice",
        );
    }

    /// Material that MOVED resets the gate: the operator acted, so the next
    /// quiet stretch is measured from there.
    #[test]
    fn material_that_moved_re_arms_the_gate() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_unit(root, "outra-unidade");
        assert!(any_blocked(root, QUIET_TURNS_BEFORE_NUDGE), "the first drift is nudged");

        // The operator crystallised something.
        std::fs::create_dir_all(root.join(".claude/.cache")).unwrap();
        std::fs::write(root.join(MATERIAL_FILE), r#"{"decisions":[]}"#).unwrap();

        assert!(!one_turn(root).is_blocking(), "a turn right after writing must not nudge");
        assert!(
            any_blocked(root, QUIET_TURNS_BEFORE_NUDGE),
            "a NEW quiet stretch is nudged again",
        );
    }

    /// No open unit, a subagent stop, or a stop another gate is already
    /// blocking: release in silence.
    #[test]
    fn the_nudge_self_restricts() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), r#"{"version":"1.0.0"}"#).unwrap();
        assert!(
            !any_blocked(root, QUIET_TURNS_BEFORE_NUDGE + 1),
            "no open unit: nothing to crystallise",
        );

        let dir2 = tempdir().unwrap();
        seed_unit(dir2.path(), "com-unidade");
        let project = dir2.path().to_str().unwrap();
        let mut sub = stop_input();
        sub.raw = json!({"stop_hook_active": true});
        for _ in 0..=QUIET_TURNS_BEFORE_NUDGE {
            let v = CrystalliseNudge.evaluate(&sub, &ctx(project)).expect("no error");
            assert!(!v.is_blocking(), "never stack on a stop already being blocked");
        }
    }
}
