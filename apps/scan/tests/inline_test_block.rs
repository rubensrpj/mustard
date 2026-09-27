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

use std::path::Path;
use std::process::Command;

use serde_json::Value;

/// A pasta do projeto de teste, numa pasta temporária que some quando o valor
/// sai de cena, também quando uma conferência quebra no meio.
fn pasta_do_projeto(nome: &str) -> tempfile::TempDir {
    tempfile::Builder::new().prefix(&format!("scan-{nome}-")).tempdir().unwrap()
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
    let model = dir.join(".claude").join("grain.model.json");
    let out = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["scan", dir.to_str().unwrap(), "--out", model.to_str().unwrap(), "--json"])
        .output()
        .expect("run scan");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let report: Value = serde_json::from_str(stdout.lines().last().unwrap_or("{}")).expect("o relato é uma linha JSON");
    (serde_json::from_str(&std::fs::read_to_string(&model).unwrap()).unwrap(), report)
}

const MAIN: &str = "mod conta;\nmod medida;\nmod regra;\nmod taxa;\n\n\
                    fn main() {\n    println!(\"{}\", conta::total());\n}\n";

/// O corpo importa `taxa` e chama `somar`; o trecho de teste importa `medida`,
/// chama `regra` pelo caminho completo e chama `somar` de novo.
const CONTA: &str = "use crate::taxa::juros;\n\n\
                     pub fn total() -> u32 {\n    somar(juros(), 2)\n}\n\n\
                     pub fn somar(a: u32, b: u32) -> u32 {\n    a + b\n}\n\n\
                     #[cfg(test)]\n\
                     mod tests {\n    use super::*;\n    use crate::medida;\n\n    \
                     #[test]\n    fn soma_os_dois() {\n        let tres = somar(1, 2);\n        \
                     let cem = medida::metro();\n        let dez = crate::regra::limite();\n        \
                     assert_eq!(tres + cem + dez, 113);\n    }\n}\n";

/// Monta o projeto de teste em `dir`.
fn projeto(dir: &Path) {
    write(dir, "src/main.rs", MAIN);
    write(dir, "src/conta.rs", CONTA);
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

fn lista(v: &Value) -> Vec<String> {
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
    lista(&decl["used_by"])
}

/// O `use` escrito dentro do trecho de teste não põe o arquivo importado em
/// `deps`; o `use` do corpo continua pondo.
#[test]
fn a_use_inside_the_test_block_is_not_a_dependency_of_the_file() {
    let temp = pasta_do_projeto("teste-use");
    projeto(temp.path());
    let (v, _) = scan(temp.path());
    let deps = lista(&module(&v, "src/conta.rs")["deps"]);
    assert!(!deps.contains(&"src/medida.rs".to_string()), "o import do teste não é dependência: {deps:?}");
    assert_eq!(deps, vec!["src/taxa.rs".to_string()], "o import do corpo segue sendo");
    assert!(
        lista(&module(&v, "src/conta.rs")["test_imports"]).contains(&"crate::medida".to_string()),
        "o import do teste fica guardado à parte"
    );
}

/// A chamada pelo caminho completo escrita dentro do trecho de teste também
/// não põe o arquivo chamado em `deps`.
#[test]
fn a_full_path_call_inside_the_test_block_is_not_a_dependency_either() {
    let temp = pasta_do_projeto("teste-caminho");
    projeto(temp.path());
    let (v, _) = scan(temp.path());
    let deps = lista(&module(&v, "src/conta.rs")["deps"]);
    assert!(!deps.contains(&"src/regra.rs".to_string()), "a chamada do teste não é dependência: {deps:?}");
    assert_eq!(
        lista(&module(&v, "src/conta.rs")["test_deps"]),
        vec!["src/medida.rs".to_string(), "src/regra.rs".to_string()],
        "os dois imports do teste ligam aos arquivos, guardados à parte"
    );
}

/// A chamada de `somar` dentro do trecho de teste não entra em quem usa
/// `somar`; a do corpo, na linha 4, entra.
#[test]
fn a_call_inside_the_test_block_is_not_a_use_of_the_function() {
    let temp = pasta_do_projeto("teste-chamada");
    projeto(temp.path());
    let (v, _) = scan(temp.path());
    assert_eq!(used_by(&v, "src/conta.rs", "somar"), vec!["src/conta.rs:4:total".to_string()]);
}

/// O arquivo segue trazendo os próprios testes, e os que o trecho de teste
/// importa o listam entre os testes que os cobrem. O `use super::*` do trecho
/// é o próprio arquivo: nem ele nem o arquivo de cima ganham o teste.
#[test]
fn the_file_keeps_its_own_tests_and_what_its_test_block_imports_lists_it() {
    let temp = pasta_do_projeto("teste-cobre");
    projeto(temp.path());
    let (v, _) = scan(temp.path());
    assert_eq!(module(&v, "src/conta.rs")["has_tests"], Value::Bool(true));
    for coberto in ["src/medida.rs", "src/regra.rs"] {
        assert_eq!(lista(&module(&v, coberto)["tests"]), vec!["src/conta.rs".to_string()], "{coberto}");
    }
    for fora in ["src/conta.rs", "src/main.rs", "src/taxa.rs"] {
        assert_eq!(lista(&module(&v, fora)["tests"]), Vec::<String>::new(), "{fora}");
    }
}

/// A passada que reaproveita `src/conta.rs` sem relê-lo sabe o mesmo que a
/// leitura inteira: o teste segue fora de `deps` e de quem usa `somar`, e
/// segue cobrindo o que importa.
#[test]
fn a_pass_that_keeps_the_file_without_reading_it_knows_the_same() {
    let temp = pasta_do_projeto("teste-reuso");
    let dir = temp.path();
    git(dir, &["init", "-q"]);
    let exclude = mustard_core::footprint_rules().join("\n") + "\n";
    std::fs::write(dir.join(".git").join("info").join("exclude"), exclude).unwrap();
    projeto(dir);
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "primeiro"]);
    let (primeiro, relato) = scan(dir);
    assert_eq!(relato["full"], Value::Bool(true), "{relato}");

    write(dir, "src/main.rs", &format!("{MAIN}\npub fn outra() {{}}\n"));
    git(dir, &["commit", "-q", "-am", "segundo"]);
    let (segundo, relato) = scan(dir);
    assert_eq!(relato["read"], serde_json::json!(["src/main.rs"]), "só o que mudou é relido: {relato}");

    for v in [&primeiro, &segundo] {
        assert_eq!(lista(&module(v, "src/conta.rs")["deps"]), vec!["src/taxa.rs".to_string()]);
        assert_eq!(used_by(v, "src/conta.rs", "somar"), vec!["src/conta.rs:4:total".to_string()]);
        assert_eq!(lista(&module(v, "src/regra.rs")["tests"]), vec!["src/conta.rs".to_string()]);
    }
}
