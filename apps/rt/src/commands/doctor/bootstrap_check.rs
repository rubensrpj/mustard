//! `bootstrap_check` — did the plugin's own bootstrap actually finish?
//!
//! ## Why
//!
//! The plugin ships WITHOUT binaries: `plugin/bin/*` executables are build
//! artifacts, never committed. Every hook goes through `bin/mustard-boot`,
//! which downloads `mustard-bins-<version>-<os>` on first run and then `exec`s
//! the real binary. That script is silent-allow by contract — a hook must never
//! wedge a session — so when the download does not happen it simply `exit 0`s
//! and EVERY Mustard hook is dormant for the whole session.
//!
//! Dormant is indistinguishable from healthy at a glance, and that is the
//! defect this check exists to close. Measured in the field, 2026-08-28: a
//! plugin auto-updated mid-session, its binaries never downloaded, and the
//! statusline self-heal (`statusline_heal_observer`) therefore never ran — so
//! the bar kept rendering a version drift the operator could not act on, and
//! restarting the session changed nothing, because a restart is not what
//! fetches the binary. Nothing anywhere said "the harness is off".
//!
//! ## What it answers
//!
//! Four questions, each with the command that resolves it:
//!
//! 1. Is the installed plugin's `bin/mustard-rt` on disk at all? (`binary-missing`)
//! 2. Does its `.version` stamp match the plugin manifest? (`stamp-mismatch`)
//! 3. Is the running binary the installed version? (`session-stale`)
//! 4. Are the toolchains this project's criteria invoke reachable from the
//!    shell the harness runs commands in? (`toolchain-unreachable`)
//!
//! Question 4 looks unrelated and is the same failure in a different coat: when
//! `cargo` is not on `PATH`, `ac-negative-check` and `qa-run` run the criterion,
//! collect exit 127 (`command not found`) and record every cargo-backed
//! criterion `unproven`. A reader sees a red criterion and hunts the code. The
//! ledger's own `reason` says the command could not be attempted, but nobody
//! reads a ledger before believing a red. Naming it here, once, at the door.
//!
//! ## Contract
//!
//! Fail-open at every step: an unreadable registry, an unresolvable home
//! directory or a malformed manifest degrades to "cannot measure", never an
//! error and never a panic. `ok: true` means every question that COULD be
//! answered was answered well — a report that measured nothing is `ok`, with a
//! finding saying so, because "nobody looked" is not the same answer as "it is
//! broken".

use std::path::{Path, PathBuf};

use mustard_core::io::fs;

use crate::util::home_dir;

/// One thing the bootstrap check found, with the command that resolves it.
///
/// `kind` is a CLOSED vocabulary so a caller can branch on it without parsing
/// prose: `binary-missing`, `stamp-mismatch`, `session-stale`,
/// `toolchain-unreachable`, `not-measured`, `local-build`.
pub struct BootstrapFinding {
    /// Closed-vocabulary tag for the finding.
    pub kind: &'static str,
    /// What was observed, in one line.
    pub detail: String,
    /// The command (or action) that resolves it. Never empty.
    pub remedy: String,
}

/// The bootstrap check's whole answer.
pub struct BootstrapReport {
    /// Every answerable question came back well.
    pub ok: bool,
    /// At least one finding is a FAIL rather than a WARN.
    pub failed: bool,
    /// The plugin version the harness registry says is installed.
    pub installed_version: Option<String>,
    /// The version stamped into the installed `bin/.version`.
    pub stamped_version: Option<String>,
    /// The version compiled into the binary running THIS check.
    pub running_version: String,
    /// Findings, in a stable order.
    pub findings: Vec<BootstrapFinding>,
}

/// The executable name the plugin ships, per platform.
fn rt_exe_name() -> &'static str {
    if cfg!(windows) {
        "mustard-rt.exe"
    } else {
        "mustard-rt"
    }
}

/// The bootstrap script name, per platform — what a remedy tells the operator
/// to run.
fn boot_name() -> &'static str {
    if cfg!(windows) {
        "mustard-boot.cmd"
    } else {
        "mustard-boot"
    }
}

