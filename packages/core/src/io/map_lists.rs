//! `map_lists` — as listas de arquivos e de declarações que a busca do mapa
//! lê do índice: a nota BM25F de cada documento para as palavras da
//! pergunta, a busca dos arquivos, o pedaço do nome e as quatro listas da
//! lista inteira de candidatos.
//!
//! É a camada de baixo da busca: a ordem única ([`crate::io::map_order`]) e a
//! busca ([`crate::io::map_search`]) leem daqui, e nada daqui lê delas. O
//! índice em si — os documentos, a gravação, a refeitura — fica na busca.

use std::collections::{HashMap, HashSet};

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OptionalExtension, Row, Statement};

use crate::domain::normalize::{Languages, Normalizer};
use crate::domain::search::{
    bm25f, Found, bm25f_weighted, file_list, name_list, name_words, ranked, round_robin, score_x1024, Fields, NameHits, Posting,
    NEAR_FORM_WEIGHT,
};
use crate::io::map_grouped;
use crate::io::map_index::{as_indexed, marked, Level, DECL_LEVEL, FILE_LEVEL};
use crate::io::map_question;
use crate::io::map_sense::Near;
use crate::platform::error::Result;

/// Os campos dos textos fixos, os últimos dos dois níveis, nesta ordem: cada
/// texto entra no campo da marca que o scan deu a ele, e a marca que não é
/// nenhuma destas entra no último.
pub(super) const TEXT_FIELDS: [&str; 3] = ["log", "error", "text"];

/// O peso de cada campo na nota da busca dos comentários ([`by_fields`]):
/// todos o mesmo, que é a nota em que a triagem mede a prova de cada
/// comentário.
const FIELD_WEIGHT: f64 = 1.0;

/// O peso do campo das palavras aprendidas, com o tamanho de uma palavra e a
/// média de uma: a palavra marcada conta como a escrita num campo de tamanho
/// médio. Na régua das 360 buscas em três projetos, com as marcas que a
/// primeira de cada três frases da mesma pergunta ensinaria, a certa ficou
/// em primeiro em 340 buscas com o peso 0,5, em 350 com 1 e em 351 com 2
/// (69 sem marca). Com as marcas de metade das perguntas, a outra metade
/// perdeu 4 buscas no primeiro lugar e nenhuma entre os 50 primeiros.
const LEARNED_WEIGHT: f64 = 1.0;

/// A pergunta de uma palavra que procura o pedaço do nome tem pelo menos
/// estas letras: com menos, o pedaço casa com nome demais.
const PIECE_MIN_CHARS: usize = 4;

/// O texto da coluna `at`; vazio quando ela não guarda texto.
pub(super) fn text(row: &Row<'_>, at: usize) -> Result<String> {
    Ok(match row.get_ref(at)? {
        ValueRef::Text(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        _ => String::new(),
    })
}

/// Os arquivos da busca de todas as colunas do nível, na ordem da nota,
/// sem os textos fixos: o que a ordem única dos arquivos soma à lista das
/// declarações ([`crate::io::map_order`]). As formas vizinhas das palavras
/// da pergunta ([`Near`]) valem metade das que ela escreveu.
pub(super) fn ranked_files_near(
    conn: &Connection,
    query: &str,
    languages: &Languages,
    limit: usize,
    near: &Near,
) -> Result<Vec<Found>> {
    let every: Vec<&str> = FILE_LEVEL.columns().collect();
    ranked_in(conn, query, languages, limit, &every, near)
}

/// Os arquivos da busca por palavras, na ordem da nota: primeiro os que o
/// pedaço do nome acha, depois os das palavras.
fn ranked_in(
    conn: &Connection,
    query: &str,
    languages: &Languages,
    limit: usize,
    columns: &[&str],
    near: &Near,
) -> Result<Vec<Found>> {
    let mut normalizer = Normalizer::new(languages);
    let words = normalizer.query(query);
    let by_words = by_words_near(conn, &FILE_LEVEL, columns, Texts::Apart, &words, &near.aligned(&words))?;
    let scores: HashMap<i64, f64> = by_words.iter().copied().collect();
    let mut path_of = conn.prepare("SELECT path FROM files WHERE rowid = ?1")?;
    let mut out: Vec<Found> = Vec::new();
    let pieces = by_piece(conn, query)?;
    for (file, score) in pieces.into_iter().map(|file| (file, scores.get(&file).copied())).chain(
        by_words.iter().map(|&(file, score)| (file, Some(score))),
    ) {
        if out.len() >= limit {
            break;
        }
        let Some(path) = path_of.query_row([file], |row| row.get::<_, String>(0)).optional()? else { continue };
        if out.iter().any(|seen| seen.path == path) {
            continue;
        }
        out.push(Found { path, score: score.map_or(0, score_x1024), text: None });
    }
    Ok(out)
}

/// A nota de cada documento do nível para as palavras da pergunta, cada uma
/// com as suas formas, contando só os campos `fields` do nível, cada um à
/// parte: a lista de cada forma e o tamanho dos campos de cada documento dela
/// saem numa consulta só.
pub(super) fn by_words(conn: &Connection, level: &Level, fields: &[&str], words: &[Vec<String>]) -> Result<Vec<(i64, f64)>> {
    by_words_as(conn, level, fields, Texts::Apart, words)
}

/// Como os campos dos textos fixos entram na conta: cada marca no seu campo,
/// ou as três num campo só, com o tamanho e a média somados.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Texts {
    Apart,
    Together,
}

