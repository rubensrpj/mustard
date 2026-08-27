//! The library half of `init` may not act on the machine — pinned by OBSERVING
//! the machine, not by reading the source.
//!
//! ## Why this file exists, and why the source ratchet did not suffice
//!
//! Three reviews in a row found the same shape: `init_with_templates` is a
//! `Result`-returning library function that took environment acts — first
//! `process::exit(1)` through the RTK gate, then `sh -c "curl … | sh"` through
//! `ensure_rtk`, then `$HOME/.claude/settings.json` through
//! `ensure_global_permissions`. Each was moved to `cli::dispatch`, where the
//! binary — and only the binary — may take them.
//!
//! The first attempt to pin that was a substring check over `init.rs`. It was
//! measured and defeated: `self::ensure_rtk();`, a local alias, tab indentation
//! and a helper in a sibling module all restored the call with the ratchet
//! green, and under the first spelling a library call really did spawn the curl
//! pipeline. A ratchet that reads text can only ever know the spellings its
//! author imagined.
//!
//! So this observes CONSEQUENCES. It runs the library in a child process with a
//! throwaway `$HOME`, `MUSTARD_GLOBAL_PERMISSIONS=1` (arming the very write we
//! forbid) and a PATH of logging shims, then asserts the machine is untouched.
//! Any spelling that reintroduces an environment act fails this, because the act
//! itself is what is measured.
//!
//! The child is this same test binary, re-run with [`PROBE_ENV`] set — the
//! cheapest way to call a library function in a process whose whole environment
//! we control.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use mustard_cli::commands::init::{InitOptions, InitOutcome, init_with_templates};

/// Set on the child: run the probe instead of the assertions.
const PROBE_ENV: &str = "MUSTARD_LIB_PURITY_PROBE";

/// The minimal payload `init` seeds from.
fn fake_templates(root: &Path) -> PathBuf {
    let templates = root.join("templates");
    fs::create_dir_all(templates.join("mustard")).expect("mkdir templates");
    fs::write(templates.join("mustard/orchestrator.md"), "# Orchestrator Rules\n")
        .expect("write orchestrator");
    fs::write(templates.join("settings.json"), r#"{"env":{"MUSTARD_TEST":"1"}}"#)
        .expect("write settings");
    fs::write(templates.join(".gitignore"), "spec/*/.events/\n").expect("write gitignore");
    templates
}

/// Executable shims that answer and record. `git` is the real one: `init` reads
/// repository state, and faking it would take paths the product never takes.
fn shim_dir(root: &Path, log: &Path) -> PathBuf {
    let dir = root.join("bin");
    fs::create_dir_all(&dir).expect("mkdir shims");
    for tool in ["sh", "bash", "curl", "cargo", "scoop", "rg", "rtk"] {
        let script = format!("#!/bin/sh\necho \"{tool} $*\" >> \"{}\"\nexit 0\n", log.display());
        let path = dir.join(tool);
        fs::write(&path, script).expect("write shim");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod shim");
        }
    }
    #[cfg(unix)]
    {
        let real_git = Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .expect("git on PATH");
        std::os::unix::fs::symlink(real_git, dir.join("git")).expect("link git");
    }
    dir
}

/// The child half: call the library, nothing else. A no-op in a normal run.
#[test]
fn library_probe_child() {
    let Ok(work) = std::env::var(PROBE_ENV) else {
        return;
    };
    let work = PathBuf::from(work);
    let templates = fake_templates(&work);
    let project = work.join("project");
    let outcome = init_with_templates(
        &project,
        &templates,
        &InitOptions { yes: true, ..InitOptions::default() },
    )
    .expect("the library install runs");
    assert_eq!(
        outcome,
        InitOutcome::Installed,
        "a seeded project must report Installed"
    );
}

/// A library call must leave the machine exactly as it found it: no global
/// settings write, no tool installer, no matter how the call is spelled.
#[test]
#[cfg_attr(not(unix), ignore = "the shims are shell scripts")]
fn a_library_init_touches_nothing_outside_the_project() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let work = tmp.path();
    let home = work.join("home");
    let log = work.join("spawn.log");
    fs::create_dir_all(&home).expect("mkdir home");
    let project = work.join("project");
    fs::create_dir_all(&project).expect("mkdir project");
    Command::new("git")
        .args(["init", "-q", "."])
        .current_dir(&project)
        .output()
        .expect("git init");
    let bin = shim_dir(work, &log);

    let out = Command::new(std::env::current_exe().expect("current exe"))
        .args(["--exact", "library_probe_child", "--nocapture"])
        .env_clear()
        .env(PROBE_ENV, work)
        .env("PATH", &bin)
        .env("HOME", &home)
        // Arm the act we forbid: with the opt-in OFF this test would pass even
        // if the library called it.
        .env("MUSTARD_GLOBAL_PERMISSIONS", "1")
        .output()
        .expect("the probe child runs");

    assert!(
        out.status.success(),
        "the probe child failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        project.join(".claude").exists(),
        "the probe must actually have installed, or it proves nothing"
    );

    // 1. No global settings write, with the opt-in explicitly ON.
    assert!(
        !home.join(".claude").join("settings.json").exists(),
        "a library call wrote {}/.claude/settings.json — environment acts belong \
         to cli::dispatch",
        home.display()
    );

    // 2. No tool installer, in any spelling.
    let spawned = fs::read_to_string(&log).unwrap_or_default();
    let installers: Vec<&str> = spawned
        .lines()
        .filter(|l| {
            l.starts_with("rtk init")
                || l.starts_with("scoop ")
                || l.contains("install.sh")
                || l.contains("cargo install")
        })
        .collect();
    assert!(
        installers.is_empty(),
        "a library call spawned installers:\n{}\nfull log:\n{spawned}",
        installers.join("\n")
    );
}
