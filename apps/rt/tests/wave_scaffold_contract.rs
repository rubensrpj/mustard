//! Integration test: the `wave-scaffold` operator contract
//! (`2026-06-10-wave-scaffold-contrato-erro`).
//!
//! Production telemetry (sialia) recorded ≥6 `missing field n` failures in 6
//! days — two of them 1-2 min after the operator read `--help`, which only
//! documented the OPTIONAL body fields and claimed "all `#[serde(default)]`".
//! The parse-error path compounded it: stderr was honest but stdout printed a
//! clean `{"created_files":[],"skipped":[]}` and exited 0, indistinguishable
//! from a successful no-op.
//!
//! Contract under test:
//! - long help declares the REQUIRED per-wave `n`/`role` fields, carries the
//!   minimal plan example, and routes to `plan-materialize`;
//! - an unreadable/unparseable plan exits 2 with an `error` field on stdout
//!   (+ an actionable `hint` for the missing-field case), aligned with the
//!   pre-existing EmptyPlan arm;
//! - `plan-materialize` maps the same unreadable failure to exit 2.

use serde_json::Value;
use std::path::Path;
use std::process::Output;
use tempfile::TempDir;

fn run_rt(cwd: &Path, args: &[&str]) -> Output {
    let bin = env!("CARGO_BIN_EXE_mustard-rt");
    std::process::Command::new(bin)
        .args(args)
        .current_dir(cwd)
        .env("CLAUDE_PROJECT_DIR", cwd.to_string_lossy().as_ref())
        .output()
        .expect("run mustard-rt")
}

fn stdout_json(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout must be JSON ({e}): {}",
            String::from_utf8_lossy(&out.stdout)
        )
    })
}

/// The long help (`--help`) must declare the REQUIRED `n`/`role` fields, show
/// the minimal valid plan, and point at the canonical producers — the old text
/// documented only the optional body with the misleading "all
/// `#[serde(default)]`" phrase, with the example buried in invisible rustdoc.
#[test]
fn wave_scaffold_long_help_declares_required_n_and_role_with_example() {
    let tmp = TempDir::new().expect("tempdir");
    let out = run_rt(tmp.path(), &["run", "wave-scaffold", "--help"]);
    assert!(out.status.success(), "--help must exit 0");
    let help = String::from_utf8_lossy(&out.stdout);

    // Required fields are declared as such, with the folder-name driver.
    assert!(help.contains("REQUIRES"), "required fields not declared:\n{help}");
    assert!(help.contains("`n: u32`"), "n not documented:\n{help}");
    assert!(help.contains("`role: String`"), "role not documented:\n{help}");
    // Spelled with angle brackets in the help — a literal brace-n sequence is
    // a clap help-template token (forced line break).
    assert!(help.contains("wave-<n>-<role>"), "folder driver missing:\n{help}");

    // The minimal valid plan example (formerly rustdoc-only) is in the help.
    assert!(help.contains("\"waves\": ["), "plan example missing:\n{help}");
    assert!(
        help.contains("{ \"n\": 1, \"role\": \"general\""),
        "example wave entry missing:\n{help}"
    );
    assert!(help.contains("\"total_waves\": 2"), "example total missing:\n{help}");

    // Canonical producers are routed.
    assert!(help.contains("plan-materialize"), "pipeline entry missing:\n{help}");

    // The misleading claim that EVERY field is defaulted is gone; only the
    // body fields are.
    assert!(
        !help.contains("optional materialised body, all"),
        "misleading all-serde-default phrase resurfaced:\n{help}"
    );
    assert!(
        help.contains("BODY fields are optional"),
        "the optional-body framing must stay scoped to the body:\n{help}"
    );
}

/// A plan whose wave entry omits the required `n` (the production failure)
/// must exit 2 with `error` + actionable `hint` on stdout — never the old
/// success-shaped `{"created_files":[],"skipped":[]}` + exit 0.
#[test]
fn wave_scaffold_plan_missing_n_reports_error_hint_and_exits_2() {
    let tmp = TempDir::new().expect("tempdir");
    let plan = tmp.path().join("plan.json");
    std::fs::write(
        &plan,
        r#"{"waves":[{"role":"general","summary":"s"}],"total_waves":1}"#,
    )
    .expect("write plan");
    let spec_dir = tmp.path().join("epic");

    let out = run_rt(
        tmp.path(),
        &[
            "run",
            "wave-scaffold",
            "--spec-dir",
            spec_dir.to_str().expect("utf8"),
            "--plan",
            plan.to_str().expect("utf8"),
        ],
    );

    assert_eq!(out.status.code(), Some(2), "missing field must exit 2");
    let json = stdout_json(&out);
    let error = json["error"].as_str().expect("error field on stdout");
    assert!(error.contains("missing field"), "summarised parse error: {error}");
    assert!(error.contains("missing field `n`"), "the missing field is named: {error}");
    let hint = json["hint"].as_str().expect("hint field for missing-field errors");
    assert!(hint.contains("\"n\""), "hint names n: {hint}");
    assert!(hint.contains("\"role\""), "hint names role: {hint}");
    assert!(hint.contains("plan-materialize"), "hint routes to the producer: {hint}");
    assert!(json["created_files"].as_array().expect("array").is_empty());
    // stderr keeps the full prefixed message.
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("plan JSON parse error"), "stderr: {stderr}");
    // Nothing was scaffolded for a plan that failed to parse.
    assert!(!spec_dir.join("wave-plan.md").exists());
}

