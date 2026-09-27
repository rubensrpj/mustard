//! O texto de dentro de cada peça que o mapa guarda: os comentários do corpo
//! de cada declaração, os nomes escritos no código dela, a documentação de
//! cima inteira e as chamadas feitas nela; de cada arquivo, os comentários do
//! começo e os outros; e o título de cada commit no histórico. Projetos
//! pequenos em Rust, TypeScript e C#, lidos pelo scan de verdade.

#[path = "support/model.rs"]
mod model;

use std::path::Path;
use std::process::Command;

use mustard_core::domain::config::ProjectConfig;
use mustard_core::domain::normalize::Languages;
use mustard_core::domain::project_map::file_history;
use mustard_core::io::{map_search, project_map as store};
use serde_json::{json, Value};

const RUST: &str = r#"//! Pedidos da loja: criar, conferir e cancelar.
//! Cada pedido passa pela conferência antes de gravar.

use std::fmt;

/// Confere o pedido antes de gravar.
pub fn conferir(total: u32) -> bool {
    // o estoque precisa cobrir o total do pedido
    let limite = estoque_disponivel();
    let aviso = "palavra_de_literal";
    total <= limite && !aviso.is_empty()
}

pub fn estoque_disponivel() -> u32 {
    // consulta o armazém central
    10
}
"#;

const CSHARP: &str = r#"// Estoque da loja: reserva e baixa de itens.
using System;

namespace Loja;

public class Estoque
{
    /// <summary>Reserva os itens do pedido.</summary>
    public void Reservar(int quantidade)
    {
        // a reserva segura o item por trinta minutos
        var saldo = Consultar(quantidade);
    }

    public int Consultar(int quantidade) => quantidade;
}
"#;

/// O serviço do carrinho, com a documentação de `aplicarCupom` de
/// [`LONG_DOC_WORDS`] palavras: mais que o teto da documentação curta.
fn typescript() -> String {
    let long_doc = (0..LONG_DOC_WORDS).map(|n| format!("regra{n}")).collect::<Vec<_>>().join(" ");
    format!(
        r#"// Serviço do carrinho: soma os itens e aplica o cupom.

export class CarrinhoService {{
  somar(itens: number[]): number {{
    // cupom de frete grátis entra depois da soma
    const total = itens.reduce((a, b) => a + b, 0);
    return aplicarCupom(total, 'CUPOM_LITERAL');
  }}
}}

/**
 * {long_doc}
 */
export function aplicarCupom(total: number, codigo: string): number {{
  return total;
}}
"#
    )
}

/// As palavras da documentação longa do carrinho.
const LONG_DOC_WORDS: usize = 90;

/// O tamanho, em letras, da documentação que entra inteira.
const WHOLE_DOC_CHARS: usize = 600;

/// Uma documentação de [`WHOLE_DOC_CHARS`] letras: 66 palavras de oito letras
/// e a última, de seis.
fn doc_of_600() -> String {
    let mut doc = (0..66).map(|n| format!("regra{n:03}")).collect::<Vec<_>>().join(" ");
    doc.push_str(" fim123");
    doc
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

/// Grava os arquivos no projeto em `dir`.
fn write(dir: &Path, files: &[(&str, &str)]) {
    for (rel, body) in files {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
}

/// Um projeto no git com os arquivos, já no primeiro commit.
fn project_with(files: &[(&str, &str)]) -> tempfile::TempDir {
    let temp = tempfile::Builder::new().prefix("scan-corpo-").tempdir().unwrap();
    let dir = temp.path();
    git(dir, &["init", "-q"]);
    std::fs::create_dir_all(dir.join(".git").join("info")).unwrap();
    let exclude = mustard_core::footprint_rules().join("\n") + "\n";
    std::fs::write(dir.join(".git").join("info").join("exclude"), exclude).unwrap();
    write(dir, files);
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "primeiro"]);
    temp
}

/// O projeto das três línguas.
fn project() -> tempfile::TempDir {
    let typescript = typescript();
    project_with(&[("src/pedido.rs", RUST), ("Loja/Estoque.cs", CSHARP), ("web/carrinho.service.ts", &typescript)])
}

/// Roda o scan sobre o projeto, com o mapa dentro dele, e devolve o mapa.
fn scan(dir: &Path) -> Value {
    model::scan(dir, &dir.join(".claude"), &[]).0
}

/// O arquivo `path` no mapa.
fn module<'m>(map: &'m Value, path: &str) -> &'m Value {
    map["modules"].as_array().unwrap().iter().find(|m| m["path"] == json!(path)).unwrap_or_else(|| panic!("{path} no mapa"))
}

/// A declaração `name` do arquivo `path` no mapa.
fn decl<'m>(map: &'m Value, path: &str, name: &str) -> &'m Value {
    module(map, path)["declarations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["name"] == json!(name))
        .unwrap_or_else(|| panic!("{name} em {path}"))
}

/// O texto do campo `field` de `value`; vazio sem ele.
fn field<'v>(value: &'v Value, field: &str) -> &'v str {
    value.get(field).and_then(Value::as_str).unwrap_or_default()
}

