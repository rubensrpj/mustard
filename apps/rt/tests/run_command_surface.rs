//! O retrato da superfície publicada de `mustard-rt run`.
//!
//! Os nomes de `run <nome>` são chamados pelos ganchos, pelo `settings.json`,
//! pelos moldes e pela prosa do produto: um renome ou um registro perdido não
//! quebra a compilação — faz o comando SUMIR em tempo de execução. Este arquivo
//! transforma isso numa falha de teste.
//!
//! O retrato, e não uma lista escrita à mão: a superfície vive no arquivo
//! `tests/fixtures/run-surface.txt`, uma linha por nome em ordem alfabética, e
//! o teste compara a árvore do clap com ele. Quem acrescenta ou tira um comando regrava o arquivo
//! com o texto que a falha imprime — nada de manter a mesma lista em dois
//! lugares.
//!
//! O texto entregue que promete um comando que o leitor não vai achar é
//! conferido por `template_parity.rs`, que lê `plugin/**` e os moldes contra
//! a árvore do clap.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "support/manifest_dir.rs"]
mod manifest_dir;
#[path = "support/executable.rs"]
mod executable;

use std::fs;
use std::path::{Path, PathBuf};

use clap::{Command, Subcommand};
use mustard_rt::commands::RunCmd;

/// O retrato da superfície, relativo à raiz do repositório.
const SURFACE_SNAPSHOT: &str = "apps/rt/tests/fixtures/run-surface.txt";

/// The repo root, resolved from this crate (`apps/rt`) so the scan does not
/// depend on the directory the test runner happens to start in.
fn repo_root() -> PathBuf {
    manifest_dir::manifest_dir().join("../..")
}

/// Os nomes gravados no retrato, em ordem alfabética.
fn snapshot_names() -> Vec<String> {
    let path = repo_root().join(SURFACE_SNAPSHOT);
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("o retrato da superfície não abriu: {}", path.display()));
    text.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()
}

/// The `run` subcommand tree as clap materialises it.
fn run_command_tree() -> Command {
    let mut cmd = RunCmd::augment_subcommands(Command::new("run"));
    // `build()` materialises what the parser/help actually expose (it is what
    // adds the auto-generated `help` subcommand).
    cmd.build();
    cmd
}

/// A árvore do clap é igual ao retrato gravado, nome por nome.
#[test]
fn published_surface_equals_the_snapshot() {
    let cmd = run_command_tree();
    let mut current: Vec<String> =
        cmd.get_subcommands().map(|c| c.get_name().to_string()).collect();
    current.sort();

    assert_eq!(
        current,
        snapshot_names(),
        "a superfície de `run` mudou. Se a mudança é a pretendida, regrave \
         {SURFACE_SNAPSHOT} com estes nomes, um por linha:\n{}",
        current.join("\n")
    );
}

/// Dois comandos no mesmo lugar da lista fariam o `run --help` embaralhar
/// sozinho: o clap ordena por `(display_order, name)`.
#[test]
fn no_command_shares_the_place_of_another_in_the_help() {
    let cmd = run_command_tree();
    let mut slots: Vec<usize> = cmd
        .get_subcommands()
        .filter(|c| c.get_name() != "help")
        .map(clap::Command::get_display_order)
        .collect();
    slots.sort_unstable();
    let mut unique = slots.clone();
    unique.dedup();
    assert_eq!(slots, unique, "dois comandos declaram o mesmo `display_order`");
}

/// Quem vai mexer numa função pergunta ao mapa quem a usa, pelo comando que a
/// pessoa roda: `run map users --name <declaração>`. A resposta cita onde a
/// declaração mora e cada uso, como `arquivo:linha:quem chama`; um nome que o
/// mapa não declara é recusado com o texto de declaração desconhecida, que
/// diz que o mapa não a tem, sem citar arquivo.
#[test]
fn map_returns_who_uses_a_declaration_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // O mapa como o scan o grava: `total` em src/preco.rs, usada duas vezes
    // por `fechar`, em src/pedido.rs.
    mustard_core::io::project_map::write_text(
        root,
        r#"{"modules": [
             {"path": "src/preco.rs", "loc": 5, "declarations": [
               {"kind": "function", "name": "total", "line": 1, "end_line": 3,
                "used_by": ["src/pedido.rs:5:fechar", "src/pedido.rs:6:fechar"]}]},
             {"path": "src/pedido.rs", "loc": 8, "declarations": [
               {"kind": "function", "name": "fechar", "line": 4, "end_line": 7}]}
           ]}"#,
    )
    .unwrap();
    let ask = |name: &str| {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
            .args(["run", "map", "users", "--name", name, "--root"])
            .arg(root)
            .current_dir(root)
            .output()
            .expect("run map users");
        let report: serde_json::Value = serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)));
        (out.status.success(), report)
    };

    let (ok, report) = ask("total");
    assert!(ok, "{report}");
    let declarations = report["declarations"].as_array().unwrap();
    assert_eq!(declarations.len(), 1, "{report}");
    assert_eq!(declarations[0]["file"], "src/preco.rs", "{report}");
    assert_eq!(
        declarations[0]["used_by"],
        serde_json::json!(["src/pedido.rs:5:fechar", "src/pedido.rs:6:fechar"]),
        "os dois usos, com o arquivo, a linha e quem chama: {report}"
    );

    let (ok, report) = ask("nao_existe");
    assert!(!ok, "{report}");
    assert_eq!(report["reason"], "unknown-declaration", "{report}");
    let hint = report["hint"].as_str().unwrap();
    assert!(hint.contains("nao_existe"), "a recusa diz o nome: {report}");
    assert!(hint.contains("Confira o nome"), "o texto de declaração desconhecida: {report}");
    // Sem arquivo pedido, quem não tem a declaração é o mapa: o banco dele
    // não é um arquivo do projeto e não aparece como se declarasse nomes.
    assert!(hint.contains("O mapa não tem declaração chamada `nao_existe`"), "{report}");
    assert!(!hint.contains(".claude/") && !hint.contains("O arquivo"), "{report}");
}

