//! `mustard-rt run doctor` — read-only installation health diagnostic.
//!
//! Runs four checks and prints a compact OK/WARN/FAIL report per category.
//! Exit 1 if any check is FAIL, 0 otherwise. Fail-open on every IO error:
//! a check that cannot complete is demoted to WARN, never crashes.
//!
//! ## Checks
//!
//! - **wiring** — every `mustard-rt on <event>` / `run <cmd>` command string
//!   referenced in `.claude/settings.json` resolves to a known event or
//!   registered run subcommand. FAIL on unresolved references. Both known-sets
//!   are *derived*, never declared: events from the shipped
//!   `plugin/hooks/hooks.json` manifest, subcommands from the live clap tree.
//! - **residue** (`--residue` only) — scan `settings.json`, SKILL.md files,
//!   and refs for mentions of paths/commands that no longer exist (dead `.js`
//!   names, `scripts/` entries with no resolvable target). WARN per hit.
//! - **scratch-residue** (`--residue` só) — tamanho total das sobras que o
//!   `scratch-gc` recolheria e o da compilação compartilhada, pela MESMA
//!   varredura dele. WARN quando há sobra ou quando a compilação passou do teto.
//! - **drift** — compare by hash the folders a fresh payload owns
//!   (`CORE_FOLDERS`) between the installed `.claude/` and the
//!   `templates/` source. Degrades to `skip` when `templates/` is not
//!   reachable from cwd (consumer project).
//! - **state health** — orphan `.pipeline-states/` files (no matching active
//!   spec), expired `closed-followup` state files, missing
//!   `grain.model.json`. WARN per anomaly.
//! - **nerd-font** — at least one Nerd Font detected in the OS font
//!   directories. WARN with install hint (`mustard install-nerd-font`) when
//!   absent. Powerline statusline themes require this; without it the
//!   transition glyphs render as tofu.
//! - **branch-protection** — which branches this repository REALLY refuses a
//!   direct commit on, measured through `mustard_core::protected_branches`
//!   (`origin/HEAD` ∪ `mustard.json#git.protected`). WARN only when
//!   `origin/HEAD` is unreadable, because protection then rests on literals
//!   this project may not use at all.
//! - **spec-index** — o índice das specs (`.claude/spec/index.ndjson`) contra
//!   os arquivos de eventos: índice que falta, linha que falta, sobra ou
//!   difere, e campo `search` calculado por outro redutor. Só lê e acusa, com
//!   WARN e a mensagem no idioma do projeto, que manda rodar
//!   `mustard-rt run index`.
//! - **switches** — as escolhas do `mustard.json` contra as configurações
//!   locais: WARN quando o Mustard está desligado no projeto, quando a opção
//!   `rtk` e o gancho do rtk divergem, e quando a assinatura do Claude Code
//!   está ligada.
//! - **claude-md** — sobras do Mustard em arquivos que não são dele (as marcas
//!   nos `CLAUDE.md`, as linhas do molde no `settings.json` da equipe), pela
//!   mesma lista que o `upsert` mostra.
//!
//! ## Onde cada conferência mora
//!
//! Esta porta guarda o que todas dividem e o que sai para fora — o resultado
//! de uma conferência, os eventos de gancho que o binário conhece, as opções e
//! a ordem em que o comando as roda. Cada conferência mora numa
//! parte da pasta ao lado, por assunto: a ligação dos ganchos (`wiring`), as
//! sobras (`residue`), o desvio dos moldes (`drift`), o que a máquina tem
//! instalado (`host`), a proteção das bases (`protection`), o estado das
//! specs (`specs`), o que o Mustard deixa no projeto (`project`) e a saída do
//! relatório (`report`).

mod drift;
mod host;
mod project;
mod protection;
mod report;
mod residue;
mod specs;
mod wiring;

use mustard_core::ClaudePaths;
use std::path::PathBuf;

use drift::check_drift;
use host::{check_claude_cli, check_nerd_font, lsp_check};
use project::{check_claude_md, check_scan_output, check_switches};
use protection::check_branch_protection;
use report::{render_report, render_report_json};
use residue::{check_residue, check_scratch_residue};
use specs::{check_spec_index, check_state_health, check_wave_integrity};
use wiring::check_wiring;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// The status of a single check.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Status {
    Ok,
    Warn,
    Fail,
    Skip,
}

impl Status {
    fn label(&self) -> &'static str {
        match self {
            Status::Ok => "OK",
            Status::Warn => "WARN",
            Status::Fail => "FAIL",
            Status::Skip => "SKIP",
        }
    }
}

/// One diagnostic check result.
struct CheckResult {
    name: &'static str,
    status: Status,
    details: Vec<String>,
}

impl CheckResult {
    fn ok(name: &'static str) -> Self {
        Self { name, status: Status::Ok, details: Vec::new() }
    }

    fn warn(name: &'static str, details: Vec<String>) -> Self {
        Self { name, status: Status::Warn, details }
    }

    fn fail(name: &'static str, details: Vec<String>) -> Self {
        Self { name, status: Status::Fail, details }
    }

