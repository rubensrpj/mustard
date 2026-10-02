//! O teste escrito dentro do próprio arquivo (`#[cfg(test)] mod tests`) não é
//! uso do código do arquivo. O que ele importa, pelo `use` ou pelo caminho de
//! uma chamada, não vira dependência do arquivo; o que ele chama não entra em
//! quem usa a função; e o arquivo que ele importa passa a listá-lo entre os
//! testes que o cobrem. O arquivo segue dizendo que traz os próprios testes.
//!
//! O projeto de teste tem um arquivo, `src/conta.rs`, com um import e uma
//! chamada no corpo, e um trecho de teste que importa `src/medida.rs` pelo
//! `use`, chama `src/regra.rs` pelo caminho completo e chama `somar`, a mesma
//! função que o corpo chama. A leitura que reaproveita o arquivo sem relê-lo
//! sabe o mesmo que a leitura inteira.

#[path = "support/model.rs"]
mod model;

use std::path::Path;
use std::process::Command;

use serde_json::Value;

/// A pasta do projeto de teste, numa pasta temporária que some quando o valor
/// sai de cena, também quando uma conferência quebra no meio.
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

/// Roda o scan e devolve o mapa gravado e o relato da passada.
fn scan(dir: &Path) -> (Value, Value) {
    model::scan(dir, &dir.join(".claude"), &[])
}

const MAIN: &str = "mod conta;\nmod medida;\nmod regra;\nmod taxa;\n\n\
                    fn main() {\n    println!(\"{}\", conta::total());\n}\n";

/// O corpo importa `taxa` e chama `somar`; o trecho de teste importa `medida`,
/// chama `regra` pelo caminho completo e chama `somar` de novo.
const ACCOUNT: &str = "use crate::taxa::juros;\n\n\
                     pub fn total() -> u32 {\n    somar(juros(), 2)\n}\n\n\
                     pub fn somar(a: u32, b: u32) -> u32 {\n    a + b\n}\n\n\
                     #[cfg(test)]\n\
                     mod tests {\n    use super::*;\n    use crate::medida;\n\n    \
                     #[test]\n    fn adds_the_two() {\n        let tres = somar(1, 2);\n        \
                     let cem = medida::metro();\n        let dez = crate::regra::limite();\n        \
                     assert_eq!(tres + cem + dez, 113);\n    }\n}\n";

/// Monta o projeto de teste em `dir`.
fn project(dir: &Path) {
    write(dir, "src/main.rs", MAIN);
    write(dir, "src/conta.rs", ACCOUNT);
    write(dir, "src/medida.rs", "pub fn metro() -> u32 {\n    100\n}\n");
    write(dir, "src/regra.rs", "pub fn limite() -> u32 {\n    10\n}\n");
    write(dir, "src/taxa.rs", "pub fn juros() -> u32 {\n    1\n}\n");
}

fn module<'a>(v: &'a Value, path: &str) -> &'a Value {
    v["modules"]
        .as_array()
        .expect("modules")
        .iter()
        .find(|m| m["path"] == path)
        .unwrap_or_else(|| panic!("{path} está no mapa"))
}

fn list(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().map(|s| s.as_str().unwrap().to_string()).collect()).unwrap_or_default()
}

/// Quem usa a declaração `name` do arquivo `path`, como o mapa grava.
fn used_by(v: &Value, path: &str, name: &str) -> Vec<String> {
    let decl = module(v, path)["declarations"]
        .as_array()
        .expect("declarations")
        .iter()
        .find(|d| d["name"] == name)
        .unwrap_or_else(|| panic!("{name} está em {path}"));
    list(&decl["used_by"])
}

/// O `use` escrito dentro do trecho de teste não põe o arquivo importado em
/// `deps`; o `use` do corpo continua pondo.
#[test]
fn a_use_inside_the_test_block_is_not_a_dependency_of_the_file() {
    let temp = project_dir("test-use");
    project(temp.path());
    let (v, _) = scan(temp.path());
    let deps = list(&module(&v, "src/conta.rs")["deps"]);
    assert!(!deps.contains(&"src/medida.rs".to_string()), "o import do teste não é dependência: {deps:?}");
    assert_eq!(deps, vec!["src/taxa.rs".to_string()], "o import do corpo segue sendo");
    assert!(
        list(&module(&v, "src/conta.rs")["test_imports"]).contains(&"crate::medida".to_string()),
        "o import do teste fica guardado à parte"
    );
}

/// A chamada pelo caminho completo escrita dentro do trecho de teste também
/// não põe o arquivo chamado em `deps`.
#[test]
fn a_full_path_call_inside_the_test_block_is_not_a_dependency_either() {
    let temp = project_dir("test-path");
    project(temp.path());
    let (v, _) = scan(temp.path());
    let deps = list(&module(&v, "src/conta.rs")["deps"]);
    assert!(!deps.contains(&"src/regra.rs".to_string()), "a chamada do teste não é dependência: {deps:?}");
    assert_eq!(
        list(&module(&v, "src/conta.rs")["test_deps"]),
        vec!["src/medida.rs".to_string(), "src/regra.rs".to_string()],
        "os dois imports do teste ligam aos arquivos, guardados à parte"
    );
}

/// A chamada de `somar` dentro do trecho de teste não entra em quem usa
/// `somar`; a do corpo, na linha 4, entra.
#[test]
fn a_call_inside_the_test_block_is_not_a_use_of_the_function() {
    let temp = project_dir("test-call");
    project(temp.path());
    let (v, _) = scan(temp.path());
    assert_eq!(used_by(&v, "src/conta.rs", "somar"), vec!["src/conta.rs:4:total".to_string()]);
}

