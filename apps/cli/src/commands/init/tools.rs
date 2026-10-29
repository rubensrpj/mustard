//! The tools `mustard init` needs on the machine: the rtk gate and the
//! best-effort ripgrep install. Both are acts on the MACHINE, so they run from
//! `cli::dispatch`, never from the library half of `init`. The code tools of
//! each detected language are never installed: the native update names the
//! command of each missing one, and this half prints it.
//!
//! rtk itself comes with the installers, in a fixed version checked against
//! `checksums.txt`; `mustard init` only refuses to go on without it. Nothing
//! here writes rtk's own configuration: its hook lives in the project's
//! `.claude/settings.local.json`, written by the seed.
//!
//! [`complete_installation`] finishes through the native update door, sharing
//! its index, tool, plugin and version checks. Library seeding stays private
//! to the project; only the CLI performs machine preparation.

use std::path::Path;
use std::process::Command;

use mustard_core::platform::process;

/// Whether `rtk --version` succeeds (RTK reachable on PATH).
fn rtk_on_path() -> bool {
    Command::new("rtk")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Probe `rtk --version` and exit hard with install instructions when it
/// fails. RTK is a mandatory dependency: the harness prefixes Bash commands
/// with `rtk`, so a Mustard install without `rtk` on `PATH` would produce a
/// `.claude/` that cannot run. We abort before touching disk rather than
/// failing later in a confusing way.
///
/// This is **not** fail-open. The exit code is `1` so a script driving the
/// binary can detect the failure and surface it to the user. NOT library
/// callers: they never reach this function, which is the whole point of it
/// living in `cli::dispatch`. `pub(crate)` makes the compiler enforce that
/// rather than leaving it to this comment.
pub(crate) fn probe_rtk() {
    // Skip the hard gate under unit tests: a clean CI runner has no `rtk`, and a
    // `process::exit` here would kill the whole test process.
    //
    // That guard is narrower than it reads, which is why this function may only
    // be called from the BINARY's dispatch and never from the library: `cfg!(test)`
    // is true while this crate compiles its own unit tests and false everywhere
    // else — an integration test in another crate compiles this as an ordinary
    // dependency and gets the `exit(1)`. That is exactly how it died on CI's
    // first run of that crate.
    if cfg!(test) || rtk_on_path() {
        return;
    }
    eprintln!(
        "\nMustard requires RTK (Rust Token Killer) on PATH.\n\
         Could not run `rtk --version` — RTK is a mandatory dependency.\n\
         The Mustard installers bring it. Otherwise, download the release your system\n\
         needs from https://github.com/rtk-ai/rtk/releases, put `rtk` on PATH and\n\
         re-run `mustard init`.\n"
    );
    std::process::exit(1);
}

/// Ensure ripgrep (`rg`) is installed. Best-effort and fail-open: a missing
/// `rg` — and a *failed* install — never blocks `init`.
///
/// Why: RTK's `grep`/`find` filters use `rg` as their search engine. When `rg`
/// is missing, RTK prints a fallback warning on every invocation that pollutes
/// every Bash tool output with ~50 tokens.
///
/// Flow: if `rg` is already on PATH, return silently. Otherwise attempt
/// auto-install via Scoop (Windows) or `cargo install ripgrep`; on Unix only
/// print manual instructions (the package manager varies).
pub(crate) fn ensure_ripgrep() {
    // No external-tool side effects under unit tests (would `cargo install
    // ripgrep` on a clean CI runner). Production keeps `cfg!(test) == false`.
    if cfg!(test) {
        return;
    }
    if rg_on_path() {
        return;
    }

    println!("  ripgrep not found - attempting auto-install (silences RTK `rg` fallback warning)...");
    if install_ripgrep() && rg_on_path() {
        println!("  ripgrep installed");
        return;
    }

    println!("  ripgrep auto-install skipped or unavailable - install manually:");
    if cfg!(windows) {
        println!("    Windows: scoop install ripgrep");
        println!("         or: cargo install ripgrep");
    } else if cfg!(target_os = "macos") {
        println!("    macOS:   brew install ripgrep");
    } else {
        println!("    Linux:   apt install ripgrep | pacman -S ripgrep | dnf install ripgrep");
    }
}

/// Whether `rg --version` succeeds (ripgrep reachable on PATH).
fn rg_on_path() -> bool {
    process::command("rg")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Best-effort ripgrep auto-install. Returns `true` only when an installer
/// command exited successfully. Every spawn failure is swallowed.
///
/// - Windows: try `scoop install ripgrep` first, then `cargo install ripgrep`.
///   O `scoop` é `scoop.cmd`: roda pelo arquivo que o `PATH` tem para ele.
/// - Unix: return `false` so the caller prints manual instructions.
fn install_ripgrep() -> bool {
    let run_ok = |cmd: &mut Command| -> bool {
        cmd.output().is_ok_and(|o| o.status.success())
    };

    if cfg!(windows) {
        if run_ok(process::command("scoop").args(["install", "ripgrep"])) {
            return true;
        }
        return run_ok(process::command("cargo").args(["install", "ripgrep"]));
    }
    false
}

/// The binary install and plugin update finish through the same native door.
/// Library callers keep the pure project-seeding contract; cancellation and
/// dry-run never reach this step.
pub(crate) fn complete_installation(root: &Path) -> anyhow::Result<()> {
    let file = if cfg!(windows) { "mustard-rt.exe" } else { "mustard-rt" };
    let sibling = std::env::current_exe()?.parent().map(|parent|parent.join(file));
    let Some(program) = sibling.filter(|path|path.is_file()) else {
        return incomplete(root,"native companion missing");
    };
    let version=Command::new(&program).arg("--version").env("MUSTARD_RT_DELEGATED","1").output()?;
    if !version.status.success() || !String::from_utf8_lossy(&version.stdout).split_whitespace().any(|part|part==mustard_core::harness_version()) {
        return incomplete(root,"native companion version differs from this CLI");
    }
    let output = Command::new(program).args(["run","upsert"])
        .env("MUSTARD_RT_DELEGATED","1").current_dir(root).output()?;
    anyhow::ensure!(output.status.success(), "native installation failed; run mustard-rt run upsert");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let receipt = report.get("installation").ok_or_else(||anyhow::anyhow!("native installation returned no verification; run mustard-rt run upsert"))?;
    println!("{}",serde_json::to_string_pretty(receipt)?);
    if let Some(warnings)=report["codeToolWarnings"].as_array() {
        for warning in warnings { if let Some(text)=warning.as_str() { println!("  {text}"); } }
    }
    Ok(())
}

fn incomplete(root:&Path,reason:&str)->anyhow::Result<()> {
    let receipt=serde_json::json!({"schema_version":1,"version":mustard_core::harness_version(),
        "complete":false,"project_ready":false,"host_ready":false,"provider_verified":false,
        "checks":[{"component":"native-installer","ready":false,"reason":reason,"remedy":"reinstall the complete Mustard bundle, then run mustard-rt run upsert"}]});
    let text=serde_json::to_string_pretty(&receipt)?;
    mustard_core::io::fs::write_atomic(root.join(".claude/mustard/installation.json"),text.as_bytes())?;
    println!("{text}");
    Ok(())
}