/// O campo da conta de cada coluna de `fields`, na ordem: cada coluna no
/// seu; com `Texts::Together`, os campos dos textos fixos no mesmo.
fn slots(fields: &[&str], texts: Texts) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::with_capacity(fields.len());
    let (mut count, mut text_slot) = (0, None);
    for field in fields {
        let joined = texts == Texts::Together && TEXT_FIELDS.contains(field);
        let slot = match text_slot {
            Some(slot) if joined => slot,
            _ => {
                count += 1;
                count - 1
            }
        };
        if joined {
            text_slot = Some(slot);
        }
        out.push(slot);
    }
    out
}

/// [`by_words`] com os campos dos textos fixos como `texts` diz.
fn by_words_as(
    conn: &Connection,
    level: &Level,
    fields: &[&str],
    texts: Texts,
    words: &[Vec<String>],
) -> Result<Vec<(i64, f64)>> {
    by_words_near(conn, level, fields, texts, words, &[])
}

/// [`by_words_as`] com as formas vizinhas de cada palavra em `near` (uma
/// lista por palavra, ou nenhuma): cada uma vale [`NEAR_FORM_WEIGHT`] da nota
/// e a palavra soma a melhor das suas formas. As palavras aprendidas ligam a
/// palavra como a pergunta a escreveu, e não as vizinhas.
pub(super) fn by_words_near(
    conn: &Connection,
    level: &Level,
    fields: &[&str],
    texts: Texts,
    words: &[Vec<String>],
    near: &[Vec<String>],
) -> Result<Vec<(i64, f64)>> {
    let Counted { mut postings, mut numbers, slots, weights } = counted(conn, level, fields, texts, words, near)?;
    let Some(learned) = level.learned else { return Ok(bm25f_weighted(&postings, &weights, &numbers)) };
    let marked = marked(conn, learned, words)?;
    if marked.iter().all(Vec::is_empty) {
        return Ok(bm25f_weighted(&postings, &weights, &numbers));
    }
    let learned_slot = numbers.avg_len.len();
    numbers.avg_len.push(1.0);
    numbers.weights.push(LEARNED_WEIGHT);
    let mut weights = weights;
    for ((forms, form_weights), docs) in postings.iter_mut().zip(weights.iter_mut()).zip(&marked) {
        let extra: Vec<Posting> = docs.iter().map(|&doc| Posting { doc, field: learned_slot, field_len: 1 }).collect();
        if extra.is_empty() {
            continue;
        }
        let mut written = false;
        for (list, weight) in forms.iter_mut().zip(form_weights.iter()) {
            if *weight >= 1.0 {
                list.extend(extra.iter().copied());
                written = true;
            }
        }
        if !written {
            forms.push(extra);
            form_weights.push(1.0);
        }
    }
    let name_slot = fields.iter().position(|field| *field == "name").map(|at| slots[at]);
    Ok(behind_the_names(bm25f_weighted(&postings, &weights, &numbers), &postings, name_slot, learned_slot))
}

