//! Two passes of the scan over a git project, with one file changed between
//! them: the second pass reads only that file, the map it writes is the same
//! one a pass reading every file gives, and neither pass writes to git or to
//! any `CLAUDE.md`. The project carries the exclude rules a Mustard install
//! writes, so the map stays out of git.

#[path = "support/model.rs"]
mod model;

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.email=scan@example.com", "-c", "user.name=scan", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A pasta do projeto onde o scan grava o mapa.
fn map_folder(dir: &Path) -> PathBuf {
    dir.join(".claude")
}

/// Roda o scan sobre o projeto e devolve o relato da passada.
fn scan(dir: &Path, extra: &[&str]) -> Value {
    model::scan(dir, &map_folder(dir), extra).1
}

fn write(dir: &Path, rel: &str, body: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

#[test]
fn a_second_pass_reads_only_the_changed_file_and_leaves_git_clean() {
    let temp = tempfile::Builder::new().prefix("scan-incremental-").tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    git(&dir, &["init", "-q"]);
    let exclude = mustard_core::footprint_rules().join("\n") + "\n";
    std::fs::write(dir.join(".git").join("info").join("exclude"), exclude).unwrap();

    write(&dir, "Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n");
    write(&dir, "src/lib.rs", "pub mod a;\npub mod b;\n");
    write(&dir, "src/a.rs", "pub fn alpha() -> u32 {\n    1\n}\n");
    write(&dir, "src/b.rs", "use crate::a::alpha;\npub fn beta() -> u32 {\n    alpha() + 1\n}\n");
    write(&dir, "CLAUDE.md", "# Demo\n");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "first"]);

    let first = scan(&dir, &[]);
    assert_eq!(first["full"], json!(true), "{first}");
    for file in ["src/a.rs", "src/b.rs", "src/lib.rs"] {
        assert!(first["read"].as_array().unwrap().contains(&json!(file)), "{file} in {first}");
    }

    // One file changes and is committed: the next pass reads only it.
    write(&dir, "src/b.rs", "use crate::a::alpha;\npub fn beta() -> u32 {\n    alpha() + 2\n}\npub fn gamma() {}\n");
    git(&dir, &["commit", "-q", "-am", "second"]);
    let second = scan(&dir, &[]);
    assert_eq!(second["full"], json!(false), "{second}");
    assert_eq!(second["read"], json!(["src/b.rs"]), "{second}");

    // Neither pass wrote to git or to the CLAUDE.md.
    assert_eq!(git(&dir, &["status", "--porcelain"]), "", "the map stays out of git");
    assert_eq!(git(&dir, &["rev-list", "--count", "HEAD"]).trim(), "2", "the scan never commits");
    assert_eq!(std::fs::read_to_string(dir.join("CLAUDE.md")).unwrap(), "# Demo\n");

    // The map read in steps is the map read at once.
    let stepped = model::read_bytes(&map_folder(&dir));
    assert_eq!(scan(&dir, &["--all"])["full"], json!(true));
    assert_eq!(model::read_bytes(&map_folder(&dir)), stepped, "reading only what changed gives the same map");

    // A change not committed is read, and read again once it is undone.
    let original = std::fs::read_to_string(dir.join("src/a.rs")).unwrap();
    write(&dir, "src/a.rs", "pub fn alpha() -> u32 {\n    7\n}\n");
    assert_eq!(scan(&dir, &[])["read"], json!(["src/a.rs"]));
    write(&dir, "src/a.rs", &original);
    assert_eq!(scan(&dir, &[])["read"], json!(["src/a.rs"]), "a file put back is read again");
    let undone = model::read_bytes(&map_folder(&dir));
    scan(&dir, &["--all"]);
    assert_eq!(model::read_bytes(&map_folder(&dir)), undone);

    // The map keeps the history and what each file imports.
    let model: Value = serde_json::from_slice(&undone).unwrap();
    assert_eq!(model["history"]["commits"].as_array().unwrap().len(), 2, "{}", model["history"]);
    // And it keeps the named edges between declarations: this last pass read
    // only `src/a.rs`, so the use of `alpha` inside `src/b.rs` came from the
    // call sites the unread file carries, not from reading it again.
    let alpha = model["modules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["path"] == json!("src/a.rs"))
        .map(|m| m["declarations"][0].clone())
        .unwrap();
    assert_eq!(alpha["used_by"], json!(["src/b.rs:3:beta"]), "{alpha}");
    assert_eq!(git(&dir, &["status", "--porcelain"]), "");

}

