// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::map_unwrap_or,
    clippy::uninlined_format_args
)]

//! Integration test for the `wikilink_footer` hook module.
//!
//! Drives `mustard-rt on PostToolUse` as a subprocess against a `.claude/`
//! fixture and asserts the rendered footer is correct, idempotent, and removed
//! when wikilinks disappear from the body. This is the end-to-end equivalent
//! of the in-module unit tests in `apps/rt/src/hooks/wikilink_footer.rs`.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const FOOTER_START: &str = "<!-- wikilinks-footer-start -->";
const FOOTER_END: &str = "<!-- wikilinks-footer-end -->";

/// Drive `mustard-rt on PostToolUse` with a Write payload pointing at `path`,
/// rooted at `cwd`. Asserts a clean (exit 0) fail-open dispatch.
fn fire_post_write(path: &Path, cwd: &Path) {
    let bin = env!("CARGO_BIN_EXE_mustard-rt");
    let input = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Write",
        "tool_input": { "file_path": path.to_str().unwrap() },
        "session_id": "wikilink-footer-test",
        "cwd": cwd.to_str().unwrap()
    });
    let mut child = Command::new(bin)
        .args(["on", "PostToolUse"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn mustard-rt");
    if let Some(stdin) = child.stdin.take() {
        let mut stdin = stdin;
        let _ = write!(stdin, "{input}");
    }
    let status = child.wait().expect("wait");
    assert_eq!(
        status.code(),
        Some(0),
        "mustard-rt PostToolUse must exit 0 (fail-open)"
    );
}

#[test]
fn wikilink_footer_renders_resolved_and_orphan_links() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    let memory = project.join(".claude").join("memory");
    fs::create_dir_all(&memory).unwrap();

    // `bar.md` resolves; `ghost` does not.
    fs::write(memory.join("bar.md"), "# bar\n").unwrap();
    let foo = memory.join("foo.md");
    fs::write(&foo, "# foo\n\nSee [[bar]] and [[ghost]].\n").unwrap();

    fire_post_write(&foo, project);

    let rendered = fs::read_to_string(&foo).unwrap();
    assert!(
        rendered.contains(FOOTER_START),
        "footer must be present: {rendered}"
    );
    assert!(
        rendered.contains(FOOTER_END),
        "footer end must be present: {rendered}"
    );
    assert!(
        rendered.contains("[bar](bar.md)"),
        "resolved link must render as clickable: {rendered}"
    );
    assert!(
        rendered.contains("⚠ não resolvido"),
        "orphan link must carry the unresolved marker: {rendered}"
    );
}

#[test]
fn wikilink_footer_second_fire_is_no_op() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    let memory = project.join(".claude").join("memory");
    fs::create_dir_all(&memory).unwrap();
    fs::write(memory.join("target.md"), "# target\n").unwrap();
    let foo = memory.join("foo.md");
    fs::write(&foo, "Body referencing [[target]].\n").unwrap();

    fire_post_write(&foo, project);
    let first = fs::read_to_string(&foo).unwrap();

    fire_post_write(&foo, project);
    let second = fs::read_to_string(&foo).unwrap();

    assert_eq!(first, second, "re-fire must produce identical output");
}

#[test]
fn wikilink_footer_strips_block_when_links_disappear() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    let memory = project.join(".claude").join("memory");
    fs::create_dir_all(&memory).unwrap();
    fs::write(memory.join("anchor.md"), "# anchor\n").unwrap();
    let foo = memory.join("foo.md");
    fs::write(&foo, "Body with [[anchor]].\n").unwrap();

    fire_post_write(&foo, project);
    assert!(
        fs::read_to_string(&foo)
            .unwrap()
            .contains(FOOTER_START),
        "footer must be present after first fire"
    );

    // Strip all wikilinks and re-fire.
    fs::write(&foo, "Body without any links.\n").unwrap();
    fire_post_write(&foo, project);

    let stripped = fs::read_to_string(&foo).unwrap();
    assert!(
        !stripped.contains(FOOTER_START),
        "footer must be removed when no wikilinks remain: {stripped}"
    );
}

#[test]
fn wikilink_footer_skips_non_atomic_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    let src = project.join("src");
    fs::create_dir_all(&src).unwrap();
    let other = src.join("README.md");
    let body = "Body with [[ghost]].";
    fs::write(&other, body).unwrap();

    fire_post_write(&other, project);

    // Path is not under .claude/{memory,knowledge,spec}/ — hook is a no-op.
    assert_eq!(body, fs::read_to_string(&other).unwrap());
}
