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
//!   `clean` recolheria e o da compilação compartilhada, pela MESMA
//!   varredura dele. WARN quando há sobra ou quando a compilação passou do teto.
//! - **binaries** — o commit e a data de três programas: o que roda o
//!   diagnóstico, o `mustard-rt` do plugin instalado e o primeiro do `PATH`.
//!   WARN quando dois vêm de commits diferentes ou quando um não diz o commit.
//! - **drift** — compare by hash the folders a fresh payload owns
//!   (`CORE_FOLDERS`) between the installed `.claude/` and the
//!   `templates/` source. Degrades to `skip` when `templates/` is not
//!   reachable from cwd (consumer project).
//! - **state health** — orphan `.pipeline-states/` files (no matching active
//!   spec), expired `closed-followup` state files, missing
//!   `grain.db`. WARN per anomaly.
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
//!   difere, e linha sem o campo `search`. Só lê e acusa, com
//!   WARN e a mensagem no idioma do projeto, que manda rodar
//!   `mustard-rt run index`.
//! - **switches** — as escolhas do `mustard.json` contra as configurações
//!   locais: WARN quando o Mustard está desligado no projeto, quando a opção
//!   `rtk` e o gancho do rtk divergem, e quando a assinatura do Claude Code
//!   está ligada.
//! - **claude-md** — sobras do Mustard em arquivos que não são dele (as marcas
//!   nos `CLAUDE.md`, as linhas do molde no `settings.json` da equipe) e as
//!   regras de bloqueio desse arquivo escritas errado, que o `upsert` troca no
//!   lugar, pela mesma lista que o `upsert` mostra.
//!
//! ## Onde cada conferência mora
//!
//! Esta porta guarda o que todas dividem e o que sai para fora — o resultado
//! de uma conferência, os eventos de gancho que o binário conhece, as opções e
//! a lista das conferências, na ordem em que o comando as roda. Cada
//! conferência mora numa parte da pasta ao lado, por assunto: a ligação dos
//! ganchos (`wiring`), as sobras (`residue`), o commit dos três programas
//! (`binaries`), o desvio dos moldes (`drift`), o
//! que a máquina tem instalado (`host`), a proteção das bases (`protection`),
//! o estado das specs (`specs`), o que o Mustard deixa no projeto (`project`)
//! e a saída do relatório (`report`).

mod binaries;
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

use binaries::check_binaries;
use drift::check_drift;
use host::{check_claude_cli, check_effort_env, check_nerd_font, check_rtk, lsp_check};
use project::{check_claude_md, check_merged_branches, check_project_version, check_scan_output, check_switches};
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

