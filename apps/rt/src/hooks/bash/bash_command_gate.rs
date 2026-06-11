//! `bash_command_gate` — the consolidated Bash-tool enforcement module.
//!
//! ## Scope (b3 Bash family, 5/5)
//!
//! This module ports **all five** of the Bash-tool JavaScript hooks
//! (b3 spec § Arquitetura table): `bash-safety`, `bash-native-redirect`,
//! `rtk-rewrite` and `review-gate` as PreToolUse(Bash) gates, plus `pr-detect`
//! as a PostToolUse(Bash) `Observer`. Consolidation **regroups, it does not
//! re-decide** — every verdict below is a 1:1 port of the JS decision logic;
//! the parity tests (and `hooks/__tests__/hooks.test.js` /
//! `harness-wave9.test.js`) are the oracle.
//!
//! `BashCommandGate` therefore implements [`Check`] for PreToolUse(Bash) **and**
//! [`Observer`] for PostToolUse(Bash).
//!
//! `rtk-rewrite` shells out to `rtk rewrite <cmd>` with a 2-second timeout.
//! On exit-0 with a non-empty, distinct stdout the gate returns
//! `Verdict::Rewrite` carrying the rewritten command. Every other path —
//! already `rtk`-prefixed, `rtk` missing, non-zero exit, timeout, empty or
//! identical output — returns `None` (silent allow, fail-open).
//!
//! `review-gate` (`git commit` gate) computes its verdict with its **own**
//! mode variable `MUSTARD_COMMIT_GATE_MODE` (default `warn`), independent of
//! the module-level enforcement mode the dispatcher applies — the dispatcher
//! repasses the verdict without downgrade.

use crate::shared::context::current_spec;
use mustard_core::platform::config::Mode;
use mustard_core::domain::economy::estimator;
use mustard_core::ClaudePaths;
use mustard_core::platform::error::Error;
use mustard_core::platform::process::rtk_command;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Observer, Trigger, Verdict};
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde_json::json;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::util::format_gate_message;
use mustard_core::time::now_iso8601;

/// The consolidated Bash-tool enforcement module.
pub struct BashCommandGate;

// ---------------------------------------------------------------------------
// bash-safety — deny dangerous commands
// ---------------------------------------------------------------------------

/// One dangerous-command rule: a substring/structural test plus the user
/// message. Ported from the `DANGEROUS` table in `bash-safety.js`.
///
/// The JS uses regexes; this port uses explicit predicates that reproduce the
/// same matches without a regex dependency. Each predicate is documented with
/// the JS pattern it mirrors.
struct DangerRule {
    /// Stable identifier for this rule (`BG01`–`BG13`).
    id: &'static str,
    /// `true` when `cmd` (already lowercased) matches this rule.
    test: fn(&str) -> bool,
    /// The user-facing reason fragment (the JS `msg`).
    msg: &'static str,
}

/// Whitespace-tolerant "word A followed by word B" check on a lowercased
/// command. Mirrors the `\bA\s+B\b`-style regexes in `bash-safety.js`.
fn has_word_pair(cmd: &str, a: &str, b: &str) -> bool {
    let mut search_from = 0;
    while let Some(rel) = cmd[search_from..].find(a) {
        let a_start = search_from + rel;
        let a_end = a_start + a.len();
        // Left boundary: start of string or a non-word char before `a`.
        let left_ok = a_start == 0
            || !cmd.as_bytes()[a_start - 1].is_ascii_alphanumeric();
        // The gap between A and B must be whitespace (at least one char).
        let rest = &cmd[a_end..];
        let trimmed = rest.trim_start();
        let had_ws = trimmed.len() < rest.len();
        if left_ok && had_ws && trimmed.starts_with(b) {
            let b_end_byte = trimmed.as_bytes().get(b.len());
            let right_ok = b_end_byte.is_none_or(|c| !c.is_ascii_alphanumeric());
            if right_ok {
                return true;
            }
        }
        search_from = a_end;
    }
    false
}

/// `true` if `cmd` contains `needle` with a word boundary on its left — the
/// `\bneedle` shape. Used for standalone-word rules (`mkfs`, `shutdown`, …).
fn has_word(cmd: &str, needle: &str) -> bool {
    let mut from = 0;
    while let Some(rel) = cmd[from..].find(needle) {
        let start = from + rel;
        let left_ok =
            start == 0 || !cmd.as_bytes()[start - 1].is_ascii_alphanumeric();
        if left_ok {
            return true;
        }
        from = start + needle.len();
    }
    false
}

/// The dangerous-command rules, in `bash-safety.js` order.
const DANGER_RULES: &[DangerRule] = &[
    // /\brm\s+(-\w*r\w*f|--no-preserve-root|-rf|-fr)\b/i
    DangerRule {
        id: "BG01",
        test: is_rm_recursive_force,
        msg: "Recursive force delete blocked",
    },
    // /\bgit\s+push\s+(-\w*f\b|--force(?!-with-lease))\b/i
    DangerRule {
        id: "BG02",
        test: is_force_push,
        msg: "Force push blocked (use --force-with-lease for safer overwrite)",
    },
    // /\bgit\s+reset\s+--hard\b/i
    DangerRule {
        id: "BG03",
        test: |c| has_word_pair(c, "git", "reset") && c.contains("--hard"),
        msg: "git reset --hard blocked",
    },
    // /\bgit\s+clean\s+-f/i
    DangerRule {
        id: "BG04",
        test: is_git_clean_force,
        msg: "git clean -f blocked",
    },
    // /\bgit\s+checkout\s+--\s*\.\s*$/i
    DangerRule {
        id: "BG05",
        test: |c| ends_with_token_seq(c, &["git", "checkout", "--", "."]),
        msg: "git checkout -- . blocked",
    },
    // /\bgit\s+restore\s+\.\s*$/i
    DangerRule {
        id: "BG06",
        test: |c| ends_with_token_seq(c, &["git", "restore", "."]),
        msg: "git restore . blocked",
    },
    // /\bgit\s+branch\s+-D\s+(main|master)\b/i
    DangerRule {
        id: "BG07",
        test: is_branch_delete_main,
        msg: "Deleting main/master branch blocked",
    },
    // /\bchmod\s+777\b/i
    DangerRule {
        id: "BG08",
        test: |c| has_word_pair(c, "chmod", "777"),
        msg: "chmod 777 blocked",
    },
    // /\bmkfs\b/i
    DangerRule {
        id: "BG09",
        test: |c| has_word(c, "mkfs"),
        msg: "mkfs blocked",
    },
    // /\bdd\s+if=/i
    DangerRule {
        id: "BG10",
        test: |c| has_word_pair(c, "dd", "if="),
        msg: "dd if= blocked",
    },
    // /\bformat\s+[A-Z]:/i
    DangerRule {
        id: "BG11",
        test: is_format_drive,
        msg: "format drive blocked",
    },
    // /\bshutdown\b/i
    DangerRule {
        id: "BG12",
        test: |c| has_word(c, "shutdown"),
        msg: "shutdown blocked",
    },
    // /\breboot\b/i
    DangerRule {
        id: "BG13",
        test: |c| has_word(c, "reboot"),
        msg: "reboot blocked",
    },
];

/// `\brm\s+(-\w*r\w*f|--no-preserve-root|-rf|-fr)\b` — `rm` followed by a flag
/// token that means recursive+force.
fn is_rm_recursive_force(cmd: &str) -> bool {
    for word in split_after(cmd, "rm") {
        if word == "--no-preserve-root" {
            return true;
        }
        if let Some(flag) = word.strip_prefix('-') {
            if flag.starts_with("--") {
                continue;
            }
            // -rf / -fr / -Rf / a flag cluster containing both r and f.
            let has_r = flag.contains('r') || flag.contains('R');
            let has_f = flag.contains('f');
            if has_r && has_f {
                return true;
            }
        }
    }
    false
}

/// `\bgit\s+push\s+(-\w*f\b|--force(?!-with-lease))\b`.
fn is_force_push(cmd: &str) -> bool {
    if !has_word_pair(cmd, "git", "push") {
        return false;
    }
    for word in cmd.split_whitespace() {
        if word == "--force" {
            return true;
        }
        if word.starts_with("--force-with-lease") {
            // Explicitly the safe form — not a force-push for this rule.
            continue;
        }
        if let Some(flag) = word.strip_prefix('-') {
            if !flag.starts_with('-') && flag.contains('f') {
                return true;
            }
        }
    }
    false
}

/// `\bgit\s+clean\s+-f` — `git clean` with a flag token containing `f`.
fn is_git_clean_force(cmd: &str) -> bool {
    if !has_word_pair(cmd, "git", "clean") {
        return false;
    }
    cmd.split_whitespace().any(|w| {
        w.strip_prefix('-')
            .is_some_and(|f| !f.starts_with('-') && f.contains('f'))
    })
}

/// `\bgit\s+branch\s+-D\s+(main|master)\b`.
fn is_branch_delete_main(cmd: &str) -> bool {
    if !has_word_pair(cmd, "git", "branch") {
        return false;
    }
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    tokens.windows(2).any(|w| {
        (w[0] == "-d" || w[0] == "-D") && (w[1] == "main" || w[1] == "master")
    })
}

/// `\bformat\s+[A-Z]:` — `format` followed by a drive letter and `:`.
/// The JS regex is case-insensitive on `format` but the drive class `[A-Z]`
/// is matched against the *original* command; this port lowercases the
/// command first, so the drive letter is matched lowercased — `format c:`
/// still matches, which is the intended behaviour.
fn is_format_drive(cmd: &str) -> bool {
    for word in split_after(cmd, "format") {
        let bytes = word.as_bytes();
        if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            return true;
        }
    }
    false
}

/// The whitespace-separated tokens that appear *after* the first occurrence of
/// `anchor` as a word. Empty when `anchor` is absent.
fn split_after<'a>(cmd: &'a str, anchor: &str) -> Vec<&'a str> {
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    if let Some(pos) = tokens.iter().position(|t| *t == anchor) {
        tokens[pos + 1..].to_vec()
    } else {
        Vec::new()
    }
}

/// `true` if the command's token sequence *ends with* `seq` (trailing
/// whitespace already removed by `split_whitespace`). Mirrors the `…\s*$`
/// anchored regexes for `git checkout -- .` and `git restore .`.
fn ends_with_token_seq(cmd: &str, seq: &[&str]) -> bool {
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    tokens.len() >= seq.len() && &tokens[tokens.len() - seq.len()..] == seq
}

/// The `bash-safety` gate: deny if any dangerous rule matches.
fn bash_safety(cmd: &str) -> Option<Verdict> {
    let lower = cmd.to_ascii_lowercase();
    for rule in DANGER_RULES {
        if (rule.test)(&lower) {
            return Some(Verdict::Deny {
                reason: format!(
                    "[bash-safety {}] {}.\nCommand: {}",
                    rule.id,
                    rule.msg,
                    truncate(cmd, 120)
                ),
            });
        }
    }
    None
}

