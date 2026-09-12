// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::map_unwrap_or,
    clippy::uninlined_format_args
)]

//! O gancho de `PostToolUse` roda como processo à parte e sai com código 0
//! diante de uma gravação que não tem nada a ver com a spec: ele está ligado
//! ao despachante e nunca derruba a sessão.
//!
//! O gancho roda numa pasta temporária, que é também o `cwd` que ele recebe.
//! Na pasta do cargo, que fica dentro do checkout, ele gravaria eventos na spec
//! ativa do projeto de verdade e deixaria lá a pasta da sessão de teste, que
//! outros comandos passariam a tomar como a sessão atual.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const SESSION: &str = "test-session-ext";

#[test]
fn amend_capture_dispatcher_exits_zero_inside_a_temp_folder() {
    let bin = env!("CARGO_BIN_EXE_mustard-rt");
    let dir = tempfile::tempdir().expect("tempdir");
    let input = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Write",
        "tool_input": { "file_path": dir.path().join("unrelated.md") },
        "session_id": SESSION,
        "cwd": dir.path()
    });
    let mut child = Command::new(bin)
        .args(["on", "PostToolUse"])
        .current_dir(dir.path())
        .env_remove("CLAUDE_PROJECT_DIR")
        .env_remove("MUSTARD_PROJECT_ROOT")
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
    assert_eq!(status.code(), Some(0), "mustard-rt must exit 0 (fail-open)");

    let checkout_session = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.claude/.session").join(SESSION);
    assert!(
        !checkout_session.exists(),
        "the hook wrote the test session into the real checkout: {}",
        checkout_session.display()
    );
}
