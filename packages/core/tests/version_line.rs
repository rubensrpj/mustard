//! The workspace version and the plugin manifest are ONE line — proven here.
//!
//! Mustard publishes its version from `plugin/.claude-plugin/plugin.json`: the
//! `bump-on-main` workflow advances it on every merge to main, tags `vX.Y.Z`
//! from it, and the release gate refuses a tag that disagrees with it. But the
//! Rust side reads a different number. A binary built without
//! `MUSTARD_RELEASE_VERSION` — which is every local build, every `install.ps1`
//! run and every CI build — resolves
//! [`mustard_core::harness_version`] to `CARGO_PKG_VERSION`, i.e. the
//! `[workspace.package] version` in the root `Cargo.toml`.
//!
//! Nothing connected the two, so the workspace version sat at `0.1.0` while the
//! plugin reached `0.1.28`. The consequence was not cosmetic: that number is
//! what `mustard-rt --version` prints, what the statusline renders as
//! `m{version}`, and what every install stamps into a project's
//! `mustard.json#version`. A stale value there does not read as "this is a dev
//! build" — it reads as a real, old release, and the harness's own drift
//! warning starts firing against itself.
//!
//! ## Which `Cargo.lock` is checked here, and which is not
//!
//! A third kind of file carries this version: `Cargo.lock` pins one per local
//! package. There are TWO of them in this repository, and they need opposite
//! treatment.
//!
//! **The root lock is NOT checked.** A test for it was written, and then
//! measured: it CANNOT fail. Plain `cargo test` repairs a stale root lock
//! before running anything, so the assertion never sees the divergence;
//! `cargo test --locked` fails in cargo itself, before any test binary starts.
//! Either way the assertion is decoration, and decoration that looks like a
//! guard is worse than no guard. The real guard there is `--locked`, which CI
//! and the release already pass, plus the `cargo update --workspace` that
//! `bump-on-main` runs — leaving that lock behind is what broke the v0.1.29
//! release on all three operating systems ("cannot update the lock file because
//! `--locked` was passed", before a line compiled).
//!
//! **The dashboard lock IS checked**, and the paragraph above is exactly why it
//! has to be. `apps/dashboard/src-tauri` is deliberately its own workspace root,
//! so no root build ever resolves it: nothing repairs it before this test reads
//! it as a plain file, which is what makes the assertion able to fail. Nothing
//! else looks either — CI excludes the dashboard on purpose (it needs per-OS
//! system libraries), and the release builds it through `tauri build` WITHOUT
//! `--locked`, so a stale lock is silently repaired at build time and the repair
//! is thrown away instead of committed. Measured on 2026-08-22: that lock still
//! named `mustard-cli` and `mustard-core` at `0.1.41` while the workspace was at
//! `0.1.44` — three releases behind, and nothing in the repository had noticed.
//! `bump-on-main` now advances that lock too; this test is what says so out loud
//! if it ever stops.
//!
//! ## The guards `bump-on-main` runs are exercised here too
//!
//! Every leg of the stamp carries a guard whose only job is to REJECT a file
//! that did not move. Those guards live in `.github/scripts/check-lock-pins.sh`
//! — one script, run by both legs of the workflow over both locks — and the
//! tests at the bottom of this file run THAT script against forged locks. The
//! thing measured is therefore the guard the release actually uses, not a second
//! implementation of the same idea kept in step by hand.
//!
//! Each of those tests reproduces a case the previous guards passed: a lock that
//! never moved while a third-party dependency happened to sit on our number, a
//! lock whose second local crate stayed behind while the first advanced, and a
//! lock that lost one of our crates outright. The last test reads the workflow
//! instead of a lock, because a guard that is never reached cannot reject
//! anything: the dev leg's decision to skip has to consult every leg its work
//! block repairs.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The plugin manifest, relative to the workspace root.
const MANIFEST_REL: &str = "plugin/.claude-plugin/plugin.json";

