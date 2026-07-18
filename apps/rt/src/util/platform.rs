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

/// Build a [`Command`] that runs an arbitrary shell `command` string through
/// the platform shell.
///
/// On Windows, `cmd.exe` does **not** parse its command line via the
/// `CommandLineToArgvW` rules that `std`'s `Command::arg` quoting assumes, so a
/// complex `command` (quotes, `()`, `|`, `&&`) passed through `arg` would be
/// corrupted. Instead the command is appended verbatim with
/// `CommandExt::raw_arg` (a SAFE API — no `unsafe`) and invoked as
/// `cmd /S /C "<command>"`: with `/S` and a command line whose first and last
/// chars are quotes, `cmd` strips exactly that outer quote pair and runs the
/// remainder literally.
#[cfg(windows)]
#[must_use]
pub fn build_shell_command(command: &str) -> Command {
    use std::os::windows::process::CommandExt;
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
        let program = cmd.get_program().to_string_lossy().into_owned();
        if cfg!(windows) {
            assert_eq!(program, "cmd");
        } else {
            assert_eq!(program, "sh");
        }
    }
}
