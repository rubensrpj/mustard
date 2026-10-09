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

/// Declara no `mustard.json` a `main` como a branch de partida, de onde vem
/// a história que o mapa guarda.
fn declare_base(dir: &Path) {
    write(dir, "mustard.json", r#"{"git": {"flow": {"*": "main"}}}"#);
}

#[test]
fn a_second_pass_reads_only_the_changed_file_and_leaves_git_clean() {
    let temp = tempfile::Builder::new().prefix("scan-incremental-").tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    git(&dir, &["init", "-q", "-b", "main"]);
    let exclude = mustard_core::footprint_rules().join("\n") + "\n";
    std::fs::write(dir.join(".git").join("info").join("exclude"), exclude).unwrap();
    declare_base(&dir);

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
    let user_module = model["modules"].as_array().unwrap().iter().find(|m| m["path"] == json!("src/usa.ts")).unwrap();
    assert_eq!(user_module["deps"], json!(["src/b/pedido.ts"]), "{user_module}");
}

/// Mudar só o teto do nome comum no `mustard.json`, sem mexer em arquivo de
/// código nem em commit, muda a ligação na passada seguinte: ela religa o
/// projeto sem reler arquivo nenhum, e a chamada que alcança nove
/// declarações, só contada com o padrão, passa a ligar suspeita a cada uma.
#[test]
fn changing_only_the_common_name_ceiling_links_again_without_reading() {
    let temp = tempfile::Builder::new().prefix("scan-incremental-ceiling-").tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    git(&dir, &["init", "-q"]);
    let exclude = mustard_core::footprint_rules().join("\n") + "\n";
    std::fs::write(dir.join(".git").join("info").join("exclude"), exclude).unwrap();

    for n in 1..=9 {
        write(&dir, &format!("src/m{n}.rs"), &format!("pub fn comum() -> u32 {{\n    {n}\n}}\n"));
    }
    write(&dir, "src/chama.rs", "pub fn chama() -> u32 {\n    comum()\n}\n");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "first"]);
    assert_eq!(scan(&dir, &[])["full"], json!(true));
    let common = |model: &Value| -> Value {
        let m1 = model["modules"].as_array().unwrap().iter().find(|m| m["path"] == json!("src/m1.rs")).unwrap();
        m1["declarations"].as_array().unwrap().iter().find(|d| d["name"] == json!("comum")).unwrap().clone()
    };
    let before = common(&model::read(&map_folder(&dir)));
    let uses = |d: &Value| d["used_by"].as_array().map_or(0, Vec::len);
    assert_eq!((before["common_calls"].as_u64().unwrap_or(0), uses(&before)), (1, 0), "{before}");

    write(&dir, "mustard.json", r#"{"scan": {"max_same_name": 9}}"#);
    let second = scan(&dir, &[]);
    assert_eq!((second["full"].clone(), second["read"].clone()), (json!(false), json!([])), "{second}");
    let after = common(&model::read(&map_folder(&dir)));
    assert_eq!((after["common_calls"].as_u64().unwrap_or(0), uses(&after)), (0, 1), "{after}");
    let used_by = after["used_by"].as_array().unwrap();
    assert_eq!(used_by[0]["at"], json!("src/chama.rs:2:chama"), "{after}");
    assert_eq!(used_by[0]["candidates"].as_array().unwrap().len(), 9, "{after}");
}