/// A resposta de quem usa separa o que o mapa provou do que ele só suspeita,
/// pelo comando que a pessoa roda: `run map users --name run`. As ligações
/// provadas vêm em `used_by`; as suspeitas, em `suspect`, agrupadas pelas
/// declarações que a chamada pode alcançar, e a resposta traz o próximo passo
/// para decidir cada uma pelo servidor de linguagem. A declaração cujo nome
/// ficou comum demais traz só a contagem das chamadas, com o jeito de achá-las.
#[test]
fn the_users_answer_puts_proven_links_first_and_groups_the_suspect_ones() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // O mapa como o scan o grava: `run` em src/a.rs, chamada com certeza por
    // `usa`, em src/com.rs; a chamada de `outra`, em src/sem.rs, pode ser a
    // `run` de src/a.rs ou a de src/b.rs; e a `run` de src/c.rs só conta três
    // chamadas do nome comum.
    let both = r#"["src/a.rs:1:run", "src/b.rs:1:run"]"#;
    mustard_core::io::project_map::write_text(
        root,
        &format!(
            r#"{{"modules": [
             {{"path": "src/a.rs", "loc": 3, "declarations": [
               {{"kind": "function", "name": "run", "line": 1, "end_line": 3,
                "used_by": ["src/com.rs:4:usa", {{"at": "src/sem.rs:2:outra", "candidates": {both}}}]}}]}},
             {{"path": "src/b.rs", "loc": 3, "declarations": [
               {{"kind": "function", "name": "run", "line": 1, "end_line": 3,
                "used_by": [{{"at": "src/sem.rs:2:outra", "candidates": {both}}}]}}]}},
             {{"path": "src/c.rs", "loc": 3, "declarations": [
               {{"kind": "function", "name": "run", "line": 1, "end_line": 3, "common_calls": 3}}]}}
           ]}}"#
        ),
    )
    .unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "map", "users", "--name", "run", "--root"])
        .arg(root)
        .current_dir(root)
        .output()
        .expect("run map users");
    let report: serde_json::Value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)));
    assert!(out.status.success(), "{report}");
    let declarations = report["declarations"].as_array().unwrap();
    let files: Vec<&str> = declarations.iter().map(|d| d["file"].as_str().unwrap()).collect();
    assert_eq!(files, ["src/a.rs", "src/b.rs", "src/c.rs"], "{report}");

    let group = serde_json::json!([{"candidates": ["src/a.rs:1:run", "src/b.rs:1:run"], "used_by": ["src/sem.rs:2:outra"]}]);
    assert_eq!(declarations[0]["used_by"], serde_json::json!(["src/com.rs:4:usa"]), "só a provada: {report}");
    assert_eq!(declarations[0]["suspect"], group, "a suspeita com as duas candidatas: {report}");
    assert_eq!(declarations[1]["used_by"], serde_json::json!([]), "nenhuma provada: {report}");
    assert_eq!(declarations[1]["suspect"], group, "{report}");
    assert!(declarations[1].get("note").is_none(), "quem tem uso suspeito não leva a nota de ninguém usa: {report}");
    let next = report["next"].as_str().unwrap_or_default();
    assert!(next.contains("goToDefinition") && next.contains("LSP"), "o próximo passo pelo servidor de linguagem: {report}");

    assert_eq!(declarations[2]["common_calls"], 3, "{report}");
    assert!(declarations[2].get("suspect").is_none() && declarations[2].get("note").is_none(), "{report}");
    let common = declarations[2]["common"].as_str().unwrap_or_default();
    assert!(common.contains('3') && common.contains("findReferences"), "a contagem e o jeito de achar: {report}");
}

