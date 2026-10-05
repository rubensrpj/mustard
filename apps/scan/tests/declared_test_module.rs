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

/// O projeto de teste num repositório git com o primeiro commit feito, para a
/// passada que só relê o que mudou.
fn committed_project(name: &str) -> tempfile::TempDir {
    let temp = project_dir(name);
    let dir = temp.path();
    git(dir, &["init", "-q"]);
    let exclude = mustard_core::footprint_rules().join("\n") + "\n";
    std::fs::write(dir.join(".git").join("info").join("exclude"), exclude).unwrap();
    project(dir);
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "primeiro"]);
    temp
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

/// O `src/lib.rs` sem a marca de teste nos módulos `helpers` e `combined`.
fn undeclared(lib: &str) -> String {
    lib.replace("#[cfg(test)]\nmod helpers;", "mod helpers;")
        .replace("#[cfg(all(test, unix))]\n#[allow(dead_code)]\nmod combined;", "mod combined;")
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
/// que deixa de ser declarado, perde. O arquivo que deixa de ser declarado e
/// passa a depender de arquivos do projeto é relido, como todo arquivo que
/// ganha dependência; o que não importa nada (`src/combined.rs`) fica como
/// estava e perde o trecho sem ser relido.
#[test]
fn a_pass_that_keeps_the_file_marks_it_by_what_its_parent_declares_now() {
    let temp = committed_project("declared-test-reuse");
    let dir = temp.path();
    let (first, report) = scan(dir);
    assert_eq!(report["full"], Value::Bool(true), "{report}");
    assert!(is_whole_test(&first, "src/helpers.rs"));

    write(dir, "src/lib.rs", &undeclared(LIB));
    git(dir, &["commit", "-q", "-am", "segundo"]);
    let (second, report) = scan(dir);
    assert_eq!(
        report["read"],
        serde_json::json!(["src/helpers.rs", "src/helpers/inner.rs", "src/lib.rs"]),
        "o pai e os que ganham dependência são relidos; `src/combined.rs` não: {report}"
    );
    for path in ["src/helpers.rs", "src/helpers/inner.rs", "src/combined.rs"] {
        assert!(!is_whole_test(&second, path), "{path}: {:?}", test_lines(&second, path));
    }

    write(dir, "src/lib.rs", LIB);
    git(dir, &["commit", "-q", "-am", "terceiro"]);
    let (third, report) = scan(dir);
    assert_eq!(report["read"], serde_json::json!(["src/lib.rs"]), "só o pai é relido: {report}");
    for path in ["src/helpers.rs", "src/helpers/inner.rs", "src/combined.rs"] {
        assert!(is_whole_test(&third, path), "{path}: {:?}", test_lines(&third, path));
    }
    assert_eq!(test_lines(&third, "src/helpers.rs"), test_lines(&first, "src/helpers.rs"));
}

/// A lista de textos que o mapa guarda em `key` do arquivo `path`.
fn strings(v: &Value, path: &str, key: &str) -> Vec<String> {
    module(v, path)[key].as_array().map(|a| a.iter().map(|s| s.as_str().unwrap().to_string()).collect()).unwrap_or_default()
}

/// O que o arquivo todo de teste importa, pelo `use` ou pelo caminho de uma
/// chamada, não é dependência dele: fica em `test_deps`, e o arquivo que ele
/// importa o lista entre os testes que o cobrem. O arquivo de teste não ganha
/// teste, nem o que o outro importa.
#[test]
fn what_a_declared_test_file_imports_is_covered_by_it_and_not_a_dependency_of_it() {
    let temp = project_dir("declared-test-deps");
    project(temp.path());
    let (v, _) = scan(temp.path());
    for path in ["src/helpers.rs", "src/helpers/inner.rs"] {
        assert_eq!(strings(&v, path, "deps"), Vec::<String>::new(), "{path}: o import do teste não é dependência");
        assert_eq!(strings(&v, path, "tests"), Vec::<String>::new(), "{path}: o teste não ganha teste");
    }
    for path in ["src/helpers.rs", "src/helpers/inner.rs"] {
        assert_eq!(strings(&v, path, "test_deps"), vec!["src/real.rs"], "{path}");
    }
    assert_eq!(strings(&v, "src/real.rs", "tests"), vec!["src/helpers.rs", "src/helpers/inner.rs"]);
    for path in ["src/plain.rs", "src/gated.rs"] {
        assert_eq!(strings(&v, path, "tests"), Vec::<String>::new(), "{path}: nenhum teste o importa");
    }
}

/// Sem o módulo marcado, o mesmo arquivo importa como código: `use` e caminho
/// de chamada são dependência, e ninguém o lista como teste.
#[test]
fn the_same_file_without_the_declaration_keeps_its_imports_as_dependencies() {
    let temp = project_dir("declared-test-deps-off");
    project(temp.path());
    write(temp.path(), "src/lib.rs", &LIB.replace("#[cfg(test)]\nmod helpers;", "mod helpers;"));
    let (v, _) = scan(temp.path());
    for path in ["src/helpers.rs", "src/helpers/inner.rs"] {
        assert_eq!(strings(&v, path, "deps"), vec!["src/real.rs"], "{path}");
        assert_eq!(strings(&v, path, "test_deps"), Vec::<String>::new(), "{path}");
    }
    assert_eq!(strings(&v, "src/real.rs", "tests"), Vec::<String>::new());
}

/// O arquivo guarda os imports como os escreveu, e a passada que não o relê
/// sabe o mesmo que a que lê tudo: o que o pai deixa de declarar como teste
/// volta a ter os imports como dependência e deixa de cobrir o que importa; o
/// que o pai passa a declarar de novo, sem ser relido, fica como estava na
/// primeira passada.
#[test]
fn a_pass_that_keeps_the_file_sorts_its_imports_by_what_its_parent_declares_now() {
    let temp = committed_project("declared-test-deps-reuse");
    let dir = temp.path();
    let (first, report) = scan(dir);
    assert_eq!(report["full"], Value::Bool(true), "{report}");
    for path in ["src/helpers.rs", "src/helpers/inner.rs"] {
        assert_eq!(strings(&first, path, "deps"), Vec::<String>::new(), "{path}");
        assert_eq!(strings(&first, path, "test_deps"), vec!["src/real.rs"], "{path}");
    }
    assert_eq!(strings(&first, "src/real.rs", "tests"), vec!["src/helpers.rs", "src/helpers/inner.rs"]);

    write(dir, "src/lib.rs", &undeclared(LIB));
    git(dir, &["commit", "-q", "-am", "segundo"]);
    let (second, _) = scan(dir);
    for path in ["src/helpers.rs", "src/helpers/inner.rs"] {
        assert_eq!(strings(&second, path, "deps"), vec!["src/real.rs"], "{path}");
        assert_eq!(strings(&second, path, "test_deps"), Vec::<String>::new(), "{path}");
    }
    assert_eq!(strings(&second, "src/real.rs", "tests"), Vec::<String>::new());

    write(dir, "src/lib.rs", LIB);
    git(dir, &["commit", "-q", "-am", "terceiro"]);
    let (third, report) = scan(dir);
    assert_eq!(report["read"], serde_json::json!(["src/lib.rs"]), "só o pai é relido: {report}");
    for path in ["src/helpers.rs", "src/helpers/inner.rs", "src/real.rs"] {
        for key in ["deps", "test_deps", "tests"] {
            assert_eq!(strings(&third, path, key), strings(&first, path, key), "{path} {key}");
        }
    }
}

/// A medida de qualidade que o mapa guarda do arquivo `path`; `None` quando o
/// scan não o mediu.
fn quality<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    module(v, path).get("quality")
}

