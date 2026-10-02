//! `map_index` — como o índice de palavras do mapa é feito e como a pergunta
//! passa por ele: os níveis (a tabela, as colunas e o peso de cada uma) e o
//! tokenizador.
//!
//! A busca por palavras (`io::map_search`), a pergunta quebrada em palavras
//! (`io::map_words`, `io::map_question`) e o sentido pelos vetores
//! (`io::map_sense`) precisam saber do índice o mesmo: que tabelas e colunas
//! ele tem, quanto pesa cada coluna e como uma palavra da pergunta vira os
//! termos que ele grava. Isso mora aqui, num módulo que só conhece o banco,
//! para que quem depende do índice não dependa da busca inteira.

use std::collections::HashMap;

use rusqlite::{params, Connection};

use crate::platform::error::Result;

/// O nível da busca que lê as marcas do glossário (`io::map_glossary`): a
/// declaração marcada, ou o arquivo dela.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Learned {
    Decls,
    Files,
}

/// De cada palavra de `words` — cada uma com as suas formas, antes do
/// tokenizador do índice —, os documentos do nível `level` que alguma marca
/// com uma das formas dela liga: a declaração do nível com o arquivo e o nome
/// da marca, ou o arquivo dela, enquanto a declaração existir. Sem repetir.
/// As marcas se leem inteiras, que são poucas; as declarações, numa consulta
/// só, e só quando alguma marca casa com a pergunta.
pub(crate) fn marked(conn: &Connection, level: Learned, words: &[Vec<String>]) -> Result<Vec<Vec<i64>>> {
    let mut out: Vec<Vec<i64>> = vec![Vec::new(); words.len()];
    if words.is_empty() {
        return Ok(out);
    }
    let mut hits: HashMap<i64, Vec<usize>> = HashMap::new();
    let mut stmt = conn.prepare("SELECT rowid, forms FROM glossary_marks")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let forms: Vec<String> = serde_json::from_str(&row.get::<_, String>(1)?).unwrap_or_default();
        let hit: Vec<usize> = (0..words.len()).filter(|&at| words[at].iter().any(|form| forms.contains(form))).collect();
        if !hit.is_empty() {
            hits.insert(row.get(0)?, hit);
        }
    }
    if hits.is_empty() {
        return Ok(out);
    }
    let ids: Vec<String> = hits.keys().map(i64::to_string).collect();
    let docs = match level {
        Learned::Decls => {
            "SELECT m.rowid, d.rowid FROM glossary_marks m JOIN decls d ON d.file = m.file AND d.name = m.name \
             JOIN decl_lengths l ON l.id = d.rowid"
        }
        Learned::Files => {
            "SELECT DISTINCT m.rowid, f.rowid FROM glossary_marks m JOIN decls d ON d.file = m.file AND d.name = m.name \
             JOIN files f ON f.path = m.file JOIN file_lengths l ON l.id = f.rowid"
        }
    };
    let mut stmt = conn.prepare(&format!("{docs} WHERE m.rowid IN ({}) ORDER BY m.rowid", ids.join(", ")))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let (mark, doc) = (row.get::<_, i64>(0)?, row.get::<_, i64>(1)?);
        for &at in hits.get(&mark).map(Vec::as_slice).unwrap_or_default() {
            if !out[at].contains(&doc) {
                out[at].push(doc);
            }
        }
    }
    Ok(out)
}

/// Um nível do índice: a tabela FTS5, a lista de cada forma por ela, a tabela
/// dos tamanhos, os campos que a lista de base lê e, depois deles, os que o
/// índice guarda sem que ela os leia, na ordem das colunas.
pub(super) struct Level {
    pub(super) fts: &'static str,
    pub(super) vocab: &'static str,
    pub(super) lengths: &'static str,
    /// A tabela das línguas, do número de documentos e das médias.
    pub(super) meta: &'static str,
    pub(super) fields: &'static [&'static str],
    pub(super) unread: &'static [&'static str],
    /// O peso de cada coluna na nota, na ordem de [`Level::columns`].
    weights: &'static [f64],
    /// Como o nível lê as marcas do glossário; `None` no que não as lê.
    pub(super) learned: Option<Learned>,
}

