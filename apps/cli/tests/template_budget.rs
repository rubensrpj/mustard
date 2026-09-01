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
//!    characters** per hook RESPONSE. The overflow is NOT cut mid-sentence:
//!    hook output past the limit "is saved to a file and replaced with a
//!    preview and file path". An injectable over budget does not lose a clause,
//!    it stops being TEXT IN FORCE and becomes a pointer the model may or may
//!    not open. For a router the window needs on every unit, that is the whole
//!    failure.
//!
//!    The ceiling is per response, so each injectable gets its OWN sibling hook
//!    and there is no composite budget between siblings — measured 2026-08-25:
//!    two siblings emitting 6,000 characters each on one `UserPromptSubmit`
//!    both arrived intact and separate. A document that outgrows 10,000 is
//!    SPLIT and given another hook, never compressed until a rule drops out
//!    (see `packages/core/templates/mustard/{orchestrator,dispatch}.md` and
//!    `plugin/refs/mustard/router-rationale.md`).
//!
//! Everything else (command / ref body size) is governed by structure
//! (progressive disclosure) and human review, not a numeric tripwire — and
//! that rationale never rides inside the loaded templates.

use std::path::{Path, PathBuf};

/// Hard cap on a command/skill `description` frontmatter field. Claude Code
/// truncates `description` (combined with `when_to_use`) at 1,536 characters in
/// the skill listing; past that the trigger text is cut mid-sentence.
const DESCRIPTION_CHAR_CAP: usize = 1_536;

/// Advisory cap per injectable template (`templates/mustard/*.md`): the size at
/// which a document is carrying prose it should not.
///
/// It is deliberately NOT the real ceiling. Measured on the rewrite that
/// introduced it, `orchestrator.md` and `dispatch.md` held nothing but rules,
/// and a tighter target could only be met by
/// deleting instruction — a first attempt at one lost four real rules before
/// they were restored. A budget that forces a rule out is a guard that lies: it
/// stays green while the product gets worse. So this is an alarm for prose
/// creep, and [`HOOK_RESPONSE_CAP`] is what actually binds.
///
/// **The count is of the file AS CHECKED OUT, line endings included.** A Windows
/// checkout carries CRLF, one extra character per line. An earlier budget left 5
/// characters of margin and passed locally while failing CI on Windows only —
/// green where it is written, red where nobody is looking.
const INJECTABLE_CHAR_CAP: usize = 8_000;

/// The real ceiling: characters one HOOK RESPONSE may carry.
///
/// Per RESPONSE, not per event. Sibling hooks on one event are separate
/// invocations and Claude Code keeps every one of their `additionalContext`
/// blocks — measured 2026-08-25 with two siblings emitting 6,000 characters
/// each. Rationale: `plugin/refs/mustard/router-rationale.md`.
const HOOK_RESPONSE_CAP: usize = 10_000;