/// Mudar só o arquivo de configuração dos apelidos de pasta faz a passada
/// seguinte ler o projeto inteiro: o import que o apelido novo liga está num
/// arquivo que não mudou, e ele passa a apontar para a pasta nova.
#[test]
fn changing_only_the_alias_configuration_reads_everything_again() {
    let temp = tempfile::Builder::new().prefix("scan-incremental-alias-").tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    git(&dir, &["init", "-q"]);
    let exclude = mustard_core::footprint_rules().join("\n") + "\n";
    std::fs::write(dir.join(".git").join("info").join("exclude"), exclude).unwrap();

    write(&dir, "tsconfig.json", "{ \"compilerOptions\": { \"paths\": { \"@app/*\": [\"src/a/*\"] } } }\n");
    write(&dir, "src/a/pedido.ts", "export const total = 1;\n");
    write(&dir, "src/b/pedido.ts", "export const total = 2;\n");
    write(&dir, "src/usa.ts", "import { total } from '@app/pedido';\n\nexport const x = total;\n");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "first"]);
    assert_eq!(scan(&dir, &[])["full"], json!(true));

    write(&dir, "tsconfig.json", "{ \"compilerOptions\": { \"paths\": { \"@app/*\": [\"src/b/*\"] } } }\n");
    git(&dir, &["commit", "-q", "-am", "second"]);
    let second = scan(&dir, &[]);
    assert_eq!(second["full"], json!(true), "{second}");
    assert!(second["read"].as_array().unwrap().contains(&json!("src/usa.ts")), "{second}");

    let model: Value = model::read(&map_folder(&dir));
    let usa = model["modules"].as_array().unwrap().iter().find(|m| m["path"] == json!("src/usa.ts")).unwrap();
    assert_eq!(usa["deps"], json!(["src/b/pedido.ts"]), "{usa}");
}

/// A importação de namespace liga aos arquivos que declaram o nome que o
/// arquivo cita, e a passada que lê só o que mudou liga igual à que lê tudo,
/// mesmo quando o nome é declarado por arquivos demais para a citação ligar a
/// uma declaração: `Status`, citado por `Uso.cs`, está em nove arquivos do
/// namespace, e mudar só `Outro.cs` não tira de `Uso.cs` os nove.
#[test]
fn a_namespace_import_links_the_same_when_only_another_file_changed() {
    let temp = tempfile::Builder::new().prefix("scan-incremental-namespace-").tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    git(&dir, &["init", "-q"]);
    let exclude = mustard_core::footprint_rules().join("\n") + "\n";
    std::fs::write(dir.join(".git").join("info").join("exclude"), exclude).unwrap();

    let declaring: Vec<String> = (1..=9).map(|i| format!("src/Models/Tipo{i}.cs")).collect();
    for path in &declaring {
        write(&dir, path, "namespace Loja.Models;\n\npublic enum Status\n{\n    Ativo,\n}\n");
    }
    write(
        &dir,
        "src/Services/Uso.cs",
        "using Loja.Models;\n\nnamespace Loja.Services;\n\npublic class Uso\n{\n    public Status Atual;\n}\n",
    );
    write(&dir, "src/Outro.cs", "namespace Loja;\n\npublic class Outro\n{\n}\n");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "first"]);
    assert_eq!(scan(&dir, &[])["full"], json!(true));

    let deps_of_uso = || -> Value {
        let model: Value = model::read(&map_folder(&dir));
        model["modules"].as_array().unwrap().iter().find(|m| m["path"] == json!("src/Services/Uso.cs")).unwrap()["deps"]
            .clone()
    };
    assert_eq!(deps_of_uso(), json!(declaring), "the nine files that declare the name it cites");

    write(&dir, "src/Outro.cs", "namespace Loja;\n\npublic class Outro\n{\n    public int Contar() => 0;\n}\n");
    git(&dir, &["commit", "-q", "-am", "second"]);
    let second = scan(&dir, &[]);
    assert_eq!(second["read"], json!(["src/Outro.cs"]), "{second}");
    assert_eq!(deps_of_uso(), json!(declaring), "a pass that did not read Uso.cs keeps its links");

    let stepped = model::read_bytes(&map_folder(&dir));
    assert_eq!(scan(&dir, &["--all"])["full"], json!(true));
    assert_eq!(model::read_bytes(&map_folder(&dir)), stepped, "reading only what changed gives the same map");
}

