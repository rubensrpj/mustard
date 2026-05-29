//! `active_spec_limit_gate` — hard cap on concurrently active pipelines.
//!
//! ## Scope (F4-d item 1 — "não se perder")
//!
//! A `PreToolUse(Skill)` gate, sibling to
//! [`crate::hooks::write::entity_registry_gate::EntityRegistryGate`]: it sits on
//! the **entry** of a new pipeline (`/feature`, `/bugfix`) and refuses (or
//! warns) when the number of pipelines already running would exceed a cap.
//! Before this gate the `26` in `active_specs` was a *display* truncation only;
//! nothing stopped an operator from fanning out an unbounded number of parallel
//! pipelines and losing track. This is the missing concurrency limit.
//!
//! ## Count — deterministic, shared projection
//!
//! "Active" is counted via
//! [`crate::commands::spec::active_specs::count_active`] — the same
//! `discover_root_specs` + `classify_spec` projection the `active-specs` picker
//! uses, so the gate and the picker can never disagree. Only `SpecKind::Active`
//! (`Outcome=Active` + `Stage ∈ {Analyze, Plan, Execute}`) counts; finished /
//! malformed specs do not.
//!
//! ## Cap — `mustard.json#maxActiveSpecs`, default 10
//!
//! The cap is read from `mustard.json#maxActiveSpecs`
//! ([`mustard_core::ProjectConfig::max_active_specs`]); when unset it defaults
//! to [`DEFAULT_MAX_ACTIVE_SPECS`] (`10`). The gate fires when **opening the new
//! pipeline would push the count past the cap** — i.e. when
//! `active_count >= cap` (the new one would be the `cap + 1`-th).
//!
//! ## Mode — `MUSTARD_MAX_ACTIVE_SPECS_MODE` = `off` | `warn` | `strict`
//!
//! Default `warn`. This is a *hard limit with an escape hatch*: `warn` (default)
//! surfaces a non-blocking advisory so the operator is told they are at the cap
//! but is never wedged; `strict` makes it a true block ([`Verdict::Deny`]) for
//! teams that want the cap enforced; `off` disables the gate entirely. The
//! escape from `strict` is to close a pipeline (or raise the cap in
//! `mustard.json`) — never a `--force`, so the limit stays meaningful.
//!
//! ## Fail-open
//!
//! Determinism + fail-open are invariants. Any error path —
//! a non-pipeline skill, an unresolvable project dir, an unreadable
//! `.claude/spec` — degrades to [`Verdict::Allow`]. `count_active` is fail-open
//! by construction (an IO error can only *under*-count), so a counting failure
//! can never spuriously trip the cap.

use crate::commands::spec::active_specs::count_active;
use crate::util::format_gate_message;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::platform::error::Error;
use std::path::Path;

/// The pipeline skills the limit gate applies to. Mirrors
/// [`crate::hooks::write::entity_registry_gate`]'s `PIPELINE_SKILLS`: only the
/// pipeline *entry* skills open a new pipeline, so only they are gated.
const PIPELINE_SKILLS: &[&str] = &["mustard:feature", "mustard:bugfix", "feature", "bugfix"];

/// Default cap on concurrently active pipelines when `mustard.json` does not
/// pin `maxActiveSpecs`. Sized to comfortably exceed the handful of pipelines a
/// single operator tracks at once, while still catching a runaway fan-out.
pub const DEFAULT_MAX_ACTIVE_SPECS: usize = 10;

/// The active-pipeline limit gate module.
pub struct ActiveSpecLimitGate;

/// Output mode of the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LimitMode {
    /// Disabled — the gate is a pure no-op (`Allow`).
    Off,
    /// Surface the over-cap advisory as a non-blocking warning (default).
    Warn,
    /// Block the new pipeline (`Deny`) until one is closed or the cap raised.
    Strict,
}

/// Read `MUSTARD_MAX_ACTIVE_SPECS_MODE` (default `warn`). `off` disables the
/// gate; `strict` makes it blocking; anything else (incl. unset) is `warn`.
fn limit_mode() -> LimitMode {
    match std::env::var("MUSTARD_MAX_ACTIVE_SPECS_MODE")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "off" => LimitMode::Off,
        "strict" => LimitMode::Strict,
        _ => LimitMode::Warn,
    }
}

/// The skill name a `PreToolUse(Skill)` invocation targets.
fn skill_name(input: &HookInput) -> &str {
    input
        .tool_input
        .get("skill")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
}

/// Resolve the effective cap for `cwd`: `mustard.json#maxActiveSpecs` when set,
/// else [`DEFAULT_MAX_ACTIVE_SPECS`]. Fail-open — an unreadable / malformed
/// `mustard.json` reads as "no override" → the default.
fn cap_for(cwd: &Path) -> usize {
    mustard_core::ProjectConfig::load(cwd)
        .max_active_specs()
        .unwrap_or(DEFAULT_MAX_ACTIVE_SPECS)
}