impl Level {
    /// Todas as colunas do nível: os campos lidos e, depois, os outros.
    pub(super) fn columns(&self) -> impl Iterator<Item = &'static str> {
        self.fields.iter().chain(self.unread).copied()
    }

    /// O peso da coluna `column` na nota do nível.
    pub(super) fn weight(&self, column: &str) -> f64 {
        self.columns().position(|name| name == column).map_or(1.0, |at| self.weights[at])
    }
}

/// O nível dos arquivos: o que a busca devolve.
///
/// Nome, log, erro e texto fixo pesam 5: o nome e a mensagem escrita são o que
/// a pergunta quase copia. O caminho pesa 1, a documentação das declarações
/// 0,25 e a do cabeçalho do arquivo 0,1, porque dizem onde o código mora e do
/// que trata, não o que ele faz; o comentário que diz o que o arquivo faz pesa
/// 1. Os títulos dos commits ficam com peso 0: entram no índice, mas não
/// ordenam o arquivo.
pub(super) const FILE_LEVEL: Level = Level {
    fts: "file_fts",
    vocab: "file_vocab",
    lengths: "file_lengths",
    meta: "search_meta",
    fields: &["name", "path", "doc", "log", "error", "text"],
    unread: &["file_doc", "file_comment", "commits"],
    weights: &[5.0, 1.0, 0.25, 5.0, 5.0, 5.0, 0.1, 1.0, 0.0],
    learned: Some(Learned::Files),
};

/// O nível das declarações. O nome da declaração pesa pouco e o caminho nada:
/// o arquivo dono já os traz, e a assinatura, que traz o nome com o tipo, pesa
/// mais (2). O caminho entra no índice quebrado em palavras, como o nome, nos
/// dois níveis. Os membros e os nomes de quem usa a declaração ficam com peso
/// 0: entram no índice, mas não ordenam a declaração. Os títulos dos commits e
/// os comentários de revisão pesam 0,25: o histórico só ajuda quando a
/// pergunta fala dele, e com peso 0 não acharia declaração nenhuma. A
/// declaração de nome ou assinatura curtos que sobe numa frase longa fica onde
/// a ordem única a põe: a lista de base e a dos nomes entram só pelo rodízio,
/// com o peso pequeno dele.
pub(super) const DECL_LEVEL: Level = Level {
    fts: "decl_fts",
    vocab: "decl_vocab",
    lengths: "decl_lengths",
    meta: "search_meta",
    fields: &["name", "path", "signature", "doc", "log", "error", "text"],
    unread: &["whole_doc", "body_comment", "body_names", "body_calls", "owner", "members", "commits", "callers"],
    weights: &[0.1, 0.0, 2.0, 1.0, 1.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 0.5, 0.0, 0.25, 0.0],
    learned: Some(Learned::Decls),
};

/// O nível dos itens das specs (`io::map_specs`): o título, a parte do
/// usuário e as palavras de busca que a gravação calculou, cada um no seu
/// campo. As médias e as línguas moram numa tabela do bloco das specs, que a
/// montagem do mapa não esvazia.
pub(super) const SPEC_LEVEL: Level = Level {
    fts: "spec_fts",
    vocab: "spec_vocab",
    lengths: "spec_lengths",
    meta: "spec_meta",
    fields: &["title", "text", "words"],
    unread: &[],
    weights: &[1.0; 3],
    learned: None,
};

/// O tokenizador das tabelas de palavras do índice, o mesmo do esquema do
/// bloco das declarações; a pergunta passa por ele antes da consulta.
pub(super) const TOKENIZER: &str = "unicode61 remove_diacritics 2";

