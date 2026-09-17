//! `mustard-rt run doctor` — read-only installation health diagnostic.
//!
//! Runs every check of one list and prints a compact OK/WARN/FAIL report per
//! category. Exit 1 if any check of the full run is FAIL, 0 otherwise.
//! Fail-open on every IO error: a check that cannot complete is demoted to
//! WARN, never crashes.
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
//! a lista das conferências, na ordem em que o comando as roda. Cada
//! conferência mora numa parte da pasta ao lado, por assunto: a ligação dos
//! ganchos (`wiring`), as sobras (`residue`), o desvio dos moldes (`drift`), o
//! que a máquina tem instalado (`host`), a proteção das bases (`protection`),
//! o estado das specs (`specs`), o que o Mustard deixa no projeto (`project`)
//! e a saída do relatório (`report`).

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
    /// A conferência que roda sozinha, por um dos nomes da lista.
    pub check: Option<String>,
    /// Output format: `text` (default) or `json`.
    pub format: String,
}

/// Onde as conferências olham: a pasta do projeto e o `.claude/` dela.
struct Place {
    cwd: PathBuf,
    claude_dir: PathBuf,
}

impl Place {
    /// A pasta de onde o comando foi chamado.
    fn here() -> Self {
        let cwd = crate::shared::context::env::workspace_root_strict()
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let claude_dir = ClaudePaths::for_project(&cwd)
            .map(|p| p.claude_dir())
            .unwrap_or_else(|_| cwd.clone());
        Self { cwd, claude_dir }
    }

    /// A raiz das specs e o idioma do projeto.
    fn project(&self) -> crate::commands::spec_events::Project {
        crate::commands::spec_events::project(&self.cwd)
    }
}

/// Em que rodada uma conferência entra.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Runs {
    /// Só na rodada inteira.
    Round,
    /// Na rodada inteira e também sozinha, por este nome no `--check`.
    Named(&'static str),
    /// Só na rodada inteira que pede as sobras (`--residue`).
    Residue,
}

/// Uma conferência do diagnóstico: quando ela entra e o que ela confere.
#[derive(Clone, Copy)]
struct Check {
    runs: Runs,
    check: fn(&Place) -> CheckResult,
}

/// Todas as conferências, na ordem da rodada inteira. Acrescentar uma
/// conferência é somar um item aqui, e nada mais: desta lista saem os nomes
/// que o `--check` aceita na linha de comando, a escolha dele, a mensagem de
/// conferência desconhecida e a rodada inteira.
const CHECKS: &[Check] = &[
    Check { runs: Runs::Round, check: |place| check_wiring(&place.claude_dir) },
    // Antes do desvio dos moldes: a leitura dele só vale quando o binário que
    // a faz é o instalado, e um arranque parado torna suspeita toda versão
    // lida depois.
    Check {
        runs: Runs::Round,
        check: |place| bootstrap_to_check_result(&crate::commands::doctor::bootstrap_check::run(&place.cwd)),
    },
    Check { runs: Runs::Round, check: |place| check_drift(&place.claude_dir) },
    Check { runs: Runs::Round, check: |place| check_state_health(&place.claude_dir) },
    Check { runs: Runs::Round, check: |_| check_claude_cli() },
    Check { runs: Runs::Round, check: |place| lsp_check(&place.cwd) },
    Check { runs: Runs::Round, check: |_| check_nerd_font() },
    Check { runs: Runs::Named("wave-integrity"), check: |place| check_wave_integrity(&place.claude_dir) },
    // O que o provedor realmente protege: uma base que só este binário recusa
    // é uma base aberta para todo mundo, e isso não aparece até um envio
    // direto passar.
    Check {
        runs: Runs::Named("branch-protection"),
        check: |place| check_branch_protection(&place.cwd, place.project().lang),
    },
    // O índice das specs contra os arquivos de eventos: só acusa.
    Check {
        runs: Runs::Named("spec-index"),
        check: |place| {
            let project = place.project();
            check_spec_index(&project.root, project.lang)
        },
    },
    // O que o scan escreve fica fora do git: só acusa.
    Check {
        runs: Runs::Named("scan-output"),
        check: |place| {
            let project = place.project();
            check_scan_output(&project.root, project.lang)
        },
    },
    // As escolhas do `mustard.json` contra as configurações locais.
    Check {
        runs: Runs::Named("switches"),
        check: |place| {
            let project = place.project();
            check_switches(&project.root, project.lang)
        },
    },
    // O que um Mustard antigo deixou em arquivos que não são dele.
    Check {
        runs: Runs::Named("claude-md"),
        check: |place| {
            let project = place.project();
            check_claude_md(&project.root, project.lang)
        },
    },
    Check { runs: Runs::Residue, check: |place| check_residue(&place.claude_dir) },
    Check {
        runs: Runs::Residue,
        check: |_| check_scratch_residue(&crate::commands::maint::scratch_gc::ScratchRoots::from_env()),
    },
];

/// Os nomes que o `--check` aceita, na ordem da lista.
fn names_of(list: &[Check]) -> Vec<&'static str> {
    list.iter()
        .filter_map(|item| match item.runs {
            Runs::Named(name) => Some(name),
            Runs::Round | Runs::Residue => None,
        })
        .collect()
}

