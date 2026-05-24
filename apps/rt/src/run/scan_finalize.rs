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

use mustard_core::fs;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

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
    let state_path = root.join(".claude").join(".scan-dispatch.json");
    let state: Option<Value> = fs::read_to_string(&state_path)
        .ok()
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
        let skills_dir = root.join(abs).join(".claude").join("skills");
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
    let mode = std::env::var("MUSTARD_SKILL_VALIDATE_MODE")
        .map_or_else(|_| "strict".to_string(), |m| m.to_lowercase());
    let skills_step;
    if mode == "off" {
        skills_step = json!({ "ran": false, "ok": true, "mode": mode });
    } else {
        let skills = run_subcommand(root, &["run", "skills", "validate", "--factual"]);
        if mode == "warn" {
            if !skills.ok {
                warnings.push(format!("skill-validate (warn mode): exit {:?}", skills.status));
            }
            skills_step = json!({
                "ran": true, "ok": true, "mode": mode, "durationMs": skills.duration_ms,
            });
        } else {
            if !skills.ok {
                errors.push(format!("skill-validate (strict): exit {:?}", skills.status));
                if !skills.stdout.is_empty() {
                    errors.push(format!("skill-validate stdout: {}", truncate(&skills.stdout, 800)));
                }
            }
            skills_step = json!({
                "ran": true, "ok": skills.ok, "mode": mode, "durationMs": skills.duration_ms,
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

    json!({
        "steps": {
            "registry": { "ran": true, "ok": registry.ok, "durationMs": registry.duration_ms },
            "cache": { "ran": true, "ok": cache.ok, "durationMs": cache.duration_ms },
            "skills": skills_step,
            "security": security_step,
            "dispatchVerify": dispatch_verify,
        },
        "errors": errors,
        "warnings": warnings,
        "totalDurationMs": start.elapsed().as_millis(),
    })
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
        let claude = dir.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(
            claude.join(".scan-dispatch.json"),
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
        let claude = dir.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(
            claude.join(".scan-dispatch.json"),
            r#"{"dispatch":[{"name":"api","path":"api","absSubprojectPath":"api"}]}"#,
        )
        .unwrap();
        let skill = dir.path().join("api").join(".claude").join("skills").join("s1");
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
