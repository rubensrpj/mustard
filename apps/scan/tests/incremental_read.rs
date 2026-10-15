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
/// A passada nativa de um mapa que outra passada nativa segura sai sem ler
/// nada e diz que está ocupada; solta a trava, a seguinte lê o projeto.
#[test]
fn a_native_pass_of_a_map_another_native_pass_holds_reads_nothing_and_says_busy() {
    let temp = project("scan-native-busy-", &[("src/a.rs", "pub fn alpha() -> u32 {\n    1\n}\n")]);
    let dir = temp.path();
    let out = model::path_in(&map_folder(dir));
    let pass = || native_pass(dir, &out);
    let running = mustard_core::io::fs::lock::LockedFile::exclusive_if_free(&mustard_core::domain::scan::native_pass_lock_path(&out))
        .unwrap()
        .expect("no other pass holds this map");
    let busy = pass();
    assert_eq!((busy["busy"].clone(), busy["read"].clone()), (json!(true), json!([])), "{busy}");
    assert!(!out.exists(), "the busy pass wrote no map");
    drop(running);
    let read = pass();
    assert_eq!(read["busy"], Value::Null, "{read}");
    assert!(read["read"].as_array().is_some_and(|files| files.contains(&json!("src/a.rs"))), "{read}");
    assert!(out.exists(), "the free pass wrote the map");
}

/// Roda a passada nativa do scan sobre `dir`, gravando o mapa em `out`, e
/// devolve o relato dela.
fn native_pass(dir: &Path, out: &Path) -> Value {
    let run = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["scan", dir.to_str().unwrap(), "--out", out.to_str().unwrap(), "--json", "--native"])
        .output()
        .expect("run scan");
    assert!(run.status.success(), "stderr: {}", String::from_utf8_lossy(&run.stderr));
    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    serde_json::from_str::<Value>(stdout.lines().last().unwrap_or("{}")).expect("the report is one JSON line")
}

/// A passada nativa grava, depois do mapa, os vetores de código do que mudou:
/// a busca que vem depois dela não tem vetor a calcular. Num projeto em
/// TypeScript, na primeira passada e na que relê um arquivo.
#[test]
fn a_native_pass_leaves_no_code_vector_for_the_next_search_to_compute() {
    let temp = project("scan-native-vectors-", &[("src/alpha.ts", "export function alphaStart(): number {\n  return 1;\n}\n")]);
    let dir = temp.path();
    let out = model::path_in(&map_folder(dir));
    let first = native_pass(dir, &out);
    assert!(first["vectors"].as_u64().is_some_and(|count| count > 0), "{first}");
    assert_eq!(mustard_core::io::code_vectors::fill_at(&out, dir).unwrap(), Some(0), "nothing left for the search");

    write(dir, "src/alpha.ts", "export function alphaStart(): number {\n  return 2;\n}\n");
    let second = native_pass(dir, &out);
    assert_eq!(second["read"], json!(["src/alpha.ts"]), "{second}");
    assert!(second["vectors"].as_u64().is_some_and(|count| count > 0), "{second}");
    assert_eq!(mustard_core::io::code_vectors::fill_at(&out, dir).unwrap(), Some(0), "nothing left for the search");
}

/// A passada que relê um arquivo calcula o vetor de código só da declaração
/// cujo texto mudou: a outra do mesmo arquivo, que só desceu de linha, e a do
/// arquivo que a chama guardam o vetor que já tinham. Num projeto em
/// TypeScript.
#[test]
fn a_pass_that_rereads_a_file_computes_code_vectors_only_for_the_declarations_whose_text_changed() {
    let temp = project(
        "scan-native-vector-reuse-",
        &[
            ("src/alpha.ts", "export function alphaStart(): number {\n  return 1;\n}\n\nexport function alphaEnd(): number {\n  return 9;\n}\n"),
            ("src/beta.ts", "import { alphaEnd } from './alpha';\n\nexport function betaRun(): number {\n  return alphaEnd() + 1;\n}\n"),
        ],
    );
    let dir = temp.path();
    let out = model::path_in(&map_folder(dir));
    let first = native_pass(dir, &out);
    assert_eq!(first["vectors"], json!(3), "{first}");

    write(dir, "src/alpha.ts", "export function alphaStart(): number {\n  const base = 1;\n  return base + 1;\n}\n\nexport function alphaEnd(): number {\n  return 9;\n}\n");
    let second = native_pass(dir, &out);
    assert_eq!(second["read"], json!(["src/alpha.ts"]), "{second}");
    assert_eq!(second["vectors"], json!(1), "only alphaStart changed its text: {second}");
    let conn = rusqlite::Connection::open(&out).unwrap();
    let kept: i64 = conn.query_row("SELECT count(*) FROM code_vectors v JOIN knowledge_symbols s ON s.id = v.id AND s.sha256 = v.sha256", [], |r| r.get(0)).unwrap();
    assert_eq!(kept, 3, "every declaration still has a current vector");
}