/// A contagem das chamadas do nome comum concorda com o número, pelo comando
/// que a pessoa roda: uma chamada só sai no singular, e três saem no plural.
#[test]
fn one_common_call_reads_in_the_singular_and_three_in_the_plural() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // O mapa como o scan o grava: a `run` de src/a.rs conta uma chamada do
    // nome comum, e a de src/b.rs conta três.
    mustard_core::io::project_map::write_text(
        root,
        r#"{"modules": [
             {"path": "src/a.rs", "loc": 3, "declarations": [
               {"kind": "function", "name": "run", "line": 1, "end_line": 3, "common_calls": 1}]},
             {"path": "src/b.rs", "loc": 3, "declarations": [
               {"kind": "function", "name": "run", "line": 1, "end_line": 3, "common_calls": 3}]}
           ]}"#,
    )
    .unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "map", "users", "--name", "run", "--root"])
        .arg(root)
        .current_dir(root)
        .output()
        .expect("run map users");
    let report: serde_json::Value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)));
    assert!(out.status.success(), "{report}");
    let declarations = report["declarations"].as_array().unwrap();
    let common: Vec<&str> = declarations.iter().map(|d| d["common"].as_str().unwrap_or_default()).collect();
    assert_eq!(declarations.len(), 2, "{report}");
    assert!(common[0].starts_with("Uma chamada de `run` ficou sem ligação,"), "uma chamada, no singular: {report}");
    assert!(common[0].contains("Para achá-la,") && !common[0].contains("chamadas"), "{report}");
    assert!(common[1].starts_with("3 chamadas de `run` ficaram sem ligação,"), "três chamadas, no plural: {report}");
    assert!(common[1].contains("Para achá-las,"), "{report}");
}

/// Pergunta ao mapa pelo comando que a pessoa roda, na raiz `root`: o JSON da
/// resposta e se o comando saiu sem erro.
fn ask_map(root: &Path, question: &str) -> (bool, serde_json::Value) {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "map", question, "--root"])
        .arg(root)
        .current_dir(root)
        .output()
        .expect("run map");
    let report: serde_json::Value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)));
    (out.status.success(), report)
}

/// `run map summary --file` devolve as partes do arquivo, na ordem das
/// linhas, com o tipo, o nome, a linha de começo e a de fim, sem os campos e
/// sem o que mora nos testes, e a linha em que os testes começam. O arquivo
/// que o mapa não guarda é recusado; sem `--file`, volta o resumo do projeto.
#[test]
fn file_summary_brings_the_parts_and_where_the_tests_start() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    mustard_core::io::project_map::write_text(
        root,
        r#"{"modules": [
             {"path": "src/a.rs", "loc": 60, "test_lines": [[40, 60]], "declarations": [
               {"kind": "function", "name": "run", "line": 12, "end_line": 20},
               {"kind": "struct", "name": "Alpha", "line": 3, "end_line": 10},
               {"kind": "field", "name": "size", "line": 4, "end_line": 4},
               {"kind": "function", "name": "a_test", "line": 45, "end_line": 50}]}
           ]}"#,
    )
    .unwrap();
    let summary = |file: Option<&str>| {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_mustard-rt"));
        command.args(["run", "map", "summary", "--root"]).arg(root).current_dir(root);
        if let Some(file) = file {
            command.args(["--file", file]);
        }
        let out = command.output().expect("run map summary");
        let report: serde_json::Value = serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)));
        (out.status.success(), report)
    };

    let (ok, report) = summary(Some("src/a.rs"));
    assert!(ok, "{report}");
    assert_eq!(report["file"], "src/a.rs", "{report}");
    assert_eq!(
        report["parts"],
        serde_json::json!([
            {"kind": "struct", "name": "Alpha", "line": 3, "end_line": 10},
            {"kind": "function", "name": "run", "line": 12, "end_line": 20}
        ]),
        "{report}"
    );
    assert_eq!(report["tests_line"], 40, "{report}");

    let (ok, report) = summary(Some("src/zz.rs"));
    assert!(!ok, "{report}");
    assert_eq!(report["reason"], "unknown-file", "{report}");

    let (ok, report) = summary(None);
    assert!(ok, "{report}");
    assert!(report["summary"].as_str().is_some_and(|text| !text.is_empty()), "{report}");
    assert!(report.get("parts").is_none(), "{report}");
}

