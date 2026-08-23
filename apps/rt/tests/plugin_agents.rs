//! Drift guards pinning the SHIPPED `plugin/agents/*.md` surface to the code.
//!
//! These files carry keys Claude Code reads, not Mustard: nothing in this
//! workspace fails to compile when one is dropped or renamed, the behaviour just
//! changes silently at runtime for every installation. That is the same failure
//! mode `command_frontmatter.rs` guards for the instruction surface, and these
//! tests are the same kind of ratchet for the agent surface.
//!
//! What is locked, and why it matters:
//!
//! - The placeholder table of `plugin/refs/agent-prompt/agent-prompt.md` must
//!   document every key in `TEMPLATE_PLACEHOLDERS`. A placeholder the renderer
//!   fills and the ref never mentions is material an author cannot know how to
//!   supply — which is how a channel ships and stays unused.
//! - Every `plugin/agents/*.md` must declare `model` and `effort`, with a value
//!   the runtime actually resolves. A dropped key re-inherits the session's
//!   model and reasoning budget, so the cheap roles quietly pay what the
//!   expensive one pays; a misspelled value is worse, because the file still
//!   reads as a deliberate decision while Claude Code ignores it.
//!
//! Reads outside the crate fail open (skip) per this codebase's test convention:
//! a workspace root this test cannot resolve is a fact about the checkout, not
//! about the shipped surface.

use std::fs;
use std::path::{Path, PathBuf};

use mustard_rt::commands::agent::render::TEMPLATE_PLACEHOLDERS;

/// The workspace root, from `apps/rt` up two levels. `None` when the layout
/// cannot be walked — callers skip rather than fail on it.
fn workspace_root() -> Option<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
}

/// The placeholder keys the ref's table documents, located by SHAPE — any table
/// row whose FIRST cell is a single backticked `{token}` — so the surrounding
/// prose can be reworded, reordered or retitled without touching this guard.
///
/// The first cell only: a `{placeholder}` named inside a Notes column is a
/// cross-reference, not a documented row, and counting it would let a mention in
/// passing stand in for an entry.
fn documented_placeholders(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| line.trim_start().starts_with('|'))
        .filter_map(|line| {
            let cell = line.trim().trim_start_matches('|').split('|').next()?.trim();
            let token = cell.strip_prefix('`')?.strip_suffix('`')?;
            (token.starts_with('{') && token.ends_with('}')).then(|| token.to_string())
        })
        .collect()
}

/// The ref's placeholder table and the renderer's substitution list must be the
/// SAME SET.
///
/// A set, deliberately, and never the count the prose states: a reworded
/// sentence would then break this test for a cosmetic reason, which is exactly
/// the false red the spec this guard ships with exists to prevent. The size is a
/// consequence of the set — it is never the claim being checked.
///
/// Both directions matter. An UNDOCUMENTED key is material the author of a spec
/// cannot know how to supply, so the channel ships and nobody uses it (that was
/// the live drift when this guard was written: `{conversation_material}` was
/// being substituted and the table did not mention it). A STALE row is worse in
/// the other direction — it sends a reader to configure something the renderer
/// no longer fills.
#[test]
fn agent_prompt_ref_documents_every_placeholder() {
    let Some(root) = workspace_root() else {
        eprintln!("[skip] cannot resolve workspace root from CARGO_MANIFEST_DIR");
        return;
    };
    let path = root.join("plugin/refs/agent-prompt/agent-prompt.md");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("placeholder contract {} is unreadable: {e}", path.display()));

    let documented = documented_placeholders(&text);
    assert!(
        !documented.is_empty(),
        "{} must keep a table whose first column is the `{{placeholder}}` keys — the contract \
         whoever plans a wave reads to know what a prompt carries",
        path.display()
    );

    let missing: Vec<&&str> = TEMPLATE_PLACEHOLDERS
        .iter()
        .filter(|key| !documented.iter().any(|d| d == *key))
        .collect();
    assert!(
        missing.is_empty(),
        "{} does not document {missing:?}. The renderer substitutes these, so a spec author \
         reading the ref cannot tell what reaches a wave — add one table row per key (source + \
         when it is empty). Documented: {documented:?}",
        path.display()
    );

    let stale: Vec<&String> = documented
        .iter()
        .filter(|d| !TEMPLATE_PLACEHOLDERS.contains(&d.as_str()))
        .collect();
    assert!(
        stale.is_empty(),
        "{} documents {stale:?}, which the renderer no longer substitutes — a reader would \
         supply material nothing reads. Renderer keys: {TEMPLATE_PLACEHOLDERS:?}",
        path.display()
    );
}