/// Build the over-cap advisory / deny reason.
fn over_cap_message(active: usize, cap: usize) -> String {
    format_gate_message(
        "Active Spec Limit",
        &format!("{active} active pipeline(s) already running (cap {cap})"),
        "opening another risks losing track of parallel work",
        "close or complete a pipeline (/complete-spec), or raise mustard.json#maxActiveSpecs",
    )
}

/// Compute the verdict for a `PreToolUse(Skill)` invocation rooted at `cwd`,
/// for an explicit `mode`.
///
/// Pure with respect to the environment: the production [`Check::evaluate`]
/// reads `MUSTARD_MAX_ACTIVE_SPECS_MODE` and threads it in, while tests pass a
/// `mode` directly — so the mode branches are exercised **without mutating
/// process-global env** (which is `unsafe` under Rust 2024, and the crate is
/// `#![forbid(unsafe_code)]`). Only `cap` / `count` still touch disk.
pub(crate) fn verdict_with(input: &HookInput, cwd: &str, mode: LimitMode) -> Verdict {
    if mode == LimitMode::Off {
        return Verdict::Allow;
    }
    // Only the Skill tool, and only the pipeline-entry skills, are gated.
    if input.tool_name.as_deref() != Some("Skill") {
        return Verdict::Allow;
    }
    if !PIPELINE_SKILLS.contains(&skill_name(input)) {
        return Verdict::Allow;
    }

    let root = Path::new(cwd);
    let cap = cap_for(root);
    let active = count_active(root);

    // Fire only when opening the new pipeline would exceed the cap (the new one
    // would be the `cap + 1`-th). `active < cap` always allows; a counting
    // failure under-counts, so it can never trip this branch.
    if active < cap {
        return Verdict::Allow;
    }

    let msg = over_cap_message(active, cap);
    match mode {
        LimitMode::Warn => Verdict::Warn { message: msg },
        LimitMode::Strict => Verdict::Deny { reason: msg },
        LimitMode::Off => Verdict::Allow,
    }
}

