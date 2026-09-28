//! As medidas de qualidade de cada arquivo, como o scan as grava no mapa: as
//! linhas não vazias do arquivo e de cada função, sem o trecho de teste
//! escrito dentro dele; as importações; as linhas que caem numa janela de dez
//! linhas seguidas igual à de outro arquivo; e a participação num ciclo de
//! importações. O arquivo de dado de teste fica sem medida e não faz outro
//! arquivo parecer repetido.
//!
//! O projeto de teste tem dois arquivos que se importam um ao outro e trazem
//! o mesmo trecho de dez linhas, um terceiro com só nove delas, um arquivo
//! com um trecho de teste dentro e um arquivo de dado de teste com o mesmo
//! trecho de dez linhas.

#[path = "support/model.rs"]
mod model;

use std::path::Path;
use std::process::Command;

use serde_json::{json, Value};

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

/// O trecho de dez linhas não vazias que dois arquivos repetem.
const SHARED: [&str; 10] = [
    "    let base = valor * 3;",
    "    let taxa = base / 7;",
    "    let juros = taxa + 11;",
    "    let multa = juros * 2;",
    "    let desconto = multa - 5;",
    "    let bruto = desconto + base;",
    "    let liquido = bruto - taxa;",
    "    let arredondado = liquido / 10 * 10;",
    "    let final_ = arredondado + 1;",
    "    let fim = final_ - valor;",
];

fn shared(lines: usize) -> String {
    SHARED.iter().take(lines).map(|line| format!("{line}\n")).collect()
}

/// A pasta do projeto: `a.rs` e `b.rs` se importam e repetem o trecho
/// inteiro, `c.rs` tem só nove linhas dele, `conta.rs` traz um trecho de
/// teste, e o dado de teste em `__mocks__` repete o trecho inteiro.
fn project() -> tempfile::TempDir {
    let dir = tempfile::Builder::new().prefix("scan-qualidade-").tempdir().unwrap();
    let root = dir.path();
    write(root, "src/main.rs", "mod a;\nmod b;\nmod c;\nmod conta;\n\nfn main() {\n    println!(\"{}\", a::calcular(1));\n}\n");
    write(
        root,
        "src/a.rs",
        &format!("use crate::b::medir;\n\npub fn calcular(valor: u32) -> u32 {{\n    let inicio = medir(valor);\n{}    fim + inicio\n}}\n", shared(10)),
    );
    write(
        root,
        "src/b.rs",
        &format!("use crate::a::calcular;\n\npub fn medir(valor: u32) -> u32 {{\n{}    fim\n}}\n\npub fn outro() -> u32 {{\n    calcular(2)\n}}\n", shared(10)),
    );
    write(root, "src/c.rs", &format!("pub fn quase(valor: u32) -> u32 {{\n{}    valor\n}}\n", shared(9)));
    write(
        root,
        "src/conta.rs",
        "pub fn total() -> u32 {\n    let um = 1;\n\n    um + 1\n}\n\n\
         #[cfg(test)]\n\
         mod tests {\n    use super::*;\n\n    #[test]\n    fn soma() {\n        assert_eq!(total(), 2);\n    }\n}\n",
    );
    write(root, "src/__mocks__/dados.rs", &format!("pub fn falso(valor: u32) -> u32 {{\n{}    fim\n}}\n", shared(10)));
    git(root, &["init", "-q"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "primeiro"]);
    dir
}

fn module<'a>(map: &'a Value, path: &str) -> &'a Value {
    map["modules"].as_array().unwrap().iter().find(|m| m["path"] == json!(path)).unwrap_or_else(|| panic!("{path} in the map"))
}

#[test]
fn two_files_with_the_same_ten_line_snippet_are_marked_repeated_and_nine_lines_are_not() {
    let dir = project();
    let (map, _) = model::scan(dir.path(), &dir.path().join(".claude"), &[]);
    assert_eq!(module(&map, "src/a.rs")["quality"]["repeated"], json!(10), "{}", module(&map, "src/a.rs"));
    assert_eq!(module(&map, "src/b.rs")["quality"]["repeated"], json!(10), "{}", module(&map, "src/b.rs"));
    assert_eq!(module(&map, "src/c.rs")["quality"].get("repeated"), None, "nine equal lines are no window: {}", module(&map, "src/c.rs"));
    // O dado de teste fica sem medida.
    assert_eq!(module(&map, "src/__mocks__/dados.rs").get("quality"), None);
}

#[test]
fn a_test_block_inside_the_file_does_not_count_in_the_size() {
    let dir = project();
    let (map, _) = model::scan(dir.path(), &dir.path().join(".claude"), &[]);
    let conta = module(&map, "src/conta.rs");
    assert_eq!(conta["loc"], json!(12), "{conta}");
    // As quatro linhas escritas do corpo e a do atributo logo acima do
    // trecho de teste, que o scan deixa fora dele; nenhuma das sete de dentro.
    assert_eq!(conta["test_lines"], json!([[8, 15]]), "{conta}");
    assert_eq!(conta["quality"]["size"], json!(5), "{conta}");
    // A função do trecho de teste não é medida; a do corpo tem quatro linhas.
    assert_eq!(conta["quality"]["functions"], json!([[1, 4]]), "{conta}");
}

#[test]
fn the_imports_and_the_cycle_are_measured_from_the_resolved_graph() {
    let dir = project();
    let (map, _) = model::scan(dir.path(), &dir.path().join(".claude"), &[]);
    for path in ["src/a.rs", "src/b.rs"] {
        let quality = &module(&map, path)["quality"];
        assert_eq!(quality["cycle"], json!(true), "{path}: {quality}");
        assert_eq!(quality["imports"], json!(1), "{path}: {quality}");
    }
    assert_eq!(module(&map, "src/c.rs")["quality"].get("cycle"), None);
}

#[test]
fn a_pass_that_reads_only_what_changed_measures_the_same() {
    let dir = project();
    let out = dir.path().join(".claude");
    let (first, _) = model::scan(dir.path(), &out, &[]);
    write(dir.path(), "src/c.rs", &format!("pub fn quase(valor: u32) -> u32 {{\n{}    valor\n}}\n", shared(8)));
    git(dir.path(), &["commit", "-qam", "segundo"]);
    let (second, report) = model::scan(dir.path(), &out, &[]);
    assert_eq!(report["full"], json!(false), "{report}");
    // Os arquivos que a passada não releu seguem medidos, a repetição
    // inclusive, que depende dos outros arquivos.
    assert_eq!(module(&second, "src/a.rs")["quality"]["repeated"], json!(10), "{}", module(&second, "src/a.rs"));
    for path in ["src/a.rs", "src/b.rs", "src/conta.rs"] {
        assert_eq!(module(&second, path)["quality"], module(&first, path)["quality"], "{path}");
    }
}