/// Para depurar o mapa, `run map dump` mostra o banco tabela por tabela, numa
/// ordem fixa: uma entrada por tabela, com as linhas dela.
#[test]
fn map_dump_brings_one_entry_per_table() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    mustard_core::io::project_map::write_text(
        root,
        r#"{"modules": [
             {"path": "src/preco.rs", "loc": 5, "declarations": [
               {"kind": "function", "name": "total", "line": 1, "end_line": 3,
                "used_by": ["src/pedido.rs:5:fechar"]}]},
             {"path": "src/pedido.rs", "loc": 8, "deps": ["src/preco.rs"], "declarations": []}
           ],
           "graph": {"nodes": 2, "edges": 1, "top_fan_in": [{"module": "src/preco.rs", "degree": 1}]},
           "state": {"head": "abc"}}"#,
    )
    .unwrap();

    let (ok, report) = ask_map(root, "dump");
    assert!(ok, "{report}");
    assert_eq!(report["question"], "dump", "{report}");
    let tables = report["tables"].as_array().unwrap();
    let names: Vec<&str> = tables.iter().map(|table| table["table"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "census", "projects", "languages", "manifests", "skeleton", "files", "decls", "texts", "routes", "links",
            "graph", "fan_in", "history_base", "history_paths", "commits", "resource_files", "lineage_files", "lineage_commits",
            "lineage_decls", "pr_texts", "pr_comments", "pr_commits", "spec_items", "spec_commits", "spec_pulls",
            "spec_marks", "glossary_asks", "glossary_marks", "notes", "knowledge_notes", "blocks"
        ],
        "uma entrada por tabela, na ordem fixa: {report}"
    );
    let rows = |name: &str| tables.iter().find(|table| table["table"] == name).unwrap()["rows"].clone();
    assert_eq!(rows("files").as_array().unwrap().len(), 2, "{report}");
    assert_eq!(rows("decls")[0]["file"], "src/preco.rs", "{report}");
    assert_eq!(rows("decls")[0]["used_by"], serde_json::json!(["src/pedido.rs:5:fechar"]), "{report}");
    assert_eq!(rows("census")[0]["head"], "abc", "{report}");
    assert_eq!(rows("fan_in")[0]["degree"], 1, "{report}");
}

/// O `gh` falso do unix: anota cada chamada e responde o texto e os
/// comentários do pull request. Um script de shell não roda no Windows: lá o
/// falso é o [`FAKE_GH_CMD`], que o sistema acha pelo `.cmd`.
const FAKE_GH_SH: &str = "#!/bin/sh\n\
     echo \"$*\" >> \"$FAKE_DIR/log\"\n\
     case \"$*\" in\n\
     \"api -i repos/{owner}/{repo}/pulls/\"*) n=${3#*pulls/} ;\n\
       [ -f \"$FAKE_DIR/pull$n.json\" ] || { echo 'gh: Not Found (HTTP 404)' >&2 ; exit 1 ; } ;\n\
       printf 'HTTP/2.0 200 OK\\r\\nEtag: W/\"e\"\\r\\n\\r\\n' ; cat \"$FAKE_DIR/pull$n.json\" ;;\n\
     \"api repos/{owner}/{repo}/pulls/\"*\"/comments?per_page=100\") n=${2#*pulls/} ; n=${n%%/*} ;\n\
       cat \"$FAKE_DIR/comments$n.json\" 2>/dev/null || echo '[]' ;;\n\
     \"api repos/{owner}/{repo}/commits/\"*\"/pulls\") echo '[]' ;;\n\
     *) echo 'gh: Not Found (HTTP 404)' >&2 ; exit 1 ;;\n\
     esac\n";

/// O mesmo `gh` falso para o Windows, em lote: a mesma anotação, as mesmas
/// respostas. A anotação vai com o redirecionamento na frente, para a linha
/// não ganhar espaço no fim nem tomar o último número por descritor.
const FAKE_GH_CMD: &str = "@echo off\r\n\
     setlocal EnableDelayedExpansion\r\n\
     >> \"%FAKE_DIR%\\log\" echo %*\r\n\
     set \"A=%*\"\r\n\
     echo !A!| findstr /b /c:\"api -i repos/{owner}/{repo}/pulls/\" >nul && goto pull\r\n\
     echo !A!| findstr /b /c:\"api repos/{owner}/{repo}/pulls/\" >nul && goto comments\r\n\
     echo !A!| findstr /b /c:\"api repos/{owner}/{repo}/commits/\" >nul && goto commits\r\n\
     goto missing\r\n\
     :pull\r\n\
     set \"N=!A:*pulls/=!\"\r\n\
     if not exist \"%FAKE_DIR%\\pull!N!.json\" goto missing\r\n\
     echo HTTP/2.0 200 OK\r\n\
     echo Etag: W/\"e\"\r\n\
     echo.\r\n\
     type \"%FAKE_DIR%\\pull!N!.json\"\r\n\
     exit /b 0\r\n\
     :comments\r\n\
     set \"N=!A:*pulls/=!\"\r\n\
     for /f \"delims=/\" %%n in (\"!N!\") do set \"N=%%n\"\r\n\
     if not exist \"%FAKE_DIR%\\comments!N!.json\" goto empty\r\n\
     type \"%FAKE_DIR%\\comments!N!.json\"\r\n\
     exit /b 0\r\n\
     :commits\r\n\
     :empty\r\n\
     echo []\r\n\
     exit /b 0\r\n\
     :missing\r\n\
     echo gh: Not Found (HTTP 404) 1>&2\r\n\
     exit /b 1\r\n";