// ---------------------------------------------------------------------------
// windows-path-redirect — deny `> C:\...` / `2> D:/...` style redirects
// ---------------------------------------------------------------------------
//
// Mustard runs on Windows, Linux and macOS, but the Bash tool always invokes
// a POSIX shell — git-bash on Windows, native bash/zsh elsewhere. A redirect
// target that starts with a Windows drive letter (`C:\`, `D:/`, …) is
// either mangled (Windows: the `:` confuses redirect parsing, the `\` is
// consumed as an escape — producing junk filenames like `CAtizscan-out.json`
// in the CWD) or interpreted literally (Linux/macOS: a file named `C:\Atiz\…`
// which is also never what the caller wanted). Either way the author meant
// an absolute path and the redirect will not produce one. This gate makes
// that failure mode loud instead of silent on every platform.

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
    let (raw, _) = if bytes[0] == b'"' || bytes[0] == b'\'' {
        let quote = bytes[0];
        let mut end = 1;
        while end < bytes.len() && bytes[end] != quote {
            end += 1;
        }
        (&trimmed[1..end.min(bytes.len())], end + 1)
    } else {
        let end = trimmed
            .find(|c: char| c.is_whitespace() || c == '|' || c == '&' || c == ';')
            .unwrap_or(trimmed.len());
        (&trimmed[..end], end)
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
fn bash_windows_redirect(cmd: &str) -> Option<Verdict> {
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

// ---------------------------------------------------------------------------
// bash-native-redirect — deny / advise native-tool equivalents
// ---------------------------------------------------------------------------

/// The redirect map from `bash-native-redirect.js`: command → (native tool,
/// tip). Order is irrelevant — lookup is by first token.
const REDIRECT_MAP: &[(&str, &str, &str)] = &[
    ("grep", "Grep", "Grep(pattern, path, output_mode) — faster, no shell overhead"),
    ("rg", "Grep", "Grep tool is built on ripgrep — same power, structured output"),
    ("egrep", "Grep", "Grep(pattern) supports full regex syntax"),
    ("fgrep", "Grep", "Grep(pattern, -i) for case-insensitive search"),
    ("cat", "Read", "Read(file_path) — structured output with line numbers"),
    ("head", "Read", "Read(file_path, limit: N) — reads first N lines"),
    ("tail", "Read", "Read(file_path, offset: N) — reads from line N"),
    ("less", "Read", "Read(file_path, offset, limit) — paginated reading"),
    ("more", "Read", "Read(file_path) — full file reading"),
    ("ls", "Glob", "Glob(pattern) — e.g. \"src/**/*.ts\" for recursive listing"),
    ("find", "Glob", "Glob(pattern) — e.g. \"**/*.cs\" for pattern matching"),
    ("tree", "Glob", "Glob(pattern) — structured file listing by pattern"),
];

/// Look up the redirect target for a (lowercased) first token.
fn redirect_for(token: &str) -> Option<(&'static str, &'static str)> {
    REDIRECT_MAP
        .iter()
        .find(|(name, _, _)| *name == token)
        .map(|(_, tool, tip)| (*tool, *tip))
}

/// Read-only search commands that `rtk` does **not** filter — it execs the
/// bare binary, which may be absent (e.g. `rg` on Windows → exit 127). These
/// are redirected to the native Grep tool *even when prefixed with `rtk`*,
/// unlike `rtk grep` / `rtk cat` (which `rtk` filters and which exist on this
/// platform — those keep passing through). User decision 2026-05-21.
const RTK_TRANSPARENT_REDIRECT: &[&str] = &["rg", "egrep", "fgrep"];

/// Strip a single leading `rtk ` wrapper token, returning the rest. When `cmd`
/// is not `rtk`-prefixed it is returned unchanged.
fn strip_leading_rtk(cmd: &str) -> &str {
    let trimmed = cmd.trim_start();
    if let Some(rest) = trimmed.strip_prefix("rtk") {
        if rest.starts_with(char::is_whitespace) {
            return rest.trim_start();
        }
    }
    cmd
}

/// Replace shell metacharacters that appear *inside single/double quotes* with
/// spaces, leaving everything else (including the quote chars and the byte
/// length) intact. Used so that a quoted argument like a Grep alternation
/// pattern (`"emit-pipeline|emit-phase"`) is not mistaken for a real shell
/// pipe by [`has_shell_operator`] / the segment splitters. Only single ASCII
/// operator bytes are swapped for a single ASCII space, so the result is
/// always valid UTF-8 and byte-aligned with the input.
fn mask_quoted_operators(cmd: &str) -> String {
    let mut out: Vec<u8> = Vec::with_capacity(cmd.len());
    let mut quote: Option<u8> = None;
    for &b in cmd.as_bytes() {
        if let Some(q) = quote {
            if b == q {
                quote = None;
                out.push(b);
            } else if matches!(b, b'&' | b'|' | b';' | b'>' | b'<' | b'`' | b'\n' | b'\r') {
                out.push(b' ');
            } else {
                out.push(b);
            }
        } else {
            if b == b'\'' || b == b'"' {
                quote = Some(b);
            }
            out.push(b);
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| cmd.to_string())
}

/// Shell operator that marks a composed command: `[|&;]`, `$(`, backtick, `<<`, `>>`.
fn has_shell_operator(cmd: &str) -> bool {
    cmd.contains('|')
        || cmd.contains('&')
        || cmd.contains(';')
        || cmd.contains("$(")
        || cmd.contains('`')
        || cmd.contains("<<")
        || cmd.contains(">>")
}

/// `true` when `c` separates one shell command from the next: `&`, `|`, `;`,
/// or a newline. Newlines matter because the Bash tool routinely receives
/// multi-line `command` strings (a sanity `echo` on line 1, the real `rtk …`
/// on line 2); bash treats the line break exactly like `;`, so the segment
/// splitters must too — otherwise an `rtk`-prefixed later line is invisible to
/// the "already wrapped" short-circuit and the gate wrongly denies the whole
/// command.
fn is_cmd_separator(c: char) -> bool {
    c == '&' || c == '|' || c == ';' || c == '\n' || c == '\r'
}

/// `(?<![<>])>(?![>])` — a lone `>` output redirect (writing to a file).
/// `>>` and `2>` are explicitly *not* this. Mirrors `OUTPUT_REDIRECT_RE`
/// applied after `stripStderrRedirects`.
fn has_output_redirect(cmd: &str) -> bool {
    let bytes = cmd.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'>' {
            continue;
        }
        let prev = i.checked_sub(1).map(|p| bytes[p]);
        let next = bytes.get(i + 1).copied();
        if prev != Some(b'<') && prev != Some(b'>') && next != Some(b'>') {
            return true;
        }
    }
    false
}

/// Strip trailing `2>/dev/null` / `2>&1` — `stripStderrRedirects`.
fn strip_stderr_redirects(cmd: &str) -> String {
    let trimmed = cmd.trim();
    for suffix in ["2>/dev/null", "2>&1"] {
        if let Some(stripped) = trimmed.strip_suffix(suffix) {
            return stripped.trim_end().to_string();
        }
    }
    trimmed.to_string()
}

/// First executable token, skipping `VAR=value` env-prefix tokens.
/// Mirrors `firstToken` in `bash-native-redirect.js`.
fn first_token(cmd: &str) -> Option<&str> {
    cmd.split_whitespace().find(|tok| !tok.contains('='))
}

/// `\bsed\s+(-\w*i\w*|-i\b)` — a `sed` invocation with an in-place flag.
fn is_sed_in_place(cmd: &str) -> bool {
    for word in split_after(cmd, "sed") {
        if let Some(flag) = word.strip_prefix('-') {
            if !flag.starts_with('-') && flag.contains('i') {
                return true;
            }
        }
    }
    false
}

/// The `bash-native-redirect` gate. Returns the verdict for a command, or
/// `None` to fall through to the next gate.
///
/// Verdicts, all 1:1 with `bash-native-redirect.js`:
/// - output redirect / `rtk` prefix / non-mapped command → `None` (pass).
/// - piped/chained with a redirectable first segment → `Inject` (advisory).
/// - read-only `grep`/`cat`/`ls`/`sed`… → `Deny`.
fn bash_native_redirect(raw_cmd: &str) -> Option<Verdict> {
    let cmd = strip_stderr_redirects(raw_cmd);
    if cmd.is_empty() {
        return None;
    }

    // Mask operators inside quotes so a quoted Grep pattern like
    // `"emit-pipeline|emit-phase"` is not mistaken for a shell pipe. Operator
    // and segment-boundary detection runs on the masked view; command names
    // and flags (never quoted) are unchanged, so token lookups still work.
    let masked = mask_quoted_operators(&cmd);

    // Output redirect — writing a file, not reading. Pass through.
    if has_output_redirect(&masked) {
        return None;
    }

    // Piped/chained: cannot deny safely. If the first segment is a
    // redirectable command, advise via `Inject`; otherwise pass.
    if has_shell_operator(&masked) {
        let first_segment = masked
            .split(is_cmd_separator)
            .next()
            .unwrap_or("")
            .trim();
        if let Some(seg_token) = first_token(first_segment) {
            // See through a leading `rtk` only for the ripgrep family (which
            // rtk does not filter); other `rtk`-prefixed first segments pass.
            let effective = if seg_token == "rtk" {
                first_token(strip_leading_rtk(first_segment)).unwrap_or("")
            } else {
                seg_token
            };
            let effective_lc = effective.to_ascii_lowercase();
            let advisable = effective != "rtk"
                && (seg_token != "rtk"
                    || RTK_TRANSPARENT_REDIRECT.contains(&effective_lc.as_str()));
            if advisable {
                if let Some((tool, tip)) = redirect_for(&effective_lc) {
                    return Some(Verdict::Inject {
                        context: format!(
                            "[Native Tool Redirect] The `{effective}` part of this piped \
                             command could use the {tool} tool instead. {tip}. Consider \
                             splitting the pipeline to use native tools where possible."
                        ),
                    });
                }
            }
        }
        return None;
    }

    let token = first_token(&cmd)?;

    // RTK-wrapped: pass through, EXCEPT for the ripgrep family (`rg`/`egrep`/
    // `fgrep`) which rtk execs as a bare binary that may be absent. Redirect
    // those to the native Grep tool; everything else (`rtk grep`, `rtk cat`,
    // `rtk cargo …`) still passes.
    if token == "rtk" {
        let inner = first_token(strip_leading_rtk(&cmd)).unwrap_or("");
        let inner_lc = inner.to_ascii_lowercase();
        if RTK_TRANSPARENT_REDIRECT.contains(&inner_lc.as_str()) {
            if let Some((tool, tip)) = redirect_for(&inner_lc) {
                return Some(Verdict::Deny {
                    reason: format!(
                        "[Native Tool Redirect] Use the {tool} tool instead of `rtk {inner_lc}` \
                         in Bash ({inner_lc} is not installed / not filtered by rtk). {tip}"
                    ),
                });
            }
        }
        return None;
    }

    // `sed`: deny only read-only sed; `sed -i` is a write, pass through.
    if token == "sed" {
        if is_sed_in_place(&cmd) {
            return None;
        }
        return Some(Verdict::Deny {
            reason: "[Native Tool Redirect] Use the Grep tool instead of `sed` in Bash. \
                     Grep(pattern) — for pattern extraction without shell sed overhead"
                .to_string(),
        });
    }

    let (tool, tip) = redirect_for(&token.to_ascii_lowercase())?;
    Some(Verdict::Deny {
        reason: format!(
            "[Native Tool Redirect] Use the {tool} tool instead of `{}` in Bash. {tip}",
            token.to_ascii_lowercase()
        ),
    })
}

// ---------------------------------------------------------------------------
// rtk-rewrite — rewrite a command through RTK
// ---------------------------------------------------------------------------

/// Subprocess timeout for `rtk rewrite` calls. The RTK binary is a local
/// process with no network I/O; 2 s is generous.
const RTK_REWRITE_TIMEOUT: Duration = Duration::from_secs(2);

/// The literal prefix of the rtk "no hook installed" advisory that rtk 0.34.1
/// emits on every invocation when no Claude Code hook is registered.
///
/// Background: `rtk hook` only supports Gemini CLI and Copilot (`gemini` /
/// `copilot` subcommands); it has no `claude` subcommand, so running `rtk init
/// -g` would violate the Mustard install contract (no writes to
/// `~/.claude/settings.json`). Mustard redirects all Bash through `bash_guard`,
/// so the advisory is pure noise inside the harness. RTK exposes no env var to
/// suppress it — the filter lives here, on the consumer side.
const RTK_NOISE_PREFIX: &str = "[rtk] /!\\ No hook installed";

/// Remove every line whose content starts with [`RTK_NOISE_PREFIX`] from `s`.
///
/// Lines are split on `\n`; a trailing newline produces a trailing empty token
/// that is preserved unchanged. Only the exact advisory prefix is matched —
/// all other output (including other `[rtk]` lines) is left intact.
///
/// # Example
///
/// ```text
/// "[rtk] /!\\ No hook installed — run `rtk init -g`\nrtk ls\n"
/// → "rtk ls\n"
/// ```
fn filter_rtk_noise(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut lines = s.split('\n').peekable();
    while let Some(line) = lines.next() {
        if line.starts_with(RTK_NOISE_PREFIX) {
            // Drop this line; do not re-add a newline separator.
            continue;
        }
        out.push_str(line);
        // Re-add the separator between lines (not after the last token when the
        // original string did not end with `\n`, to preserve trailing behaviour).
        if lines.peek().is_some() {
            out.push('\n');
        }
    }
    out
}

/// Spawn `rtk rewrite <cmd>` in a worker thread and return its stdout on
/// success.
///
/// Fail-open on four paths — returns `None` when:
/// 1. The binary cannot be spawned (ENOENT — `rtk` not installed).
/// 2. The process exits with a non-zero status (no RTK equivalent for `cmd`).
/// 3. The subprocess does not finish within [`RTK_REWRITE_TIMEOUT`].
/// 4. Stdout is empty or pure whitespace (RTK signalled "no rewrite").
///
/// The binary name defaults to `"rtk"` but can be overridden via the
/// `MUSTARD_RTK_BIN` environment variable — required for the fail-open unit
/// test (AC-7) so tests never depend on a real `rtk` in `PATH`.
#[must_use]
fn run_rtk_rewrite_subprocess(cmd: &str) -> Option<String> {
    let binary = std::env::var("MUSTARD_RTK_BIN").unwrap_or_else(|_| "rtk".into());
    run_rtk_rewrite_subprocess_with_bin(cmd, &binary)
}

/// Inner implementation of [`run_rtk_rewrite_subprocess`], accepting an
/// explicit binary name. Extracted so tests can inject a fake binary name
/// without mutating process environment (which is `unsafe` in edition 2024).
#[cfg_attr(not(test), allow(dead_code))]
fn run_rtk_rewrite_subprocess_with_bin(cmd: &str, binary: &str) -> Option<String> {
    let mut command = Command::new(binary);
    command
        .args(["rewrite", cmd])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    // Path 1: binary not found / ENOENT — fail open.
    let Ok(mut child) = command.spawn() else { return None };

    let (tx, rx) = std::sync::mpsc::channel();
    let stdout_handle = child.stdout.take();

    std::thread::spawn(move || {
        let status = child.wait();
        let _ = tx.send((status, child));
    });

    match rx.recv_timeout(RTK_REWRITE_TIMEOUT) {
        Ok((Ok(status), _child)) => {
            // Path 2: non-zero exit → no RTK equivalent — fail open.
            if !status.success() {
                return None;
            }
            let mut output = String::new();
            if let Some(mut out) = stdout_handle {
                use std::io::Read;
                let _ = out.read_to_string(&mut output);
            }
            // Filter the rtk "no hook installed" advisory before trimming.
            // Defense-in-depth: if a future rtk version emits the warning on
            // stdout instead of stderr (already null'd), this prevents a valid
            // rewritten command from being discarded as noise-only output.
            let output = filter_rtk_noise(&output);
            let trimmed = output.trim();
            // Path 4: empty / whitespace stdout — fail open.
            if trimmed.is_empty() {
                return None;
            }
            Some(trimmed.to_string())
        }
        // Wait itself errored — fail open.
        Ok((Err(_), _child)) => None,
        // Path 3: timeout — kill the child and fail open.
        Err(_) => {
            if let Ok((_, mut child)) = rx.recv_timeout(Duration::from_millis(0)) {
                let _ = child.kill();
            }
            None
        }
    }
}

/// Read `MUSTARD_RTK_GATE_MODE` and resolve to a [`Mode`]. Default
/// [`Mode::Warn`] — the `rtk`-on-everything mandate of
/// `2026-05-20-rtk-mandatory-everywhere` is enforced by **auto-rewriting** the
/// command (prepend `rtk`), not by rejecting it. Rewriting reaches the exact
/// same end state (every command runs under `rtk`) with ZERO round-trip: the
/// harness applies the [`Verdict::Rewrite`] through `updatedInput` (see
/// `protocol.rs`), and the eligibility guard [`should_blanket_prefix`] already
/// excludes the forms `rtk` cannot wrap (builtins, subshells), which pass
/// untouched. Denying-to-teach (`Mode::Strict`) is retained as an explicit
/// opt-in for a setup that wants a hard block, but it is no longer the default:
/// for an agent it only adds a re-submit round-trip with no durable learning.
/// Parse goes through [`Mode::parse`]; any unrecognised value collapses to
/// `Warn`.
fn rtk_gate_mode() -> Mode {
    std::env::var("MUSTARD_RTK_GATE_MODE")
        .ok()
        .and_then(|raw| Mode::parse(&raw))
        .unwrap_or(Mode::Warn)
}

/// Shells out to `rtk rewrite <cmd>` with a 2s timeout. On exit-0 with a
/// non-empty, distinct stdout, returns `Verdict::Rewrite` carrying the
/// rewritten command in `updatedInput`. Every other path — already
/// `rtk`-prefixed, `rtk` missing, non-zero exit, timeout, empty/identical
/// output — falls through to the blanket-prefix path. Mirrors the JS
/// `rtk-rewrite.js` contract and extends it with the Golden Rule blanket
/// fallback.
///
/// Split into two layers so unit tests can inject a closure instead of the
/// real subprocess and a [`Mode`] instead of mutating process env (`unsafe`
/// under edition 2024 because of `#![forbid(unsafe_code)]`):
/// - [`rtk_rewrite_with`] — pure logic, closure- and mode-injectable.
/// - [`rtk_rewrite`]      — production wrapper passing the real subprocess
///   and [`rtk_gate_mode`].
///
/// Returns `(Verdict, coverage_tag)` so the caller can emit `coverage` in
/// telemetry without embedding it in `tool_input`. The coverage tag for the
/// strict-mode Deny path is `"deny"`.
fn rtk_rewrite(cmd: &str) -> Option<(Verdict, &'static str)> {
    #[cfg(test)]
    {
        if RTK_REWRITE_TEST_OVERRIDE.with(|c| c.get()) {
            // Test override forces the gate off so unrelated `verdict_for`
            // tests can exercise bash-safety / native-redirect without the
            // strict-mode rtk gate denying every unprefixed command. Tests
            // that exercise the gate itself call `rtk_rewrite_with` directly
            // with an explicit `Mode`.
            return rtk_rewrite_with(cmd, |_| None, Mode::Off);
        }
    }
    rtk_rewrite_with(cmd, run_rtk_rewrite_subprocess, rtk_gate_mode())
}

/// Pure, testable core of `rtk-rewrite`. Accepts any `rewriter` closure so
/// tests can inject a fake subprocess without touching `PATH`, and a [`Mode`]
/// so tests can drive the gate without mutating process env (`unsafe` under
/// edition 2024 because of `#![forbid(unsafe_code)]`).
///
/// Short-circuits (returns `None`) when the command is already `rtk`-prefixed.
///
/// **`Mode::Off`** — gate disabled: returns `None` (no rewrite, no deny).
///
/// **`Mode::Strict`** (opt-in) — when the command is eligible for blanket
/// wrapping (not a builtin, no leading subshell/backtick) and the user did
/// not pre-prefix `rtk`, returns `Verdict::Deny` so the UI surfaces both the
/// original command and the required form. The agent then re-submits the
/// command with the `rtk` prefix. Coverage tag `"deny"`. NOT the default — a
/// deny only buys a round-trip; the rewrite below reaches the same end state.
///
/// **`Mode::Warn`** (default) — auto-rewrite the command:
/// - **Specific path**: delegates to `rewriter` for the actual rewrite attempt.
///   If the rewriter yields a non-empty, distinct string, returns
///   `Verdict::Rewrite` tagged `coverage = "specific"`.
/// - **Blanket-prefix fallback** (Golden Rule): when the specific path produces
///   nothing, [`should_blanket_prefix`] decides whether to prepend `rtk` to the
///   whole command. Returns `Verdict::Rewrite` tagged `coverage = "blanket"`.
///
/// Returns `(Verdict, coverage_tag)` so the caller can emit the tag in
/// telemetry without embedding it in `tool_input` (which is forwarded to
/// Claude Code as-is).
fn rtk_rewrite_with<F: FnOnce(&str) -> Option<String>>(
    cmd: &str,
    rewriter: F,
    mode: Mode,
) -> Option<(Verdict, &'static str)> {
    // Gate disabled — no rewrite, no deny.
    if mode == Mode::Off {
        return None;
    }

    // Short-circuit: already wrapped — never double-prefix, never deny.
    // Any segment of a pipeline/chain that starts with `rtk` counts as
    // wrapped — the user has demonstrated rtk awareness.
    if has_rtk_in_any_segment(cmd) {
        return None;
    }

    // Upfront eligibility gate. A command that isn't safe to blanket-wrap
    // (real subshell, leading backtick-exec, head token is a shell builtin)
    // isn't safe to hand to the external `rtk rewrite` either — RTK's own
    // rewriter can corrupt those forms (e.g. `(cargo build; cargo test)`
    // → `(cargo build; rtk cargo test)`). Skip both paths *and* the strict
    // deny path: a builtin like `cd` cannot be exec'd through `rtk`, so we
    // must not ask the agent to "reenvie como: rtk cd /tmp" — that would
    // fail. Builtins / subshells are silently allowed in every mode.
    if !should_blanket_prefix(cmd) {
        return None;
    }

    // --- Strict mode: deny so the UI surfaces the rule ---
    if mode == Mode::Strict {
        let trimmed = cmd.trim();
        let head: String = trimmed.chars().take(120).collect();
        let suffix = if trimmed.chars().count() > 120 { "..." } else { "" };
        return Some((
            Verdict::Deny {
                reason: format!(
                    "[bash_guard rtk] Comando sem prefixo `rtk` — Mustard exige rtk em todo Bash.\n\
                     Reenvie como: rtk {head}{suffix}"
                ),
            },
            "deny",
        ));
    }

    // --- Warn mode: specific path ---
    if let Some(rewritten) = rewriter(cmd) {
        // Strip any rtk "no hook installed" advisory before evaluating the
        // rewriter's output. rtk 0.34.1 emits this line on every invocation
        // when no Claude Code hook is registered; Mustard uses bash_guard
        // instead, so the advisory is pure noise. Filtering here covers both
        // the real subprocess path and the injected-closure path used by tests.
        let rewritten_clean = filter_rtk_noise(&rewritten);
        let rewritten = rewritten_clean.trim();
        if !rewritten.is_empty() && rewritten != cmd.trim() {
            return Some((
                Verdict::Rewrite {
                    tool_input: json!({ "command": rewritten }),
                },
                "specific",
            ));
        }
    }

    // --- Warn mode: blanket-prefix fallback (Golden Rule) ---
    // When RTK has no specific filter for `cmd`, prepend `rtk ` so that:
    //   a) all future RTK filters automatically apply retroactively, and
    //   b) every rewrite is captured in telemetry for coverage analysis.
    blanket_prefix(cmd).map(|prefixed| {
        (
            Verdict::Rewrite {
                tool_input: json!({ "command": prefixed }),
            },
            "blanket",
        )
    })
}

/// Shell builtins and keywords that must never be wrapped with `rtk`.
/// RTK would try to exec them as a binary and fail (exit 127).
const SHELL_BUILTINS: &[&str] = &[
    "cd", "pwd", "export", "set", "unset", "alias", "unalias", "source", ".",
    "eval", "exec", "exit", "return", ":", "read", "shift", "let", "local",
    "declare", "typeset", "readonly", "hash", "times", "umask", "wait",
    "getopts", "caller", "enable", "bind", "bg", "fg", "disown", "jobs",
    "kill", "command", "builtin", "type", "true", "false", "test", "[",
    "[[", "((", "if", "then", "else", "elif", "fi", "for", "do", "done",
    "while", "until", "case", "esac", "select", "function", "time", "coproc",
    "in",
];

/// Returns the slice of `s` after stripping any leading `VAR=value` env
/// assignments (tokens matching `[A-Za-z_][A-Za-z0-9_]*=…` followed by
/// whitespace). If no env assignments are present the original slice is
/// returned unchanged.
fn strip_env_prefix(s: &str) -> &str {
    let mut rest = s.trim_start();
    loop {
        // An env assignment token starts with an identifier character.
        let bytes = rest.as_bytes();
        if bytes.is_empty() {
            break;
        }
        let first = bytes[0] as char;
        if !(first.is_ascii_alphabetic() || first == '_') {
            break;
        }
        // Find the boundary of this whitespace-separated token.
        let token_end = rest
            .find(|c: char| c.is_ascii_whitespace())
            .unwrap_or(rest.len());
        let token = &rest[..token_end];
        // Must contain `=` to be an env assignment.
        if !token.contains('=') {
            break;
        }
        // The part before `=` must be a valid identifier.
        let eq_pos = token.find('=').unwrap_or(0); // safe: contains '=' confirmed above
        let name = &token[..eq_pos];
        let is_ident = !name.is_empty()
            && name
                .chars()
                .enumerate()
                .all(|(i, c)| {
                    if i == 0 {
                        c.is_ascii_alphabetic() || c == '_'
                    } else {
                        c.is_ascii_alphanumeric() || c == '_'
                    }
                });
        if !is_ident {
            break;
        }
        // Advance past this env token and any trailing whitespace.
        rest = rest[token_end..].trim_start();
    }
    rest
}

/// Returns `true` when it is safe to prepend `rtk ` to `cmd`.
///
/// Returns `false` when:
/// - `cmd` is empty.
/// - The first segment (after any `VAR=value` env assignments) is a real
///   subshell `(…)` or backtick exec `` `…` `` — those forms have no head
///   binary, so `rtk` has nothing to wrap.
/// - The first executable token is a shell builtin or keyword (RTK would
///   exit 127 trying to exec it).
///
/// A literal `(`, `` ` ``, or `$(` *inside an argument* (e.g.
/// `git log --grep="(scope)"`, `echo $(date)`) is fine — the shell expands
/// those before invoking `rtk`, which then execs the real binary.
fn should_blanket_prefix(cmd: &str) -> bool {
    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        return false;
    }
    // Strip compound operators (&&, ||, ;, |) and inspect only the first
    // segment. Prepending `rtk` before the whole pipeline is fine — the shell
    // expands `&&` / `||` after `rtk` exits. Mask quoted operators first so a
    // quoted argument containing `|`/`;` does not split the head off early.
    let masked = mask_quoted_operators(trimmed);
    let seg_end = masked.find(is_cmd_separator).unwrap_or(masked.len());
    let first_segment = trimmed[..seg_end].trim();
    // After stripping env assignments, inspect the head of the segment.
    let after_env = strip_env_prefix(first_segment);
    // A segment that *starts* with `(` or `` ` `` has no head binary for `rtk`
    // to wrap (real subshell / backtick exec). Skip.
    if after_env.starts_with('(') || after_env.starts_with('`') {
        return false;
    }
    // The executable token: a shell builtin/keyword cannot be exec'd by `rtk`.
    let executable = after_env.split_whitespace().next().unwrap_or("");
    if SHELL_BUILTINS.contains(&executable) {
        return false;
    }
    true
}

/// Builds the blanket-prefixed command string, handling `VAR=val` env
/// assignments that must remain before the shell expands the line.
///
/// For plain commands (`cargo run -p foo`) this returns `"rtk cargo run -p
/// foo"`. For env-prefixed commands (`RUST_LOG=debug cargo run`) the env
/// assignments are kept at the front and `rtk` is inserted before the
/// executable: `"RUST_LOG=debug rtk cargo run"`. This is the form that both
/// the shell and RTK accept — the shell sets the env vars before invoking
/// `rtk`, which then execs the real program.
///
/// Returns `None` when [`should_blanket_prefix`] rejects the command.
fn blanket_prefix(cmd: &str) -> Option<String> {
    if !should_blanket_prefix(cmd) {
        return None;
    }
    let trimmed = cmd.trim();
    // Detect how many leading env-assignment tokens precede the executable.
    let after_env = strip_env_prefix(trimmed);
    if after_env.len() == trimmed.len() {
        // No env prefix — simple case.
        Some(format!("rtk {trimmed}"))
    } else {
        // There are env assignments. Insert `rtk` between them and the rest.
        // env_part_len is derived from pointer arithmetic below; no separate var needed.
        // env_part includes the trailing space(s) stripped by strip_env_prefix,
        // so we re-take the original slice up to the start of after_env.
        let env_part = trimmed[..trimmed.len() - after_env.len()].trim_end();
        Some(format!("{env_part} rtk {after_env}"))
    }
}

/// Returns `true` when any `&&`/`||`/`;`/`|` segment of `cmd` has `rtk`
/// as its first executable token (after stripping `VAR=value` env
/// assignments).
///
/// Used as the "already wrapped" short-circuit for the rtk gate: when
/// the user has piped to or chained an `rtk`-prefixed stage (e.g.
/// `echo '{...}' | rtk mustard-rt run memory decision`), neither
/// denying nor blanket-prefixing is correct — the user has already
/// demonstrated rtk awareness and chosen where the wrap applies.
fn has_rtk_in_any_segment(cmd: &str) -> bool {
    // Split on the masked view so a quoted operator (e.g. `|` inside a Grep
    // pattern) does not create a phantom segment that hides a real `rtk` stage.
    // The head token of each segment is a command name, never quoted, so it is
    // identical in the masked and original strings.
    mask_quoted_operators(cmd)
        .split(is_cmd_separator)
        .any(|seg| {
            let after_env = strip_env_prefix(seg.trim());
            after_env.split_whitespace().next() == Some("rtk")
        })
}

#[cfg(test)]
thread_local! {
    /// In test builds, when set, this short-circuits `rtk_rewrite` to return `None`,
    /// isolating gate tests from the real `rtk` binary on PATH and from the
    /// strict-mode rtk gate (default since spec
    /// `2026-05-20-rtk-mandatory-everywhere`).
    ///
    /// **Side-effect warning for future authors:** any test that calls
    /// `verdict_for(...)` will have the rtk gate forced to `Mode::Off` — it will
    /// NEVER see a `Verdict::Rewrite` *or* a strict `Verdict::Deny` from the
    /// production `rtk_rewrite()` path. Tests that need to exercise the rewrite
    /// or strict-deny logic must call `rtk_rewrite_with` directly with an explicit
    /// `Mode` (see `rtk_rewrite_tests` below) or drive the binary as a subprocess
    /// (see `tests/rtk_rewrite_emission.rs`).
    pub(super) static RTK_REWRITE_TEST_OVERRIDE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

// ---------------------------------------------------------------------------
// review-gate — validate before `git commit`
// ---------------------------------------------------------------------------

/// Build timeout for the strict-mode build check (`BUILD_TIMEOUT_MS` in
/// `review-gate.js`): 5 minutes.
const BUILD_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// `\bgit\s+commit\b` — `git commit` anywhere in the command (tolerates an
/// `rtk` prefix). Mirrors `isGitCommit` in `review-gate.js`.
fn is_git_commit(cmd: &str) -> bool {
    let lower = cmd.to_ascii_lowercase();
    has_word_pair(&lower, "git", "commit")
}

/// `true` when a `git commit` stages its changes **as part of the commit** —
/// `-a`/`-am`/`--all` (all tracked) or an explicit `-- <pathspec>` separator.
///
/// For these forms the index is legitimately empty at `PreToolUse` time (the
/// staging happens inside the commit), so the "No staged changes detected"
/// advisory is a false positive — the commit will, in fact, record changes.
/// A plain `git commit` (relying on a pre-staged index) is NOT inline-staging,
/// so it still warns. Detection is conservative: a short-flag cluster is only
/// matched when it is all letters and contains `a` (so `-am` matches, `-m` and
/// the long `--amend` do not), avoiding a false suppression on `git commit -m`.
fn commit_stages_inline(cmd: &str) -> bool {
    cmd.split_whitespace().any(|tok| {
        tok == "--all"
            || tok == "--" // explicit pathspec separator: `git commit -- <paths>`
            || (tok.len() >= 2
                && tok.starts_with('-')
                && !tok.starts_with("--")
                && tok[1..].bytes().all(|b| b.is_ascii_alphabetic())
                && tok[1..].contains('a'))
    })
}

/// The `MUSTARD_COMMIT_GATE_MODE` mode for the commit gate.
///
/// Default is `warn` (retro-compat with `getCommitGateMode` in
/// `review-gate.js` — *not* the crate-wide strict default). An unrecognised
/// value also falls back to `warn`.
fn commit_gate_mode() -> Mode {
    std::env::var("MUSTARD_COMMIT_GATE_MODE")
        .ok()
        .and_then(|raw| Mode::parse(&raw))
        .unwrap_or(Mode::Warn)
}

/// `true` when the hook profile is `strict` — mirrors `isStrictMode()` in
/// `_lib/hook-env.js`. Used by `review-gate.js` to decide `deny` vs `allow`
/// in warn-mode.
fn is_strict_profile() -> bool {
    std::env::var("MUSTARD_HOOK_PROFILE")
        .is_ok_and(|v| v.trim().eq_ignore_ascii_case("strict"))
}

const SENSITIVE_EXT: &[&str] = &[
    ".env", ".pem", ".key", ".secret", ".p12", ".pfx", ".cer", ".crt",
];

/// `true` if a staged path matches a sensitive-file pattern. Mirrors the
/// `sensitiveFiles` filter in `review-gate.js`.
fn is_sensitive_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    if SENSITIVE_EXT.iter().any(|ext| normalized.ends_with(ext)) {
        return true;
    }
    // /credentials/i and /\.env\./i — substring matches.
    normalized.contains("credentials") || normalized.contains(".env.")
}

/// `true` if a staged path lives under a generated/build output directory.
/// Mirrors the `generated` filter in `review-gate.js`.
fn is_generated_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    ["dist/", "node_modules/", "obj/", "bin/"]
        .iter()
        .any(|prefix| normalized.starts_with(prefix))
}

