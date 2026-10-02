//! The `run` subcommands for installation maintenance (`maint/`).
//!
//! A new command takes its variant in [`MaintCmd`] and its arm in
//! [`dispatch`] below (the compiler demands the arm), its line in
//! `tests/fixtures/run-surface.txt`, which `tests/run_command_surface.rs`
//! compares with the clap tree, and a caller in the product text, which
//! `tests/template_parity.rs` demands with no exception list.
//!
//! [`crate::commands::RunCmd`] hoists this enum with `#[command(flatten)]`, so
//! every name stays FLAT: `mustard-rt run <name>`, never `run maint <name>`.
//! `display_order` pins each command to its historical slot in the flat
//! `run --help` listing (clap sorts subcommands by `(display_order, name)`) -
//! splitting the god-enum into families must not reshuffle the published CLI.

use clap::Subcommand;
use std::path::PathBuf;

use crate::commands::{maint};

/// The `run` subcommands owned by installation maintenance (`maint/`).
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum - clap-Subcommand; boxing breaks derive
pub enum MaintCmd {
    /// Recolhe as cópias descartáveis que os agentes deixam no diretório
    /// temporário (ou no `scratchpad/` de uma sessão do Claude Code): pasta
    /// com cópia deste projeto ou `target/` de compilação, sem mudança há
    /// mais de 12 horas, e que não é a da sessão atual.
    ///
    /// Só lista por padrão; `--apply` apaga as listadas e esvazia a
    /// compilação compartilhada `~/.cache/mustard/scratch-target` acima de
    /// 8 GB. `--path <dir>` apaga uma pasta só, sem o filtro de idade, depois
    /// de conferir que ela está no temp e é uma cópia — fora do temp é
    /// recusado (exit 1). A exclusão é do próprio binário, nunca de shell.
    ///
    /// Sem `--path`, lista também as cópias de obra do projeto da pasta
    /// atual: saem as de obra fechada, descartada ou que não existe mais, sem
    /// regra de idade; a de obra aberta fica. A cópia da página de um
    /// descarte sai só com mais de um dia; a de descarte recém-feito fica. A
    /// pasta principal nunca entra.
    #[command(name = "clean")]
    #[command(display_order = 18)]
    ScratchGc {
        /// Só lista, sem apagar nada (o padrão). Não combina com `--apply`
        /// nem com `--path`: pedir para só listar e apontar uma pasta para
        /// apagar é contraditório, e a exclusão não tem volta — o parser
        /// recusa a chamada (exit 2) antes de qualquer coisa ser tocada.
        #[arg(long, default_value_t = true, conflicts_with_all = ["apply", "path"])]
        dry_run: bool,
        /// Apaga as candidatas listadas. Obrigatório para mexer no disco.
        #[arg(long)]
        apply: bool,
        /// Apaga só esta pasta, conferida, sem o filtro de idade. Não combina
        /// com `--apply`: são dois modos, e um calado pelo outro engana.
        #[arg(long, conflicts_with = "apply")]
        path: Option<PathBuf>,
    },
    /// Roda uma régua de medida (um teste ignorado) compilando o código
    /// certo, e grava com o número a versão que o gerou.
    ///
    /// `<teste>` é o nome da régua, como `measure_the_spend_of_the_search`; o
    /// comando acha o pacote dela no código (`--package` diz qual, quando o
    /// nome está em mais de um). Sem `--commit` mede a pasta atual: o commit é
    /// o `HEAD`, e o que falta comitar entra como um resumo no nome da pasta e
    /// na prova. Com `--commit <sha>` separa o código daquele commit numa cópia
    /// própria, sem mexer na pasta atual.
    ///
    /// Cada código compila na sua pasta, `~/.cache/mustard/medida/<commit>`
    /// (`MUSTARD_MEASURE_DIR` troca a base): o mesmo código reaproveita a
    /// compilação, e código diferente nunca divide pasta. O programa de teste
    /// vem do JSON do cargo, nunca do mais novo da pasta, e a régua mora no
    /// `src/` da biblioteca do pacote. Roda com o `scan` compilado do mesmo
    /// código ao lado e com o commit, o sujo e o resumo em variáveis
    /// `MUSTARD_MEASURE_*`; a régua que não imprime a linha `PROVA` falha.
    ///
    /// Com `--trees <pasta>` (ou `SPEND_TREES` em `--env`), o mapa de cada
    /// projeto da pasta é refeito antes da régua: o banco velho sai e o `scan`
    /// compilado do mesmo código grava um novo, e a medida espera a leitura da
    /// história de cada mapa terminar antes de rodar a régua; o scan que falha
    /// recusa a medida. A linha `PROVA` traz também `gancho=<commit>`: o
    /// commit que o `mustard-rt` do plugin instalado carimbou em si, ou `não
    /// instalado`.
    ///
    /// Imprime ao fim as linhas `PROVA`, uma linha `PECAS` por mapa que a
    /// régua abriu (o estado de cada peça da busca nele, ligada ou ainda não
    /// ligada) e o caminho do resultado (`--out`, ou `<pasta>/<teste>.json`,
    /// que a régua lê em `MUSTARD_MEASURE_OUT`). Ao terminar, só as três
    /// pastas de medida usadas por último ficam. Só mede
    /// o código-fonte do Mustard: em outro projeto recusa (exit 1).
    #[command(display_order = 22)]
    Measure {
        /// O nome da régua: o teste ignorado a rodar.
        test: String,
        /// Mede o código deste commit, numa cópia própria, em vez da pasta atual.
        #[arg(long, value_name = "sha")]
        commit: Option<String>,
        /// O pacote que tem a régua; sem ele o comando a procura no código.
        #[arg(long, value_name = "pacote")]
        package: Option<String>,
        /// Uma variável de ambiente para a régua, `CHAVE=VALOR`; repete-se. As
        /// `MUSTARD_MEASURE_*` são do comando e se recusam.
        #[arg(long = "env", value_name = "K=V")]
        env: Vec<String>,
        /// O arquivo onde a régua grava o resultado.
        #[arg(long, value_name = "arquivo")]
        out: Option<PathBuf>,
        /// A pasta com a árvore de cada projeto da régua (`SPEND_TREES`): o
        /// mapa de cada uma é refeito antes da medida, com o `scan` compilado
        /// do mesmo código, e a medida espera a história de cada mapa terminar
        /// antes da régua.
        #[arg(long, value_name = "pasta")]
        trees: Option<PathBuf>,
    },
    /// Install or update Mustard in the current project (the plugin's
    /// bootstrap door).
    ///
    /// Idempotent. The settings file — `.claude/settings.local.json`, since
    /// the install is always private-mode and never touches the shared
    /// `.claude/settings.json` — plus `.claude/.gitignore` and the
    /// project-root `mustard.json` are yours and are merged, never clobbered:
    /// an existing file is preserved, only what is missing is created or
    /// backfilled. Mustard's own texts are always rewritten, in the language
    /// of `language.text`: the session map `.claude/mustard/session-map.md`,
    /// the two page templates under `.claude/mustard/pages/` and the three
    /// agents under `.claude/agents/mustard/`. They are the harness's own
    /// text, not project configuration. So a copy you edited is replaced and
    /// listed under `updated`. One that already matched the shipped text comes
    /// back under `preserved`, because there was nothing left to write. Emits the
    /// `UpsertReport` as deterministic pretty JSON.
    ///
    /// What an older Mustard wrote into files that are not its own — the
    /// marks in the `CLAUDE.md` files, the seed's lines in the team's
    /// `.claude/settings.json`, a planted `.claude/CLAUDE.md` — leaves in the
    /// same call, with no question: the rules of the Guards go first to the
    /// project's pending list, in one item, never to the lesson bank, and
    /// `cleanup` and `cleaned` say what left. A file without a mark is only
    /// listed. The commit stays with the person.
    ///
    /// The local settings also allow the folder of the project's separate
    /// copies, which live outside the project. While `mustard.json` has no
    /// `localFiles`, the answer carries `localFilesFound`: the files git
    /// ignores outside an ignored folder (such as `.env`), for the person to
    /// confirm once. `--local-files` records the confirmed list and
    /// `--prepare` the command that prepares each copy before it compiles.
    #[command(display_order = 19)]
    Upsert {
        /// The local files each copy receives, comma-separated and relative
        /// to the project root, as the person confirmed them; an empty value
        /// records that the project needs none.
        #[arg(long, value_name = "a,b")]
        local_files: Option<String>,
        /// The command that brings the dependencies into each copy, such as
        /// `npm ci`; an empty value records that the project has none.
        #[arg(long, value_name = "command")]
        prepare: Option<String>,
    },
}

