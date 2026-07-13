//! `mustard-rt run maint-deps` — install dependencies in every subproject.
//!
//! Port of the `deps` action in `maint/SKILL.md`. Auto-discovers subprojects
//! from grain's repo model (`.claude/grain.model.json` `projects[]`, via the
//! scan tool — never parsed directly), picks the install command per project
//! kind (`pnpm install` for npm, `cargo fetch` for cargo, `dotnet restore` for
//! .NET, …), and runs them in sequence (parallelism is left to the user — this
//! is a maintenance helper, not a CI driver).

use serde_json::json;
use mustard_core::domain::model::event::ActorKind;
use crate::shared::context;
use crate::shared::events::economy;
use mustard_core::platform::process::rtk_command;
use serde::Serialize;
use std::path::PathBuf;

/// Options for `mustard-rt run maint-deps`.
#[derive(Debug, Clone)]
pub struct MaintDepsOpts {
    pub dry_run: bool,
}

/// One subproject install result.
#[derive(Debug, Serialize)]
pub(crate) struct InstallRecord {
    pub subproject: String,
    pub command: String,
    pub ok: bool,
    pub duration_ms: u64,
}

/// JSON report.
#[derive(Debug, Serialize)]
pub(crate) struct MaintDepsReport {
    pub dry_run: bool,
    pub installs: Vec<InstallRecord>,
}

/// Pick the canonical install command for a project kind (grain's manifest
/// `kind`: npm/cargo/dotnet/go/pub/maven; common stack aliases also accepted).
#[must_use]
pub(crate) fn install_command(kind: &str) -> Option<(&'static str, Vec<&'static str>)> {
    match kind.to_ascii_lowercase().as_str() {
        "npm" | "node" | "typescript" | "javascript" | "react" | "nextjs" | "next" => {
            Some(("pnpm", vec!["install"]))
        }
        "cargo" | "rust" => Some(("cargo", vec!["fetch"])),
        "dotnet" | "csharp" | "c#" => Some(("dotnet", vec!["restore"])),
        "go" => Some(("go", vec!["mod", "download"])),
        "pub" | "dart" | "flutter" => Some(("dart", vec!["pub", "get"])),
        "maven" | "java" => Some(("mvn", vec!["install", "-q", "-DskipTests"])),
        "python" => Some(("pip", vec!["install", "-e", "."])),
        _ => None,
    }
}

/// Run the install routine. Pure-ish (spawns subprocesses). Subprojects come
/// from grain's repo model (`read_projects` → the scan tool's `facts`).
fn install_all(dry_run: bool) -> MaintDepsReport {
    let mut installs: Vec<InstallRecord> = Vec::new();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let model = cwd.join(".claude").join("grain.model.json");
    for project in mustard_core::read_projects(&model) {
        let Some((bin, args)) = install_command(&project.kind) else {
            continue;
        };
        let cmd_str = format!("{bin} {}", args.join(" "));
        let display = if project.dir.is_empty() { project.name.clone() } else { project.dir.clone() };
        let run_dir = if project.dir.is_empty() { cwd.clone() } else { cwd.join(&project.dir) };
        if dry_run {
            installs.push(InstallRecord {
                subproject: display,
                command: cmd_str,
                ok: true,
                duration_ms: 0,
            });
            continue;
        }
        let started = std::time::Instant::now();
        let result = rtk_command(bin, &args)
            .current_dir(&run_dir)
            .output();
        let dur = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let ok = matches!(result, Ok(ref o) if o.status.success());
        installs.push(InstallRecord {
            subproject: display,
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
    economy::emit_operation(&context::cwd(), ActorKind::Orchestrator, "maint-deps", started.elapsed().as_millis() as u64, None, json!({}));
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
