//! The RTK hard gate, pinned where it now lives: `cli::dispatch`, not the
//! library.
//!
//! ## Why this test exists
//!
//! The gate used to sit inside `init_with_templates`, which returns `Result<()>`
//! — so a library caller could be killed by `process::exit(1)` instead of
//! getting an error. It moved to the binary's dispatch arm, and the move was
//! reviewed with two findings this file answers:
//!
//! 1. nothing pinned the gate in its new home, so a later edit could delete it
//!    silently;
//! 2. the first verification used `PATH=/nonexistent`, which makes every spawn
//!    fail instantly and therefore hides what the process actually tries to run.
//!
//! So this drives the REAL binary as a child process — the only way to observe a
//! `process::exit` as an assertion rather than as a casualty — and it does so
//! against a SHIMMED PATH: `sh`, `curl`, `cargo` and `scoop` all answer, and
//! every invocation is logged. A gate that quietly stopped gating, or an
//! installer that came back into the library, both show up here as a changed
//! log rather than as a green run.
//!
//! LIMIT, declared rather than hidden: the two behavioural tests are
//! `ignore`d off unix, because the shims are shell scripts. Windows is where
//! `install_rtk` takes its most expensive branch (`scoop install rtk`, then
//! `cargo install --git`), so the gate has no coverage on the runner where it
//! would cost the most. Closing that means shims the Windows shell can run —
//! its own unit, not a line here.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Build a PATH directory where every external tool answers and records itself.
/// `git` is the real one — `init` reads repository state, and a stub would make
/// the install take paths no operator ever takes.
fn shim_dir(log: &Path, with_rtk: bool) -> PathBuf {
    let dir = log.parent().expect("the log has a parent").join("bin");
    fs::create_dir_all(&dir).expect("mkdir shim dir");
    let mut tools = vec!["sh", "bash", "curl", "cargo", "scoop", "rg"];
    if with_rtk {
        tools.push("rtk");
    }
    for tool in tools {
        let script = format!(
            "#!/bin/sh\necho \"{tool} $*\" >> \"{}\"\nexit 0\n",
            log.display()
        );
        let path = dir.join(tool);
        fs::write(&path, script).expect("write shim");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod shim");
        }
    }
    // The real git: `init` inspects the repository, and faking that would test a
    // path the product never runs.
    //
    // The whole block is unix-only, and BOTH halves have to be. It used to
    // compute `real_git` unconditionally and only symlink it under
    // `#[cfg(unix)]`, which made the variable unused on Windows — invisible
    // while warnings were merely warnings, and the FIRST thing `-D warnings`
    // caught when this repository turned it on. The lookup would not have
    // worked there either: it shells out to `sh -c command -v git`, and the
    // `.expect` would have panicked rather than degraded.
    #[cfg(unix)]
    {
        let real_git = Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .expect("git on PATH");
        std::os::unix::fs::symlink(&real_git, dir.join("git")).expect("link git");
    }
    dir
}

/// Run `mustard init --yes` in `project`, with `bin` as the whole PATH and
/// `home` as `$HOME`.
///
/// The home is a PARAMETER, never the operator's own. It used to be
/// `std::env::var("HOME")`, and review measured the cost: regress the
/// global-settings opt-in and `cargo test` writes into the developer's real
/// `~/.claude/`. A test that can damage the machine it runs on is worse than the
/// regression it was watching for.
fn run_init(project: &Path, bin: &Path, home: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mustard"))
        .args(["init", "--yes"])
        .current_dir(project)
        .env_clear()
        .env("PATH", bin)
        .env("HOME", home)
        .output()
        .expect("the mustard binary runs")
}

fn fresh_repo(root: &Path) -> PathBuf {
    let project = root.join("project");
    fs::create_dir_all(&project).expect("mkdir project");
    Command::new("git")
        .args(["init", "-q", "."])
        .current_dir(&project)
        .output()
        .expect("git init");
    project
}

/// Without `rtk` the install refuses BEFORE touching disk, and says so with a
/// non-zero exit. This is the behaviour the gate exists for, and the reason it
/// may end the process at all.
#[test]
#[cfg_attr(not(unix), ignore = "the shims are shell scripts")]
fn a_missing_rtk_refuses_the_install_and_writes_nothing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let log = tmp.path().join("spawn.log");
    let bin = shim_dir(&log, false);
    let project = fresh_repo(tmp.path());
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).expect("mkdir home");

    let out = run_init(&project, &bin, &home);

    assert!(!out.status.success(), "a missing rtk must fail the install");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("RTK"),
        "the refusal must name what is missing"
    );
    assert!(
        !project.join(".claude").exists() && !project.join("mustard.json").exists(),
        "the gate refuses BEFORE touching disk — found a half-written install"
    );
}

