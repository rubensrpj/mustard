//! `mustard-rt run scan-finalize` — a port of `scripts/scan/finalize.js`.
//!
//! Post-dispatch finalization for `/scan`, run after every Task agent returns:
//! refreshes the entity registry, updates the detect cache, validates generated
//! skills, runs the security scan, and verifies each dispatched subproject
//! honoured the HARD CONTRACT (wrote `SKILL.md` or the `_no-patterns.md`
//! marker).
//!
//! Contract: stdout is the JSON result; the process always exits `0`
//! (fail-open). Port note: the JS spawned the four sub-scripts via `node`;
//! this port spawns the same `mustard-rt` binary (`run sync-registry`,
//! `run sync-detect`, `run skills validate --factual`, `run security-scan`).
//! The JS ran the four in parallel — this port runs them sequentially, which
//! is simpler and still fast since each is a short binary invocation.

use crate::run::scan::refs_installer::{install_refs, DetectedStack};
use mustard_core::fs;
use mustard_core::ClaudePaths;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

/// Resolve `<root>/.claude/.cache/scan-dispatch.json` (the W2 cache reorg
/// location). Returns `None` when `root` violates the I1 guard — callers
/// degrade to a no-op rather than mis-route the lookup.
fn scan_dispatch_path_for(root: &Path) -> Option<PathBuf> {
    ClaudePaths::for_project(root)
        .ok()
        .map(|p| p.scan_dispatch_path())
}

/// One sub-step's outcome.
struct StepResult {
    ok: bool,
    status: Option<i32>,
    stdout: String,
    stderr: String,
    duration_ms: u128,
}

/// Run a `mustard-rt run …` subcommand in `root`.
fn run_subcommand(root: &Path, args: &[&str]) -> StepResult {
    let start = Instant::now();
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            return StepResult {
                ok: false,
                status: None,
                stdout: String::new(),
                stderr: e.to_string(),
                duration_ms: start.elapsed().as_millis(),
            };
        }
    };
    let output = Command::new(&exe).args(args).current_dir(root).output();
    match output {
        Ok(out) => StepResult {
            ok: out.status.success(),
            status: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).to_string(),
            duration_ms: start.elapsed().as_millis(),
        },
        Err(e) => StepResult {
            ok: false,
            status: None,
            stdout: String::new(),
            stderr: e.to_string(),
            duration_ms: start.elapsed().as_millis(),
        },
    }
}