/// Um projeto git com o mapa fora dele, como a instalação o deixa, e um
/// commit com `files`.
fn project(prefix: &str, files: &[(&str, &str)]) -> tempfile::TempDir {
    let temp = tempfile::Builder::new().prefix(prefix).tempdir().unwrap();
    let dir = temp.path();
    git(dir, &["init", "-q", "-b", "main"]);
    let exclude = mustard_core::footprint_rules().join("\n") + "\n";
    std::fs::write(dir.join(".git").join("info").join("exclude"), exclude).unwrap();
    for (rel, body) in files {
        write(dir, rel, body);
    }
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "first"]);
    temp
}

/// A linha em que o mapa gravado diz que `name` começa em `file`, se ele a
/// declara.
fn line_of(dir: &Path, file: &str, name: &str) -> Option<u64> {
    let model = model::read(&map_folder(dir));
    let module = model["modules"].as_array().unwrap().iter().find(|m| m["path"] == json!(file))?.clone();
    module["declarations"].as_array()?.iter().find(|d| d["name"] == json!(name)).and_then(|d| d["line"].as_u64())
}

/// O mapa lido por partes é o mapa lido de uma vez.
fn same_as_a_full_pass(dir: &Path) {
    let stepped = model::read_bytes(&map_folder(dir));
    assert_eq!(scan(dir, &["--all"])["full"], json!(true));
    assert_eq!(model::read_bytes(&map_folder(dir)), stepped, "reading only what changed gives the same map");
}

/// Editar um arquivo sem commit e ler de novo dá a linha nova, relendo só
/// ele; a passada seguinte, sem nada mudado, não relê nada.
#[test]
fn an_edit_without_commit_is_read_and_gives_the_new_line() {
    let temp = project("scan-blob-edit-", &[("src/a.rs", "pub fn alpha() {}\n"), ("src/b.rs", "pub fn beta() {}\n")]);
    let dir = temp.path();
    assert_eq!(scan(dir, &[])["full"], json!(true));
    assert_eq!(line_of(dir, "src/a.rs", "alpha"), Some(1));

    write(dir, "src/a.rs", "// topo\n\npub fn alpha() {}\n");
    let second = scan(dir, &[]);
    assert_eq!(second["full"], json!(false), "{second}");
    assert_eq!(second["read"], json!(["src/a.rs"]), "{second}");
    assert_eq!(line_of(dir, "src/a.rs", "alpha"), Some(3));
    assert_eq!(scan(dir, &[])["read"], json!([]), "nothing changed since");
    same_as_a_full_pass(dir);
}

