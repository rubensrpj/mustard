//! The critical-Guard reader: which `## Guards` lines of an instruction file
//! the post-edit gate enforces. It only reads; nothing in Mustard writes a
//! `CLAUDE.md` any more.

use std::path::Path;

use mustard_core::io::fs as mfs;

// ===========================================================================
// Critical-Guard convention + parser (the data-driven Guards gate)
// ===========================================================================
//
// A `## Guards` line becomes **critical** — enforced by the post-edit gate,
// not merely advised — when it opens with the [`CRITICAL_MARKER`] token. Every
// unmarked Guard stays advisory. A critical Guard is *mechanically checkable*
// only in one documented form:
//
// ```text
// - [critical] never <forbidden> in <path-glob>
// ```
//
// where `<forbidden>` is a literal substring and `<path-glob>` a path pattern
// (each may be backtick-quoted). The gate Denies an edit that introduces
// `<forbidden>` in a file matching `<path-glob>`. A critical Guard in ANY other
// form is critical-but-unenforceable: surfaced as a strong advisory, never a
// Deny — the gate never fabricates enforcement of free prose.

/// The leading token that promotes a `## Guards` line to **critical**. ASCII
/// case-insensitive when parsed; the canonical authored spelling is lowercase.
pub(crate) const CRITICAL_MARKER: &str = "[critical]";

/// A critical Guard mined from a subproject's `## Guards` block.
pub(crate) struct CriticalGuard {
    /// The guard prose, with the bullet and `[critical]` marker stripped.
    pub(crate) text: String,
    /// The enforceable clause — `Some` only for the checkable `never … in …`
    /// form; `None` for critical-but-unenforceable prose (advisory only).
    pub(crate) checkable: Option<CheckableGuard>,
}

/// The machine-checkable clause of a critical Guard: forbid the `forbidden`
/// literal substring inside any file whose path matches `glob`.
pub(crate) struct CheckableGuard {
    /// The literal substring an edit must not introduce.
    pub(crate) forbidden: String,
    /// The path-glob scoping which files the rule covers.
    pub(crate) glob: String,
}

/// Read a subproject's ALREADY-RESOLVED instruction file and return its critical
/// Guards.
///
/// Takes the path rather than the directory on purpose: its one caller
/// (`hooks::write::post_edit::guards_gate`) located that file by walking up
/// through [`crate::shared::context::guards_file`], which under a private
/// install resolves to `CLAUDE.local.md`. Re-deriving the name here would be a
/// second place the mode is decided — and the one that got it wrong.
///
/// Fail-open: an unreadable/absent file, or one with no `## Guards` block,
/// yields an empty vec (no critical rules → the gate never blocks).
pub(crate) fn critical_guards(instruction_file: &Path) -> Vec<CriticalGuard> {
    match mfs::read_to_string(instruction_file) {
        Ok(content) => critical_guards_in(&content),
        Err(_) => Vec::new(),
    }
}