/// O arquivo segue trazendo os próprios testes, e os que o trecho de teste
/// importa o listam entre os testes que os cobrem. O `use super::*` do trecho
/// é o próprio arquivo: nem ele nem o arquivo de cima ganham o teste.
#[test]
fn the_file_keeps_its_own_tests_and_what_its_test_block_imports_lists_it() {
    let temp = project_dir("test-covers");
    project(temp.path());
    let (v, _) = scan(temp.path());
    assert_eq!(module(&v, "src/conta.rs")["has_tests"], Value::Bool(true));
    for covered in ["src/medida.rs", "src/regra.rs"] {
        assert_eq!(list(&module(&v, covered)["tests"]), vec!["src/conta.rs".to_string()], "{covered}");
    }
    for outside in ["src/conta.rs", "src/main.rs", "src/taxa.rs"] {
        assert_eq!(list(&module(&v, outside)["tests"]), Vec::<String>::new(), "{outside}");
    }
}

/// A passada que reaproveita `src/conta.rs` sem relê-lo sabe o mesmo que a
/// leitura inteira: o teste segue fora de `deps` e de quem usa `somar`, e
/// segue cobrindo o que importa.
#[test]
fn a_pass_that_keeps_the_file_without_reading_it_knows_the_same() {
    let temp = project_dir("test-reuse");
    let dir = temp.path();
    git(dir, &["init", "-q"]);
    let exclude = mustard_core::footprint_rules().join("\n") + "\n";
    std::fs::write(dir.join(".git").join("info").join("exclude"), exclude).unwrap();
    project(dir);
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "primeiro"]);
    let (first, report) = scan(dir);
    assert_eq!(report["full"], Value::Bool(true), "{report}");

    write(dir, "src/main.rs", &format!("{MAIN}\npub fn outra() {{}}\n"));
    git(dir, &["commit", "-q", "-am", "segundo"]);
    let (second, report) = scan(dir);
    assert_eq!(report["read"], serde_json::json!(["src/main.rs"]), "só o que mudou é relido: {report}");

    for v in [&first, &second] {
        assert_eq!(list(&module(v, "src/conta.rs")["deps"]), vec!["src/taxa.rs".to_string()]);
        assert_eq!(used_by(v, "src/conta.rs", "somar"), vec!["src/conta.rs:4:total".to_string()]);
        assert_eq!(list(&module(v, "src/regra.rs")["tests"]), vec!["src/conta.rs".to_string()]);
    }
}

/// O trecho de teste com outros atributos entre o `#[cfg(test)]` e o `mod`
/// (`src/lib.rs`), o do `#[cfg(test)]` colado ao `mod` (`src/colado.rs`) e o
/// de um `#[cfg(test)]` separado do `mod` por um item que não é atributo
/// (`src/cortado.rs`): os três importam `src/x.rs` de dentro do `mod tests`.
const ATTRIBUTE_QUEUE: &[(&str, &str)] = &[
    ("Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n"),
    ("src/x.rs", "pub fn y() {}\n"),
    (
        "src/lib.rs",
        "pub mod colado;\npub mod cortado;\npub mod x;\n\n\
         #[cfg(test)]\n#[allow(dead_code)]\nmod tests {\n    use crate::x::y;\n}\n",
    ),
    ("src/colado.rs", "#[cfg(test)]\nmod tests {\n    use crate::x::y;\n}\n"),
    (
        "src/cortado.rs",
        "#[cfg(test)]\nconst LIMITE: u8 = 0;\n#[allow(dead_code)]\nmod tests {\n    use crate::x::y;\n}\n",
    ),
];

/// Monta o projeto da fila de atributos e devolve, de `path`, as dependências
/// do código e as do teste, lado a lado.
fn deps_and_test_deps(label: &str, path: &str) -> (Vec<String>, Vec<String>, Value) {
    let temp = project_dir(label);
    for (rel, body) in ATTRIBUTE_QUEUE {
        write(temp.path(), rel, body);
    }
    let (v, _) = scan(temp.path());
    let deps = list(&module(&v, path)["deps"]);
    let test_deps = list(&module(&v, path)["test_deps"]);
    (deps, test_deps, v)
}

/// Com `#[allow(dead_code)]` entre o `#[cfg(test)]` e o `mod tests`, o
/// `use crate::x::y` do trecho põe `src/x.rs` só nas dependências de teste de
/// `src/lib.rs`, e `src/x.rs` lista `src/lib.rs` entre os testes que o cobrem.
#[test]
fn another_attribute_between_the_test_marker_and_the_module_keeps_the_block_a_test() {
    let (deps, test_deps, v) = deps_and_test_deps("queue-middle", "src/lib.rs");
    assert_eq!((deps, test_deps), (Vec::<String>::new(), vec!["src/x.rs".to_string()]));
    assert!(list(&module(&v, "src/x.rs")["tests"]).contains(&"src/lib.rs".to_string()));
}

/// O `#[cfg(test)]` colado ao `mod tests` segue marcando o trecho como teste.
#[test]
fn the_test_marker_glued_to_the_module_still_marks_the_block() {
    let (deps, test_deps, _) = deps_and_test_deps("queue-glued", "src/colado.rs");
    assert_eq!((deps, test_deps), (Vec::<String>::new(), vec!["src/x.rs".to_string()]));
}

/// Um item que não é atributo entre o `#[cfg(test)]` e o `mod` corta a fila:
/// a marca é do item, o `mod` não é teste, e o import dele é dependência do
/// código, mesmo com outro atributo colado ao `mod`.
#[test]
fn an_item_that_is_not_an_attribute_between_them_cuts_the_queue() {
    let (deps, test_deps, _) = deps_and_test_deps("queue-cut", "src/cortado.rs");
    assert_eq!((deps, test_deps), (vec!["src/x.rs".to_string()], Vec::<String>::new()));
}