/// Os termos que o índice de busca do mapa em `dir` guarda na coluna
/// `column` da declaração `name`, em ordem.
fn indexed(dir: &Path, name: &str, column: &str) -> Vec<String> {
    let conn = rusqlite::Connection::open(model::path_in(&dir.join(".claude"))).unwrap();
    let id: i64 = conn.query_row("SELECT rowid FROM decls WHERE name = ?1", [name], |row| row.get(0)).unwrap();
    let mut stmt = conn.prepare("SELECT DISTINCT term FROM decl_vocab WHERE doc = ?1 AND col = ?2 ORDER BY term").unwrap();
    stmt.query_map(rusqlite::params![id, column], |row| row.get(0)).unwrap().collect::<Result<_, _>>().unwrap()
}

#[test]
fn a_comment_inside_a_function_stays_in_it_and_not_in_the_one_beside_it() {
    let temp = project();
    let map = scan(temp.path());
    let conferir = decl(&map, "src/pedido.rs", "conferir");
    assert_eq!(field(conferir, "body_comment"), "o estoque precisa cobrir o total do pedido");
    assert_eq!(field(decl(&map, "src/pedido.rs", "estoque_disponivel"), "body_comment"), "consulta o armazém central");
    let reservar = decl(&map, "Loja/Estoque.cs", "Reservar");
    assert_eq!(field(reservar, "body_comment"), "a reserva segura o item por trinta minutos");
    assert_eq!(field(decl(&map, "Loja/Estoque.cs", "Consultar"), "body_comment"), "");
    assert!(
        field(decl(&map, "Loja/Estoque.cs", "Estoque"), "body_comment").contains("trinta minutos"),
        "the type that holds the method holds its comments too"
    );
    let somar = decl(&map, "web/carrinho.service.ts", "somar");
    assert_eq!(field(somar, "body_comment"), "cupom de frete grátis entra depois da soma");
    assert_eq!(field(decl(&map, "web/carrinho.service.ts", "aplicarCupom"), "body_comment"), "");
}

#[test]
fn a_quoted_text_is_not_among_the_names_of_the_body() {
    let temp = project();
    let map = scan(temp.path());
    assert_eq!(
        field(decl(&map, "src/pedido.rs", "conferir"), "body_names"),
        "conferir total u32 bool limite estoque_disponivel aviso is_empty"
    );
    let somar = field(decl(&map, "web/carrinho.service.ts", "somar"), "body_names");
    let names: Vec<&str> = somar.split(' ').collect();
    for name in ["itens", "reduce", "aplicarCupom", "total"] {
        assert!(names.contains(&name), "{name} in {somar}");
    }
    assert!(!somar.contains("CUPOM_LITERAL"), "{somar}");
    assert!(!somar.contains("cupom"), "a comment is not a name either: {somar}");
    assert_eq!(names.iter().filter(|name| **name == "total").count(), 1, "each name once: {somar}");
}

#[test]
fn a_documentation_of_600_characters_is_kept_whole() {
    let doc = doc_of_600();
    assert_eq!(doc.chars().count(), WHOLE_DOC_CHARS);
    let lines: String = doc.split(' ').collect::<Vec<_>>().chunks(10).map(|words| format!("/// {}\n", words.join(" "))).collect();
    let source = format!("{lines}pub fn regra() {{}}\n\n/// Curta.\npub fn curta() {{}}\n");
    let temp = project_with(&[("src/regra.rs", &source)]);
    let map = scan(temp.path());
    let regra = decl(&map, "src/regra.rs", "regra");
    assert_eq!(field(regra, "whole_doc"), doc);
    let short = field(regra, "doc");
    assert!(short.chars().count() < WHOLE_DOC_CHARS && doc.starts_with(short), "{short}");
    assert!(indexed(temp.path(), "regra", "whole_doc").contains(&"fim123".to_string()), "the last word reaches the index");
    assert!(!indexed(temp.path(), "regra", "doc").contains(&"fim123".to_string()));
    let curta = decl(&map, "src/regra.rs", "curta");
    assert_eq!((field(curta, "doc"), field(curta, "whole_doc")), ("Curta.", ""), "a doc under the ceiling is kept once");
    let whole = indexed(temp.path(), "curta", "whole_doc");
    assert!(whole.contains(&"curta".to_string()), "{whole:?}");
    assert_eq!(whole, indexed(temp.path(), "curta", "doc"), "the index takes the short doc as the whole one");
}

#[test]
fn a_call_on_a_line_of_the_function_reaches_its_calls() {
    let temp = project();
    scan(temp.path());
    let conferir = indexed(temp.path(), "conferir", "body_calls");
    for word in ["estoqu", "disponivel", "empti", "is"] {
        assert!(conferir.contains(&word.to_string()), "{word} in {conferir:?}");
    }
    assert_eq!(indexed(temp.path(), "estoque_disponivel", "body_calls"), Vec::<String>::new());
    let somar = indexed(temp.path(), "somar", "body_calls");
    assert!(somar.contains(&"reduc".to_string()) && somar.contains(&"aplic".to_string()), "{somar:?}");
    assert!(indexed(temp.path(), "Reservar", "body_calls").contains(&"consult".to_string()));
    assert!(!indexed(temp.path(), "Consultar", "body_calls").contains(&"consult".to_string()));
}

