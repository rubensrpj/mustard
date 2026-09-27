//! `map_search` — a busca do mapa pelo índice de palavras que o scan grava
//! junto das declarações (`io::project_map::DECLS`), sem ler o mapa inteiro.
//!
//! O índice tem dois níveis, a declaração e o arquivo, com uma coluna por
//! campo:
//!
//! - a declaração: o nome, o caminho do arquivo, a assinatura, a
//!   documentação e os textos fixos escritos nela — as mensagens de log, as
//!   de erro e os outros textos, um campo para cada marca;
//! - o arquivo: os nomes que ele declara, o caminho, a documentação das
//!   declarações dele e os textos fixos do arquivo inteiro, nos mesmos três
//!   campos.
//!
//! Cada texto passa pela normalização de toda busca (`domain::normalize`),
//! nas línguas do projeto, antes de entrar: o nome colado entra quebrado
//! (`parseGitLog` vira `parse git log`), e cada palavra entra com as suas
//! formas. O tamanho de cada campo, em palavras, mora numa tabela comum; as
//! línguas, o número de documentos e o tamanho médio de cada campo, em
//! `search_meta`. Os nomes das declarações entram inteiros numa tabela
//! trigram, para o pedaço do nome. O arquivo escrito por máquina fica fora
//! do índice.
//!
//! A busca devolve arquivos, cada um com o texto fixo dele que mais casa com
//! a pergunta, quando algum casa. A nota é o BM25F de `domain::search`, calculado
//! aqui sobre as listas do banco: a lista de cada forma e o tamanho dos
//! campos saem numa consulta só. Antes dela, cada forma da pergunta passa
//! pelo tokenizador do índice, que tira os acentos que a normalização não
//! conhece: a forma procurada é a que ele gravou. A pergunta de uma palavra só, com 4 letras
//! ou mais, procura também o pedaço no nome das declarações, e o que ela acha
//! vem na frente. O índice feito em outras línguas que as da busca — a
//! configuração do projeto mudou depois da gravação — se refaz antes dela.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::types::{Value as Sql, ValueRef};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row, Statement};
use serde::Deserialize;

use crate::domain::normalize::{Languages, Normalizer};
use crate::domain::project_map::{Found, FoundText, MapRefusal};
use crate::domain::search::{bm25f, score_x1024, Fields, Posting};
use crate::io::project_map::{model_path, open_existing, unreadable};
use crate::platform::error::Result;

/// Um nível do índice: a tabela FTS5, a lista de cada forma por ela, a tabela
/// dos tamanhos e os campos, na ordem das colunas.
struct Level {
    fts: &'static str,
    vocab: &'static str,
    lengths: &'static str,
    fields: &'static [&'static str],
}

/// O nível dos arquivos: o que a busca devolve.
const FILE_LEVEL: Level = Level {
    fts: "file_fts",
    vocab: "file_vocab",
    lengths: "file_lengths",
    fields: &["name", "path", "doc", "log", "error", "text"],
};

/// O nível das declarações.
const DECL_LEVEL: Level = Level {
    fts: "decl_fts",
    vocab: "decl_vocab",
    lengths: "decl_lengths",
    fields: &["name", "path", "signature", "doc", "log", "error", "text"],
};

/// Os campos dos textos fixos, os últimos dos dois níveis, nesta ordem: cada
/// texto entra no campo da marca que o scan deu a ele, e a marca que não é
/// nenhuma destas entra no último.
const TEXT_FIELDS: [&str; 3] = ["log", "error", "text"];

/// Um texto fixo como o scan o grava, na tabela dos textos de cada arquivo.
#[derive(Deserialize)]
struct Written {
    #[serde(default)]
    line: u64,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    value: String,
    #[serde(default)]
    owner: String,
}

impl Written {
    /// O campo do texto entre os de [`TEXT_FIELDS`].
    fn field(&self) -> usize {
        TEXT_FIELDS.iter().position(|kind| *kind == self.kind).unwrap_or(TEXT_FIELDS.len() - 1)
    }
}

