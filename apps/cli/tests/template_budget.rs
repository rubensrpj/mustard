//! `template_budget` — the leanness gate for the `.md` template corpus, aligned
//! to Claude Code's OWN standards for command/skill/injectable markdown.
//!
//! Claude Code does NOT cap the BODY of a command by word count. The published
//! doctrine is "progressive disclosure": keep the primary file lean and push
//! detail into reference files that load on demand
//! (`code.claude.com/docs/en/skills`, `.../memory`). Mustard already follows
//! that structurally — the LIGHT command body + the `refs/` tree that opens only
//! when a flow reaches it. So this test does NOT re-impose a home-grown word
//! budget; the 2026-07-07 audit's leanness intent is now anchored to the two
//! places where Claude Code publishes a REAL, runtime-breaking limit:
//!
//! 1. A command/skill `description` is truncated at **1,536 characters** in the
//!    skill listing. Past that, the trigger text is cut mid-sentence and the
//!    command mis-triggers.
//! 2. An injectable spliced as `additionalContext` is truncated at **10,000
//!    characters** by the harness — past that it is cut mid-sentence, silently.
//!    9,500 leaves margin for the composition separator + siblings.
//!
//! Everything else (command / ref body size) is governed by structure
//! (progressive disclosure) and human review, not a numeric tripwire — and
//! that rationale never rides inside the loaded templates.

use std::path::{Path, PathBuf};

/// Hard cap on a command/skill `description` frontmatter field. Claude Code
/// truncates `description` (combined with `when_to_use`) at 1,536 characters in
/// the skill listing; past that the trigger text is cut mid-sentence.
const DESCRIPTION_CHAR_CAP: usize = 1_536;

/// Character cap per injectable template (`templates/mustard/*.md`). The real
/// `additionalContext` ceiling is 10,000 characters; 9,500 leaves margin for
/// the composition separator + any sibling block injected in the same hook.
const INJECTABLE_CHAR_CAP: usize = 9_500;

/// The `plugin/` tree — home of the command/ref corpus.
fn plugin_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugin")
}

/// The core seed tree — the compiled-in harness seeds; the `mustard/`
/// injectables are spliced as `additionalContext` by the session hooks.
fn core_templates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/core/templates")
}

fn collect_md(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_md(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path);
        }
    }
}

/// Extract the `description:` value from a template's YAML frontmatter (the
/// block between the leading `---` fences). Handles a single-line scalar and a
/// folded/literal block (`>` / `|`). Returns `None` when the file has no
/// frontmatter or no `description` key (refs, injectables) — those are skipped.
fn frontmatter_description(text: &str) -> Option<String> {
    let after_open = text.strip_prefix("---")?;
    let end = after_open.find("\n---")?;
    let lines: Vec<&str> = after_open[..end].lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let Some(rest) = line.trim_start().strip_prefix("description:") else {
            continue;
        };
        let rest = rest.trim();
        // Folded / literal scalar: the value is the indented lines that follow.
        if matches!(rest, ">" | "|" | ">-" | "|-") {
            let mut folded = String::new();
            for cont in &lines[i + 1..] {
                if cont.trim().is_empty() {
                    continue;
                }
                // A non-indented line is the next key — the block ended.
                if !cont.starts_with([' ', '\t']) {
                    break;
                }
                if !folded.is_empty() {
                    folded.push(' ');
                }
                folded.push_str(cont.trim());
            }
            return Some(folded);
        }
        // Single-line scalar (optionally quoted).
        return Some(rest.trim_matches(['"', '\'']).to_string());
    }
    None
}

/// A command whose `description` (the auto-trigger + `/` listing text) exceeds
/// Claude Code's 1,536-character cut-off mis-triggers, because the harness
/// truncates it mid-sentence. Scan every command `.md` and hold the cap.
#[test]
fn command_descriptions_fit_the_listing_cap() {
    let mut files = Vec::new();
    collect_md(&plugin_dir().join("commands"), &mut files);
    assert!(
        !files.is_empty(),
        "no command templates found under {}/commands",
        plugin_dir().display()
    );

    let mut violations: Vec<String> = Vec::new();
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let Some(desc) = frontmatter_description(&text) else {
            continue;
        };
        let chars = desc.chars().count();
        if chars > DESCRIPTION_CHAR_CAP {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
            violations.push(format!(
                "{name}: description is {chars} chars (cap {DESCRIPTION_CHAR_CAP} — \
                 Claude Code truncates it mid-sentence in the skill listing)"
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "command descriptions over Claude Code's 1,536-char listing cap - shorten them:\n{}",
        violations.join("\n"),
    );
}

/// Every injectable template must fit the `additionalContext` payload with
/// margin: the harness caps that payload at 10,000 characters, and an
/// injectable that exceeds it would be truncated mid-sentence at runtime —
/// silently. 9,500 leaves room for the composition separators and any sibling
/// block injected in the same hook response.
#[test]
fn injectable_templates_fit_the_additional_context_cap() {
    let dir = core_templates_dir().join("mustard");
    let mut files = Vec::new();
    collect_md(&dir, &mut files);
    assert!(
        !files.is_empty(),
        "no injectable templates found under {} — init would seed nothing",
        dir.display()
    );

    let mut violations: Vec<String> = Vec::new();
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            violations.push(format!("{}: unreadable", path.display()));
            continue;
        };
        let chars = text.chars().count();
        if chars > INJECTABLE_CHAR_CAP {
            violations.push(format!(
                "{}: {chars} characters (cap {INJECTABLE_CHAR_CAP} — the harness truncates additionalContext at 10,000)",
                path.display()
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "injectable templates over the additionalContext budget:\n{}",
        violations.join("\n"),
    );
}