/// Um projeto no git, na branch `main` declarada como base, com o remoto do
/// GitHub e um `gh` falso no caminho, que anota cada chamada e responde o
/// texto e os comentários do pull request 7.
struct PullRequestProject {
    dir: tempfile::TempDir,
    fake: tempfile::TempDir,
}

impl PullRequestProject {
    fn new() -> Self {
        let project = Self { dir: tempfile::tempdir().unwrap(), fake: tempfile::tempdir().unwrap() };
        let root = project.root();
        project.git(&["init", "-q", "-b", "main"]);
        project.git(&["remote", "add", "origin", "https://github.com/dono/loja.git"]);
        fs::write(root.join(".git/info/exclude"), mustard_core::footprint_rules().join("\n") + "\n").unwrap();
        project.config(true);
        let (name, script) = if cfg!(windows) { ("gh.cmd", FAKE_GH_CMD) } else { ("gh", FAKE_GH_SH) };
        executable::write_executable(&project.fake.path().join(name), script);
        fs::write(
            project.fake.path().join("pull7.json"),
            r#"{"number": 7, "title": "Muda o ler", "body": "O ler passa a somar dois.\n\nDetalhes que não aparecem."}"#,
        )
        .unwrap();
        project
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn git(&self, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(self.root())
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn commit(&self, body: &str, title: &str) -> String {
        fs::create_dir_all(self.root().join("src")).unwrap();
        fs::write(self.root().join("src/a.rs"), body).unwrap();
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", title]);
        self.git(&["rev-parse", "HEAD"])
    }

    /// A base `main` e muitas chamadas por passada; `on` é a chave do
    /// texto dos pull requests.
    fn config(&self, on: bool) {
        let config = serde_json::json!({
            "git": { "flow": { "*": "main" }, "pullRequestText": on },
            "map": { "pullRequestCalls": 20 },
        });
        fs::write(self.root().join("mustard.json"), config.to_string()).unwrap();
    }

    /// Roda `mustard-rt run <args>` com `path` no lugar do caminho dos
    /// programas: o JSON da resposta e se saiu sem erro.
    fn run(&self, args: &[&str], path: &str) -> (bool, serde_json::Value) {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
            .arg("run")
            .args(args)
            .arg("--root")
            .arg(self.root())
            .current_dir(self.root())
            .env("PATH", path)
            .env("FAKE_DIR", self.fake.path())
            .output()
            .unwrap();
        let report = serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|e| panic!("{e}: {} {}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)));
        (out.status.success(), report)
    }

    /// O caminho dos programas com o `gh` falso na frente.
    fn with_fake_gh(&self) -> String {
        let mut folders = vec![self.fake.path().to_path_buf()];
        folders.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
        std::env::join_paths(folders).unwrap().to_string_lossy().into_owned()
    }

    /// As chamadas que o `gh` falso recebeu.
    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.fake.path().join("log")).unwrap_or_default().lines().map(str::to_string).collect()
    }
}

/// O caminho dos programas só com o `git`, sem o `gh`: no unix, uma pasta com
/// um atalho para o `git`; no Windows, o `git.exe` não se copia sozinho
/// (ele precisa das pastas ao lado), e vale a pasta em que o `PATH` o acha,
/// que não traz o `gh`.
fn path_with_git_only(folder: &Path) -> String {
    #[cfg(unix)]
    {
        let git = std::process::Command::new("sh").args(["-c", "command -v git"]).output().unwrap();
        std::os::unix::fs::symlink(String::from_utf8_lossy(&git.stdout).trim(), folder.join("git")).unwrap();
        folder.display().to_string()
    }
    #[cfg(not(unix))]
    {
        let _ = folder;
        let path = std::env::var_os("PATH").unwrap_or_default();
        let found = std::env::split_paths(&path).find(|dir| dir.join("git.exe").is_file() && !dir.join("gh.exe").exists());
        found.expect("uma pasta do PATH com o git e sem o gh").display().to_string()
    }
}

/// Pelo comando que a pessoa roda: depois do scan, o texto do pull request
/// mesclado vem do provedor uma vez, com o comentário de revisão preso à
/// linha da função, e o `map history` mostra o título, o comentário e, com
/// `--pr`, o primeiro parágrafo da descrição; o comentário geral fica fora.
/// O scan seguinte não lê de novo; com a chave desligada, nada chama o
/// provedor; sem o `gh` a história sai com o título e o número, sem erro; e
/// o commit novo da base, visto pela pergunta ao mapa, traz o texto do pull
/// request dele na mesma resposta.
#[test]
fn history_carries_the_pull_request_text_read_once_after_the_scan() {
    assert!(
        mustard_core::Scan::locate().is_compiled_alongside(),
        "o teste precisa do scan compilado junto com ele: rode `cargo build -p scan` antes de `cargo test -p mustard-rt`"
    );
    let project = PullRequestProject::new();
    let created = project.commit("pub fn ler(x: u32) -> u32 {\n    x + 1\n}\n", "cria o ler");
    let changed = project.commit("pub fn ler(x: u32) -> u32 {\n    x + 2\n}\n", "muda o ler (#7)");
    let comments = serde_json::json!([
        { "path": "src/a.rs", "line": 2, "commit_id": changed, "side": "RIGHT", "subject_type": "line", "body": "soma dois mesmo?" },
        { "path": "src/a.rs", "line": null, "original_line": null, "commit_id": changed, "subject_type": "file", "body": "o arquivo todo" },
    ]);
    fs::write(project.fake.path().join("comments7.json"), comments.to_string()).unwrap();
    let path = project.with_fake_gh();

    let (ok, scanned) = project.run(&["scan"], &path);
    assert!(ok, "{scanned}");
    // Os dois commits têm o mesmo segundo: a ordem entre eles não conta.
    let mut calls = project.calls();
    calls.sort();
    assert_eq!(
        calls,
        [
            "api -i repos/{owner}/{repo}/pulls/7".to_string(),
            format!("api repos/{{owner}}/{{repo}}/commits/{created}/pulls"),
            "api repos/{owner}/{repo}/pulls/7/comments?per_page=100".to_string(),
        ],
    );
    let (ok, report) = project.run(&["map", "history", "--name", "ler", "--file", "src/a.rs", "--pr", "7"], &path);
    assert!(ok, "{report}");
    let read_result = &report["declarations"][0];
    assert_eq!(read_result["pulls"], serde_json::json!(["#7 Muda o ler"]), "{report}");
    assert_eq!(read_result["comments"], serde_json::json!(["#7 soma dois mesmo?"]), "{report}");
    assert_eq!(report["pull"]["description"], "O ler passa a somar dois.", "{report}");

    let (ok, again) = project.run(&["scan"], &path);
    assert!(ok, "{again}");
    assert_eq!(project.calls().len(), 3, "um pull request já lido não é lido de novo sem mudança");

    project.config(false);
    project.commit("pub fn ler(x: u32) -> u32 {\n    x + 3\n}\n", "muda o ler de novo (#8)");
    let (ok, off) = project.run(&["scan"], &path);
    assert!(ok, "{off}");
    assert_eq!(project.calls().len(), 3, "a chave desligada não chama o provedor");

    project.config(true);
    let only_git = tempfile::tempdir().unwrap();
    let without_gh = path_with_git_only(only_git.path());
    let (ok, scanned) = project.run(&["scan"], &without_gh);
    assert!(ok, "{scanned}");
    let (ok, report) = project.run(&["map", "history", "--name", "ler", "--file", "src/a.rs"], &without_gh);
    assert!(ok, "sem o gh, a história sai sem erro: {report}");
    let lines: Vec<&str> = report["declarations"][0]["commits"].as_array().unwrap().iter().filter_map(|c| c.as_str()).collect();
    assert!(lines[0].contains("muda o ler de novo") && lines[0].ends_with("#8"), "{report}");
    assert_eq!(project.calls().len(), 3, "{report}");

    fs::write(project.fake.path().join("pull9.json"), r#"{"number": 9, "title": "Ler dobrado", "body": "O ler dobra."}"#).unwrap();
    project.commit("pub fn ler(x: u32) -> u32 {\n    x * 2\n}\n", "dobra o ler (#9)");
    let (ok, report) = project.run(&["map", "history", "--pr", "9"], &path);
    assert!(ok, "{report}");
    assert_eq!(report["pull"]["description"], "O ler dobra.", "a atualização do mapa antes da resposta leu o texto: {report}");
    assert!(project.calls().contains(&"api -i repos/{owner}/{repo}/pulls/9".to_string()));
}

/// Perguntar a um projeto fora do git, que ainda não tem mapa, recusa com mapa
/// ausente e não deixa um banco vazio no lugar: a pergunta seguinte recusa
/// igual. Dentro do git a pergunta cria o mapa antes de responder (ver
/// `map_created_when_missing.rs`); fora dele não há de onde ler o mapa.
#[test]
fn asking_the_project_without_a_map_refuses_and_does_not_create_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for question in ["summary", "dump", "summary"] {
        let (ok, report) = ask_map(root, question);
        assert!(!ok, "{question}: {report}");
        assert_eq!(report["reason"], "map-missing", "{question}: {report}");
    }
    assert!(!mustard_core::io::project_map::model_path(root).exists(), "a pergunta criou o mapa");
    assert!(!root.join(".claude").exists(), "a pergunta criou a pasta do mapa");
}

/// A ajuda do `run pending`, como o usuário a pede, escreve o número de uma
/// pendência como `P-N`, inteiro na linha da opção: nenhum `P-` fica partido
/// por uma quebra no lugar do número.
#[test]
fn the_pending_help_shows_the_item_id_as_p_n() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "pending", "--help"])
        .output()
        .expect("mustard-rt run pending --help");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let help = String::from_utf8_lossy(&out.stdout);

    let close = help.lines().find(|line| line.trim_start().starts_with("--close")).expect("the --close line");
    assert!(close.contains("`P-N` as delivered"), "the id stays on the option's line: {close}");
    let spelled: Vec<String> = help.match_indices("P-").map(|(at, _)| help[at..].chars().take(3).collect()).collect();
    assert!(spelled.len() >= 6, "the summary and the five options name the id: {help}");
    assert!(spelled.iter().all(|id| *id == "P-N"), "every id is spelled P-N: {spelled:?}\n{help}");
}