/// O peso de cada campo na nota: todos o mesmo, como na prova da busca que
/// achou 119 dos 140 pontos da régua de perguntas. Os campos dos textos
/// fixos entraram com o mesmo peso: nesta busca, a régua foi de 101 para
/// 114 pontos com eles.
const FIELD_WEIGHT: f64 = 1.0;

/// A pergunta de uma palavra que procura o pedaço do nome tem pelo menos
/// estas letras: com menos, o pedaço casa com nome demais.
const PIECE_MIN_CHARS: usize = 4;

/// O tokenizador das tabelas de palavras do índice, o mesmo do esquema do
/// bloco das declarações; a pergunta passa por ele antes da consulta.
const TOKENIZER: &str = "unicode61 remove_diacritics 2";

/// A chave, em `search_meta`, das línguas em que as palavras foram
/// preparadas.
const LANGUAGES_KEY: &str = "languages";

/// As palavras de um campo, cada uma com as suas formas, pela normalização
/// de toda busca.
type Words = Vec<Vec<String>>;

/// Um documento do índice: o número dele — o da linha no mapa — e as
/// palavras de cada campo, na ordem do nível.
struct Doc {
    id: i64,
    fields: Vec<Words>,
}

/// Uma declaração do índice: o documento dela, o nome inteiro, para o pedaço
/// do nome, e o número do arquivo dela.
struct Decl {
    doc: Doc,
    name: String,
    file: i64,
}

/// Refaz o índice inteiro a partir das tabelas dos arquivos e das
/// declarações, com as palavras preparadas nas línguas `languages`. Roda na
/// transação de quem grava: o índice e as linhas de que ele sai entram
/// juntos.
pub(crate) fn rebuild(conn: &Connection, languages: &Languages) -> Result<()> {
    let (files, decls) = documents(conn, &mut Normalizer::new(languages))?;
    forget(conn)?;
    fill(conn, &FILE_LEVEL, &files)?;
    fill(conn, &DECL_LEVEL, decls.iter().map(|decl| &decl.doc))?;
    {
        let mut insert = conn.prepare("INSERT INTO decl_trigram(rowid, name, file) VALUES (?1, ?2, ?3)")?;
        for decl in &decls {
            insert.execute(params![decl.doc.id, decl.name, decl.file])?;
        }
    }
    conn.execute(
        "INSERT INTO search_meta(key, value) VALUES (?1, ?2)",
        params![LANGUAGES_KEY, languages.codes().join(",")],
    )?;
    for table in [FILE_LEVEL.fts, DECL_LEVEL.fts, "decl_trigram"] {
        conn.execute(&format!("INSERT INTO {table}({table}) VALUES ('optimize')"), [])?;
    }
    Ok(())
}

/// Esvazia o índice, línguas inclusive: a primeira busca o refaz nas dela.
pub(crate) fn forget(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "INSERT INTO file_fts(file_fts) VALUES ('delete-all');\
         INSERT INTO decl_fts(decl_fts) VALUES ('delete-all');\
         DELETE FROM decl_trigram; DELETE FROM file_lengths; DELETE FROM decl_lengths; DELETE FROM search_meta;",
    )?;
    Ok(())
}