/// Read `buildCommand` from the project-root `mustard.json` through the single
/// config owner. Fail-open to `None` when the file is absent or the key unset.
fn read_build_command(project_dir: &str) -> Option<String> {
    mustard_core::ProjectConfig::load(Path::new(project_dir)).build_command()
}

/// The outcome of a build run. `env_error` marks a fail-open condition
/// (`ENOENT` / timeout) — the JS port never blocks on those.
struct BuildOutcome {
    ok: bool,
    env_error: bool,
    output: String,
}

/// Run the staged build command under [`BUILD_TIMEOUT`].
///
/// `std::process::Command` has no native timeout, so the child is spawned and
/// waited on in a thread; if the wait does not finish inside the budget the
/// child is killed and the run is reported as an `env_error` (fail-open,
/// matching the JS `SIGTERM` branch). A spawn failure (`ENOENT`) is likewise
/// an `env_error`.
fn run_build(cmd: &str, project_dir: &str) -> BuildOutcome {
    // Shell out so the command string is interpreted the same way the JS
    // `execSync` does. `cmd /C` on Windows, `sh -c` elsewhere.
    let mut command = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", cmd]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", cmd]);
        c
    };
    command
        .current_dir(project_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match command.spawn() {
        Ok(child) => child,
        // Spawn failure (missing shell / ENOENT) → fail-open.
        Err(err) => {
            return BuildOutcome {
                ok: false,
                env_error: true,
                output: err.to_string(),
            };
        }
    };

    let (tx, rx) = std::sync::mpsc::channel();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    // The wait runs on a worker thread so the caller can apply a timeout.
    std::thread::spawn(move || {
        let status = child.wait();
        let _ = tx.send((status, child));
    });

    match rx.recv_timeout(BUILD_TIMEOUT) {
        Ok((Ok(status), _child)) => {
            let mut output = String::new();
            if let Some(mut out) = stdout {
                use std::io::Read;
                let _ = out.read_to_string(&mut output);
            }
            if let Some(mut err) = stderr {
                use std::io::Read;
                let _ = err.read_to_string(&mut output);
            }
            BuildOutcome {
                ok: status.success(),
                env_error: false,
                output: output.trim().to_string(),
            }
        }
        // Wait itself failed → fail-open.
        Ok((Err(err), _child)) => BuildOutcome {
            ok: false,
            env_error: true,
            output: err.to_string(),
        },
        // Timed out — kill the child and fail open (the JS `SIGTERM` branch).
        Err(_) => {
            if let Ok((_, mut child)) = rx.recv_timeout(Duration::from_millis(0)) {
                let _ = child.kill();
            }
            BuildOutcome {
                ok: false,
                env_error: true,
                output: format!("[timeout] {cmd}"),
            }
        }
    }
}