/// O arquivo que o módulo marcado como teste nomeia, e o que mora na pasta
/// dele, ficam sem medida de qualidade, como o arquivo de teste pelo caminho;
/// o código do projeto segue medido.
#[test]
fn a_declared_test_file_is_left_without_quality_measures() {
    let temp = project_dir("declared-test-quality");
    project(temp.path());
    let (v, _) = scan(temp.path());
    for path in ["src/helpers.rs", "src/helpers/inner.rs", "src/combined.rs"] {
        assert_eq!(quality(&v, path), None, "{path}");
    }
    assert_eq!(quality(&v, "src/real.rs").map(|q| q["size"].clone()), Some(serde_json::json!(3)));
}

/// Sem o módulo marcado, o mesmo arquivo é medido como o código do projeto: as
/// linhas escritas e os imports contam.
#[test]
fn the_same_file_without_the_declaration_is_measured() {
    let temp = project_dir("declared-test-quality-off");
    project(temp.path());
    write(temp.path(), "src/lib.rs", &undeclared(LIB));
    let (v, _) = scan(temp.path());
    for (path, size, imports) in [("src/helpers.rs", 5, 1), ("src/helpers/inner.rs", 3, 1), ("src/combined.rs", 1, 0)] {
        let measured = quality(&v, path).unwrap_or_else(|| panic!("{path} é medido"));
        assert_eq!(measured["size"], serde_json::json!(size), "{path}: {measured}");
        assert_eq!(measured["imports"], serde_json::json!(imports), "{path}: {measured}");
    }
}

/// A passada que lê só o que mudou mede pelo que o pai declara agora: o
/// arquivo que o pai deixa de declarar ganha medida, e o que passa a declarar
/// de novo, sem ser relido, a perde.
#[test]
fn a_pass_that_keeps_the_file_measures_it_by_what_its_parent_declares_now() {
    let temp = committed_project("declared-test-quality-reuse");
    let dir = temp.path();
    let (first, _) = scan(dir);
    for path in ["src/helpers.rs", "src/helpers/inner.rs", "src/combined.rs"] {
        assert_eq!(quality(&first, path), None, "{path}");
    }

    write(dir, "src/lib.rs", &undeclared(LIB));
    git(dir, &["commit", "-q", "-am", "segundo"]);
    let (second, _) = scan(dir);
    for path in ["src/helpers.rs", "src/helpers/inner.rs", "src/combined.rs"] {
        assert!(quality(&second, path).is_some(), "{path} deixa de ser declarado e passa a ser medido");
    }

    write(dir, "src/lib.rs", LIB);
    git(dir, &["commit", "-q", "-am", "terceiro"]);
    let (third, report) = scan(dir);
    assert_eq!(report["read"], serde_json::json!(["src/lib.rs"]), "só o pai é relido: {report}");
    for path in ["src/helpers.rs", "src/helpers/inner.rs", "src/combined.rs"] {
        assert_eq!(quality(&third, path), None, "{path}");
    }
}
