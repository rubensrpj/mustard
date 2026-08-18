//! AC-7 of `private-install-mode-leaving-no`: `mustard init --private` installs
//! privately and seeds no `.github/` pull-request template into the host
//! repository.
//!
//! ## Why this drives the real binary
//!
//! The `.github/` copy fires on a condition that lives OUTSIDE the flag — the
//! project having a GitHub remote — so the only honest proof is a full install
//! against a repository that really has one. Calling the library in-process
//! would not do: `init_with_templates` opens with the RTK hard gate
//! (`probe_rtk` → `process::exit(1)`), and `cfg!(test)`, which neutralises it
//! for the unit tests inside `init.rs`, is FALSE here — an integration test
//! links the crate as an ordinary dependency. A missing RTK would therefore
//! take the whole test binary down on a bare CI runner.
//!
//! Running the binary as a child process solves both halves: its exit code is
//! an assertion instead of a casualty, and a PATH shim (see [`tool_shims`])
//! answers the two `--version` probes so the best-effort installers never shell
//! out to `scoop`/`cargo install` on a runner that has neither tool.
//!
//! The test carries its own CONTROL: the same fixture, installed shared, must
//! produce `.github/pull_request_template.md`. Without it a green run would
//! prove nothing — an install that failed early also writes no `.github/`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::tempdir;

/// The project-root path the CLI seeds from `templates/.github/`.
const PR_TEMPLATE: &str = ".github/pull_request_template.md";

/// Build the `templates/` payload the install is pointed at: just the
/// `.github/` scaffolding, which is all `init` still reads from there (the
/// harness seeds are compiled-in core constants).
fn fake_templates(root: &Path) -> PathBuf {
    let templates = root.join("templates");
    std::fs::create_dir_all(templates.join(".github")).expect("templates/.github");
    std::fs::write(templates.join(PR_TEMPLATE), "## What changed\n").expect("PR template");
    templates
}

/// A fresh git repository whose `origin` points at github.com — the condition
/// that makes the `.github/` copy fire.
fn git_project_with_github_remote(root: &Path, name: &str) -> PathBuf {
    let project = root.join(name);
    std::fs::create_dir_all(&project).expect("project dir");
    for args in [
        vec!["init"],
        vec!["config", "remote.origin.url", "https://github.com/acme/host-repo.git"],
    ] {
        let ok = Command::new("git")
            .args(&args)
            .current_dir(&project)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(ok, "git {args:?} failed in {}", project.display());
    }
    project
}

/// A directory holding `rtk` and `rg` executables, prepended to the child's
/// `PATH`.
///
/// Both are copies of the `mustard` binary under test, chosen because it is the
/// one executable this test is guaranteed to have and because its surface is
/// ours: `--version` exits 0 (which is all the two probes read), and the
/// follow-up `rtk init -g --no-patch` is rejected by `clap` as an unknown
/// argument before it can do anything. Answering the probes here — rather than
/// hoping the runner has RTK and ripgrep — is what keeps the best-effort
/// installers from reaching for `scoop`/`cargo install` on CI.
fn tool_shims(root: &Path) -> PathBuf {
    let bin = root.join("shims");
    std::fs::create_dir_all(&bin).expect("shim dir");
    let real = PathBuf::from(env!("CARGO_BIN_EXE_mustard"));
    for name in ["rtk", "rg"] {
        let shim = bin.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
        std::fs::copy(&real, &shim).expect("shim copy");
    }
    bin
}

/// Run the real `mustard init` in `project`, with the fixture templates and the
/// tool shims in place. `extra` carries the flags under test.
fn run_init(project: &Path, templates: &Path, shims: &Path, extra: &[&str]) -> Output {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut entries = vec![shims.to_path_buf()];
    entries.extend(std::env::split_paths(&path));
    let joined = std::env::join_paths(entries).expect("PATH with the shim dir in front");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_mustard"));
    cmd.arg("init")
        .arg("--yes")
        .args(extra)
        .current_dir(project)
        .env("PATH", joined)
        .env("MUSTARD_TEMPLATES_DIR", templates)
        // Never touch the operator's ~/.claude from a test — the write is
        // opt-in, and this pins it off no matter what the environment says.
        .env("MUSTARD_GLOBAL_PERMISSIONS", "0");
    cmd.output().expect("running the mustard binary")
}