    fn skip(name: &'static str, reason: &str) -> Self {
        Self { name, status: Status::Skip, details: vec![reason.to_string()] }
    }
}

// ---------------------------------------------------------------------------
// Known valid events
// ---------------------------------------------------------------------------

/// The shipped hook manifest (`plugin/hooks/hooks.json`), embedded at build
/// time. That file is the only thing that decides which `<event>` names the
/// harness ever hands to `mustard-rt on`; embedding it makes the doctor read
/// the same artefact the harness reads, the way the wiring check's
/// `known_run_subcommands` reads the same clap tree the binary dispatches on.
/// A hand-kept copy drifted in both directions (it carried `PreCompact`, which
/// nothing registers, and omitted `Stop` and `WorktreeCreate`, which are
/// registered).
const SHIPPED_HOOKS_MANIFEST: &str = include_str!("../../../../../plugin/hooks/hooks.json");

/// All hook event names `mustard-rt on <event>` recognizes — the keys of the
/// shipped manifest's `hooks` object.
///
/// Degrades to an empty set when the manifest cannot be parsed. An empty set
/// means "cannot judge", and the wiring check's `validate_command_string`
/// treats it that way: it reports nothing rather than declaring every wired
/// event unknown.
#[must_use]
pub fn known_hook_events() -> std::collections::BTreeSet<String> {
    let Ok(manifest) = serde_json::from_str::<serde_json::Value>(SHIPPED_HOOKS_MANIFEST) else {
        return std::collections::BTreeSet::new();
    };
    manifest
        .get("hooks")
        .and_then(serde_json::Value::as_object)
        .map(|hooks| hooks.keys().cloned().collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Options for `mustard-rt run doctor`.
pub struct DoctorOpts {
    /// Also scan for dead file/script references (slower).
    pub residue: bool,
    /// Named check to run in isolation (e.g. `skill-discovery`,
    /// `claude-paths`, `workspace-leaks`, `i1`).
    pub check: Option<String>,
    /// Output format: `text` (default) or `json`.
    pub format: String,
}

/// Dispatch `mustard-rt run doctor [--residue] [--check <CHECK>] [--format json|--json]`.
pub fn run(opts: DoctorOpts) {
    let cwd = crate::shared::context::env::workspace_root_strict()
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let claude_dir = ClaudePaths::for_project(&cwd)
        .map(|p| p.claude_dir())
        .unwrap_or_else(|_| cwd.clone());

    // When a specific --check is requested, run only that check.
    if let Some(ref check_name) = opts.check {
        let result = match check_name.as_str() {
            "wave-integrity" => check_wave_integrity(&claude_dir),
                "branch-protection" => {
                let project = crate::commands::spec_events::project(&cwd);
                check_branch_protection(&cwd, project.lang)
            }
            "spec-index" => {
                let project = crate::commands::spec_events::project(&cwd);
                check_spec_index(&project.root, project.lang)
            }
            "scan-output" => {
                let project = crate::commands::spec_events::project(&cwd);
                check_scan_output(&project.root, project.lang)
            }
            "switches" => {
                let project = crate::commands::spec_events::project(&cwd);
                check_switches(&project.root, project.lang)
            }
            "claude-md" => {
                let project = crate::commands::spec_events::project(&cwd);
                check_claude_md(&project.root, project.lang)
            }
            other => {
                eprintln!(
                    "doctor: unknown check '{other}'. Known: \
                     wave-integrity, branch-protection, spec-index, scan-output, switches, claude-md"
                );
                std::process::exit(1);
            }
        };
        if opts.format == "json" {
            render_report_json(&[result]);
        } else {
            render_report(&[result]);
        }
        return;
    }

    // Default: run all checks.
    let mut results: Vec<CheckResult> = vec![
        check_wiring(&claude_dir),
        // Before drift: a drift reading is only meaningful once the binary the
        // reading comes FROM is known to be the installed one. A dormant
        // bootstrap makes every version answer below it untrustworthy.
        bootstrap_to_check_result(&crate::commands::doctor::bootstrap_check::run(&cwd)),
        check_drift(&claude_dir),
        check_state_health(&claude_dir),
        check_claude_cli(),
        lsp_check(&cwd),
        check_nerd_font(),
        // Wave-integrity check — always in the full run.
        check_wave_integrity(&claude_dir),
        // O que o provedor realmente protege — sempre na rodada inteira: uma
        // base que só este binário recusa é uma base aberta para todo mundo, e
        // isso não aparece até um envio direto passar.
        {
            let project = crate::commands::spec_events::project(&cwd);
            check_branch_protection(&cwd, project.lang)
        },
        // O índice das specs contra os arquivos de eventos: só acusa.
        {
            let project = crate::commands::spec_events::project(&cwd);
            check_spec_index(&project.root, project.lang)
        },
        // What the scan writes stays outside git: it is only reported.
        {
            let project = crate::commands::spec_events::project(&cwd);
            check_scan_output(&project.root, project.lang)
        },
        // The switches of `mustard.json` against the local settings.
        {
            let project = crate::commands::spec_events::project(&cwd);
            check_switches(&project.root, project.lang)
        },
        // What an older Mustard left in files that are not its own.
        {
            let project = crate::commands::spec_events::project(&cwd);
            check_claude_md(&project.root, project.lang)
        },
    ];

    if opts.residue {
        results.push(check_residue(&claude_dir));
        results.push(check_scratch_residue(
            &crate::commands::maint::scratch_gc::ScratchRoots::from_env(),
        ));
    }

    if opts.format == "json" {
        render_report_json(&results);
    } else {
        render_report(&results);
    }

    if results.iter().any(|r| r.status == Status::Fail) {
        std::process::exit(1);
    }
}

/// Fold the bootstrap report into the doctor's OK/WARN/FAIL envelope.
///
/// `binary-missing`, `stamp-mismatch` and `toolchain-unreachable` are FAIL:
/// each one means the harness cannot do a job it will nonetheless APPEAR to
/// do — dormant hooks, or criteria recorded `unproven` that read like failing
/// tests. `session-stale` is a WARN: everything works, it is just older than
/// what is installed.
fn bootstrap_to_check_result(
    report: &crate::commands::doctor::bootstrap_check::BootstrapReport,
) -> CheckResult {
    if report.ok && report.findings.is_empty() {
        let mut r = CheckResult::ok("bootstrap");
        r.details.push(format!(
            "plugin {} · binary {} · stamp {}",
            report.installed_version.as_deref().unwrap_or("?"),
            report.running_version,
            report.stamped_version.as_deref().unwrap_or("—"),
        ));
        return r;
    }
    let details: Vec<String> = report
        .findings
        .iter()
        .map(|f| format!("{}: {} — fix: {}", f.kind, f.detail, f.remedy))
        .collect();
    if report.failed {
        CheckResult::fail("bootstrap", details)
    } else if report.ok {
        let mut r = CheckResult::ok("bootstrap");
        r.details = details;
        r
    } else {
        CheckResult::warn("bootstrap", details)
    }
}

/// O que os testes das partes do diagnóstico dividem, o teste que roda várias
/// conferências juntas e o que mede o tamanho de cada parte.
#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use tempfile::tempdir;

    use super::*;

    pub(super) fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    pub(super) fn make_minimal_settings(hooks_dir: &Path, command: &str) {
        let settings = format!(
            r#"{{ "hooks": {{ "PreToolUse": [{{ "hooks": [{{ "type": "command", "command": "{command}" }}] }}] }} }}"#
        );
        write_file(&hooks_dir.join("settings.json"), &settings);
    }

    #[test]
    fn doctor_report_includes_lsp_check() {
        let dir = tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();

        // Minimal settings.json so wiring check doesn't fail hard.
        make_minimal_settings(&claude_dir, "mustard-rt on PreToolUse");
        // grain.model.json to keep state-health from warning.
        write_file(&claude_dir.join("grain.model.json"), "{}");

        // Run all checks the same way `run()` does, rooted at the tempdir.
        let results: Vec<CheckResult> = vec![
            check_wiring(&claude_dir),
            check_drift(&claude_dir),
            check_state_health(&claude_dir),
            lsp_check(dir.path()),
        ];

        let has_lsp = results.iter().any(|r| r.name == "lsp");
        assert!(has_lsp, "expected a check named 'lsp' in the report");
    }

    /// As linhas de código de um arquivo: as que não são vazias nem
    /// comentário, antes do módulo de testes dele.
    fn code_lines(source: &str) -> usize {
        let lines: Vec<&str> = source.lines().map(str::trim).collect();
        let end = lines
            .windows(2)
            .position(|pair| pair[0] == "#[cfg(test)]" && pair[1] == "mod tests {")
            .unwrap_or(lines.len());
        lines[..end].iter().filter(|line| !line.is_empty() && !line.starts_with("//")).count()
    }

    /// Nenhum arquivo do diagnóstico passa de 800 linhas de código: a porta e
    /// cada parte da pasta dela, contadas sem as linhas vazias, os comentários
    /// e o módulo de testes.
    #[test]
    fn no_file_of_the_doctor_goes_over_the_code_line_cap() {
        const CAP: usize = 800;
        let doctor = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("commands").join("doctor");
        let mut parts: Vec<PathBuf> = std::fs::read_dir(doctor.join("doctor"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
            .collect();
        parts.sort();
        assert!(!parts.is_empty(), "as partes do diagnóstico não foram achadas em {}", doctor.display());
        let files: Vec<PathBuf> = std::iter::once(doctor.join("doctor.rs")).chain(parts).collect();
        let measured: Vec<(String, usize)> = files
            .iter()
            .map(|path| (path.display().to_string(), code_lines(&std::fs::read_to_string(path).unwrap())))
            .collect();
        let over: Vec<&(String, usize)> = measured.iter().filter(|(_, lines)| *lines > CAP).collect();
        assert!(over.is_empty(), "passam de {CAP} linhas de código: {over:?}");
    }
}
