//! `windows_redirect` — rewrite `> C:\...` / `2> D:/...` style redirects.
//!
//! Mustard runs on Windows, Linux and macOS, but the Bash tool always invokes
//! a POSIX shell — git-bash on Windows, native bash/zsh elsewhere. A redirect
//! target that starts with a Windows drive letter (`C:\`, `D:/`, …) is
//! either mangled (Windows: the `:` confuses redirect parsing, the `\` is
//! consumed as an escape — producing junk filenames like `CAtizscan-out.json`
//! in the CWD) or interpreted literally (Linux/macOS: a file named `C:\Atiz\…`
//! which is also never what the caller wanted). Either way the author meant
//! an absolute path and the redirect will not produce one, and there is a
//! certain fix: the same path, with the drive letter as a folder under `/`
//! and forward slashes, like `/c/Atiz/scan-out.json`. The gate rewrites the
//! command to that form and lets it run, with a short note of what changed.
//!
//! It reads the commands [`super::lex::segments`] found: the output redirects
//! of each one, and the file a `tee` writes. A path is judged by its raw
//! spelling, because the shell eats the backslashes of an unquoted word.

use serde_json::Value;

use mustard_core::domain::model::contract::{HookInput, Verdict};
use mustard_core::{translate, SupportedLocale};

use super::lex::Segment;

/// The rewrite the gate makes when it finds a Windows-style redirect target:
/// `raw` is the word exactly as written (quotes included, when there are
/// any), to replace inside the command text; `original` and `posix` are the
/// path before and after the fix, unquoted, for the note.
struct WindowsRedirectHit {
    raw: String,
    original: String,
    posix: String,
}

/// The Windows-style path (`X:\…` or `X:/…`) in the form the POSIX shell
/// understands: the drive letter becomes a lowercase folder under `/`, and
/// every backslash becomes a forward slash. `path` is assumed to already
/// pass [`looks_like_windows_path`], so its first two bytes are a single
/// ASCII letter and a colon.
fn to_posix_path(path: &str) -> String {
    let drive = path.as_bytes()[0].to_ascii_lowercase() as char;
    let rest = path[2..].trim_start_matches(['\\', '/']).replace('\\', "/");
    format!("/{drive}/{rest}")
}

/// The first output redirect (`>`, `>>`, `2>`, `&>`, `>|`) or `tee` file whose
/// target looks like a Windows path (`X:\…` or `X:/…`).
fn windows_path_target(segments: &[Segment]) -> Option<WindowsRedirectHit> {
    for seg in segments {
        for redirect in &seg.redirects {
            let writes = redirect.op.contains('>') && !redirect.op.ends_with('&');
            let original = redirect.target.raw_unquoted();
            if writes && looks_like_windows_path(original) {
                return Some(WindowsRedirectHit {
                    raw: redirect.target.raw.clone(),
                    posix: to_posix_path(original),
                    original: original.to_string(),
                });
            }
        }
        if seg.name() == "tee"
            && let Some(file) = seg.args.iter().find(|a| !a.text.starts_with('-'))
        {
            let original = file.raw_unquoted();
            if looks_like_windows_path(original) {
                return Some(WindowsRedirectHit {
                    raw: file.raw.clone(),
                    posix: to_posix_path(original),
                    original: original.to_string(),
                });
            }
        }
    }
    None
}