/// O que a nota precisa antes do glossário: as ocorrências de cada forma de
/// cada palavra, os números do índice, o campo da conta de cada coluna e o
/// peso de cada forma.
struct Counted {
    postings: Vec<Vec<Vec<Posting>>>,
    numbers: Fields,
    slots: Vec<usize>,
    weights: Vec<Vec<f64>>,
}

/// As ocorrências de cada forma de cada palavra nos campos `fields` do
/// nível, com as das formas vizinhas (`near`, uma lista por palavra ou
/// nenhuma) depois das que a pergunta escreveu; os números do índice, o
/// campo da conta de cada coluna e o peso de cada forma: 1 para a escrita e
/// [`NEAR_FORM_WEIGHT`] para a vizinha.
fn counted(
    conn: &Connection,
    level: &Level,
    fields: &[&str],
    texts: Texts,
    words: &[Vec<String>],
    near: &[Vec<String>],
) -> Result<Counted> {
    let indexed = as_indexed(conn, words)?;
    let near_indexed = if near.iter().all(Vec::is_empty) { Vec::new() } else { as_indexed(conn, near)? };
    let sizes: Vec<String> = fields.iter().map(|field| format!("l.{field}")).collect();
    let mut lists = conn.prepare(&format!(
        "SELECT v.doc, v.col, {} FROM {} v JOIN {} l ON l.id = v.doc WHERE v.term = ?1",
        sizes.join(", "),
        level.vocab,
        level.lengths
    ))?;
    let slots = slots(fields, texts);
    let mut found = Vec::with_capacity(indexed.len());
    let mut weights = Vec::with_capacity(indexed.len());
    for (at, forms) in indexed.iter().enumerate() {
        let extra: &[String] = near_indexed.get(at).map_or(&[], Vec::as_slice);
        let mut lines = Vec::with_capacity(forms.len() + extra.len());
        let mut of_word = Vec::with_capacity(forms.len() + extra.len());
        for form in forms {
            lines.push(postings(&mut lists, fields, &slots, form)?);
            of_word.push(1.0);
        }
        for form in extra.iter().filter(|form| !forms.contains(form)) {
            lines.push(postings(&mut lists, fields, &slots, form)?);
            of_word.push(NEAR_FORM_WEIGHT);
        }
        found.push(lines);
        weights.push(of_word);
    }
    let numbers = fields_of(conn, level, fields, &slots)?;
    Ok(Counted { postings: found, numbers, slots, weights })
}

/// A nota BM25F de cada documento do nível das declarações (`decl`) ou dos
/// arquivos para as palavras da pergunta, contando só os campos `fields`,
/// cada um à parte e com o mesmo peso, sem as palavras aprendidas: a nota
/// dos campos de comentário é só do que está escrito neles.
pub(super) fn by_fields(conn: &Connection, decl: bool, fields: &[&str], words: &[Vec<String>]) -> Result<Vec<(i64, f64)>> {
    let level = if decl { &DECL_LEVEL } else { &FILE_LEVEL };
    let Counted { postings, mut numbers, .. } = counted(conn, level, fields, Texts::Apart, words, &[])?;
    numbers.weights.iter_mut().for_each(|weight| *weight = FIELD_WEIGHT);
    Ok(bm25f(&postings, &numbers))
}

/// A nota com a regra das palavras aprendidas: o documento que só o campo
/// `learned` achou fica logo abaixo do mais fraco dos que casam pelo campo
/// `name`, quando estava acima dele. Sem o campo do nome, ou sem documento
/// que case por ele, a nota fica como veio.
fn behind_the_names(
    scores: Vec<(i64, f64)>,
    postings: &[Vec<Vec<Posting>>],
    name: Option<usize>,
    learned: usize,
) -> Vec<(i64, f64)> {
    let (mut named, mut plain) = (HashSet::new(), HashSet::new());
    for posting in postings.iter().flatten().flatten() {
        if posting.field != learned {
            plain.insert(posting.doc);
        }
        if Some(posting.field) == name {
            named.insert(posting.doc);
        }
    }
    let Some(floor) = scores.iter().filter(|(doc, _)| named.contains(doc)).map(|&(_, score)| score).reduce(f64::min)
    else {
        return scores;
    };
    let below = floor.next_down();
    ranked(scores.into_iter().map(|(doc, score)| if plain.contains(&doc) { (doc, score) } else { (doc, score.min(below)) }))
}

