//! O módulo sem corpo que o arquivo marca como teste (`#[cfg(test)] mod x;`)
//! nomeia um arquivo que é todo de teste: o mapa o guarda como um trecho de
//! teste que vai da linha 1 ao fim, e o que mora na pasta dele também. A
//! chamada que esse arquivo escreve não é uso de quem ela chama.
//!
//! O projeto de teste tem `src/real.rs` com o código, `src/helpers.rs` e
//! `src/helpers/inner.rs` declarados como teste, e um módulo de cada forma que
//! não é: sem marca, com a marca negada e com `#[path]`, antes ou depois da
//! marca.

#[path = "support/model.rs"]
mod model;

use std::path::Path;
use std::process::Command;

use mustard_core::domain::project_map::WHOLE_FILE_END;
use serde_json::Value;

fn project_dir(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new().prefix(&format!("scan-{name}-")).tempdir().unwrap()
}

fn write(dir: &Path, rel: &str, body: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(["-c", "user.email=scan@example.com", "-c", "user.name=scan", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

fn scan(dir: &Path) -> (Value, Value) {
    model::scan(dir, &dir.join(".claude"), &[])
}

const LIB: &str = "pub mod real;\npub mod plain;\n\n\
                   #[cfg(test)]\nmod helpers;\n\n\
                   #[cfg(all(test, unix))]\n#[allow(dead_code)]\nmod combined;\n\n\
                   #[cfg(not(test))]\nmod gated;\n\n\
                   #[cfg(test)]\n#[path = \"../fixtures/before.rs\"]\nmod before;\n\n\
                   #[path = \"../fixtures/after.rs\"]\n#[cfg(test)]\nmod after;\n";

const HELPERS: &str = "mod inner;\n\nuse crate::real::limit;\n\npub fn searched() -> u32 {\n    limit()\n}\n\n\
                       #[test]\nfn finds_it() {\n    assert_eq!(searched(), 10);\n}\n";

fn project(dir: &Path) {
    write(dir, "Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n");
    write(dir, "src/lib.rs", LIB);
    write(dir, "src/real.rs", "pub fn limit() -> u32 {\n    10\n}\n");
    write(dir, "src/plain.rs", "pub fn plain() {}\n");
    write(dir, "src/helpers.rs", HELPERS);
    write(dir, "src/helpers/inner.rs", "pub fn deep() -> u32 {\n    crate::real::limit()\n}\n");
    write(dir, "src/combined.rs", "pub fn combined() {}\n");
    write(dir, "src/gated.rs", "pub fn gated() {}\n");
    write(dir, "src/before.rs", "pub fn before() {}\n");
    write(dir, "src/after.rs", "pub fn after() {}\n");
}

fn module<'a>(v: &'a Value, path: &str) -> &'a Value {
    v["modules"]
        .as_array()
        .expect("modules")
        .iter()
        .find(|m| m["path"] == path)
        .unwrap_or_else(|| panic!("{path} está no mapa"))
}

/// Os trechos de teste que o mapa guarda do arquivo `path`.
fn test_lines(v: &Value, path: &str) -> Vec<(u64, u64)> {
    module(v, path)["test_lines"]
        .as_array()
        .map(|ranges| ranges.iter().map(|r| (r[0].as_u64().unwrap(), r[1].as_u64().unwrap())).collect())
        .unwrap_or_default()
}

/// O arquivo `path` é todo de teste, pelo trecho que o mapa guarda dele.
fn is_whole_test(v: &Value, path: &str) -> bool {
    test_lines(v, path).contains(&(1, WHOLE_FILE_END))
}

/// Quem usa a declaração `name` do arquivo `path`, como o mapa grava.
fn used_by(v: &Value, path: &str, name: &str) -> Vec<String> {
    let decl = module(v, path)["declarations"]
        .as_array()
        .expect("declarations")
        .iter()
        .find(|d| d["name"] == name)
        .unwrap_or_else(|| panic!("{name} está em {path}"));
    decl["used_by"].as_array().map(|a| a.iter().map(|s| s.as_str().unwrap().to_string()).collect()).unwrap_or_default()
}

/// O arquivo que o módulo marcado como teste nomeia, e o que mora na pasta
/// dele, são todo de teste; o do módulo com `all(test, ..)` e outro atributo
/// no meio também.
#[test]
fn the_file_a_module_declares_as_test_is_a_test_block_from_its_first_line() {
    let temp = project_dir("declared-test");
    project(temp.path());
    let (v, _) = scan(temp.path());
    for path in ["src/helpers.rs", "src/helpers/inner.rs", "src/combined.rs"] {
        assert!(is_whole_test(&v, path), "{path}: {:?}", test_lines(&v, path));
    }
}

/// O código do projeto, o módulo sem marca, o de marca negada e o que leva
/// `#[path]` (antes ou depois da marca, e com o arquivo que o nome dele
/// teria, mas que o atributo não nomeia) seguem sem trecho de teste.
#[test]
fn a_module_that_is_not_declared_as_test_keeps_its_file_a_program_file() {
    let temp = project_dir("not-declared-test");
    project(temp.path());
    let (v, _) = scan(temp.path());
    for path in ["src/real.rs", "src/plain.rs", "src/gated.rs", "src/before.rs", "src/after.rs", "src/lib.rs"] {
        assert!(!is_whole_test(&v, path), "{path}: {:?}", test_lines(&v, path));
    }
}

/// A chamada escrita no arquivo todo de teste não é uso de `limit`; sem o
/// módulo marcado, ela é.
#[test]
fn a_call_written_in_a_declared_test_file_is_not_a_use_of_the_function() {
    let temp = project_dir("declared-test-calls");
    project(temp.path());
    let (v, _) = scan(temp.path());
    assert_eq!(used_by(&v, "src/real.rs", "limit"), Vec::<String>::new());

    write(temp.path(), "src/lib.rs", &LIB.replace("#[cfg(test)]\nmod helpers;", "mod helpers;"));
    let (v, _) = scan(temp.path());
    assert_eq!(
        used_by(&v, "src/real.rs", "limit"),
        vec!["src/helpers.rs:6:searched".to_string(), "src/helpers/inner.rs:2:deep".to_string()]
    );
}

/// A passada que lê só o que mudou sabe o mesmo que a que lê tudo: o arquivo
/// que o pai passa a declarar como teste, sem ser relido, ganha o trecho; o
/// que deixa de ser declarado, perde.
#[test]
fn a_pass_that_keeps_the_file_marks_it_by_what_its_parent_declares_now() {
    let temp = project_dir("declared-test-reuse");
    let dir = temp.path();
    git(dir, &["init", "-q"]);
    let exclude = mustard_core::footprint_rules().join("\n") + "\n";
    std::fs::write(dir.join(".git").join("info").join("exclude"), exclude).unwrap();
    project(dir);
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "primeiro"]);
    let (first, report) = scan(dir);
    assert_eq!(report["full"], Value::Bool(true), "{report}");
    assert!(is_whole_test(&first, "src/helpers.rs"));

    write(dir, "src/lib.rs", &LIB.replace("#[cfg(test)]\nmod helpers;", "mod helpers;"));
    git(dir, &["commit", "-q", "-am", "segundo"]);
    let (second, report) = scan(dir);
    assert_eq!(report["read"], serde_json::json!(["src/lib.rs"]), "só o pai é relido: {report}");
    for path in ["src/helpers.rs", "src/helpers/inner.rs"] {
        assert!(!is_whole_test(&second, path), "{path}: {:?}", test_lines(&second, path));
    }

    write(dir, "src/lib.rs", LIB);
    git(dir, &["commit", "-q", "-am", "terceiro"]);
    let (third, report) = scan(dir);
    assert_eq!(report["read"], serde_json::json!(["src/lib.rs"]), "só o pai é relido: {report}");
    for path in ["src/helpers.rs", "src/helpers/inner.rs"] {
        assert!(is_whole_test(&third, path), "{path}: {:?}", test_lines(&third, path));
    }
    assert_eq!(test_lines(&third, "src/helpers.rs"), test_lines(&first, "src/helpers.rs"));
}