/// A ajuda do `run map`, como o usuário a pede, descreve o `summary` como o
/// resumo do mapa do projeto, até 3 kB: o início da sessão não o coloca mais,
/// e a ajuda não pode mandar o leitor procurá-lo lá.
#[test]
fn the_map_help_describes_the_summary_of_the_map_and_not_the_session_start() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "map", "--help"])
        .output()
        .expect("mustard-rt run map --help");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let help = String::from_utf8_lossy(&out.stdout).split_whitespace().collect::<Vec<_>>().join(" ");

    assert!(help.contains("`summary` (the summary of the project map, up to 3 kB;"), "the summary is described: {help}");
    assert!(
        !help.contains("session-start") && !help.contains("session start"),
        "the help does not send the reader to the session start: {help}"
    );
}

/// O código das parcelas, com a palavra `parcela` só num comentário.
const INSTALLMENTS: &str = "export function splitInstallments(total: number, count: number) {\n  // divide o total em parcela iguais\n  return Array.from({ length: count }, () => total / count);\n}\n\nexport function payInstallment(value: number) {\n  return value;\n}\n";

/// Um projeto no git com o código das parcelas e o mapa dele em dia: o
/// comando do mapa e o gancho da busca leem o mesmo projeto, sem passada do
/// scan e sem filtro.
fn installments_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    git(&["init", "-q", "-b", "dev"]);
    fs::write(root.join(".git/info/exclude"), mustard_core::footprint_rules().join("\n") + "\n").unwrap();
    fs::write(root.join("mustard.json"), r#"{"search":{"filter":"none"}}"#).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/parcelas.ts"), INSTALLMENTS).unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "semente"]);
    let now = mustard_core::io::project_map::listing(root).expect("dentro do git");
    let map = serde_json::json!({
        "state": {"head": now.head, "listing": now.digest(), "base": now.base.name, "base_tip": now.base.tip},
        "modules": [{
            "path": "src/parcelas.ts", "language": "typescript", "loc": 8,
            "blob": git(&["hash-object", "--", "src/parcelas.ts"]),
            "declarations": [
                {"kind": "function", "name": "splitInstallments", "line": 1, "end_line": 4,
                 "signature": "export function splitInstallments(total: number, count: number)",
                 "body_comment": "divide o total em parcela iguais"},
                {"kind": "function", "name": "payInstallment", "line": 6, "end_line": 8}
            ]
        }]
    });
    mustard_core::io::project_map::write_text(root, &map.to_string()).unwrap();
    dir
}

