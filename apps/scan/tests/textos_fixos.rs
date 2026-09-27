//! Os textos fixos que o mapa guarda: a mensagem de erro, a de log e os
//! outros textos escritos no código, cada um com a linha, a marca e a
//! declaração que o contém. Projetos pequenos em Rust, TypeScript e C#,
//! lidos pelo scan de verdade, e a busca do mapa pela frase que o usuário
//! viu na tela.

#[path = "support/model.rs"]
mod model;

use std::path::Path;
use std::process::Command;

use mustard_core::domain::config::ProjectConfig;
use mustard_core::domain::normalize::Languages;
use mustard_core::io::map_search;
use serde_json::{json, Value};

const RUST: &str = r#"pub fn carregar(id: u32) -> Result<u32, String> {
    tracing::info!("carregando o registro {id}");
    if id == 0 {
        return Err("pedido não encontrado".to_string());
    }
    let rota = "api/registros";
    let sozinha = "palavra";
    Ok(id)
}

#[cfg(test)]
mod tests {
    #[test]
    fn acha() {
        assert_eq!(super::carregar(1), Ok(1), "o registro de teste existe");
    }
}
"#;

const TYPESCRIPT: &str = r#"import { NotFoundException } from '@nestjs/common';

export class ClientesService {
  buscar(id: number) {
    console.log(`buscando o cliente ${id}`);
    if (!id) {
      throw new NotFoundException('cliente não encontrado');
    }
    return 'ok';
  }
}
"#;

const CSHARP: &str = r#"namespace Loja.Estoque;

public class EstoqueService
{
    private readonly ILogger _logger;

    public Item Conferir(int id)
    {
        _logger.LogInformation("Conferindo o estoque {Id}", id);
        if (id == 0) throw new KeyNotFoundException("Item fora do estoque");
        return new Item();
    }
}
"#;

/// Um arquivo de teste inteiro: o texto dele descreve o teste.
const TEST_FILE: &str = r#"describe('busca de clientes', () => {
  it('devolve o cliente pelo número', () => {});
});
"#;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(["-c", "user.email=scan@example.com", "-c", "user.name=scan", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// Um projeto no git com os três arquivos, já no primeiro commit.
fn project() -> tempfile::TempDir {
    let temp = tempfile::Builder::new().prefix("scan-textos-").tempdir().unwrap();
    let dir = temp.path();
    git(dir, &["init", "-q"]);
    let exclude = mustard_core::footprint_rules().join("\n") + "\n";
    std::fs::write(dir.join(".git").join("info").join("exclude"), exclude).unwrap();
    for (rel, body) in [
        ("src/consulta.rs", RUST),
        ("web/clientes.service.ts", TYPESCRIPT),
        ("Loja/Estoque/EstoqueService.cs", CSHARP),
        ("web/clientes.service.test.ts", TEST_FILE),
    ] {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "primeiro"]);
    temp
}

/// Roda o scan sobre o projeto, com o mapa dentro dele, e devolve o mapa e o
/// relato da passada.
fn scan(dir: &Path) -> (Value, Value) {
    model::scan(dir, &dir.join(".claude"), &[])
}

/// Os textos fixos que o mapa guarda para o arquivo `path`.
fn texts(map: &Value, path: &str) -> Value {
    let module = map["modules"].as_array().unwrap().iter().find(|m| m["path"] == json!(path));
    module.unwrap_or_else(|| panic!("{path} no mapa")).get("texts").cloned().unwrap_or(json!([]))
}

#[test]
fn a_thrown_error_message_is_kept_with_the_function_that_throws_it() {
    let temp = project();
    let (map, _) = scan(temp.path());
    for (path, line, value, owner) in [
        ("src/consulta.rs", 4, "pedido não encontrado", "carregar"),
        ("web/clientes.service.ts", 7, "cliente não encontrado", "buscar"),
        ("Loja/Estoque/EstoqueService.cs", 10, "Item fora do estoque", "Conferir"),
    ] {
        let kept = texts(&map, path);
        assert!(
            kept.as_array().unwrap().contains(&json!({"line": line, "kind": "error", "value": value, "owner": owner})),
            "{path}: {kept}"
        );
    }
}

