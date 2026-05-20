//! `rtk_command` — Golden Rule helper for spawning subprocesses through RTK.
//!
//! The operator's global Golden Rule says **"ALWAYS prefix Bash commands with
//! `rtk`"**. The `bash_guard` hook enforces that at the Bash-tool boundary, but
//! the `mustard-rt` binary itself also shells out (e.g. `git` in
//! `run/diff_context.rs`) and those subprocess calls must follow the same
//! rule. This helper builds a [`std::process::Command`] whose head program is
//! always `rtk`, with the original program name passed as the first argument
//! so RTK can apply its filter (or pass through unchanged).
//!
//! It has exactly one responsibility: prepend `rtk` to `program`. It does not
//! probe for `rtk` on `PATH`, does not fall back to a raw call, does not add
//! filters of its own. Probing belongs in `mustard init` (which fails hard if
//! `rtk --version` does not succeed); fallback belongs nowhere — RTK is a
//! mandatory dependency of Mustard.

use std::process::Command;

/// Build a [`Command`] that invokes `program` through `rtk`.
///
/// * `rtk_command("git", &["status"])` produces `Command::new("rtk")` with
///   args `["git", "status"]` — RTK execs `git status` after applying any
///   matching filter.
/// * `rtk_command("rtk", &["gain"])` already has `rtk` as the program; the
///   helper returns `Command::new("rtk")` with args `["gain"]` without
///   double-prefixing.
///
/// The caller may further configure the returned `Command` (e.g.
/// `.current_dir(...)`, `.stdin(Stdio::null())`, `.output()`) — this helper
/// only sets the program and the argument list.
#[must_use]
pub fn rtk_command(program: &str, args: &[&str]) -> Command {
    let mut command = Command::new("rtk");
    if program == "rtk" {
        // Already RTK — do not double-prefix; just forward args.
        command.args(args);
    } else {
        command.arg(program);
        command.args(args);
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `program == "rtk"` must not produce `rtk rtk …` — the helper forwards
    /// the args directly so the resulting argv is `["rtk", "gain"]`.
    #[test]
    fn does_not_double_prefix_when_program_is_rtk() {
        let cmd = rtk_command("rtk", &["gain"]);
        assert_eq!(cmd.get_program(), "rtk");
        let args: Vec<&std::ffi::OsStr> = cmd.get_args().collect();
        assert_eq!(args, vec![std::ffi::OsStr::new("gain")]);
    }

    /// `program == "git"` must prefix: argv becomes `["git", "status"]` under
    /// the `rtk` program.
    #[test]
    fn prefixes_non_rtk_program() {
        let cmd = rtk_command("git", &["status"]);
        assert_eq!(cmd.get_program(), "rtk");
        let args: Vec<&std::ffi::OsStr> = cmd.get_args().collect();
        assert_eq!(
            args,
            vec![std::ffi::OsStr::new("git"), std::ffi::OsStr::new("status")]
        );
    }

    /// Argument order must be preserved verbatim — RTK delegates to the real
    /// binary and any reordering would change semantics.
    #[test]
    fn preserves_arg_order() {
        let cmd = rtk_command("git", &["log", "--oneline", "-n", "5"]);
        let args: Vec<&std::ffi::OsStr> = cmd.get_args().collect();
        assert_eq!(
            args,
            vec![
                std::ffi::OsStr::new("git"),
                std::ffi::OsStr::new("log"),
                std::ffi::OsStr::new("--oneline"),
                std::ffi::OsStr::new("-n"),
                std::ffi::OsStr::new("5"),
            ]
        );
    }
}