#[test]
fn the_workspace_version_equals_the_published_plugin_version() {
    let Some(manifest) = find_manifest() else {
        // Compiled outside the repository (a vendored or packaged source tree):
        // there is no manifest to agree with, and inventing a failure there
        // would break a legitimate build. Absence is missing evidence.
        return;
    };
    let raw = std::fs::read_to_string(&manifest).expect("plugin manifest is readable");
    let published = json_string_field(&raw, "version")
        .unwrap_or_else(|| panic!("no `version` field in {}", manifest.display()));

    assert_eq!(
        env!("CARGO_PKG_VERSION"),
        published,
        "the workspace version and {MANIFEST_REL} disagree.\n\
         `bump-on-main` advances the manifest on every merge to main; the root \
         Cargo.toml must move with it, because a locally built binary falls back \
         to the workspace version for `--version`, the statusline mark and the \
         `mustard.json` stamp. Set `[workspace.package] version` to {published}."
    );
}

/// …and the running harness agrees with both. This is the value every consumer
/// actually reads, so proving the file matches is not enough on its own —
/// `harness_version` could grow a third source tomorrow.
#[test]
fn the_running_harness_reports_that_same_line() {
    let running = mustard_core::harness_version();
    assert!(!running.is_empty(), "the harness version is never empty");

    // A release build stamps `MUSTARD_RELEASE_VERSION` from the tag, and the
    // release gate already proved the tag equals the manifest — so that path is
    // covered elsewhere and would only be re-asserted here by duplicating the
    // gate. The path this file exists for is the UNSTAMPED one.
    if option_env!("MUSTARD_RELEASE_VERSION").is_none() {
        assert_eq!(
            running,
            env!("CARGO_PKG_VERSION"),
            "an unstamped build must report the workspace version"
        );
    }
}