/// A resposta do gancho da busca ao `Grep` com o padrão `pattern`, como o
/// Claude Code a pede ao programa: o motivo da recusa, quando ele responde.
fn hook_answer_to_grep(root: &Path, pattern: &str) -> Option<String> {
    use std::io::Write as _;
    let input = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Grep",
        "cwd": root.to_str().unwrap(),
        "session_id": "grep-parity-test",
        "tool_input": { "pattern": pattern, "output_mode": "content" }
    });
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["on", "PreToolUse"])
        .current_dir(root)
        .env_remove("CLAUDE_PROJECT_DIR")
        .env_remove("MUSTARD_WORKSPACE_ROOT")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn mustard-rt");
    child.stdin.take().unwrap().write_all(input.to_string().as_bytes()).unwrap();
    let out = child.wait_with_output().expect("wait mustard-rt");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let denied = parsed.pointer("/hookSpecificOutput/permissionDecision")? == "deny";
    let reason = parsed.pointer("/hookSpecificOutput/permissionDecisionReason")?.as_str()?;
    denied.then(|| reason.to_string())
}

/// O gancho encaminha o padrão sem o reinterpretar; a porta e o `map search`
/// executam a pesquisa real antes do cruzamento. O vazio é recusado.
#[test]
fn the_grep_handoff_and_explicit_map_search_preserve_the_native_pattern() {
    let project = installments_project();
    let root = project.path();
    let pattern = "splitInstallments|parcela";
    let handoff = hook_answer_to_grep(root, pattern).expect("supported Grep routes through the gateway");
    assert!(handoff.contains("mcp__mustard__search"), "{handoff}");
    let request = serde_json::json!({"tool":"Grep","input":{"pattern":pattern,"output_mode":"content"},"intent":"","purpose":"locate","choose":false});
    assert!(handoff.contains(&request.to_string()), "complete transported request: {handoff}");
    let gateway = std::process::Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "search", "--request", &request.to_string(), "--root"])
        .arg(root).current_dir(root).output().unwrap();
    assert!(gateway.status.success(), "{}",String::from_utf8_lossy(&gateway.stderr));
    let response: serde_json::Value=serde_json::from_slice(&gateway.stdout).unwrap();
    let native = std::process::Command::new("rg")
        .args(["--with-filename","--no-heading","--color=never","-n","--",pattern,"."])
        .current_dir(root).output().unwrap();
    assert_eq!(response["result"]["content"].as_str().unwrap(),String::from_utf8(native.stdout).unwrap().trim_end_matches('\n'));
    assert_eq!(response["remote_model_calls"],0);

    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
            .args(["run", "map"])
            .args(args)
            .arg("--root")
            .arg(root)
            .current_dir(root)
            .env_remove("CLAUDE_PROJECT_DIR")
            .output()
            .expect("run map search")
    };
    let out = run(&["search", pattern]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let answer = String::from_utf8_lossy(&out.stdout);
    assert!(answer.contains("src/parcelas.ts") && answer.contains("splitInstallments"), "explicit map recovery: {answer}");

    for options in [&["src", "--glob", "*.ts", "-i"][..], &["src", "--type", "ts"][..]] {
        let out = run(&[&["search", pattern][..], options].concat());
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("splitInstallments"), "the folder and {options:?} reach the search: {stdout}");
    }
    let outside = run(&["search", pattern, "--glob", "*.rs"]);
    let outside:serde_json::Value=serde_json::from_slice(&outside.stdout).unwrap();
    assert_eq!(outside["exit_code"],1,"native no-match status");
    assert_eq!(outside["result"]["stdout"],"");
    assert!(outside["evidence"].is_null());

    let blank = run(&["search", "  "]);
    assert!(!blank.status.success());
    let report: serde_json::Value = serde_json::from_slice(&blank.stdout).unwrap();
    assert_eq!(report["reason"], "missing-argument", "{report}");
}