/// With `rtk` present the install completes AND still reaches the best-effort
/// tool installers. They moved to the dispatch arm together with the gate; this
/// asserts the terminal user did not quietly lose them.
#[test]
#[cfg_attr(not(unix), ignore = "the shims are shell scripts")]
fn the_binary_still_runs_the_tool_installers_after_a_successful_install() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let log = tmp.path().join("spawn.log");
    let bin = shim_dir(&log, true);
    let project = fresh_repo(tmp.path());
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).expect("mkdir home");

    let out = run_init(&project, &bin, &home);

    assert!(out.status.success(), "a present rtk must let the install run");
    assert!(
        project.join(".claude").exists() && project.join("mustard.json").exists(),
        "the install must have written the project"
    );
    let spawned = fs::read_to_string(&log).unwrap_or_default();
    // `rtk init -g --no-patch`, NOT `rtk `. The prefix was the first spelling and
    // it pinned nothing: the gate itself runs `rtk --version` through
    // `rtk_on_path` before `init` even starts, so the assertion passed with both
    // installer calls deleted from dispatch (measured in review). Only
    // `ensure_rtk` issues this line.
    assert!(
        spawned.lines().any(|l| l.starts_with("rtk init")),
        "the binary must still reach the RTK tooling; log was:\n{spawned}"
    );
}

/// `--dry-run` prints a plan and changes nothing — including the machine.
///
/// This pins one half of `InitOutcome`'s mapping by CONSEQUENCE. Review measured
/// that flipping `Ok(InitOutcome::DryRun)` to `::Installed` left the whole suite
/// green while the flipped binary went on to write `~/.claude/settings.json` and
/// run `rtk init -g --no-patch` on a run that was supposed to touch nothing.
/// Dispatch has no `!dry_run` guard of its own any more; the enum is the guard,
/// so the enum needs a test.
#[test]
#[cfg_attr(not(unix), ignore = "the shims are shell scripts")]
fn a_dry_run_changes_neither_the_project_nor_the_machine() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let log = tmp.path().join("spawn.log");
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).expect("mkdir home");
    let bin = shim_dir(&log, true);
    let project = fresh_repo(tmp.path());

    let out = Command::new(env!("CARGO_BIN_EXE_mustard"))
        .args(["init", "--yes", "--dry-run"])
        .current_dir(&project)
        .env_clear()
        .env("PATH", &bin)
        .env("HOME", &home)
        // Armed on purpose: with the opt-in off this would pass even if the
        // dry run wrote global settings.
        .env("MUSTARD_GLOBAL_PERMISSIONS", "1")
        .output()
        .expect("the mustard binary runs");

    assert!(out.status.success(), "a dry run must succeed");
    assert!(
        !project.join(".claude").exists() && !project.join("mustard.json").exists(),
        "a dry run wrote into the project"
    );
    assert!(
        !home.join(".claude").join("settings.json").exists(),
        "a dry run wrote the operator's global settings"
    );
    let spawned = fs::read_to_string(&log).unwrap_or_default();
    assert!(
        !spawned.lines().any(|l| l.starts_with("rtk init") || l.starts_with("scoop ")),
        "a dry run ran a tool installer; log was:\n{spawned}"
    );
}

/// The library must not take environment acts. This reads the source rather than
/// running anything, because the regression it guards is a CALL coming back —
/// and a behavioural test cannot see that from inside the same process.
///
/// It is the WEAKER of the two guards on that invariant and is kept only as a
/// fast, readable signpost: `library_is_pure.rs` is the one that actually holds,
/// because it measures the acts instead of matching their spelling.
///
/// Why a ratchet at all: review measured that restoring `ensure_rtk()` /
/// `ensure_ripgrep()` into `init_with_templates` left the entire suite green,
/// including this file's other two tests. A revert of the fix was invisible.
#[test]
fn the_library_half_of_init_calls_no_environment_installer() {
    let source = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/commands/init.rs"),
    )
    .expect("init.rs is readable");

    for call in ["    ensure_rtk();", "    ensure_ripgrep();"] {
        assert!(
            !source.contains(call),
            "`{}` is back inside init.rs. These installers belong to `cli::dispatch`: \
             from the library they run `sh -c \"curl … | sh\"` for any caller, which is \
             how the dashboard's integration test came to spawn it twice on CI.",
            call.trim()
        );
    }

    // And the call site that IS allowed must still exist, so this test cannot
    // pass by the installers having disappeared altogether.
    let dispatch = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/cli.rs"),
    )
    .expect("cli.rs is readable");
    for call in [
        "init::ensure_rtk();",
        "init::ensure_ripgrep();",
        "init::ensure_global_permissions_if_opted_in();",
    ] {
        assert!(
            dispatch.contains(call),
            "`{call}` vanished from cli::dispatch — the terminal user lost the tooling"
        );
    }

    // And they must be gated on what the run actually DID, never on "no error".
    // `Ok` covers the operator answering Cancel to an existing `.claude/`, and on
    // that path this arm once ran `rtk init -g --no-patch` — a global write after
    // an explicit refusal. Measured through a pty in review.
    assert!(
        dispatch.contains("outcome == init::InitOutcome::Installed"),
        "the installers must be gated on InitOutcome::Installed; `is_ok()` also \
         means the operator cancelled, and acting on a refusal is the defect this \
         whole exercise is about"
    );
}