/// The four model aliases Claude Code resolves in agent frontmatter.
const MODEL_ALIASES: &[&str] = &["opus", "sonnet", "haiku", "fable"];

/// The five reasoning-effort levels Claude Code accepts in agent frontmatter.
const EFFORT_LEVELS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

/// `true` for a value Claude Code resolves to a model: one of the four aliases,
/// the literal `inherit` (take the session's — the default when the key is
/// absent, declared explicitly here so a MISSING key never reads as a choice),
/// or a full model id, which is always `claude-`-prefixed.
///
/// The whole id is read, not just its prefix. A test that stopped at
/// `starts_with("claude-")` certified `claude-opus-5"  # papel` — leftovers and
/// all — as a value the runtime resolves, which is the certification this
/// ratchet exists to withhold. An id is lowercase letters, digits, `-`, `.` and
/// the `[…]` context suffix (`claude-opus-5[1m]`); a space, a quote or a `#` in
/// there is not a model, it is something that never made it out of the line.
fn model_is_accepted(value: &str) -> bool {
    if MODEL_ALIASES.contains(&value) || value == "inherit" {
        return true;
    }
    let Some(id) = value.strip_prefix("claude-") else {
        return false;
    };
    !id.is_empty()
        && id.chars().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '.' | '[' | ']')
        })
}

/// `true` for one of the five reasoning-effort levels.
fn effort_is_accepted(value: &str) -> bool {
    EFFORT_LEVELS.contains(&value)
}

/// Read the frontmatter block (between the first two `---` fences) of a shipped
/// agent file. Panics with the path when the file or the block is missing — in a
/// ratchet, an unreadable input is a failure, never a silent pass.
fn agent_frontmatter(path: &Path) -> String {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("shipped agent {} is unreadable: {e}", path.display()));
    let body = text
        .strip_prefix("---")
        .unwrap_or_else(|| panic!("{} must open with a frontmatter fence", path.display()));
    let end = body
        .find("\n---")
        .unwrap_or_else(|| panic!("{} has no closing frontmatter fence", path.display()));
    body[..end].to_string()
}

/// The value of a TOP-LEVEL frontmatter key (column 0 only, so a `key:` quoted
/// inside a description is never mistaken for a declaration). `None` when the
/// key is absent.
fn declared(frontmatter: &str, key: &str) -> Option<String> {
    frontmatter
        .lines()
        .filter(|line| !line.starts_with(char::is_whitespace))
        .find_map(|line| line.strip_prefix(key)?.strip_prefix(':'))
        .map(scalar_value)
}

/// The YAML scalar a `key:` line declares — what the runtime reads, and nothing
/// else on the line.
///
/// Handing back the whole tail is what made `model: sonnet   # deliberate`
/// — valid YAML, resolved by Claude Code as `sonnet` — fail with "which Claude
/// Code does not resolve", teaching the next author to delete the note rather
/// than to keep it. A quoted scalar ends at its closing quote; an unquoted one
/// ends where an inline comment starts, which in YAML is a `#` preceded by
/// whitespace (a `#` inside `a#b` is part of the value).
///
/// Reading a value is never a way to LAUNDER a broken line. After the closing
/// quote YAML allows nothing but a comment, so `model: "sonnet" garbage` is not
/// a document Claude Code can load at all — a louder failure than the misspelling
/// this ratchet exists to catch. Returning `sonnet` for it would certify the
/// file as compliant; the whole line comes back instead, so the vocabulary check
/// rejects it and the message shows the reader exactly what is on the line.
fn scalar_value(raw: &str) -> String {
    let raw = raw.trim();
    for quote in ['"', '\''] {
        if let Some(rest) = raw.strip_prefix(quote) {
            // An unterminated quote closes nothing: hand the line back whole.
            let Some((inner, after)) = rest.split_once(quote) else {
                return raw.to_string();
            };
            let after = after.trim_start();
            return if after.is_empty() || after.starts_with('#') {
                inner.to_string()
            } else {
                raw.to_string()
            };
        }
    }
    if raw.starts_with('#') {
        return String::new();
    }
    let end = raw
        .char_indices()
        .find(|&(at, c)| c == '#' && raw[..at].ends_with(char::is_whitespace))
        .map_or(raw.len(), |(at, _)| at);
    raw[..end].trim_end().to_string()
}

