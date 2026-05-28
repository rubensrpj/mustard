//! Shared wave helpers — a port of `scripts/_lib/wave-lib.js`.
//!
//! `detect_role` and `parse_files_section` are used by `wave-dependency`,
//! `exec-rewave-check` and `wave-size-check`.

use crate::commands::spec::spec_sections::is_heading;

/// Classify a file path into a coarse architectural role.
///
/// Mirrors the JS substring-alternation regexes (`detectRole`): the first
/// matching category wins, in order schema → api → ui → test → lib.
#[must_use]
pub fn detect_role(file_path: &str) -> &'static str {
    let lower = file_path.to_lowercase();
    let any = |needles: &[&str]| needles.iter().any(|n| lower.contains(n));
    if any(&["schema", "migration", "entity", "model", "drizzle", "prisma"]) {
        "schema"
    } else if any(&["api", "controller", "route", "endpoint", "handler", "service"]) {
        "api"
    } else if any(&["ui", "component", "view", "page", "screen", "widget"]) {
        "ui"
    } else if any(&["test", "spec", "__tests__"]) {
        "test"
    } else {
        "lib"
    }
}

/// Whether a trimmed line starts a new `## ` section (any heading).
fn is_section_break(trimmed: &str) -> bool {
    if let Some(rest) = trimmed.strip_prefix("##") {
        // JS `/^##\s/` — `##` followed by exactly one whitespace char.
        return rest.starts_with([' ', '\t']);
    }
    false
}

/// Parse the leading `- path` / `- \`path\`` bullet of a line, mirroring the JS
/// `/^-\s+`?([^\s`]+)`?/` pattern. Returns the captured path.
fn parse_bullet(trimmed: &str) -> Option<&str> {
    let rest = trimmed.strip_prefix('-')?;
    if !rest.starts_with([' ', '\t']) {
        return None;
    }
    let rest = rest.trim_start_matches([' ', '\t']);
    let rest = rest.strip_prefix('`').unwrap_or(rest);
    // `[^\s`]+` — take chars up to whitespace or a backtick.
    let token: &str = rest
        .split(|c: char| c.is_whitespace() || c == '`')
        .next()
        .unwrap_or("");
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

/// Parse the `## Files` section of a spec and return the listed paths.
///
/// Returns `None` when the section is absent; `Some(vec![])` when present but
/// empty.
#[must_use]
pub fn parse_files_section(spec_text: &str) -> Option<Vec<String>> {
    let lines: Vec<&str> = spec_text.split('\n').collect();
    let start = lines.iter().position(|l| is_heading(l, "files"))?;

    let mut paths = Vec::new();
    for line in lines.iter().skip(start + 1) {
        let trimmed = line.trim();
        if is_section_break(trimmed) {
            break;
        }
        if let Some(token) = parse_bullet(trimmed) {
            if !token.starts_with('#') {
                paths.push(token.to_string());
            }
        }
    }
    Some(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_role_classifies_paths() {
        assert_eq!(detect_role("src/schema/user.ts"), "schema");
        assert_eq!(detect_role("src/api/users.ts"), "api");
        assert_eq!(detect_role("src/components/Btn.tsx"), "ui");
        assert_eq!(detect_role("src/foo.test.ts"), "test");
        assert_eq!(detect_role("src/util/helpers.ts"), "lib");
    }

    #[test]
    fn parse_files_reads_bullets() {
        let spec = "# Spec\n\n## Files\n- src/a.ts\n- `src/b.ts`\n\n## Tasks\n- [ ] x\n";
        let files = parse_files_section(spec).unwrap();
        assert_eq!(files, vec!["src/a.ts", "src/b.ts"]);
    }

    #[test]
    fn parse_files_absent_section_is_none() {
        assert!(parse_files_section("# Spec\n\n## Tasks\n").is_none());
    }

    #[test]
    fn parse_files_empty_section_is_empty_vec() {
        let files = parse_files_section("## Files\n\n## Tasks\n").unwrap();
        assert!(files.is_empty());
    }
}
