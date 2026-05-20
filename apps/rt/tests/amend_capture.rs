//! Integration smoke test for the `amend_capture` hook module.
//!
//! AC-3 … AC-10 are tested as unit tests inside
//! `apps/rt/src/hooks/amend_capture.rs` (the `#[cfg(test)]` block), where
//! `crate::run` and `crate::util` resolve correctly. This file provides a
//! complementary external round-trip: it drives `mustard-rt on PostToolUse`
//! as a subprocess and asserts the exit code is 0 (fail-open / no crash),
//! confirming the module is wired into the dispatcher and the binary builds.

use std::io::Write;
use std::process::{Command, Stdio};

/// `mustard-rt on PostToolUse` with amend-neutral input must exit 0.
#[test]
fn amend_capture_dispatcher_exits_zero() {
    let bin = env!("CARGO_BIN_EXE_mustard-rt");
    let input = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Write",
        "tool_input": { "file_path": "/tmp/unrelated.md" },
        "session_id": "test-session-ext",
        "cwd": "."
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
        let _ = write!(stdin, "{}", input);
    }
    let status = child.wait().expect("wait");
    assert_eq!(status.code(), Some(0), "mustard-rt must exit 0 (fail-open)");
}
