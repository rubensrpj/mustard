//! AC-7 of `private-install-mode-leaving-no`: `mustard init --private` installs
//! privately and seeds no `.github/` pull-request template into the host
//! repository.
//!
//! ## Why this drives the real binary
//!
//! The `.github/` copy fires on a condition that lives OUTSIDE the flag — the
//! project having a GitHub remote — so the only honest proof is a full install
//! against a repository that really has one, driven end to end.
//!
//! Running the binary as a child process also keeps the best-effort installers
//! honest: a PATH shim (see [`tool_shims`]) answers the two `--version` probes
//! so they never shell out to `scoop`/`cargo install` on a runner that has
//! neither tool.
//!
//! ## A hazard this header used to describe, now removed at the source
//!
//! This comment once gave a second reason: `init_with_templates` opened with the
//! RTK hard gate (`probe_rtk` → `process::exit(1)`), and `cfg!(test)` — which
//! neutralises it for the unit tests inside `init.rs` — is FALSE for an
//! integration test, which links the crate as an ordinary dependency. A missing
//! RTK would take the whole test binary down on a bare CI runner.
//!
//! That was correct, and the workaround here was sound. But the hazard itself
//! survived, and `apps/dashboard/server/tests/mustard_cli_test.rs` walked into
//! it the first time CI ran that crate — the process vanished mid-test. So the
//! gate moved OUT of the library and into `cli::dispatch`, where the terminal
//! user still meets it and no library caller can be killed by it.
//!
//! Moving the gate alone was not enough, and the second half is worth recording
//! because it nearly shipped. While the gate exited at the top of
//! `init_with_templates`, the best-effort installers below it (`ensure_rtk`,
//! `ensure_ripgrep`) could only run with the tools ALREADY present — their
//! install branches were unreachable from `init`. Removing the exit made them
//! live for library callers, and a shimmed-PATH run of the dashboard's test
//! caught it spawning `sh -c "curl … | sh"` twice. Both now sit beside the gate
//! in `cli::dispatch`, for the same reason: putting software on the operator's
//! machine is an environment act, and a library call must never take it.
//!
//! So an in-process `init_with_templates` no longer exits the process and no
//! longer installs anything. This test drives the binary for the reasons above,
//! not for either of those. The gate itself is pinned by
//! `apps/cli/tests/rtk_gate.rs`.
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

/// The dashboard registry inside a home directory —
/// `<home>/.claude/dashboard-projects.json`, the file a successful `init`
/// appends the installed project to.
fn registry(home: &Path) -> PathBuf {
    home.join(".claude").join("dashboard-projects.json")
}

/// Run the real `mustard init` in `project`, with the fixture templates and the
/// tool shims in place. `extra` carries the flags under test.
///
/// `home` is a PARAMETER, never the operator's own. A successful install
/// registers the project with the dashboard, and that write resolves the
/// registry from `$HOME` — so a child that inherited the real one appended a
/// permanent row to `~/.claude/dashboard-projects.json` on every `cargo test`,
/// each naming a temporary directory that was deleted seconds later. That is
/// how a developer's registry came to hold 135 dead `/tmp` paths.
fn run_init(project: &Path, templates: &Path, shims: &Path, home: &Path, extra: &[&str]) -> Output {
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
        // The home the registry is resolved from. Both spellings, because the
        // registry reads `USERPROFILE` on Windows and `HOME` everywhere else —
        // and this test is NOT unix-gated, so setting only one would leave the
        // Windows runner writing the real file.
        .env("HOME", home)
        .env("USERPROFILE", home)
        // Never touch the operator's ~/.claude from a test — the write is
        // opt-in, and this pins it off no matter what the environment says.
        // It does NOT cover the dashboard registry, which `init` writes
        // unconditionally; only the isolated `home` above does.
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
    // The child's `$HOME`: everything `init` writes outside the project lands
    // here, where the tempdir takes it away again.
    let home = work.path().join("home");
    std::fs::create_dir_all(&home).expect("isolated home");

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

    let out = run_init(&private, &templates, &shims, &home, &[]);
    assert_ok("private init", &out);
    assert!(
        !private.join(".github").exists(),
        "private install must not seed .github/ into the host repository",
    );

    // The registry redirection, asserted POSITIVELY. Checking only that the
    // operator's own file did not change would pass just as green for an init
    // that died before writing anything at all — the row has to be somewhere,
    // and the whole point is that "somewhere" is the tempdir. The registry
    // stores the CANONICAL path, because a relative or symlinked one resolves
    // differently depending on where the dashboard was started.
    let canonical = private.canonicalize().unwrap_or_else(|_| private.clone());
    let rows = mustard_core::dashboard_registry::read_at(&registry(&home));
    assert!(
        rows.iter().any(|e| Path::new(&e.path) == canonical),
        "the install must have registered {} in the TEST's registry; rows were {:?}",
        canonical.display(),
        rows.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
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

/// AC-6 — a CLI-suite `init` puts its dashboard row in the TEST's registry and
/// leaves the operator's machine registry untouched.
///
/// ## Why both halves live in ONE test
///
/// Either half alone passes for the wrong reason. "The machine registry did not
/// change" is also true of an `init` that crashed before writing anything, and
/// of a build where registration was deleted outright — the very regression the
/// dashboard cares about. "A row was written" is also true of a run that wrote
/// it into the operator's real `~/.claude/`, which is the defect being fixed.
/// Only the pair — the row is HERE, and the real file did not move — separates
/// isolation from inaction.
///
/// ## How the machine registry is measured
///
/// READ twice, around the run, and compared as raw BYTES; never written. The
/// path is resolved from THIS process's environment, which is the operator's
/// own: the parent's `$HOME` is deliberately left alone so that the file being
/// watched is the real one. Comparing bytes rather than row counts catches a
/// rewrite that happens to preserve the length, and comparing `Option`s catches
/// the case the leak actually took — a file appearing where there was none.
#[test]
fn init_writes_only_into_the_isolated_home() {
    let work = tempdir().expect("temp dir");
    let templates = fake_templates(work.path());
    let shims = tool_shims(work.path());
    let home = work.path().join("home");
    std::fs::create_dir_all(&home).expect("isolated home");

    // The operator's real registry — the file every `cargo test` used to grow a
    // row in. `None` means no home resolves at all, and then `None` on both
    // sides below is an honest match: nothing appeared where nothing was.
    let machine = mustard_core::dashboard_registry::registry_path();
    let before = machine.as_ref().and_then(|p| std::fs::read(p).ok());

    let project = git_project_with_github_remote(work.path(), "isolated-host");
    let out = run_init(&project, &templates, &shims, &home, &[]);
    assert_ok("isolated init", &out);

    // POSITIVE — the row is in the test's own registry. The registry stores the
    // CANONICAL path, because a relative or symlinked one resolves differently
    // depending on where the dashboard was started.
    let canonical = project.canonicalize().unwrap_or_else(|_| project.clone());
    let rows = mustard_core::dashboard_registry::read_at(&registry(&home));
    assert!(
        rows.iter().any(|e| Path::new(&e.path) == canonical),
        "the isolated registry must carry {}; rows were {:?}",
        canonical.display(),
        rows.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
    );

    // NEGATIVE — and the machine's own registry did not move a byte.
    let after = machine.as_ref().and_then(|p| std::fs::read(p).ok());
    assert!(
        before == after,
        "running the suite changed the operator's dashboard registry at {} — \
         the child inherited the real $HOME instead of the test's",
        machine
            .as_ref()
            .map_or_else(|| "<unresolved home>".to_string(), |p| p.display().to_string()),
    );
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
