//! The library half of `init` may not act on the machine — pinned by OBSERVING
//! the machine, not by reading the source.
//!
//! ## Why this file exists
//!
//! Four reviews in a row found the same shape: `init_with_templates` is a
//! `Result`-returning library function that took environment acts — first
//! `process::exit(1)` through the RTK gate, then `sh -c "curl … | sh"` through
//! `ensure_rtk`, then `$HOME/.claude/settings.json` through
//! `ensure_global_permissions`. Each moved to `cli::dispatch`, where the binary —
//! and only the binary — may take them.
//!
//! A substring ratchet over `init.rs` was tried first and defeated by five
//! spellings. So this measures CONSEQUENCES instead. That version was defeated
//! too, six ways, and every one of them is a lesson this file now encodes:
//!
//! 1. **Do not shim the tool you are watching for.** `rg` was in the shim list,
//!    so `rg_on_path()` succeeded and `ensure_ripgrep` returned before doing
//!    anything. The pin disarmed the half it claimed to measure. Neither `rg`
//!    nor `rtk` is shimmed here, so both installers engage and announce
//!    themselves.
//! 2. **Watch the whole `$HOME`, not one path.** The old assertion named
//!    `.claude/settings.json`; a write to `.bashrc` passed green.
//! 3. **Deny by default.** The spawn log was filtered for four known installer
//!    spellings, so `brew` / `apt-get` / `npm` / `pip` went unseen — and really
//!    executed. Now every logged line must be `git`; anything else fails.
//! 4. **Run in an environment a real caller has.** `env_clear()` plus four
//!    variables is not one: acts gated on `TERM` were invisible. The child now
//!    inherits and overrides only what it must.
//! 5. **Calibrate the instrument.** The log was read through
//!    `unwrap_or_default()` and asserted empty — a broken rig and a clean run
//!    were the same answer. The log must now be non-empty.
//! 6. **Cover every public door.** Only `init_with_templates` was driven;
//!    restoring the acts into the sibling `init` — the entry point its own doc
//!    advertises — was green. Both are driven now.
//!
//! The child is this same test binary, re-run with [`PROBE_ENV`] set.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use mustard_cli::commands::init::{InitOptions, InitOutcome, init, init_with_templates};

/// Set on the child: run the probe instead of the assertions.
const PROBE_ENV: &str = "MUSTARD_LIB_PURITY_PROBE";

/// Every external tool the install could plausibly reach for, EXCEPT the two it
/// is watched for reaching (`rtk`, `rg`) — shimming those would make their
/// probes succeed and the installers return before acting, which is lesson 1.
const SHIMMED: &[&str] = &[
    "sh", "bash", "zsh", "curl", "wget", "cargo", "rustup", "scoop", "choco", "winget", "brew",
    "apt", "apt-get", "yum", "dnf", "pacman", "apk", "npm", "pnpm", "yarn", "pip", "pip3",
];

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

/// Shims that answer and record. `git` is the real one: `init` reads repository
/// state, and faking it would take paths the product never takes.
fn shim_dir(root: &Path, log: &Path) -> PathBuf {
    let dir = root.join("bin");
    fs::create_dir_all(&dir).expect("mkdir shims");
    for tool in SHIMMED {
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
        // `git` is the REAL one, behind a wrapper that also logs. A bare symlink
        // was the first spelling and it made the log empty at HEAD: the only
        // tool the library runs is git, so nothing was ever recorded, and the
        // "no foreign tool" assertion was asserting over a file that did not
        // exist. Logging git is what calibrates the rig — see lesson 5.
        let real_git = Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .expect("git on PATH");
        let wrapper = format!(
            "#!/bin/sh\necho \"git $*\" >> \"{}\"\nexec {} \"$@\"\n",
            log.display(),
            real_git
        );
        let path = dir.join("git");
        fs::write(&path, wrapper).expect("write git wrapper");
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod git");
    }
    dir
}

