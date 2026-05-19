//! `mustard-rt run verify-pipeline` — a port of `scripts/verify-pipeline.js`.
//!
//! Runs build/test verification for the active pipeline: discovers the
//! subprojects (via `mustard-rt run sync-detect`), runs each subproject's
//! build + test command, and reports pass/fail/skip per subproject.
//!
//! Port note: the JS version shelled to `sync-detect.js`. This port shells to
//! the binary's own `run sync-detect` face (`current_exe()`), parses the same
//! JSON, and falls back to scanning `pipeline-config.md` exactly as the JS did.
//!
//! Fail-open: any discovery error degrades to the defaults probe. Exit `1`
//! when any verification command fails, `0` otherwise (the JS contract).
//!
//! `--format json` (default) prints `{ passed, failed, skipped, timestamp }`.
//! `--format html` additionally writes a standalone HTML report and prints its
//! path on stderr.

use crate::report::{table, Report};
use crate::util::now_iso8601;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Per-command timeout (2 min), matching `verify-pipeline.js`.
const CMD_TIMEOUT_SECS: u64 = 120;

/// A discovered subproject's verification commands.
struct VerifyTarget {
    name: String,
    cwd: PathBuf,
    build: Option<String>,
    test: Option<String>,
}

/// Discover targets via `mustard-rt run sync-detect`.
///
/// Under `cfg(test)` this is a no-op: `current_exe()` would resolve to the
/// libtest binary, and spawning it with `run sync-detect` re-runs the whole
/// suite (a fork bomb). Production builds spawn the real `mustard-rt`.
fn discover_via_sync_detect(cwd: &Path) -> Vec<VerifyTarget> {
    if cfg!(test) {
        return Vec::new();
    }
    let Ok(exe) = std::env::current_exe() else {
        return Vec::new();
    };
    let output = Command::new(exe)
        .args(["run", "sync-detect"])
        .current_dir(cwd)
        .output();
    let Ok(out) = output else {
        return Vec::new();
    };
    let Ok(text) = String::from_utf8(out.stdout) else {
        return Vec::new();
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    let subprojects = parsed
        .get("subprojects")
        .or_else(|| parsed.get("projects"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for (i, sp) in subprojects.iter().enumerate() {
        let build = sp
            .get("buildCommand")
            .or_else(|| sp.get("validateCommand"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let test = sp
            .get("testCommand")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        if build.is_none() && test.is_none() {
            continue;
        }
        let name = sp
            .get("name")
            .or_else(|| sp.get("path"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("subproject-{i}"));
        let sp_cwd = sp
            .get("path")
            .and_then(Value::as_str)
            .map(|p| cwd.join(p))
            .unwrap_or_else(|| cwd.to_path_buf());
        out.push(VerifyTarget { name, cwd: sp_cwd, build, test });
    }
    out
}

/// Fallback: scan `pipeline-config.md` for a Build Command column.
fn discover_via_config(cwd: &Path) -> Vec<VerifyTarget> {
    let config_path = cwd.join(".claude").join("pipeline-config.md");
    let Ok(config) = std::fs::read_to_string(&config_path) else {
        return Vec::new();
    };
    let lines: Vec<&str> = config.split('\n').collect();
    // Find the header row containing "Build".
    let mut header_idx = None;
    let mut build_col = None;
    for (i, line) in lines.iter().enumerate() {
        if line.contains('|') && line.to_lowercase().contains("build") {
            let cols: Vec<&str> = line.split('|').map(str::trim).collect();
            for (ci, col) in cols.iter().enumerate() {
                if col.to_lowercase().contains("build") {
                    build_col = Some(ci);
                    break;
                }
            }
            header_idx = Some(i);
            break;
        }
    }
    let (Some(hi), Some(bc)) = (header_idx, build_col) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    // Skip the header + separator row.
    for line in lines.iter().skip(hi + 2) {
        if !line.starts_with('|') {
            break;
        }
        let cols: Vec<&str> = line.split('|').map(str::trim).collect();
        let name = cols.get(1).copied().unwrap_or("").to_string();
        let build_cmd = cols.get(bc).copied().unwrap_or("");
        if !build_cmd.is_empty() && build_cmd != "-" && build_cmd != "N/A" {
            out.push(VerifyTarget {
                name,
                cwd: cwd.to_path_buf(),
                build: Some(build_cmd.replace('`', "")),
                test: None,
            });
        }
    }
    out
}

/// Last-resort defaults probe — `npm test` if a `package.json` exists,
/// `dotnet build` if a `.csproj` exists.
fn discover_defaults(cwd: &Path) -> Vec<VerifyTarget> {
    let Ok(entries) = std::fs::read_dir(cwd) else {
        return Vec::new();
    };
    let names: Vec<String> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    if names.iter().any(|n| n == "package.json") {
        return vec![VerifyTarget {
            name: "default".to_string(),
            cwd: cwd.to_path_buf(),
            build: Some("npm test".to_string()),
            test: None,
        }];
    }
    if names.iter().any(|n| n.ends_with(".csproj")) {
        return vec![VerifyTarget {
            name: "default".to_string(),
            cwd: cwd.to_path_buf(),
            build: Some("dotnet build".to_string()),
            test: None,
        }];
    }
    Vec::new()
}

/// Build the platform shell invocation for a verification `command` string.
///
/// Verification commands come from `sync-detect`, the `pipeline-config.md`
/// table, or the defaults probe — arbitrary strings that may carry quotes or
/// shell operators. On Windows, `cmd.exe` does not parse its command line via
/// the `CommandLineToArgvW` rules that `std`'s `Command::arg` quoting assumes,
/// so the command is appended verbatim with `CommandExt::raw_arg` (a SAFE API)
/// and run as `cmd /S /C "<command>"`, mirroring `qa_run::build_shell_command`.
#[cfg(windows)]
fn build_shell_command(command: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let mut c = Command::new("cmd");
    c.raw_arg(format!("/S /C \"{command}\""));
    c
}

/// See the `#[cfg(windows)]` variant for the rationale.
#[cfg(not(windows))]
fn build_shell_command(command: &str) -> Command {
    let mut c = Command::new("sh");
    c.arg("-c").arg(command);
    c
}

/// Run one shell command in `cwd` with a timeout. Returns `Ok` on exit 0,
/// `Err(excerpt)` otherwise.
fn run_command(command: &str, cwd: &Path) -> std::result::Result<(), String> {
    let mut cmd = build_shell_command(command);
    cmd.current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(CMD_TIMEOUT_SECS);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = child.wait_with_output().ok();
                if status.success() {
                    return Ok(());
                }
                let excerpt: String = out
                    .map(|o| {
                        let s = String::from_utf8_lossy(&o.stderr).to_string();
                        if s.trim().is_empty() {
                            String::from_utf8_lossy(&o.stdout).to_string()
                        } else {
                            s
                        }
                    })
                    .unwrap_or_default()
                    .chars()
                    .take(500)
                    .collect();
                return Err(excerpt);
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("timeout after {}ms", CMD_TIMEOUT_SECS * 1000));
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// The verification result, JSON-shaped exactly as `verify-pipeline.js`.
struct VerifyResult {
    passed: Vec<String>,
    failed: Vec<Value>,
    skipped: Vec<String>,
    timestamp: String,
}

impl VerifyResult {
    fn to_json(&self) -> Value {
        json!({
            "passed": self.passed,
            "failed": self.failed,
            "skipped": self.skipped,
            "timestamp": self.timestamp,
        })
    }
}

/// Discover verification targets — sync-detect, then the `pipeline-config.md`
/// fallback, then the defaults probe. Kept separate from [`verify_targets`] so
/// tests can drive verification without the `sync-detect` subprocess spawn.
fn discover_targets(cwd: &Path) -> Vec<VerifyTarget> {
    let mut targets = discover_via_sync_detect(cwd);
    if targets.is_empty() {
        targets = discover_via_config(cwd);
    }
    if targets.is_empty() {
        targets = discover_defaults(cwd);
    }
    targets
}

/// Run verification across all discovered targets.
fn verify(cwd: &Path) -> VerifyResult {
    verify_targets(&discover_targets(cwd))
}

/// Run verification across an explicit target list.
fn verify_targets(targets: &[VerifyTarget]) -> VerifyResult {
    let mut result = VerifyResult {
        passed: Vec::new(),
        failed: Vec::new(),
        skipped: Vec::new(),
        timestamp: now_iso8601(),
    };
    for target in targets {
        let cmds: Vec<&String> = [&target.build, &target.test]
            .into_iter()
            .filter_map(Option::as_ref)
            .collect();
        if cmds.is_empty() {
            result.skipped.push(target.name.clone());
            continue;
        }
        let mut all_passed = true;
        for cmd in cmds {
            if let Err(excerpt) = run_command(cmd, &target.cwd) {
                all_passed = false;
                result.failed.push(json!({
                    "name": target.name,
                    "command": cmd,
                    "error": excerpt,
                }));
            }
        }
        if all_passed {
            result.passed.push(target.name.clone());
        }
    }
    result
}

/// Write the standalone HTML report.
fn write_html_report(cwd: &Path, result: &VerifyResult) -> Option<PathBuf> {
    let dir = cwd.join(".claude").join(".qa-reports");
    std::fs::create_dir_all(&dir).ok()?;
    let mut report = Report::new(
        "Pipeline Verification",
        format!(
            "{} passed · {} failed · {} skipped",
            result.passed.len(),
            result.failed.len(),
            result.skipped.len()
        ),
    );
    let mut rows: Vec<Vec<String>> = Vec::new();
    for p in &result.passed {
        rows.push(vec![p.clone(), "PASS".to_string(), String::new()]);
    }
    for f in &result.failed {
        rows.push(vec![
            f.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
            "FAIL".to_string(),
            f.get("error").and_then(Value::as_str).unwrap_or("").chars().take(120).collect(),
        ]);
    }
    for s in &result.skipped {
        rows.push(vec![s.clone(), "SKIP".to_string(), String::new()]);
    }
    report.section("Targets", &table(&["Target", "Status", "Detail"], &rows));
    let path = dir.join("verify-pipeline.html");
    std::fs::write(&path, report.render()).ok()?;
    Some(path)
}

/// Dispatch `mustard-rt run verify-pipeline`.
pub fn run(format: &str) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let result = verify(&cwd);

    if format == "html" {
        match write_html_report(&cwd, &result) {
            Some(path) => eprintln!("[verify-pipeline] HTML report: {}", path.display()),
            None => eprintln!("[verify-pipeline] WARN: could not write HTML report"),
        }
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&result.to_json()).unwrap_or_else(|_| "{}".to_string())
    );
    if !result.failed.is_empty() {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn config_fallback_parses_build_column() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(
            claude.join("pipeline-config.md"),
            "| Agent | Build Command |\n|-------|---------------|\n| api | `cargo build` |\n",
        )
        .unwrap();
        let targets = discover_via_config(dir.path());
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].name, "api");
        assert_eq!(targets[0].build.as_deref(), Some("cargo build"));
    }

    #[test]
    fn verify_targets_empty_yields_empty_result() {
        // `verify_targets` is exercised directly — `verify()` would spawn
        // `current_exe() run sync-detect`, which under a test binary recurses.
        let result = verify_targets(&[]);
        assert!(result.passed.is_empty());
        assert!(result.failed.is_empty());
        assert_eq!(result.timestamp.len(), 24);
    }

    #[test]
    fn verify_targets_skips_command_less_target() {
        let result = verify_targets(&[VerifyTarget {
            name: "doc".to_string(),
            cwd: std::env::temp_dir(),
            build: None,
            test: None,
        }]);
        assert_eq!(result.skipped, vec!["doc".to_string()]);
    }

    #[test]
    fn html_report_is_standalone() {
        let dir = tempdir().unwrap();
        let result = VerifyResult {
            passed: vec!["api".into()],
            failed: vec![json!({ "name": "ui", "error": "boom" })],
            skipped: vec![],
            timestamp: now_iso8601(),
        };
        let path = write_html_report(dir.path(), &result).unwrap();
        let html = std::fs::read_to_string(path).unwrap();
        assert!(html.starts_with("<!doctype html>"));
        assert!(!html.contains("href=") && !html.contains("src="));
        assert!(html.contains("api"));
    }
}