#[test]
fn the_text_of_a_log_call_gets_the_log_mark() {
    let temp = project();
    let (map, _) = scan(temp.path());
    for (path, line, value, owner) in [
        ("src/consulta.rs", 2, "carregando o registro {id}", "carregar"),
        ("web/clientes.service.ts", 5, "buscando o cliente ${id}", "buscar"),
        ("Loja/Estoque/EstoqueService.cs", 9, "Conferindo o estoque {Id}", "Conferir"),
    ] {
        let kept = texts(&map, path);
        assert!(
            kept.as_array().unwrap().contains(&json!({"line": line, "kind": "log", "value": value, "owner": owner})),
            "{path}: {kept}"
        );
    }
}

/// Os valores dos textos fixos que o mapa guarda para o arquivo `path`.
fn values(map: &Value, path: &str) -> Vec<String> {
    texts(map, path).as_array().unwrap().iter().filter_map(|text| text["value"].as_str().map(str::to_string)).collect()
}

#[test]
fn only_a_text_of_two_words_or_with_the_shape_of_a_path_or_key_is_kept() {
    let temp = project();
    let (map, _) = scan(temp.path());
    let rust = values(&map, "src/consulta.rs");
    assert!(rust.contains(&"api/registros".to_string()), "a path is kept: {rust:?}");
    assert!(!rust.contains(&"palavra".to_string()), "one word with no separator is not: {rust:?}");
    let typescript = values(&map, "web/clientes.service.ts");
    assert!(!typescript.contains(&"ok".to_string()), "{typescript:?}");
    assert!(!typescript.iter().any(|value| value.contains("nestjs")), "the import is not a text: {typescript:?}");
    let path = texts(&map, "src/consulta.rs").as_array().unwrap().iter().find(|text| text["value"] == json!("api/registros")).cloned();
    assert_eq!(path.map(|path| path["kind"].clone()), Some(json!("text")), "the rest is marked text");
}

#[test]
fn a_text_written_in_the_test_block_or_in_a_test_file_is_not_kept() {
    let temp = project();
    let (map, _) = scan(temp.path());
    let rust = values(&map, "src/consulta.rs");
    assert!(!rust.iter().any(|value| value.contains("registro de teste")), "{rust:?}");
    assert_eq!(rust.len(), 3, "{rust:?}");
    let test_file = values(&map, "web/clientes.service.test.ts");
    assert!(test_file.is_empty(), "a test file keeps no text: {test_file:?}");
}

#[test]
fn a_pass_that_does_not_read_the_file_again_keeps_the_same_texts() {
    let temp = project();
    let (first, _) = scan(temp.path());
    std::fs::write(temp.path().join("src/outro.rs"), "pub fn outro() {}\n").unwrap();
    git(temp.path(), &["add", "-A"]);
    git(temp.path(), &["commit", "-q", "-m", "segundo"]);
    let (second, report) = scan(temp.path());
    assert_eq!(report["full"], json!(false), "{report}");
    assert_eq!(report["read"], json!(["src/outro.rs"]), "{report}");
    for path in ["src/consulta.rs", "web/clientes.service.ts", "Loja/Estoque/EstoqueService.cs"] {
        assert!(!values(&first, path).is_empty(), "{path}");
        assert_eq!(texts(&second, path), texts(&first, path), "{path}");
    }
}

#[test]
fn the_search_for_the_message_finds_the_file_with_the_text_its_line_and_its_function() {
    let temp = project();
    scan(temp.path());
    let languages = Languages::of(&ProjectConfig::default());
    let map = model::path_in(&temp.path().join(".claude"));
    let found = map_search::search_at(&map, "pedido não encontrado", &languages, 10).expect("a busca lê o mapa");
    let first = found.first().expect("a busca acha o arquivo");
    assert_eq!(first.path, "src/consulta.rs", "{found:?}");
    let text = first.text.as_ref().expect("o texto que casou");
    assert_eq!(
        (text.line, text.kind.as_str(), text.value.as_str(), text.owner.as_str()),
        (4, "error", "pedido não encontrado", "carregar")
    );
}
