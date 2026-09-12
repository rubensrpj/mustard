//! `windows_redirect` — deny `> C:\...` / `2> D:/...` style redirects.
//!
//! Mustard runs on Windows, Linux and macOS, but the Bash tool always invokes
//! a POSIX shell — git-bash on Windows, native bash/zsh elsewhere. A redirect
//! target that starts with a Windows drive letter (`C:\`, `D:/`, …) is
//! either mangled (Windows: the `:` confuses redirect parsing, the `\` is
//! consumed as an escape — producing junk filenames like `CAtizscan-out.json`
//! in the CWD) or interpreted literally (Linux/macOS: a file named `C:\Atiz\…`
//! which is also never what the caller wanted). Either way the author meant
//! an absolute path and the redirect will not produce one. This gate makes
//! that failure mode loud instead of silent on every platform.
//!
//! It reads the commands [`super::lex::segments`] found: the output redirects
//! of each one, and the file a `tee` writes. A path is judged by its raw
//! spelling, because the shell eats the backslashes of an unquoted word.

use mustard_core::domain::model::contract::Verdict;

use super::lex::{truncate, Segment};

/// The first output redirect (`>`, `>>`, `2>`, `&>`, `>|`) or `tee` file whose
/// target looks like a Windows path (`X:\…` or `X:/…`).
fn windows_path_target(segments: &[Segment]) -> Option<String> {
    for seg in segments {
        for redirect in &seg.redirects {
            let writes = redirect.op.contains('>') && !redirect.op.ends_with('&');
            let target = redirect.target.raw_unquoted();
            if writes && looks_like_windows_path(target) {
                return Some(target.to_string());
            }
        }
        if seg.name() == "tee"
            && let Some(file) = seg.args.iter().find(|a| !a.text.starts_with('-'))
            && looks_like_windows_path(file.raw_unquoted())
        {
            return Some(file.raw_unquoted().to_string());
        }
    }
    None
}

/// True for tokens that begin with a Windows-style drive letter prefix.
fn looks_like_windows_path(tok: &str) -> bool {
    let mut chars = tok.chars();
    let Some(c) = chars.next() else { return false };
    if !c.is_ascii_alphabetic() {
        return false;
    }
    if chars.next() != Some(':') {
        return false;
    }
    matches!(chars.next(), Some('\\' | '/'))
}