impl Check for ActiveSpecLimitGate {
    /// Gate a `PreToolUse(Skill)` invocation of a pipeline-entry skill on the
    /// concurrent-pipeline cap. Mode-driven (`off`/`warn`/`strict`); fail-open.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        let cwd = ctx.project_dir_or_cwd(input);
        Ok(verdict_with(input, &cwd, limit_mode()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::Path as StdPath;

    // Tests drive `verdict_with` with an explicit `LimitMode` rather than
    // mutating `MUSTARD_MAX_ACTIVE_SPECS_MODE` — env mutation is `unsafe` under
    // Rust 2024 and this crate is `#![forbid(unsafe_code)]`. The env-reading
    // wrapper `limit_mode()` is covered by the dedicated parse test below.

    /// Create `<root>/.claude/spec/<name>/spec.md` + `meta.json` with the given
    /// lifecycle, so `count_active` sees a real candidate.
    fn make_spec(root: &StdPath, name: &str, stage: &str, outcome: &str) {
        let dir = root.join(".claude").join("spec").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("spec.md"), format!("# {name}\n\n## Resumo\n\nx\n")).unwrap();
        std::fs::write(
            dir.join("meta.json"),
            format!(r#"{{"stage":"{stage}","outcome":"{outcome}","scope":null,"parent":null,"checkpoint":null}}"#),
        )
        .unwrap();
    }

    fn skill_input(skill: &str, cwd: &str) -> HookInput {
        HookInput {
            tool_name: Some("Skill".to_string()),
            tool_input: json!({ "skill": skill }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(cwd.to_string()),
            ..HookInput::default()
        }
    }

    /// Plant `mustard.json` with an explicit `maxActiveSpecs` cap.
    fn write_cap(root: &StdPath, cap: usize) {
        std::fs::write(root.join("mustard.json"), format!(r#"{{"maxActiveSpecs":{cap}}}"#)).unwrap();
    }

    #[test]
    fn allows_below_cap() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_cap(root, 3);
        // 2 active < cap 3 → allow, even in strict mode.
        make_spec(root, "2026-01-01-a", "Plan", "Active");
        make_spec(root, "2026-01-02-b", "Execute", "Active");
        let input = skill_input("feature", root.to_str().unwrap());
        assert_eq!(
            verdict_with(&input, root.to_str().unwrap(), LimitMode::Strict),
            Verdict::Allow
        );
    }

    #[test]
    fn strict_denies_when_at_cap() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_cap(root, 2);
        // N = 2; with N active, opening the (N+1)-th must be denied.
        make_spec(root, "2026-01-01-a", "Plan", "Active");
        make_spec(root, "2026-01-02-b", "Analyze", "Active");
        let input = skill_input("mustard:feature", root.to_str().unwrap());
        let verdict = verdict_with(&input, root.to_str().unwrap(), LimitMode::Strict);
        assert!(verdict.is_blocking(), "at cap in strict mode must deny: {verdict:?}");
    }

    #[test]
    fn warn_mode_warns_when_at_cap_but_never_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_cap(root, 1);
        make_spec(root, "2026-01-01-a", "Plan", "Active");
        let input = skill_input("bugfix", root.to_str().unwrap());
        match verdict_with(&input, root.to_str().unwrap(), LimitMode::Warn) {
            Verdict::Warn { message } => {
                assert!(message.contains("cap 1"), "got {message}");
                assert!(!Verdict::Warn { message }.is_blocking());
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn off_mode_allows_even_over_cap() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_cap(root, 1);
        make_spec(root, "2026-01-01-a", "Plan", "Active");
        make_spec(root, "2026-01-02-b", "Execute", "Active");
        let input = skill_input("feature", root.to_str().unwrap());
        assert_eq!(
            verdict_with(&input, root.to_str().unwrap(), LimitMode::Off),
            Verdict::Allow
        );
    }

    #[test]
    fn non_pipeline_skill_allows() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_cap(root, 0); // even an aggressive 0 cap...
        make_spec(root, "2026-01-01-a", "Plan", "Active");
        // ...does not gate a non-pipeline skill.
        let input = skill_input("some-other-skill", root.to_str().unwrap());
        assert_eq!(
            verdict_with(&input, root.to_str().unwrap(), LimitMode::Strict),
            Verdict::Allow
        );
    }

    #[test]
    fn fail_open_when_no_spec_dir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // No `.claude/spec` directory exists → `count_active` fails open to 0 →
        // 0 active < cap 1 → Allow. A counting failure can only under-count, so
        // it can never spuriously trip the cap.
        std::fs::write(root.join("mustard.json"), r#"{"maxActiveSpecs":1}"#).unwrap();
        let input = skill_input("feature", root.to_str().unwrap());
        assert_eq!(
            verdict_with(&input, root.to_str().unwrap(), LimitMode::Strict),
            Verdict::Allow
        );
    }

    #[test]
    fn default_cap_allows_under_ten() {
        // No `maxActiveSpecs` in mustard.json → DEFAULT_MAX_ACTIVE_SPECS (10).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), "{}").unwrap();
        for i in 0..9 {
            make_spec(root, &format!("2026-01-{i:02}-s"), "Plan", "Active");
        }
        assert_eq!(cap_for(root), DEFAULT_MAX_ACTIVE_SPECS);
        let input = skill_input("feature", root.to_str().unwrap());
        // 9 active < 10 → allow.
        assert_eq!(
            verdict_with(&input, root.to_str().unwrap(), LimitMode::Strict),
            Verdict::Allow
        );
    }

    #[test]
    fn closed_followup_and_malformed_do_not_count() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_cap(root, 1);
        // 1 genuinely active + 1 closed-followup + 1 malformed.
        make_spec(root, "2026-01-01-active", "Plan", "Active");
        make_spec(root, "2026-01-02-followup", "Close", "Active");
        // Malformed: spec.md with no meta.json and no header lines.
        let mdir = root.join(".claude").join("spec").join("2026-01-03-broken");
        std::fs::create_dir_all(&mdir).unwrap();
        std::fs::write(mdir.join("spec.md"), "# broken\n\n## Resumo\n\nx\n").unwrap();
        // Only the 1 Active counts; 1 active == cap 1 → at cap → deny in strict.
        let input = skill_input("feature", root.to_str().unwrap());
        let verdict = verdict_with(&input, root.to_str().unwrap(), LimitMode::Strict);
        assert!(verdict.is_blocking(), "only Active specs count; 1 == cap 1 → deny: {verdict:?}");
    }

    #[test]
    fn non_pre_tool_use_allows() {
        let dir = tempfile::tempdir().unwrap();
        let input = skill_input("feature", dir.path().to_str().unwrap());
        let ctx = Ctx {
            project_dir: dir.path().to_string_lossy().into_owned(),
            trigger: Some(Trigger::PostToolUse),
            workspace_root: None,
        };
        assert_eq!(
            ActiveSpecLimitGate.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }

    #[test]
    fn limit_mode_defaults_to_warn_when_unset() {
        // No test in this crate mutates `MUSTARD_MAX_ACTIVE_SPECS_MODE` (env
        // mutation is `unsafe` under Rust 2024 + `#![forbid(unsafe_code)]`), so
        // the var is unset here and the wrapper must default to the
        // hard-limit-with-escape-hatch `Warn`.
        assert_eq!(limit_mode(), LimitMode::Warn);
    }
}