/// List staged file paths via `git diff --cached --name-only`.
///
/// Fail-open: `None` when git is unavailable or the command fails (the JS
/// `catch` branch — no staged-file warnings produced). Goes through
/// [`rtk_command`] so the subprocess follows Mustard's Golden Rule.
fn staged_files(project_dir: &str) -> Option<Vec<String>> {
    let output = rtk_command("git", &["diff", "--cached", "--name-only"])
        .current_dir(project_dir)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Some(
        text.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect(),
    )
}

/// List active pipeline names under `.claude/.pipeline-states/*.json`.
fn active_pipelines(project_dir: &str) -> Vec<String> {
    let Ok(paths) = ClaudePaths::for_project(Path::new(project_dir)) else {
        return Vec::new();
    };
    let dir = paths.pipeline_states_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    entries
        .filter_map(std::result::Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.strip_suffix(".json").map(str::to_string)
        })
        .collect()
}

/// Emit the `commit-gate.check` harness event. Best-effort — telemetry is
/// never load-bearing, so any failure is swallowed.
fn emit_commit_gate_event(
    project_dir: &str,
    session_id: Option<&str>,
    mode: Mode,
    warnings: usize,
    blocking_findings: &[&str],
    has_sensitive: bool,
    build_ok: Option<bool>,
) {
    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id.unwrap_or("unknown").to_string(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("review-gate".to_string()),
            actor_type: None,
        },
        event: "commit-gate.check".to_string(),
        payload: json!({
            "mode": mode.as_str(),
            "warnings": warnings,
            "blockingFindings": blocking_findings,
            "hasSensitive": has_sensitive,
            "buildOk": build_ok,
        }),
        spec: current_spec(project_dir),
    };
    // `commit-gate.check` is non-pipeline → per-spec NDJSON via W5 router.
    let _ = crate::shared::events::route::emit(project_dir, &event);
}