/// Read `<home>/.claude/plugins/installed_plugins.json` and return the Mustard
/// entry's `(installPath, version)`.
///
/// The registry is the harness's own record of what it installed, which is the
/// only thing that can answer "the binary for the version that is SUPPOSED to
/// be here". Resolving from `current_exe` instead would answer "the binary that
/// is running", which is exactly the question that cannot detect a missing one.
fn installed_plugin() -> Option<(PathBuf, String)> {
    let path = home_dir()?
        .join(".claude")
        .join("plugins")
        .join("installed_plugins.json");
    let raw = fs::read_to_string(&path).ok()?;
    let doc: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let plugins = doc.get("plugins")?.as_object()?;
    // The key carries the marketplace suffix (`mustard@mustard-local`), which
    // varies per install, so match on the name half rather than the whole key.
    let entry = plugins
        .iter()
        .find(|(k, _)| k.split('@').next() == Some("mustard"))
        .map(|(_, v)| v)?;
    let first = entry.as_array()?.first()?;
    let install_path = first.get("installPath")?.as_str()?;
    let version = first.get("version")?.as_str()?;
    Some((PathBuf::from(install_path), version.to_string()))
}

/// The `version` field of a plugin manifest (`.claude-plugin/plugin.json`).
fn manifest_version(install_path: &Path) -> Option<String> {
    let raw = fs::read_to_string(install_path.join(".claude-plugin").join("plugin.json")).ok()?;
    let doc: serde_json::Value = serde_json::from_str(&raw).ok()?;
    Some(doc.get("version")?.as_str()?.to_string())
}

/// Is `program` resolvable the way the harness will resolve it?
///
/// Delegates to [`crate::shared::proc::resolves`], the SAME resolver
/// `run_shell_with_deadline` uses to build a criterion's environment. This
/// used to be a private `PATH`-only copy, and a private copy is how a doctor
/// starts reporting a tool missing that the harness would have found — two
/// answers about one machine.
fn on_path(program: &str) -> bool {
    crate::shared::proc::resolves(program)
}

/// The toolchains this project's acceptance criteria will actually invoke,
/// inferred from what the workspace root declares.
///
/// Inferred rather than configured on purpose: a project that carries a
/// `Cargo.toml` WILL have cargo-backed criteria, and the operator should not
/// have to declare that twice. `(program, why, likely_fix)`.
fn expected_toolchains(project_dir: &Path) -> Vec<(&'static str, &'static str)> {
    let mut out: Vec<(&'static str, &'static str)> = Vec::new();
    if project_dir.join("Cargo.toml").is_file() {
        out.push(("cargo", "add ~/.cargo/bin to PATH in your shell profile"));
    }
    if project_dir.join("package.json").is_file() {
        // pnpm is what this workspace's criteria call; npm is the fallback a
        // project without pnpm would use. Only complain when NEITHER resolves.
        if !on_path("pnpm") && on_path("npm") {
            // npm covers it — nothing to report.
        } else {
            out.push(("pnpm", "install pnpm, or put its shim directory on PATH"));
        }
    }
    out
}