/// Os documentos dos dois níveis, lidos das tabelas do mapa, com as palavras
/// já preparadas: cada arquivo que não é escrito por máquina e cada
/// declaração dele. Cada texto se prepara uma vez: o caminho, uma vez por
/// arquivo; os nomes, a documentação e os textos fixos do arquivo são as
/// palavras das declarações dele e dos textos dele, sem repetir a palavra de
/// mesmas formas — o mesmo que preparar o texto delas junto. Cada texto fixo
/// é também da declaração mais interna que contém a linha dele.
fn documents(conn: &Connection, normalizer: &mut Normalizer) -> Result<(Vec<Doc>, Vec<Decl>)> {
    let mut files: Vec<Doc> = Vec::new();
    // As palavras que o arquivo já tem nos nomes, na documentação e em cada
    // campo dos textos.
    let mut seen: Vec<[HashSet<Vec<String>>; 5]> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();
    let mut stmt = conn.prepare("SELECT rowid, path, file_class FROM files ORDER BY rowid")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        if !text(row, 2)?.is_empty() {
            continue;
        }
        let path = text(row, 1)?;
        let words = normalizer.forms(&path);
        at.insert(path, files.len());
        let mut fields = vec![Vec::new(); FILE_LEVEL.fields.len()];
        fields[1] = words;
        files.push(Doc { id: row.get(0)?, fields });
        seen.push(Default::default());
    }
    let mut rows_of: Vec<DeclRow> = Vec::new();
    let mut stmt = conn.prepare("SELECT rowid, file, name, signature, doc, line, end_line FROM decls ORDER BY rowid")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let Some(&owner) = at.get(&text(row, 1)?) else { continue };
        let line = |at: usize| -> Result<u64> { Ok(row.get::<_, Option<i64>>(at)?.unwrap_or(0).max(0) as u64) };
        rows_of.push(DeclRow {
            id: row.get(0)?,
            owner,
            name: text(row, 2)?,
            signature: text(row, 3)?,
            doc: text(row, 4)?,
            lines: (line(5)?, line(6)?),
            texts: vec![Vec::new(); TEXT_FIELDS.len()],
        });
    }
    // Os textos de cada arquivo: as palavras vão para o arquivo e para a
    // declaração dele que contém a linha.
    let mut decls_of: Vec<Vec<usize>> = vec![Vec::new(); files.len()];
    for (at, row) in rows_of.iter().enumerate() {
        decls_of[row.owner].push(at);
    }
    for (path, written) in written_texts(conn)? {
        let Some(&owner) = at.get(&path) else { continue };
        for written in written {
            let words = normalizer.forms(&written.value);
            let field = written.field();
            add_new(&mut files[owner].fields[3 + field], &mut seen[owner][2 + field], &words);
            if let Some(decl) = innermost(&rows_of, &decls_of[owner], written.line) {
                rows_of[decl].texts[field].push(words);
            }
        }
    }
    let mut decls = Vec::new();
    for row in rows_of {
        let (name_words, doc_words) = (normalizer.forms(&row.name), normalizer.forms(&row.doc));
        let file = &mut files[row.owner];
        add_new(&mut file.fields[0], &mut seen[row.owner][0], &name_words);
        add_new(&mut file.fields[2], &mut seen[row.owner][1], &doc_words);
        let mut fields = vec![name_words, file.fields[1].clone(), normalizer.forms(&row.signature), doc_words];
        for texts in row.texts {
            let mut words: Words = Vec::new();
            add_new(&mut words, &mut HashSet::new(), &texts.concat());
            fields.push(words);
        }
        decls.push(Decl { doc: Doc { id: row.id, fields }, name: row.name, file: file.id });
    }
    Ok((files, decls))
}

/// Uma declaração lida do mapa para o índice: o número dela, o arquivo, o
/// nome, a assinatura, a documentação, a primeira e a última linha, e as
/// palavras dos textos fixos que caem nela, por campo.
struct DeclRow {
    id: i64,
    owner: usize,
    name: String,
    signature: String,
    doc: String,
    lines: (u64, u64),
    texts: Vec<Vec<Words>>,
}

/// Acrescenta a `into` cada palavra de `words` que `seen` ainda não tem.
fn add_new(into: &mut Words, seen: &mut HashSet<Vec<String>>, words: &Words) {
    for word in words {
        if seen.insert(word.clone()) {
            into.push(word.clone());
        }
    }
}

/// Das declarações `of` um arquivo, a que contém a linha `line`: a mais
/// interna, a que começa mais abaixo; empatadas, a última. A declaração sem a
/// última linha gravada cobre só o que vem depois dela.
fn innermost(rows: &[DeclRow], of: &[usize], line: u64) -> Option<usize> {
    let mut best: Option<usize> = None;
    for &at in of {
        let (first, last) = rows[at].lines;
        if first <= line && (last == 0 || last >= line) && best.is_none_or(|b| rows[b].lines.0 <= first) {
            best = Some(at);
        }
    }
    best
}

/// Os textos fixos de cada arquivo, como o scan os gravou. O arquivo cuja
/// coluna não se lê fica sem eles.
fn written_texts(conn: &Connection) -> Result<Vec<(String, Vec<Written>)>> {
    let mut stmt = conn.prepare("SELECT path, texts FROM texts ORDER BY rowid")?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let written: Vec<Written> = serde_json::from_str(&text(row, 1)?).unwrap_or_default();
        if !written.is_empty() {
            out.push((text(row, 0)?, written));
        }
    }
    Ok(out)
}

