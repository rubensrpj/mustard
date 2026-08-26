//! `paths` — the one reader of a DECLARED file path.
//!
//! A declared injectable path is written by hand as often as it is seeded: in
//! `mustard.json#inject`, in the `--inject` flag of a hook registration, and in
//! the seed itself. One file therefore has several honest spellings — a `./`
//! prefix, backslashes on Windows, a trailing separator, mixed case on a
//! case-insensitive filesystem.
//!
//! Comparing the raw strings makes each of those a different file, and the
//! symptom is never an error: a sibling hook silently delivers nothing, or the
//! blocks that belong to the whole invocation are dropped because no sibling
//! recognised itself as the elected one. Both were found in review of the unit
//! that introduced sibling hooks, in two of the three places that needed the
//! comparison — which is why it lives here now instead of being written a
//! fourth time.

/// `true` when two declared paths name the SAME file.
///
/// Normalisation is deliberately conservative: separators, one leading `./`,
/// trailing separators, and ASCII case. It never resolves symlinks and never
/// touches the filesystem — callers compare paths that may not exist yet
/// (install time), and a filesystem probe would make the answer depend on
/// state the caller cannot see.
#[must_use]
pub fn same_declared_file(a: &str, b: &str) -> bool {
    fn norm(s: &str) -> String {
        let s = s.trim().replace('\\', "/");
        let s = s.strip_prefix("./").unwrap_or(&s).to_string();
        s.trim_end_matches('/').to_ascii_lowercase()
    }
    norm(a) == norm(b)
}

#[cfg(test)]
mod tests {
    use super::same_declared_file;

    #[test]
    fn equivalent_spellings_name_one_file() {
        let canonical = ".claude/mustard/orchestrator.md";
        for spelling in [
            ".claude/mustard/orchestrator.md",
            "./.claude/mustard/orchestrator.md",
            ".claude\\mustard\\orchestrator.md",
            ".claude/Mustard/Orchestrator.md",
            "  .claude/mustard/orchestrator.md  ",
        ] {
            assert!(same_declared_file(spelling, canonical), "`{spelling}` should match");
        }
    }

    #[test]
    fn different_files_stay_different() {
        assert!(!same_declared_file(
            ".claude/mustard/orchestrator.md",
            ".claude/mustard/dispatch.md",
        ));
        // A prefix is not a match: `dispatch.md` and `dispatch.md.bak` are two
        // files, and treating them as one would elect the wrong sibling.
        assert!(!same_declared_file(
            ".claude/mustard/dispatch.md",
            ".claude/mustard/dispatch.md.bak",
        ));
    }
}
