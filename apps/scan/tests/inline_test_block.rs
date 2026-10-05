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
//!
//! A função de teste escrita solta no arquivo do programa, fora de módulo de
//! teste, também é trecho de teste; no arquivo que é ele mesmo de teste, a
//! chamada dela segue sendo uso de quem ela chama.

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

/// O arquivo que escreve cada forma de trecho de teste que não é o módulo
/// com `#[cfg(test)]` exato: o módulo com `all(test, ..)`, o com `any(test,
/// ..)`, o `use`, a função e o tipo soltos com `#[cfg(test)]` (o tipo com
/// outro atributo antes da marca e outro depois, a função com `test` depois
/// de outra condição) e o módulo de teste comum com funções de teste dentro.
/// Cada um importa um arquivo diferente.
const TEST_FORMS: &str = "use crate::taxa::juros;\n\
\n\
pub fn total() -> u32 {\n    juros()\n}\n\
\n\
#[cfg(all(test, unix))]\n\
mod with_all {\n    use crate::medida;\n}\n\
\n\
#[cfg(any(test, feature = \"testing\"))]\n\
mod with_any {\n    use crate::regra;\n}\n\
\n\
#[cfg(test)]\n\
use crate::pronta::sample;\n\
\n\
#[cfg(test)]\n\
fn seed_value() -> u32 {\n    sample()\n}\n\
\n\
#[allow(dead_code)]\n\
#[cfg(test)]\n\
#[derive(Debug)]\n\
struct Seed {\n    value: u32,\n}\n\
\n\
#[cfg(all(unix, test))]\n\
fn sums_it() {\n    seed_value();\n}\n\
\n\
#[cfg(test)]\n\
mod tests {\n    #[test]\n    fn one() {}\n\n    #[test]\n    fn two() {}\n}\n";

/// Cada marca de teste que o módulo escrito com outra condição, ou o item
/// solto, usa para valer como trecho de teste, e as que não valem: a que
/// nega o teste (`not(test)`), a que cita `test` só como texto
/// (`feature = "test"`) e a que o põe só dentro de uma negação.
const NOT_TEST_FORMS: &str = "#[cfg(not(test))]\n\
mod prod {\n    use crate::x::y;\n}\n\
\n\
#[cfg(feature = \"test\")]\n\
mod lab {\n    use crate::x::y;\n}\n\
\n\
#[cfg(all(feature = \"test\", unix))]\n\
fn gated() {}\n\
\n\
#[cfg(all(not(test), unix))]\n\
fn other() {}\n\
\n\
#[cfg(any(unix, not(test)))]\n\
fn either() {}\n";

fn forms_project(label: &str, name: &str, body: &str) -> (Value, Value) {
    let temp = project_dir(label);
    write(temp.path(), "Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n");
    let lib = format!("pub mod {name};\npub mod medida;\npub mod pronta;\npub mod regra;\npub mod taxa;\npub mod x;\n");
    write(temp.path(), "src/lib.rs", &lib);
    write(temp.path(), &format!("src/{name}.rs"), body);
    for (rel, fun) in [("medida", "metro"), ("pronta", "sample"), ("regra", "limite"), ("taxa", "juros"), ("x", "y")] {
        write(temp.path(), &format!("src/{rel}.rs"), &format!("pub fn {fun}() -> u32 {{\n    1\n}}\n"));
    }
    scan(temp.path())
}

