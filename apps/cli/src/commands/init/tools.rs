//! The tools `mustard init` needs on the machine: the rtk gate, the
//! best-effort ripgrep install, and the best-effort code-tool install (one
//! language-server program + Claude Code plugin per detected language). All
//! three are acts on the MACHINE, so they run from `cli::dispatch`, never
//! from the library half of `init`.
//!
//! rtk itself comes with the installers, in a fixed version checked against
//! `checksums.txt`; `mustard init` only refuses to go on without it. Nothing
//! here writes rtk's own configuration: its hook lives in the project's
//! `.claude/settings.local.json`, written by the seed.
//!
//! [`ensure_code_tools`] carries no language name of its own — it only walks
//! what `mustard_core::platform::code_tools` returns, keeping this crate free
//! of a hardcoded language/framework identifier. It also never
//! `cfg!(test)`-skips: the table it drives is exactly what the proof needs to
//! exercise, so it takes the `PATH` it searches and spawns against as a
//! parameter instead, and a test points that parameter at fake programs in a
//! temporary directory rather than touching the real machine.

use std::path::{Path, PathBuf};
use std::process::Command;

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
    Command::new("rg")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Best-effort ripgrep auto-install. Returns `true` only when an installer
/// command exited successfully. Every spawn failure is swallowed.
///
/// - Windows: try `scoop install ripgrep` first, then `cargo install ripgrep`.
/// - Unix: return `false` so the caller prints manual instructions.
fn install_ripgrep() -> bool {
    let run_ok = |cmd: &mut Command| -> bool {
        cmd.output().is_ok_and(|o| o.status.success())
    };

    if cfg!(windows) {
        if run_ok(Command::new("scoop").args(["install", "ripgrep"])) {
            return true;
        }
        return run_ok(Command::new("cargo").args(["install", "ripgrep"]));
    }
    false
}

/// Ensure every language `project_root` involves has its code tool set up —
/// the program a language-server plugin drives, and the plugin itself from
/// Claude Code's official catalog — the same best-effort shape
/// [`ensure_ripgrep`] already applies to `rg`.
///
/// Per language `mustard_core::platform::code_tools::detect_code_languages`
/// returns: when the program is not on `path_env` and the package manager its
/// install command starts with IS, the install command runs; either way, the
/// catalog plugin is then installed and enabled (already-installed is not an
/// error — `claude plugin install`/`enable` are idempotent). A language the
/// catalog does not cover (Dart, today) gets its own "no plugin" notice
/// instead. Every failure degrades to a printed warning with the command to
/// run by hand; `init` always finishes.
pub(crate) fn ensure_code_tools(project_root: &Path, model_path: &Path, path_env: &str) {
    use mustard_core::platform::code_tools::{code_tool_for_language, detect_code_languages};

    let languages = detect_code_languages(project_root, model_path);
    for language in &languages {
        let Some(tool) = code_tool_for_language(language) else {
            println!("  {language}: no code-tool plugin in the catalog yet - install a language server manually if you want one");
            continue;
        };

        if !on_path(tool.program, path_env) {
            let pkg_manager = tool.install_cmd.split_whitespace().next().unwrap_or_default();
            if on_path(pkg_manager, path_env) {
                run_shell_words(tool.install_cmd, path_env);
            }
        }

        if !on_path(tool.program, path_env) {
            if let Some(found_at) = find_in_conventional_dirs(tool.program) {
                println!(
                    "  {language}: {} found at {} but not on PATH - add its folder to PATH",
                    tool.program,
                    found_at.display()
                );
            } else {
                println!(
                    "  {language}: {} not found on PATH - install manually: {}",
                    tool.program, tool.install_cmd
                );
            }
        }

        if let Some(plugin) = tool.plugin {
            let full = format!("{plugin}@claude-plugins-official");
            if !run_ok(Command::new("claude").args(["plugin", "install", &full]).env("PATH", path_env)) {
                println!("  {language}: could not install the {full} plugin - run manually: claude plugin install {full}");
            }
            if !run_ok(Command::new("claude").args(["plugin", "enable", &full]).env("PATH", path_env)) {
                println!("  {language}: could not enable the {full} plugin - run manually: claude plugin enable {full}");
            }
        }
    }
}

/// Whether `cmd.output()` succeeded. Shared by every spawn in
/// [`ensure_code_tools`] so a swallowed spawn failure is written once.
fn run_ok(cmd: &mut Command) -> bool {
    cmd.output().is_ok_and(|o| o.status.success())
}

/// Whether `program` resolves inside `path_env` (an OS-formatted `PATH`
/// string, not necessarily the process's own environment — the parameter a
/// test points at a temporary directory of fake programs).
fn on_path(program: &str, path_env: &str) -> bool {
    if program.is_empty() {
        return false;
    }
    let sep = if cfg!(windows) { ';' } else { ':' };
    let names: Vec<String> = if cfg!(windows) {
        ["exe", "cmd", "bat"].iter().map(|ext| format!("{program}.{ext}")).collect()
    } else {
        vec![program.to_string()]
    };
    path_env
        .split(sep)
        .any(|dir| names.iter().any(|n| Path::new(dir).join(n).is_file()))
}