/// The testable core of [`critical_guards`]: parse the `## Guards` block out of
/// CLAUDE.md `content` and return its critical entries. The marker and `facts`
/// comment lines older scans left (`GUARDS_*`, all `<!-- … -->`) are skipped,
/// so a marked block and a curated marker-less block parse identically.
pub(crate) fn critical_guards_in(content: &str) -> Vec<CriticalGuard> {
    let lines: Vec<&str> = content.lines().collect();
    let Some(start) = lines.iter().position(|l| l.trim_end() == "## Guards") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in &lines[start + 1..] {
        if line.starts_with("## ") {
            break; // The next H2 heading ends the Guards block.
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("<!--") {
            continue; // Blank, or an enrich marker / facts comment.
        }
        let Some(bullet) = strip_bullet(trimmed) else {
            continue; // Not a do/don't bullet (e.g. the census `Tipo:` line).
        };
        let Some(clause) = strip_critical_marker(bullet) else {
            continue; // An advisory guard — not marked critical.
        };
        out.push(CriticalGuard {
            checkable: parse_checkable(clause),
            text: clause.trim().to_string(),
        });
    }
    out
}

/// Strip a leading markdown bullet (`-`/`*` + whitespace). `None` when the line
/// is not a bullet.
fn strip_bullet(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('-').or_else(|| line.strip_prefix('*'))?;
    rest.starts_with(char::is_whitespace).then(|| rest.trim_start())
}

/// If `text` opens with the [`CRITICAL_MARKER`] (ASCII case-insensitive), return
/// the remainder (the guard clause); else `None`.
fn strip_critical_marker(text: &str) -> Option<&str> {
    let head = text.get(..CRITICAL_MARKER.len())?;
    head.eq_ignore_ascii_case(CRITICAL_MARKER)
        .then(|| text[CRITICAL_MARKER.len()..].trim_start())
}

/// Parse the one enforceable clause form out of a critical Guard's prose:
/// `never <forbidden> in <path-glob>`. Each operand may be backtick-quoted
/// (recommended — lets it hold spaces or the word "in"); the `never` keyword and
/// the ` in ` separator are ASCII case-insensitive. Returns `None` for any other
/// prose — that Guard stays advisory (no fabricated enforcement).
fn parse_checkable(clause: &str) -> Option<CheckableGuard> {
    let rest = strip_ci_keyword(clause.trim(), "never")?;
    // Backtick form: `forbidden` in `glob` (the glob tail may be bare).
    if let Some(after_open) = rest.strip_prefix('`') {
        let end = after_open.find('`')?;
        let forbidden = after_open[..end].to_string();
        let after = strip_ci_keyword(after_open[end + 1..].trim_start(), "in")?;
        return build_checkable(forbidden, dequote(after.trim()));
    }
    // Bare form: split on the LAST " in " — a path-glob never contains " in ",
    // so the tail is the glob and the head the forbidden literal.
    let lower = rest.to_ascii_lowercase();
    let idx = lower.rfind(" in ")?;
    build_checkable(
        dequote(rest[..idx].trim()),
        dequote(rest[idx + " in ".len()..].trim()),
    )
}

/// If `s` starts with `kw` (ASCII case-insensitive) followed by whitespace,
/// return the trimmed remainder; else `None`. Never slices a non-char-boundary.
fn strip_ci_keyword<'a>(s: &'a str, kw: &str) -> Option<&'a str> {
    let head = s.get(..kw.len())?;
    if !head.eq_ignore_ascii_case(kw) {
        return None;
    }
    let rest = &s[kw.len()..];
    rest.starts_with(char::is_whitespace).then(|| rest.trim_start())
}

/// Strip a single pair of surrounding backticks, then trim.
fn dequote(s: &str) -> String {
    s.strip_prefix('`')
        .and_then(|t| t.strip_suffix('`'))
        .unwrap_or(s)
        .trim()
        .to_string()
}

/// Build a [`CheckableGuard`] unless either operand is empty.
fn build_checkable(forbidden: String, glob: String) -> Option<CheckableGuard> {
    (!forbidden.is_empty() && !glob.is_empty()).then_some(CheckableGuard { forbidden, glob })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_checkable_recognises_the_enforceable_forms() {
        // Backtick form.
        let c = parse_checkable("never `console.log` in `**/*.ts`").expect("backtick form");
        assert_eq!(c.forbidden, "console.log");
        assert_eq!(c.glob, "**/*.ts");
        // Bare form — split on the last " in ".
        let c = parse_checkable("never TODO in **/*.rs").expect("bare form");
        assert_eq!(c.forbidden, "TODO");
        assert_eq!(c.glob, "**/*.rs");
        // The `never` keyword is case-insensitive.
        assert!(parse_checkable("NEVER `x` in `**/*.rs`").is_some());
        // Prose that is not `never … in …` is not checkable (stays advisory).
        assert!(parse_checkable("always write a test first").is_none());
        assert!(parse_checkable("keep the public API stable").is_none());
    }

    #[test]
    fn critical_guards_in_extracts_only_marked_lines() {
        let content = "# Sub\n\n## Guards\n\n- [critical] never `.unwrap()` in `**/*.rs`\n- always document public items\n- [CRITICAL] keep the schema backward compatible\n\nTipo: cargo · 12 arquivos\n";
        let guards = critical_guards_in(content);
        assert_eq!(guards.len(), 2, "two [critical] lines, marker case-insensitive");
        // The checkable one carries a parsed clause.
        assert!(guards[0].checkable.is_some());
        assert_eq!(
            guards[0].checkable.as_ref().expect("checkable").forbidden,
            ".unwrap()"
        );
        // The advisory-only critical carries none (free prose).
        assert!(guards[1].checkable.is_none());
        // The unmarked bullet and the census line are ignored.
        assert!(!guards.iter().any(|g| g.text.contains("document public")));
    }

    #[test]
    fn critical_guards_in_absent_block_is_empty() {
        assert!(critical_guards_in("# Sub\n\nno guards section here\n").is_empty());
    }

}