/// O leitor do `--check` montado a partir de uma lista: recusa na linha de
/// comando o nome que ela não tem, em vez de o comando responder um relatório
/// vazio que se lê como "está tudo certo".
fn parser_of(list: &[Check]) -> clap::builder::PossibleValuesParser {
    clap::builder::PossibleValuesParser::new(names_of(list))
}

/// O leitor do `--check` da linha de comando.
#[must_use]
pub fn check_parser() -> clap::builder::PossibleValuesParser {
    parser_of(CHECKS)
}

/// A conferência de nome `name`, ou a mensagem que diz quais existem.
fn pick<'a>(list: &'a [Check], name: &str) -> Result<&'a Check, String> {
    list.iter().find(|item| matches!(item.runs, Runs::Named(known) if known == name)).ok_or_else(|| {
        format!("doctor: unknown check '{name}'. Known: {}", names_of(list).join(", "))
    })
}

/// As conferências da rodada inteira, na ordem da lista; as das sobras só
/// quando pedidas.
fn round(list: &[Check], residue: bool) -> impl Iterator<Item = &Check> {
    list.iter().filter(move |item| item.runs != Runs::Residue || residue)
}

/// O que o diagnóstico responde antes de imprimir: o resultado da conferência
/// pedida ou de toda a rodada, ou a recusa de um nome que a lista não tem.
fn answer(list: &[Check], opts: &DoctorOpts, place: &Place) -> Result<Vec<CheckResult>, String> {
    match &opts.check {
        Some(name) => pick(list, name).map(|item| vec![(item.check)(place)]),
        None => Ok(round(list, opts.residue).map(|item| (item.check)(place)).collect()),
    }
}

/// Dispatch `mustard-rt run doctor [--residue] [--check <CHECK>] [--format json|--json]`.
pub fn run(opts: DoctorOpts) {
    let results = match answer(CHECKS, &opts, &Place::here()) {
        Ok(results) => results,
        Err(unknown) => {
            eprintln!("{unknown}");
            std::process::exit(1);
        }
    };
    if opts.format == "json" {
        render_report_json(&results);
    } else {
        render_report(&results);
    }
    // Uma conferência pedida sozinha só informa; a rodada inteira sai com 1
    // quando alguma falha.
    if opts.check.is_none() && results.iter().any(|r| r.status == Status::Fail) {
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
    use std::path::Path;

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

    /// Uma conferência de mentira somada à lista chega aos quatro lugares sem
    /// mexer em mais nada: aos nomes que o `--check` aceita na linha de
    /// comando, à escolha dele, à mensagem de conferência desconhecida e à
    /// rodada inteira. E a linha de comando de verdade lê a mesma lista.
    #[test]
    fn a_check_added_to_the_list_reaches_the_four_places_with_nothing_else_touched() {
        const FAKE: &str = "de-mentira";
        let mut list = CHECKS.to_vec();
        list.push(Check { runs: Runs::Named(FAKE), check: |_| CheckResult::ok(FAKE) });
        let dir = tempdir().unwrap();
        let place = Place { cwd: dir.path().to_path_buf(), claude_dir: dir.path().join(".claude") };
        let opts = |check: Option<&str>| DoctorOpts {
            residue: false,
            check: check.map(str::to_string),
            format: "text".into(),
        };

        let accepts = |list: &[Check]| {
            clap::Command::new("doctor")
                .arg(clap::Arg::new("check").long("check").value_parser(parser_of(list)))
                .try_get_matches_from(["doctor", "--check", FAKE])
                .is_ok()
        };
        assert!(accepts(&list), "the command line refuses the check the list has");
        assert!(!accepts(CHECKS), "the command line takes a check the list does not have");

        let alone = answer(&list, &opts(Some(FAKE)), &place).unwrap();
        assert_eq!(alone.iter().map(|r| r.name).collect::<Vec<_>>(), [FAKE], "--check {FAKE} runs something else");

        let unknown = answer(&list, &opts(Some("nenhuma")), &place).err().unwrap();
        assert!(unknown.contains(FAKE), "the unknown-check message does not name {FAKE}: {unknown}");
        assert!(unknown.contains("claude-md"), "the unknown-check message lost the checks of the list: {unknown}");

        let all = answer(&list, &opts(None), &place).unwrap();
        let names: Vec<&str> = all.iter().map(|r| r.name).collect();
        assert_eq!(names.last(), Some(&FAKE), "the full run leaves {FAKE} out: {names:?}");
        assert_eq!(all.len(), round(CHECKS, false).count() + 1, "the full run changed more than the new check");

        let run = <crate::commands::doctor::cli::DoctorCmd as clap::Subcommand>::augment_subcommands(
            clap::Command::new("run"),
        );
        let doctor = run.find_subcommand("doctor").expect("the doctor is registered");
        let check = doctor.get_arguments().find(|arg| arg.get_id() == "check").expect("--check is declared");
        let taken: Vec<String> = check.get_possible_values().iter().map(|v| v.get_name().to_string()).collect();
        assert_eq!(taken, names_of(CHECKS), "the real --check does not read the list of checks");
    }

    /// Nenhum arquivo do diagnóstico passa do teto de linhas de código: a porta e
    /// cada parte da pasta dela, pela medida única do núcleo.
    #[test]
    fn no_file_of_the_doctor_goes_over_the_code_line_cap() {
        let gate = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("commands").join("doctor").join("doctor.rs");
        assert_eq!(mustard_core::io::fs::files_over_code_line_cap(&gate), Ok(Vec::new()));
    }
}