/// Split `cmd` on whitespace and spawn it against `path_env`. Naive
/// whitespace splitting is enough: every install command in the code-tool
/// table is a plain argument list, no quoting.
fn run_shell_words(cmd: &str, path_env: &str) -> bool {
    let mut parts = cmd.split_whitespace();
    let Some(program) = parts.next() else {
        return false;
    };
    run_ok(Command::new(program).args(parts).env("PATH", path_env))
}

/// Per-user toolchain directories a program can land in without being on
/// `PATH` — e.g. `rustup component add` puts `rust-analyzer` in
/// `~/.cargo/bin`. Reads the REAL `HOME`, never `path_env`: this only runs
/// against the actual machine, to word the warning precisely, and a test that
/// never reaches this branch (its fake program IS on `path_env`) does not pay
/// for it.
fn find_in_conventional_dirs(program: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    [".cargo/bin", ".local/bin", ".dotnet/tools", "go/bin"]
        .iter()
        .map(|d| home.join(d).join(program))
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write a fake executable named `name` under `dir` that appends its own
    /// argv to `log` — the "programs used in fake temp dir, log arguments
    /// instead of installing anything real" shape the proof needs. On Windows
    /// it is a `.cmd` batch file, the extension `on_path` looks for there; a
    /// shell script under a bare name is found on Unix only.
    fn write_fake_program(dir: &Path, name: &str, log: &Path) {
        let path = if cfg!(windows) { dir.join(format!("{name}.cmd")) } else { dir.join(name) };
        let script = if cfg!(windows) {
            format!("@echo off\r\necho %0 %* >> \"{}\"\r\nexit /b 0\r\n", log.display())
        } else {
            format!("#!/bin/sh\necho \"$0 $*\" >> \"{}\"\nexit 0\n", log.display())
        };
        std::fs::write(&path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    /// The proof: a Rust project, with every real toolchain replaced by a
    /// fake `rustup` and a fake `claude` at the front of `PATH`. Neither
    /// `rust-analyzer` nor the fake `rustup`/`claude` install anything real —
    /// the log file proves the install command and both plugin commands ran,
    /// with the exact plugin id the table declares for rust.
    ///
    /// Unix only: the install spawns each program by its bare name, and
    /// Windows starts a `.cmd` through the shell, not through the direct
    /// spawn this fake program relies on.
    #[test]
    #[cfg(unix)]
    fn the_install_sets_up_the_code_tool_of_each_language() {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        let model_path = project.path().join(".claude").join("grain.model.json");

        let bin = tempfile::tempdir().unwrap();
        let log = bin.path().join("log.txt");
        write_fake_program(bin.path(), "rustup", &log);
        write_fake_program(bin.path(), "claude", &log);
        // rust-analyzer stays absent: the "not on PATH" branch runs too.

        let path_env = bin.path().display().to_string();
        ensure_code_tools(project.path(), &model_path, &path_env);

        let logged = std::fs::read_to_string(&log).unwrap();
        assert!(logged.contains("rustup component add rust-analyzer"), "{logged}");
        assert!(logged.contains("claude") && logged.contains("plugin install rust-analyzer-lsp@claude-plugins-official"), "{logged}");
        assert!(logged.contains("plugin enable rust-analyzer-lsp@claude-plugins-official"), "{logged}");
    }

    /// A language the catalog does not cover — Dart is the named example in
    /// the spec, reachable only through the scanned model's stack registry
    /// (`flutter` → `dart`); Java reaches the very same "no catalog entry"
    /// branch through the manifest probe alone, so it proves the same rule
    /// without spawning the external `scan` binary a fabricated model would
    /// need. Neither language ever reaches a program/plugin command; each
    /// only gets its own notice, so the fake log stays untouched.
    #[test]
    fn a_language_without_a_catalog_entry_gets_a_no_plugin_notice_instead_of_an_install_attempt() {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("pom.xml"), "<project></project>\n").unwrap();
        let model_path = project.path().join(".claude").join("grain.model.json");

        let bin = tempfile::tempdir().unwrap();
        let log = bin.path().join("log.txt");
        write_fake_program(bin.path(), "claude", &log);
        let path_env = bin.path().display().to_string();

        ensure_code_tools(project.path(), &model_path, &path_env);

        assert!(!log.is_file(), "no program should have been spawned for a language with no catalog entry");
    }

    #[test]
    fn on_path_finds_a_program_inside_the_given_path_string() {
        let dir = tempfile::tempdir().unwrap();
        write_fake_program(dir.path(), "toolx", &dir.path().join("unused.log"));
        let path_env = dir.path().display().to_string();
        assert!(on_path("toolx", &path_env));
        assert!(!on_path("tooly", &path_env));
    }
}