/// The `review-gate` gate: validate a `git commit` command.
///
/// `mode` is the commit-gate's **own** [`Mode`] (`MUSTARD_COMMIT_GATE_MODE`,
/// default `warn`), resolved by the caller — passing it in keeps the gate
/// testable without mutating process environment.
///
/// Returns `None` for every non-commit command and for `Mode::Off`.
/// Otherwise reproduces `review-gate.js` 1:1:
/// - strict mode + a blocking finding (staged secret / broken build) → `Deny`;
/// - any warnings → `Warn` (or `Deny` when the hook profile is `strict`);
/// - no warnings → `None` (pass).
// review_gate contains a single sequential logic block; splitting it would
// require threading many local variables through helper fns with no clarity gain.
#[allow(clippy::too_many_lines)]
fn review_gate(cmd: &str, ctx: &Ctx, mode: Mode) -> Option<Verdict> {
    // Mode `off` — skip entirely.
    if mode == Mode::Off {
        return None;
    }
    if !is_git_commit(cmd) {
        return None;
    }

    let project_dir = ctx.project_dir.as_str();
    let mut warnings: Vec<String> = Vec::new();
    // Strict-blocking findings: `secrets` or `build`.
    let mut blocking: Vec<(&'static str, String)> = Vec::new();
    let mut has_sensitive = false;

    // Check 1-4: staged changes — sensitive / generated / large.
    match staged_files(project_dir) {
        // A `git commit -a/-am` (or `commit -- <paths>`) stages its changes as
        // part of the commit, so an empty index here is expected — not a
        // missing-changes problem. Only a plain `git commit` relying on a
        // pre-staged index warns; an inline-staging commit falls through to the
        // (empty) file-scan arm below, which is a harmless no-op.
        Some(files) if files.is_empty() && !commit_stages_inline(cmd) => {
            warnings.push("No staged changes detected".to_string());
        }
        Some(files) => {
            let sensitive: Vec<&String> =
                files.iter().filter(|f| is_sensitive_path(f)).collect();
            if !sensitive.is_empty() {
                let list = sensitive
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let msg = format!("Sensitive files staged: {list}");
                warnings.push(msg.clone());
                blocking.push(("secrets", msg));
                has_sensitive = true;
            }
            let generated: Vec<&String> =
                files.iter().filter(|f| is_generated_path(f)).collect();
            if !generated.is_empty() {
                let list = generated
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                warnings.push(format!("Generated/build files staged: {list}"));
            }
            if files.len() > 30 {
                warnings.push(format!(
                    "Large commit: {} files staged. Consider splitting.",
                    files.len()
                ));
            }
        }
        // git unavailable — fail open, no staged warnings.
        None => {}
    }

    // Check 5: build integrity — strict mode only.
    let mut build_ok: Option<bool> = None;
    if mode == Mode::Strict {
        if let Some(build_cmd) = read_build_command(project_dir) {
            let result = run_build(&build_cmd, project_dir);
            if !result.ok && !result.env_error {
                build_ok = Some(false);
                let out = truncate(&result.output, 300);
                let suffix = if result.output.len() > 300 { "…" } else { "" };
                let msg = format!("Build broken: {out}{suffix}");
                warnings.push(msg.clone());
                blocking.push(("build", msg));
            } else if result.ok {
                build_ok = Some(true);
            }
            // env_error → fail-open: leave `build_ok` as `None`, no warning.
        }
    }

    // Check 6: active pipeline advisory.
    let pipelines = active_pipelines(project_dir);
    if !pipelines.is_empty() {
        warnings.push(format!(
            "Active pipeline(s): {}. Ensure changes match spec.",
            pipelines.join(", ")
        ));
    }

    // Emit the harness event (best-effort).
    let blocking_types: Vec<&str> = blocking.iter().map(|(t, _)| *t).collect();
    emit_commit_gate_event(
        project_dir,
        ctx_session_id(ctx),
        mode,
        warnings.len(),
        &blocking_types,
        has_sensitive,
        build_ok,
    );

    // Strict mode: block on real sensor failures.
    if mode == Mode::Strict && !blocking.is_empty() {
        let what = blocking
            .iter()
            .map(|(_, m)| m.as_str())
            .collect::<Vec<_>>()
            .join(" | ");
        return Some(Verdict::Deny {
            reason: format_gate_message(
                "Commit Gate",
                &what,
                "committing secrets or a broken build is unrecoverable",
                "unstage the flagged files / fix the build, or set MUSTARD_COMMIT_GATE_MODE=warn",
            ),
        });
    }

    // Warn mode (or strict with no blocking finding): advisory on warnings.
    if !warnings.is_empty() {
        let reason = format_gate_message(
            "Review Gate",
            &warnings.join(" | "),
            "these may not belong in the commit",
            "review the staged changes before committing",
        );
        // `review-gate.js`: `permissionDecision: isStrictMode() ? 'deny' : 'allow'`.
        return Some(if is_strict_profile() {
            Verdict::Deny { reason }
        } else {
            Verdict::Warn { message: reason }
        });
    }

    None
}

/// `Ctx` carries no session id today, so the commit-gate event uses a
/// placeholder. Kept as a helper so a future `Ctx` field is a one-line change.
fn ctx_session_id(_ctx: &Ctx) -> Option<&str> {
    None
}

// ---------------------------------------------------------------------------
// pr-detect — DORA telemetry on `gh pr` commands (PostToolUse(Bash))
// ---------------------------------------------------------------------------

/// Classify a command as a PR event. Mirrors `classify` in `pr-detect.js`:
/// a conservative match at the start of the token sequence, tolerating a
/// leading `rtk` wrapper.
fn classify_pr(command: &str) -> Option<&'static str> {
    let cleaned = command.trim();
    // Strip a leading `rtk ` wrapper (case-insensitive).
    let cleaned = if cleaned.len() >= 4 && cleaned[..4].eq_ignore_ascii_case("rtk ") {
        cleaned[4..].trim_start()
    } else {
        cleaned
    };
    let tokens: Vec<&str> = cleaned.split_whitespace().collect();
    if tokens.len() >= 3 && tokens[0].eq_ignore_ascii_case("gh") && tokens[1] == "pr" {
        match tokens[2] {
            "create" => return Some("pr.opened"),
            "merge" => return Some("pr.merged"),
            _ => {}
        }
    }
    None
}

/// The git branch via `git rev-parse --abbrev-ref HEAD`. Fail-open `None`.
fn detect_branch(project_dir: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(project_dir)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() { None } else { Some(branch) }
}

/// The most recently modified `.pipeline-states/*.json` (excluding
/// `*.metrics.json`), by mtime. Mirrors `detectMostRecentSpec` in
/// `pr-detect.js`. Fail-open `None`.
fn detect_recent_spec(project_dir: &str) -> Option<String> {
    let paths = ClaudePaths::for_project(Path::new(project_dir)).ok()?;
    let dir = paths.pipeline_states_dir();
    let entries = std::fs::read_dir(&dir).ok()?;
    let mut best: Option<(std::time::SystemTime, String)> = None;
    for entry in entries.filter_map(std::result::Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !std::path::Path::new(&name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json")) || name.ends_with(".metrics.json") {
            continue;
        }
        let Ok(mtime) = entry.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        if best.as_ref().is_none_or(|(t, _)| mtime > *t) {
            let spec = name.trim_end_matches(".json").to_string();
            best = Some((mtime, spec));
        }
    }
    best.map(|(_, spec)| spec)
}

/// `true` when the Bash tool reported a non-zero exit code. Mirrors the
/// `tool_response.exit_code` check in `pr-detect.js` — permissive: a missing
/// exit code is treated as success.
fn bash_failed(input: &HookInput) -> bool {
    input
        .raw
        .get("tool_response")
        .and_then(|r| r.get("exit_code"))
        .and_then(serde_json::Value::as_i64)
        .is_some_and(|code| code != 0)
}

/// Emit a `pr.opened` / `pr.merged` harness event. Best-effort telemetry.
fn emit_pr_event(
    project_dir: &str,
    session_id: Option<&str>,
    event: &str,
    command: &str,
) {
    let branch = detect_branch(project_dir);
    let spec = detect_recent_spec(project_dir);
    let command_field = if command.len() > 200 {
        format!("{}...", truncate(command, 200))
    } else {
        command.to_string()
    };
    let harness_event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id.unwrap_or("unknown").to_string(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("pr-detect".to_string()),
            actor_type: None,
        },
        event: event.to_string(),
        payload: json!({
            "branch": branch,
            "spec": spec,
            "command": command_field,
        }),
        spec: spec.clone(),
    };
    // `pr.detect` family events are non-pipeline → NDJSON via W5 router.
    let _ = crate::shared::events::route::emit(project_dir, &harness_event);
}

/// Truncate a string to `max` bytes (char-boundary safe).
fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

// ---------------------------------------------------------------------------
// Contract impls
// ---------------------------------------------------------------------------

impl BashCommandGate {
    /// Pull the `command` string out of a Bash tool input.
    fn command_of(input: &HookInput) -> Option<String> {
        input
            .tool_input
            .get("command")
            .and_then(|v| v.as_str())
            .map(str::to_string)
    }
}

impl Check for BashCommandGate {
    /// Run the four ported PreToolUse(Bash) gates in `bash-safety` →
    /// `bash-native-redirect` → `rtk-rewrite` → `review-gate` order.
    ///
    /// `bash-safety` is the non-negotiable gate (it has no mode in the JS —
    /// always strict). The first gate to reach a decisive verdict wins; gates
    /// that pass return `None` and the next runs. `review-gate` runs last and
    /// only fires on `git commit` — it computes its verdict with its own
    /// `MUSTARD_COMMIT_GATE_MODE`, independent of the module enforcement mode.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        // Only PreToolUse(Bash) is a gate.
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        if input.tool_name.as_deref() != Some("Bash") {
            return Ok(Verdict::Allow);
        }
        let Some(cmd) = Self::command_of(input) else {
            return Ok(Verdict::Allow);
        };

        // `bash-safety` is checked first: a dangerous command must be denied
        // regardless of any redirect/rewrite advice.
        if let Some(verdict) = bash_safety(&cmd) {
            return Ok(verdict);
        }
        // `bash-windows-redirect`: catch `> C:\...` style redirects before the
        // POSIX shell mangles them into junk filenames in the CWD.
        if let Some(verdict) = bash_windows_redirect(&cmd) {
            return Ok(verdict);
        }
        if let Some(verdict) = bash_native_redirect(&cmd) {
            return Ok(verdict);
        }
        if let Some((verdict, coverage)) = rtk_rewrite(&cmd) {
            // Emit `rtk-rewrite` telemetry before returning. Best-effort —
            // a store failure must never block the tool call.
            if let Verdict::Rewrite { ref tool_input } = verdict {
                let rewritten = tool_input
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let spec_slug = current_spec(&ctx.project_dir);
                // Emit a `pipeline.economy.savings.rtk-rewrite` NDJSON event
                // (W3A: SQLite savings writes → NDJSON). Tokens we did NOT have
                // to ship as a verbose Bash response because `rtk` summarised
                // the command. `RtkRewrite` bucket — `BashCommandGateBlock` is
                // reserved for deny verdicts so the dashboard can surface
                // "rewrites vs blocks" without conflating the two.
                {
                    let model = std::env::var("CLAUDE_MODEL").unwrap_or_default();
                    let saved = i64::from(estimator::estimate_input_tokens(&cmd, &model));
                    let saved = saved.max(1);
                    let savings_event = HarnessEvent {
                        v: SCHEMA_VERSION,
                        ts: now_iso8601(),
                        session_id: input.session_id.as_deref().unwrap_or("unknown").to_string(),
                        wave: 0,
                        actor: Actor {
                            kind: ActorKind::Hook,
                            id: Some("bash_guard".to_string()),
                            actor_type: None,
                        },
                        event: "pipeline.economy.savings.rtk-rewrite".to_string(),
                        payload: json!({
                            "source": "RtkRewrite",
                            "tokens_saved": saved,
                            "spec_id": spec_slug.clone(),
                            "wave_id": std::env::var("MUSTARD_ACTIVE_WAVE").ok().filter(|s| !s.is_empty()),
                            "agent_id": "bash_guard",
                        }),
                        spec: spec_slug.clone(),
                    };
                    let _ = crate::shared::events::route::emit(&ctx.project_dir, &savings_event);
                }
                // Harness event for downstream readers.
                let event = HarnessEvent {
                    v: SCHEMA_VERSION,
                    ts: now_iso8601(),
                    session_id: input.session_id.as_deref().unwrap_or("unknown").to_string(),
                    wave: 0,
                    actor: Actor {
                        kind: ActorKind::Hook,
                        id: Some("rtk-rewrite".to_string()),
                        actor_type: None,
                    },
                    event: "rtk-rewrite".to_string(),
                    payload: json!({
                        "event": "rtk-rewrite",
                        "tokens_affected": i64::try_from(cmd.len()).unwrap_or(i64::MAX),
                        "note": "rewritten via rtk",
                        "coverage": coverage,
                        "command_head": &cmd[..cmd.len().min(60)],
                        "rewritten_head": &rewritten[..rewritten.len().min(60)],
                    }),
                    spec: spec_slug,
                };
                // `rtk-rewrite` is non-pipeline → NDJSON via W5 router.
                let _ = crate::shared::events::route::emit(&ctx.project_dir, &event);
            }
            return Ok(verdict);
        }
        if let Some(verdict) = review_gate(&cmd, ctx, commit_gate_mode()) {
            return Ok(verdict);
        }
        Ok(Verdict::Allow)
    }
}