/// O texto da coluna `at`; vazio quando ela não guarda texto.
fn text(row: &Row<'_>, at: usize) -> Result<String> {
    Ok(match row.get_ref(at)? {
        ValueRef::Text(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        _ => String::new(),
    })
}

/// Grava os documentos de um nível: as formas de cada campo na tabela FTS5,
/// o tamanho de cada campo em palavras, e o número de documentos e o tamanho
/// médio de cada campo em `search_meta`.
fn fill<'d>(conn: &Connection, level: &Level, docs: impl IntoIterator<Item = &'d Doc>) -> Result<()> {
    let columns = level.fields.join(", ");
    let slots: Vec<String> = (2..=level.fields.len() + 1).map(|at| format!("?{at}")).collect();
    let slots = slots.join(", ");
    let mut words = conn.prepare(&format!("INSERT INTO {}(rowid, {columns}) VALUES (?1, {slots})", level.fts))?;
    let mut lengths = conn.prepare(&format!("INSERT INTO {}(id, {columns}) VALUES (?1, {slots})", level.lengths))?;
    let mut total = vec![0u64; level.fields.len()];
    let mut count = 0u64;
    for doc in docs {
        let mut texts = vec![Sql::Integer(doc.id)];
        let mut sizes = vec![Sql::Integer(doc.id)];
        for (field, prepared) in doc.fields.iter().enumerate() {
            total[field] += prepared.len() as u64;
            sizes.push(Sql::Integer(prepared.len() as i64));
            texts.push(Sql::Text(prepared.iter().flatten().map(String::as_str).collect::<Vec<_>>().join(" ")));
        }
        words.execute(params_from_iter(texts))?;
        lengths.execute(params_from_iter(sizes))?;
        count += 1;
    }
    let mut meta = conn.prepare("INSERT INTO search_meta(key, value) VALUES (?1, ?2)")?;
    meta.execute(params![format!("{}.docs", level.fts), count as i64])?;
    for (field, name) in level.fields.iter().enumerate() {
        let avg = if count == 0 { 0.0 } else { total[field] as f64 / count as f64 };
        meta.execute(params![format!("{}.{name}", level.fts), avg])?;
    }
    Ok(())
}

/// Os arquivos que mais casam com a pergunta `query` no mapa do projeto em
/// `root`, até `limit`, com as palavras cortadas nas línguas `languages`:
/// primeiro os que o pedaço do nome acha, depois os das palavras, da nota
/// mais alta para a mais baixa. As recusas são as de todo leitor do mapa:
/// sem o arquivo, [`MapRefusal::MapMissing`].
pub fn search(root: &Path, query: &str, languages: &Languages, limit: usize) -> std::result::Result<Vec<Found>, MapRefusal> {
    search_at(&model_path(root), query, languages, limit)
}

/// A busca de [`search`] no mapa gravado em `model`.
pub fn search_at(
    model: &Path,
    query: &str,
    languages: &Languages,
    limit: usize,
) -> std::result::Result<Vec<Found>, MapRefusal> {
    let mut db = open_existing(model)?;
    if !made_in(db.conn(), languages).map_err(unreadable)? {
        db.write(|tx| if made_in(tx, languages)? { Ok(()) } else { rebuild(tx, languages) }).map_err(unreadable)?;
    }
    found(db.conn(), query, languages, limit).map_err(unreadable)
}

/// `true` quando o índice foi feito nas línguas `languages`.
fn made_in(conn: &Connection, languages: &Languages) -> Result<bool> {
    let stored: Option<String> = conn
        .query_row("SELECT value FROM search_meta WHERE key = ?1", [LANGUAGES_KEY], |row| row.get(0))
        .optional()?;
    Ok(stored.is_some_and(|stored| stored == languages.codes().join(",")))
}

