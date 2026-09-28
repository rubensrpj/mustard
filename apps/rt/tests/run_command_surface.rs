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
//! O arquivo também lê as superfícies de instrução ENTREGUES (`plugin/**`): a
//! árvore do clap sozinha não pega um texto que promete um comando que o leitor
//! não vai achar, e uma instrução errada falha tão em silêncio quanto um
//! registro perdido.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "support/manifest_dir.rs"]
mod manifest_dir;

use std::fs;
use std::path::{Path, PathBuf};

use clap::{Command, Subcommand};
use mustard_rt::commands::RunCmd;

/// O retrato da superfície, relativo à raiz do repositório.
const SURFACE_SNAPSHOT: &str = "apps/rt/tests/fixtures/run-surface.txt";

/// Instruction surfaces SHIPPED to the reader, relative to the repo root, with
/// the file extension each one is scanned through (`None` = every file).
///
/// `plugin/**/*.md` is what the agent loads at runtime (commands, refs, agent
/// prompts). Each entry is ASSERTED to exist and to yield files — a surface that
/// silently disappears would turn this guard into a green no-op.
const DOC_SURFACES: &[(&str, Option<&str>)] = &[("plugin", Some("md"))];

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

/// Recursively collect files under `dir` in a deterministic (sorted) order,
/// keeping only `ext` when it is set. An unreadable directory yields nothing.
fn collect_files(dir: &Path, ext: Option<&str>, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            if name == "node_modules" || name == "target" || name == ".git" {
                continue;
            }
            collect_files(&path, ext, out);
        } else if ext
            .is_none_or(|want| path.extension().and_then(|e| e.to_str()).is_some_and(|e| e == want))
        {
            out.push(path);
        }
    }
}