/// O índice de busca do mapa em `out`, tabela por tabela, como o banco o
/// guarda: cada forma de cada campo de cada documento, o tamanho dos campos,
/// os números de cada nível, os nomes da tabela trigram, as declarações com o
/// número de cada uma e o catálogo dos símbolos.
fn search_index(out: &Path) -> Vec<Vec<String>> {
    let conn = rusqlite::Connection::open(out).unwrap();
    [
        "SELECT term, doc, col, offset FROM decl_vocab ORDER BY 1, 2, 3, 4",
        "SELECT term, doc, col, offset FROM file_vocab ORDER BY 1, 2, 3, 4",
        "SELECT * FROM decl_lengths ORDER BY id",
        "SELECT * FROM file_lengths ORDER BY id",
        "SELECT key, value FROM search_meta ORDER BY key",
        "SELECT rowid, name, folded, file FROM decl_trigram ORDER BY rowid",
        "SELECT rowid, file, name FROM decls ORDER BY rowid",
        "SELECT rowid, path FROM files ORDER BY rowid",
        "SELECT id, path, name, line, end_line, sha256, position FROM knowledge_symbols ORDER BY id",
    ]
    .iter()
    .map(|query| {
        let mut stmt = conn.prepare(query).unwrap();
        let width = stmt.column_count();
        stmt.query_map([], |row| Ok((0..width).map(|at| format!("{:?}", row.get_ref(at).unwrap())).collect::<Vec<_>>().join("|")))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    })
    .collect()
}

/// A passada que relê um arquivo troca só a parte dele no índice de busca,
/// sem refazer o índice inteiro, e o índice fica o mesmo da passada que lê
/// tudo num mapa novo, também depois de o arquivo ganhar uma declaração. Num
/// projeto em TypeScript.
#[test]
fn a_pass_that_rereads_one_file_rewrites_only_its_part_of_the_search_index() {
    let temp = project(
        "scan-index-in-place-",
        &[
            ("src/alpha.ts", "export function alphaStart(): number {\n  return 1;\n}\n"),
            ("src/beta.ts", "export const betaLabel = 'beta';\n"),
            ("src/delta.ts", "export class DeltaBox {\n  size = 3;\n}\n"),
            ("src/epsilon.ts", "export interface EpsilonShape {\n  side: number;\n}\n"),
            ("src/gamma.ts", "import { alphaStart } from './alpha';\nexport function gammaFifth(): number {\n  return alphaStart() + 5;\n}\n"),
            ("src/zeta.ts", "export function zetaLast(text: string): string {\n  return text.toUpperCase();\n}\n"),
        ],
    );
    let dir = temp.path();
    let out = model::path_in(&map_folder(dir));
    assert_eq!(scan(dir, &[])["full"], json!(true));
    // Uma marca no índice que só a montagem inteira dele apaga.
    let conn = rusqlite::Connection::open(&out).unwrap();
    conn.execute("INSERT INTO search_meta(key, value) VALUES ('kept-by-the-pass', 1)", []).unwrap();
    drop(conn);

    let gamma = "import { alphaStart } from './alpha';\nexport function gammaFifth(): number {\n  return alphaStart() + 50;\n}\n";
    write(dir, "src/gamma.ts", gamma);
    let second = scan(dir, &[]);
    assert_eq!(second["read"], json!(["src/gamma.ts"]), "{second}");
    let conn = rusqlite::Connection::open(&out).unwrap();
    let kept: i64 = conn.query_row("SELECT count(*) FROM search_meta WHERE key = 'kept-by-the-pass'", [], |row| row.get(0)).unwrap();
    assert_eq!(kept, 1, "the pass did not rebuild the whole index");
    conn.execute("DELETE FROM search_meta WHERE key = 'kept-by-the-pass'", []).unwrap();
    drop(conn);

    write(dir, "src/gamma.ts", &format!("{gamma}export function gammaAdded(): void {{}}\n"));
    assert_eq!(scan(dir, &[])["read"], json!(["src/gamma.ts"]));
    let whole = tempfile::tempdir().unwrap();
    model::scan(dir, whole.path(), &[]);
    assert_eq!(search_index(&out), search_index(&model::path_in(whole.path())), "the index after one file is the index of a full pass");
}

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