impl Observer for BashCommandGate {
    /// `pr-detect`: emit a DORA `pr.opened` / `pr.merged` event when a
    /// `gh pr create` / `gh pr merge` command succeeds on PostToolUse(Bash).
    ///
    /// Pure telemetry — never affects a verdict. Fail-open throughout.
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        if ctx.trigger != Some(Trigger::PostToolUse) {
            return;
        }
        if input.tool_name.as_deref() != Some("Bash") {
            return;
        }
        let Some(cmd) = Self::command_of(input) else {
            return;
        };
        let Some(event) = classify_pr(&cmd) else {
            return;
        };
        // Only emit on success — a non-zero exit code suppresses the event.
        if bash_failed(input) {
            return;
        }
        let session = input.session_id.as_deref();
        emit_pr_event(&ctx.project_dir, session, event, &cmd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn pre_bash(command: &str) -> (HookInput, Ctx) {
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": command }),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: String::new(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        (input, ctx)
    }

    /// Run the `Check` for a PreToolUse(Bash) command.
    fn verdict_for(command: &str) -> Verdict {
        RTK_REWRITE_TEST_OVERRIDE.with(|c| c.set(true));
        let (input, ctx) = pre_bash(command);
        BashCommandGate.evaluate(&input, &ctx).expect("check never errors")
    }

    // --- bash-safety parity (hooks.test.js "bash-safety.js") ----------------

    #[test]
    fn safety_blocks_rm_rf() {
        assert!(verdict_for("rm -rf /").is_blocking());
    }

    #[test]
    fn safety_blocks_force_push() {
        assert!(verdict_for("git push --force origin main").is_blocking());
    }

    #[test]
    fn safety_allows_normal_git() {
        // `git status` is safe — not blocked. With blanket-prefix active it
        // returns Rewrite (rtk wraps it) rather than bare Allow, so we test
        // non-blocking rather than exact equality.
        assert!(!verdict_for("git status").is_blocking());
    }

    #[test]
    fn safety_allows_dotnet_build() {
        // Same reasoning — blanket-prefix may produce Rewrite, not Allow.
        assert!(!verdict_for("dotnet build").is_blocking());
    }

    #[test]
    fn safety_blocks_reset_hard_and_mkfs() {
        assert!(verdict_for("git reset --hard HEAD~1").is_blocking());
        assert!(verdict_for("mkfs.ext4 /dev/sda").is_blocking());
    }

    #[test]
    fn safety_allows_force_with_lease() {
        // `--force-with-lease` is the safe form — not blocked by force-push.
        // Blanket-prefix may wrap it with `rtk`; we only assert non-blocking.
        assert!(!verdict_for("git push --force-with-lease origin dev").is_blocking());
    }

    // --- bash-native-redirect parity (hooks.test.js "bash-native-redirect.js")

    #[test]
    fn redirect_denies_simple_grep_suggesting_grep() {
        let v = verdict_for("grep -r pattern src/");
        match v {
            Verdict::Deny { reason } => assert!(reason.contains("Grep")),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn redirect_denies_cat_suggesting_read() {
        let v = verdict_for("cat src/main.ts");
        match v {
            Verdict::Deny { reason } => assert!(reason.contains("Read")),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn redirect_denies_ls_suggesting_glob() {
        let v = verdict_for("ls -la src/");
        match v {
            Verdict::Deny { reason } => assert!(reason.contains("Glob")),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn redirect_denies_bare_ls() {
        let v = verdict_for("ls");
        match v {
            Verdict::Deny { reason } => assert!(reason.contains("Glob"), "reason: {reason}"),
            other => panic!("expected Deny for bare `ls`, got {other:?}"),
        }
    }

    #[test]
    fn redirect_native_function_denies_bare_ls() {
        let v = bash_native_redirect("ls");
        match v {
            Some(Verdict::Deny { reason }) => assert!(reason.contains("Glob"), "reason: {reason}"),
            other => panic!("expected Some(Deny), got {other:?}"),
        }
    }

    #[test]
    fn redirect_denies_head_tail_find() {
        for cmd in ["head -20 file.txt", "tail -50 app.log", "find . -name '*.ts'"] {
            assert!(verdict_for(cmd).is_blocking(), "expected deny for: {cmd}");
        }
    }

    #[test]
    fn redirect_allows_piped_command() {
        // First segment `grep` is redirectable → Inject (advisory), not Deny.
        assert!(!verdict_for("grep foo bar.txt | wc -l").is_blocking());
    }

    #[test]
    fn redirect_allows_chained_command() {
        assert!(!verdict_for("grep foo bar.txt && echo found").is_blocking());
    }

    #[test]
    fn redirect_warns_on_piped_redirectable_first_segment() {
        let v = verdict_for("grep foo bar.txt | sort | uniq");
        match v {
            Verdict::Inject { context } => {
                assert!(context.contains("Grep"));
                assert!(context.contains("Native Tool Redirect"));
            }
            other => panic!("expected Inject advisory, got {other:?}"),
        }
    }

    #[test]
    fn redirect_allows_rtk_prefixed() {
        // `rtk grep` keeps passing — rtk filters grep and the binary exists.
        assert!(!verdict_for("rtk grep -r pattern src/").is_blocking());
    }

    /// `rtk rg` must be redirected to the Grep tool: rtk does not filter `rg`,
    /// so it execs the bare binary, which may be absent (Windows → exit 127).
    /// Regression for the 2026-05-21 pipeline failure.
    #[test]
    fn redirect_denies_rtk_rg_suggesting_grep() {
        let v = bash_native_redirect("rtk rg -n pattern src/");
        match v {
            Some(Verdict::Deny { reason }) => {
                assert!(reason.contains("Grep"), "reason: {reason}");
                assert!(reason.contains("rg"), "reason should name rg: {reason}");
            }
            other => panic!("expected Deny for `rtk rg`, got {other:?}"),
        }
    }

    /// The original failing command: `rtk rg` with an alternation pattern whose
    /// `|` is inside quotes (must NOT be read as a shell pipe) plus a `2>&1`
    /// stderr redirect. Must still reach the ripgrep redirect → Deny.
    #[test]
    fn redirect_denies_rtk_rg_with_quoted_pipe_pattern() {
        let v = bash_native_redirect(
            r#"rtk rg -n "emit-pipeline|emit-phase|pipeline\.status" src/ 2>&1"#,
        );
        match v {
            Some(Verdict::Deny { reason }) => assert!(reason.contains("Grep"), "reason: {reason}"),
            other => panic!("expected Deny for quoted-pipe `rtk rg`, got {other:?}"),
        }
    }

    /// A quoted operator must not be treated as a shell pipe: bare `grep` with
    /// an alternation pattern is a plain (non-piped) command → hard Deny, not
    /// the advisory Inject reserved for genuine pipelines.
    #[test]
    fn redirect_quoted_pipe_pattern_is_not_a_real_pipe() {
        let v = bash_native_redirect(r#"grep -n "foo|bar" src/"#);
        match v {
            Some(Verdict::Deny { reason }) => assert!(reason.contains("Grep"), "reason: {reason}"),
            other => panic!("expected hard Deny (quoted | is not a pipe), got {other:?}"),
        }
    }

    /// A genuine pipe is still detected (advisory Inject), even with a quoted
    /// operator earlier in the line.
    #[test]
    fn redirect_real_pipe_still_advisory() {
        let v = bash_native_redirect(r#"grep -n "foo|bar" src/ | sort"#);
        assert!(
            matches!(v, Some(Verdict::Inject { .. })),
            "real pipe must stay advisory, got {v:?}"
        );
    }

    #[test]
    fn redirect_allows_non_mapped_commands() {
        // Blanket-prefix wraps unknown commands — Rewrite, not Allow. Assert
        // non-blocking (the gate intent) rather than exact Verdict::Allow.
        assert!(!verdict_for("git status").is_blocking());
        assert!(!verdict_for("npm run build").is_blocking());
    }

    #[test]
    fn redirect_allows_sed_in_place() {
        assert!(!verdict_for("sed -i 's/old/new/g' file.txt").is_blocking());
    }

    #[test]
    fn redirect_allows_output_redirect() {
        assert!(!verdict_for("cat file.txt > output.txt").is_blocking());
    }

    #[test]
    fn redirect_handles_env_var_prefix() {
        assert!(verdict_for("NODE_ENV=test grep pattern file.txt").is_blocking());
    }

    #[test]
    fn redirect_strips_stderr_redirect_before_analysis() {
        assert!(verdict_for("grep pattern file 2>/dev/null").is_blocking());
    }

    #[test]
    fn redirect_denies_read_only_sed() {
        assert!(verdict_for("sed -n '1,5p' file.txt").is_blocking());
    }

    // --- bash-windows-redirect ---------------------------------------------
    //
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

    #[test]
    fn windows_redirect_gate_wins_over_native_redirect() {
        // End-to-end through evaluate: `cat ... > C:\...` would normally be
        // denied by bash-native-redirect (cat → Read), but the Windows-path
        // gate runs first with a more specific reason.
        let v = verdict_for("cat src/main.rs > C:\\Atiz\\dump.txt");
        match v {
            Verdict::Deny { reason } => assert!(
                reason.contains("bash-windows-redirect"),
                "expected windows-redirect reason first, got: {reason}"
            ),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    // --- gate routing -------------------------------------------------------

    #[test]
    fn non_bash_tool_allows() {
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: String::new(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        assert_eq!(
            BashCommandGate.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }

    #[test]
    fn non_pre_tool_use_trigger_allows() {
        // The gate only runs on PreToolUse — any other trigger self-allows.
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": "rm -rf /" }),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: String::new(),
            trigger: Some(Trigger::PostToolUse),
            workspace_root: None,
        };
        assert_eq!(
            BashCommandGate.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }

    // --- review-gate parity (harness-wave9.test.js, tests 7-9) --------------

    /// `review-gate` only fires on a `git commit` command.
    #[test]
    fn review_gate_detects_git_commit() {
        assert!(is_git_commit("git commit -m \"feat: x\""));
        assert!(is_git_commit("rtk git commit -m \"feat: x\""));
        assert!(!is_git_commit("git add ."));
        assert!(!is_git_commit("git push origin dev"));
    }

    /// Regression (#3): a commit that stages its own changes (`-a`/`-am`/`--all`,
    /// or `commit -- <paths>`) must NOT trip the "No staged changes" advisory —
    /// the index is legitimately empty at PreToolUse time. A plain `git commit`
    /// (and `--amend`) still relies on a pre-staged index, so it is not inline.
    #[test]
    fn commit_stages_inline_detects_self_staging_forms() {
        assert!(commit_stages_inline("git commit -am \"msg\""));
        assert!(commit_stages_inline("rtk git commit -a -m \"msg\""));
        assert!(commit_stages_inline("git commit --all -m x"));
        assert!(commit_stages_inline("git commit -- src/a.ts"));
        // Plain index-driven commits are NOT inline-staging.
        assert!(!commit_stages_inline("git commit -m \"msg\""));
        assert!(!commit_stages_inline("git commit"));
        assert!(!commit_stages_inline("git commit --amend -m x"));
        // `-m"attached"` is not an all-letters cluster → not misread as `-a`.
        assert!(!commit_stages_inline("git commit -m\"add auth\""));
    }

    /// A non-commit Bash command never triggers the review gate — non-blocking.
    /// (With blanket-prefix active these may return Rewrite rather than bare
    /// Allow — the gate's contract is "does not block", not "returns Allow".)
    #[test]
    fn review_gate_ignores_non_commit_commands() {
        assert!(!verdict_for("git status").is_blocking());
        assert!(!verdict_for("npm run build").is_blocking());
    }

    /// `Mode::Off` skips the gate entirely — even on a `git commit`.
    #[test]
    fn review_gate_off_mode_returns_none() {
        let ctx = Ctx {
            project_dir: String::new(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        assert_eq!(review_gate("git commit -m x", &ctx, Mode::Off), None);
    }

    /// With no git repo, the gate self-passes — git unavailable → no warnings.
    #[test]
    fn review_gate_fails_open_without_git_repo() {
        let dir = tempdir().unwrap();
        let ctx = Ctx {
            project_dir: dir.path().to_string_lossy().into_owned(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        // No `.git`, no `.pipeline-states` → no warnings → no verdict.
        assert_eq!(review_gate("git commit -m x", &ctx, Mode::Warn), None);
    }

    /// In a real git repo with a staged `.env`, the gate denies in strict mode
    /// (wave9 test 7) and only warns in warn mode (wave9 test 9).
    #[test]
    fn review_gate_strict_denies_staged_secret() {
        let dir = tempdir().unwrap();
        let repo = dir.path();
        // Skip gracefully if git is unavailable, mirroring the JS test.
        if Command::new("git")
            .args(["init"])
            .current_dir(repo)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| !s.success())
            .unwrap_or(true)
        {
            return;
        }
        std::fs::write(repo.join(".env"), "SECRET=abc123").unwrap();
        let _ = Command::new("git")
            .args(["add", ".env"])
            .current_dir(repo)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        let ctx = Ctx {
            project_dir: repo.to_string_lossy().into_owned(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        let warn = review_gate("git commit -m \"feat: x\"", &ctx, Mode::Warn);
        let strict = review_gate("git commit -m \"feat: x\"", &ctx, Mode::Strict);
        // Warn mode → non-blocking advisory; strict → blocking deny.
        assert!(
            matches!(warn, Some(Verdict::Warn { .. })),
            "warn-mode verdict: {warn:?}"
        );
        match strict {
            Some(Verdict::Deny { reason }) => {
                assert!(
                    reason.to_lowercase().contains("sensitive"),
                    "reason: {reason}"
                );
            }
            other => panic!("expected strict Deny, got {other:?}"),
        }
    }

    /// `format_gate_message` reproduces the `formatGateMessage` shape.
    #[test]
    fn gate_message_format_matches_js() {
        let msg = format_gate_message(
            "Review Gate",
            "Sensitive files staged: .env",
            "these may not belong in the commit",
            "review the staged changes before committing",
        );
        assert!(msg.starts_with("[Review Gate] "));
        assert!(msg.contains("Saída: "));
        assert!(msg.ends_with('.'));
    }

    // --- pr-detect parity (pr-detect.js) ------------------------------------

    /// `gh pr create` / `gh pr merge` classify to the right DORA events.
    #[test]
    fn pr_detect_classifies_pr_commands() {
        assert_eq!(classify_pr("gh pr create --fill"), Some("pr.opened"));
        assert_eq!(classify_pr("gh pr merge 42 --squash"), Some("pr.merged"));
        // Tolerates a leading `rtk` wrapper.
        assert_eq!(classify_pr("rtk gh pr create"), Some("pr.opened"));
    }

    /// A non-PR command classifies to nothing.
    #[test]
    fn pr_detect_ignores_non_pr_commands() {
        assert_eq!(classify_pr("gh pr view 42"), None);
        assert_eq!(classify_pr("git commit -m x"), None);
        assert_eq!(classify_pr("gh issue list"), None);
        assert_eq!(classify_pr("echo gh pr create"), None);
    }

    /// The `Observer` only emits on a successful PostToolUse(Bash) `gh pr`
    /// command — a non-zero `exit_code` suppresses it, and a non-PostToolUse
    /// trigger is a no-op. (Smoke test: `observe` is infallible.)
    #[test]
    fn pr_detect_observer_is_infallible() {
        let dir = tempdir().unwrap();
        let ctx = Ctx {
            project_dir: dir.path().to_string_lossy().into_owned(),
            trigger: Some(Trigger::PostToolUse),
            workspace_root: None,
        };
        let ok = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": "gh pr create --fill" }),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        };
        // Must not panic; emits an event to the temp project's harness log.
        BashCommandGate.observe(&ok, &ctx);

        let failed = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": "gh pr create --fill" }),
            hook_event_name: Some("PostToolUse".to_string()),
            raw: json!({ "tool_response": { "exit_code": 1 } }),
            ..HookInput::default()
        };
        assert!(bash_failed(&failed));
        // Failed command → observer is a no-op (no panic, nothing emitted).
        BashCommandGate.observe(&failed, &ctx);
    }

    /// The civil-date timestamp is well-formed (`YYYY-MM-DDThh:mm:ss.sssZ`).
    #[test]
    fn iso8601_timestamp_is_well_formed() {
        let ts = now_iso8601();
        assert_eq!(ts.len(), 24, "ts: {ts}");
        assert!(ts.ends_with('Z'));
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[10..11], "T");
    }

    // --- bash-safety regression: one test per BG rule ----------------------
    //
    // Each entry: (id, blocking_command, Option<safe_command>).
    // Asserts: (1) the blocking command is denied; (2) the deny reason contains
    // the rule id; (3) when a safe variant is supplied, it is allowed.

    #[test]
    fn safety_regression_all_bg_rules() {
        let cases: &[(&str, &str, Option<&str>)] = &[
            // BG01 — recursive force delete
            ("BG01", "rm -rf /tmp/work", None),
            // BG02 — force push (safe variant: --force-with-lease)
            ("BG02", "git push --force origin main", Some("git push --force-with-lease origin main")),
            // BG03 — git reset --hard
            ("BG03", "git reset --hard HEAD~1", None),
            // BG04 — git clean -f
            ("BG04", "git clean -fd", None),
            // BG05 — git checkout -- .
            ("BG05", "git checkout -- .", None),
            // BG06 — git restore .
            ("BG06", "git restore .", None),
            // BG07 — delete main/master branch
            ("BG07", "git branch -D main", None),
            // BG08 — chmod 777
            ("BG08", "chmod 777 /etc/passwd", None),
            // BG09 — mkfs
            ("BG09", "mkfs.ext4 /dev/sda1", None),
            // BG10 — dd if=
            ("BG10", "dd if=/dev/zero of=/dev/sda", None),
            // BG11 — format drive
            ("BG11", "format c:", None),
            // BG12 — shutdown
            ("BG12", "shutdown -h now", None),
            // BG13 — reboot
            ("BG13", "reboot", None),
        ];

        for (id, blocking, safe_opt) in cases {
            let verdict = verdict_for(blocking);
            match &verdict {
                Verdict::Deny { reason } => {
                    assert!(
                        reason.contains(id),
                        "rule {id}: deny reason does not contain id — reason: {reason}"
                    );
                }
                other => panic!("rule {id}: expected Deny for {blocking:?}, got {other:?}"),
            }

            if let Some(safe_cmd) = safe_opt {
                // The safe variant must not be blocked by bash-safety.
                // (It may still be blocked by a different gate — check only
                // that bash_safety itself passes it through.)
                let safe_verdict = bash_safety(safe_cmd);
                assert!(
                    safe_verdict.is_none(),
                    "rule {id}: bash_safety should allow safe variant {safe_cmd:?}, got {safe_verdict:?}"
                );
            }
        }
    }
}

// Behavioral tests for `rtk_rewrite` decision logic.
//
// Strategy: [`rtk_rewrite_with`] accepts a closure that stands in for the real
// `rtk rewrite` subprocess. Each test injects a purpose-built closure so the
// eight AC scenarios can be verified without requiring `rtk` on PATH and without
// touching process environment. The subprocess fail-open test (AC-7) calls
// `run_rtk_rewrite_subprocess_with_bin` directly with a fake binary name,
// avoiding `std::env::set_var` which is `unsafe` in edition 2024.

#[cfg(test)]
mod rtk_rewrite_tests {
    use super::*;

    // -----------------------------------------------------------------------
    // AC-1: Already rtk-prefixed → short-circuit, rewriter never called.
    // -----------------------------------------------------------------------

    #[test]
    fn rtk_rewrite_passes_through_when_rtk_prefixed() {
        // The panic closure must never be invoked when the command already
        // starts with `rtk` — the short-circuit returns `None` immediately.
        // Tested under `Mode::Warn` (historical behavior); `Mode::Strict`
        // also short-circuits via the same already-prefixed check.
        let result = rtk_rewrite_with(
            "rtk grep x",
            |_| panic!("should not be called"),
            Mode::Warn,
        );
        assert!(result.is_none(), "expected None for already-prefixed command");
    }

    // -----------------------------------------------------------------------
    // AC-2: Rewriter returns a changed string → Verdict::Rewrite with the
    //       rewritten command in tool_input["command"], coverage = "specific".
    // -----------------------------------------------------------------------

    #[test]
    fn rtk_rewrite_emits_updated_input_when_rewriter_returns_change() {
        let result = rtk_rewrite_with("grep -n x src/", |c| Some(format!("rtk {c}")), Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(
                    tool_input["command"],
                    "rtk grep -n x src/",
                    "tool_input[\"command\"] mismatch: {tool_input}"
                );
                assert_eq!(coverage, "specific", "expected specific coverage tag");
            }
            other => panic!("expected Verdict::Rewrite, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // AC-3: Rewriter returns None for a non-builtin command → blanket prefix
    //       kicks in and returns Verdict::Rewrite, coverage = "blanket".
    //       Builtins (e.g. `cd`) still return None — see
    //       rtk_rewrite_skips_shell_builtin_cd below.
    // -----------------------------------------------------------------------

    #[test]
    fn rtk_rewrite_blanket_when_rewriter_returns_none_for_non_builtin() {
        let result = rtk_rewrite_with("grep -n x src/", |_| None, Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(
                    tool_input["command"],
                    "rtk grep -n x src/",
                    "expected blanket-prefixed command; got: {tool_input}"
                );
                assert_eq!(coverage, "blanket", "expected blanket coverage tag");
            }
            other => panic!("expected blanket Verdict::Rewrite, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // AC-4: Rewriter returns the same string → specific path skipped;
    //       blanket path fires for non-builtins, returning Rewrite.
    // -----------------------------------------------------------------------

    #[test]
    fn rtk_rewrite_blanket_when_rewriter_returns_identical() {
        let result = rtk_rewrite_with("grep -n x", |_| Some("grep -n x".to_string()), Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(
                    tool_input["command"],
                    "rtk grep -n x",
                    "expected blanket-prefixed command; got: {tool_input}"
                );
                assert_eq!(coverage, "blanket");
            }
            other => panic!("expected blanket Verdict::Rewrite, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // AC-5: Rewriter returns empty string → specific path skipped;
    //       blanket path fires for non-builtins.
    // -----------------------------------------------------------------------

    #[test]
    fn rtk_rewrite_blanket_when_rewriter_returns_empty() {
        let result = rtk_rewrite_with("grep -n x src/", |_| Some(String::new()), Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(tool_input["command"], "rtk grep -n x src/");
                assert_eq!(coverage, "blanket");
            }
            other => panic!("expected blanket Verdict::Rewrite, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // AC-6: Rewriter returns whitespace-only → specific path skipped;
    //       blanket path fires for non-builtins.
    // -----------------------------------------------------------------------

    #[test]
    fn rtk_rewrite_blanket_when_rewriter_returns_whitespace_only() {
        let result = rtk_rewrite_with("grep -n x src/", |_| Some("   \n  ".to_string()), Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(tool_input["command"], "rtk grep -n x src/");
                assert_eq!(coverage, "blanket");
            }
            other => panic!("expected blanket Verdict::Rewrite, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // AC-7 (closure path): Trailing newline is stripped from the rewritten
    //       command before it is placed in tool_input["command"].
    // -----------------------------------------------------------------------

    #[test]
    fn rtk_rewrite_strips_trailing_whitespace() {
        let result = rtk_rewrite_with("grep x", |_| Some("rtk grep x\n".to_string()), Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(
                    tool_input["command"],
                    "rtk grep x",
                    "trailing newline must be stripped; got: {tool_input}"
                );
                assert_eq!(coverage, "specific");
            }
            other => panic!("expected Verdict::Rewrite, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // AC-7 (subprocess path): When the binary name is a nonexistent executable,
    //       run_rtk_rewrite_subprocess_with_bin must return None (fail-open).
    //
    // Uses the with_bin helper directly so no process environment mutation is
    // needed — `std::env::set_var` is `unsafe` in edition 2024, and
    // `#![forbid(unsafe_code)]` prevents using an `unsafe` block.
    // -----------------------------------------------------------------------

    #[test]
    fn rtk_rewrite_fail_open() {
        // Invoke the subprocess layer with a binary name that cannot exist on
        // any PATH — the spawn must fail and the function must return None.
        let result = run_rtk_rewrite_subprocess_with_bin("grep x", "__not_a_real_binary_xyzzy__");
        assert!(
            result.is_none(),
            "expected None when subprocess binary does not exist; got {result:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Blanket-prefix Golden Rule tests (new AC set)
    // -----------------------------------------------------------------------

    // BL-1: Unknown command with no specific RTK filter → blanket prefix.
    #[test]
    fn rtk_rewrite_blanket_prefixes_unknown_command() {
        let result = rtk_rewrite_with("cargo run -p foo", |_| None, Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(tool_input["command"], "rtk cargo run -p foo");
                assert_eq!(coverage, "blanket");
            }
            other => panic!("expected blanket Verdict::Rewrite, got {other:?}"),
        }
    }

    // BL-2: `node` command with no specific RTK filter → blanket prefix.
    #[test]
    fn rtk_rewrite_blanket_prefixes_node_command() {
        let result = rtk_rewrite_with(r#"node -e "42""#, |_| None, Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(tool_input["command"], r#"rtk node -e "42""#);
                assert_eq!(coverage, "blanket");
            }
            other => panic!("expected blanket Verdict::Rewrite, got {other:?}"),
        }
    }

    // BL-3: Env-prefixed command — `rtk` is inserted AFTER env assignments so
    //       the shell sets the variable before invoking RTK.
    //       `RUST_LOG=debug rtk cargo run` is the correct form;
    //       `rtk RUST_LOG=debug cargo run` would fail (RTK tries to exec
    //       `RUST_LOG=debug` as a binary name).
    #[test]
    fn rtk_rewrite_blanket_with_env_prefix() {
        let result = rtk_rewrite_with("RUST_LOG=debug cargo run", |_| None, Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(
                    tool_input["command"],
                    "RUST_LOG=debug rtk cargo run",
                    "rtk must be inserted after env assignments, not before them"
                );
                assert_eq!(coverage, "blanket");
            }
            other => panic!("expected blanket Verdict::Rewrite, got {other:?}"),
        }
    }

    // BL-4: Shell builtin `cd` → blanket must be suppressed (RTK cannot exec
    //       a shell builtin; it would exit 127 with "Binary 'cd' not found").
    #[test]
    fn rtk_rewrite_skips_shell_builtin_cd() {
        let result = rtk_rewrite_with("cd /tmp", |_| None, Mode::Warn);
        assert!(
            result.is_none(),
            "expected None for shell builtin 'cd'; got {result:?}"
        );
    }

    // BL-5: Env prefix before a builtin — env assignments do not change the
    //       fact that the executable token is a builtin.
    #[test]
    fn rtk_rewrite_skips_shell_builtin_with_env() {
        let result = rtk_rewrite_with("FOO=bar cd /tmp", |_| None, Mode::Warn);
        assert!(
            result.is_none(),
            "expected None for env-prefixed builtin 'cd'; got {result:?}"
        );
    }

    // BL-6: Subshell expression `(cmd)` — blanket-prefix is ambiguous here;
    //       `rtk (cargo build; cargo test)` is not valid shell.
    #[test]
    fn rtk_rewrite_skips_subshell() {
        let result = rtk_rewrite_with("(cargo build; cargo test)", |_| None, Mode::Warn);
        assert!(
            result.is_none(),
            "expected None for subshell expression; got {result:?}"
        );
    }

    // BL-7: Command substitution mid-command (`echo $(date)`) — the head
    //       binary is `echo`, and the shell expands `$(date)` BEFORE running
    //       `rtk`, so blanket-prefix is safe (and desirable for token savings).
    #[test]
    fn rtk_rewrite_blanket_with_command_substitution_in_arg() {
        let result = rtk_rewrite_with("echo $(date)", |_| None, Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(tool_input["command"], "rtk echo $(date)");
                assert_eq!(coverage, "blanket");
            }
            other => panic!("expected blanket Verdict::Rewrite, got {other:?}"),
        }
    }

    // BL-7b: A real backtick exec at the start of the command has no head
    //        binary for `rtk` to wrap, so it must still be skipped.
    #[test]
    fn rtk_rewrite_skips_leading_backtick_exec() {
        let result = rtk_rewrite_with("`echo hi`", |_| None, Mode::Warn);
        assert!(
            result.is_none(),
            "expected None for leading backtick exec; got {result:?}"
        );
    }

    // BL-7c: A literal `(` *inside an argument* is fine — the shell sees it as
    //        text, not a subshell. Blanket must still wrap the head binary.
    #[test]
    fn rtk_rewrite_blanket_with_paren_inside_arg() {
        let result = rtk_rewrite_with(r"git log --grep=(scope)", |_| None, Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(tool_input["command"], r"rtk git log --grep=(scope)");
                assert_eq!(coverage, "blanket");
            }
            other => panic!("expected blanket Verdict::Rewrite for paren-in-arg, got {other:?}"),
        }
    }

    // BL-7d: Backtick inside an argument (`echo \`date\``) — the head binary
    //        is `echo`. Wrap.
    #[test]
    fn rtk_rewrite_blanket_with_backtick_inside_arg() {
        let result = rtk_rewrite_with("echo `date`", |_| None, Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(tool_input["command"], "rtk echo `date`");
                assert_eq!(coverage, "blanket");
            }
            other => panic!("expected blanket Verdict::Rewrite for backtick-in-arg, got {other:?}"),
        }
    }

    // BL-8: When the specific rewriter returns a real rewrite, the specific
    //       path wins — no blanket path is attempted.
    #[test]
    fn rtk_rewrite_specific_takes_precedence_over_blanket() {
        let result = rtk_rewrite_with("cargo build", |_| Some("rtk cargo build".to_string()), Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(tool_input["command"], "rtk cargo build");
                assert_eq!(coverage, "specific", "specific rewriter must win over blanket");
            }
            other => panic!("expected specific Verdict::Rewrite, got {other:?}"),
        }
    }

    // BL-9: Compound command with `&&` — first segment token (`cargo`) is not
    //       a builtin so blanket prefix applies to the whole string.
    //       The shell expands `&&` after RTK exits, so `rtk cargo run &&
    //       cargo test` correctly runs `cargo test` in the same shell context.
    #[test]
    fn rtk_rewrite_compound_blanket_uses_first_segment_check() {
        // RTK wraps only the first segment's executable; the `&& cargo test`
        // tail is carried along unchanged. The shell parent handles `&&`.
        let result = rtk_rewrite_with("cargo run && cargo test", |_| None, Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(tool_input["command"], "rtk cargo run && cargo test");
                assert_eq!(coverage, "blanket");
            }
            other => panic!("expected blanket Verdict::Rewrite for compound cmd, got {other:?}"),
        }
    }

    // BL-10: Already-prefixed command → short-circuit returns None immediately
    //        (rewriter closure is never invoked).
    #[test]
    fn rtk_rewrite_already_prefixed_no_op() {
        let result = rtk_rewrite_with("rtk cargo run", |_| panic!("should not be called"), Mode::Warn);
        assert!(
            result.is_none(),
            "expected None for already-prefixed command; got {result:?}"
        );
    }

    /// Bug-fix regression (2026-05-20): a pipeline whose first stage is not
    /// `rtk` but whose second stage IS `rtk` must short-circuit, never deny.
    #[test]
    fn rtk_rewrite_pipe_with_rtk_in_second_segment_no_op() {
        let cmd = "echo '{\"type\":\"decision\"}' | rtk mustard-rt run memory decision";
        let result = rtk_rewrite_with(cmd, |_| panic!("should not be called"), Mode::Strict);
        assert!(result.is_none(), "rtk in second pipe segment must short-circuit, got {result:?}");
    }

    /// Compound command: first stage prefixed with rtk, second not.
    /// Either segment having rtk counts as wrapped — do not deny.
    #[test]
    fn rtk_rewrite_compound_with_rtk_first_no_op() {
        let cmd = "rtk cargo build && cargo test";
        let result = rtk_rewrite_with(cmd, |_| panic!("should not be called"), Mode::Strict);
        assert!(result.is_none(), "rtk in first compound segment must short-circuit, got {result:?}");
    }

    /// Bug-fix regression (2026-05-21): a multi-line `command` string whose
    /// first line is a non-`rtk` sanity command (`echo …`) but whose second
    /// line IS `rtk`-prefixed must short-circuit. A newline separates shell
    /// commands exactly like `;`, so the later `rtk` stage counts as wrapped —
    /// the gate must not deny the whole multi-line command.
    #[test]
    fn rtk_rewrite_newline_with_rtk_on_second_line_no_op() {
        let cmd = "echo \"--- sanity ---\"\nrtk mustard-rt run emit-pipeline --help";
        let result = rtk_rewrite_with(cmd, |_| panic!("should not be called"), Mode::Strict);
        assert!(
            result.is_none(),
            "rtk on a later line must short-circuit, got {result:?}"
        );
    }

    /// `\r\n` (Windows) line endings split the same way as `\n`.
    #[test]
    fn rtk_rewrite_crlf_with_rtk_on_second_line_no_op() {
        let cmd = "echo sanity\r\nrtk mustard-rt run emit-pipeline --help";
        let result = rtk_rewrite_with(cmd, |_| panic!("should not be called"), Mode::Strict);
        assert!(
            result.is_none(),
            "rtk on a later CRLF line must short-circuit, got {result:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Gate-mode tests (spec: 2026-05-20-rtk-mandatory-everywhere)
    //
    // The gate-mode parameter is passed in directly so tests do not need to
    // mutate `MUSTARD_RTK_GATE_MODE` (`std::env::set_var` is `unsafe` under
    // edition 2024, and the crate forbids `unsafe`).
    // -----------------------------------------------------------------------

    /// `Mode::Strict` on a non-prefixed, eligible command → `Verdict::Deny`
    /// carrying the operator-facing reason. The rewriter closure must not be
    /// invoked — strict mode short-circuits before the specific path.
    #[test]
    fn rtk_strict_denies_unprefixed() {
        let result = rtk_rewrite_with(
            "grep -n x src/",
            |_| panic!("rewriter must not be called in strict mode"),
            Mode::Strict,
        );
        match result {
            Some((Verdict::Deny { reason }, coverage)) => {
                assert_eq!(coverage, "deny", "expected coverage tag 'deny'");
                assert!(
                    reason.contains("Mustard exige rtk"),
                    "reason should explain the rule; got: {reason}"
                );
                assert!(
                    reason.contains("rtk grep -n x src/"),
                    "reason should suggest the rtk-prefixed FULL command (not just the head token); got: {reason}"
                );
            }
            other => panic!("expected Verdict::Deny in strict mode, got {other:?}"),
        }
    }

    /// `Mode::Warn` preserves the historical rewrite behavior — a non-prefixed
    /// command becomes a `Verdict::Rewrite` with `coverage = "blanket"`.
    /// Regression guard for the spec's opt-out path.
    #[test]
    fn rtk_warn_rewrites_unprefixed() {
        let result = rtk_rewrite_with("grep -n x src/", |_| None, Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(tool_input["command"], "rtk grep -n x src/");
                assert_eq!(coverage, "blanket");
            }
            other => panic!("expected Verdict::Rewrite in warn mode, got {other:?}"),
        }
    }

    /// `Mode::Off` disables the gate entirely — the rewriter closure must not
    /// be invoked and the result must be `None` (Allow at the caller).
    #[test]
    fn rtk_off_allows_unprefixed() {
        let result = rtk_rewrite_with(
            "grep -n x src/",
            |_| panic!("rewriter must not be called in off mode"),
            Mode::Off,
        );
        assert!(
            result.is_none(),
            "expected None (Allow) in off mode; got {result:?}"
        );
    }

    // -----------------------------------------------------------------------
    // filter_rtk_noise — spec 2026-05-26-rtk-quiet-hook-warning
    //
    // rtk 0.34.1 emits "[rtk] /!\ No hook installed — run `rtk init -g`…"
    // on every invocation when no Claude Code hook is registered. Since
    // Mustard uses bash_guard (not rtk hook) for redirection, the line is
    // pure noise. The filter must strip it and leave all other output intact.
    // -----------------------------------------------------------------------

    /// Sole rtk-noise line → empty string after filtering.
    #[test]
    fn filter_rtk_noise_removes_advisory_line() {
        let input = "[rtk] /!\\ No hook installed — run `rtk init -g` for automatic token savings\n";
        let result = filter_rtk_noise(input);
        assert!(
            !result.contains("No hook installed"),
            "advisory must be removed; got: {result:?}"
        );
    }

    /// Mixed output: noise line + valid rewritten command → only command remains.
    #[test]
    fn filter_rtk_noise_keeps_other_lines() {
        let input = "[rtk] /!\\ No hook installed — run `rtk init -g` for automatic token savings\nrtk cargo build\n";
        let result = filter_rtk_noise(input);
        assert!(
            !result.contains("No hook installed"),
            "advisory must be removed; got: {result:?}"
        );
        assert!(
            result.contains("rtk cargo build"),
            "command line must survive; got: {result:?}"
        );
    }

    /// Output with no noise line → returned unchanged.
    #[test]
    fn filter_rtk_noise_passthrough_when_no_noise() {
        let input = "rtk git status\n";
        let result = filter_rtk_noise(input);
        assert_eq!(result, input, "clean output must pass through unchanged");
    }

    /// Multiple lines including the noise prefix: only that line is dropped.
    #[test]
    fn filter_rtk_noise_drops_only_noise_line() {
        let other = "[rtk] some other rtk annotation\n";
        let noise = "[rtk] /!\\ No hook installed — run `rtk init -g` for automatic token savings\n";
        let cmd   = "rtk cargo test\n";
        let input = format!("{other}{noise}{cmd}");
        let result = filter_rtk_noise(&input);
        assert!(!result.contains("No hook installed"), "noise must be gone; got: {result:?}");
        assert!(result.contains("[rtk] some other rtk annotation"), "other [rtk] line must survive; got: {result:?}");
        assert!(result.contains("rtk cargo test"), "command must survive; got: {result:?}");
    }

    /// Subprocess path: when the rewriter returns only the noise advisory,
    /// filtering leaves the output empty → specific path skipped → blanket
    /// fires. The final rewritten command must not contain the noise text.
    #[test]
    fn filter_rtk_noise_noise_only_falls_through_to_blanket() {
        let noise_only = "[rtk] /!\\ No hook installed — run `rtk init -g` for automatic token savings\n";
        let result = rtk_rewrite_with(
            "cargo build",
            |_| Some(noise_only.to_string()),
            Mode::Warn,
        );
        // After filtering the specific output is empty → blanket fires.
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                let cmd = tool_input["command"].as_str().unwrap_or("");
                assert!(
                    !cmd.contains("No hook installed"),
                    "noise must not appear in the rewritten command; got: {cmd:?}"
                );
                assert_eq!(coverage, "blanket", "noise-only specific path must fall through to blanket");
            }
            other => panic!("expected Verdict::Rewrite (blanket fallback), got {other:?}"),
        }
    }
}