#[test]
fn the_comment_at_the_top_of_the_file_is_its_doc_and_not_its_other_comments() {
    let temp = project();
    let map = scan(temp.path());
    let rust = module(&map, "src/pedido.rs");
    assert_eq!(
        field(rust, "file_doc"),
        "Pedidos da loja: criar, conferir e cancelar. Cada pedido passa pela conferência antes de gravar."
    );
    assert_eq!(
        field(rust, "file_comment"),
        "Confere o pedido antes de gravar. o estoque precisa cobrir o total do pedido consulta o armazém central"
    );
    let csharp = module(&map, "Loja/Estoque.cs");
    assert_eq!(field(csharp, "file_doc"), "Estoque da loja: reserva e baixa de itens.");
    assert!(!field(csharp, "file_comment").contains("baixa de itens"), "{}", field(csharp, "file_comment"));
    let typescript = module(&map, "web/carrinho.service.ts");
    assert_eq!(field(typescript, "file_doc"), "Serviço do carrinho: soma os itens e aplica o cupom.");
    assert!(field(typescript, "file_comment").starts_with("cupom de frete grátis"), "{}", field(typescript, "file_comment"));
}

#[test]
fn the_history_answers_the_three_newest_titles_of_the_file() {
    let temp = project_with(&[("src/conta.rs", "pub fn conta() {}\n"), ("src/leitor.rs", "pub fn leitor() {}\n")]);
    let dir = temp.path();
    for (step, title) in ["soma o saldo", "trava o saque", "mostra o extrato"].iter().enumerate() {
        write(dir, &[("src/conta.rs", &format!("pub fn conta() {{}}\n// passo {step}\n"))]);
        git(dir, &["commit", "-q", "-am", title]);
    }
    write(dir, &[("src/leitor.rs", "pub fn leitor() {}\n// outro\n")]);
    git(dir, &["commit", "-q", "-am", "muda o leitor"]);
    scan(dir);
    let map = store::read(dir).expect("o mapa foi gravado");
    let conta = file_history(&map.history, "src/conta.rs").expect("the file is in the history");
    assert_eq!(conta.commits, 4);
    assert_eq!(conta.titles, ["mostra o extrato", "trava o saque", "soma o saldo"]);
    assert_eq!(file_history(&map.history, "src/leitor.rs").unwrap().titles, ["muda o leitor", "primeiro"]);
}

#[test]
fn a_pass_that_does_not_read_the_file_again_keeps_its_texts_and_titles() {
    let temp = project();
    let dir = temp.path();
    let first = scan(dir);
    write(dir, &[("src/outro.rs", "pub fn outro() {}\n")]);
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "segundo"]);
    let (second, report) = model::scan(dir, &dir.join(".claude"), &[]);
    assert_eq!(report["full"], json!(false), "{report}");
    assert_eq!(report["read"], json!(["src/outro.rs"]), "{report}");
    for path in ["src/pedido.rs", "Loja/Estoque.cs", "web/carrinho.service.ts"] {
        assert_eq!(module(&second, path)["declarations"], module(&first, path)["declarations"], "{path}");
        for key in ["file_doc", "file_comment"] {
            assert!(!field(module(&first, path), key).is_empty(), "{path} {key}");
            assert_eq!(module(&second, path)[key], module(&first, path)[key], "{path} {key}");
        }
    }
    let map = store::read(dir).expect("o mapa foi gravado");
    assert_eq!(file_history(&map.history, "src/pedido.rs").unwrap().titles, ["primeiro"]);
    assert_eq!(file_history(&map.history, "src/outro.rs").unwrap().titles, ["segundo"]);
}

#[test]
fn the_unfiltered_search_gives_the_same_list_as_before() {
    let temp = project();
    scan(temp.path());
    let languages = Languages::of(&ProjectConfig::default());
    let map = model::path_in(&temp.path().join(".claude"));
    let got = |query: &str| -> Vec<(String, u64)> {
        let found = map_search::search_at(&map, query, &languages, 10).expect("a busca lê o mapa");
        found.into_iter().map(|f| (f.path, f.score)).collect()
    };
    let list = |pairs: &[(&str, u64)]| -> Vec<(String, u64)> { pairs.iter().map(|(p, s)| (p.to_string(), *s)).collect() };
    assert_eq!(got("conferir pedido"), list(&[("src/pedido.rs", 2391), ("Loja/Estoque.cs", 698)]));
    assert_eq!(got("armazém central"), list(&[]));
    assert_eq!(got("cupom frete"), list(&[("web/carrinho.service.ts", 1281)]));
    assert_eq!(got("reserva trinta minutos"), list(&[("Loja/Estoque.cs", 1643)]));
    assert_eq!(got("regra7 regra8"), list(&[("web/carrinho.service.ts", 1255)]));
}