/// Walk up from this crate to the workspace root — the directory holding the
/// plugin manifest. `None` when there is none.
fn workspace_root() -> Option<PathBuf> {
    let mut dir: &Path = Path::new(env!("CARGO_MANIFEST_DIR"));
    loop {
        if dir.join(MANIFEST_REL).is_file() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

/// The plugin manifest itself. `None` when there is none — see the caller for
/// why that is not a failure.
fn find_manifest() -> Option<PathBuf> {
    workspace_root().map(|root| root.join(MANIFEST_REL))
}

/// Read one top-level `"key": "value"` string out of a JSON document without
/// pulling a parser into this crate's dev-dependencies for one field.
fn json_string_field(raw: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let rest = raw.split_once(&needle)?.1;
    let rest = rest.trim_start().strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let (value, _) = rest.split_once('"')?;
    Some(value.to_string())
}

#[cfg(test)]
mod helper_tests {
    use super::json_string_field;

    #[test]
    fn reads_the_field_and_ignores_the_rest() {
        let doc = "{\n  \"name\": \"mustard\",\n  \"version\": \"1.2.3\",\n  \"x\": 1\n}";
        assert_eq!(json_string_field(doc, "version").as_deref(), Some("1.2.3"));
        assert_eq!(json_string_field(doc, "name").as_deref(), Some("mustard"));
        assert_eq!(json_string_field(doc, "absent"), None);
    }
}

/// The dashboard's own workspace lock, relative to the workspace root.
const DASHBOARD_LOCK_REL: &str = "apps/dashboard/src-tauri/Cargo.lock";

/// The dashboard's own manifest, relative to the workspace root.
const DASHBOARD_MANIFEST_REL: &str = "apps/dashboard/src-tauri/Cargo.toml";

/// One `[[package]]` record of a `Cargo.lock`, reduced to the three fields this
/// file asks about.
#[derive(serde::Deserialize)]
struct LockPackage {
    name: String,
    version: String,
    /// `None` for a package that lives in this repository (reached by path);
    /// a package pulled from a registry carries a `source` line.
    source: Option<String>,
}

/// A `Cargo.lock`, reduced to its package list.
///
/// Read with the real `toml` parser: it is a REGULAR dependency of this crate
/// (`packages/core/Cargo.toml`, for `vocabulary`), so a test target already
/// links it and there is nothing to save by hand-rolling one. The hand-rolled
/// reader that stood here claimed the opposite in a comment, which had stopped
/// being true — and a parser written to dodge a cost that is not there is a
/// second lock-format reader to keep correct for no reason.
#[derive(serde::Deserialize)]
struct Lock {
    #[serde(default)]
    package: Vec<LockPackage>,
}

/// The `[package] name` of a manifest — the one package in the dashboard's lock
/// that is NOT ours to version.
fn package_name(manifest: &Path) -> Option<String> {
    std::fs::read_to_string(manifest).ok()?.lines().find_map(|line| {
        line.strip_prefix("name = \"")?.strip_suffix('"').map(str::to_string)
    })
}

/// Every crate of THIS repository that the dashboard consumes must be pinned at
/// THIS repository's version in the dashboard's own lock.
///
/// This is the assertion the module doc argues for: the dashboard is its own
/// workspace root, so no root build resolves its lock, and this test reads that
/// file as data — nothing repairs it first, which is what lets the assertion
/// fail. `mustard-dashboard` itself is excluded because it carries a version of
/// its own on purpose; every other local package is one of ours.
#[test]
fn the_dashboard_lock_pins_this_repositorys_crates_at_this_version() {
    let Some(root) = workspace_root() else {
        // Compiled outside the repository — see the sibling test for why absence
        // is missing evidence rather than a failure.
        return;
    };
    let lock_path = root.join(DASHBOARD_LOCK_REL);
    let Ok(raw) = std::fs::read_to_string(&lock_path) else {
        eprintln!("[skip] {} is absent from this source tree", lock_path.display());
        return;
    };

    let own = package_name(&root.join(DASHBOARD_MANIFEST_REL)).unwrap_or_default();
    let lock: Lock = toml::from_str(&raw)
        .unwrap_or_else(|e| panic!("{} is not a readable lock: {e}", lock_path.display()));
    let ours: Vec<LockPackage> = lock
        .package
        .into_iter()
        .filter(|p| p.source.is_none() && p.name != own)
        .collect();

    // Without this the filters could silently match nothing — a reworded lock
    // format would turn the guard into a test that always passes, which is the
    // decoration the module doc refuses.
    assert!(
        !ours.is_empty(),
        "{} names no local package other than `{own}` — either the dashboard \
         stopped depending on this repository (then delete this test) or the \
         lock format moved and the parser above no longer reads it",
        lock_path.display()
    );

    let expected = env!("CARGO_PKG_VERSION");
    let stale: Vec<String> = ours
        .iter()
        .filter(|p| p.version != expected)
        .map(|p| format!("{} {}", p.name, p.version))
        .collect();
    assert!(
        stale.is_empty(),
        "{} still pins {stale:?}, but this repository is at {expected}.\n\
         Nothing repairs that file on its own: the dashboard is its own workspace \
         root, CI excludes it (per-OS system libraries) and the release builds it \
         without `--locked`, so a stale pin is patched at build time and thrown \
         away. Advance it with\n  \
         cargo update --workspace --manifest-path {DASHBOARD_MANIFEST_REL}\n\
         and commit the lock. `bump-on-main` runs that same line on every release \
         — if this is red after a release, that step is what broke.",
        lock_path.display()
    );
}

// ---------------------------------------------------------------------------
// The guards `bump-on-main` runs over each leg of the stamp
// ---------------------------------------------------------------------------

/// The bump workflow, relative to the workspace root.
const BUMP_WORKFLOW_REL: &str = ".github/workflows/bump-on-main.yml";

/// The one guard both legs of the bump run over both locks.
const LOCK_GUARD_REL: &str = ".github/scripts/check-lock-pins.sh";

/// The `source` line a registry dependency carries — what tells the guard a
/// package is not ours to stamp.
const REGISTRY: &str = "registry+https://github.com/rust-lang/crates.io-index";

/// A virtual workspace manifest: no `[package]` table, so nothing is excluded
/// from the guard's derived sweep. The shape of this repository's root.
const VIRTUAL_MANIFEST: &str = "[workspace]\nresolver = \"2\"\nmembers = []\n";

/// A manifest that declares a package of its own — the shape of
/// `apps/dashboard/src-tauri`, whose package carries a version of its own on
/// purpose and is never ours to stamp.
const DASHBOARD_MANIFEST: &str = "[package]\nname = \"mustard-dashboard\"\nversion = \"0.1.0\"\n";

/// Write a `Cargo.toml`/`Cargo.lock` pair into `dir`, and answer with the lock.
/// Each package is `(name, version, foreign)`; a foreign one gets the `source`
/// line that marks it as coming from a registry.
fn forge_lock(dir: &Path, manifest: &str, packages: &[(&str, &str, bool)]) -> PathBuf {
    std::fs::write(dir.join("Cargo.toml"), manifest).expect("the forged manifest is writable");

    let mut raw = String::from(
        "# This file is automatically @generated by Cargo.\n\
         # It is not intended for manual editing.\n\
         version = 4\n",
    );
    for (name, version, foreign) in packages {
        raw.push_str(&format!("\n[[package]]\nname = \"{name}\"\nversion = \"{version}\"\n"));
        if *foreign {
            raw.push_str(&format!("source = \"{REGISTRY}\"\nchecksum = \"00\"\n"));
        }
        raw.push_str("dependencies = [\n \"nothing\",\n]\n");
    }

    let lock = dir.join("Cargo.lock");
    std::fs::write(&lock, raw).expect("the forged lock is writable");
    lock
}

/// Run the guard the workflow runs, over the `Cargo.lock` sitting in `dir`.
///
/// `None` when the script is not in this source tree, or when no `bash` can be
/// spawned to run it — a shell guard on a machine with no shell is missing
/// evidence about the checkout, the same reading the sibling tests give to an
/// absent manifest. Every runner `bump-on-main` and CI use ships one.
///
/// The working directory is the fixture's, so the lock is named relatively and
/// no test has to reason about how a shell reads a Windows path.
fn run_lock_guard(root: &Path, dir: &Path, version: &str, crates: &[&str]) -> Option<Output> {
    let script = root.join(LOCK_GUARD_REL);
    if !script.is_file() {
        eprintln!("[skip] {} is absent from this source tree", script.display());
        return None;
    }
    let mut cmd = Command::new("bash");
    cmd.current_dir(dir)
        .arg(script.to_string_lossy().replace('\\', "/"))
        .arg("Cargo.lock")
        .arg(version)
        .args(crates);
    match cmd.output() {
        Ok(out) => Some(out),
        Err(e) => {
            eprintln!("[skip] cannot spawn bash to run {LOCK_GUARD_REL}: {e}");
            None
        }
    }
}

/// The bump workflow's text, or `None` when this source tree has no workflow.
fn bump_workflow(root: &Path) -> Option<String> {
    let path = root.join(BUMP_WORKFLOW_REL);
    match std::fs::read_to_string(&path) {
        Ok(raw) => Some(raw),
        Err(_) => {
            eprintln!("[skip] {} is absent from this source tree", path.display());
            None
        }
    }
}

/// The arguments of one `check-lock-pins.sh` invocation: `(lock, crates)`.
///
/// The `bash ` prefix is part of the needle, so a line that only NAMES the
/// script — a comment pointing a reader at it — is not read as a call. A real
/// call may be wrapped in a command substitution and carry a trailing comment,
/// so reading stops at the first token that leaves the command: `&&`, `||`, or
/// a `#`.
fn guard_invocation(line: &str) -> Option<(String, Vec<String>)> {
    let tail = line.split_once(&format!("bash {LOCK_GUARD_REL} "))?.1;
    let args: Vec<String> = tail
        .split_whitespace()
        .take_while(|t| !t.starts_with('&') && !t.starts_with('|') && !t.starts_with('#'))
        .map(str::to_string)
        .collect();
    // <lock> <version> <crate>… — anything shorter is not a call this test can
    // read, and passing it over silently would hide a malformed guard.
    assert!(
        args.len() >= 3,
        "{BUMP_WORKFLOW_REL} runs the guard as `{}`, which names no crate at all",
        line.trim()
    );
    Some((args[0].clone(), args[2..].to_vec()))
}

/// Every `check-lock-pins.sh` invocation the workflow makes.
fn guard_invocations(workflow: &str) -> Vec<(String, Vec<String>)> {
    workflow.lines().filter_map(guard_invocation).collect()
}

/// A lock whose local crates never moved must be REJECTED even when some
/// third-party dependency happens to sit on the target number.
///
/// This is the measured case, forged. On v0.1.44 the guard was
/// `grep -q '^version = "$nv"$' Cargo.lock` and `tracing` was itself at 0.1.44,
/// so the line the guard looked for was in the file for a reason that had
/// nothing to do with us: the guard passed on its own, on a lock that had not
/// moved. The comment three lines above it already warned that a bare number
/// "does not tell our version from a dependency that happens to carry the same
/// one" — the warning was written for the command that works and never applied
/// to the line that checks.
#[test]
fn bump_guard_rejects_a_lock_whose_local_crates_did_not_move() {
    let Some(root) = workspace_root() else {
        return;
    };
    let dir = tempfile::tempdir().expect("a temp dir for the forged lock");
    let lock = forge_lock(
        dir.path(),
        VIRTUAL_MANIFEST,
        &[
            ("mustard-cli", "0.1.44", false),
            ("mustard-core", "0.1.44", false),
            ("tracing", "0.1.45", true),
        ],
    );

    // The trap, stated out loud: the exact line the replaced guard grepped for
    // IS in this file. Without this the fixture would prove nothing.
    let raw = std::fs::read_to_string(&lock).expect("the forged lock is readable");
    assert!(
        raw.lines().any(|line| line == "version = \"0.1.45\""),
        "the fixture must carry the bare `version = \"0.1.45\"` line a number-matching guard \
         accepted, otherwise it does not reproduce the case"
    );

    let Some(out) = run_lock_guard(&root, dir.path(), "0.1.45", &["mustard-cli", "mustard-core"])
    else {
        return;
    };
    let said = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        !out.status.success(),
        "the guard passed a lock whose crates are all still at 0.1.44, because a dependency that \
         is not ours sits on 0.1.45. It said: {said}"
    );
    assert!(
        said.contains("mustard-cli@0.1.44") && said.contains("mustard-core@0.1.44"),
        "the guard must name the crates that stayed behind, not just fail: {said}"
    );

    // …and the workflow itself no longer carries the number-matching form.
    let Some(workflow) = bump_workflow(&root) else {
        return;
    };
    let bare: Vec<&str> = workflow
        .lines()
        .filter(|line| line.contains("Cargo.lock") && line.contains("grep -q \"^version = "))
        .collect();
    assert!(
        bare.is_empty(),
        "{BUMP_WORKFLOW_REL} still matches a lock by bare version number: {bare:?}"
    );
}

/// A lock pins more than one crate of ours, and the guard checks ALL of them.
///
/// The dashboard's lock pins `mustard-core` AND `mustard-cli`; the guard it had
/// read the first and stopped. With the second one behind, the guard passed, the
/// commit was tagged, and the test that would have caught it went red on a
/// commit CI never runs — pushes made with `GITHUB_TOKEN` do not trigger
/// workflows.
#[test]
fn bump_guard_checks_every_local_crate_of_each_lock() {
    let Some(root) = workspace_root() else {
        return;
    };

    // The first crate moved, the second did not — and the lock's own package
    // carries 0.1.0 on purpose, so it is never a finding.
    let behind = tempfile::tempdir().expect("a temp dir for the forged lock");
    forge_lock(
        behind.path(),
        DASHBOARD_MANIFEST,
        &[
            ("mustard-cli", "0.1.44", false),
            ("mustard-core", "0.1.45", false),
            ("mustard-dashboard", "0.1.0", false),
            ("serde", "1.0.228", true),
        ],
    );
    let Some(out) =
        run_lock_guard(&root, behind.path(), "0.1.45", &["mustard-cli", "mustard-core"])
    else {
        return;
    };
    let said = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        !out.status.success(),
        "the guard passed a lock whose second local crate stayed at 0.1.44. It said: {said}"
    );
    assert!(
        said.contains("mustard-cli@0.1.44"),
        "the guard must name the crate that stayed behind: {said}"
    );
    assert!(
        !said.contains("mustard-dashboard"),
        "the lock's own package carries its own version on purpose and must never be a finding: \
         {said}"
    );

    // The same lock with both crates on the stamp is accepted — a guard that
    // rejected everything would satisfy the assertions above and be useless.
    let level = tempfile::tempdir().expect("a temp dir for the forged lock");
    forge_lock(
        level.path(),
        DASHBOARD_MANIFEST,
        &[
            ("mustard-cli", "0.1.45", false),
            ("mustard-core", "0.1.45", false),
            ("mustard-dashboard", "0.1.0", false),
            ("serde", "1.0.228", true),
        ],
    );
    let Some(out) = run_lock_guard(&root, level.path(), "0.1.45", &["mustard-cli", "mustard-core"])
    else {
        return;
    };
    assert!(
        out.status.success(),
        "the guard rejected a lock that carries the stamp on every crate of ours: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // …and the workflow asks it about BOTH locks, naming more than one crate
    // every time. A guard that measures everything is worth nothing if the leg
    // it is pointed at is the only one anybody checks.
    let Some(workflow) = bump_workflow(&root) else {
        return;
    };
    let calls = guard_invocations(&workflow);
    assert!(
        !calls.is_empty(),
        "{BUMP_WORKFLOW_REL} never runs {LOCK_GUARD_REL} — the guard is not reached at all"
    );
    for (lock, crates) in &calls {
        assert!(
            crates.len() >= 2,
            "{BUMP_WORKFLOW_REL} asks the guard about {lock} naming only {crates:?}; a single \
             hand-picked name is the defect this guard replaced"
        );
    }
    for leg in ["Cargo.lock", "apps/dashboard/src-tauri/Cargo.lock"] {
        assert!(
            calls.iter().any(|(lock, _)| lock == leg),
            "{BUMP_WORKFLOW_REL} never runs the guard over {leg}. Invocations: {calls:?}"
        );
    }
}

/// A crate of ours that VANISHED from a lock must be named, not passed over.
///
/// This is the hole a set derived from the lock alone cannot see: the crate that
/// left simply stops being asked about, so every package still there is on the
/// stamp and the guard goes green — on exactly the case it exists for, a
/// dependency removed by accident. The named argument list is what closes it.
#[test]
fn bump_guard_rejects_a_lock_that_lost_one_of_our_crates() {
    let Some(root) = workspace_root() else {
        return;
    };
    let dir = tempfile::tempdir().expect("a temp dir for the forged lock");
    forge_lock(
        dir.path(),
        DASHBOARD_MANIFEST,
        &[
            ("mustard-core", "0.1.45", false),
            ("mustard-dashboard", "0.1.0", false),
            ("serde", "1.0.228", true),
        ],
    );

    let Some(out) = run_lock_guard(&root, dir.path(), "0.1.45", &["mustard-cli", "mustard-core"])
    else {
        return;
    };
    let said = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        !out.status.success(),
        "the guard approved the reduced set that was left after `mustard-cli` disappeared from \
         the lock. It said: {said}"
    );
    assert!(
        said.contains("mustard-cli"),
        "the guard must name WHICH crate vanished — `something is missing` sends the reader back \
         to diff two lock files by hand: {said}"
    );
}