/// `replacement` wrapped in the same quote `raw` opens and closes with, or
/// bare when `raw` carries none — so a path with a space (`"C:\Program
/// Files\out.txt"`) stays quoted after the rewrite.
fn requote(raw: &str, replacement: &str) -> String {
    for quote in ['"', '\''] {
        if raw.len() >= 2 && raw.starts_with(quote) && raw.ends_with(quote) {
            return format!("{quote}{replacement}{quote}");
        }
    }
    replacement.to_string()
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

/// The `windows-path-redirect` gate. Returns `Rewrite` when the command pipes
/// output to a Windows-style absolute path; the POSIX shell mangles it into
/// a junk filename in the CWD, so the gate swaps the path for the form the
/// shell understands and lets the command run, with a note written in `lang`.
pub(super) fn bash_windows_redirect(segments: &[Segment], cmd: &str, input: &HookInput, lang: SupportedLocale) -> Option<Verdict> {
    let hit = windows_path_target(segments)?;
    let command = cmd.replacen(&hit.raw, &requote(&hit.raw, &hit.posix), 1);
    let mut tool_input = input.tool_input.clone();
    let fields = tool_input.as_object_mut()?;
    fields.insert("command".to_string(), Value::String(command));
    let note = translate("command_guard.windows_path_rewritten", lang)
        .replace("{original}", &hit.original)
        .replace("{posix}", &hit.posix);
    Some(Verdict::Rewrite { tool_input, note: Some(note) })
}

#[cfg(test)]
mod tests {
    use super::super::lex::segments;
    use super::*;
    use serde_json::json;

    fn bash_windows_redirect(cmd: &str) -> Option<Verdict> {
        let input = HookInput { tool_input: json!({ "command": cmd }), ..HookInput::default() };
        super::bash_windows_redirect(&segments(cmd), cmd, &input, SupportedLocale::PtBr)
    }

    /// The rewritten `command` of a `Rewrite` verdict, or a panic naming what
    /// came instead.
    fn rewritten_command(v: Option<Verdict>) -> String {
        match v {
            Some(Verdict::Rewrite { tool_input, .. }) => tool_input["command"].as_str().unwrap().to_string(),
            other => panic!("expected a rewrite, got {other:?}"),
        }
    }

    // The POSIX shell that powers the Bash tool mangles redirects to `C:\...`
    // style paths, producing junk filenames like `CAtizscan-out.json` in the
    // CWD. The gate rewrites the path before the shell ever sees the command.

    #[test]
    fn windows_redirect_rewrites_backslash_drive() {
        let cmd = "mustard-rt run scan > C:\\Atiz\\scan-out.json";
        for lang in [SupportedLocale::PtBr, SupportedLocale::EnUs] {
            let input = HookInput { tool_input: json!({ "command": cmd }), ..HookInput::default() };
            match super::bash_windows_redirect(&segments(cmd), cmd, &input, lang) {
                Some(Verdict::Rewrite { tool_input, note }) => {
                    assert_eq!(tool_input["command"], "mustard-rt run scan > /c/Atiz/scan-out.json");
                    let expected = translate("command_guard.windows_path_rewritten", lang)
                        .replace("{original}", "C:\\Atiz\\scan-out.json")
                        .replace("{posix}", "/c/Atiz/scan-out.json");
                    assert_eq!(note, Some(expected));
                }
                other => panic!("expected Rewrite, got {other:?}"),
            }
        }
    }

    #[test]
    fn windows_redirect_rewrites_forward_slash_drive() {
        let v = bash_windows_redirect("cmd > C:/temp/scan-validate-out.json");
        assert_eq!(rewritten_command(v), "cmd > /c/temp/scan-validate-out.json");
    }

    #[test]
    fn windows_redirect_rewrites_append() {
        let v = bash_windows_redirect("echo line >> D:\\logs\\app.log");
        assert_eq!(rewritten_command(v), "echo line >> /d/logs/app.log");
    }

    #[test]
    fn windows_redirect_rewrites_stderr_to_windows_path() {
        let v = bash_windows_redirect("rtk cargo test 2> C:\\Atiz\\mustard\\test-err.txt");
        assert_eq!(rewritten_command(v), "rtk cargo test 2> /c/Atiz/mustard/test-err.txt");
    }

    #[test]
    fn windows_redirect_rewrites_combined_redirect() {
        let v = bash_windows_redirect("cmd &> C:\\Atiz\\out.txt");
        assert_eq!(rewritten_command(v), "cmd &> /c/Atiz/out.txt");
    }

    #[test]
    fn windows_redirect_rewrites_quoted_target() {
        let v = bash_windows_redirect("cmd > \"C:\\Program Files\\out.txt\"");
        assert_eq!(rewritten_command(v), "cmd > \"/c/Program Files/out.txt\"");
    }

    #[test]
    fn windows_redirect_rewrites_tee_to_windows_path() {
        let v = bash_windows_redirect("cmd | tee C:\\Atiz\\scan-out.json");
        assert_eq!(rewritten_command(v), "cmd | tee /c/Atiz/scan-out.json");
        let v = bash_windows_redirect("cmd | tee -a C:/Atiz/scan-out.json");
        assert_eq!(rewritten_command(v), "cmd | tee -a /c/Atiz/scan-out.json");
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
    fn a_redirect_after_a_separator_is_still_rewritten() {
        let v = bash_windows_redirect("cd x && cmd > C:\\a.txt");
        assert_eq!(rewritten_command(v), "cd x && cmd > /c/a.txt");
    }

    #[test]
    fn a_windows_path_inside_a_heredoc_is_not_a_redirect() {
        assert!(bash_windows_redirect("cat <<'EOF'\nwrote > C:\\a.txt\nEOF").is_none());
    }
}