/// O manifesto de ganchos que o plugin entrega (`plugin/hooks/hooks.json`),
/// embutido na compilação. Esse arquivo é o único que decide quais nomes de
/// `<evento>` o Claude Code entrega a `mustard-rt on`; embuti-lo faz o
/// doutor ler o mesmo artefato que o Claude Code lê, como a conferência da
/// fiação lê, em `known_run_subcommands`, a mesma árvore do clap em que o
/// binário despacha. O conjunto de eventos vem dele e não de uma lista à mão,
/// que sairia de sincronia: `PreCompact`, `Stop` e `WorktreeCreate` entram
/// porque o manifesto os registra.
///
/// O script de build o copia antes para `OUT_DIR`: embutido de `plugin/`,
/// fora deste crate, o cargo o guardava pelo caminho absoluto da cópia que o
/// compilou, e trocar de cópia recompilava o crate.
const SHIPPED_HOOKS_MANIFEST: &str = include_str!(concat!(env!("OUT_DIR"), "/hooks.json"));

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
    pub root: Option<PathBuf>,
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
    fn at(root: Option<&std::path::Path>) -> Self {
        let cwd = root.map(std::path::Path::to_path_buf).unwrap_or_else(|| crate::shared::context::env::workspace_root_strict()
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))));
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
    Check {runs:Runs::Named("session-plugins"),check:|place| session_plugins(crate::shared::host_installation::snapshot(&place.project().root))},
    Check {runs:Runs::Named("hook-roots"),check:|_| hook_roots(mustard_core::claude_config_dir().as_deref())},
    Check {runs:Runs::Named("integrations"),check:|place| {
        let report=crate::shared::integration_health::snapshot(&place.project().root);
        if report["ok"]==true {CheckResult::ok("integrations")}
        else {CheckResult::warn("integrations",vec![report.to_string()])}
    }},
    Check { runs: Runs::Round, check: |place| check_wiring(&place.claude_dir) },
    // Antes do desvio dos moldes: a leitura dele só vale quando o binário que
    // a faz é o instalado, e um arranque parado torna suspeita toda versão
    // lida depois.
    Check {
        runs: Runs::Round,
        check: |place| bootstrap_to_check_result(&crate::commands::doctor::bootstrap_check::run(&place.cwd)),
    },
    // De que commit vêm o programa em uso, o do plugin e o do sistema.
    Check { runs: Runs::Named("binaries"), check: |place| check_binaries(place.project().lang) },
    Check { runs: Runs::Round, check: |place| check_drift(&place.claude_dir) },
    Check { runs: Runs::Round, check: |place| check_state_health(&place.claude_dir) },
    Check { runs: Runs::Round, check: |_| check_claude_cli() },
    Check { runs: Runs::Round, check: |place| lsp_check(&place.cwd) },
    Check { runs: Runs::Round, check: |_| check_rtk() },
    Check { runs: Runs::Round, check: |_| check_nerd_font() },
    Check { runs: Runs::Round, check: |_| check_effort_env() },
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
    // As branches já mergeadas que seguem vivas e a versão gravada no
    // projeto, que o início da sessão deixou de dizer: só acusam.
    Check {
        runs: Runs::Round,
        check: |place| {
            let project = place.project();
            check_merged_branches(&project.root, project.lang)
        },
    },
    Check {
        runs: Runs::Round,
        check: |place| {
            let project = place.project();
            check_project_version(&project.root, project.lang)
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

/// As pastas de plugin com ganchos do Mustard que a pasta de configuração
/// `config` guarda, cada uma com a versão que o caminho dela diz.
///
/// Mais de uma é aviso: cada uma põe os ganchos dela na sessão, e o usuário
/// recebe tudo em dobro. Foi o que aconteceu com a cópia de desenvolvimento ao
/// lado da instalada. Uma só, ou nenhuma, passa.
fn hook_roots(config: Option<&std::path::Path>) -> CheckResult {
    let found = mustard_hook_roots(config);
    if found.len() < 2 {
        return CheckResult::ok("hook-roots");
    }
    let listed: Vec<String> = found.iter().map(|(path, version)| format!("{version}: {path}")).collect();
    CheckResult::warn("hook-roots", vec![format!("more than one plugin root with Mustard hooks ({}): {}", found.len(), listed.join(" · "))])
}

/// Cada pasta de plugin sob `config` cujo manifesto de ganchos
/// (`hooks/hooks.json`) chama o `mustard-boot`, com a versão que o caminho
/// dela diz, em ordem de caminho. Sem pasta de configuração, nenhuma.
///
/// O manifesto é lido, e não o nome da pasta: o plugin de outro produto que
/// tenha `mustard` no caminho não conta, e a cópia de desenvolvimento, que
/// tem outro nome, conta.
fn mustard_hook_roots(config: Option<&std::path::Path>) -> Vec<(String, String)> {
    let Some(config) = config else {
        return Vec::new();
    };
    let mut found: Vec<(String, String)> = Vec::new();
    let mut stack = vec![config.join("plugins")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let manifest = path.join("hooks").join("hooks.json");
            match std::fs::read_to_string(&manifest) {
                Ok(text) if text.contains("mustard-boot") => {
                    let version = path.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
                    found.push((path.to_string_lossy().to_string(), version));
                }
                _ => stack.push(path),
            }
        }
    }
    found.sort();
    found
}

/// A pasta do plugin que cada sessão do projeto usa, com o binário que o
/// gancho dela executa e a linha de versão dele. Sem sessão observada, pula;
/// com uma pasta de outra versão que a deste binário, ou sem o módulo do
/// painel, avisa.
fn session_plugins(mut report: serde_json::Value) -> CheckResult {
    crate::shared::host_installation::add_binary_versions(&mut report);
    let rows=report["observations"].as_array();
    let status=if rows.is_none_or(Vec::is_empty) {Status::Skip}
        else if rows.is_some_and(|rows|rows.iter().any(|row|row["matches_running_version"]==false || row["mods_module_present"]==false)) {Status::Warn}
        else {Status::Ok};
    CheckResult {name:"session-plugins",status,details:vec![report.to_string()]}
}

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
    let results = match answer(CHECKS, &opts, &Place::at(opts.root.as_deref())) {
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
        // A project map, to keep state-health from warning.
        mustard_core::io::project_map::write_text(dir.path(), "{}").unwrap();

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

    /// A rodada inteira, pela LISTA de verdade — não uma chamada avulsa a
    /// `check_rtk()` —, traz uma conferência chamada `rtk`. Tirar o item da
    /// lista `CHECKS` derruba este teste; tirar só a chamada avulsa não.
    #[test]
    fn the_full_round_includes_the_rtk_check() {
        let dir = tempdir().unwrap();
        let place = Place { cwd: dir.path().to_path_buf(), claude_dir: dir.path().join(".claude") };
        let opts = DoctorOpts { root: None, residue: false, check: None, format: "text".into() };
        let all = answer(CHECKS, &opts, &place).unwrap();
        assert!(all.iter().any(|r| r.name == "rtk"), "expected a check named 'rtk' in the round: {:?}", all.iter().map(|r| r.name).collect::<Vec<_>>());
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
            root: None,
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

    /// A sessão cujo processo o cache registra como usuário de uma pasta do
    /// plugin aponta para essa pasta, com o binário que o gancho roda e a linha
    /// de versão que ele responde, perguntada fora do projeto e sem passar a
    /// chamada adiante. A pasta sincronizada dos argumentos, sem binário, não
    /// aparece; nem a pasta antiga cujo registro é de um processo morto com o
    /// mesmo número.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_session_points_at_the_cache_folder_that_records_it_and_says_its_binary_version() {
        use std::os::unix::fs::symlink;
        let dir = tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let manifest = r#"{"name":"mustard","version":"0.2.21"}"#;
        let cache = dir.path().join("config/plugins/cache/mustard-local/mustard");
        let (current, stale) = (cache.join("0.2.21"), cache.join("0.2.10"));
        let synced = dir.path().join("remote/plugins/7c685082/e8c99c382fbeaa27");
        for plugin in [&current, &stale, &synced] {
            write_file(&plugin.join(".claude-plugin/plugin.json"), manifest);
        }
        write_file(&synced.join("bin/mustard-boot"), "#!/bin/sh\n");
        write_file(&current.join(".in_use/21"), r#"{"pid":21,"procStart":"3308387"}"#);
        write_file(&stale.join(".in_use/21"), r#"{"pid":21,"procStart":"53166493"}"#);
        write_file(&current.join("hooks/register.js"), "export default () => {};");
        // O binário é gravado por um shell à parte: os testes rodam em paralelo
        // no mesmo processo, e o arquivo que este processo mantém aberto para
        // escrita o Linux recusa rodar ("Text file busy").
        for plugin in [&current, &stale] {
            std::fs::create_dir_all(plugin.join("bin")).unwrap();
            let written = std::process::Command::new("/bin/sh")
                .args(["-c", "printf '%s' \"$2\" > \"$1\" && chmod 755 \"$1\"", "sh"])
                .arg(plugin.join("bin/mustard-rt"))
                .arg("#!/bin/sh\nprintf 'mustard-rt 0.2.21 (build test) delegated=%s cwd=%s\\n' \"$MUSTARD_RT_DELEGATED\" \"$(pwd -P)\"\n")
                .status()
                .unwrap();
            assert!(written.success());
        }
        let executable = dir.path().join("ccd-cli/2.1.295");
        write_file(&executable, "");
        let process = dir.path().join("proc/21");
        std::fs::create_dir_all(&process).unwrap();
        symlink(&root, process.join("cwd")).unwrap();
        symlink(&executable, process.join("exe")).unwrap();
        std::fs::write(process.join("cmdline"), format!("claude\0--plugin-dir={}\0", synced.display())).unwrap();
        std::fs::write(process.join("stat"), "21 (claude) S 1 21 21 0 -1 4194560 100 0 0 0 1 1 0 0 20 0 8 0 3308387 123 456\n").unwrap();

        let checked = session_plugins(crate::shared::host_installation::inspect(
            &dir.path().join("proc"),
            Some(&dir.path().join("config")),
            &root,
        ));
        let report: serde_json::Value = serde_json::from_str(&checked.details[0]).unwrap();
        let rows = report["observations"].as_array().unwrap();
        assert_eq!(rows.len(), 1, "{report}");
        assert_eq!(rows[0]["plugin_root"], current.display().to_string());
        assert_eq!(rows[0]["source"], "plugin-cache-in-use");
        assert_eq!(rows[0]["rt_binary"], current.join("bin/mustard-rt").display().to_string());
        let version = rows[0]["rt_version"].as_str().unwrap_or_else(|| panic!("no binary version: {report}"));
        let asked_from = version.strip_prefix("mustard-rt 0.2.21 (build test) delegated=1 cwd=").unwrap_or_else(|| panic!("{version}"));
        assert_eq!(Path::new(asked_from), std::env::temp_dir().canonicalize().unwrap(), "the version is asked from the temporary folder, outside any project");
        assert!(!checked.details[0].contains(&synced.display().to_string()), "{report}");
        assert!(!checked.details[0].contains(&stale.display().to_string()), "{report}");
    }

    /// Nenhum arquivo do diagnóstico passa do teto de linhas de código: a porta e
    /// cada parte da pasta dela, pela medida única do núcleo.
    #[test]
    fn no_file_of_the_doctor_goes_over_the_code_line_cap() {
        let gate = crate::manifest_dir::manifest_dir().join("src").join("commands").join("doctor").join("doctor.rs");
        assert_eq!(mustard_core::io::fs::files_over_code_line_cap(&gate), Ok(Vec::new()));
    }
    /// Mais de uma pasta de plugin com ganchos do Mustard é aviso, e a linha
    /// nomeia cada uma com a versão dela: cada pasta põe os ganchos dela na
    /// sessão, e o usuário recebe tudo em dobro. Uma só passa. O plugin de
    /// outro produto não conta, porque o manifesto dele não chama o
    /// `mustard-boot`.
    ///
    /// Em campo (21/10/2026) a cópia de desenvolvimento ficou ao lado da
    /// instalada, e o usuário recebeu cada aviso duas vezes.
    #[test]
    fn more_than_one_plugin_root_with_mustard_hooks_is_a_warning() {
        let dir = tempfile::tempdir().expect("temp");
        let config = dir.path();
        let put = |path: &str, text: &str| {
            let file = config.join("plugins").join(path).join("hooks").join("hooks.json");
            std::fs::create_dir_all(file.parent().expect("a pasta")).expect("pasta");
            std::fs::write(&file, text).expect("manifesto");
        };
        let mustard = r#"{"hooks":{"PreToolUse":[{"command":"${CLAUDE_PLUGIN_ROOT}/bin/mustard-boot on PreToolUse"}]}}"#;
        put("cache/mustard-local/mustard/0.2.25", mustard);
        put("outro/plugin-de-outro-produto", r#"{"hooks":{"Stop":[{"command":"./bin/outro"}]}}"#);

        // Uma só raiz do Mustard: nada a dizer.
        assert_eq!(hook_roots(Some(config)).status, Status::Ok);

        put("cache/mustard-local/mustard/0.2.26", mustard);
        let said = hook_roots(Some(config));
        let line = said.details.join(" ");
        assert_eq!(said.status, Status::Warn, "{line}");
        assert!(line.contains("0.2.25") && line.contains("0.2.26"), "a linha diz a versão de cada uma: {line}");
        assert!(!line.contains("outro-produto"), "o plugin de outro produto não conta: {line}");

        // Sem pasta de configuração, nada a dizer.
        assert_eq!(hook_roots(None).status, Status::Ok);
    }

}
