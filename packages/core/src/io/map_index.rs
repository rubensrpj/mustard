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
        #[cfg(test)]
        if let Some(tuned) = tuning::weight(self.fts, column) {
            return tuned;
        }
        self.columns().position(|name| name == column).map_or(1.0, |at| self.weights[at])
    }
}

/// O nível dos arquivos: o que a busca devolve.
///
/// Os pesos saem de uma subida coordenada, coluna a coluna, sobre os assuntos
/// 0 a 19 da régua de 120 buscas de cada um dos três projetos de prova,
/// medida só com a frase e com os nomes, e conferida nos assuntos 20 a 39. O
/// ganho é o do arquivo certo entre os cinco primeiros (vale mais o mais
/// perto do primeiro) e entre os cem, nos candidatos do filtro, mais o dos
/// cinco da resposta sem o filtro. Nos assuntos de conferência o ganho foi de
/// 575 para 611; só com a frase, o certo entre os cinco da resposta foi de 85
/// para 101 das 180 buscas, e entre os cinco candidatos, de 77 para 84. Na
/// régua inteira de 120 buscas por projeto, os cinco da resposta só com a
/// frase foram de 65 para 75 (Mustard), de 49 para 66 (Sialia) e de 33 para
/// 63 (Suzano); com os nomes, de 112 para 113, de 62 para 68 e de 83 para 95.
/// Nome, log, erro e texto fixo pesam mais que o caminho e a documentação: o
/// nome e a mensagem escrita são o que a pergunta quase copia. Os títulos dos
/// commits ficam com peso 0: entram no índice, mas nenhum peso acima de zero
/// subiu a régua.
///
/// Três técnicas foram medidas na mesma régua e ficaram de fora, porque
/// nenhuma subiu o arquivo certo entre os cinco primeiros nas 360 buscas só
/// com a frase (184 nos candidatos e 204 na resposta do banco, sem elas):
/// reescrever a pergunta com até cinco palavras da documentação e dos
/// comentários dos cinco primeiros achados, com peso 0,3, deixou o primeiro
/// do banco certo em 91 buscas contra 110 e os cinco candidatos em 172
/// contra 184, e com peso 0,1 ou 0,2 ficou igual ou abaixo; o passeio
/// aleatório pelas chamadas, semeado pelos dez primeiros e somando de 0,15 a
/// 1 da nota da última semente, ficou igual ou abaixo em todos os pontos e
/// derrubou os cem candidatos do Suzano de 95 para 91 a 87; e o corte de
/// "não achei" pela chance, pela nota do primeiro e pela distância ao
/// segundo, que sem errar nenhuma busca com o arquivo certo entre os
/// candidatos pegou 1 dos 20 pedidos inventados só com a frase e 5 com os
/// nomes.
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

/// O nível das declarações, com os pesos medidos como os do nível dos
/// arquivos. O nome da declaração pesa pouco e o caminho nada: o arquivo
/// dono já os traz, e a assinatura, que traz o nome com o tipo, pesa mais. O
/// caminho entra no índice quebrado em palavras, como o nome, nos dois níveis;
/// no das declarações, o peso 0,5 ou 1 baixou o ganho da régua de 360 buscas
/// nos assuntos pares (de 42 para 38 e 36) e não o subiu nos ímpares, e no dos
/// arquivos o peso 0,5 o baixou e o 2 o deixou igual, por isso o do arquivo
/// fica em 1. Os
/// membros e os nomes de quem usa a declaração ficam com peso 0: entram no
/// índice, mas nenhum peso acima de zero subiu a régua (o dos nomes de quem
/// usa a baixou em todos os pesos medidos). Os títulos dos commits e os
/// comentários de revisão pesam 0,25, como a documentação do arquivo: a
/// régua, feita de perguntas sobre o código, não tem pergunta sobre o que o
/// histórico diz, e por isso qualquer peso acima de zero lhe custa quase o
/// mesmo — uma busca a menos entre os cinco primeiros e até quatro entre os
/// cem, em 119 —, e 0,25 é o que menos custa; com peso 0 o histórico não
/// acharia declaração nenhuma.
///
/// Com as palavras vizinhas e a ordem dos vetores somadas à busca, os pesos
/// e o peso do tamanho do campo foram medidos de novo, na régua de 354
/// buscas (só a frase e com os nomes), e ficaram. O `B` do nível das
/// declarações em 0,5 tirou 4 buscas do primeiro lugar da resposta no
/// Mustard (de 60 para 56) e 6 dos cinco primeiros na Sialia (de 81 para
/// 75), e em 0,3 tirou 8 e 7; o `B` do nível dos arquivos em 0,5 subiu o
/// primeiro do banco (de 29 para 35 no Mustard) mas baixou a resposta no
/// Suzano (de 39 para 36 em primeiro, de 69 para 67 nos cinco). O peso do
/// nome em 0,3 e o da assinatura em 1 não subiram nenhum projeto sem
/// baixar outro. A declaração de nome ou assinatura curtos que sobe numa
/// frase longa fica onde a ordem única a põe: a lista de base e a dos nomes
/// entram só pelo rodízio, com o peso pequeno dele.
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

/// Os pesos que uma medida põe no lugar dos da tabela, só nos testes.
#[cfg(test)]
pub(super) mod tuning {
    use std::collections::HashMap;
    use std::sync::RwLock;

    static WEIGHTS: RwLock<Option<HashMap<(String, String), f64>>> = RwLock::new(None);

    pub(super) fn weight(table: &str, column: &str) -> Option<f64> {
        WEIGHTS.read().unwrap().as_ref()?.get(&(table.to_string(), column.to_string())).copied()
    }

    pub(crate) fn set(table: &str, column: &str, weight: f64) {
        WEIGHTS
            .write()
            .unwrap()
            .get_or_insert_with(HashMap::new)
            .insert((table.to_string(), column.to_string()), weight);
    }
}

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
