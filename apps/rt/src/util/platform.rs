//! Platform shell helpers.
//!
//! Before this module, `build_shell_command` was duplicated verbatim (the
//! `#[cfg(windows)]` / `#[cfg(not(windows))]` pair) in both
//! `commands::pipeline::verify_pipeline` and `commands::review::qa_run`. This
//! is the single home; both call [`build_shell_command`] directly.
//!
//! Note: other modules wrap a *specific* program through the platform shell
//! (`status::run_git` runs `cmd /C git …`) or use a different, std-quoted
//! variant (`close_gates::run_command` uses `cmd /c <cmd>` via `Command::args`,
//! which does not handle cmd.exe quoting the way [`build_shell_command`] does).
//! Those are intentionally NOT folded in here — they are different shapes, not
//! copies of this one.

use std::process::Command;
#[cfg(windows)]
use std::{path::PathBuf, sync::OnceLock};

/// The POSIX shell the Windows build runs commands through, resolved once per
/// process. `None` once resolution has failed — the `cmd.exe` fallback then
/// applies for the rest of the run, so the shell never changes mid-spec.
#[cfg(windows)]
static POSIX_SHELL: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Locate the POSIX shell that ships beside `git` on Windows.
///
/// **Deliberately NOT a PATH lookup for `bash`.** On Windows `bash.exe` in the
/// system directory is the WSL launcher: it runs in a different filesystem
/// namespace with a different toolchain, so `rg`/`cargo`/`node` are absent and
/// `apps/rt/…` does not resolve. Routing commands there is worse than `cmd.exe`,
/// not better. `git` is already a hard dependency of this harness, and its
/// Windows install ships the shell one level up from the `git.exe` on PATH
/// (`…\Git\cmd\git.exe` and `…\Git\bin\git.exe` both sit under the install
/// root, where `bin\bash.exe` lives), so `git` is the anchor.
#[cfg(windows)]
fn find_posix_shell() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        if !dir.join("git.exe").is_file() {
            continue;
        }
        let Some(root) = dir.parent() else { continue };
        let bash = root.join("bin").join("bash.exe");
        if bash.is_file() {
            return Some(bash);
        }
    }
    None
}

/// The resolved POSIX shell, or `None` when this machine has no `git` install
/// carrying one.
#[cfg(windows)]
pub(crate) fn posix_shell() -> Option<&'static PathBuf> {
    POSIX_SHELL.get_or_init(find_posix_shell).as_ref()
}

/// Build a [`Command`] that runs an arbitrary shell `command` string through
/// the platform shell.
///
/// On Windows this prefers the POSIX shell beside `git` ([`find_posix_shell`]),
/// falling back to `cmd.exe`. The fallback is a REAL degradation, not a
/// cosmetic one: `cmd.exe` does not treat `'` as a quote character, so a
/// POSIX-style `rg 'token' path` searches for a literal `'token'` — it matches
/// nothing in any tree state, exits 1 with an empty stderr, and is
/// indistinguishable from an honest no-match. That is what let
/// [`crate::commands::review::ac_negative_check`] stamp unrunnable criteria
/// `proven: red`, since exit≠0 is the whole of its red rule.
///
/// The residual under the POSIX shell is backslash paths: `apps\rt\x.rs`
/// collapses to `appsrtx.rs`, because `\` escapes there. That failure is LOUD —
/// the program names the mangled path on stderr and the excerpt carries it — so
/// it degrades to a visible error, never to a silent red.
///
/// On the `cmd.exe` fallback the command is appended verbatim with
/// `CommandExt::raw_arg` (a SAFE API — no `unsafe`) and invoked as
/// `cmd /S /C "<command>"`: with `/S` and a command line whose first and last
/// chars are quotes, `cmd` strips exactly that outer quote pair and runs the
/// remainder literally. `std`'s `Command::arg` quoting assumes
/// `CommandLineToArgvW` rules that `cmd.exe` does not follow, and would corrupt
/// a command carrying quotes, `()`, `|` or `&&`. `bash.exe` DOES follow those
/// rules, so the POSIX path uses plain `arg`.
#[cfg(windows)]
#[must_use]
pub fn build_shell_command(command: &str) -> Command {
    use std::os::windows::process::CommandExt;
    if let Some(bash) = posix_shell() {
        let mut c = Command::new(bash);
        c.arg("-c").arg(command);
        return c;
    }
    let mut c = Command::new("cmd");
    c.raw_arg(format!("/S /C \"{command}\""));
    c
}

/// See the `#[cfg(windows)]` variant for the rationale. On Unix the shell is
/// `sh -c <command>`.
#[cfg(not(windows))]
#[must_use]
pub fn build_shell_command(command: &str) -> Command {
    let mut c = Command::new("sh");
    c.arg("-c").arg(command);
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_command_without_panicking() {
        // Exercise the constructor on the host platform; we don't spawn here.
        let cmd = build_shell_command("echo hello");
        assert!(!cmd.get_program().is_empty(), "a program was chosen");
    }

    /// The regression this module exists for. A criterion written POSIX-style
    /// must have its single quotes CONSUMED as quoting, not handed to the
    /// program as literal characters. Under `cmd.exe` the output keeps the
    /// apostrophes, which is exactly how `rg 'token' path` came to search for
    /// `'token'`, match nothing in any tree state, and still be read as a red
    /// that proves discrimination.
    #[test]
    fn single_quotes_quote_instead_of_reaching_the_program() {
        let out = build_shell_command("echo 'a b'")
            .output()
            .expect("the platform shell spawns");
        let seen = String::from_utf8_lossy(&out.stdout).trim().to_string();
        assert_eq!(seen, "a b", "single quotes must not survive into the argument");
    }

    /// A shell was found, and it is the one that ships with `git` — NOT the
    /// WSL launcher in the system directory, which would run the command in
    /// another filesystem namespace where the project's own toolchain is
    /// absent. Asserting only `is_some()` would pass for the wrong shell.
    #[cfg(windows)]
    #[test]
    fn the_windows_posix_shell_is_gits_and_not_wsls() {
        let Some(shell) = posix_shell() else {
            panic!("no POSIX shell beside git — POSIX-style criteria cannot run here");
        };
        let lower = shell.to_string_lossy().to_lowercase().replace('\\', "/");
        assert!(lower.ends_with("/bin/bash.exe"), "resolved {lower}");
        assert!(!lower.contains("/windows/system32/"), "resolved the WSL shim: {lower}");
    }
}