/// As ocorrências da forma `form` nas colunas `fields`, cada uma no campo da
/// conta que `slots` dá à coluna dela, com o tamanho desse campo: a soma das
/// colunas que caem nele.
fn postings(lists: &mut Statement<'_>, fields: &[&str], slots: &[usize], form: &str) -> Result<Vec<Posting>> {
    let mut rows = lists.query([form])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let column = text(row, 1)?;
        let Some(at) = fields.iter().position(|name| *name == column) else { continue };
        let field = slots[at];
        let mut field_len = 0u64;
        for (column, _) in slots.iter().enumerate().filter(|(_, slot)| **slot == field) {
            field_len += row.get::<_, i64>(2 + column)?.max(0) as u64;
        }
        out.push(Posting { doc: row.get(0)?, field, field_len });
    }
    Ok(out)
}

/// O número de documentos do nível e o tamanho médio de cada campo da conta,
/// a soma das médias das colunas de `fields` que `slots` põe nele, como o
/// índice as gravou, com o mesmo peso em todos.
fn fields_of(conn: &Connection, level: &Level, fields: &[&str], slots: &[usize]) -> Result<Fields> {
    let mut meta = conn.prepare(&format!("SELECT value FROM {} WHERE key = ?1", level.meta))?;
    let mut number = |key: String| -> Result<f64> {
        Ok(meta.query_row([key], |row| row.get::<_, f64>(0)).optional()?.unwrap_or(0.0))
    };
    let docs = number(format!("{}.docs", level.fts))? as usize;
    let count = slots.iter().max().map_or(0, |last| last + 1);
    let mut avg_len = vec![0.0; count];
    let mut weights: Vec<Option<f64>> = vec![None; count];
    for (name, slot) in fields.iter().zip(slots) {
        avg_len[*slot] += number(format!("{}.{name}", level.fts))?;
        weights[*slot].get_or_insert_with(|| level.weight(name));
    }
    Ok(Fields { docs, avg_len, weights: weights.into_iter().map(|weight| weight.unwrap_or(FIELD_WEIGHT)).collect() })
}

/// Os campos da declaração que a lista de base lê: o nome, o caminho, a
/// assinatura e a documentação.
const BASE_FIELDS: &[&str] = &["name", "path", "signature", "doc"];

/// As quatro listas de declarações que o rodízio da lista inteira junta, cada
/// uma na ordem da nota dela. As listas de palavras leem a `query` seguida da
/// `intent`; a dos nomes, só as palavras da `query`. Na lista de tudo, os
/// textos fixos da declaração contam como um campo só, como no laboratório
/// que afinou a busca com filtro: cada marca num campo à parte dava ao texto
/// de erro, que quase nenhuma declaração tem, uma média perto de zero, e a
/// palavra dele quase não pesava. A pergunta leva só a raiz da primeira
/// língua, a do texto do projeto, como no laboratório, e a das outras só na
/// palavra que a primeira não acha em nenhum documento dos dois níveis
/// ([`map_question::in_text_language`]).
pub(super) struct Sources {
    /// A de base: o nome, o caminho, a assinatura e a documentação.
    pub base: Vec<i64>,
    /// A dos nomes, pelo pedaço do nome.
    pub names: Vec<i64>,
    /// A de todos os campos da declaração.
    pub everything: Vec<i64>,
    /// As declarações dos arquivos, na ordem da nota do arquivo.
    pub files: Vec<i64>,
    /// A consulta agrupada: as primeiras declarações de uma consulta só ao
    /// índice, com as palavras em OR e o começo delas, pela nota `bm25()`.
    /// Não entra no rodízio: a ordem única a soma à parte
    /// ([`crate::io::map_order`]).
    pub grouped: Vec<i64>,
}

impl Sources {
    /// A lista inteira: o rodízio das quatro, nesta ordem.
    pub(super) fn whole(&self) -> Vec<i64> {
        round_robin(&[self.base.clone(), self.names.clone(), self.everything.clone(), self.files.clone()])
    }
}

