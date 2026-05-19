//! `mustard-rt run mark-checklist-item` — a port of
//! `scripts/mark-checklist-item.js`.
//!
//! Marks a single `- [ ]` item as `- [x]` in a spec's `## Checklist` section.
//! Idempotent. Resolves a bare spec name to
//! `.claude/spec/active/<name>/spec.md`.
//!
//! Output (stdout): one line — `marked` | `already-marked` | `error: <reason>`.
//! Exit codes: 0 success/no-op, 1 not-found/no-section/not-located, 2 bad args.

use std::path::{Path, PathBuf};

/// Print `error: <msg>` and exit with `code`.
fn die(code: i32, msg: &str) -> ! {
    println!("error: {msg}");
    std::process::exit(code);
}

/// Resolve a spec argument to a `spec.md` path.
fn resolve_spec_path(spec: &str, cwd: &Path) -> Option<PathBuf> {
    let p = Path::new(spec);
    if p.is_absolute() && spec.ends_with(".md") && p.exists() {
        return Some(p.to_path_buf());
    }
    let active = cwd
        .join(".claude")
        .join("spec")
        .join("active")
        .join(spec)
        .join("spec.md");
    if active.exists() {
        return Some(active);
    }
    let as_dir = Path::new(spec).join("spec.md");
    if as_dir.exists() {
        return Some(as_dir);
    }
    None
}

/// Locate the `## Checklist` section. Returns `(start_idx, end_idx)` where
/// `start_idx` is the first body line after the header and `end_idx` is the
/// next `## ` header (exclusive) or end-of-file.
fn find_checklist_section(lines: &[&str]) -> Option<(usize, usize)> {
    let start = lines.iter().position(|l| {
        // `^##\s+Checklist\b`
        l.strip_prefix("##")
            .map(|r| {
                let t = r.trim_start_matches([' ', '\t']);
                t.len() != r.len()
                    && {
                        let lower = t.to_lowercase();
                        lower.strip_prefix("checklist").map_or(false, |tail| {
                            tail.chars()
                                .next()
                                .map_or(true, |c| !(c.is_ascii_alphanumeric() || c == '_'))
                        })
                    }
            })
            .unwrap_or(false)
    })? + 1;
    let mut end = lines.len();
    for (i, l) in lines.iter().enumerate().skip(start) {
        // `^##\s`
        if l.strip_prefix("##").map(|r| r.starts_with([' ', '\t'])).unwrap_or(false) {
            end = i;
            break;
        }
    }
    Some((start, end))
}

/// Parsed checkbox line: `(prefix, state, gap, text)`.
struct Checkbox<'a> {
    prefix: &'a str,
    state: char,
    gap: &'a str,
    text: &'a str,
}

/// Parse a `^(\s*-\s+)\[([ xX])\](\s+)(.*)$` checkbox line.
fn parse_checkbox(line: &str) -> Option<Checkbox<'_>> {
    let trimmed_start = line.len() - line.trim_start().len();
    let after_ws = &line[trimmed_start..];
    let rest = after_ws.strip_prefix('-')?;
    if !rest.starts_with([' ', '\t']) {
        return None;
    }
    let dash_gap_len = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    let prefix_end = trimmed_start + 1 + dash_gap_len;
    let body = &line[prefix_end..];
    let inner = body.strip_prefix('[')?;
    let state = inner.chars().next()?;
    if !matches!(state, ' ' | 'x' | 'X') {
        return None;
    }
    let after_state = &inner[state.len_utf8()..];
    let after_bracket = after_state.strip_prefix(']')?;
    if after_bracket.is_empty() || !after_bracket.starts_with([' ', '\t']) {
        return None;
    }
    let gap_len = after_bracket.len() - after_bracket.trim_start_matches([' ', '\t']).len();
    let text = &after_bracket[gap_len..];
    Some(Checkbox {
        prefix: &line[..prefix_end],
        state,
        gap: &after_bracket[..gap_len],
        text,
    })
}