/// A plan file that cannot be read at all (wrong path) also exits 2 with the
/// `error` field; the missing-field `hint` is NOT attached (different cause,
/// different fix).
#[test]
fn wave_scaffold_unreadable_plan_file_reports_error_and_exits_2() {
    let tmp = TempDir::new().expect("tempdir");
    let out = run_rt(
        tmp.path(),
        &[
            "run",
            "wave-scaffold",
            "--spec-dir",
            tmp.path().join("epic").to_str().expect("utf8"),
            "--plan",
            tmp.path().join("nope.json").to_str().expect("utf8"),
        ],
    );

    assert_eq!(out.status.code(), Some(2), "unreadable plan must exit 2");
    let json = stdout_json(&out);
    let error = json["error"].as_str().expect("error field on stdout");
    // Scrubbed constant: the absolutized (cwd-dependent) plan path and the
    // OS-specific io message stay on stderr — `run` stdout is byte-stable.
    assert_eq!(error, "cannot read plan", "read failure scrubbed on stdout");
    assert!(json.get("hint").is_none(), "no missing-field hint for a read failure");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("cannot read plan"), "stderr keeps the full message: {stderr}");
    assert!(stderr.contains("nope.json"), "stderr carries the path: {stderr}");
}

/// The pre-existing EmptyPlan contract is unchanged: `error` on stdout, exit 2.
#[test]
fn wave_scaffold_empty_plan_contract_unchanged() {
    let tmp = TempDir::new().expect("tempdir");
    let plan = tmp.path().join("plan.json");
    std::fs::write(&plan, r#"{"waves":[]}"#).expect("write plan");

    let out = run_rt(
        tmp.path(),
        &[
            "run",
            "wave-scaffold",
            "--spec-dir",
            tmp.path().join("epic").to_str().expect("utf8"),
            "--plan",
            plan.to_str().expect("utf8"),
        ],
    );

    assert_eq!(out.status.code(), Some(2), "empty plan still exits 2");
    let json = stdout_json(&out);
    assert_eq!(json["error"], Value::String("plan.waves is empty".into()));
}

/// Happy path through the real binary stays exit 0 with no `error`/`hint`
/// keys — the new failure mapping must not leak into success output.
#[test]
fn wave_scaffold_valid_plan_still_exits_0_without_error_field() {
    let tmp = TempDir::new().expect("tempdir");
    let plan = tmp.path().join("plan.json");
    std::fs::write(
        &plan,
        r#"{"waves":[{"n":1,"role":"general","summary":"s","depends_on":[]}],"total_waves":1,"lang":"en-US"}"#,
    )
    .expect("write plan");
    let spec_dir = tmp.path().join("epic");

    let out = run_rt(
        tmp.path(),
        &[
            "run",
            "wave-scaffold",
            "--spec-dir",
            spec_dir.to_str().expect("utf8"),
            "--plan",
            plan.to_str().expect("utf8"),
        ],
    );

    assert_eq!(out.status.code(), Some(0), "valid plan exits 0");
    let json = stdout_json(&out);
    assert!(json.get("error").is_none(), "no error key on success: {json}");
    assert!(json.get("hint").is_none(), "no hint key on success: {json}");
    assert!(!json["created_files"].as_array().expect("array").is_empty());
    assert!(spec_dir.join("wave-plan.md").exists());
}

/// Exit alignment: `plan-materialize` (the preferred pipeline entry) maps the
/// same unreadable-plan failure to exit 2 — it already carried the
/// `scaffold.error` field but exited 0, so the orchestrator never noticed.
#[test]
fn wave_scaffold_alignment_plan_materialize_unreadable_exits_2() {
    let tmp = TempDir::new().expect("tempdir");
    let spec_dir = tmp.path().join(".claude").join("spec").join("ghost");
    std::fs::create_dir_all(&spec_dir).expect("spec dir");

    let out = run_rt(
        tmp.path(),
        &[
            "run",
            "plan-materialize",
            "--spec-dir",
            spec_dir.to_str().expect("utf8"),
            "--plan",
            tmp.path().join("nope.json").to_str().expect("utf8"),
        ],
    );

    assert_eq!(out.status.code(), Some(2), "unreadable plan must exit 2");
    let json = stdout_json(&out);
    assert_eq!(json["scaffold"]["error"], Value::String("plan unreadable".into()));
    assert_eq!(json["events"], serde_json::json!([]), "no events for a failed scaffold");
}
