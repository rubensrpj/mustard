//! The crate's ONE glob matcher.
//!
//! `*` matches within a path segment (never crosses `/`), `**` matches any
//! run including `/`. Two readers ask this question about the same declared
//! pattern — the boundary gate deciding whether an edit falls inside a spec's
//! `## Files`, and the wave traceability pass deciding whether a DECLARED glob
//! covers a path a criterion inspects — and a second matcher is how those two
//! readers would disagree about what a wave declared. It lives here, not in
//! either consumer, so neither layer imports the other for it (found in
//! review, 2026-07-30: the command layer was importing it from
//! `hooks::write::boundary_gate`, inverting the usual dependency direction).

/// Match `r` against a glob `p` (`*` = one path segment, `**` = anything).
///
/// Regex-free: the glob is walked as literal/`*`/`**` tokens by a recursive
/// segment matcher — for the patterns specs use this is the simplest shape.
pub(crate) fn glob_match(r: &str, p: &str) -> bool {
    glob_match_at(r.as_bytes(), p.as_bytes())
}

/// Recursive byte-wise glob matcher. `**` matches any run (incl. `/`); `*`
/// matches any run *not* containing `/`.
fn glob_match_at(text: &[u8], pat: &[u8]) -> bool {
    if pat.is_empty() {
        return text.is_empty();
    }
    if pat.starts_with(b"**") {
        let rest = &pat[2..];
        // `**` consumes zero-or-more of anything.
        let mut i = 0;
        loop {
            if glob_match_at(&text[i..], rest) {
                return true;
            }
            if i >= text.len() {
                return false;
            }
            i += 1;
        }
    }
    if pat[0] == b'*' {
        let rest = &pat[1..];
        // `*` consumes zero-or-more non-`/`.
        let mut i = 0;
        loop {
            if glob_match_at(&text[i..], rest) {
                return true;
            }
            if i >= text.len() || text[i] == b'/' {
                return false;
            }
            i += 1;
        }
    }
    if !text.is_empty() && text[0] == pat[0] {
        return glob_match_at(&text[1..], &pat[1..]);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one semantic distinction the two consumers rely on: `*` stays
    /// inside a segment, `**` crosses them. Two-sided per pattern so neither
    /// half can pass by the matcher answering the same thing for everything.
    #[test]
    fn star_stays_in_its_segment_and_double_star_crosses() {
        assert!(glob_match("src/lib.rs", "src/*.rs"));
        assert!(!glob_match("src/a/lib.rs", "src/*.rs"), "`*` must not cross `/`");
        assert!(glob_match("src/a/b/lib.rs", "src/**/lib.rs"));
        assert!(glob_match("src/lib.rs", "**/lib.rs"));
        assert!(!glob_match("src/lib.rs", "src/*.ts"), "the literal tail still binds");
        assert!(glob_match("exact/path.rs", "exact/path.rs"), "no glob is plain equality");
    }
}