/// Trocar de branch dá as linhas da branch nova, relendo só o que nela é
/// diferente.
#[test]
fn switching_branch_gives_the_lines_of_the_new_branch() {
    let temp = project("scan-blob-branch-", &[("src/a.rs", "pub fn alpha() {}\n"), ("src/b.rs", "pub fn beta() {}\n")]);
    let dir = temp.path();
    git(dir, &["checkout", "-q", "-b", "other"]);
    write(dir, "src/a.rs", "\n\n\npub fn alpha() {}\n");
    git(dir, &["commit", "-q", "-am", "moved"]);
    git(dir, &["checkout", "-q", "main"]);
    assert_eq!(scan(dir, &[])["full"], json!(true));
    assert_eq!(line_of(dir, "src/a.rs", "alpha"), Some(1));

    git(dir, &["checkout", "-q", "other"]);
    let there = scan(dir, &[]);
    assert_eq!(there["read"], json!(["src/a.rs"]), "{there}");
    assert_eq!(line_of(dir, "src/a.rs", "alpha"), Some(4));
    same_as_a_full_pass(dir);
}

/// Voltar a branch um commit tira do mapa a função que só existia nele.
#[test]
fn moving_the_branch_back_one_commit_drops_what_only_it_had() {
    let temp = project("scan-blob-back-", &[("src/a.rs", "pub fn alpha() {}\n")]);
    let dir = temp.path();
    write(dir, "src/a.rs", "pub fn alpha() {}\npub fn gamma() {}\n");
    git(dir, &["commit", "-q", "-am", "gamma"]);
    assert_eq!(scan(dir, &[])["full"], json!(true));
    assert_eq!(line_of(dir, "src/a.rs", "gamma"), Some(2));

    git(dir, &["reset", "-q", "--hard", "HEAD~1"]);
    let back = scan(dir, &[]);
    assert_eq!(back["read"], json!(["src/a.rs"]), "{back}");
    assert_eq!(line_of(dir, "src/a.rs", "gamma"), None, "the function only the dropped commit had is gone");
    assert_eq!(line_of(dir, "src/a.rs", "alpha"), Some(1));
    same_as_a_full_pass(dir);
}

/// O arquivo com o mesmo conteúdo em outra branch não é relido: mudar de
/// branch para uma em que o conteúdo foi e voltou lê zero arquivos.
#[test]
fn the_same_content_on_another_branch_is_not_read_again() {
    let temp = project("scan-blob-same-", &[("src/a.rs", "pub fn alpha() {}\n"), ("src/b.rs", "pub fn beta() {}\n")]);
    let dir = temp.path();
    git(dir, &["checkout", "-q", "-b", "other"]);
    write(dir, "src/a.rs", "pub fn alpha() { todo!() }\n");
    git(dir, &["commit", "-q", "-am", "there"]);
    write(dir, "src/a.rs", "pub fn alpha() {}\n");
    git(dir, &["commit", "-q", "-am", "and back"]);
    git(dir, &["checkout", "-q", "main"]);
    assert_eq!(scan(dir, &[])["full"], json!(true));

    git(dir, &["checkout", "-q", "other"]);
    let there = scan(dir, &[]);
    assert_eq!(there["full"], json!(false), "{there}");
    assert_eq!(there["read"], json!([]), "{there}");
    let model = model::read(&map_folder(dir));
    assert_eq!(model["history"]["commits"].as_array().unwrap().len(), 3, "the history follows the branch");
}

/// Um repositório ainda sem commit lê só o que mudou, como qualquer outro:
/// o conteúdo de cada arquivo tem blob mesmo antes do primeiro commit.
#[test]
fn a_repository_with_no_commit_yet_reads_only_what_changed() {
    let temp = tempfile::Builder::new().prefix("scan-blob-no-commit-").tempdir().unwrap();
    let dir = temp.path();
    git(dir, &["init", "-q"]);
    let exclude = mustard_core::footprint_rules().join("\n") + "\n";
    std::fs::write(dir.join(".git").join("info").join("exclude"), exclude).unwrap();
    write(dir, "a.rs", "pub fn a() {}\n");
    write(dir, "b.rs", "pub fn b() {}\n");
    assert_eq!(scan(dir, &[])["full"], json!(true));
    assert_eq!(scan(dir, &[])["read"], json!([]), "nothing changed");
    write(dir, "b.rs", "pub fn b() {}\npub fn c() {}\n");
    assert_eq!(scan(dir, &[])["read"], json!(["b.rs"]));
}
