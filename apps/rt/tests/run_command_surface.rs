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
//! It also reads the SHIPPED instruction surfaces (`plugin/**`) from disk: the
//! CLI tree alone cannot catch a ritual that promises something the reader will
//! not find, and a wrong instruction fails just as silently as a dropped
//! registration.
//!
//! Adding a command: append its name here (sorted) in the same change. Renaming
//! or removing one: update every caller first — this list is the contract.

use std::fs;
use std::path::{Path, PathBuf};

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
    "wave-size-check",
    "wave-tree",
    "work-unit-open",
    "worktree-gc",
];

/// Instruction surfaces SHIPPED to the reader, relative to the repo root, with
/// the file extension each one is scanned through (`None` = every file).
///
/// `plugin/**/*.md` is what the agent loads at runtime (commands, refs, agent
/// prompts); `apps/dashboard/src` is what the UI prints in its hints. Each entry
/// is ASSERTED to exist and to yield files — a surface that silently disappears
/// would turn this guard into a green no-op.
const DOC_SURFACES: &[(&str, Option<&str>)] = &[("plugin", Some("md")), ("apps/dashboard/src", None)];

/// The repo root, resolved from this crate (`apps/rt`) so the scan does not
/// depend on the directory the test runner happens to start in.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Recursively collect files under `dir` in a deterministic (sorted) order,
/// keeping only `ext` when it is set. An unreadable directory yields nothing.
fn collect_files(dir: &Path, ext: Option<&str>, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            if name == "node_modules" || name == "target" || name == ".git" {
                continue;
            }
            collect_files(&path, ext, out);
        } else if ext
            .is_none_or(|want| path.extension().and_then(|e| e.to_str()).is_some_and(|e| e == want))
        {
            out.push(path);
        }
    }
}

/// Every `mustard-rt run <name>` token in `text`, in order of appearance.
///
/// All three invocation spellings are recognised — the same set
/// `template_parity`'s `CALLER_PREFIXES` feeds to its `extract_run_names`.
/// Recognising only the bare one
/// would let a Windows-flavoured hint (`mustard-rt.exe run …`) or a packaging
/// script (`$RtExe run …`) name a dead command and still pass.
///
/// A token runs to the first byte outside `[a-z0-9-]`, which drops the trailing
/// backtick / period / paren that normally closes the instruction in prose.
/// Placeholders are SKIPPED rather than reported: an explicit `<`, `{`, `$` or
/// backtick start teaches a shape, not a command, and any other non-token byte
/// (the `…` elision in `scan.md`) yields an empty token.
fn documented_run_tokens(text: &str) -> Vec<String> {
    const PREFIXES: &[&str] = &["mustard-rt run ", "mustard-rt.exe run ", "$RtExe run "];
    const PLACEHOLDER_STARTS: &[u8] = b"<{$`";

    let bytes = text.as_bytes();
    let mut out = Vec::new();
    // One cursor per spelling: each advances independently through the text, and
    // the run with the smallest next hit is consumed, so the tokens stay in order
    // of appearance no matter which spelling produced them.
    let mut cursors = vec![0usize; PREFIXES.len()];
    loop {
        let Some((which, pos)) = PREFIXES
            .iter()
            .enumerate()
            .filter_map(|(i, p)| text[cursors[i]..].find(p).map(|off| (i, cursors[i] + off)))
            .min_by_key(|(_, pos)| *pos)
        else {
            break;
        };
        let start = pos + PREFIXES[which].len();
        cursors[which] = start;
        // Every other cursor must clear this hit too, else the same region is
        // re-scanned forever by the spellings that did not match here.
        for (i, c) in cursors.iter_mut().enumerate() {
            if i != which && *c <= pos {
                *c = start;
            }
        }
        if start >= bytes.len() || PLACEHOLDER_STARTS.contains(&bytes[start]) {
            continue;
        }
        let mut end = start;
        while end < bytes.len()
            && (bytes[end].is_ascii_lowercase() || bytes[end].is_ascii_digit() || bytes[end] == b'-')
        {
            end += 1;
        }
        if end > start {
            out.push(text[start..end].to_string());
        }
    }
    out
}

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

