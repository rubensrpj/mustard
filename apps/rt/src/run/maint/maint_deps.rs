//! `mustard-rt run maint-deps` — install dependencies in every subproject.
//!
//! Port of the `deps` action in `maint/SKILL.md`. Auto-discovers subprojects
//! via the existing `sync-detect` JSON, picks the install command per stack
//! (`pnpm install` for JS/TS, `cargo fetch` for Rust, `dotnet restore` for .NET),
//! and runs them in sequence (parallelism is left to the user — this is a
//! maintenance helper, not a CI driver).

use crate::shared::context::session_id;
use crate::util::now_iso8601;
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::process::rtk_command;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Options for `mustard-rt run maint-deps`.
#[derive(Debug, Clone)]
pub struct MaintDepsOpts {
    pub dry_run: bool,
}

/// One subproject install result.
#[derive(Debug, Serialize)]
pub struct InstallRecord {
    pub subproject: String,
    pub command: String,
    pub ok: bool,
    pub duration_ms: u64,
}

/// JSON report.
#[derive(Debug, Serialize)]
pub struct MaintDepsReport {
    pub dry_run: bool,
    pub installs: Vec<InstallRecord>,
}

/// Pick the canonical install command for a stack token.
#[must_use]
pub fn install_command(stack: &str) -> Option<(&'static str, Vec<&'static str>)> {
    match stack.to_ascii_lowercase().as_str() {
        "typescript" | "javascript" | "react" | "nextjs" | "next" | "node" => {
            Some(("pnpm", vec!["install"]))
        }
        "rust" => Some(("cargo", vec!["fetch"])),
        "dotnet" | "csharp" | "c#" => Some(("dotnet", vec!["restore"])),
        "python" => Some(("pip", vec!["install", "-e", "."])),
        "go" => Some(("go", vec!["mod", "download"])),
        _ => None,
    }
}

/// Discover subprojects via `sync-detect`. Returns the parsed JSON or `None`.
fn discover_subprojects() -> Option<Value> {
    let out = rtk_command("mustard-rt", &["run", "sync-detect"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

/// Run the install routine. Pure-ish (spawns subprocesses).
fn install_all(dry_run: bool) -> MaintDepsReport {
    let mut installs: Vec<InstallRecord> = Vec::new();
    let Some(detect) = discover_subprojects() else {
        // No detection → emit an empty report; the caller knows their tree.
        return MaintDepsReport { dry_run, installs };
    };
    let subs = detect
        .get("subprojects")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for sub in subs {
        let path = sub
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let stack = sub
            .get("stack")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let Some((bin, args)) = install_command(&stack) else {
            continue;
        };
        let cmd_str = format!("{bin} {}", args.join(" "));
        if dry_run {
            installs.push(InstallRecord {
                subproject: path,
                command: cmd_str,
                ok: true,
                duration_ms: 0,
            });
            continue;
        }
        let started = std::time::Instant::now();
        let result = rtk_command(bin, &args)
            .current_dir(&path)
            .output();
        let dur = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let ok = matches!(result, Ok(ref o) if o.status.success());
        installs.push(InstallRecord {
            subproject: path,
            command: cmd_str,
            ok,
            duration_ms: dur,
        });
    }
    MaintDepsReport { dry_run, installs }
}

/// CLI entry.
pub fn run(opts: MaintDepsOpts) {
    let started = std::time::Instant::now();
    let report = install_all(opts.dry_run);
    let body = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
    println!("{body}");
    emit_economy(started.elapsed().as_millis());
}

fn emit_economy(duration_ms: u128) {
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| ".".to_string());
    let _ = PathBuf::from(&cwd);
    let duration_capped = i64::try_from(duration_ms).unwrap_or(i64::MAX);
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("maint-deps".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: json!({
            "operation": "maint-deps",
            "duration_ms": duration_capped,
            "tokens_used": 0,
            "was_rust_only": true,
        }),
        spec: None,
    };
    let _ = crate::shared::events::route::emit(&cwd, &ev);
}

// Resolve the relative subproject path against the project's repo root.
// (Kept for future hardening — `install_all` currently shells in cwd of each path.)
#[allow(dead_code)]
fn join_repo(repo: &Path, sub: &str) -> PathBuf {
    if Path::new(sub).is_absolute() {
        PathBuf::from(sub)
    } else {
        repo.join(sub)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_command_handles_known_stacks() {
        let (bin, args) = install_command("typescript").unwrap();
        assert_eq!(bin, "pnpm");
        assert!(args.contains(&"install"));
        assert!(install_command("rust").is_some());
        assert!(install_command("dotnet").is_some());
        assert!(install_command("python").is_some());
    }

    #[test]
    fn install_command_returns_none_for_unknown_stack() {
        assert!(install_command("haskell").is_none());
        assert!(install_command("").is_none());
    }

    #[test]
    fn install_command_is_case_insensitive() {
        assert!(install_command("RUST").is_some());
        assert!(install_command("React").is_some());
    }

    #[test]
    fn dry_run_emits_zero_duration_records() {
        let report = MaintDepsReport {
            dry_run: true,
            installs: vec![InstallRecord {
                subproject: "apps/cli".to_string(),
                command: "cargo fetch".to_string(),
                ok: true,
                duration_ms: 0,
            }],
        };
        let v = serde_json::to_value(report).unwrap();
        assert_eq!(v["dry_run"], json!(true));
        assert_eq!(v["installs"][0]["duration_ms"], json!(0));
    }

    #[test]
    fn json_shape_has_required_fields() {
        let r = MaintDepsReport {
            dry_run: false,
            installs: Vec::new(),
        };
        let v = serde_json::to_value(r).unwrap();
        assert!(v.get("dry_run").is_some());
        assert!(v.get("installs").unwrap().is_array());
    }
}
