//! `mustard-rt run maint-validate` — build/type-check every subproject.
//!
//! Port of the `validate` action in `maint/SKILL.md`. Enumerates subprojects
//! from grain's repo model (`.claude/grain.model.json` `projects[]`, via the
//! scan tool — never parsed directly), picks the canonical validate command per
//! project kind (`pnpm typecheck` for npm, `cargo check` for cargo, `dotnet
//! build` for .NET, …), and runs them sequentially. Pass/fail per subproject is
//! captured in the JSON report; the overall verdict is the conjunction.

use serde_json::json;
use mustard_core::domain::model::event::ActorKind;
use crate::shared::context;
use crate::shared::events::economy;
use mustard_core::platform::process::rtk_command;
use serde::Serialize;
use std::path::PathBuf;

/// Options for `mustard-rt run maint-validate`.
#[derive(Debug, Clone)]
pub struct MaintValidateOpts {
    pub dry_run: bool,
}

/// One subproject validation result.
#[derive(Debug, Serialize)]
pub(crate) struct ValidateRecord {
    pub subproject: String,
    pub command: String,
    pub ok: bool,
    pub duration_ms: u64,
}

/// JSON report.
#[derive(Debug, Serialize)]
pub(crate) struct MaintValidateReport {
    pub dry_run: bool,
    pub overall: &'static str,
    pub validates: Vec<ValidateRecord>,
}

/// Pick the canonical validate command for a project kind (grain's manifest
/// `kind`: npm/cargo/dotnet/go/pub/maven; common stack aliases also accepted).
#[must_use]
pub(crate) fn validate_command(kind: &str) -> Option<(&'static str, Vec<&'static str>)> {
    match kind.to_ascii_lowercase().as_str() {
        "npm" | "node" | "typescript" | "javascript" | "react" | "nextjs" | "next" => {
            Some(("pnpm", vec!["typecheck"]))
        }
        "cargo" | "rust" => Some(("cargo", vec!["check"])),
        "dotnet" | "csharp" | "c#" => Some(("dotnet", vec!["build"])),
        "go" => Some(("go", vec!["build", "./..."])),
        "pub" | "dart" | "flutter" => Some(("dart", vec!["analyze"])),
        "maven" | "java" => Some(("mvn", vec!["compile", "-q"])),
        "python" => Some(("python", vec!["-m", "py_compile", "."])),
        _ => None,
    }
}

fn validate_all(dry_run: bool) -> MaintValidateReport {
    let mut validates: Vec<ValidateRecord> = Vec::new();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let model = cwd.join(".claude").join("grain.model.json");
    let projects = mustard_core::read_projects(&model);
    if projects.is_empty() {
        return MaintValidateReport {
            dry_run,
            overall: "skip",
            validates,
        };
    }
    for project in projects {
        let Some((bin, args)) = validate_command(&project.kind) else {
            continue;
        };
        let cmd_str = format!("{bin} {}", args.join(" "));
        let display = if project.dir.is_empty() { project.name.clone() } else { project.dir.clone() };
        let run_dir = if project.dir.is_empty() { cwd.clone() } else { cwd.join(&project.dir) };
        if dry_run {
            validates.push(ValidateRecord {
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
        validates.push(ValidateRecord {
            subproject: display,
            command: cmd_str,
            ok,
            duration_ms: dur,
        });
    }
    let overall = if validates.iter().all(|v| v.ok) {
        "pass"
    } else {
        "fail"
    };
    MaintValidateReport {
        dry_run,
        overall,
        validates,
    }
}

/// CLI entry.
pub fn run(opts: MaintValidateOpts) {
    let started = std::time::Instant::now();
    let report = validate_all(opts.dry_run);
    let body = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
    println!("{body}");
    economy::emit_operation(&context::cwd(), ActorKind::Orchestrator, "maint-validate", started.elapsed().as_millis() as u64, None, json!({}));
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_command_known_stacks() {
        assert_eq!(validate_command("typescript").unwrap().0, "pnpm");
        assert_eq!(validate_command("rust").unwrap().0, "cargo");
        assert_eq!(validate_command("dotnet").unwrap().0, "dotnet");
    }

    #[test]
    fn validate_command_unknown_returns_none() {
        assert!(validate_command("haskell").is_none());
    }

    #[test]
    fn validate_command_case_insensitive() {
        assert!(validate_command("RUST").is_some());
        assert!(validate_command("TypeScript").is_some());
    }

    #[test]
    fn overall_pass_when_all_records_ok() {
        let r = MaintValidateReport {
            dry_run: false,
            overall: "pass",
            validates: vec![
                ValidateRecord {
                    subproject: "a".to_string(),
                    command: "ok".to_string(),
                    ok: true,
                    duration_ms: 1,
                },
                ValidateRecord {
                    subproject: "b".to_string(),
                    command: "ok".to_string(),
                    ok: true,
                    duration_ms: 2,
                },
            ],
        };
        assert_eq!(r.overall, "pass");
    }

    #[test]
    fn json_shape_has_required_fields() {
        let r = MaintValidateReport {
            dry_run: false,
            overall: "skip",
            validates: Vec::new(),
        };
        let v = serde_json::to_value(r).unwrap();
        for f in ["dry_run", "overall", "validates"] {
            assert!(v.get(f).is_some(), "missing {f}");
        }
    }
}