/// Dispatch `mustard-rt run mark-checklist-item`.
pub fn run(spec: Option<&str>, item: Option<&str>, line: Option<usize>, cwd_arg: Option<&str>) {
    let Some(spec) = spec else {
        die(2, "--spec is required");
    };
    if item.is_none() && line.is_none() {
        die(2, "either --item or --line is required");
    }
    if item.is_some() && line.is_some() {
        die(2, "--item and --line are mutually exclusive");
    }

    let cwd = cwd_arg
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let Some(spec_path) = resolve_spec_path(spec, &cwd) else {
        die(1, &format!("spec not found: {spec}"));
    };

    let raw = match std::fs::read_to_string(&spec_path) {
        Ok(r) => r,
        Err(e) => die(1, &format!("cannot read spec: {e}")),
    };
    let mut lines: Vec<String> = raw.split('\n').map(String::from).collect();
    let line_refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let Some((start, end)) = find_checklist_section(&line_refs) else {
        die(1, "no `## Checklist` section in spec");
    };

    let target_idx: usize = if let Some(n) = line {
        let idx = n.wrapping_sub(1);
        if n == 0 || idx < start || idx >= end {
            die(
                1,
                &format!(
                    "--line {n} is outside the Checklist section (lines {}-{end})",
                    start + 1
                ),
            );
        }
        if parse_checkbox(&lines[idx]).is_none() {
            die(1, &format!("--line {n} is not a checkbox"));
        }
        idx
    } else {
        let item = item.unwrap_or("");
        let mut found: Option<usize> = None;
        for i in start..end {
            if let Some(cb) = parse_checkbox(&lines[i]) {
                if cb.state == ' ' && cb.text.contains(item) {
                    found = Some(i);
                    break;
                }
            }
        }
        match found {
            Some(i) => i,
            None => {
                // Idempotency: was the only match already `[x]`?
                for i in start..end {
                    if let Some(cb) = parse_checkbox(&lines[i]) {
                        if (cb.state == 'x' || cb.state == 'X') && cb.text.contains(item) {
                            println!("already-marked");
                            std::process::exit(0);
                        }
                    }
                }
                die(1, &format!("no `- [ ]` item matching: {item}"));
            }
        }
    };

    let new_line = {
        let cb = match parse_checkbox(&lines[target_idx]) {
            Some(c) => c,
            None => die(1, "target line is not a checkbox"),
        };
        if cb.state == 'x' || cb.state == 'X' {
            println!("already-marked");
            std::process::exit(0);
        }
        format!("{}[x]{}{}", cb.prefix, cb.gap, cb.text)
    };
    lines[target_idx] = new_line;

    if let Err(e) = std::fs::write(&spec_path, lines.join("\n")) {
        die(1, &format!("cannot write spec: {e}"));
    }
    println!("marked");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_spec(body: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(&path, body).unwrap();
        (dir, path)
    }

    #[test]
    fn parses_checkbox_lines() {
        let cb = parse_checkbox("  - [ ] do the thing").unwrap();
        assert_eq!(cb.state, ' ');
        assert_eq!(cb.text, "do the thing");
        assert!(parse_checkbox("- not a checkbox").is_none());
    }

    #[test]
    fn finds_checklist_section() {
        let lines = vec!["# Spec", "## Checklist", "- [ ] a", "## Next"];
        let (start, end) = find_checklist_section(&lines).unwrap();
        assert_eq!((start, end), (2, 3));
    }

    #[test]
    fn marks_item_by_substring() {
        let (_d, path) = write_spec("## Checklist\n- [ ] alpha\n- [ ] beta\n");
        let mut lines: Vec<String> =
            std::fs::read_to_string(&path).unwrap().split('\n').map(String::from).collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let (start, end) = find_checklist_section(&refs).unwrap();
        let mut idx = None;
        for i in start..end {
            if let Some(cb) = parse_checkbox(&lines[i]) {
                if cb.state == ' ' && cb.text.contains("beta") {
                    idx = Some(i);
                }
            }
        }
        let i = idx.unwrap();
        let cb = parse_checkbox(&lines[i]).unwrap();
        lines[i] = format!("{}[x]{}{}", cb.prefix, cb.gap, cb.text);
        assert_eq!(lines[i], "- [x] beta");
    }
}
