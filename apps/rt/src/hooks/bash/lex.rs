//! Shell-command lexical helpers shared by the Bash gate family.
//!
//! One concern: reading a raw Bash `command` string — word boundaries, quoted
//! spans, segment separators, `rtk` prefixes. No verdicts are produced here;
//! the sibling gates (`safety`, `native_redirect`, `rtk_rewrite`,
//! `review_gate`, `pr_detect`) call these helpers directly.

/// Truncate a string to `max` bytes (char-boundary safe).
pub(super) fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Whitespace-tolerant "word A followed by word B" check on a lowercased
/// command. Mirrors the `\bA\s+B\b`-style regexes in `bash-safety.js`.
pub(super) fn has_word_pair(cmd: &str, a: &str, b: &str) -> bool {
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
pub(super) fn has_word(cmd: &str, needle: &str) -> bool {
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

/// `true` if the command's token sequence *ends with* `seq` (trailing
/// whitespace already removed by `split_whitespace`). Mirrors the `…\s*$`
/// anchored regexes for `git checkout -- .` and `git restore .`.
pub(super) fn ends_with_token_seq(cmd: &str, seq: &[&str]) -> bool {
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    tokens.len() >= seq.len() && &tokens[tokens.len() - seq.len()..] == seq
}

/// The whitespace-separated tokens that appear *after* the first occurrence of
/// `anchor` as a word. Empty when `anchor` is absent.
pub(super) fn split_after<'a>(cmd: &'a str, anchor: &str) -> Vec<&'a str> {
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    if let Some(pos) = tokens.iter().position(|t| *t == anchor) {
        tokens[pos + 1..].to_vec()
    } else {
        Vec::new()
    }
}

/// Replace shell metacharacters that appear *inside single/double quotes* with
/// spaces, leaving everything else (including the quote chars and the byte
/// length) intact. Used so that a quoted argument like a Grep alternation
/// pattern (`"emit-pipeline|emit-phase"`) is not mistaken for a real shell
/// pipe by the operator and segment scans. Only single ASCII operator bytes
/// are swapped for a single ASCII space, so the result is always valid UTF-8
/// and byte-aligned with the input.
pub(super) fn mask_quoted_operators(cmd: &str) -> String {
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

/// `true` when `c` separates one shell command from the next: `&`, `|`, `;`,
/// or a newline. Newlines matter because the Bash tool routinely receives
/// multi-line `command` strings (a sanity `echo` on line 1, the real `rtk …`
/// on line 2); bash treats the line break exactly like `;`, so the segment
/// splitters must too — otherwise an `rtk`-prefixed later line is invisible to
/// the "already wrapped" short-circuit and the gate wrongly denies the whole
/// command.
pub(super) fn is_cmd_separator(c: char) -> bool {
    c == '&' || c == '|' || c == ';' || c == '\n' || c == '\r'
}

/// Strip a single leading `rtk ` wrapper token, returning the rest. When `cmd`
/// is not `rtk`-prefixed it is returned unchanged.
pub(super) fn strip_leading_rtk(cmd: &str) -> &str {
    let trimmed = cmd.trim_start();
    if let Some(rest) = trimmed.strip_prefix("rtk") {
        if rest.starts_with(char::is_whitespace) {
            return rest.trim_start();
        }
    }
    cmd
}