fn found(conn: &Connection, query: &str, languages: &Languages, limit: usize) -> Result<Vec<Found>> {
    let mut normalizer = Normalizer::new(languages);
    let words = normalizer.query(query);
    let by_words = by_words(conn, &FILE_LEVEL, &words)?;
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
    let mut texts = conn.prepare("SELECT texts FROM texts WHERE path = ?1")?;
    for found in &mut out {
        found.text = best_text(&mut texts, &found.path, &words, &mut normalizer)?;
    }
    Ok(out)
}

/// O texto fixo do arquivo em `path` que mais casa com as palavras `words` da
/// pergunta: o que tem mais delas, por alguma das formas; empatados, o de
/// linha mais acima. `None` quando nenhum tem nenhuma.
fn best_text(
    texts: &mut Statement<'_>,
    path: &str,
    words: &[Vec<String>],
    normalizer: &mut Normalizer,
) -> Result<Option<FoundText>> {
    let mut rows = texts.query([path])?;
    let Some(row) = rows.next()? else { return Ok(None) };
    let written: Vec<Written> = serde_json::from_str(&text(row, 0)?).unwrap_or_default();
    let mut best: Option<(usize, Written)> = None;
    for candidate in written {
        let forms: HashSet<String> = normalizer.forms(&candidate.value).into_iter().flatten().collect();
        let hits = words.iter().filter(|word| word.iter().any(|form| forms.contains(form))).count();
        if hits > 0 && best.as_ref().is_none_or(|(most, _)| hits > *most) {
            best = Some((hits, candidate));
        }
    }
    Ok(best.map(|(_, w)| FoundText { line: w.line, kind: w.kind, value: w.value, owner: w.owner }))
}

/// A nota de cada documento do nível para as palavras da pergunta, cada uma
/// com as suas formas: a lista de cada forma e o tamanho dos campos de cada
/// documento dela saem numa consulta só.
fn by_words(conn: &Connection, level: &Level, words: &[Vec<String>]) -> Result<Vec<(i64, f64)>> {
    let words = as_indexed(conn, words)?;
    let sizes: Vec<String> = level.fields.iter().map(|field| format!("l.{field}")).collect();
    let mut lists = conn.prepare(&format!(
        "SELECT v.doc, v.col, {} FROM {} v JOIN {} l ON l.id = v.doc WHERE v.term = ?1",
        sizes.join(", "),
        level.vocab,
        level.lengths
    ))?;
    let postings = words
        .iter()
        .map(|forms| forms.iter().map(|form| postings(&mut lists, level, form)).collect::<Result<Vec<_>>>())
        .collect::<Result<Vec<_>>>()?;
    Ok(bm25f(&postings, &fields_of(conn, level)?))
}

/// As formas de cada palavra da pergunta como o tokenizador do índice as
/// grava: sem os acentos que ele tira, e cortadas onde ele corta. A forma
/// feita só de letras minúsculas e algarismos do ASCII ele grava como vem;
/// quando alguma não é assim, a pergunta passa por ele ([`through_tokenizer`]).
fn as_indexed(conn: &Connection, words: &[Vec<String>]) -> Result<Vec<Vec<String>>> {
    let as_is = |form: &String| !form.is_empty() && form.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    if words.iter().flatten().all(as_is) {
        return Ok(words.to_vec());
    }
    through_tokenizer(conn, words)
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
        let term = text(row, 1)?;
        if !out[word].contains(&term) {
            out[word].push(term);
        }
    }
    Ok(out)
}

/// As ocorrências da forma `form` no nível, com o tamanho do campo de cada
/// uma.
fn postings(lists: &mut Statement<'_>, level: &Level, form: &str) -> Result<Vec<Posting>> {
    let mut rows = lists.query([form])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let column = text(row, 1)?;
        let Some(field) = level.fields.iter().position(|name| *name == column) else { continue };
        out.push(Posting { doc: row.get(0)?, field, field_len: row.get::<_, i64>(2 + field)?.max(0) as u64 });
    }
    Ok(out)
}

