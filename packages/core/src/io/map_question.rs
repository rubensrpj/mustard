//! A pergunta da busca com filtro contra o índice do mapa: cada palavra leva
//! a raiz da primeira língua, a do texto do projeto, e só a que não acha nada
//! no índice ganha também a das outras línguas ([`in_text_language`]).
//!
//! O corte que a pergunta leva vive em
//! [`Normalizer::query_in_text_language`]; aqui mora a resposta ao "acha
//! algo?" que ele faz por palavra: se alguma forma dela tem ocorrência em
//! algum documento dos níveis que a lista inteira lê.

use rusqlite::{Connection, Statement};

use crate::domain::normalize::{Languages, Normalizer};
use crate::io::map_search::as_indexed;
use crate::platform::error::Result;

/// A tabela de palavras de um nível do índice e a dos tamanhos dos
/// documentos dele: só o documento que está nesta última conta.
#[derive(Debug, Clone, Copy)]
pub(super) struct Vocabulary<'a> {
    pub vocab: &'a str,
    pub lengths: &'a str,
}

/// As palavras de `text`, uma lista de formas por palavra, para a busca com
/// filtro sobre o índice em `conn` feito nas línguas `languages`. As palavras
/// cujas formas da primeira língua não aparecem em nenhum documento de
/// `levels` levam também as formas das outras línguas; as demais, só as da
/// primeira.
pub(super) fn in_text_language(
    conn: &Connection,
    languages: &Languages,
    text: &str,
    levels: &[Vocabulary<'_>],
) -> Result<Vec<Vec<String>>> {
    let mut lookups = levels
        .iter()
        .map(|level| {
            conn.prepare(&format!(
                "SELECT 1 FROM {} v JOIN {} l ON l.id = v.doc WHERE v.term = ?1 LIMIT 1",
                level.vocab, level.lengths
            ))
        })
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Normalizer::new(languages).query_in_text_language(text, |forms| appear(conn, &mut lookups, forms))
}

/// `true` quando alguma das `forms` da palavra, como o tokenizador do índice
/// as grava, tem ocorrência em algum dos níveis.
fn appear(conn: &Connection, lookups: &mut [Statement<'_>], forms: &[String]) -> Result<bool> {
    let indexed = as_indexed(conn, &[forms.to_vec()])?;
    for form in indexed.iter().flatten() {
        for lookup in lookups.iter_mut() {
            if lookup.exists([form])? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::{json, Value};
    use tempfile::{tempdir, TempDir};

    use crate::domain::normalize::Languages;
    use crate::domain::search::CANDIDATES;
    use crate::io::map_search::candidates;
    use crate::io::project_map::{model_path, open_existing, save_at};

    /// As línguas de um projeto com o texto em português e o código em inglês.
    fn languages() -> Languages {
        Languages::new(["pt-BR", "en-US"])
    }

    /// Um projeto com o mapa deste JSON gravado pela porta, como o scan grava.
    fn saved(map: &Value) -> TempDir {
        let dir = tempdir().unwrap();
        save_at(&model_path(dir.path()), map, "scan 1", &languages()).unwrap();
        dir
    }

    /// O mapa de um arquivo por declaração `(caminho, nome, documentação)`.
    fn map_of(files: &[(&str, &str, &str)]) -> Value {
        let modules: Vec<Value> = files
            .iter()
            .map(|(path, name, doc)| {
                json!({"path": path, "declarations": [
                    {"kind": "class", "name": name, "line": 1, "end_line": 5, "signature": format!("class {name}"), "doc": doc}]})
            })
            .collect();
        json!({ "modules": modules })
    }

    /// O id da declaração `name` no mapa.
    fn id_of(dir: &Path, name: &str) -> i64 {
        let db = open_existing(&model_path(dir)).unwrap();
        db.conn().query_row("SELECT rowid FROM decls WHERE name = ?1", [name], |row| row.get(0)).unwrap()
    }

    /// Os candidatos da busca com filtro para a pergunta `query`.
    fn whole(dir: &Path, query: &str) -> Vec<i64> {
        candidates(dir, query, "", &languages(), CANDIDATES).unwrap().whole
    }

    /// Num projeto de texto em português, a palavra inglesa que não existe no
    /// índice (`users`) acha a classe pela raiz inglesa (`user`, de
    /// `UserRepository`), sem a documentação escrever a palavra.
    #[test]
    fn a_word_that_finds_nothing_in_the_text_language_finds_the_class_by_the_english_root() {
        let dir = saved(&map_of(&[
            ("src/repo.rs", "UserRepository", "Grava o cadastro"),
            ("src/pagamento.rs", "Pagamento", "Cobra a fatura"),
        ]));
        assert_eq!(whole(dir.path(), "users"), vec![id_of(dir.path(), "UserRepository")]);
    }

    /// A palavra que a primeira língua já acha continua só com a forma dela:
    /// `commands` acha a declaração que escreve `commands` e não a que escreve
    /// `command`, que a raiz inglesa traria junto.
    #[test]
    fn a_word_the_text_language_already_finds_keeps_only_its_own_form() {
        let dir = saved(&map_of(&[
            ("src/plural.rs", "Varios", "Runs the commands of the queue"),
            ("src/singular.rs", "Unico", "Runs one command of the queue"),
        ]));
        assert_eq!(whole(dir.path(), "commands"), vec![id_of(dir.path(), "Varios")]);
    }

    /// A decisão é palavra por palavra: na mesma pergunta, `users` ganha a raiz
    /// inglesa e `commands` fica só com a forma dela.
    #[test]
    fn each_word_of_the_question_widens_on_its_own() {
        let dir = saved(&map_of(&[
            ("src/repo.rs", "UserRepository", "Grava o cadastro"),
            ("src/plural.rs", "Varios", "Runs the commands of the queue"),
            ("src/singular.rs", "Unico", "Runs one command of the queue"),
        ]));
        let found = whole(dir.path(), "users commands");
        let (repo, plural, singular) = (id_of(dir.path(), "UserRepository"), id_of(dir.path(), "Varios"), id_of(dir.path(), "Unico"));
        assert!(found.contains(&repo) && found.contains(&plural), "{found:?}");
        assert!(!found.contains(&singular), "the singular only writes the english root of `commands`: {found:?}");
    }
}