/// As formas de cada palavra da pergunta como o tokenizador do índice as
/// grava: sem os acentos que ele tira, e cortadas onde ele corta. A forma
/// feita só de letras minúsculas e algarismos do ASCII ele grava como vem;
/// quando alguma não é assim, a pergunta passa por ele ([`through_tokenizer`]).
pub(super) fn as_indexed(conn: &Connection, words: &[Vec<String>]) -> Result<Vec<Vec<String>>> {
    let as_is = |form: &String| !form.is_empty() && form.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    if words.iter().flatten().all(as_is) {
        return Ok(words.to_vec());
    }
    through_tokenizer(conn, words)
}

/// Se o índice lê alguma das `forms` da palavra em algum campo que conta na
/// nota, no nível dos arquivos ou no das declarações: a palavra que o mapa
/// escreve.
pub(super) fn is_read(conn: &Connection, forms: &[String]) -> Result<bool> {
    let indexed = as_indexed(conn, &[forms.to_vec()])?;
    for level in [&FILE_LEVEL, &DECL_LEVEL] {
        let columns: Vec<String> =
            level.columns().filter(|column| level.weight(column) > 0.0).map(|column| format!("'{column}'")).collect();
        let mut statement = conn.prepare(&format!(
            "SELECT 1 FROM {} WHERE term = ?1 AND col IN ({}) LIMIT 1",
            level.vocab,
            columns.join(", ")
        ))?;
        for form in indexed.iter().flatten() {
            if statement.exists([form])? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Cada forma passa por uma tabela temporária com o tokenizador do índice, e
/// os termos que ele fez dela ficam como formas da mesma palavra.
fn through_tokenizer(conn: &Connection, words: &[Vec<String>]) -> Result<Vec<Vec<String>>> {
    conn.execute_batch(&format!(
        "CREATE VIRTUAL TABLE IF NOT EXISTS temp.question USING fts5(text, tokenize='{TOKENIZER}');
         CREATE VIRTUAL TABLE IF NOT EXISTS temp.question_terms USING fts5vocab('temp', 'question', 'instance');
         DELETE FROM temp.question;"
    ))?;
    let mut owner: Vec<usize> = Vec::new();
    let mut insert = conn.prepare("INSERT INTO temp.question(rowid, text) VALUES (?1, ?2)")?;
    for (word, forms) in words.iter().enumerate() {
        for form in forms {
            insert.execute(params![owner.len() as i64, form])?;
            owner.push(word);
        }
    }
    let mut out: Vec<Vec<String>> = vec![Vec::new(); words.len()];
    let mut terms = conn.prepare("SELECT doc, term FROM temp.question_terms ORDER BY doc, offset")?;
    let mut rows = terms.query([])?;
    while let Some(row) = rows.next()? {
        let Some(&word) = usize::try_from(row.get::<_, i64>(0)?).ok().and_then(|at| owner.get(at)) else { continue };
        let term: String = row.get(1)?;
        if !out[word].contains(&term) {
            out[word].push(term);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A forma de letras minúsculas e algarismos do ASCII, que a pergunta não
    /// passa pelo tokenizador, é a que ele grava; a com outra letra, não.
    #[test]
    fn the_tokenizer_keeps_a_lowercase_ascii_form_and_folds_the_other_accents() {
        let conn = Connection::open_in_memory().unwrap();
        let words = |list: &[&[&str]]| -> Vec<Vec<String>> {
            list.iter().map(|forms| forms.iter().map(|form| form.to_string()).collect()).collect()
        };
        let plain = words(&[&["parse", "pars"], &["git2"], &["log"]]);
        assert_eq!(through_tokenizer(&conn, &plain).unwrap(), plain);
        assert_eq!(through_tokenizer(&conn, &words(&[&["šablona"], &["log"]])).unwrap(), words(&[&["sablona"], &["log"]]));
    }
}
