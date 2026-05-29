// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::map_unwrap_or,
    clippy::uninlined_format_args
)]

//! Regression test: scan cold-path opt-in gate (F1-c — functional-refactor).
//!
//! F1-c flips the cold-path LLM (`scan/interpret`) to an **opt-in, default-OFF**
//! fallback behind `MUSTARD_SCAN_LLM`:
//!
//! - **Default path** (gate unset): `sync-registry` spawns **no** `claude`
//!   subprocess. Entities come from the deterministic structural extractor, and
//!   a `pipeline.economy.savings.scan-structural-extract` event is emitted into
//!   the `.events` sink recording the round-trip that was avoided.
//! - **Gate ON + zero-structural subproject**: the model runs only as a
//!   COMPLEMENT. With no `claude` on PATH it fails open (exit 0, no crash).
//!
//! Cross-platform: on Windows the fake binary is a `.cmd` file; on Unix a
//! shell script with a shebang. Both echo a canned JSON interpretation and
//! exit 0.

use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Write the fake `claude` binary into `bin_dir` and return its path.
///
/// The fake binary prints a canned JSON response that `interpret_with` can
/// parse, then exits 0. The response contains one entity so we can assert
/// it surfaced in `entity-registry.json`.
fn write_fake_claude(bin_dir: &Path) -> PathBuf {
    // The canned response — a minimal interpretation JSON.
    let json = r#"{"entities":[{"name":"ColdPathEntity","file":"src/lib.rs","edges":[]}],"enums":[],"patternsOverlay":{}}"#;

    #[cfg(windows)]
    {
        // On Windows, a .cmd file is invoked via `cmd /C` so the shell resolves
        // the .cmd extension. We write the JSON response to a side-car file and
        // use `type` to output it — this avoids the cmd.exe batch-file `"`
        // stripping bug that breaks `echo {json}` when json contains double
        // quotes (cmd's batch parser removes them before passing to echo).
        let response_file = bin_dir.join("claude_response.json");
        std::fs::write(&response_file, json).expect("write claude_response.json");
        let path = bin_dir.join("claude.cmd");
        // Use `type` to output the response file verbatim (preserves all chars
        // including `"`) then exit 0. The response file path uses a %~dp0
        // reference so the .cmd works regardless of cwd.
        let content = "@echo off\r\ntype \"%~dp0claude_response.json\"\r\n".to_string();
        std::fs::write(&path, content).expect("write fake claude.cmd");
        path
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let path = bin_dir.join("claude");
        let content = format!("#!/bin/sh\nprintf '%s' '{json}'\n");
        std::fs::write(&path, &content).expect("write fake claude");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake claude");
        path
    }
}

/// Build a minimal project layout that `sync-registry` can scan with a Rust
/// source file declaring a struct — the deterministic structural extractor
/// recovers it offline, no `claude` required.
fn setup_project(project: &Path) {
    std::fs::create_dir_all(project.join(".claude")).expect("create .claude");
    std::fs::create_dir_all(project.join("src")).expect("create src");
    std::fs::write(
        project.join("src/lib.rs"),
        "pub struct StructEntity { pub id: i64 }\n",
    )
    .expect("write src/lib.rs");
    std::fs::write(
        project.join("Cargo.toml"),
        "[package]\nname = \"fake-project\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write Cargo.toml");
}

/// Prepend `dir` to the current `PATH`, returning the modified value.
fn prepend_path(dir: &Path) -> String {
    let sep = if cfg!(windows) { ";" } else { ":" };
    let existing = std::env::var("PATH").unwrap_or_default();
    format!("{}{sep}{existing}", dir.display())
}