/// A passada que lê um arquivo novo e relê um que ganhou uma função
/// regrava, depois deles, as linhas de um arquivo que não leu, com os
/// cartões que só o banco guardava: só os cartões dos dois se refazem, e o
/// mapa sai igual ao da leitura inteira.
#[test]
fn a_pass_that_moves_the_rows_of_an_untouched_file_keeps_its_cards() {
    let temp = project(
        "scan-moved-cards-",
        &[
            ("src/a.ts", "export function a(): number {\n  return 1;\n}\n"),
            ("src/m.ts", "export function m(): number {\n  return 2;\n}\n"),
            ("src/z.ts", "export function z(): number {\n  return 3;\n}\n"),
        ],
    );
    let dir = temp.path();
    assert_eq!(scan(dir, &[])["full"], json!(true));

    write(dir, "src/b.ts", "export function b(): number {\n  return 4;\n}\n");
    write(dir, "src/m.ts", "export function m(): number {\n  return 2;\n}\n\nexport function n(): number {\n  return 5;\n}\n");
    let report = scan(dir, &[]);
    assert_eq!(report["read"], json!(["src/b.ts", "src/m.ts"]), "{report}");
    assert_eq!(report["enriched"], json!(2), "{report}");
    same_as_a_full_pass(dir);
}

/// A passada que relê um arquivo refaz os cartões de conhecimento só dos
/// arquivos cujas entradas mudaram: o arquivo editado e, quando a declaração
/// chamada muda de linha, o arquivo que a chama, cujo cartão aponta para ela.
/// Os outros ficam com os cartões que já tinham, e o mapa sai igual ao da
/// leitura inteira.
#[test]
fn rereading_one_file_rebuilds_only_the_cards_whose_inputs_changed() {
    let temp = project(
        "scan-cards-",
        &[
            ("src/total.ts", "export function total(): number {\n  return 1;\n}\n"),
            ("src/pedido.ts", "import { total } from './total';\n\nexport function pedido(): number {\n  return total() + 1;\n}\n"),
            ("src/avulso.ts", "export function avulso(): number {\n  return 3;\n}\n"),
            ("src/outro.ts", "export function outro(): number {\n  return 4;\n}\n"),
        ],
    );
    let dir = temp.path();
    let first = scan(dir, &[]);
    assert_eq!(first["full"], json!(true), "{first}");
    assert_eq!(first["enriched"], first["files"], "{first}");

    // Só o corpo de um arquivo que ninguém chama muda.
    write(dir, "src/avulso.ts", "export function avulso(): number {\n  return 5;\n}\n");
    let second = scan(dir, &[]);
    assert_eq!(second["read"], json!(["src/avulso.ts"]), "{second}");
    assert_eq!(second["enriched"], json!(1), "{second}");

    // A função chamada desce uma linha: o arquivo dela e o de quem a chama.
    write(dir, "src/total.ts", "// Soma do pedido.\nexport function total(): number {\n  return 1;\n}\n");
    let third = scan(dir, &[]);
    assert_eq!(third["read"], json!(["src/total.ts"]), "{third}");
    assert_eq!(third["enriched"], json!(2), "{third}");
    same_as_a_full_pass(dir);
}

/// O uso que nenhum arquivo escreve, plantado numa declaração do mapa: só a
/// passada que resolve de novo as chamadas que a alcançam o tira.
const GHOST_USE: &str = "src/ghost.ts:9:ghost";