/// The size a hook response has to carry for this text: the LARGER of its
/// character count and its byte count.
///
/// Which unit the harness counts is not documented, and the two differ wherever
/// the text is not plain ASCII — the shipped templates carry accents, em dashes
/// and `▸`/`⨯`. Measuring the smaller number would call a file clean while it is
/// already past the ceiling and degraded to a path, so the conservative reading
/// is the only honest one here.
fn payload_size(text: &str) -> usize {
    text.chars().count().max(text.len())
}

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
        let chars = payload_size(&text);
        if chars > INJECTABLE_CHAR_CAP {
            violations.push(format!(
                "{}: {chars} (larger of chars/bytes; cap {INJECTABLE_CHAR_CAP} — a hook response \
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

/// AC-1 — every declared injectable owns the ceiling of its OWN hook response,
/// and siblings on one event impose no composite budget on each other.
///
/// This replaces a per-EVENT sum that measured a constraint the harness does not
/// have. That test asserted two 6,000-character documents on one event would
/// blow the response; the experiment run on 2026-08-25 registered exactly that
/// shape — two sibling `UserPromptSubmit` hooks emitting 6,000 characters each,
/// 12,000 combined — and BOTH arrived intact, in separate blocks, each with its
/// own header and end marker. Nothing was truncated. The official guide states
/// the same rule: "Text from `additionalContext` is kept from every hook and
/// passed to Claude together."
///
/// So the cap is per hook RESPONSE. The rule this test holds is the one that
/// follows: each injectable is delivered by its own sibling hook, so each is
/// measured on its own against the real 10,000, and a document that outgrows it
/// is split — never compressed past the point where a rule goes out.
#[test]
fn each_injectable_owns_its_hook_ceiling() {
    let dir = core_templates_dir().join("mustard");
    let entries = mustard_core::platform::project_seed::default_inject_entries();
    assert!(!entries.is_empty(), "no injectables are declared — the router reaches nobody");

    let mut violations = Vec::new();
    for entry in &entries {
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
        let chars = payload_size(&text);
        if chars > HOOK_RESPONSE_CAP {
            violations.push(format!(
                "{name} on {}: {chars} (larger of chars/bytes) over the {HOOK_RESPONSE_CAP} a single hook \
                 response carries. Its own sibling hook cannot save it — SPLIT the document \
                 and give each half a hook, never compress a rule out to fit.",
                entry.on,
            ));
        }
    }
    assert!(violations.is_empty(), "injectables over their own hook ceiling:\n{}", violations.join("\n"));
}

/// The ratchet that would have caught the compaction overflow: no single hook
/// RESPONSE may exceed the ceiling, whatever it composes.
///
/// The per-injectable check above answers "does this document fit?" — and a
/// review found that was not the binding question. A `SessionStart` response
/// folds the terrain census, every `sessionStart` injectable and two advisories
/// into ONE string. An earlier revision of this unit also folded the
/// `userPromptSubmit` family in on a compaction, and the response measured
/// 11,973 characters on this repository: over the cap, so the router became a
/// file path instead of text in force.
///
/// The predecessor of this test summed per EVENT, which was wrong in the other
/// direction — sibling hooks are separate responses and do not share a budget
/// (measured 2026-08-25). What binds is the RESPONSE, so that is what this
/// measures: the largest set of blocks any one invocation can compose.
#[test]
fn no_single_hook_response_can_exceed_the_ceiling() {
    // What a `SessionStart` response composes alongside its injectables: the
    // terrain census (16 rows at ~45 chars, plus header and truncation line)
    // and the two advisories. Sized from `TERRAIN_ROWS_CAP`, not guessed.
    const COMPOSED_SIBLINGS: usize = 1_600;

    let dir = core_templates_dir().join("mustard");
    let mut per_event: std::collections::BTreeMap<String, (usize, Vec<String>)> =
        std::collections::BTreeMap::new();
    for entry in mustard_core::platform::project_seed::default_inject_entries() {
        let name = Path::new(&entry.file)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let text = std::fs::read_to_string(dir.join(&name)).unwrap_or_else(|e| {
            panic!("declared injectable {} has no seed: {e}", entry.file)
        });
        let slot = per_event.entry(entry.on.clone()).or_insert((0, Vec::new()));
        slot.0 += payload_size(&text);
        slot.1.push(name);
    }

    // Each injectable rides its OWN sibling hook, so an event's injectables are
    // never summed against each other. What IS summed into one response is the
    // largest injectable plus everything the hook composes around it.
    let mut violations = Vec::new();
    for (event, (_total, files)) in &per_event {
        let largest = files
            .iter()
            .map(|f| {
                std::fs::read_to_string(dir.join(f))
                    .map(|s| payload_size(&s))
                    .unwrap_or(0)
            })
            .max()
            .unwrap_or(0);
        let composed = largest + COMPOSED_SIBLINGS;
        if composed > HOOK_RESPONSE_CAP {
            violations.push(format!(
                "{event}: the largest injectable [{}] plus the {COMPOSED_SIBLINGS} chars the \
                 hook composes around it is {composed}, over the {HOOK_RESPONSE_CAP} one \
                 RESPONSE carries. Split the document and give each half its own sibling hook.",
                files.join(", "),
            ));
        }
    }
    assert!(violations.is_empty(), "hook responses over the ceiling:\n{}", violations.join("\n"));
}