/// Recursively collect every `.ndjson` line under `<project>/.claude`.
fn collect_ndjson_lines(project: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let root = project.join(".claude");
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("ndjson") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    out.extend(content.lines().map(str::to_string));
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// F1-c default path: with `MUSTARD_SCAN_LLM` UNSET, `sync-registry` must
///
///   1. exit 0,
///   2. recover entities from the structural extractor (`StructEntity`),
///   3. NOT invoke the fake `claude` (its `ColdPathEntity` must be ABSENT —
///      proving no subprocess ran), and
///   4. emit a `pipeline.economy.savings.scan-structural-extract` event into
///      the `.events` sink.
///
/// The fake binary IS on PATH; the gate being OFF is what keeps it unspawned.
#[test]
fn scan_default_off_emits_savings_and_skips_claude() {
    let bin_dir = tempfile::tempdir().expect("bin tempdir");
    let project_dir = tempfile::tempdir().expect("project tempdir");

    write_fake_claude(bin_dir.path());
    setup_project(project_dir.path());

    let bin = env!("CARGO_BIN_EXE_mustard-rt");
    let new_path = prepend_path(bin_dir.path());

    let output = Command::new(bin)
        .args(["run", "sync-registry", "--force"])
        .current_dir(project_dir.path())
        .env("PATH", &new_path)
        .env("CLAUDE_PROJECT_DIR", project_dir.path().to_string_lossy().as_ref())
        .env("MUSTARD_INTERPRET_CACHE", "off")
        // MUSTARD_SCAN_LLM intentionally UNSET → default-OFF.
        .env_remove("MUSTARD_SCAN_LLM")
        .output()
        .expect("run mustard-rt");

    assert!(
        output.status.success(),
        "mustard-rt run sync-registry exited {:?}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let registry_path = project_dir.path().join(".claude").join("entity-registry.json");
    assert!(registry_path.exists(), "entity-registry.json not written");
    let raw = std::fs::read_to_string(&registry_path).expect("read registry");

    // Structural extraction recovered the struct without any `claude` call.
    assert!(
        raw.contains("StructEntity"),
        "structural entity missing from registry:\n{raw}"
    );
    // The fake binary's entity must be ABSENT — proof no subprocess ran.
    assert!(
        !raw.contains("ColdPathEntity"),
        "default-OFF path must NOT spawn claude — found cold-path entity:\n{raw}"
    );

    // A savings event must have landed in the `.events` sink.
    let lines = collect_ndjson_lines(project_dir.path());
    let has_savings = lines.iter().any(|l| {
        l.contains("pipeline.economy.savings.scan-structural-extract")
            && l.contains("\"tokens_saved\"")
    });
    assert!(
        has_savings,
        "expected a scan-structural-extract savings event in .events; lines:\n{}",
        lines.join("\n")
    );
}

/// F1-c complement path: with `MUSTARD_SCAN_LLM=1` AND a subproject the
/// structural extractor leaves EMPTY (no source declarations), `sync-registry`
/// attempts the model complement. With the fake binary on PATH it surfaces the
/// cold-path entity; either way the run must exit 0 (fail-open).
#[test]
fn scan_gate_on_zero_structural_runs_complement() {
    let bin_dir = tempfile::tempdir().expect("bin tempdir");
    let project_dir = tempfile::tempdir().expect("project tempdir");

    write_fake_claude(bin_dir.path());
    // Empty-structural fixture: a manifest + a non-declaration file so the
    // visitor finds a file but the extractor recovers ZERO entities.
    std::fs::create_dir_all(project_dir.path().join(".claude")).expect("create .claude");
    std::fs::create_dir_all(project_dir.path().join("src")).expect("create src");
    std::fs::write(
        project_dir.path().join("Cargo.toml"),
        "[package]\nname = \"empty-proj\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write Cargo.toml");
    // A file with no struct/enum/class declaration → structural floor is empty.
    std::fs::write(
        project_dir.path().join("src/lib.rs"),
        "pub fn add(a: i64, b: i64) -> i64 { a + b }\n",
    )
    .expect("write src/lib.rs");

    let bin = env!("CARGO_BIN_EXE_mustard-rt");
    let new_path = prepend_path(bin_dir.path());

    let output = Command::new(bin)
        .args(["run", "sync-registry", "--force"])
        .current_dir(project_dir.path())
        .env("PATH", &new_path)
        .env("CLAUDE_PROJECT_DIR", project_dir.path().to_string_lossy().as_ref())
        .env("MUSTARD_INTERPRET_CACHE", "off")
        .env("MUSTARD_SCAN_LLM", "1")
        .output()
        .expect("run mustard-rt");

    assert!(
        output.status.success(),
        "mustard-rt must exit 0 (fail-open)\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let registry_path = project_dir.path().join(".claude").join("entity-registry.json");
    assert!(registry_path.exists(), "entity-registry.json not written");
}

/// Fail-open: gate ON, zero-structural subproject, but NO `claude` on PATH.
/// `sync-registry` must still exit 0 and write a registry — the absent binary
/// degrades to the empty floor, never a crash.
#[test]
fn scan_gate_on_absent_binary_exits_zero() {
    let project_dir = tempfile::tempdir().expect("project tempdir");
    std::fs::create_dir_all(project_dir.path().join(".claude")).expect("create .claude");
    std::fs::create_dir_all(project_dir.path().join("src")).expect("create src");
    std::fs::write(
        project_dir.path().join("Cargo.toml"),
        "[package]\nname = \"empty-proj\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write Cargo.toml");
    std::fs::write(
        project_dir.path().join("src/lib.rs"),
        "pub fn noop() {}\n",
    )
    .expect("write src/lib.rs");

    let bin = env!("CARGO_BIN_EXE_mustard-rt");

    // Isolate PATH so no real `claude` is found even on a dev host.
    let empty_bin_dir = tempfile::tempdir().expect("empty bin dir");
    let isolated_path = empty_bin_dir.path().to_string_lossy().into_owned();

    let output = Command::new(bin)
        .args(["run", "sync-registry", "--force"])
        .current_dir(project_dir.path())
        .env("PATH", &isolated_path)
        .env("CLAUDE_PROJECT_DIR", project_dir.path().to_string_lossy().as_ref())
        .env("MUSTARD_INTERPRET_CACHE", "off")
        .env("MUSTARD_SCAN_LLM", "1")
        .output()
        .expect("run mustard-rt");

    assert!(
        output.status.success(),
        "mustard-rt must exit 0 even when claude is absent\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