/// O módulo com `all(test, ..)` ou `any(test, ..)`, o `use`, a função e o
/// tipo soltos com `#[cfg(test)]` são trecho de teste: as linhas deles vão
/// para `test_lines` (a do módulo sem o atributo em cima, a do item solto com
/// a fila de atributos) e o que importam fica nas dependências de teste. O
/// módulo de teste com funções de teste dentro dá um trecho só, o do módulo.
#[test]
fn every_form_of_test_code_is_a_test_block() {
    let (v, _) = forms_project("test-forms", "forms", TEST_FORMS);
    let forms = module(&v, "src/forms.rs");
    let ranges: Vec<(u64, u64)> = forms["test_lines"]
        .as_array()
        .map(|a| a.iter().map(|r| (r[0].as_u64().unwrap(), r[1].as_u64().unwrap())).collect())
        .unwrap_or_default();
    assert_eq!(ranges, vec![(8, 10), (13, 15), (17, 18), (20, 23), (25, 30), (32, 35), (38, 44)], "{forms}");
    assert_eq!(list(&forms["deps"]), vec!["src/taxa.rs".to_string()], "só o import do corpo é dependência");
    assert_eq!(
        list(&forms["test_deps"]),
        vec!["src/medida.rs".to_string(), "src/pronta.rs".to_string(), "src/regra.rs".to_string()],
        "o que o teste importa fica à parte"
    );
    assert_eq!(used_by(&v, "src/forms.rs", "seed_value"), Vec::<String>::new(), "a chamada do teste não é uso");
    // A declaração começa na primeira marca da fila de atributos, e é essa
    // linha que o mapa guarda; ela cai dentro do trecho de teste, e a do corpo
    // não.
    let in_tests = |line: u64| ranges.iter().any(|&(first, last)| (first..=last).contains(&line));
    for decl in forms["declarations"].as_array().unwrap() {
        let (name, line) = (decl["name"].as_str().unwrap(), decl["line"].as_u64().unwrap());
        assert_eq!(in_tests(line), name != "total", "{name} na linha {line}: {forms}");
    }
}

/// A condição que nega o teste, a que cita `test` só como texto e a que o põe
/// só dentro de uma negação não fazem trecho de teste: o código é do
/// programa, e o import dele é dependência.
#[test]
fn a_condition_that_does_not_mean_test_is_not_a_test_block() {
    let (v, _) = forms_project("test-forms-not", "prod", NOT_TEST_FORMS);
    let prod = module(&v, "src/prod.rs");
    assert_eq!(prod.get("test_lines"), None, "{prod}");
    assert_eq!(list(&prod["deps"]), vec!["src/x.rs".to_string()], "{prod}");
    assert_eq!(list(&prod["test_deps"]), Vec::<String>::new(), "{prod}");
}

/// O arquivo do programa com `total` chamando `somar` (linha 4), duas funções
/// de teste soltas, fora de módulo de teste (a primeira com um comentário em
/// cima e outro atributo depois da marca, linhas 12 a 18; a segunda, linhas
/// 20 a 24), e `doubled`, que só tem outro atributo e chama `somar` na linha
/// 28. Cada teste chama `somar`, e cada um chama um arquivo diferente pelo
/// caminho completo.
const LOOSE_TESTS: &str = "use crate::taxa::juros;\n\n\
                           pub fn total() -> u32 {\n    somar(juros(), 2)\n}\n\n\
                           pub fn somar(a: u32, b: u32) -> u32 {\n    a + b\n}\n\n\
                           /// Soma dois.\n#[test]\n#[should_panic]\n\
                           fn adds_the_two() {\n    let tres = somar(1, 2);\n    \
                           let dez = crate::regra::limite();\n    assert_eq!(tres + dez, 13);\n}\n\n\
                           #[test]\nfn adds_again() {\n    let cem = crate::medida::metro();\n    \
                           assert_eq!(somar(cem, 1), 101);\n}\n\n\
                           #[inline]\npub fn doubled(a: u32) -> u32 {\n    somar(a, a)\n}\n";

/// O arquivo de teste (`src/conta_test.rs`), que escreve a mesma função de
/// teste solta e chama `somar` na linha 5.
const TEST_FILE: &str = "use crate::conta::somar;\n\n\
                         #[test]\nfn adds_in_the_test_file() {\n    assert_eq!(somar(1, 2), 3);\n}\n";

/// Monta o projeto das funções de teste soltas e devolve o mapa dele.
fn loose_tests_map(label: &str) -> Value {
    let temp = project_dir(label);
    write(temp.path(), "Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n");
    write(temp.path(), "src/lib.rs", "pub mod conta;\npub mod conta_test;\npub mod medida;\npub mod regra;\npub mod taxa;\n");
    write(temp.path(), "src/conta.rs", LOOSE_TESTS);
    write(temp.path(), "src/conta_test.rs", TEST_FILE);
    for (rel, fun) in [("medida", "metro"), ("regra", "limite"), ("taxa", "juros")] {
        write(temp.path(), &format!("src/{rel}.rs"), &format!("pub fn {fun}() -> u32 {{\n    1\n}}\n"));
    }
    scan(temp.path()).0
}

