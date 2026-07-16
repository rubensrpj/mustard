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

use mustard_core::domain::model::contract::Verdict;

use super::lex::truncate;

/// Scan `cmd` for a shell redirect (`>`, `>>`, `2>`, `&>`, `|&`, `| tee`)
/// whose immediate target looks like a Windows path (`X:\…` or `X:/…`).
/// Returns the offending target so the deny message can quote it.
fn windows_path_redirect_target(cmd: &str) -> Option<String> {
    let bytes = cmd.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        // Skip quoted spans — redirects are shell-level, not inside quotes.
        if b == b'"' || b == b'\'' {
            let quote = b;
            i += 1;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            i += 1;
            continue;
        }
        // Detect a redirect operator. We collapse all variants to "operator
        // ended at position `end`" then sniff the next non-space token.
        let end = match (b, bytes.get(i + 1).copied(), bytes.get(i + 2).copied()) {
            // `&>` and `&>>`
            (b'&', Some(b'>'), Some(b'>')) => Some(i + 3),
            (b'&', Some(b'>'), _) => Some(i + 2),
            // `>>`
            (b'>', Some(b'>'), _) => Some(i + 2),
            // `2>` (and `1>`); not preceded by `<` or `>`.
            (d, Some(b'>'), next) if d.is_ascii_digit() => {
                if next == Some(b'>') {
                    Some(i + 3)
                } else if next != Some(b'&') {
                    Some(i + 2)
                } else {
                    None
                }
            }
            // Bare `>`. Skip `>>` (handled above) and `>&` (fd dup).
            (b'>', next, _) => {
                let prev = i.checked_sub(1).map(|p| bytes[p]);
                if prev == Some(b'<') || prev == Some(b'>') || next == Some(b'&') {
                    None
                } else {
                    Some(i + 1)
                }
            }
            _ => None,
        };
        if let Some(start) = end {
            if let Some(target) = next_token_after(cmd, start) {
                if looks_like_windows_path(&target) {
                    return Some(target);
                }
            }
            i = start;
            continue;
        }
        i += 1;
    }
    // Also catch `| tee <winpath>` / `| tee -a <winpath>` — tee writes a file.
    for seg in cmd.split('|') {
        let seg = seg.trim_start();
        let mut tokens = seg.split_whitespace();
        if tokens.next() != Some("tee") {
            continue;
        }
        for tok in tokens {
            if tok.starts_with('-') {
                continue;
            }
            if looks_like_windows_path(tok) {
                return Some(tok.to_string());
            }
            break;
        }
    }
    None
}

/// Extract the next whitespace-delimited token starting at or after `start`,
/// stripping a single layer of surrounding quotes if present.
fn next_token_after(cmd: &str, start: usize) -> Option<String> {
    let rest = cmd.get(start..)?;
    let trimmed = rest.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    let bytes = trimmed.as_bytes();
    let raw = if bytes[0] == b'"' || bytes[0] == b'\'' {
        let quote = bytes[0];
        let mut end = 1;
        while end < bytes.len() && bytes[end] != quote {
            end += 1;
        }
        &trimmed[1..end.min(bytes.len())]
    } else {
        let end = trimmed
            .find(|c: char| c.is_whitespace() || c == '|' || c == '&' || c == ';')
            .unwrap_or(trimmed.len());
        &trimmed[..end]
    };
    Some(raw.to_string())
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
pub(super) fn bash_windows_redirect(cmd: &str) -> Option<Verdict> {
    let target = windows_path_redirect_target(cmd)?;
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
    use super::*;

    // Regression: the POSIX shell that powers the Bash tool mangles redirects
    // to `C:\...` style paths, producing junk filenames like
    // `CAtizscan-out.json` in the CWD. The gate must catch this before the
    // shell ever sees the command.

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
}