/// Every `mustard-rt run <name>` token in `text`, in order of appearance.
///
/// All three invocation spellings are recognised — the same set
/// `template_parity`'s `CALLER_PREFIXES` feeds to its `extract_run_names`.
/// Recognising only the bare one
/// would let a Windows-flavoured hint (`mustard-rt.exe run …`) or a packaging
/// script (`$RtExe run …`) name a dead command and still pass.
///
/// A token runs to the first byte outside `[a-z0-9-]`, which drops the trailing
/// backtick / period / paren that normally closes the instruction in prose.
/// Placeholders are SKIPPED rather than reported: an explicit `<`, `{`, `$` or
/// backtick start teaches a shape, not a command, and any other non-token byte
/// (the `…` elision in `scan.md`) yields an empty token.
fn documented_run_tokens(text: &str) -> Vec<String> {
    const PREFIXES: &[&str] = &["mustard-rt run ", "mustard-rt.exe run ", "$RtExe run "];
    const PLACEHOLDER_STARTS: &[u8] = b"<{$`";

    let bytes = text.as_bytes();
    let mut out = Vec::new();
    // One cursor per spelling: each advances independently through the text, and
    // the run with the smallest next hit is consumed, so the tokens stay in order
    // of appearance no matter which spelling produced them.
    let mut cursors = vec![0usize; PREFIXES.len()];
    while let Some((which, pos)) = PREFIXES
        .iter()
        .enumerate()
        .filter_map(|(i, p)| text[cursors[i]..].find(p).map(|off| (i, cursors[i] + off)))
        .min_by_key(|(_, pos)| *pos)
    {
        let start = pos + PREFIXES[which].len();
        cursors[which] = start;
        // Every other cursor must clear this hit too, else the same region is
        // re-scanned forever by the spellings that did not match here.
        for (i, c) in cursors.iter_mut().enumerate() {
            if i != which && *c <= pos {
                *c = start;
            }
        }
        if start >= bytes.len() || PLACEHOLDER_STARTS.contains(&bytes[start]) {
            continue;
        }
        let mut end = start;
        while end < bytes.len()
            && (bytes[end].is_ascii_lowercase() || bytes[end].is_ascii_digit() || bytes[end] == b'-')
        {
            end += 1;
        }
        if end > start {
            out.push(text[start..end].to_string());
        }
    }
    out
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
fn a_superficie_publicada_e_igual_ao_retrato() {
    let cmd = run_command_tree();
    let mut atual: Vec<String> =
        cmd.get_subcommands().map(|c| c.get_name().to_string()).collect();
    atual.sort();

    assert_eq!(
        atual,
        snapshot_names(),
        "a superfície de `run` mudou. Se a mudança é a pretendida, regrave \
         {SURFACE_SNAPSHOT} com estes nomes, um por linha:\n{}",
        atual.join("\n")
    );
}

/// Dois comandos no mesmo lugar da lista fariam o `run --help` embaralhar
/// sozinho: o clap ordena por `(display_order, name)`.
#[test]
fn nenhum_comando_divide_o_lugar_de_outro_na_ajuda() {
    let cmd = run_command_tree();
    let mut slots: Vec<usize> = cmd
        .get_subcommands()
        .filter(|c| c.get_name() != "help")
        .map(clap::Command::get_display_order)
        .collect();
    slots.sort_unstable();
    let mut unicos = slots.clone();
    unicos.dedup();
    assert_eq!(slots, unicos, "dois comandos declaram o mesmo `display_order`");
}

/// Every `mustard-rt run <name>` a SHIPPED instruction surface tells the reader
/// (or an agent) to type must be a name the CLI actually publishes.
///
/// Field defect: `wave-scaffold` was absorbed into
/// `plan-materialize`, but a shipped hint still told the reader to run it.
/// Nothing broke at build time — the command simply does not exist, so an
/// obedient agent burns a call on a clap error. `template_parity` runs the same
/// forward check over the template/plugin/packaging corpus; this one walks the
/// plugin tree as the reader's own instruction surface.
#[test]
fn every_documented_run_command_exists() {
    let root = repo_root();
    let publicados = snapshot_names();
    let mut offenders = Vec::new();

    for (rel, ext) in DOC_SURFACES {
        let dir = root.join(rel);
        // Assert the surface instead of skipping it (the idiom `template_parity`
        // already uses): a moved or renamed directory would otherwise make this
        // guard pass while scanning nothing — a dead guard reads exactly like a
        // clean one, which is the failure mode the whole test exists to prevent.
        assert!(dir.is_dir(), "declared instruction surface `{rel}` is missing — update DOC_SURFACES");
        let mut files = Vec::new();
        collect_files(&dir, *ext, &mut files);
        assert!(
            !files.is_empty(),
            "instruction surface `{rel}` yielded 0 files — the guard would pass vacuously"
        );
        for file in files {
            let Ok(text) = fs::read_to_string(&file) else {
                continue;
            };
            for name in documented_run_tokens(&text) {
                if !publicados.contains(&name) {
                    let shown = file.strip_prefix(&root).unwrap_or(&file);
                    offenders.push(format!("{} -> `mustard-rt run {name}`", shown.display()));
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "shipped instructions name `mustard-rt run` commands the CLI does not \
         publish — the call dies on a clap error at runtime, and the reader has \
         no way to tell. Fix the surface or register the command:\n{}",
        offenders.join("\n")
    );
}

/// The guard above is only as good as its tokenizer, and a tokenizer that
/// silently stops matching turns the whole test green-and-blind. Pin the three
/// spellings it must catch and the placeholder shapes it must ignore.
#[test]
fn documented_run_tokens_catches_every_spelling_and_skips_placeholders() {
    let found = documented_run_tokens(
        "run `mustard-rt run resume` first.\n\
         On Windows: `mustard-rt.exe run doctor`.\n\
         Packaging uses `$RtExe run upsert`.\n\
         Shapes teach nothing: `mustard-rt run <name>`, `mustard-rt run {kind}`, \
         `mustard-rt run $Cmd`.\n",
    );
    assert_eq!(
        found,
        vec!["resume", "doctor", "upsert"],
        "all three invocation spellings must be caught, in order, and every \
         placeholder skipped",
    );
    // Every name it caught here is real — the guard flags exactly the ones that
    // are not.
    let publicados = snapshot_names();
    for name in &found {
        assert!(publicados.contains(name), "{name} should be a real command");
    }
    assert_eq!(
        documented_run_tokens("`mustard-rt run wave-scaffold` (the shipped defect)"),
        vec!["wave-scaffold"],
        "the absorbed command must still be recognised as a name — that is what \
         makes the guard fail when a surface names it",
    );
    assert!(!publicados.contains(&"wave-scaffold".to_string()));
}

/// Quem vai mexer numa função pergunta ao mapa quem a usa, pelo comando que a
/// pessoa roda: `run map users --name <declaração>`. A resposta cita onde a
/// declaração mora e cada uso, como `arquivo:linha:quem chama`; um nome que o
/// mapa não declara é recusado com o texto de declaração desconhecida, que
/// diz que o mapa não a tem, sem citar arquivo.
#[test]
fn o_mapa_devolve_quem_usa_uma_declaracao_pelo_nome() {
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
    let duas = r#"["src/a.rs:1:run", "src/b.rs:1:run"]"#;
    mustard_core::io::project_map::write_text(
        root,
        &format!(
            r#"{{"modules": [
             {{"path": "src/a.rs", "loc": 3, "declarations": [
               {{"kind": "function", "name": "run", "line": 1, "end_line": 3,
                "used_by": ["src/com.rs:4:usa", {{"at": "src/sem.rs:2:outra", "candidates": {duas}}}]}}]}},
             {{"path": "src/b.rs", "loc": 3, "declarations": [
               {{"kind": "function", "name": "run", "line": 1, "end_line": 3,
                "used_by": [{{"at": "src/sem.rs:2:outra", "candidates": {duas}}}]}}]}},
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

    let grupo = serde_json::json!([{"candidates": ["src/a.rs:1:run", "src/b.rs:1:run"], "used_by": ["src/sem.rs:2:outra"]}]);
    assert_eq!(declarations[0]["used_by"], serde_json::json!(["src/com.rs:4:usa"]), "só a provada: {report}");
    assert_eq!(declarations[0]["suspect"], grupo, "a suspeita com as duas candidatas: {report}");
    assert_eq!(declarations[1]["used_by"], serde_json::json!([]), "nenhuma provada: {report}");
    assert_eq!(declarations[1]["suspect"], grupo, "{report}");
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
fn o_resumo_de_um_arquivo_traz_as_partes_e_onde_os_testes_comecam() {
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
fn o_despejo_do_mapa_traz_uma_entrada_por_tabela() {
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
            "graph", "fan_in", "history_base", "history_paths", "commits", "lineage_files", "lineage_commits",
            "lineage_decls", "pr_texts", "pr_comments", "pr_commits", "spec_items", "spec_commits", "spec_pulls",
            "spec_marks", "glossary_asks", "glossary_marks", "blocks"
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
        let gh = project.fake.path().join("gh");
        fs::write(
            &gh,
            "#!/bin/sh\n\
             echo \"$*\" >> \"$FAKE_DIR/log\"\n\
             case \"$*\" in\n\
             \"api -i repos/{owner}/{repo}/pulls/\"*) n=${3#*pulls/} ;\n\
               [ -f \"$FAKE_DIR/pull$n.json\" ] || { echo 'gh: Not Found (HTTP 404)' >&2 ; exit 1 ; } ;\n\
               printf 'HTTP/2.0 200 OK\\r\\nEtag: W/\"e\"\\r\\n\\r\\n' ; cat \"$FAKE_DIR/pull$n.json\" ;;\n\
             \"api repos/{owner}/{repo}/pulls/\"*\"/comments?per_page=100\") n=${2#*pulls/} ; n=${n%%/*} ;\n\
               cat \"$FAKE_DIR/comments$n.json\" 2>/dev/null || echo '[]' ;;\n\
             \"api repos/{owner}/{repo}/commits/\"*\"/pulls\") echo '[]' ;;\n\
             *) echo 'gh: Not Found (HTTP 404)' >&2 ; exit 1 ;;\n\
             esac\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
        }
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
        format!("{}:{}", self.fake.path().display(), std::env::var("PATH").unwrap_or_default())
    }

    /// As chamadas que o `gh` falso recebeu.
    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.fake.path().join("log")).unwrap_or_default().lines().map(str::to_string).collect()
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
fn a_historia_traz_o_texto_do_pull_request_lido_uma_vez_depois_do_scan() {
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
    let ler = &report["declarations"][0];
    assert_eq!(ler["pulls"], serde_json::json!(["#7 Muda o ler"]), "{report}");
    assert_eq!(ler["comments"], serde_json::json!(["#7 soma dois mesmo?"]), "{report}");
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
    let git = std::process::Command::new("sh").args(["-c", "command -v git"]).output().unwrap();
    let only_git = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(String::from_utf8_lossy(&git.stdout).trim(), only_git.path().join("git")).unwrap();
    let without_gh = only_git.path().display().to_string();
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

/// Perguntar a um projeto que ainda não tem mapa recusa com mapa ausente e não
/// deixa um banco vazio no lugar: a pergunta seguinte recusa igual, e o scan é
/// quem cria o mapa.
#[test]
fn perguntar_ao_projeto_sem_mapa_recusa_e_nao_cria_o_arquivo() {
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