/// Os usos que o mapa gravado dá à declaração `name` de `file`.
fn uses_in_the_map(dir: &Path, file: &str, name: &str) -> Vec<Value> {
    let map = model::read(&map_folder(dir));
    let module = map["modules"].as_array().unwrap().iter().find(|m| m["path"] == json!(file)).expect("o arquivo está no mapa");
    let decl = module["declarations"].as_array().unwrap().iter().find(|d| d["name"] == json!(name)).expect("a declaração está no mapa");
    decl["used_by"].as_array().cloned().unwrap_or_default()
}

/// Planta o uso falso na declaração `name` de `file` (`plant`) ou o tira,
/// pelo porto, com a marca guardada.
fn ghost_use(dir: &Path, file: &str, name: &str, plant: bool) {
    model::edit_keeping_the_mark(&map_folder(dir), |map| {
        let module = map["modules"].as_array_mut().unwrap().iter_mut().find(|m| m["path"] == json!(file)).unwrap();
        let decl = module["declarations"].as_array_mut().unwrap().iter_mut().find(|d| d["name"] == json!(name)).unwrap();
        let decl = decl.as_object_mut().unwrap();
        let uses = decl.entry("used_by").or_insert(json!([])).as_array_mut().unwrap();
        uses.retain(|site| *site != json!(GHOST_USE));
        if plant {
            uses.push(json!(GHOST_USE));
        }
        // A lista vazia não se grava, como na passada do scan.
        if uses.is_empty() {
            decl.remove("used_by");
        }
    });
}

/// A passada que relê um arquivo resolve de novo só as chamadas que podem
/// mudar com ele, e o mapa sai o da leitura inteira, com a função do arquivo
/// lido que só desceu de linha e a que trocou de nome. O uso plantado na
/// declaração que nenhuma chamada do arquivo lido alcança fica; a declaração
/// que muda de tipo tem as chamadas dela resolvidas de novo, e o uso plantado
/// sai. Num projeto em TypeScript.
#[test]
fn a_pass_that_rereads_one_file_resolves_again_only_the_calls_that_can_change() {
    let temp = project(
        "scan-relink-",
        &[
            ("src/alpha.ts", "export function alpha(): number {\n  return 1;\n}\n"),
            ("src/beta.ts", "import { alpha } from './alpha';\nexport function beta(): number {\n  return alpha();\n}\n"),
            ("src/gamma.ts", "export function gamma(): number {\n  return 2;\n}\nexport function delta(): number {\n  return 3;\n}\n"),
            ("src/uses.ts", "import { gamma, delta } from './gamma';\nexport function both(): number {\n  return gamma() + delta();\n}\n"),
        ],
    );
    let dir = temp.path();
    assert_eq!(scan(dir, &[])["full"], json!(true));

    let gamma = |body: u32| {
        format!("// topo\n\nexport function gamma(): number {{\n  return {body};\n}}\nexport function epsilon(): number {{\n  return 3;\n}}\n")
    };
    write(dir, "src/gamma.ts", &gamma(20));
    let second = scan(dir, &[]);
    assert_eq!((second["full"].clone(), second["read"].clone()), (json!(false), json!(["src/gamma.ts"])), "{second}");
    assert_eq!(uses_in_the_map(dir, "src/gamma.ts", "gamma"), [json!("src/uses.ts:3:both")]);
    same_as_a_full_pass(dir);

    // O uso plantado só sai da declaração cujas chamadas se resolvem de novo.
    // O cartão de conhecimento da declaração o guarda, e por isso o mapa só se
    // compara com o da leitura inteira depois que ela muda.
    ghost_use(dir, "src/alpha.ts", "alpha", true);
    write(dir, "src/gamma.ts", &gamma(30));
    assert_eq!(scan(dir, &[])["read"], json!(["src/gamma.ts"]));
    assert_eq!(
        uses_in_the_map(dir, "src/alpha.ts", "alpha"),
        [json!("src/beta.ts:3:beta"), json!(GHOST_USE)],
        "the calls that reach alpha were not resolved again"
    );

    write(dir, "src/alpha.ts", "export const alpha = 1;\n");
    assert_eq!(scan(dir, &[])["full"], json!(false));
    assert!(!uses_in_the_map(dir, "src/alpha.ts", "alpha").contains(&json!(GHOST_USE)), "a declaration that changed kind is linked again");
    same_as_a_full_pass(dir);
}