/// O número de documentos do nível e o tamanho médio de cada campo, como o
/// índice os gravou, com o mesmo peso em todos os campos.
fn fields_of(conn: &Connection, level: &Level) -> Result<Fields> {
    let mut meta = conn.prepare("SELECT value FROM search_meta WHERE key = ?1")?;
    let mut number = |key: String| -> Result<f64> {
        Ok(meta.query_row([key], |row| row.get::<_, f64>(0)).optional()?.unwrap_or(0.0))
    };
    let docs = number(format!("{}.docs", level.fts))? as usize;
    let avg_len =
        level.fields.iter().map(|name| number(format!("{}.{name}", level.fts))).collect::<Result<Vec<_>>>()?;
    Ok(Fields { docs, avg_len, weights: vec![FIELD_WEIGHT; level.fields.len()] })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::search::TOP;
    use crate::io::project_map as store;
    use serde_json::{json, Value};
    use tempfile::{tempdir, TempDir};

    /// As línguas de um projeto com o texto em português e o código em inglês.
    fn languages() -> Languages {
        Languages::new(["pt-BR", "en-US"])
    }

    /// Um arquivo do mapa de teste: o caminho, a classe (vazia no código
    /// escrito à mão) e as declarações, cada uma com o nome e a documentação.
    type File<'a> = (&'a str, &'a str, &'a [(&'a str, &'a str)]);

    /// O mapa em JSON, no formato do scan, com estes arquivos.
    fn map_of(files: &[File<'_>]) -> Value {
        let modules: Vec<Value> = files
            .iter()
            .map(|(path, class, decls)| {
                let decls: Vec<Value> = decls
                    .iter()
                    .map(|(name, doc)| json!({"kind": "function", "name": name, "line": 1, "signature": format!("fn {name}()"), "doc": doc}))
                    .collect();
                json!({"path": path, "file_class": class, "declarations": decls})
            })
            .collect();
        json!({ "modules": modules })
    }

    /// Um projeto com o mapa destes arquivos gravado pela porta, como o scan
    /// grava: o índice de busca entra junto.
    fn saved(files: &[File<'_>]) -> TempDir {
        let dir = tempdir().unwrap();
        store::save_at(&model_path(dir.path()), &map_of(files), "scan 1", &languages()).unwrap();
        dir
    }

    /// Os caminhos que a busca devolve.
    fn paths(dir: &Path, query: &str) -> Vec<String> {
        search(dir, query, &languages(), TOP).unwrap().into_iter().map(|found| found.path).collect()
    }

    /// Os caminhos que só as palavras acham, com a nota, na ordem da nota.
    fn by_words_only(dir: &Path, query: &str) -> Vec<(String, u64)> {
        let db = open_existing(&model_path(dir)).unwrap();
        let words = Normalizer::new(&languages()).query(query);
        by_words(db.conn(), &FILE_LEVEL, &words)
            .unwrap()
            .into_iter()
            .map(|(id, score)| {
                let path = db.conn().query_row("SELECT path FROM files WHERE rowid = ?1", [id], |row| row.get(0)).unwrap();
                (path, score_x1024(score))
            })
            .collect()
    }

    #[test]
    fn a_glued_name_is_found_by_its_words_and_by_one_of_them() {
        let dir =
            saved(&[("src/consulta.rs", "", &[("buscarPedido", "")]), ("src/cliente.rs", "", &[("salvarCliente", "")])]);
        assert_eq!(paths(dir.path(), "buscar pedido"), ["src/consulta.rs"]);
        assert_eq!(paths(dir.path(), "pedido"), ["src/consulta.rs"]);
        let words: Vec<String> = by_words_only(dir.path(), "pedido").into_iter().map(|(path, _)| path).collect();
        assert_eq!(words, ["src/consulta.rs"], "the words alone find the glued name");
    }

    #[test]
    fn a_portuguese_query_finds_a_portuguese_identifier() {
        let dir = saved(&[
            ("src/usuarios/cadastro.rs", "", &[]),
            ("src/pagamentos/processador_pagamento.rs", "", &[("ProcessadorPagamento", "")]),
        ]);
        assert_eq!(
            paths(dir.path(), "processar os pagamentos").first().map(String::as_str),
            Some("src/pagamentos/processador_pagamento.rs")
        );
    }

    /// Os arquivos de um mapa um pouco maior: nomes colados, caminhos,
    /// documentação e um arquivo escrito por máquina.
    const LARGER: &[File<'static>] = &[
        ("src/pedidos/busca.rs", "", &[("buscarPedido", "Busca o pedido do cliente."), ("listarPedidos", "")]),
        ("src/clientes/cadastro.rs", "", &[("salvarCliente", "Grava o cliente novo."), ("Cliente", "")]),
        ("src/relatorios/mensal.rs", "", &[("RelatorioMensal", "O relatório do mês, por cliente.")]),
        ("src/pedidos/pagamento.rs", "", &[("pagarPedido", ""), ("estornarPagamento", "Devolve o pagamento.")]),
        ("src/gen/pedido_pb.rs", "generated", &[("PedidoMessage", "")]),
        ("src/util/texto.rs", "", &[("normalizarTexto", "Tira o acento do texto do pedido.")]),
    ];

    /// O BM25F calculado documento a documento, sobre o mapa em memória, sem
    /// o banco: os campos de cada arquivo, as formas de cada palavra, e cada
    /// ocorrência contada no documento em que ela está.
    fn document_by_document(files: &[File<'_>], query: &str) -> Vec<(String, u64)> {
        let mut normalizer = Normalizer::new(&languages());
        let docs: Vec<(String, Vec<Vec<Vec<String>>>)> = files
            .iter()
            .filter(|(_, class, _)| class.is_empty())
            .map(|(path, _, decls)| {
                let names: Vec<&str> = decls.iter().map(|(name, _)| *name).collect();
                let notes: Vec<&str> = decls.iter().map(|(_, doc)| *doc).filter(|doc| !doc.is_empty()).collect();
                let fields = [names.join(" "), (*path).to_string(), notes.join(" ")];
                ((*path).to_string(), fields.iter().map(|text| normalizer.forms(text)).collect())
            })
            .collect();
        let count = docs.len();
        let avg_len: Vec<f64> =
            (0..3).map(|field| docs.iter().map(|(_, fields)| fields[field].len()).sum::<usize>() as f64 / count as f64).collect();
        let words: Vec<Vec<Vec<Posting>>> = normalizer
            .query(query)
            .iter()
            .map(|forms| {
                forms
                    .iter()
                    .map(|form| {
                        let mut postings = Vec::new();
                        for (doc, (_, fields)) in docs.iter().enumerate() {
                            for (field, words) in fields.iter().enumerate() {
                                let found = words.iter().filter(|word| word.contains(form)).count();
                                postings.extend(
                                    std::iter::repeat_n(Posting { doc: doc as i64, field, field_len: words.len() as u64 }, found),
                                );
                            }
                        }
                        postings
                    })
                    .collect()
            })
            .collect();
        let fields = Fields { docs: count, avg_len, weights: vec![1.0; 3] };
        bm25f(&words, &fields).into_iter().map(|(doc, score)| (docs[doc as usize].0.clone(), score_x1024(score))).collect()
    }

    #[test]
    fn the_list_from_one_query_per_form_equals_the_one_computed_document_by_document() {
        let dir = saved(LARGER);
        for query in ["buscar pedido cliente", "salvar os pedidos do cliente", "relatório mensal", "pagamento do pedido", "texto"] {
            let from_the_index = by_words_only(dir.path(), query);
            assert!(!from_the_index.is_empty(), "{query}");
            assert_eq!(from_the_index, document_by_document(LARGER, query), "{query}");
        }
    }

    #[test]
    fn a_three_letter_question_does_not_look_for_the_piece_of_a_name() {
        let dir = saved(&[("src/reader.rs", "", &[("readSessionSpecFile", "")])]);
        assert_eq!(paths(dir.path(), "ssi"), Vec::<String>::new());
        assert_eq!(paths(dir.path(), "ssio"), ["src/reader.rs"]);
    }

    #[test]
    fn a_single_word_found_only_inside_a_name_brings_that_name_first() {
        let dir = saved(&[
            ("src/sessionspec/list.rs", "", &[("list", "")]),
            ("src/reader.rs", "", &[("readSessionSpecFile", "")]),
        ]);
        let words: Vec<String> = by_words_only(dir.path(), "sessionspec").into_iter().map(|(path, _)| path).collect();
        assert_eq!(words, ["src/sessionspec/list.rs"], "the words alone never find the piece");
        assert_eq!(paths(dir.path(), "sessionspec"), ["src/reader.rs", "src/sessionspec/list.rs"]);
    }

    #[test]
    fn a_machine_written_file_is_not_found() {
        let dir = saved(&[("src/gen/pedido_pb.rs", "generated", &[("PedidoMessage", "")]), ("src/pedido.rs", "", &[("Pedido", "")])]);
        assert_eq!(paths(dir.path(), "pedido"), ["src/pedido.rs"]);
        assert_eq!(paths(dir.path(), "pedido message"), ["src/pedido.rs"]);
        assert_eq!(paths(dir.path(), "PedidoMessage"), ["src/pedido.rs"], "the piece never finds the machine-written name");
    }

    /// A letra com acento que a normalização não conhece, como o `š`, o
    /// índice grava sem o acento: a pergunta com ela e a pergunta sem ela
    /// acham o mesmo arquivo.
    #[test]
    fn a_letter_the_normalization_keeps_accented_is_found_as_the_index_wrote_it() {
        let dir = saved(&[("src/modelo.rs", "", &[("render", "šablona")]), ("src/cliente.rs", "", &[("salvar", "")])]);
        assert_eq!(paths(dir.path(), "šablona"), ["src/modelo.rs"]);
        assert_eq!(paths(dir.path(), "sablona"), ["src/modelo.rs"]);
    }

    /// A forma de letras minúsculas e algarismos do ASCII, que a pergunta não
    /// passa pelo tokenizador, é a que ele grava; a com outra letra, não.
    #[test]
    fn the_tokenizer_keeps_a_lowercase_ascii_form_and_folds_the_other_accents() {
        let dir = saved(LARGER);
        let db = open_existing(&model_path(dir.path())).unwrap();
        let words = |list: &[&[&str]]| -> Vec<Vec<String>> {
            list.iter().map(|forms| forms.iter().map(|form| form.to_string()).collect()).collect()
        };
        let plain = words(&[&["parse", "pars"], &["git2"], &["log"]]);
        assert_eq!(through_tokenizer(db.conn(), &plain).unwrap(), plain);
        assert_eq!(through_tokenizer(db.conn(), &words(&[&["šablona"], &["log"]])).unwrap(), words(&[&["sablona"], &["log"]]));
    }

    /// A pergunta passa pelo mesmo tokenizador das duas tabelas de palavras.
    #[test]
    fn the_question_goes_through_the_tokenizer_of_both_word_tables() {
        let dir = saved(LARGER);
        let db = open_existing(&model_path(dir.path())).unwrap();
        for table in [FILE_LEVEL.fts, DECL_LEVEL.fts] {
            let sql: String =
                db.conn().query_row("SELECT sql FROM sqlite_master WHERE name = ?1", [table], |row| row.get(0)).unwrap();
            assert!(sql.contains(&format!("tokenize='{TOKENIZER}'")), "{table}: {sql}");
        }
    }

    /// O índice nasce com a gravação do scan, nas línguas dela; o mapa escrito
    /// à mão fica sem ele, e a primeira busca o faz nas línguas dela.
    #[test]
    fn the_save_writes_the_index_and_a_hand_written_map_gets_it_at_the_first_search() {
        let dir = saved(LARGER);
        let db = open_existing(&model_path(dir.path())).unwrap();
        assert!(made_in(db.conn(), &languages()).unwrap());
        let names: i64 = db.conn().query_row("SELECT count(*) FROM decl_trigram", [], |row| row.get(0)).unwrap();
        assert_eq!(names, 8, "every declaration of a hand-written file");
        drop(db);

        let written = tempdir().unwrap();
        store::write_text(written.path(), &map_of(LARGER).to_string()).unwrap();
        let db = open_existing(&model_path(written.path())).unwrap();
        assert!(!made_in(db.conn(), &languages()).unwrap());
        drop(db);
        assert_eq!(paths(written.path(), "buscar pedido cliente"), paths(dir.path(), "buscar pedido cliente"));
        let db = open_existing(&model_path(written.path())).unwrap();
        assert!(made_in(db.conn(), &languages()).unwrap());
    }
}
