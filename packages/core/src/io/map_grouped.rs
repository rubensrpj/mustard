//! `map_grouped` — a consulta agrupada das declarações: uma consulta só ao
//! índice com todas as palavras do pedido em OR, ordenada pela nota `bm25()`
//! do próprio FTS5.
//!
//! Cada palavra entra pela raiz e, na de quatro letras ou mais, também pelo
//! começo dela (`apag*` acha apagar, apagando e apagado); o índice guarda os
//! começos de três e de quatro letras (`prefix='3 4'`), e a consulta os lê
//! sem varrer o vocabulário. As colunas pesam pelo que a pergunta quase
//! copia: o nome mais que o caminho, a assinatura e a documentação; a
//! mensagem de log, a de erro e o texto fixo depois deles; o resto pesa 1.
//! A lista traz as primeiras [`DEPTH`] declarações; a ordem única dos
//! arquivos ([`crate::io::map_order`]) as soma às outras por posição
//! recíproca.
//!
//! O pedaço no meio da palavra (a tabela trigram com `%` dos dois lados)
//! não entra: nas três réguas de prova ele piorou o arquivo certo em
//! primeiro.

use rusqlite::Connection;

use crate::domain::normalize::{Languages, Normalizer};
use crate::io::map_words::question;
use crate::platform::error::Result;

/// Quantas declarações a consulta devolve.
pub(super) const DEPTH: usize = 100;

/// A palavra de pelo menos este tamanho também é procurada pelo começo.
const PREFIX_FROM: usize = 4;

/// O peso de cada coluna de `decl_fts`, na ordem do esquema: nome 4;
/// caminho, assinatura e documentação 2; log, erro e texto 1,5; o resto 1
/// (a lista é mais curta que as colunas, e o FTS5 dá 1 às que faltam).
const COLUMN_WEIGHTS: [f64; 7] = [4.0, 2.0, 2.0, 2.0, 1.5, 1.5, 1.5];

/// Se a consulta lê o começo das palavras. Só nos testes uma medida desliga
/// o começo, com `MAP_GROUPED_PREFIX=0`, para comparar a consulta com e sem
/// ele.
fn word_start() -> bool {
    #[cfg(test)]
    if std::env::var("MAP_GROUPED_PREFIX").is_ok_and(|value| value == "0") {
        return false;
    }
    true
}

/// A expressão `MATCH` das palavras de `text`: as raízes e, nas palavras de
/// quatro letras ou mais, o começo delas; vazia quando a pergunta não tem
/// palavra que o índice leia.
pub(super) fn expression(conn: &Connection, text: &str, languages: &Languages) -> Result<String> {
    let mut normalizer = Normalizer::new(languages);
    let mut terms: Vec<String> = Vec::new();
    for word in question(conn, &mut normalizer, text)? {
        let prefixed = word_start() && word.plain.chars().count() >= PREFIX_FROM;
        for form in word.indexed.iter().filter(|form| !form.is_empty()) {
            let form = form.replace('"', "");
            let term = if prefixed { format!("\"{form}\"*") } else { format!("\"{form}\"") };
            if !terms.contains(&term) {
                terms.push(term);
            }
        }
    }
    Ok(terms.join(" OR "))
}

