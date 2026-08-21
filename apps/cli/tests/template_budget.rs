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
//! 2. An injectable spliced as `additionalContext` is capped at **10,000
//!    characters** per hook response. The overflow is NOT cut mid-sentence:
//!    `code.claude.com/docs/en/hooks` says hook output past the limit "is saved
//!    to a file and replaced with a preview and file path". That is why the cap
//!    still binds, and it is a different reason than the one this comment used
//!    to give — an injectable over budget does not lose a clause, it stops
//!    being TEXT IN FORCE and becomes a pointer the model may or may not open.
//!    For a router injected on every prompt, that is the whole failure.
//!    9,500 leaves margin for the composition separator + siblings: the hooks
//!    fold every injectable of ONE event into a single `additionalContext`
//!    (the dispatcher fold is last-writer-wins), alongside the terrain census
//!    and the advisories. A document that outgrows the budget is SPLIT across
//!    two events — two hook invocations, two ceilings — never compressed to fit
//!    (see `packages/core/templates/mustard/{orchestrator,dispatch}.md`).
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
/// `additionalContext` ceiling is 10,000 characters per hook response; 9,500
/// leaves margin for the composition separator + any sibling block injected in
/// the same hook. The remedy for a template that outgrows it is a SPLIT onto a
/// second event, not a rewrite that says less.
const INJECTABLE_CHAR_CAP: usize = 9_500;

/// The real ceiling: characters one HOOK RESPONSE may carry.
const HOOK_RESPONSE_CAP: usize = 10_000;

/// Room the per-event budget leaves for everything the hook composes ALONGSIDE
/// its injectables — the terrain census, the version-drift advisory, the
/// pending-prune advisory, and the blank line between each block.
///
/// **Why a per-EVENT budget exists at all.** The per-file cap above answers
/// "does this document fit?", and that was never the binding question: a hook
/// returns ONE string, and `SessionStart` folds the census plus every injectable
/// of that event plus two advisories into it. Measured on the real hook, a
/// repository with about 50 subprojects pushed the composed payload to 10,079
/// characters while every individual file still passed its own check — a green
/// ratchet over a reachable failure, which is the exact defect shape this
/// project keeps finding.
///
/// The census used to be the unbounded term; it is now capped at a known number
/// of rows (`TERRAIN_ROWS_CAP`), which is what makes this reserve a fact rather
/// than a hope. 2,000 characters covers the capped census with room for both
/// advisories.
const COMPOSED_SIBLING_RESERVE: usize = 2_000;

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
/// margin: the harness caps that payload at 10,000 characters per hook
/// response, and the overflow is saved to a FILE the window receives only as a
/// preview plus a path — so an injectable over budget silently stops being in
/// force, which for a router injected every prompt is the whole point of it.
/// 9,500 leaves room for the composition separators and any sibling block
/// injected in the same hook response.
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
                "{}: {chars} characters (cap {INJECTABLE_CHAR_CAP} — a hook response \
                 carries 10,000 characters of additionalContext; the overflow becomes a \
                 file path instead of text in force. SPLIT it onto a second event, do \
                 not compress it)",
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

/// AC-1 — the budget is measured PER EVENT, summing every injectable the same
/// hook returns, not per file.
///
/// The per-file check above cannot see the binding constraint. A hook answers
/// with ONE `additionalContext` string: `SessionStart` folds the terrain census,
/// every injectable declared on that event and two advisories into it, and the
/// harness caps the result at 10,000 characters. Two documents of 6,000 each
/// pass the per-file cap and blow the response.
///
/// Measured before this test existed: a repository with roughly 50 subprojects
/// composed a 10,079-character `SessionStart` payload while both injectables sat
/// comfortably under 9,500. The ratchet was green over a reachable failure.
#[test]
fn event_budget_sums_every_injectable_of_the_same_hook() {
    use std::collections::BTreeMap;

    let dir = core_templates_dir().join("mustard");
    let mut per_event: BTreeMap<String, (usize, Vec<String>)> = BTreeMap::new();

    for entry in mustard_core::platform::project_seed::default_inject_entries() {
        // The declared path is project-relative (`.claude/mustard/x.md`); the
        // SEED that fills it lives in the templates tree under the same name.
        let name = Path::new(&entry.file)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let path = dir.join(&name);
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!("declared injectable {} has no seed at {}: {e}", entry.file, path.display())
        });
        let slot = per_event.entry(entry.on.clone()).or_insert((0, Vec::new()));
        slot.0 += text.chars().count();
        slot.1.push(format!("{name} ({} chars)", text.chars().count()));
    }

    assert!(!per_event.is_empty(), "no injectables are declared — the router reaches nobody");

    let budget = HOOK_RESPONSE_CAP - COMPOSED_SIBLING_RESERVE;
    let mut violations = Vec::new();
    for (event, (total, files)) in &per_event {
        // Blank line between blocks, plus one for the census the hook prepends.
        let composed = total + files.len().saturating_mul(2);
        if composed > budget {
            violations.push(format!(
                "{event}: {composed} characters across {} injectable(s) [{}] — budget {budget} \
                 ({HOOK_RESPONSE_CAP} per hook response minus {COMPOSED_SIBLING_RESERVE} for the \
                 census and the advisories the same hook composes). Each file fits on its own; \
                 the RESPONSE does not. Move one onto another event — never compress.",
                files.len(),
                files.join(", "),
            ));
        }
    }
    assert!(violations.is_empty(), "hook responses over budget:\n{}", violations.join("\n"));
}
