//! Locks the frontmatter of the SHIPPED `plugin/commands/*.md` surface.
//!
//! Three of these keys are read by Claude Code itself, not by Mustard, so
//! nothing in this repository fails to compile when one is dropped, renamed or
//! added to the wrong file — the behaviour just changes silently at runtime for
//! every installation. That is the same failure mode `run_command_surface.rs`
//! guards for the CLI surface, and this test is the same kind of ratchet for
//! the instruction surface.
//!
//! What is locked, and why each one matters:
//!
//! - The five utility commands declare `disable-model-invocation: true`, so the
//!   model stops considering them for automatic invocation. They are things the
//!   user runs.
//! - `upsert` is deliberately NOT in that set, and this test asserts the
//!   absence so a later "let's finish the list" change has to argue with it.
//!   Its own description defines an automatic trigger — it is the bootstrap
//!   door, reached when another `/mustard:*` command is blocked because Mustard
//!   is not installed. Disabling model invocation would close exactly the door
//!   that has to open without the user knowing the command's name.
//! - `review` runs in a forked subagent (`context: fork`) and waits for its
//!   result in the same turn (`background: false`), because a pull-request
//!   review reads a large diff the main window does not need to keep.
//! - `qa` carries NO fork key at all. Its whole contract is an OBSERVED exit
//!   code; a summary relayed from a subagent is second-hand evidence of the one
//!   gate the pipeline treats as final. This is the assertion most likely to be
//!   "helpfully" broken later, which is why it is here.

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The commands the user invokes and the model should not trigger on its own.
const UTILITIES: &[&str] = &["knowledge", "maint", "skills", "stats", "status"];

/// Read the frontmatter block (between the first two `---` fences) of a shipped
/// command file. Panics with the path when the file or the block is missing —
/// in a ratchet, an unreadable input is a failure, never a silent pass.
fn frontmatter(command: &str) -> String {
    let path = repo_root().join(format!("plugin/commands/{command}.md"));
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("shipped command {} is unreadable: {e}", path.display()));
    let body = text
        .strip_prefix("---")
        .unwrap_or_else(|| panic!("{} must open with a frontmatter fence", path.display()));
    let end = body
        .find("\n---")
        .unwrap_or_else(|| panic!("{} has no closing frontmatter fence", path.display()));
    body[..end].to_string()
}

#[test]
fn command_frontmatter_utilities_are_not_model_invocable() {
    for cmd in UTILITIES {
        let fm = frontmatter(cmd);
        assert!(
            fm.contains("disable-model-invocation: true"),
            "plugin/commands/{cmd}.md must declare `disable-model-invocation: true` — \
             it is a command the user runs, not one the model triggers. Frontmatter:\n{fm}"
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
fn command_frontmatter_review_runs_forked_and_waits() {
    let fm = frontmatter("review");
    for key in ["context: fork", "agent:", "background: false"] {
        assert!(
            fm.contains(key),
            "plugin/commands/review.md must declare `{key}` — a pull-request review reads a \
             large diff that does not belong in the main window, and its result is needed in \
             the same turn. Frontmatter:\n{fm}"
        );
    }
}

#[test]
fn command_frontmatter_qa_is_never_forked() {
    let fm = frontmatter("qa");
    for key in ["context: fork", "context:fork", "agent:", "background:"] {
        assert!(
            !fm.contains(key),
            "plugin/commands/qa.md must NOT declare `{key}`. The QA gate's contract is an \
             OBSERVED exit code; a forked run reports it second-hand, and this gate is the one \
             the pipeline treats as final. Frontmatter:\n{fm}"
        );
    }
}
