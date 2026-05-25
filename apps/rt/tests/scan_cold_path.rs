// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::map_unwrap_or,
    clippy::uninlined_format_args
)]

//! Regression test: scan cold-path (W2 — mustard-unification).
//!
//! Injects a fake `claude` CLI binary onto PATH, drives
//! `mustard-rt run sync-registry` against a minimal project, and asserts that
//! the binary's stdout is consumed and produces non-empty entities in
//! `entity-registry.json`.
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
    let json = r#"{"entities":[{"name":"FakeEntity","file":"src/lib.rs","edges":[]}],"enums":[],"patternsOverlay":{}}"#;

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

/// Build a minimal project layout that `sync-registry` can scan:
/// a tiny Rust-looking source file so the scanner finds at least one file.
fn setup_project(project: &Path) {
    std::fs::create_dir_all(project.join(".claude")).expect("create .claude");
    std::fs::create_dir_all(project.join("src")).expect("create src");
    std::fs::write(project.join("src/lib.rs"), "pub struct FakeEntity { pub id: i64 }\n")
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// AC-W2-4: the fake `claude` binary is invoked during `sync-registry` and
/// its output surfaces as non-empty `entities[]` in `entity-registry.json`.
///
/// Note: `sync-registry` only calls `interpret` on a cache miss (i.e., when
/// the cluster cache is absent or stale). The temp project has no
/// `.cluster-cache.json`, so it always hits the cold path on the first run.
#[test]
fn scan_cold_path_uses_fake_binary() {
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
        // Disable interpret cache so every run hits the cold path.
        .env("MUSTARD_INTERPRET_CACHE", "off")
        .output()
        .expect("run mustard-rt");

    // sync-registry must exit 0 (fail-open contract).
    assert!(
        output.status.success(),
        "mustard-rt run sync-registry exited {:?}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // entity-registry.json must exist and contain the FakeEntity from the
    // fake claude binary's response.
    let registry_path = project_dir.path().join(".claude").join("entity-registry.json");
    assert!(
        registry_path.exists(),
        "entity-registry.json not written"
    );
    let raw = std::fs::read_to_string(&registry_path).expect("read registry");
    assert!(
        raw.contains("FakeEntity"),
        "FakeEntity from fake claude response not found in registry:\n{raw}"
    );
}

/// When the fake binary is NOT on PATH (no `claude` anywhere), interpret fails
/// open — `sync-registry` still exits 0 and writes a registry (possibly with
/// empty entities[], but no crash).
#[test]
fn scan_cold_path_absent_binary_exits_zero() {
    let project_dir = tempfile::tempdir().expect("project tempdir");
    setup_project(project_dir.path());

    let bin = env!("CARGO_BIN_EXE_mustard-rt");

    // Override PATH to a temp dir that has no `claude` binary — ensures the
    // probe fails cleanly even when a real `claude` is installed on the host.
    let empty_bin_dir = tempfile::tempdir().expect("empty bin dir");
    let isolated_path = empty_bin_dir.path().to_string_lossy().into_owned();

    let output = Command::new(bin)
        .args(["run", "sync-registry", "--force"])
        .current_dir(project_dir.path())
        .env("PATH", &isolated_path)
        .env("CLAUDE_PROJECT_DIR", project_dir.path().to_string_lossy().as_ref())
        .env("MUSTARD_INTERPRET_CACHE", "off")
        .output()
        .expect("run mustard-rt");

    assert!(
        output.status.success(),
        "mustard-rt must exit 0 even when claude is absent\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
