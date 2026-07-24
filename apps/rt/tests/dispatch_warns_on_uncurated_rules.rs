// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::uninlined_format_args)]

//! A dispatched agent must be TOLD when the `## Guards` block it receives is
//! still the `/scan` scaffold.
//!
//! The pending block's whole body is HTML comments, so a verbatim copy hands the
//! agent an empty rule set that renders exactly like a curated one — the agent
//! cannot tell "this project has no rules yet" from "these are the rules".
//!
//! Two-sided by construction: the uncurated block must carry the notice AND the
//! curated one must be copied byte-for-byte without it. A renderer that always
//! warned would pass a one-sided test while crying wolf on every real dispatch.
//!
//! Lives in `tests/` rather than in-file because the acceptance criterion runs
//! `cargo test -p mustard-rt dispatch_warns_on_uncurated_rules -- --exact`, and
//! libtest matches `--exact` against the FULL test path — which equals the bare
//! function name only at the root of an integration-test binary.

use mustard_rt::commands::agent::render::sections::read_guards_block;
use mustard_rt::commands::scan_claude::{GUARDS_CLOSE, GUARDS_DONE_OPEN, GUARDS_PENDING_OPEN};

#[test]
fn dispatch_warns_on_uncurated_rules() {
    // --- Side 1: the uncurated scaffold is marked. ---
    let pending_dir = tempfile::tempdir().unwrap();
    std::fs::write(
        pending_dir.path().join("CLAUDE.md"),
        format!(
            "# Sub\n\n## Guards\n\n{GUARDS_PENDING_OPEN}\n\
             <!-- facts: kind=cargo; frameworks=serde -->\n{GUARDS_CLOSE}\n"
        ),
    )
    .unwrap();

    let pending = read_guards_block(pending_dir.path());
    assert!(
        pending.starts_with("> NOTE:"),
        "the notice must lead the block — the agent reads top-down: {pending}"
    );
    assert!(
        pending.contains("NO project rules"),
        "the notice must state that the rules are absent: {pending}"
    );
    assert!(
        pending.contains(GUARDS_PENDING_OPEN),
        "the raw block must survive — the notice is a prefix, not a swap: {pending}"
    );

    // --- Side 2: a curated block is copied verbatim, with no notice. ---
    let curated_dir = tempfile::tempdir().unwrap();
    let body = format!(
        "{GUARDS_DONE_OPEN}\n<!-- facts: kind=cargo; frameworks=serde -->\n\
         - DO keep every hook fail-open\n- DON'T block the session on your own error\n{GUARDS_CLOSE}"
    );
    std::fs::write(
        curated_dir.path().join("CLAUDE.md"),
        format!("# Sub\n\n## Guards\n\n{body}\n\n## Stack\nrust\n"),
    )
    .unwrap();

    let curated = read_guards_block(curated_dir.path());
    assert_eq!(
        curated, body,
        "a curated block must reach the agent byte-for-byte, with no notice"
    );

    // --- Fail-open (inject contract): a missing source yields no injection. ---
    let empty_dir = tempfile::tempdir().unwrap();
    assert_eq!(
        read_guards_block(empty_dir.path()),
        "",
        "no CLAUDE.md ⇒ no injection at all — never a bare notice"
    );
}