/// Mudar só a configuração de apelidos do JavaScript faz a passada seguinte
/// ler o projeto inteiro, como a do TypeScript.
#[test]
fn changing_only_the_javascript_alias_configuration_reads_everything_again() {
    let temp = tempfile::Builder::new().prefix("scan-incremental-js-alias-").tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    git(&dir, &["init", "-q"]);
    let exclude = mustard_core::footprint_rules().join("\n") + "\n";
    std::fs::write(dir.join(".git").join("info").join("exclude"), exclude).unwrap();

    write(&dir, "jsconfig.json", "{ \"compilerOptions\": { \"paths\": { \"@app/*\": [\"src/a/*\"] } } }\n");
    write(&dir, "src/a/pedido.js", "export const total = 1;\n");
    write(&dir, "src/b/pedido.js", "export const total = 2;\n");
    write(&dir, "src/usa.js", "import { total } from '@app/pedido';\n\nexport const x = total;\n");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "first"]);
    assert_eq!(scan(&dir, &[])["full"], json!(true));

    write(&dir, "jsconfig.json", "{ \"compilerOptions\": { \"paths\": { \"@app/*\": [\"src/b/*\"] } } }\n");
    git(&dir, &["commit", "-q", "-am", "second"]);
    let second = scan(&dir, &[]);
    assert_eq!(second["full"], json!(true), "{second}");
    assert!(second["read"].as_array().unwrap().contains(&json!("src/usa.js")), "{second}");

    let model: Value = model::read(&map_folder(&dir));
    let user_module = model["modules"].as_array().unwrap().iter().find(|m| m["path"] == json!("src/usa.js")).unwrap();
    assert_eq!(user_module["deps"], json!(["src/b/pedido.js"]), "{user_module}");
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

    let deps_of_usage = || -> Value {
        let model: Value = model::read(&map_folder(&dir));
        model["modules"].as_array().unwrap().iter().find(|m| m["path"] == json!("src/Services/Uso.cs")).unwrap()["deps"]
            .clone()
    };
    assert_eq!(deps_of_usage(), json!(declaring), "the nine files that declare the name it cites");

    write(&dir, "src/Outro.cs", "namespace Loja;\n\npublic class Outro\n{\n    public int Contar() => 0;\n}\n");
    git(&dir, &["commit", "-q", "-am", "second"]);
    let second = scan(&dir, &[]);
    assert_eq!(second["read"], json!(["src/Outro.cs"]), "{second}");
    assert_eq!(deps_of_usage(), json!(declaring), "a pass that did not read Uso.cs keeps its links");

    let stepped = model::read_bytes(&map_folder(&dir));
    assert_eq!(scan(&dir, &["--all"])["full"], json!(true));
    assert_eq!(model::read_bytes(&map_folder(&dir)), stepped, "reading only what changed gives the same map");
}

/// O nome que o próprio código liga vence, também na passada que lê só o que
/// mudou: a variável e o parâmetro chamados pelo nome da `fill` importada, o
/// `fs` trazido da biblioteca antes de `remove_dir_all`, e o `Result` da
/// língua escrito num arquivo que só traz o `Error` do projeto. Mudar só outro
/// arquivo deixa as ligações de quem não foi relido como a leitura inteira.
#[test]
fn a_name_the_code_binds_itself_links_the_same_when_only_another_file_changed() {
    let temp = project(
        "scan-incremental-own-name-",
        &[
            ("src/a.rs", "pub fn fill() -> u32 {\n    1\n}\n"),
            (
                "src/b.rs",
                "use crate::a::fill;\n\npub fn with_local() -> u32 {\n    let fill = |x: u32| x + 1;\n    fill(2)\n}\n\n\
                 pub fn with_parameter(fill: fn() -> u32) -> u32 {\n    fill()\n}\n\n\
                 pub fn vizinha() -> u32 {\n    fill()\n}\n",
            ),
            ("src/fs/mod.rs", "pub mod real;\n\npub fn remove_dir_all() -> u32 {\n    0\n}\n"),
            ("src/fs/real.rs", "use std::fs::{self};\n\npub fn limpa() {\n    let _ = fs::remove_dir_all(\"x\");\n}\n"),
            ("src/erro.rs", "pub type Result<T> = std::result::Result<T, Error>;\npub struct Error;\n"),
            ("src/so_erro.rs", "use crate::erro::Error;\n\npub fn so_erro() -> Result<u32, Error> {\n    Ok(1)\n}\n"),
            ("src/outro.rs", "pub fn outro() {}\n"),
        ],
    );
    let dir = temp.path();
    assert_eq!(scan(dir, &[])["full"], json!(true));

    write(dir, "src/outro.rs", "pub fn outro() {}\npub fn mais() {}\n");
    git(dir, &["commit", "-q", "-am", "second"]);
    let second = scan(dir, &[]);
    assert_eq!(second["read"], json!(["src/outro.rs"]), "{second}");

    let used_by = |file: &str, name: &str| -> Value {
        let model = model::read(&map_folder(dir));
        let module = model["modules"].as_array().unwrap().iter().find(|m| m["path"] == json!(file)).unwrap().clone();
        let decl = module["declarations"].as_array().unwrap().iter().find(|d| d["name"] == json!(name)).unwrap().clone();
        decl["used_by"].clone()
    };
    assert_eq!(used_by("src/a.rs", "fill"), json!(["src/b.rs:13:vizinha"]), "only the neighbour calls the import");
    assert_eq!(used_by("src/fs/mod.rs", "remove_dir_all"), Value::Null, "fs came from outside the project");
    assert_eq!(used_by("src/erro.rs", "Result"), Value::Null, "the language's Result is not the project's");
    same_as_a_full_pass(dir);
}

