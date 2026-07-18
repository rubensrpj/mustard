//! Locks the published `mustard-rt run` CLI surface.
//!
//! The `run <name>` commands are called by hooks, `settings.json`, the SKILL
//! templates and the orchestrator prompts across the whole product: a rename or
//! a dropped registration does not break the build, it makes the command
//! silently VANISH at runtime. That is exactly the failure the two-registration
//! rule (variant + `dispatch()` arm, per family in `commands/<family>/cli.rs`)
//! guards against by hand — this test guards it mechanically, straight off the
//! clap `Command` tree.
//!
//! Adding a command: append its name here (sorted) in the same change. Renaming
//! or removing one: update every caller first — this list is the contract.

use clap::{Command, Subcommand};
use mustard_rt::commands::RunCmd;

/// Every subcommand `mustard-rt run --help` publishes, sorted by name.
///
/// 80 declared variants + `help`, which clap generates at build time.
const RUN_SUBCOMMANDS: &[&str] = &[
    "active-specs",
    "adapt-cursor",
    "agent-prompt-render",
    "amend-finalize",
    "analyze-validation",
    "approve-spec",
    "artifact-update",
    "capability",
    "claude-dir-prune",
    "close-orchestrate",
    "close-pipeline",
    "complete-spec",
    "context-slice",
    "dependency-precheck",
    "diagnose-otel",
    "diff-context",
    "digest-adherence-finalize",
    "docs-stale-check",
    "doctor",
    "emit-event",
    "emit-phase",
    "emit-pipeline",
    "equivalence-learn",
    "event-projections",
    "exec-rewave-check",
    "feature",
    "gate-regression-check",
    "git-settle",
    "glossary-coverage",
    "grill-capture",
    "help",
    "language-audit",
    "maint-deps",
    "maint-validate",
    "mark-checklist-item",
    "metrics",
    "metrics-wave-status",
    "orient",
    "otel-collector",
    "otel-stop",
    "pipeline-summary",
    "plan-materialize",
    "plan-prepare",
    "qa-run",
    "rebuild-specs",
    "rehook",
    "resume-bootstrap",
    "review-dispatch",
    "review-prefetch",
    "review-result",
    "scan",
    "scan-guards-apply",
    "scan-guards-list",
    "scan-patterns-apply",
    "scan-patterns-decline",
    "scan-patterns-list",
    "scan-patterns-sweep",
    "scan-spec",
    "scope-classify",
    "scope-decompose",
    "security-scan",
    "spec-children",
    "spec-children-tree",
    "spec-draft",
    "status",
    "statusline",
    "tactical-fix-create",
    "tactical-fix-detect",
    "unhook",
    "upsert",
    "verify-pipeline",
    "wave-advance",
    "wave-collapse",
    "wave-dependency",
    "wave-done",
    "wave-files",
    "wave-scaffold",
    "wave-size-check",
    "wave-tree",
    "work-unit-open",
    "worktree-gc",
];

/// Build the `run` command tree exactly as `main.rs` hands it to clap.
fn run_command_tree() -> Command {
    let mut cmd = RunCmd::augment_subcommands(Command::new("run"));
    // `build()` materialises what the parser/help actually expose (it is what
    // adds the auto-generated `help` subcommand).
    cmd.build();
    cmd
}

#[test]
fn run_subcommand_names_are_locked() {
    let cmd = run_command_tree();
    let mut names: Vec<&str> = cmd.get_subcommands().map(clap::Command::get_name).collect();
    names.sort_unstable();

    assert_eq!(
        names, RUN_SUBCOMMANDS,
        "the `run` CLI surface changed: hooks, settings.json and the SKILL \
         templates call these names by hand, so a rename or a dropped \
         registration silently kills the command"
    );
}

#[test]
fn every_declared_command_keeps_its_help_slot() {
    // clap orders the flat `run --help` listing by `(display_order, name)`.
    // The families are split across `commands/<family>/cli.rs`, so each variant
    // pins its historical slot explicitly. A duplicate or a gap would reshuffle
    // the published listing — assert the 80 declared commands still carry the
    // exact permutation 0..=79 (`help` is clap's own, appended last).
    let cmd = run_command_tree();
    let mut orders: Vec<usize> = cmd
        .get_subcommands()
        .filter(|c| c.get_name() != "help")
        .map(clap::Command::get_display_order)
        .collect();
    orders.sort_unstable();

    let expected: Vec<usize> = (0..RUN_SUBCOMMANDS.len() - 1).collect();
    assert_eq!(orders, expected, "display_order slots must stay a gapless permutation");
}

/// The `--spec` / `--from-spec` flags are interchangeable on the spec-path
/// commands. Field friction (sialia): an orchestrator that reached for the
/// sibling command's flag (`scope-classify --spec` / `analyze-validation
/// --from-spec`) hit a hard clap error and burned a retry. Each command keeps
/// its canonical flag and accepts the sibling spelling as a hidden alias.
#[test]
fn spec_path_flag_aliases_are_interchangeable() {
    let tree = run_command_tree();
    let accepts = |args: &[&str]| tree.clone().try_get_matches_from(args).is_ok();

    // Canonical `--from-spec`, alias `--spec`.
    for flag in ["--from-spec", "--spec"] {
        assert!(accepts(&["run", "scope-classify", flag, "x.md"]), "scope-classify {flag}");
        assert!(accepts(&["run", "plan-prepare", flag, "x.md"]), "plan-prepare {flag}");
        assert!(accepts(&["run", "scope-decompose", flag, "x.md"]), "scope-decompose {flag}");
    }
    // Canonical `--spec`, alias `--from-spec`.
    for flag in ["--spec", "--from-spec"] {
        assert!(accepts(&["run", "analyze-validation", flag, "x.md"]), "analyze-validation {flag}");
        assert!(accepts(&["run", "qa-run", flag, "x"]), "qa-run {flag}");
    }
}
