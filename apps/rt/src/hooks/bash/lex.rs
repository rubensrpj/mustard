//! Shell-command lexical helpers shared by the Bash gate family.
//!
//! One concern: reading a raw Bash `command` string — word boundaries, quoted
//! spans, segment separators, `rtk` prefixes. No verdicts are produced here;
//! the sibling gates (`safety`, `native_redirect`, `rtk_rewrite`,
//! `review_gate`, `pr_detect`) call these helpers directly.

use mustard_core::domain::text::{Boundaries, WordChars};

/// The word boundaries of the command guard, for
/// [`mustard_core::domain::text::has_word_sequence`]: a letter or digit is a
/// word char and `_` is not (`git_push` has a boundary before `push`). With
/// two words it is the `\bA\s+B\b` shape of `bash-safety.js`.
pub(super) const SHELL_WORDS: Boundaries =
    Boundaries { word_chars: WordChars::Alphanumeric, left: true, right: true };

/// Only the left boundary — the `\bneedle` shape of the standalone-word rules
/// (`mkfs`, `shutdown`, …).
pub(super) const SHELL_WORD_START: Boundaries = Boundaries { right: false, ..SHELL_WORDS };

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

/// Byte offset where one `VAR=value` token ends — whitespace, but never
/// whitespace that sits INSIDE a command substitution, a backtick pair or a
/// quoted value.
///
/// Bash ends a word at an unquoted space; `$( … )`, `` ` … ` `` and `'…'`/`"…"`
/// suspend that. Reading the token with a plain "find the first space" therefore
/// cuts `D=$(mktemp -d)` in half, and every offset derived from it points into
/// the middle of a substitution. Nesting is counted, not merely detected, so
/// `$(a $(b c))` closes on its own parenthesis.
pub(super) fn env_token_end(rest: &str) -> usize {
    let mut depth = 0usize;
    let mut backtick = false;
    let mut quote: Option<char> = None;
    let mut prev = '\0';
    for (i, c) in rest.char_indices() {
        let escaped = prev == '\\';
        prev = if escaped { '\0' } else { c };
        if escaped {
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' => quote = Some(c),
            '`' => backtick = !backtick,
            '(' if rest[..i].ends_with('$') => depth += 1,
            ')' if depth > 0 => depth -= 1,
            c if c.is_ascii_whitespace() && depth == 0 && !backtick => return i,
            _ => {}
        }
    }
    rest.len()
}

/// Returns the slice of `s` after stripping any leading `VAR=value` env
/// assignments (tokens matching `[A-Za-z_][A-Za-z0-9_]*=…` followed by
/// whitespace). If no env assignments are present the original slice is
/// returned unchanged.
pub(super) fn strip_env_prefix(s: &str) -> &str {
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
        // Find the boundary of this token. Whitespace ends it — EXCEPT inside a
        // command substitution or a quoted value, where a space is part of the
        // value and not a boundary.
        //
        // **Why this matters, measured in the field on 2026-08-20.** A value
        // like `D=$(mktemp -d)` was cut at the space, so the token was read as
        // `D=$(mktemp` and the insertion offset landed INSIDE the substitution:
        // the rewriter emitted `D=$(mktemp rtk -d)`, which fails with
        // "too few X's in template 'rtk'" and leaves `D` EMPTY. Every script
        // that then did `cd "$D"` or `git -C "$D" init` ran in the CURRENT
        // directory instead — three separate agents corrupted the operator's
        // repository that way in one session, one of them overwriting the
        // project's `mustard.json`, another rewriting its git identity.
        //
        // A rewriter is allowed to be unhelpful; it is never allowed to change
        // what a command MEANS.
        let token_end = env_token_end(rest);
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
