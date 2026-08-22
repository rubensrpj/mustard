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
fn model_is_accepted(value: &str) -> bool {
    // A model id is ONE token. Without this guard the `claude-` prefix rule
    // certifies anything merely starting with it — `claude-opus-5 lixo`, or a
    // value whose trailing comment survived a parsing slip — so the ratchet
    // would report the file as compliant while holding a value no runtime
    // resolves. A guard that only ever accepts is the decoration this file
    // exists to refuse.
    if value.is_empty() || value.contains(char::is_whitespace) {
        return false;
    }
    MODEL_ALIASES.contains(&value) || value == "inherit" || value.starts_with("claude-")
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

/// Read one frontmatter scalar, dropping a trailing comment.
///
/// YAML opens a comment at a `#` that starts the value or follows whitespace —
/// EXCEPT inside a quoted scalar, where it is literal text. A quoted value must
/// therefore be read to its CLOSING quote. Stopping at the opening one and
/// shaving the rest with `trim_matches` is wrong in BOTH directions, which is
/// how it escaped a first review: `model: "sonnet"  # role` came back as
/// `sonnet"  # role` and was rejected though the runtime resolves it, while
/// `model: "claude-opus-5"  # role` came back as `claude-opus-5"  # role` and
/// was ACCEPTED by the `claude-` prefix rule, certifying a value no runtime
/// resolves.
///
/// An unterminated quote is not a scalar this can read, so it is returned whole
/// and left for the vocabulary check to reject.
fn scalar(value: &str) -> &str {
    let value = value.trim();
    let mut chars = value.char_indices();
    if let Some((_, quote)) = chars.next().filter(|(_, c)| *c == '"' || *c == '\'') {
        return match chars.find(|(_, c)| *c == quote) {
            Some((close, _)) => &value[quote.len_utf8()..close],
            None => value,
        };
    }
    let bytes = value.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'#' && (i == 0 || bytes[i - 1].is_ascii_whitespace()) {
            return value[..i].trim_end();
        }
    }
    value
}

/// The value of a TOP-LEVEL frontmatter key (column 0 only, so a `key:` quoted
/// inside a description is never mistaken for a declaration). Quotes and a
/// trailing comment are removed; `None` when the key is absent.
fn declared(frontmatter: &str, key: &str) -> Option<String> {
    frontmatter
        .lines()
        .filter(|line| !line.starts_with(char::is_whitespace))
        .find_map(|line| line.strip_prefix(key)?.strip_prefix(':'))
        .map(|value| scalar(value).to_string())
        // `model: # todo` declares the key and no value. Reporting that as an
        // unresolvable value renders a diagnostic with a BLANK value in it; the
        // absent-key path instead explains what a missing declaration costs,
        // which is the message this input actually needs.
        .filter(|value| !value.is_empty())
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

/// A declaration is routinely annotated in place, and YAML opens a comment at a
/// `#` that follows whitespace. Reading that comment as part of the value turns
/// a VALID declaration into a reported failure, which teaches the next author to
/// delete their note rather than catching a real mistake — the ratchet would be
/// lying about its own input.
#[test]
fn a_trailing_comment_is_not_part_of_the_value() {
    let annotated = "name: x\nmodel: sonnet   # cheap distillation role\neffort: low\t# fast\n";
    let model = declared(annotated, "model").unwrap_or_default();
    let effort = declared(annotated, "effort").unwrap_or_default();
    assert_eq!(model, "sonnet", "a trailing comment is not part of the value");
    assert_eq!(effort, "low", "a tab before the `#` opens a comment too");
    assert!(model_is_accepted(&model), "`{model}` must still be accepted");
    assert!(effort_is_accepted(&effort), "`{effort}` must still be accepted");

    // A `#` opening the value is a comment as well, so nothing was declared —
    // and `None` is what routes the failure to the message that explains what a
    // missing declaration costs, instead of one quoting a blank value.
    assert_eq!(declared("model: # todo\n", "model"), None);
    assert_eq!(declared("model:\n", "model"), None);

    // And a value carrying no comment is returned untouched.
    assert_eq!(
        declared("model: inherit\n", "model").as_deref(),
        Some("inherit")
    );
}

/// A QUOTED declaration is the case a first pass got wrong in both directions:
/// stopping at the opening quote rejected `"sonnet"  # role`, which the runtime
/// resolves, and accepted `"claude-opus-5"  # role`, which it does not. Both
/// spellings are ordinary YAML, so both are pinned here.
#[test]
fn a_quoted_value_is_read_to_its_closing_quote() {
    for (line, want) in [
        ("model: \"sonnet\"   # cheap role\n", "sonnet"),
        ("model: 'sonnet' # cheap role\n", "sonnet"),
        ("model: \"claude-opus-5\"  # the big one\n", "claude-opus-5"),
        ("model: \"claude-opus-5\"\n", "claude-opus-5"),
        ("model: 'inherit'\n", "inherit"),
    ] {
        let got = declared(line, "model").unwrap_or_default();
        assert_eq!(got, want, "reading {line:?}");
        assert!(model_is_accepted(&got), "`{got}` must be accepted");
    }

    // The mirror: a value that keeps its comment must NOT slip through on the
    // `claude-` prefix rule. A model id is one token.
    for bad in [
        "claude-opus-5\"  # the big one",
        "claude-opus-5 lixo",
        "sonnet   # cheap role",
    ] {
        assert!(
            !model_is_accepted(bad),
            "`{bad}` must be rejected — no runtime resolves it"
        );
    }

    // An unterminated quote is not a scalar we can read: returned whole, then
    // rejected, rather than silently becoming something else.
    assert_eq!(
        declared("model: \"sonnet\n", "model").as_deref(),
        Some("\"sonnet")
    );
}