/// As primeiras [`DEPTH`] declarações do nível das declarações para as
/// palavras de `text`, da melhor nota para a pior.
pub(super) fn list(conn: &Connection, text: &str, languages: &Languages) -> Result<Vec<i64>> {
    let expression = expression(conn, text, languages)?;
    if expression.is_empty() {
        return Ok(Vec::new());
    }
    let weights: Vec<String> = COLUMN_WEIGHTS.iter().map(f64::to_string).collect();
    let mut stmt = conn.prepare(&format!(
        "SELECT rowid FROM decl_fts WHERE decl_fts MATCH ?1 ORDER BY bm25(decl_fts, {}), rowid LIMIT ?2",
        weights.join(", ")
    ))?;
    let rows = stmt.query_map(rusqlite::params![expression, DEPTH as i64], |row| row.get::<_, i64>(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::project_map::{self as store, model_path, open_existing};
    use serde_json::{json, Value};
    use tempfile::{tempdir, TempDir};

    fn languages() -> Languages {
        Languages::new(["pt-BR", "en-US"])
    }

    fn saved(modules: Vec<Value>) -> TempDir {
        let dir = tempdir().unwrap();
        store::save_at(&model_path(dir.path()), &json!({ "modules": modules }), "scan 1", &languages()).unwrap();
        dir
    }

    fn module(path: &str, name: &str, doc: &str) -> Value {
        json!({"path": path, "declarations": [
            {"kind": "function", "name": name, "line": 1, "end_line": 5, "signature": format!("fn {name}()"), "doc": doc}]})
    }

    /// O nome da declaração de cada número da lista.
    fn names(dir: &TempDir, list: &[i64]) -> Vec<String> {
        let db = open_existing(&model_path(dir.path())).unwrap();
        list.iter()
            .map(|id| db.conn().query_row("SELECT name FROM decls WHERE rowid = ?1", [id], |row| row.get(0)).unwrap())
            .collect()
    }

    fn fillers() -> Vec<Value> {
        (0..10).map(|n| module(&format!("src/outro{n}.rs"), &format!("fazer{n}"), "algo bem diferente aqui")).collect()
    }

    /// A palavra de quatro letras ou mais também é procurada pelo começo
    /// dela: `cobr*` acha o cobrador, que a raiz `cobr` sozinha não acha. A
    /// de três letras fica só na forma inteira.
    #[test]
    fn a_word_of_four_letters_or_more_also_finds_the_words_that_start_like_it() {
        let mut modules = fillers();
        modules.push(module("src/mes.rs", "fechar", "fecha o cobrador do mes"));
        let dir = saved(modules);
        let db = open_existing(&model_path(dir.path())).unwrap();
        let found = list(db.conn(), "cobrar", &languages()).unwrap();
        assert_eq!(names(&dir, &found), ["fechar"], "the collector starts like the word");
        let text = expression(db.conn(), "cobrar log", &languages()).unwrap();
        assert!(text.contains("\"cobr\"*") && text.contains("\"cobrar\"*"), "{text}");
        assert!(text.contains("\"log\"") && !text.contains("\"log\"*"), "a three-letter word has no start: {text}");
        assert!(text.contains(" OR "), "{text}");
    }

    /// O nome pesa mais que a documentação: a declaração que se chama como
    /// a palavra vem antes da que só a cita na documentação.
    #[test]
    fn a_declaration_named_like_the_word_comes_before_one_that_only_documents_it() {
        let mut modules = fillers();
        modules.push(module("src/a.rs", "explicar", "cobrar"));
        modules.push(module("src/b.rs", "cobrar", "faz outra coisa bem hoje"));
        let dir = saved(modules);
        let db = open_existing(&model_path(dir.path())).unwrap();
        let found = list(db.conn(), "cobrar", &languages()).unwrap();
        assert_eq!(names(&dir, &found), ["cobrar", "explicar"]);
    }

    #[test]
    fn the_list_holds_the_first_hundred_declarations() {
        let modules: Vec<Value> =
            (0..120).map(|n| module(&format!("src/m{n}.rs"), &format!("cobrar{n}"), "cobra o pedido")).collect();
        let dir = saved(modules);
        let db = open_existing(&model_path(dir.path())).unwrap();
        assert_eq!(list(db.conn(), "cobrar pedido", &languages()).unwrap().len(), DEPTH);
        assert!(list(db.conn(), "de a o", &languages()).unwrap().is_empty(), "no word the index reads, no list");
    }

    /// O índice das declarações guarda o começo de três e de quatro letras
    /// das palavras, para a consulta ler o começo sem varrer o vocabulário.
    #[test]
    fn the_declaration_index_keeps_the_three_and_four_letter_starts() {
        let dir = saved(fillers());
        let db = open_existing(&model_path(dir.path())).unwrap();
        let sql: String =
            db.conn().query_row("SELECT sql FROM sqlite_master WHERE name = 'decl_fts'", [], |row| row.get(0)).unwrap();
        assert!(sql.contains("prefix='3 4'"), "{sql}");
    }
}
