//! Locks the SHIPPED `plugin/commands/*.md` surface — which files are DOORS,
//! and the frontmatter keys that decide how each one is reached.
//!
//! These keys are read by Claude Code itself, not by Mustard, so nothing in
//! this repository fails to compile when one is dropped, renamed or added to
//! the wrong file — the behaviour just changes silently at runtime for every
//! installation. That is the same failure mode `run_command_surface.rs` guards
//! for the CLI surface, and this test is the same kind of ratchet for the
//! instruction surface.
//!
//! What is locked, and why each one matters:
//!
//! - **The surface is exactly four doors** — `git`, `pr`, `spec`, `upsert`. A
//!   door is a command file the USER types; everything else in the directory is
//!   an internal flow the router dispatches, which says so with
//!   `user-invocable: false`. The reduction from fifteen doors to four was the
//!   point of a whole work unit, and it is one careless frontmatter block away
//!   from being undone: a new command file that simply forgets the key
//!   re-exposes itself, and nobody notices. This test is that notice.
//! - `upsert` is deliberately NOT `disable-model-invocation`, and the absence is
//!   asserted so a later "let's finish the list" change has to argue with it.
//!   Its own description defines an automatic trigger — it is the bootstrap
//!   door, reached when another `/mustard:*` command is blocked because Mustard
//!   is not installed. Disabling model invocation would close exactly the door
//!   that has to open without the user knowing the command's name.
//! - `pr` carries NO fork key at all. It runs the QA gate, whose whole contract
//!   is an OBSERVED exit code; a summary relayed from a forked subagent is
//!   second-hand evidence of the one gate the pipeline treats as final. The
//!   assertion moved here with the gate — QA stopped being a door of its own and
//!   became step 3a of this one — and it is the assertion most likely to be
//!   "helpfully" broken later, which is why it is still here.

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The whole exposed surface: the commands a user may type. Kept sorted.
const DOORS: &[&str] = &["git", "pr", "spec", "upsert"];

/// The marker an internal flow carries to stay OUT of the door surface.
const NOT_A_DOOR: &str = "user-invocable: false";

/// Read the frontmatter block (between the first two `---` fences) of a shipped
/// command file. Panics with the path when the file or the block is missing —
/// in a ratchet, an unreadable input is a failure, never a silent pass.
fn frontmatter(command: &str) -> String {
    frontmatter_of(&repo_root().join(format!("plugin/commands/{command}.md")))
}

/// [`frontmatter`] by path, for the walk that does not know the names yet.
fn frontmatter_of(path: &Path) -> String {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("shipped command {} is unreadable: {e}", path.display()));
    let body = text
        .strip_prefix("---")
        .unwrap_or_else(|| panic!("{} must open with a frontmatter fence", path.display()));
    let end = body
        .find("\n---")
        .unwrap_or_else(|| panic!("{} has no closing frontmatter fence", path.display()));
    body[..end].to_string()
}

/// Every `plugin/commands/*.md` stem, sorted — the shipped command files.
fn shipped_commands() -> Vec<String> {
    let dir = repo_root().join("plugin/commands");
    let entries = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("shipped command dir {} is unreadable: {e}", dir.display()));
    let mut names: Vec<String> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
        .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(str::to_string))
        .collect();
    names.sort();
    names
}

#[test]
fn exposed_doors_are_exactly_the_four() {
    let mut exposed: Vec<String> = Vec::new();
    for name in shipped_commands() {
        if !frontmatter(&name).contains(NOT_A_DOOR) {
            exposed.push(name);
        }
    }
    let expected: Vec<String> = DOORS.iter().map(|s| (*s).to_string()).collect();
    assert_eq!(
        exposed, expected,
        "the user-invocable surface must stay exactly {expected:?}. A command file with no \
         `{NOT_A_DOOR}` in its frontmatter IS a door — the user sees it and types it. Everything \
         that is not one of the four is a flow the router dispatches: add the key, or fold the \
         command into the door that already owns its subject (review/QA/close -> pr; off/on/doctor \
         -> upsert; the census refresh -> the base gate; cancelling a unit -> git delete)."
    );
}

#[test]
fn command_frontmatter_internal_flows_are_not_model_invocable_doors() {
    for name in shipped_commands() {
        if DOORS.contains(&name.as_str()) {
            continue;
        }
        let fm = frontmatter(&name);
        assert!(
            fm.contains(NOT_A_DOOR),
            "plugin/commands/{name}.md is not one of the four doors, so it must declare \
             `{NOT_A_DOOR}` — it is dispatched by the router, never typed. Frontmatter:\n{fm}"
        );
    }
}

#[test]
fn command_frontmatter_upsert_stays_the_bootstrap_door() {
    let fm = frontmatter("upsert");
    assert!(
        !fm.contains("disable-model-invocation"),
        "plugin/commands/upsert.md must stay model-invocable. It is reached when another \
         /mustard:* command is blocked because Mustard is not installed — a trigger the user \
         cannot be expected to know by name. If this must change, change the command's \
         description first. Frontmatter:\n{fm}"
    );
}

#[test]
fn command_frontmatter_pr_is_never_forked() {
    let fm = frontmatter("pr");
    for key in ["context: fork", "context:fork", "agent:", "background:"] {
        assert!(
            !fm.contains(key),
            "plugin/commands/pr.md must NOT declare `{key}`. Its merge step runs the QA gate, \
             whose contract is an OBSERVED exit code; a forked run reports it second-hand, and \
             this gate is the one the pipeline treats as final. Frontmatter:\n{fm}"
        );
    }
}