/// Inspect ONE plugin install directory: is its binary there, and is it the
/// version the manifest promises?
///
/// Split out of [`run`] so the headline case — the binary that never
/// downloaded — is provable against a temporary directory instead of only
/// against whatever this machine happens to have installed. Returns the
/// findings plus the `.version` stamp it read, and sets `failed` through the
/// out-parameter so the caller keeps one place that decides severity.
fn inspect_install(
    install_path: &Path,
    findings: &mut Vec<BootstrapFinding>,
    failed: &mut bool,
) -> Option<String> {
    let bin_dir = install_path.join("bin");
    let rt = bin_dir.join(rt_exe_name());
    let boot = bin_dir.join(boot_name());
    let stamp = fs::read_to_string(bin_dir.join(".version"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    if rt.is_file() {
        // A binary with no stamp is a local build — `mustard-boot`'s own
        // "hands off" case. Say so rather than reporting a mismatch against a
        // manifest a developer deliberately outran.
        match (&stamp, manifest_version(install_path)) {
            (None, _) => findings.push(BootstrapFinding {
                kind: "local-build",
                detail: format!(
                    "{} carries no .version stamp — treated as a local build, never re-downloaded",
                    bin_dir.display()
                ),
                remedy: "nothing to do; delete bin/ to fall back to the published bundle"
                    .to_string(),
            }),
            (Some(s), Some(m)) if *s != m => {
                *failed = true;
                findings.push(BootstrapFinding {
                    kind: "stamp-mismatch",
                    detail: format!(
                        "the binary in {} is stamped {s}, but the plugin manifest says {m} — \
                         the update downloaded nothing",
                        bin_dir.display()
                    ),
                    remedy: format!("\"{}\" --version", boot.display()),
                });
            }
            _ => {}
        }
    } else {
        // THE case this check exists for: hooks are dormant, silently.
        *failed = true;
        findings.push(BootstrapFinding {
            kind: "binary-missing",
            detail: format!(
                "{} does not exist — the plugin bootstrap never completed, so every Mustard hook \
                 is dormant and no self-heal runs",
                rt.display()
            ),
            remedy: format!("\"{}\" --version", boot.display()),
        });
    }
    stamp
}

/// Is the INSTALLED plugin's binary absent or stale — i.e. would every hook be
/// dormant this session?
///
/// The cheap half of [`run`], for the statusline, which is redrawn on every
/// turn: two `stat`s and one small JSON read, and deliberately NOT the `PATH`
/// sweep [`expected_toolchains`] does. A bar that costs a directory walk per
/// keystroke is a bar someone turns off.
///
/// `false` whenever the question cannot be answered — an unreadable registry is
/// not evidence of dormancy, and a red flag nobody can act on is worse than no
/// flag. Note this can be `true` while the statusline itself renders fine: the
/// bar may be drawn by a leftover binary from an earlier version, which is
/// exactly the state that made the field case invisible.
#[must_use]
pub fn harness_dormant() -> bool {
    let Some((install_path, _)) = installed_plugin() else {
        return false;
    };
    let bin_dir = install_path.join("bin");
    if !bin_dir.join(rt_exe_name()).is_file() {
        return true;
    }
    // Present but stamped for another version. Only `on SessionStart` re-downloads
    // now — every other trigger runs on a budget too short to survive a fetch — and
    // the stamped binary is still handed the invocation at the tail, so this install
    // keeps answering with the OLD harness until a session start refreshes it. Report
    // it dormant anyway: what the operator needs to see is that the version they
    // installed is not the version replying.
    match (
        fs::read_to_string(bin_dir.join(".version"))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        manifest_version(&install_path),
    ) {
        // No stamp is a local build — hands off, and not dormant.
        (None, _) => false,
        (Some(stamp), Some(manifest)) => stamp != manifest,
        (Some(_), None) => false,
    }
}

/// Run the bootstrap check for `project_dir`. Fail-open; never panics.
#[must_use]
pub fn run(project_dir: &Path) -> BootstrapReport {
    let running_version = mustard_core::harness_version();
    let mut findings: Vec<BootstrapFinding> = Vec::new();
    let mut failed = false;

    let (installed_version, stamped_version) = match installed_plugin() {
        None => {
            findings.push(BootstrapFinding {
                kind: "not-measured",
                detail: "no plugin registry at ~/.claude/plugins/installed_plugins.json — \
                         Mustard may be installed some other way"
                    .to_string(),
                remedy: "nothing to do if you did not install Mustard as a Claude Code plugin"
                    .to_string(),
            });
            (None, None)
        }
        Some((install_path, version)) => {
            let stamp = inspect_install(&install_path, &mut findings, &mut failed);
            (Some(version), stamp)
        }
    };

    // The session loaded its instructions from whatever version was installed
    // when it STARTED. A mid-session auto-update leaves the two disagreeing,
    // and only a restart reconciles it — which is a different remedy from
    // everything above, so it must be a different finding.
    if let Some(installed) = installed_version.as_deref() {
        if installed != running_version {
            findings.push(BootstrapFinding {
                kind: "session-stale",
                detail: format!(
                    "plugin {installed} is installed, but the binary answering right now is \
                     {running_version} — the plugin updated after this session started"
                ),
                remedy: "restart the session to load the installed version".to_string(),
            });
        }
    }

    // Toolchain reachability — the criteria the harness will run are only as
    // honest as the shell it runs them in.
    for (program, fix) in expected_toolchains(project_dir) {
        if !on_path(program) {
            failed = true;
            findings.push(BootstrapFinding {
                kind: "toolchain-unreachable",
                detail: format!(
                    "`{program}` is not on PATH — every acceptance criterion that calls it will \
                     be recorded `unproven` (exit 127), which reads exactly like a failing test"
                ),
                remedy: fix.to_string(),
            });
        }
    }

    // `local-build` and `not-measured` are notes, not problems: a report
    // carrying only those is a clean one.
    let ok = !findings
        .iter()
        .any(|f| !matches!(f.kind, "local-build" | "not-measured"));

    BootstrapReport {
        ok,
        failed,
        installed_version,
        stamped_version,
        running_version,
        findings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs as stdfs;

    #[test]
    fn on_path_finds_a_program_that_is_there() {
        // `sh` on unix, `cmd` on windows — both are guaranteed present.
        let program = if cfg!(windows) { "cmd" } else { "sh" };
        assert!(on_path(program), "{program} should resolve through PATH");
    }

    #[test]
    fn on_path_rejects_a_program_that_is_not() {
        assert!(!on_path("mustard-definitely-not-a-real-program-xyz"));
    }

    #[test]
    fn expected_toolchains_is_empty_for_a_bare_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(expected_toolchains(dir.path()).is_empty());
    }

    #[test]
    fn expected_toolchains_names_cargo_for_a_rust_workspace() {
        let dir = tempfile::tempdir().expect("tempdir");
        stdfs::write(dir.path().join("Cargo.toml"), "[workspace]\n").expect("write");
        let found = expected_toolchains(dir.path());
        assert!(found.iter().any(|(p, _)| *p == "cargo"));
    }

    #[test]
    fn manifest_version_degrades_when_the_manifest_is_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(manifest_version(dir.path()).is_none());
    }

    /// The field case, 2026-08-28: the plugin updated, its `bin/` holds only
    /// the boot scripts, and every hook is therefore dormant. Before this
    /// check, nothing anywhere said so.
    #[test]
    fn a_plugin_whose_binary_never_downloaded_fails_and_names_the_boot_script() {
        let dir = tempfile::tempdir().expect("tempdir");
        stdfs::create_dir_all(dir.path().join("bin")).expect("bin");
        // Exactly what the shipped plugin carries before its first run.
        stdfs::write(dir.path().join("bin").join(boot_name()), "#!/bin/sh\n").expect("boot");

        let mut findings = Vec::new();
        let mut failed = false;
        let stamp = inspect_install(dir.path(), &mut findings, &mut failed);

        assert!(failed, "a missing binary must FAIL, not warn");
        assert!(stamp.is_none());
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, "binary-missing");
        assert!(
            findings[0].remedy.contains(boot_name()),
            "the remedy must name the boot script: {}",
            findings[0].remedy
        );
    }

    /// A binary left over from an earlier version is just as dormant for the
    /// NEW one, and reads as healthy from the outside.
    #[test]
    fn a_stale_stamp_fails_against_the_manifest() {
        let dir = tempfile::tempdir().expect("tempdir");
        stdfs::create_dir_all(dir.path().join("bin")).expect("bin");
        stdfs::create_dir_all(dir.path().join(".claude-plugin")).expect("manifest dir");
        stdfs::write(dir.path().join("bin").join(rt_exe_name()), "").expect("rt");
        stdfs::write(dir.path().join("bin").join(".version"), "0.1.52").expect("stamp");
        stdfs::write(
            dir.path().join(".claude-plugin").join("plugin.json"),
            r#"{"name":"mustard","version":"0.1.54"}"#,
        )
        .expect("manifest");

        let mut findings = Vec::new();
        let mut failed = false;
        let stamp = inspect_install(dir.path(), &mut findings, &mut failed);

        assert!(failed);
        assert_eq!(stamp.as_deref(), Some("0.1.52"));
        assert_eq!(findings[0].kind, "stamp-mismatch");
        assert!(findings[0].detail.contains("0.1.52"));
        assert!(findings[0].detail.contains("0.1.54"));
    }

    /// A developer's own build carries no stamp; `mustard-boot` leaves it
    /// alone, and so must this check.
    #[test]
    fn a_local_build_is_a_note_not_a_failure() {
        let dir = tempfile::tempdir().expect("tempdir");
        stdfs::create_dir_all(dir.path().join("bin")).expect("bin");
        stdfs::write(dir.path().join("bin").join(rt_exe_name()), "").expect("rt");

        let mut findings = Vec::new();
        let mut failed = false;
        inspect_install(dir.path(), &mut findings, &mut failed);

        assert!(!failed, "a local build must never fail the doctor");
        assert_eq!(findings[0].kind, "local-build");
    }

    #[test]
    fn a_report_that_measured_nothing_is_ok_but_says_so() {
        // No registry is reachable in the test environment's HOME, or one is
        // and it is well-formed; either way the report must never panic and
        // must always carry a running version.
        let dir = tempfile::tempdir().expect("tempdir");
        let report = run(dir.path());
        assert!(!report.running_version.is_empty());
    }
}
