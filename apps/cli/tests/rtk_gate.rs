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
    let real_git = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .expect("git on PATH");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real_git, dir.join("git")).expect("link git");
    dir
}

/// Run `mustard init --yes` in `project`, with `bin` as the whole PATH.
fn run_init(project: &Path, bin: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mustard"))
        .args(["init", "--yes"])
        .current_dir(project)
        .env_clear()
        .env("PATH", bin)
        .env("HOME", std::env::var("HOME").unwrap_or_default())
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

    let out = run_init(&project, &bin);

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

    let out = run_init(&project, &bin);

    assert!(out.status.success(), "a present rtk must let the install run");
    assert!(
        project.join(".claude").exists() && project.join("mustard.json").exists(),
        "the install must have written the project"
    );
    let spawned = fs::read_to_string(&log).unwrap_or_default();
    assert!(
        spawned.lines().any(|l| l.starts_with("rtk ")),
        "the binary must still reach the RTK tooling; log was:\n{spawned}"
    );
}