/// The dev leg's decision to skip must consult every leg its work block repairs.
///
/// The legs are DERIVED from the work block itself: whatever that block stages
/// is what it repairs. Each of those paths must be read into a variable before
/// the decision, and the decision must consult that variable. Three of the four
/// were consulted while the message on the `then` branch claimed all four, so a
/// dev whose root `Cargo.lock` alone lagged skipped the entire block and that
/// lock never moved — the defect wearing different clothes.
#[test]
fn dev_leg_decision_consults_what_the_work_block_repairs() {
    let Some(root) = workspace_root() else {
        return;
    };
    let Some(workflow) = bump_workflow(&root) else {
        return;
    };
    let lines: Vec<&str> = workflow.lines().collect();

    let leg_at = lines
        .iter()
        .position(|line| line.contains("git checkout -B dev-sync"))
        .unwrap_or_else(|| panic!("{BUMP_WORKFLOW_REL} no longer checks out the dev leg"));
    let decision_at = (leg_at..lines.len())
        .find(|&i| {
            let t = lines[i].trim();
            t.starts_with("if [") && t.ends_with("; then")
        })
        .unwrap_or_else(|| panic!("{BUMP_WORKFLOW_REL}: the dev leg makes no `if [ … ]; then`"));
    let decision = lines[decision_at];

    // What the work block repairs is what it stages.
    let staged_at = (decision_at..lines.len())
        .find(|&i| lines[i].trim().starts_with("git add "))
        .unwrap_or_else(|| panic!("{BUMP_WORKFLOW_REL}: the dev leg stages nothing"));
    let legs: Vec<&str> = lines[staged_at]
        .trim()
        .trim_start_matches("git add ")
        .split_whitespace()
        .map(|arg| arg.trim_matches('"'))
        .collect();
    assert!(
        legs.len() >= 4,
        "{BUMP_WORKFLOW_REL}: the dev leg stages {legs:?} — the stamp has four legs"
    );

    for leg in &legs {
        let readers: Vec<&str> = lines[leg_at..decision_at]
            .iter()
            .filter_map(|line| assignment(line))
            .filter(|(_, rhs)| mentions(rhs, leg))
            .map(|(name, _)| name)
            .collect();
        assert!(
            !readers.is_empty(),
            "{BUMP_WORKFLOW_REL}: nothing in the dev leg reads `{leg}` into a variable before the \
             decision, so the decision cannot be consulting it"
        );
        assert!(
            readers.iter().any(|name| mentions(decision, &format!("${name}"))),
            "{BUMP_WORKFLOW_REL}: the decision `{}` never consults {readers:?}, the variable(s) \
             read from `{leg}` — a leg the work block repairs is skipped over when the other \
             three happen to be in step",
            decision.trim()
        );
    }
}

/// A shell assignment `name=rhs` at the start of a line, or `None`.
fn assignment(line: &str) -> Option<(&str, &str)> {
    let t = line.trim();
    if t.starts_with('#') {
        return None;
    }
    let (name, rhs) = t.split_once('=')?;
    let named = !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.starts_with(|c: char| c.is_ascii_digit());
    named.then_some((name, rhs))
}

/// `true` when `haystack` names `token` as a whole word.
///
/// The boundary is what separates the legs: `Cargo.lock` occurs inside
/// `apps/dashboard/src-tauri/Cargo.lock`, and treating that as a mention would
/// let one reader stand in for two different files.
fn mentions(haystack: &str, token: &str) -> bool {
    let boundary = |c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '/' || c == '.');
    haystack.match_indices(token).any(|(at, _)| {
        let before = haystack[..at].chars().next_back().is_none_or(boundary);
        let after = haystack[at + token.len()..].chars().next().is_none_or(|c| {
            !(c.is_ascii_alphanumeric() || c == '_')
        });
        before && after
    })
}