/// A ajuda do `run map` ensina a busca com o texto do `Grep` e as opções que
/// o gancho entende, e não fala das opções de medida `--query`, `--intent`,
/// `--described` e `--said`, que seguem aceitas, escondidas.
#[test]
fn the_map_help_teaches_the_search_with_the_text_of_grep_and_hides_the_measuring_options() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "map", "--help"])
        .output()
        .expect("mustard-rt run map --help");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let help = String::from_utf8_lossy(&out.stdout).into_owned();
    for shown in ["[PATTERN]", "--glob", "--type", "--ignore-case", "--word-regexp", "--fixed-strings"] {
        assert!(help.contains(shown), "the help shows {shown}: {help}");
    }
    for hidden in ["--query", "--intent", "--described", "--said"] {
        assert!(!help.contains(hidden), "the measuring option {hidden} stays hidden: {help}");
    }

    let project = installments_project();
    let root = project.path();
    let old = std::process::Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "map", "search", "--query", "splitInstallments", "--root"])
        .arg(root)
        .current_dir(root)
        .output()
        .expect("run map search --query");
    let report: serde_json::Value = serde_json::from_slice(&old.stdout).unwrap();
    assert_eq!(report["ok"], true, "the hidden option is still accepted: {report}");

    let described = std::process::Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "map", "search", "--query", "splitInstallments", "--described", "Procura o parcelamento"])
        .args(["--said", "Vou olhar o parcelamento.", "--root"])
        .arg(root)
        .current_dir(root)
        .output()
        .expect("run map search --described --said");
    assert!(described.status.success(), "{}", String::from_utf8_lossy(&described.stderr));
    let report: serde_json::Value = serde_json::from_slice(&described.stdout).unwrap();
    assert_eq!(report["ok"], true, "the hidden options of the description and the speech are accepted: {report}");
}