/// As quatro listas da lista inteira, antes do rodízio, com as formas
/// vizinhas das palavras da pergunta ([`Near`]) nas três listas de palavras.
pub(super) fn sources_near(
    conn: &Connection,
    query: &str,
    intent: &str,
    languages: &Languages,
    near: &Near,
) -> Result<Sources> {
    let levels = [
        map_question::Vocabulary { vocab: DECL_LEVEL.vocab, lengths: DECL_LEVEL.lengths },
        map_question::Vocabulary { vocab: FILE_LEVEL.vocab, lengths: FILE_LEVEL.lengths },
    ];
    let words = map_question::in_text_language(conn, languages, format!("{query} {intent}").trim(), &levels)?;
    let near = near.aligned(&words);
    let base = base_list(conn, &words, &near)?;
    let every_decl_field: Vec<&str> = DECL_LEVEL.columns().collect();
    let everything = by_words_near(conn, &DECL_LEVEL, &every_decl_field, Texts::Together, &words, &near)?;
    let every_file_field: Vec<&str> = FILE_LEVEL.columns().collect();
    let file_scores: HashMap<i64, f64> =
        by_words_near(conn, &FILE_LEVEL, &every_file_field, Texts::Apart, &words, &near)?.into_iter().collect();
    let base_scores: HashMap<i64, f64> = base.iter().copied().collect();
    let names = name_list(&name_hits(conn, query)?, fields_of(conn, &DECL_LEVEL, &[], &[])?.docs);
    let files = file_list(&decl_files(conn)?, &file_scores, &base_scores);
    let ids = |list: Vec<(i64, f64)>| list.into_iter().map(|(id, _)| id).collect::<Vec<_>>();
    let grouped = map_grouped::list(conn, format!("{query} {intent}").trim(), languages)?;
    Ok(Sources { base: ids(base), names: ids(names), everything: ids(everything), files, grouped })
}

/// A lista de base: o BM25F no nível das declarações, sobre o nome, o
/// caminho, a assinatura e a documentação.
pub(super) fn base_list(conn: &Connection, words: &[Vec<String>], near: &[Vec<String>]) -> Result<Vec<(i64, f64)>> {
    by_words_near(conn, &DECL_LEVEL, BASE_FIELDS, Texts::Apart, words, near)
}

/// De cada palavra de nome da `query`, as declarações do nível cujo nome
/// dobrado a contém, pela tabela trigram.
pub(super) fn name_hits(conn: &Connection, query: &str) -> Result<Vec<NameHits>> {
    let mut stmt = conn.prepare(
        "SELECT t.rowid, t.folded FROM decl_trigram t \
         WHERE t.folded LIKE ?1 AND EXISTS (SELECT 1 FROM decl_lengths l WHERE l.id = t.rowid)",
    )?;
    let mut out = Vec::new();
    for word in name_words(query) {
        let mut names = Vec::new();
        let mut rows = stmt.query([format!("%{word}%")])?;
        while let Some(row) = rows.next()? {
            names.push((row.get::<_, i64>(0)?, text(row, 1)?.chars().count()));
        }
        out.push(NameHits { word_chars: word.chars().count(), names });
    }
    Ok(out)
}

/// Cada declaração do nível com o número do arquivo dela.
pub(super) fn decl_files(conn: &Connection) -> Result<Vec<(i64, i64)>> {
    let mut stmt = conn.prepare("SELECT l.id, t.file FROM decl_lengths l JOIN decl_trigram t ON t.rowid = l.id")?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Os arquivos das declarações cujo nome traz a pergunta como pedaço, o nome
/// mais curto primeiro: só quando a pergunta é uma palavra só, com
/// [`PIECE_MIN_CHARS`] letras ou mais.
fn by_piece(conn: &Connection, query: &str) -> Result<Vec<i64>> {
    let piece = query.trim();
    if piece.split_whitespace().count() != 1 || piece.chars().count() < PIECE_MIN_CHARS {
        return Ok(Vec::new());
    }
    let escaped = piece.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    let mut stmt = conn.prepare(
        "SELECT file FROM decl_trigram WHERE name LIKE ?1 ESCAPE '\\' ORDER BY length(name), rowid",
    )?;
    let files = stmt.query_map([format!("%{escaped}%")], |row| row.get::<_, i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out: Vec<i64> = Vec::new();
    for file in files {
        if !out.contains(&file) {
            out.push(file);
        }
    }
    Ok(out)
}