/// Answering **Cancel** to an existing `.claude/` must leave the machine alone.
///
/// This is the defect `InitOutcome` exists for: the installers were once gated
/// on `outcome.is_ok()`, and `Ok` covers Cancel, so a refused run still wrote
/// RTK's global config. Two independent pty measurements found it; this pins it.
///
/// Linux only, and deliberately not `unix`: the prompt is an arrow-key menu, so
/// the run needs a real terminal, and `script -qec` is the Linux spelling —
/// macOS's `script` takes neither flag. The ubuntu runner is where a regression
/// would be caught, and saying so beats a test that quietly skips everywhere.
#[test]
#[cfg_attr(not(target_os = "linux"), ignore = "needs `script -qec` for a pty")]
fn answering_cancel_leaves_the_machine_untouched() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let log = tmp.path().join("spawn.log");
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).expect("mkdir home");
    let bin = shim_dir(&log, true);
    let project = fresh_repo(tmp.path());

    // First install, so the second run meets an existing `.claude/`.
    let first = run_init(&project, &bin, &home);
    assert!(first.status.success(), "the seeding run must succeed");
    fs::write(&log, "").expect("truncate log");

    // Second run, interactive: one arrow-down moves from the Merge default to
    // Cancel, then Enter.
    let script = format!(
        "cd {} && env -i PATH={} HOME={} MUSTARD_GLOBAL_PERMISSIONS=1 {} init",
        project.display(),
        bin.display(),
        home.display(),
        env!("CARGO_BIN_EXE_mustard"),
    );
    let mut child = Command::new("script")
        .args(["-qec", &script, "/dev/null"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("script(1) runs");
    {
        use std::io::Write as _;
        let stdin = child.stdin.as_mut().expect("child stdin");
        stdin.write_all(b"\x1b[B\n").expect("send arrow-down + enter");
    }
    let out = child.wait_with_output().expect("the cancelled run finishes");
    let screen = String::from_utf8_lossy(&out.stdout).replace('\r', "");
    assert!(
        screen.contains("Cancel"),
        "the run did not reach the Cancel choice; screen was:\n{screen}"
    );

    assert!(
        !home.join(".claude").join("settings.json").exists(),
        "a cancelled run wrote the operator's global settings"
    );
    let spawned = fs::read_to_string(&log).unwrap_or_default();
    assert!(
        !spawned.lines().any(|l| l.starts_with("rtk init") || l.starts_with("scoop ")),
        "a cancelled run ran a tool installer; log was:\n{spawned}"
    );
}

/// The POSITIVE control for the global-settings act.
///
/// Every other assertion in this file and in `library_is_pure.rs` says the act
/// must NOT happen somewhere. Review measured what that leaves open: deleting
/// `init::ensure_global_permissions_if_opted_in();` from dispatch kills the
/// feature outright and every one of those tests stays green, because "never
/// written" satisfies them all. A one-sided pin cannot tell a correctly placed
/// act from a deleted one.
#[test]
#[cfg_attr(not(unix), ignore = "the shims are shell scripts")]
fn the_binary_still_writes_global_settings_when_the_operator_opted_in() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let log = tmp.path().join("spawn.log");
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).expect("mkdir home");
    let bin = shim_dir(&log, true);
    let project = fresh_repo(tmp.path());

    let out = Command::new(env!("CARGO_BIN_EXE_mustard"))
        .args(["init", "--yes"])
        .current_dir(&project)
        .env_clear()
        .env("PATH", &bin)
        .env("HOME", &home)
        .env("MUSTARD_GLOBAL_PERMISSIONS", "1")
        .output()
        .expect("the mustard binary runs");

    assert!(out.status.success(), "the install must succeed");
    assert!(
        home.join(".claude").join("settings.json").exists(),
        "the opted-in operator lost the global-settings write; stdout was:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
}