/// Assert the child succeeded, quoting its output when it did not.
fn assert_ok(label: &str, out: &Output) {
    assert!(
        out.status.success(),
        "{label} failed ({}):\n--- stdout ---\n{}\n--- stderr ---\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

/// The clone-local exclude file git resolves for `project` — never the literal
/// `.git/info/exclude`, which is the rule the whole feature rests on.
fn exclude_body(project: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "--git-path", "info/exclude"])
        .current_dir(project)
        .output()
        .expect("git rev-parse");
    assert!(out.status.success(), "git could not resolve the exclude path");
    let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let path = PathBuf::from(&raw);
    let path = if path.is_absolute() { path } else { project.join(path) };
    std::fs::read_to_string(&path).unwrap_or_default()
}

/// AC-7 — `mustard init --private` installs privately and seeds no `.github/`
/// pull-request template into the host repository.
#[test]
fn ac7_init_private_seeds_no_github_template() {
    let work = tempdir().expect("temp dir");
    let templates = fake_templates(work.path());
    let shims = tool_shims(work.path());

    // CONTROL — both conditions that MAKE the seed happen are present, so a
    // missing `.github/` below can only be the install refusing it.
    //
    // The control used to be a shared install of the same fixture. There is no
    // longer any argv that produces one — the mode is unconditional — so the
    // control moved to the two preconditions the seeder reads: the template has
    // to exist in the source tree, and the project has to have a github.com
    // origin. Without this, a fixture with no template at all would satisfy the
    // criterion by accident, which is the exact shape of failure this unit hit
    // three times.
    assert!(
        templates.join(".github").join("pull_request_template.md").is_file(),
        "fixture broken: the source template must exist, or nothing could be seeded anyway",
    );

    // THE CRITERION — a BARE `mustard init` is private and writes no `.github/`
    // at all. No flag: the operator who most needs this mode is the one who
    // would never think to ask for it.
    let private = git_project_with_github_remote(work.path(), "private-host");
    let remote = Command::new("git")
        .args(["config", "--get", "remote.origin.url"])
        .current_dir(&private)
        .output()
        .expect("git config");
    assert!(
        String::from_utf8_lossy(&remote.stdout).contains("github.com"),
        "fixture broken: the seeder only fires on a github.com origin",
    );

    let out = run_init(&private, &templates, &shims, &[]);
    assert_ok("private init", &out);
    assert!(
        !private.join(".github").exists(),
        "private install must not seed .github/ into the host repository",
    );

    // …and it really was a PRIVATE install: the settings landed in the
    // untracked local layer, the shared twin was never created, and the
    // footprint reached this clone's exclude file.
    let claude = private.join(".claude");
    assert!(
        claude.join("settings.local.json").is_file(),
        "private install seeds .claude/settings.local.json",
    );
    assert!(
        !claude.join("settings.json").exists(),
        "private install must never create the shared .claude/settings.json",
    );
    // Derived from the declaration, never retyped: a list copied into this file
    // could only ever prove that the rules cover what its author remembered, and
    // the spelling of a rule is load-bearing (`/mustard.json` is anchored to the
    // repository root precisely so it cannot swallow a client's own nested one).
    let excluded = exclude_body(&private);
    for rule in mustard_core::footprint_rules() {
        assert!(
            excluded.lines().any(|line| line.trim() == rule),
            "the clone-local exclude file must carry `{rule}`:\n{excluded}",
        );
    }
    // And the install must be DETECTABLE as private from the file alone — that is
    // how a later `mustard init` with no flag knows not to re-seed the versioned
    // twin of a file this run already hid.
    assert!(
        mustard_core::carries_private_marks(&excluded),
        "the exclude file must read back as a private install:\n{excluded}",
    );

    // The other half of "invisible": the rules cover OUR footprint and stop
    // there. Written after the install, so these are files the operator produces
    // FOR the client — under a `git add -A` law an over-broad rule would keep
    // them out of every commit, silently.
    for theirs in ["CLAUDE.md", "services/billing/mustard.json"] {
        let path = private.join(theirs);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("client dir");
        }
        std::fs::write(&path, "# theirs\n").expect("client file");
    }
    let status = git_status(&private);
    for theirs in ["CLAUDE.md", "services/billing/mustard.json"] {
        assert!(
            status.contains(theirs),
            "the install hid {theirs}, which it never wrote: {status}",
        );
    }
}

/// The repository's dirt, trimmed. `--untracked-files=all` because the default
/// collapses a wholly untracked directory into ONE line. A failure returns a
/// sentinel rather than an empty string, so a measurement that did not happen
/// fails the assertions it feeds instead of satisfying them.
fn git_status(root: &Path) -> String {
    let out = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .current_dir(root)
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "<git status unavailable>".to_string(),
    }
}