/// Every `mustard-rt run <name>` a SHIPPED instruction surface tells the reader
/// (or an agent) to type must be a name the CLI actually publishes.
///
/// Field defect (btw-plan-rework-fixes): `wave-scaffold` was absorbed into
/// `plan-materialize`, but the dashboard's `wave-integrity` hint still told the
/// reader to run it. Nothing broke at build time — the command simply does not
/// exist, so an obedient agent burns a call on a clap error. `template_parity`
/// runs the same forward check over the template/plugin/packaging corpus; this
/// one adds `apps/dashboard/src`, which that corpus never walks — exactly where
/// the defect lived.
#[test]
fn every_documented_run_command_exists() {
    let root = repo_root();
    let mut offenders = Vec::new();

    for (rel, ext) in DOC_SURFACES {
        let dir = root.join(rel);
        // Assert the surface instead of skipping it (the idiom `template_parity`
        // already uses): a moved or renamed directory would otherwise make this
        // guard pass while scanning nothing — a dead guard reads exactly like a
        // clean one, which is the failure mode the whole test exists to prevent.
        assert!(dir.is_dir(), "declared instruction surface `{rel}` is missing — update DOC_SURFACES");
        let mut files = Vec::new();
        collect_files(&dir, *ext, &mut files);
        assert!(
            !files.is_empty(),
            "instruction surface `{rel}` yielded 0 files — the guard would pass vacuously"
        );
        for file in files {
            let Ok(text) = fs::read_to_string(&file) else {
                continue;
            };
            for name in documented_run_tokens(&text) {
                if !RUN_SUBCOMMANDS.contains(&name.as_str()) {
                    let shown = file.strip_prefix(&root).unwrap_or(&file);
                    offenders.push(format!("{} -> `mustard-rt run {name}`", shown.display()));
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "shipped instructions name `mustard-rt run` commands the CLI does not \
         publish — the call dies on a clap error at runtime, and the reader has \
         no way to tell. Fix the surface or register the command:\n{}",
        offenders.join("\n")
    );
}

/// The guard above is only as good as its tokenizer, and a tokenizer that
/// silently stops matching turns the whole test green-and-blind. Pin the three
/// spellings it must catch and the placeholder shapes it must ignore.
#[test]
fn documented_run_tokens_catches_every_spelling_and_skips_placeholders() {
    let found = documented_run_tokens(
        "run `mustard-rt run status` first.\n\
         On Windows: `mustard-rt.exe run doctor`.\n\
         Packaging uses `$RtExe run upsert`.\n\
         Shapes teach nothing: `mustard-rt run <name>`, `mustard-rt run {kind}`, \
         `mustard-rt run $Cmd`.\n",
    );
    assert_eq!(
        found,
        vec!["status", "doctor", "upsert"],
        "all three invocation spellings must be caught, in order, and every \
         placeholder skipped",
    );
    // Every name it caught here is real — the guard flags exactly the ones that
    // are not.
    for name in &found {
        assert!(RUN_SUBCOMMANDS.contains(&name.as_str()), "{name} should be a real command");
    }
    assert_eq!(
        documented_run_tokens("`mustard-rt run wave-scaffold` (the shipped defect)"),
        vec!["wave-scaffold"],
        "the absorbed command must still be recognised as a name — that is what \
         makes the guard fail when a surface names it",
    );
    assert!(!RUN_SUBCOMMANDS.contains(&"wave-scaffold"));
}

/// The shipped `pr close` ritual must NAME submodules.
///
/// `plugin/commands/git.md` declares "Submodules before parent, always" as an
/// iron rule, and its `commit`, `push` and `pr` steps each obey it — while
/// `pr close`, three lines below that rule, described a single-repo exit. The
/// tool followed the doc: it settled the parent, answered `settled`, and left
/// the submodule sitting on the work branch. `git-settle` now reports one entry
/// per repo (`repos` / `complete`); this keeps the instruction surface from
/// drifting back away from it.
#[test]
fn pr_close_ritual_names_submodules() {
    // Resolved through the shared `repo_root()` helper rather than a second
    // inline `CARGO_MANIFEST_DIR` join: the two units that met in this file each
    // taught it to read shipped surfaces, and keeping both resolutions is the
    // drift this test exists to catch.
    let git_md = repo_root().join("plugin/commands/git.md");
    let text = fs::read_to_string(&git_md).expect("plugin/commands/git.md is the shipped ritual");

    let row = text
        .lines()
        .find(|l| l.trim_start().starts_with("| `pr close"))
        .expect("the actions table still describes `pr close`");
    assert!(
        row.to_lowercase().contains("submodule"),
        "the `pr close` action must state the submodule-first order its own iron rule promises: {row}"
    );

    let step = text
        .lines()
        .find(|l| l.trim_start().starts_with("- **pr close**"))
        .expect("the procedure still spells out `pr close`");
    assert!(
        step.to_lowercase().contains("submodule"),
        "the `pr close` procedure must close each repo of the unit, submodules first: {step}"
    );
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

/// Same friction, the other half of the family: the four `--spec-dir` commands
/// were left out of the earlier alias fix, so the habit the interface teaches
/// (`--spec` / `--from-spec`) was punished here by a hard clap error and a
/// burned retry. `--spec-dir` stays canonical; the two siblings are hidden
/// aliases.
#[test]
fn spec_dir_flag_aliases_are_interchangeable() {
    let tree = run_command_tree();
    let accepts = |args: &[&str]| tree.clone().try_get_matches_from(args).is_ok();

    for flag in ["--spec-dir", "--spec", "--from-spec"] {
        assert!(
            accepts(&["run", "plan-materialize", flag, "d", "--plan", "p.json"]),
            "plan-materialize {flag}"
        );
        assert!(accepts(&["run", "pipeline-summary", flag, "d"]), "pipeline-summary {flag}");
        assert!(accepts(&["run", "wave-tree", flag, "d"]), "wave-tree {flag}");
        assert!(accepts(&["run", "wave-size-check", flag, "d"]), "wave-size-check {flag}");
    }
}