/// As linhas dos trechos de teste de `path`, como o mapa grava.
fn test_lines(v: &Value, path: &str) -> Vec<(u64, u64)> {
    module(v, path)["test_lines"]
        .as_array()
        .map(|a| a.iter().map(|r| (r[0].as_u64().unwrap(), r[1].as_u64().unwrap())).collect())
        .unwrap_or_default()
}

/// Quem usa `somar`, em ordem, sem a ordem em que o mapa a grava.
fn somar_used_by(v: &Value) -> Vec<String> {
    let mut sites = used_by(v, "src/conta.rs", "somar");
    sites.sort();
    sites
}

/// A função com `#[test]` solta no arquivo do programa é trecho de teste, com
/// o comentário de cima fora e os atributos colados dentro: as duas ganham as
/// linhas delas, a declaração de cada uma cai dentro do trecho e as do
/// programa (`total`, `somar`, `doubled`) ficam de fora, mesmo `doubled`, que
/// só tem outro atributo.
#[test]
fn a_loose_test_function_in_a_program_file_is_a_test_block() {
    let v = loose_tests_map("loose-lines");
    let ranges = test_lines(&v, "src/conta.rs");
    assert_eq!(ranges, vec![(12, 18), (20, 24)], "{}", module(&v, "src/conta.rs"));
    let in_tests = |line: u64| ranges.iter().any(|&(first, last)| (first..=last).contains(&line));
    for decl in module(&v, "src/conta.rs")["declarations"].as_array().unwrap() {
        let (name, line) = (decl["name"].as_str().unwrap(), decl["line"].as_u64().unwrap());
        assert_eq!(in_tests(line), name.starts_with("adds"), "{name} na linha {line}");
    }
}

/// A chamada escrita na função de teste solta não é uso de `somar`: só as do
/// corpo (`total`, na linha 4, e `doubled`, na 28) e a do arquivo de teste
/// ficam.
#[test]
fn a_call_inside_a_loose_test_function_is_not_a_use_of_the_function() {
    let v = loose_tests_map("loose-calls");
    assert_eq!(
        somar_used_by(&v),
        vec![
            "src/conta.rs:28:doubled".to_string(),
            "src/conta.rs:4:total".to_string(),
            "src/conta_test.rs:5:adds_in_the_test_file".to_string(),
        ]
    );
}

/// O caminho completo escrito na função de teste solta não é dependência do
/// arquivo, e o arquivo que ele nomeia o lista entre os testes que o cobrem.
#[test]
fn a_path_written_in_a_loose_test_function_is_a_test_dependency() {
    let v = loose_tests_map("loose-deps");
    assert_eq!(list(&module(&v, "src/conta.rs")["deps"]), vec!["src/taxa.rs".to_string()]);
    assert_eq!(
        list(&module(&v, "src/conta.rs")["test_deps"]),
        vec!["src/medida.rs".to_string(), "src/regra.rs".to_string()]
    );
    for covered in ["src/medida.rs", "src/regra.rs"] {
        assert_eq!(list(&module(&v, covered)["tests"]), vec!["src/conta.rs".to_string()], "{covered}");
    }
}

/// No arquivo que é ele mesmo de teste, a função com `#[test]` não é trecho
/// de teste do arquivo: nenhuma linha dele vai para os trechos, e a chamada
/// dela segue em quem usa `somar`, que é o que mostra quem a testa.
#[test]
fn a_loose_test_function_in_a_test_file_keeps_its_call_among_the_uses() {
    let v = loose_tests_map("loose-test-file");
    assert_eq!(module(&v, "src/conta_test.rs").get("test_lines"), None, "{}", module(&v, "src/conta_test.rs"));
    assert!(
        somar_used_by(&v).contains(&"src/conta_test.rs:5:adds_in_the_test_file".to_string()),
        "{:?}",
        somar_used_by(&v)
    );
}