/// Dispatch one `maint`-family `run` subcommand.
pub fn dispatch(cmd: MaintCmd) {
    match cmd {
        MaintCmd::ScratchGc { dry_run, apply, path } => {
            // `dry_run` vale `true` por padrão e o `conflicts_with_all` recusa
            // `--dry-run` junto de `--apply` OU de `--path`: quando um dos dois
            // chega aqui, `dry_run` é só o padrão, nunca um pedido explícito.
            // Por isso descartá-lo é seguro — quem decide é `--apply`/`--path`.
            let _ = dry_run;
            maint::scratch_gc::run(maint::scratch_gc::ScratchGcOpts { apply, path });
        }
        MaintCmd::Measure { test, commit, package, env, out, trees } => {
            maint::measure::run(&maint::measure::MeasureOpts { test, commit, package, env, out, trees });
        }
        MaintCmd::Upsert { local_files, prepare } => {
            maint::upsert::run(&maint::upsert::UpsertOpts { local_files, prepare });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::MaintCmd;
    use clap::Parser;

    /// A wrapper so the family enum can be parsed on its own — the binary's own
    /// `Cli` lives in `main.rs` and is out of reach from the lib.
    #[derive(Parser)]
    struct Probe {
        #[command(subcommand)]
        cmd: MaintCmd,
    }

    /// `--dry-run --path X` apagava a pasta: o `dry_run` só conflitava com
    /// `--apply` e o dispatch o descarta. Agora o parser recusa a combinação,
    /// e o descarte no dispatch só vê o valor padrão.
    #[test]
    fn scratch_gc_dry_run_conflicts_with_path_and_apply() {
        let parse = |args: &[&str]| {
            let mut argv = vec!["probe", "clean"];
            argv.extend_from_slice(args);
            Probe::try_parse_from(argv)
        };
        assert!(parse(&["--dry-run", "--path", "/tmp/x"]).is_err(), "--dry-run with --path must be refused");
        assert!(parse(&["--dry-run", "--apply"]).is_err(), "--dry-run with --apply must be refused");
        assert!(parse(&["--path", "/tmp/x", "--apply"]).is_err(), "--path with --apply must be refused");

        let Ok(Probe { cmd: MaintCmd::ScratchGc { path, apply, .. } }) = parse(&["--path", "/tmp/x"]) else {
            panic!("--path alone must parse");
        };
        assert_eq!(path.as_deref(), Some(std::path::Path::new("/tmp/x")));
        assert!(!apply);
        assert!(parse(&["--apply"]).is_ok());
        assert!(parse(&["--dry-run"]).is_ok());
        assert!(parse(&[]).is_ok());
    }

    /// A limpeza das sobras não pede código: o `upsert` não aceita mais o
    /// `--confirm`.
    #[test]
    fn upsert_takes_no_confirm_code() {
        assert!(Probe::try_parse_from(["probe", "upsert"]).is_ok());
        assert!(Probe::try_parse_from(["probe", "upsert", "--confirm", "abcd1234"]).is_err());
    }

    /// O `upsert` recebe a lista confirmada e o comando de preparo, e o valor
    /// vazio chega como resposta, não como falta dela.
    #[test]
    fn upsert_takes_the_local_files_and_the_prepare_command() {
        let Ok(Probe { cmd: MaintCmd::Upsert { local_files, prepare } }) = Probe::try_parse_from([
            "probe",
            "upsert",
            "--local-files",
            ".env,apps/api/.env.local",
            "--prepare",
            "pnpm install --frozen-lockfile",
        ]) else {
            panic!("both options must parse");
        };
        assert_eq!(local_files.as_deref(), Some(".env,apps/api/.env.local"));
        assert_eq!(prepare.as_deref(), Some("pnpm install --frozen-lockfile"));

        let Ok(Probe { cmd: MaintCmd::Upsert { local_files, prepare } }) =
            Probe::try_parse_from(["probe", "upsert", "--local-files", "", "--prepare", ""])
        else {
            panic!("empty answers must parse");
        };
        assert_eq!((local_files.as_deref(), prepare.as_deref()), (Some(""), Some("")));
    }

    /// O `measure` pede o nome da régua e aceita o commit, o pacote, o
    /// resultado e as variáveis, estas quantas vezes se repetirem.
    #[test]
    fn measure_takes_the_test_and_its_options() {
        let Ok(Probe { cmd: MaintCmd::Measure { test, commit, package, env, out, trees } }) = Probe::try_parse_from([
            "probe",
            "measure",
            "measure_the_spend_of_the_search",
            "--commit",
            "abc1234",
            "--package",
            "mustard-rt",
            "--env",
            "A=1",
            "--env",
            "B=2",
            "--out",
            "/tmp/saida.json",
            "--trees",
            "/tmp/arvores",
        ]) else {
            panic!("the full form must parse");
        };
        assert_eq!(test, "measure_the_spend_of_the_search");
        assert_eq!((commit.as_deref(), package.as_deref()), (Some("abc1234"), Some("mustard-rt")));
        assert_eq!(env, ["A=1", "B=2"]);
        assert_eq!(out.as_deref(), Some(std::path::Path::new("/tmp/saida.json")));
        assert_eq!(trees.as_deref(), Some(std::path::Path::new("/tmp/arvores")));

        assert!(Probe::try_parse_from(["probe", "measure", "so_o_nome"]).is_ok());
        assert!(Probe::try_parse_from(["probe", "measure"]).is_err(), "sem o nome da régua não há o que medir");
    }
}