/// The `windows-path-redirect` gate. Returns `Deny` when the command pipes
/// output to a Windows-style absolute path; the POSIX shell mangles it into
/// a junk filename in the CWD.
pub(super) fn bash_windows_redirect(segments: &[Segment], cmd: &str) -> Option<Verdict> {
    let target = windows_path_target(segments)?;
    Some(Verdict::Deny {
        reason: format!(
            "[bash-windows-redirect] Refusing to redirect to Windows-style path `{target}`.\n\
             The Bash tool runs a POSIX shell on every platform (git-bash on Windows, \
             bash/zsh on Linux/macOS). A `C:\\…` / `C:/…` redirect target is either \
             mangled (Windows: the `:` breaks redirect parsing and the `\\` is consumed \
             as an escape, producing junk filenames like `CAtizscan-out.json` in the \
             current directory) or taken literally (Linux/macOS: a file named `C:\\Atiz\\…`).\n\
             Fix: on Windows use a POSIX path (e.g. `/c/Atiz/...`) or run from PowerShell; \
             on Linux/macOS use a real POSIX absolute path. Relative paths work everywhere.\n\
             Command: {}",
            truncate(cmd, 160)
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::super::lex::segments;
    use super::*;

    fn bash_windows_redirect(cmd: &str) -> Option<Verdict> {
        super::bash_windows_redirect(&segments(cmd), cmd)
    }

    // The POSIX shell that powers the Bash tool mangles redirects to `C:\...`
    // style paths, producing junk filenames like `CAtizscan-out.json` in the
    // CWD. The gate must catch this before the shell ever sees the command.

    #[test]
    fn windows_redirect_denies_backslash_drive() {
        let v = bash_windows_redirect("mustard-rt run scan > C:\\Atiz\\scan-out.json");
        match v {
            Some(Verdict::Deny { reason }) => {
                assert!(reason.contains("bash-windows-redirect"), "reason: {reason}");
                assert!(reason.contains("C:\\Atiz\\scan-out.json"), "reason: {reason}");
            }
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn windows_redirect_denies_forward_slash_drive() {
        let v = bash_windows_redirect("cmd > C:/temp/scan-validate-out.json");
        assert!(matches!(v, Some(Verdict::Deny { .. })));
    }

    #[test]
    fn windows_redirect_denies_append() {
        let v = bash_windows_redirect("echo line >> D:\\logs\\app.log");
        assert!(matches!(v, Some(Verdict::Deny { .. })));
    }

    #[test]
    fn windows_redirect_denies_stderr_to_windows_path() {
        let v = bash_windows_redirect("rtk cargo test 2> C:\\Atiz\\mustard\\test-err.txt");
        assert!(matches!(v, Some(Verdict::Deny { .. })));
    }

    #[test]
    fn windows_redirect_denies_combined_redirect() {
        let v = bash_windows_redirect("cmd &> C:\\Atiz\\out.txt");
        assert!(matches!(v, Some(Verdict::Deny { .. })));
    }

    #[test]
    fn windows_redirect_denies_quoted_target() {
        let v = bash_windows_redirect("cmd > \"C:\\Program Files\\out.txt\"");
        assert!(matches!(v, Some(Verdict::Deny { .. })));
    }

    #[test]
    fn windows_redirect_denies_tee_to_windows_path() {
        let v = bash_windows_redirect("cmd | tee C:\\Atiz\\scan-out.json");
        assert!(matches!(v, Some(Verdict::Deny { .. })));
        let v = bash_windows_redirect("cmd | tee -a C:/Atiz/scan-out.json");
        assert!(matches!(v, Some(Verdict::Deny { .. })));
    }

    #[test]
    fn windows_redirect_allows_posix_absolute() {
        // `/c/Atiz/...` is the git-bash equivalent and works correctly.
        assert!(bash_windows_redirect("cmd > /c/Atiz/scan-out.json").is_none());
    }

    #[test]
    fn windows_redirect_allows_relative_target() {
        assert!(bash_windows_redirect("cmd > output.txt").is_none());
        assert!(bash_windows_redirect("cmd > ./out/scan.json").is_none());
        assert!(bash_windows_redirect("cmd >> logs/app.log").is_none());
    }

    #[test]
    fn windows_redirect_allows_fd_dup() {
        // `2>&1` is fd duplication, not a path. Must not trigger.
        assert!(bash_windows_redirect("cmd 2>&1").is_none());
        assert!(bash_windows_redirect("cmd >&2").is_none());
    }

    #[test]
    fn windows_redirect_allows_windows_path_in_argument() {
        // Path is a program argument (no redirect), not a redirect target.
        // The gate only catches `>`-style mangling.
        assert!(bash_windows_redirect("node script.js --out C:\\Atiz\\x.json").is_none());
    }

    #[test]
    fn windows_redirect_allows_windows_path_inside_quoted_string() {
        // The `>` is inside a quoted string, so the shell does not treat it
        // as a redirect operator. Must not trigger.
        assert!(bash_windows_redirect("echo 'wrote > C:\\Atiz\\x.json'").is_none());
    }

    #[test]
    fn a_redirect_after_a_separator_is_still_checked() {
        let v = bash_windows_redirect("cd x && cmd > C:\\a.txt");
        assert!(matches!(v, Some(Verdict::Deny { .. })));
    }

    #[test]
    fn a_windows_path_inside_a_heredoc_is_not_a_redirect() {
        assert!(bash_windows_redirect("cat <<'EOF'\nwrote > C:\\a.txt\nEOF").is_none());
    }
}