/// Every `plugin/agents/*.md` path, sorted — the shipped agent files.
fn shipped_agents(root: &Path) -> Vec<PathBuf> {
    let dir = root.join("plugin/agents");
    let entries = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("shipped agent dir {} is unreadable: {e}", dir.display()));
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
        .collect();
    paths.sort();
    paths
}

/// Every shipped agent must declare BOTH `model` and `effort`, with a value the
/// runtime actually resolves.
///
/// Both halves matter, and the second is the one that looks redundant. A DROPPED
/// key silently re-inherits the session's model and its effort — which is the
/// expensive default the declarations exist to escape, and it fails by costing
/// more, never by breaking. A MISSPELLED value is worse: the file still reads as
/// a deliberate declaration to whoever opens it, while Claude Code ignores it.
/// Presence alone would certify exactly that file as compliant.
///
/// What is deliberately NOT locked is WHICH model each agent picks. Whether
/// `guards` runs on sonnet or haiku is an operating decision to retune as costs
/// and models move; pinning it here would turn every retune into a red test and
/// teach the next author to edit the guard instead of thinking.
#[test]
fn shipped_agents_declare_model_and_effort() {
    let Some(root) = workspace_root() else {
        eprintln!("[skip] cannot resolve workspace root from CARGO_MANIFEST_DIR");
        return;
    };
    let agents = shipped_agents(&root);
    assert!(
        !agents.is_empty(),
        "plugin/agents/ ships no *.md file — the agent surface this guard locks is gone"
    );

    for path in agents {
        let shown = path.display().to_string();
        let fm = agent_frontmatter(&path);

        let model = declared(&fm, "model").unwrap_or_else(|| {
            panic!(
                "{shown} declares no `model:`. Without it the agent inherits the session's model, \
                 which is the expensive default these declarations exist to escape — and it fails \
                 by costing more, never by breaking. Declare one of {MODEL_ALIASES:?}, a full \
                 `claude-*` id, or `inherit` to say the session's model is the deliberate choice. \
                 Frontmatter:\n{fm}"
            )
        });
        assert!(
            model_is_accepted(&model),
            "{shown} declares `model: {model}`, which Claude Code does not resolve — the file \
             reads as a decision and the runtime ignores it. Accepted: {MODEL_ALIASES:?}, \
             `inherit`, or a full `claude-*` id."
        );

        let effort = declared(&fm, "effort").unwrap_or_else(|| {
            panic!(
                "{shown} declares no `effort:`. Reasoning budget then follows the session for \
                 every role alike, so distilling facts the binary already computed costs what \
                 adversarial verification costs. Declare one of {EFFORT_LEVELS:?}. \
                 Frontmatter:\n{fm}"
            )
        });
        assert!(
            effort_is_accepted(&effort),
            "{shown} declares `effort: {effort}`, which Claude Code does not resolve — the file \
             reads as a decision and the runtime ignores it. Accepted: {EFFORT_LEVELS:?}."
        );
    }
}

/// The vocabulary check above is only worth having if it actually rejects the
/// near-misses — a plausible-looking value is exactly how a declaration goes
/// inert without anyone noticing.
#[test]
fn rejects_values_outside_the_accepted_vocabulary() {
    for good in ["opus", "sonnet", "haiku", "fable", "inherit", "claude-opus-5"] {
        assert!(model_is_accepted(good), "`model: {good}` must be accepted");
    }
    for bad in ["gpt", "sonnet-4", "cheapest", "", "Sonnet", "opus5"] {
        assert!(
            !model_is_accepted(bad),
            "`model: {bad}` must be rejected — Claude Code would ignore it"
        );
    }

    for good in EFFORT_LEVELS {
        assert!(effort_is_accepted(good), "`effort: {good}` must be accepted");
    }
    for bad in ["fast", "cheap", "none", "", "LOW", "highest"] {
        assert!(
            !effort_is_accepted(bad),
            "`effort: {bad}` must be rejected — Claude Code would ignore it"
        );
    }
}