/// The child half: drive BOTH public entry points, nothing else. A no-op in a
/// normal run.
#[test]
fn library_probe_child() {
    let Ok(work) = std::env::var(PROBE_ENV) else {
        return;
    };
    let work = PathBuf::from(work);
    let templates = fake_templates(&work);
    let opts = InitOptions { yes: true, ..InitOptions::default() };

    // Door 1: the explicit-templates entry point.
    let first = init_with_templates(&work.join("project"), &templates, &opts)
        .expect("init_with_templates runs");
    assert_eq!(first, InitOutcome::Installed, "a seeded project reports Installed");

    // Door 2: `init`, which its own doc calls "the library entry point the
    // dashboard backend calls". Restoring the acts HERE was green before this
    // call existed. `MUSTARD_TEMPLATES_DIR` is how the parent points it at the
    // fixture without a process-global default.
    let second = init(&work.join("project-two"), &opts).expect("init runs");
    assert_eq!(second, InitOutcome::Installed, "the second door also installs");

    // Door 1 again, in dry-run: pins that `DryRun` is not `Installed` at the
    // library level, where no terminal is needed to reach it.
    let dry = init_with_templates(
        &work.join("project-three"),
        &templates,
        &InitOptions { yes: true, dry_run: true, ..InitOptions::default() },
    )
    .expect("a dry run runs");
    assert_eq!(dry, InitOutcome::DryRun, "a dry run must report DryRun, not Installed");
}

/// A library call must leave the machine exactly as it found it — whatever the
/// call is spelled like, whichever door it comes through.
#[test]
#[cfg_attr(not(unix), ignore = "the shims are shell scripts")]
fn a_library_init_touches_nothing_outside_the_project() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let work = tmp.path();
    let home = work.join("home");
    let log = work.join("spawn.log");
    fs::create_dir_all(&home).expect("mkdir home");
    for name in ["project", "project-two", "project-three"] {
        let project = work.join(name);
        fs::create_dir_all(&project).expect("mkdir project");
        Command::new("git")
            .args(["init", "-q", "."])
            .current_dir(&project)
            .output()
            .expect("git init");
    }
    let bin = shim_dir(work, &log);

    // INHERIT the environment and override only what the probe needs. Clearing
    // it and re-adding a handful puts the code under measurement in a world no
    // real caller lives in — an act gated on `TERM` was invisible that way.
    let out = Command::new(std::env::current_exe().expect("current exe"))
        .args(["--exact", "library_probe_child", "--nocapture"])
        .env(PROBE_ENV, work)
        .env("PATH", &bin)
        .env("HOME", &home)
        .env("TERM", "xterm-256color")
        .env("MUSTARD_TEMPLATES_DIR", work.join("templates"))
        // Armed on purpose: with the opt-in OFF this passes even if the library
        // calls the global-settings write.
        .env("MUSTARD_GLOBAL_PERMISSIONS", "1")
        .output()
        .expect("the probe child runs");

    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "the probe child failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        work.join("project").join(".claude").exists()
            && work.join("project-two").join(".claude").exists(),
        "both doors must actually have installed, or this proves nothing"
    );

    // 1. `$HOME` must be untouched ENTIRELY — not one enumerated path.
    let leftovers: Vec<String> = fs::read_dir(&home)
        .expect("read home")
        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect();
    assert!(
        leftovers.is_empty(),
        "a library call wrote into $HOME: {leftovers:?} — environment acts belong to cli::dispatch"
    );

    // 2. The instrument must have recorded something, or an empty log would be
    //    indistinguishable from a broken rig.
    let spawned = fs::read_to_string(&log).expect("the shim log exists — the rig must be live");
    assert!(
        !spawned.trim().is_empty(),
        "the shim log is empty: the rig did not observe even `git`, so its silence means nothing"
    );

    // 3. DENY BY DEFAULT: `git` is the only tool the library may run. An
    //    allowlist of known installer spellings missed brew/apt-get/npm/pip.
    let foreign: Vec<&str> = spawned
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with("git "))
        .collect();
    assert!(
        foreign.is_empty(),
        "a library call ran something other than git:\n{}\nfull log:\n{spawned}",
        foreign.join("\n")
    );

    // 4. Neither installer may even ANNOUNCE itself. On Linux `install_ripgrep`
    //    spawns nothing, so its only trace is what it prints — without this the
    //    whole ripgrep half of the invariant would be unobservable here.
    for trace in ["ripgrep", "RTK", "Global settings", "Global env"] {
        assert!(
            !stdout.contains(trace),
            "the library reached an environment act: found {trace:?} in the child's output:\n{stdout}"
        );
    }
}
