// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! A gate that declines without saying why costs a whole run to diagnose.
//!
//! The approval recorder mints `<spec>/.approved-by-user` only when a selected
//! option label carries an approval stem (`approv` / `aprov`). A question whose
//! options read "Sim, pode ir" / "Go ahead" therefore recorded nothing — and
//! said nothing. The author of the question could not learn that the condition
//! existed: it was documented only in the recorder's own source, and the run
//! died later at `approve-spec`, which names the MISSING MARKER but not the
//! reason it is missing.
//!
//! This drives `mustard-rt on PostToolUse` as a subprocess (the same end-to-end
//! shape as `plan_approval_marker.rs`, since `hooks` is private to the lib) and
//! asserts the decline is now explained on stderr, while still recording
//! nothing.
//!
//! A SECOND decline joined it: an answer typed as free text (the harness's
//! `Other` row / notes field) rather than selected from the offered options is
//! not an approval whatever words it contains, and its remedy differs — select
//! the option instead of typing it — so it gets its own explanation. Both
//! declines record nothing; the one thing the recorder ACCEPTS, a genuine
//! selection of an approval-stemmed option, is unchanged.
//!
//! Lives in `tests/` rather than in-file because the acceptance criterion runs
//! `cargo test -p mustard-rt approval_refusal_names_the_unmet_condition --
//! --exact`, and libtest matches `--exact` against the FULL test path — which
//! equals the bare function name only at the root of an integration-test binary.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Drive `mustard-rt on PostToolUse` with an `AskUserQuestion` answer rooted at
/// `cwd`, returning the captured stderr. `offered` are the option labels the
/// question put on the menu (`tool_input`, authored by the model); `answers` is
/// what came back. The recorder requires the answer to be exactly one of the
/// offered labels, so the two are supplied separately — as the harness does.
/// Asserts the clean (exit 0) fail-open dispatch every hook face owes the
/// session.
fn answer_and_capture_stderr(
    cwd: &Path,
    session: &str,
    offered: &[&str],
    answers: serde_json::Value,
) -> String {
    let bin = env!("CARGO_BIN_EXE_mustard-rt");
    let options: Vec<serde_json::Value> = offered
        .iter()
        .map(|l| serde_json::json!({ "label": l }))
        .collect();
    let input = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "AskUserQuestion",
        "tool_input": {
            "questions": [{ "question": "Approve the plan?", "options": options }]
        },
        "tool_response": { "questions": [], "answers": answers },
        "session_id": session,
        "cwd": cwd.to_str().unwrap()
    });
    let mut child = Command::new(bin)
        .args(["on", "PostToolUse"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn mustard-rt");
    if let Some(stdin) = child.stdin.take() {
        let mut stdin = stdin;
        let _ = write!(stdin, "{input}");
    }
    let out = child.wait_with_output().expect("wait");
    assert_eq!(
        out.status.code(),
        Some(0),
        "mustard-rt PostToolUse must exit 0 (fail-open)"
    );
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Seed a Full spec in PLAN (the exact window where an approval is pending) and
/// bind the session to it, so the recorder's facts 1 and 2 both hold.
fn seed_full_plan_spec(project: &Path, session: &str, spec: &str) {
    let spec_dir = project.join(".claude").join("spec").join(spec);
    fs::create_dir_all(&spec_dir).unwrap();
    fs::write(
        spec_dir.join("meta.json"),
        r#"{"scope":"full (wave plan)","stage":"Plan","outcome":"Active"}"#,
    )
    .unwrap();
    let session_dir = project.join(".claude").join(".session").join(session);
    fs::create_dir_all(&session_dir).unwrap();
    fs::write(session_dir.join("active-spec"), spec).unwrap();
}

fn marker(project: &Path, spec: &str) -> std::path::PathBuf {
    project
        .join(".claude")
        .join("spec")
        .join(spec)
        .join(".approved-by-user")
}

#[test]
fn approval_refusal_names_the_unmet_condition() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    seed_full_plan_spec(project, "s-decline", "epic");

    // A genuine SELECTION of an offered option, in words the stem matcher does
    // not recognise.
    let stderr = answer_and_capture_stderr(
        project,
        "s-decline",
        &["Sim, pode ir", "Não, revisar"],
        serde_json::json!({ "Approve the plan?": "Sim, pode ir" }),
    );

    // 1. The condition is NAMED — which spec, which label, which stems.
    assert!(
        stderr.contains("epic"),
        "the refusal must name the spec awaiting approval:\n{stderr}"
    );
    assert!(
        stderr.contains("Sim, pode ir"),
        "the refusal must quote the label that failed the condition:\n{stderr}"
    );
    assert!(
        stderr.contains("approv") && stderr.contains("aprov"),
        "the refusal must name the stems that would satisfy it:\n{stderr}"
    );

    // 2. And the gate is unchanged: an unrecognised answer still records nothing.
    assert!(
        !marker(project, "epic").exists(),
        "explaining the refusal must not weaken it — no marker may be minted"
    );
}

#[test]
fn a_recognised_approval_mints_the_marker_and_explains_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    seed_full_plan_spec(project, "s-ok", "epic");

    let stderr = answer_and_capture_stderr(
        project,
        "s-ok",
        &["Aprovar e implementar agora", "Rejeitar"],
        serde_json::json!({ "Approve the plan?": "Aprovar e implementar agora" }),
    );

    assert!(
        marker(project, "epic").exists(),
        "a recognised approval must still mint the marker"
    );
    assert!(
        !stderr.contains("[approval]"),
        "nothing was declined, so nothing should be explained:\n{stderr}"
    );
}

/// The forged approval, end to end: the user typed their own words instead of
/// picking an option (the harness's `Other` row / notes field), and the text
/// happens to contain an approval word. Nothing is minted, and the operator is
/// told the ONE thing that would have worked — selecting the option.
#[test]
fn free_text_is_declined_and_told_to_select_instead() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    seed_full_plan_spec(project, "s-typed", "epic");

    let stderr = answer_and_capture_stderr(
        project,
        "s-typed",
        &["Aprovar e implementar agora", "Rejeitar"],
        serde_json::json!({
            "Approve the plan?":
                "the run died at approve-spec because nobody could approve the plan"
        }),
    );

    assert!(
        !marker(project, "epic").exists(),
        "free text carrying an approval word must never mint the marker:\n{stderr}"
    );
    assert!(
        stderr.contains("free text") && stderr.contains("SELECTING"),
        "the operator must be told to select the option instead of typing:\n{stderr}"
    );
}

#[test]
fn a_dismissed_dialog_explains_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    seed_full_plan_spec(project, "s-cancel", "epic");

    // An empty answer map is a cancelled dialog: no question was answered, so
    // no condition was failed and there is nothing to tell the author.
    let stderr = answer_and_capture_stderr(project, "s-cancel", &["Aprovar"], serde_json::json!({}));

    assert!(
        !stderr.contains("[approval]"),
        "a dismissed dialog must not be reported as a failed condition:\n{stderr}"
    );
    assert!(!marker(project, "epic").exists());
}