/// An annotation is not a defect: a declaration that says WHY it picked what it
/// picked must survive the ratchet, quoted or commented.
///
/// This is the case that was measured against the shipped surface — a real agent
/// file was edited to carry `model: sonnet   # deliberate` and the ratchet
/// rejected it, reporting a value Claude Code resolves as one it does not. A
/// guard that rejects valid input teaches the author to delete the annotation,
/// which is the opposite of what a ratchet is for.
#[test]
fn scalar_value_reads_the_declaration_through_quotes_and_comments() {
    let fm = "name: mustard-review\n\
              model: sonnet   # cheap enough for this role\n\
              effort: \"high\"\n\
              tools: Read, Grep\n";
    assert_eq!(declared(fm, "model").as_deref(), Some("sonnet"));
    assert_eq!(declared(fm, "effort").as_deref(), Some("high"));
    assert_eq!(declared(fm, "absent"), None);

    // …and the value read this way is the one the vocabulary accepts, which is
    // the half that matters: reading it right and then rejecting it would be
    // the same red with a different cause.
    for (line, want) in [
        ("model: 'inherit'  # the session's, on purpose", "inherit"),
        ("model: \"claude-opus-5\"  # the full id", "claude-opus-5"),
        ("model: claude-opus-5[1m]", "claude-opus-5[1m]"),
    ] {
        let fm = format!("{line}\n");
        let read = declared(&fm, "model").expect("the line declares a model");
        assert_eq!(read, want, "`{line}` declares `{want}`");
        assert!(model_is_accepted(&read), "`{line}` declares a model the runtime resolves");
    }
}

/// Reading the value must not become a way of laundering a broken one.
///
/// The prefix check this replaced accepted anything starting with `claude-`, so
/// `model: "claude-opus-5"  # papel` — whose value, before the reader was fixed,
/// was the string `claude-opus-5"  # papel` — passed, certifying as deliberate a
/// value no runtime resolves. Leftovers after the id are still a failure; only
/// the ones that are genuinely a YAML comment or quoting are not.
#[test]
fn scalar_value_still_rejects_leftovers_after_the_id() {
    for bad in [
        "claude-opus-5\"  # papel", // what the old reader handed the old check
        "claude-opus-5 papel",      // a note with no `#` is part of the scalar
        "claude-opus-5\tpapel",
        "claude-",     // the prefix alone names no model
        "claude-Opus-5", // ids are lowercase
        "sonnet  # inline",
    ] {
        assert!(
            !model_is_accepted(bad),
            "`model: {bad}` must be rejected — Claude Code resolves no such value"
        );
    }

    // The same leftovers behind a closing quote. YAML allows a comment there and
    // nothing else, so none of these three lines is a document Claude Code can
    // load — reading the quoted part alone would hand back `sonnet`, `opus` and
    // `claude-opus-5` and certify all three files as compliant.
    for line in [
        "model: \"sonnet\" garbage",
        "model: 'opus' junk here",
        "model: \"claude-opus-5\" papel",
    ] {
        let fm = format!("{line}\n");
        let read = declared(&fm, "model").expect("the line does declare a `model:` key");
        assert!(
            !model_is_accepted(&read),
            "`{line}` is not loadable YAML, and reading past the closing quote laundered it into \
             `{read}` — a value the ratchet then certifies as deliberate"
        );
    }

    // A `#` that is not preceded by whitespace is part of the value in YAML, so
    // the reader must not cut there and quietly produce something acceptable.
    assert_eq!(declared("model: claude-opus#5\n", "model").as_deref(), Some("claude-opus#5"));
    assert!(!model_is_accepted("claude-opus#5"));

    // An effort with an annotation reads as the level, and a misspelled one
    // stays rejected with the annotation stripped.
    assert_eq!(declared("effort: max # top of the scale\n", "effort").as_deref(), Some("max"));
    assert!(effort_is_accepted("max"));
    assert_eq!(declared("effort: highest # nearly\n", "effort").as_deref(), Some("highest"));
    assert!(!effort_is_accepted("highest"));
}