/// O contexto com o campo `config`, do tipo `field_type`.
fn csharp_context(field_type: &str) -> String {
    format!("namespace Loja;\n\npublic class Ctx\n{{\n    public {field_type} config;\n}}\n")
}

/// O campo que troca de tipo no arquivo relido leva a chamada lida por ele
/// noutro arquivo (`_ctx.config.Lingua()`) ao método do tipo novo, como na
/// leitura inteira: o arquivo que escreve a chamada não muda, e o nome
/// chamado também não. Num projeto em C#.
#[test]
fn a_field_that_changes_type_in_the_reread_file_takes_the_call_read_through_it_to_the_new_type() {
    let reader = "namespace Loja;\n\npublic class Leitor\n{\n    private readonly Ctx _ctx;\n\n    public int Ler()\n    {\n        return _ctx.config.Lingua();\n    }\n}\n";
    let temp = project(
        "scan-field-type-",
        &[
            ("src/Config.cs", "namespace Loja;\n\npublic class Config\n{\n    public int Lingua() { return 1; }\n}\n"),
            ("src/Outra.cs", "namespace Loja;\n\npublic class Outra\n{\n    public int Lingua() { return 0; }\n}\n"),
            ("src/Ctx.cs", &csharp_context("Config")),
            ("src/Leitor.cs", reader),
        ],
    );
    let dir = temp.path();
    assert_eq!(scan(dir, &[])["full"], json!(true));
    let call = json!("src/Leitor.cs:9:Ler");
    assert_eq!(uses_in_the_map(dir, "src/Config.cs", "Lingua"), [call.clone()]);

    write(dir, "src/Ctx.cs", &csharp_context("Outra"));
    let second = scan(dir, &[]);
    assert_eq!((second["full"].clone(), second["read"].clone()), (json!(false), json!(["src/Ctx.cs"])), "{second}");
    assert_eq!(uses_in_the_map(dir, "src/Outra.cs", "Lingua"), [call], "the call follows the new type of the field");
    assert!(uses_in_the_map(dir, "src/Config.cs", "Lingua").is_empty(), "the old type keeps no use");
    same_as_a_full_pass(dir);
}

/// Nove funções `comum` e dois arquivos que a chamam sem importar nenhuma:
/// cada chamada alcança as nove, acima do teto do nome comum, e só se conta.
/// Reler um dos chamadores conta a chamada dele uma vez só, como na leitura
/// inteira. Num projeto em TypeScript.
#[test]
fn rereading_one_caller_counts_its_common_call_once() {
    let caller = |name: &str| format!("export function {name}(): number {{\n  return comum();\n}}\n");
    let mut files: Vec<(String, String)> =
        (1..=9).map(|n| (format!("src/m{n}.ts"), format!("export function comum(): number {{\n  return {n};\n}}\n"))).collect();
    files.push(("src/a.ts".to_string(), caller("a")));
    files.push(("src/b.ts".to_string(), caller("b")));
    let files: Vec<(&str, &str)> = files.iter().map(|(rel, body)| (rel.as_str(), body.as_str())).collect();
    let temp = project("scan-common-calls-", &files);
    let dir = temp.path();
    assert_eq!(scan(dir, &[])["full"], json!(true));
    let common_calls = || -> Value {
        let map = model::read(&map_folder(dir));
        let m1 = map["modules"].as_array().unwrap().iter().find(|m| m["path"] == json!("src/m1.ts")).unwrap();
        m1["declarations"].as_array().unwrap().iter().find(|d| d["name"] == json!("comum")).unwrap()["common_calls"].clone()
    };
    assert_eq!(common_calls(), json!(2));

    write(dir, "src/a.ts", &format!("// o primeiro chamador\n{}", caller("a")));
    let second = scan(dir, &[]);
    assert_eq!((second["full"].clone(), second["read"].clone()), (json!(false), json!(["src/a.ts"])), "{second}");
    assert_eq!(common_calls(), json!(2), "the reread caller's call is counted once");
    same_as_a_full_pass(dir);
}