/// A chamada pelo caminho da biblioteca (`std::fs::read_to_string`) não liga
/// à função de mesmo nome da pasta `fs` do projeto, também na passada que lê
/// só o que mudou: o caminho do arquivo não relido fica com ele no mapa.
#[test]
fn a_call_through_a_library_path_links_the_same_when_only_another_file_changed() {
    let temp = project(
        "scan-incremental-library-path-",
        &[
            ("src/lib.rs", "pub mod io;\npub mod leitor;\npub mod outro;\n"),
            ("src/io/mod.rs", "pub mod fs;\n"),
            ("src/io/fs/mod.rs", "pub fn read_to_string(_: &str) -> String {\n    String::new()\n}\n"),
            (
                "src/leitor.rs",
                "use crate::io::fs;\n\npub fn by_the_project() -> String {\n    fs::read_to_string(\"a\")\n}\n\n\
                 pub fn by_the_library() -> String {\n    std::fs::read_to_string(\"b\").unwrap()\n}\n",
            ),
            ("src/outro.rs", "pub fn outro() {}\n"),
        ],
    );
    let dir = temp.path();
    assert_eq!(scan(dir, &[])["full"], json!(true));

    write(dir, "src/outro.rs", "pub fn outro() {}\npub fn mais() {}\n");
    git(dir, &["commit", "-q", "-am", "second"]);
    let second = scan(dir, &[]);
    assert_eq!(second["read"], json!(["src/outro.rs"]), "{second}");

    let model = model::read(&map_folder(dir));
    let module = model["modules"].as_array().unwrap().iter().find(|m| m["path"] == json!("src/io/fs/mod.rs")).unwrap().clone();
    let decl = module["declarations"].as_array().unwrap().iter().find(|d| d["name"] == json!("read_to_string")).unwrap().clone();
    assert_eq!(decl["used_by"], json!(["src/leitor.rs:4:by_the_project"]), "the library call does not link");
    same_as_a_full_pass(dir);
}