/// Verify each dispatched subproject produced skills or the no-patterns marker.
fn verify_dispatch(root: &Path) -> (Value, Vec<String>) {
    let mut warnings = Vec::new();
    let state: Option<Value> = scan_dispatch_path_for(root)
        .as_ref()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok());
    let dispatch = state
        .as_ref()
        .and_then(|s| s.get("dispatch"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if dispatch.is_empty() {
        return (json!({ "ran": false, "ok": Value::Null, "subprojects": [] }), warnings);
    }

    let mut subprojects: Vec<Value> = Vec::new();
    let mut any_empty = false;
    for sub in &dispatch {
        let name = sub.get("name").and_then(Value::as_str).unwrap_or("");
        let abs = sub.get("absSubprojectPath").and_then(Value::as_str).unwrap_or("");
        // `absSubprojectPath` is recorded relative to root by the orchestrator.
        let abs_sub = root.join(abs);
        let skills_dir = ClaudePaths::for_project(&abs_sub)
            .map(|p| p.skills_dir())
            .unwrap_or_else(|_| abs_sub.clone());
        let mut skills: Vec<String> = Vec::new();
        let mut has_marker = false;
        let status: &str;
        if !skills_dir.exists() {
            status = "missing-dir";
        } else {
            if let Ok(entries) = fs::read_dir(&skills_dir) {
                for entry in entries {
                    let entry_name = entry.file_name.clone();
                    if entry.is_dir {
                        if entry.path.join("SKILL.md").exists() {
                            skills.push(entry_name);
                        }
                    } else if entry_name == "_no-patterns.md" {
                        has_marker = true;
                    }
                }
            }
            skills.sort();
            status = if !skills.is_empty() {
                "skills"
            } else if has_marker {
                "no-patterns-marker"
            } else {
                "empty"
            };
        }
        if status == "empty" || status == "missing-dir" {
            any_empty = true;
            warnings.push(format!(
                "dispatchVerify: {name} returned empty skills/ — HARD CONTRACT violated \
                 (expected SKILL.md OR _no-patterns.md at {}). Re-dispatch needed.",
                skills_dir.display()
            ));
        }
        subprojects.push(json!({
            "name": name,
            "skillsDir": skills_dir.to_string_lossy(),
            "status": status,
            "skillsWritten": skills.len(),
            "skills": skills,
            "hasNoPatternsMarker": has_marker,
        }));
    }
    (
        json!({ "ran": true, "ok": !any_empty, "subprojects": subprojects }),
        warnings,
    )
}

/// Run the finalization. Separate from [`run`] so tests can drive it.
fn finalize(root: &Path, skip_security: bool) -> Value {
    let start = Instant::now();
    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // Step 4.7 — refresh entity registry.
    let registry = run_subcommand(root, &["run", "sync-registry", "--force"]);
    if !registry.ok {
        errors.push(format!("registry: exit {:?}", registry.status));
        if !registry.stderr.is_empty() {
            warnings.push(format!("registry stderr: {}", truncate(&registry.stderr, 500)));
        }
    }

    // Step 5 — update detect cache.
    let cache = run_subcommand(root, &["run", "sync-detect"]);
    if !cache.ok {
        errors.push(format!("cache: exit {:?}", cache.status));
    }

    // Step 6 — validate skills (--factual), mode-gated.
    //
    // T3.12 — when the first validate pass fails, log a warning and
    // re-dispatch validation ONE TIME. If the second pass still fails,
    // fail-open with `attempts: 2` so the caller can see both attempts.
    // Hard cap at 2 — no infinite retry loop.
    let mode = std::env::var("MUSTARD_SKILL_VALIDATE_MODE")
        .map_or_else(|_| "strict".to_string(), |m| m.to_lowercase());
    let skills_step;
    if mode == "off" {
        skills_step = json!({ "ran": false, "ok": true, "mode": mode, "attempts": 0 });
    } else {
        let first = run_subcommand(root, &["run", "skills", "validate", "--factual"]);
        let (skills, attempts, retried) = if first.ok {
            (first, 1u32, false)
        } else {
            warnings.push(format!(
                "skill-validate first pass failed (exit {:?}); re-dispatching once",
                first.status
            ));
            let second = run_subcommand(root, &["run", "skills", "validate", "--factual"]);
            (second, 2u32, true)
        };
        if mode == "warn" {
            if !skills.ok {
                warnings.push(format!("skill-validate (warn mode): exit {:?}", skills.status));
            }
            skills_step = json!({
                "ran": true,
                "ok": true,
                "mode": mode,
                "durationMs": skills.duration_ms,
                "attempts": attempts,
                "retried": retried,
                "validated": skills.ok,
            });
        } else {
            if !skills.ok {
                // Fail-open: surface as warning, not error, when the second
                // attempt also failed — the contract is to report attempts,
                // not to bubble an exit code that would block the scan.
                if attempts == 2 {
                    warnings.push(format!(
                        "skill-validate (strict): both attempts failed (exit {:?})",
                        skills.status
                    ));
                } else {
                    errors.push(format!("skill-validate (strict): exit {:?}", skills.status));
                }
                if !skills.stdout.is_empty() {
                    errors.push(format!("skill-validate stdout: {}", truncate(&skills.stdout, 800)));
                }
            }
            skills_step = json!({
                "ran": true,
                "ok": skills.ok,
                "mode": mode,
                "durationMs": skills.duration_ms,
                "attempts": attempts,
                "retried": retried,
                "validated": skills.ok,
            });
        }
    }

    // Security scan.
    let security_step;
    if skip_security {
        security_step = json!({ "ran": false, "ok": Value::Null, "findings": 0 });
    } else {
        let security = run_subcommand(root, &["run", "security-scan", "--json"]);
        // security-scan exits 0 (clean) or 1 (findings) — both are useful.
        if matches!(security.status, Some(0 | 1)) {
            let findings = serde_json::from_str::<Value>(&security.stdout)
                .ok()
                .and_then(|v| v.get("secrets").and_then(Value::as_array).map(Vec::len))
                .unwrap_or(0);
            security_step = json!({
                "ran": true, "ok": true, "findings": findings,
                "durationMs": security.duration_ms,
            });
        } else {
            warnings.push(format!("security: unexpected exit {:?}", security.status));
            security_step = json!({
                "ran": true, "ok": false, "findings": 0,
                "durationMs": security.duration_ms,
            });
        }
    }

    let (dispatch_verify, dv_warnings) = verify_dispatch(root);
    warnings.extend(dv_warnings);

    // T3.5 — install stack-matched refs per dispatched subproject. Fires
    // after validate so refs land alongside freshly-validated skills.
    // Fail-open: per-subproject errors are surfaced in `refsInstall` but
    // never block the rest of finalize.
    let refs_install = install_refs_step(root, &mut warnings);

    json!({
        "steps": {
            "registry": { "ran": true, "ok": registry.ok, "durationMs": registry.duration_ms },
            "cache": { "ran": true, "ok": cache.ok, "durationMs": cache.duration_ms },
            "skills": skills_step,
            "security": security_step,
            "dispatchVerify": dispatch_verify,
            "refsInstall": refs_install,
        },
        "errors": errors,
        "warnings": warnings,
        "totalDurationMs": start.elapsed().as_millis(),
    })
}

/// For every dispatched subproject in `<root>/.claude/.cache/scan-dispatch.json`,
/// install matching stack-templates refs.
///
/// Reads the dispatch state's `name`/`path`/`role`/`stackSummary` to build
/// a [`DetectedStack`] per subproject — `stackSummary` is mined as a stack
/// id when present, `role` becomes the `roles` entry. The result is a
/// per-subproject JSON object with counts that the finalize report can
/// surface.
fn install_refs_step(root: &Path, warnings: &mut Vec<String>) -> Value {
    let Some(state) = scan_dispatch_path_for(root)
        .as_ref()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
    else {
        return json!({ "ran": false, "subprojects": [] });
    };
    let dispatch = state
        .get("dispatch")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if dispatch.is_empty() {
        return json!({ "ran": false, "subprojects": [] });
    }

    let mut out: Vec<Value> = Vec::new();
    for sub in &dispatch {
        let name = sub.get("name").and_then(Value::as_str).unwrap_or("");
        let path = sub
            .get("absSubprojectPath")
            .and_then(Value::as_str)
            .or_else(|| sub.get("path").and_then(Value::as_str))
            .unwrap_or("");
        let role = sub.get("role").and_then(Value::as_str).unwrap_or("");
        let stack_summary = sub
            .get("stackSummary")
            .and_then(Value::as_str)
            .unwrap_or("");
        // Mine a stack id from the summary — first whitespace-separated
        // token, lowercased. Empty when no summary.
        let stack_id = stack_summary
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let stack = DetectedStack {
            id: stack_id,
            roles: if role.is_empty() {
                vec![]
            } else {
                vec![role.to_string()]
            },
            extras: vec![],
        };
        let target = if path.is_empty() {
            root.to_path_buf()
        } else {
            root.join(path)
        };
        let report = install_refs(&stack, &target);
        if !report.errors.is_empty() {
            for err in &report.errors {
                warnings.push(format!("refs-install [{name}]: {err}"));
            }
        }
        out.push(json!({
            "name": name,
            "installed": report.installed.len(),
            "skippedIdentical": report.skipped_identical.len(),
            "skippedNoMatch": report.skipped_no_match.len(),
            "errors": report.errors.len(),
        }));
    }
    json!({ "ran": true, "subprojects": out })
}

/// Truncate a string to `n` chars.
fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Dispatch `mustard-rt run scan-finalize`.
pub fn run(skip_security: bool) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let result = finalize(&cwd, skip_security);
    println!(
        "{}",
        serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".into())
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn verify_dispatch_flags_empty_subproject() {
        let dir = tempdir().unwrap();
        let cache = ClaudePaths::for_project(dir.path()).unwrap().cache_dir();
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(
            cache.join("scan-dispatch.json"),
            r#"{"dispatch":[{"name":"api","path":"api","absSubprojectPath":"api"}]}"#,
        )
        .unwrap();
        let (verdict, warnings) = verify_dispatch(dir.path());
        assert_eq!(verdict["ok"], json!(false));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("HARD CONTRACT"));
    }

    #[test]
    fn verify_dispatch_passes_with_skill() {
        let dir = tempdir().unwrap();
        let cache = ClaudePaths::for_project(dir.path()).unwrap().cache_dir();
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(
            cache.join("scan-dispatch.json"),
            r#"{"dispatch":[{"name":"api","path":"api","absSubprojectPath":"api"}]}"#,
        )
        .unwrap();
        let api_dir = dir.path().join("api");
        std::fs::create_dir_all(&api_dir).unwrap();
        let skill = ClaudePaths::for_project(&api_dir).unwrap().skills_dir().join("s1");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), "x").unwrap();
        let (verdict, warnings) = verify_dispatch(dir.path());
        assert_eq!(verdict["ok"], json!(true));
        assert!(warnings.is_empty());
    }

    #[test]
    fn verify_dispatch_no_state_is_inert() {
        let dir = tempdir().unwrap();
        let (verdict, warnings) = verify_dispatch(dir.path());
        assert_eq!(verdict["ran"], json!(false));
        assert!(warnings.is_empty());
    }
}