/// Um projeto git com o mapa fora dele, como a instalação o deixa, e um
/// commit com `files`.
fn project(prefix: &str, files: &[(&str, &str)]) -> tempfile::TempDir {
    let temp = tempfile::Builder::new().prefix(prefix).tempdir().unwrap();
    let dir = temp.path();
    git(dir, &["init", "-q", "-b", "main"]);
    let exclude = mustard_core::footprint_rules().join("\n") + "\n";
    std::fs::write(dir.join(".git").join("info").join("exclude"), exclude).unwrap();
    declare_base(dir);
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
    assert_eq!(model["history"]["commits"].as_array().unwrap().len(), 1, "the history stays on the base branch");
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

/// Uma dependência que nenhum arquivo tem. Plantada no mapa, só a passada que
/// refaz o grafo a tira.
const FAKE: &str = "src/nenhum.rs";

/// As dependências que o mapa gravado dá a `file`.
fn deps_in_the_map(dir: &Path, file: &str) -> Vec<Value> {
    let map = model::read(&map_folder(dir));
    let module = map["modules"].as_array().unwrap().iter().find(|m| m["path"] == json!(file)).expect("o arquivo está no mapa");
    module["deps"].as_array().cloned().unwrap_or_default()
}

/// Planta a dependência falsa em `file` (`plant`) ou a tira, pelo porto, com
/// a marca guardada.
fn fake_dependency(dir: &Path, file: &str, plant: bool) {
    model::edit_keeping_the_mark(&map_folder(dir), |map| {
        let module = map["modules"].as_array_mut().unwrap().iter_mut().find(|m| m["path"] == json!(file)).unwrap();
        let module = module.as_object_mut().unwrap();
        let deps = module.entry("deps").or_insert(json!([])).as_array_mut().unwrap();
        deps.retain(|dep| *dep != json!(FAKE));
        if plant {
            deps.push(json!(FAKE));
        }
        // A lista vazia não se grava, como na passada do scan.
        if deps.is_empty() {
            module.remove("deps");
        }
    });
}

/// `change` roda entre duas passadas, com a dependência falsa plantada em
/// `file` antes: a segunda não relê nada nem refaz o grafo, e a falsa fica.
/// Depois ela sai, e o mapa é o que a passada gravou.
fn only_the_census_is_redone(dir: &Path, file: &str, expected_read: &[&str], change: impl FnOnce()) {
    fake_dependency(dir, file, true);
    change();
    let report = scan(dir, &[]);
    assert_eq!((report["full"].clone(), report["read"].clone()), (json!(false), json!(expected_read)), "{report}");
    assert!(deps_in_the_map(dir, file).contains(&json!(FAKE)), "the graph was not rebuilt");
    assert!(!model::is_behind(dir), "the new listing mark is written");
    fake_dependency(dir, file, false);
}

/// O Laravel, que o caminho `artisan` marca, está nas pilhas do projeto e
/// nas do subprojeto: as duas respostas, nessa ordem.
fn laravel_marked(dir: &Path) -> (bool, bool) {
    let map = model::read(&map_folder(dir));
    let has = |stacks: &Value| stacks.as_array().is_some_and(|all| all.iter().any(|s| s["name"] == json!("laravel")));
    (has(&map["detected_stacks"]), has(&map["projects"][0]["detected_stacks"]))
}

/// Um projeto PHP sem o Laravel nas dependências, com um arquivo de código,
/// um manifesto e um `README.md`.
fn php_project(prefix: &str) -> tempfile::TempDir {
    project(
        prefix,
        &[
            ("composer.json", "{\"name\": \"demo/app\", \"require\": {\"php\": \"^8.2\"}}\n"),
            ("app/Models/User.php", "<?php\nnamespace App\\Models;\n\nclass User {}\n"),
            ("README.md", "# Demo\n"),
        ],
    )
}

/// Editar documentação relê o texto e atualiza seu índice sem refazer o
/// código/grafo; a marca da listagem também fica atual.
#[test]
fn editing_documentation_reads_only_the_resource_and_keeps_the_graph() {
    let temp = project(
        "scan-not-code-",
        &[
            ("src/a.rs", "pub fn alpha() {}\n"),
            ("src/b.rs", "use crate::a::alpha;\npub fn beta() {\n    alpha();\n}\n"),
            ("README.md", "# Demo\n"),
        ],
    );
    let dir = temp.path();
    assert_eq!(scan(dir, &[])["full"], json!(true));
    fake_dependency(dir, "src/a.rs", true);

    write(dir, "README.md", "# Demo\n\nMais uma linha.\n");
    assert!(model::is_behind(dir), "the edit puts the map behind");
    let report = scan(dir, &[]);
    assert_eq!(report["full"], json!(false), "{report}");
    assert_eq!(report["read"], json!(["README.md"]), "{report}");
    assert_eq!(report["files"], json!(2), "{report}");
    assert_eq!(report["head"], json!(git(dir, &["rev-parse", "HEAD"]).trim()), "{report}");
    assert!(!model::is_behind(dir), "the new listing mark is written");
    assert!(deps_in_the_map(dir, "src/a.rs").contains(&json!(FAKE)), "the graph was not rebuilt");
}

/// O mapa que uma passada deste scan grava leva a marca que `scan format`
/// diz, em todo bloco, e por ela não está atrás do projeto. O mesmo mapa
/// regravado com a marca de outra compilação do scan está atrás com o
/// projeto parado, no mesmo commit e com o mesmo conteúdo; a passada
/// seguinte lê tudo, grava a marca de agora, e o mapa volta a estar em dia.
#[test]
fn a_map_of_another_scan_build_is_behind_until_a_pass_of_this_one_writes_it() {
    let temp = project("scan-format-mark-", &[("src/a.rs", "pub fn alpha() {}\n"), ("src/b.rs", "pub fn beta() {}\n")]);
    let dir = temp.path();
    assert_eq!(scan(dir, &[])["full"], json!(true));
    let format = model::scan_format();
    assert!(format.contains("+map-"), "the mark carries the digest of the scan sources: {format}");
    assert_eq!(model::marks(&map_folder(dir)), std::collections::BTreeSet::from([format.clone()]), "every block carries it");
    assert!(!model::is_behind(dir), "the map of this build is not behind");

    model::mark_as(&map_folder(dir), "0.0.0+map-other");
    assert!(model::is_behind(dir), "the same project, parked, but a map of another build");
    assert!(
        !mustard_core::io::project_map::is_behind(dir, &|| None),
        "without a scan to ask, the mark does not count and the content is the same"
    );

    let report = scan(dir, &[]);
    assert_eq!(report["full"], json!(true), "the pass reads everything again: {report}");
    assert_eq!(model::marks(&map_folder(dir)), std::collections::BTreeSet::from([format]));
    assert!(!model::is_behind(dir), "written by this build, the map is up to date again");
}

/// Um arquivo que não é código entra, muda e sai, e um deles marca o
/// Laravel pelo caminho: a passada não relê nada, refaz as pilhas do projeto
/// e do subprojeto, e o mapa é o de uma passada que lê tudo.
#[test]
fn a_file_that_is_not_code_entering_changing_and_leaving_gives_the_map_of_a_full_pass() {
    let temp = php_project("scan-path-marker-");
    let dir = temp.path();
    assert_eq!(scan(dir, &[])["full"], json!(true));
    assert_eq!(laravel_marked(dir), (false, false));
    let file = "app/Models/User.php";

    only_the_census_is_redone(dir, file, &[], || write(dir, "artisan", "#!/usr/bin/env php\n<?php\n"));
    assert_eq!(laravel_marked(dir), (true, true), "the path marker entered");
    same_as_a_full_pass(dir);

    only_the_census_is_redone(dir, file, &[], || write(dir, "artisan", "#!/usr/bin/env php\n<?php\n// outra\n"));
    same_as_a_full_pass(dir);

    only_the_census_is_redone(dir, file, &["docs/notas.txt"], || write(dir, "docs/notas.txt", "notas\n"));
    same_as_a_full_pass(dir);

    only_the_census_is_redone(dir, file, &["README.md"], || write(dir, "README.md", "# Demo\n\nOutra linha.\n"));
    same_as_a_full_pass(dir);

    only_the_census_is_redone(dir, file, &[], || std::fs::remove_file(dir.join("artisan")).unwrap());
    assert_eq!(laravel_marked(dir), (false, false), "the path marker left");
    same_as_a_full_pass(dir);
}

/// O `artisan` dentro de `tests/fixtures/` é de um teste: não marca o
/// Laravel, nem na passada que só refaz o censo nem na que lê tudo.
#[test]
fn a_path_marker_inside_test_fixtures_marks_no_stack_on_either_pass() {
    let temp = php_project("scan-fixture-marker-");
    let dir = temp.path();
    assert_eq!(scan(dir, &[])["full"], json!(true));

    only_the_census_is_redone(dir, "app/Models/User.php", &[], || {
        write(dir, "tests/fixtures/artisan", "#!/usr/bin/env php\n<?php\n");
    });
    assert_eq!(laravel_marked(dir), (false, false), "the census-only pass");
    same_as_a_full_pass(dir);
    assert_eq!(laravel_marked(dir), (false, false), "the full pass");
}

/// Um arquivo de código novo, e um manifesto novo, continuam lidos: a
/// passada refaz o grafo, e a dependência falsa plantada antes sai.
#[test]
fn a_new_code_file_or_manifest_is_still_read() {
    let temp = project("scan-new-source-", &[("src/a.rs", "pub fn alpha() {}\n"), ("README.md", "# Demo\n")]);
    let dir = temp.path();
    assert_eq!(scan(dir, &[])["full"], json!(true));

    for (file, body) in [("src/c.rs", "pub fn gamma() {}\n"), ("tools/package.json", "{\"name\": \"tools\"}\n")] {
        fake_dependency(dir, "src/a.rs", true);
        write(dir, file, body);
        let report = scan(dir, &[]);
        assert_eq!(report["read"], json!([file]), "{report}");
        assert!(!deps_in_the_map(dir, "src/a.rs").contains(&json!(FAKE)), "the graph was rebuilt for {file}");
        same_as_a_full_pass(dir);
    }
}

/// Um arquivo de código que o git ainda lista, mas que a caminhada deixou de
/// ver por uma regra nova do `.gitignore`, sai do mapa como na passada que
/// lê tudo.
#[test]
fn a_code_file_the_walk_stops_seeing_leaves_the_map() {
    let temp = project("scan-now-ignored-", &[("src/a.rs", "pub fn alpha() {}\n"), ("src/b.rs", "pub fn beta() {}\n")]);
    let dir = temp.path();
    assert_eq!(scan(dir, &[])["full"], json!(true));

    write(dir, ".gitignore", "src/b.rs\n");
    let report = scan(dir, &[]);
    assert_eq!(report["files"], json!(1), "{report}");
    same_as_a_full_pass(dir);
}

/// A passada que só relê a tela liga igual à que lê tudo: o `index.ts` da
/// pasta importada, que não é relido, guarda no mapa o que repassa, e a tela
/// segue dependendo do serviço que declara o nome.
#[test]
fn a_pass_that_rereads_only_the_importer_follows_what_the_kept_file_passes_on() {
    let screen = "import { buscarPedido } from './pedidos';\n\nexport function tela() {\n  return buscarPedido(1);\n}\n";
    let temp = project(
        "scan-reexport-",
        &[
            ("web/src/pedidos/index.ts", "export * from './pedido.service';\n"),
            ("web/src/pedidos/pedido.service.ts", "export function buscarPedido(id: number) {\n  return id;\n}\n"),
            ("web/src/tela.ts", screen),
        ],
    );
    let dir = temp.path();
    assert_eq!(scan(dir, &[])["full"], json!(true));

    write(dir, "web/src/tela.ts", &format!("// a tela dos pedidos\n{screen}"));
    let report = scan(dir, &[]);
    assert_eq!(report["read"], json!(["web/src/tela.ts"]), "{report}");
    assert_eq!(deps_in_the_map(dir, "web/src/tela.ts"), vec![json!("web/src/pedidos/pedido.service.ts")]);
    same_as_a_full_pass(dir);
}
