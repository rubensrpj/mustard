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
//! Depois desses, cada nível guarda o texto de dentro das peças, em campos
//! que a busca sem filtro não lê — a nota dela não muda com eles: da
//! declaração, a documentação inteira, os comentários, os nomes e as
//! chamadas escritos nas linhas dela; do arquivo, os comentários do começo e
//! os outros, juntados dos que o scan guardou fora das declarações e dos de
//! cada declaração de fora, que ele guarda uma vez só, nela.
//!
//! Cada texto passa pela normalização de toda busca (`domain::normalize`),
//! nas línguas do projeto, antes de entrar: o nome colado entra quebrado
//! (`parseGitLog` vira `parse git log`), e cada palavra entra com as suas
//! formas. O tamanho de cada campo, em palavras, mora numa tabela comum; as
//! línguas, o número de documentos e o tamanho médio de cada campo, em
//! `search_meta`. Os nomes das declarações entram inteiros numa tabela
//! trigram, para o pedaço do nome. O arquivo escrito por máquina fica fora
//! do índice. A declaração de teste — a de um arquivo de teste e a escrita
//! num trecho de teste de outro arquivo — entra na tabela trigram, que acha
//! o arquivo pelo pedaço do nome, mas não no nível das declarações. O mesmo
//! vale para o parâmetro escrito no cabeçalho do tipo dono, como o do
//! construtor primário do C#, que o scan grava com o tipo de declaração
//! próprio dele: a assinatura do dono já o traz.
//!
//! O glossário do mapa (`io::map_glossary`) entra nos dois níveis como mais
//! um campo, o das palavras aprendidas: a palavra da pergunta que uma edição
//! confirmada ligou a uma declaração conta nela e no arquivo dela. O
//! documento que só esse campo achou não passa à frente de nenhum que casa
//! pelo nome.
//!
//! Um terceiro nível, à parte, guarda os itens das specs (`io::map_specs`):
//! o título, o texto e as palavras de busca de cada um, com a
//! tabela própria de números, `spec_meta`. Ele muda quando uma spec muda,
//! e não quando o scan monta o mapa.
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
//!
//! A busca com filtro ([`candidates`]) devolve declarações, e não arquivos:
//! quatro listas do nível das declarações — a de base, a dos nomes, a de
//! todos os campos e a dos arquivos — juntadas por rodízio
//! (`domain::search::round_robin`) numa lista inteira, de onde saem os
//! primeiros até o teto. Na de todos os campos, os três campos dos textos
//! fixos contam como um só. Só as declarações do nível entram nas quatro: a
//! de teste nunca é candidata. Cada candidato leva o que o mapa guarda dele: o
//! dono, os membros, os comentários do corpo e os títulos dos commits mais
//! novos do arquivo. As ligações ([`links`]) dão ao corte do filtro os
//! métodos de cada tipo e as implementações de cada método de contrato.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::types::{Value as Sql, ValueRef};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row, Statement};
use serde::Deserialize;

use crate::domain::ast::is_test_path;
use crate::domain::normalize::{Languages, Normalizer};
use crate::domain::project_map::{
    outer_declarations, spec_sentence, Found, FoundItem, FoundText, MapRefusal, SPEC_SENTENCE_CHARS,
};
use crate::domain::map_filter::FilterCandidate;
use crate::domain::map_select::{Linked, Links};
use crate::domain::project_map::DeclAt;
use crate::domain::search::{
    bm25f, file_list, folded_name, name_list, name_words, ranked, round_robin, score_x1024, Fields, NameHits, Posting,
};
use crate::io::map_db::MapDb;
use crate::io::map_fill;
use crate::io::map_glossary::{self, Learned};
use crate::io::project_map::{model_path, open_existing, unreadable, MapBlock, SEARCHED};
use crate::platform::error::Result;

/// Um nível do índice: a tabela FTS5, a lista de cada forma por ela, a tabela
/// dos tamanhos, os campos que a busca sem filtro lê e, depois deles, os que
/// o índice guarda sem que ela os leia, na ordem das colunas.
struct Level {
    fts: &'static str,
    vocab: &'static str,
    lengths: &'static str,
    /// A tabela das línguas, do número de documentos e das médias.
    meta: &'static str,
    fields: &'static [&'static str],
    unread: &'static [&'static str],
    /// Como o nível lê as marcas do glossário; `None` no que não as lê.
    learned: Option<Learned>,
}

impl Level {
    /// Todas as colunas do nível: os campos lidos e, depois, os outros.
    fn columns(&self) -> impl Iterator<Item = &'static str> {
        self.fields.iter().chain(self.unread).copied()
    }
}

/// O nível dos arquivos: o que a busca devolve.
const FILE_LEVEL: Level = Level {
    fts: "file_fts",
    vocab: "file_vocab",
    lengths: "file_lengths",
    meta: "search_meta",
    fields: &["name", "path", "doc", "log", "error", "text"],
    unread: &["file_doc", "file_comment"],
    learned: Some(Learned::Files),
};

/// O nível das declarações.
const DECL_LEVEL: Level = Level {
    fts: "decl_fts",
    vocab: "decl_vocab",
    lengths: "decl_lengths",
    meta: "search_meta",
    fields: &["name", "path", "signature", "doc", "log", "error", "text"],
    unread: &["whole_doc", "body_comment", "body_names", "body_calls"],
    learned: Some(Learned::Decls),
};

/// O nível dos itens das specs (`io::map_specs`): o título, a parte do
/// usuário e as palavras de busca que a gravação calculou, cada um no seu
/// campo. As médias e as línguas moram numa tabela do bloco das specs, que a
/// montagem do mapa não esvazia.
const SPEC_LEVEL: Level = Level {
    fts: "spec_fts",
    vocab: "spec_vocab",
    lengths: "spec_lengths",
    meta: "spec_meta",
    fields: &["title", "text", "words"],
    unread: &[],
    learned: None,
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
/// do nome, o número do arquivo dela e se ela fica fora do nível das
/// declarações.
struct Decl {
    doc: Doc,
    name: String,
    file: i64,
    unlisted: bool,
}

/// Refaz o índice inteiro a partir das tabelas dos arquivos e das
/// declarações, com as palavras preparadas nas línguas `languages`. Roda na
/// transação de quem grava: o índice e as linhas de que ele sai entram
/// juntos. A declaração de teste e o parâmetro escrito no cabeçalho do dono
/// ficam fora do nível das declarações, de onde saem os candidatos da busca
/// com filtro, e entram na tabela trigram.
pub(crate) fn rebuild(conn: &Connection, languages: &Languages) -> Result<()> {
    let (files, decls) = documents(conn, &mut Normalizer::new(languages))?;
    forget(conn)?;
    fill(conn, &FILE_LEVEL, &files)?;
    fill(conn, &DECL_LEVEL, decls.iter().filter(|decl| !decl.unlisted).map(|decl| &decl.doc))?;
    {
        let mut insert = conn.prepare("INSERT INTO decl_trigram(rowid, name, folded, file) VALUES (?1, ?2, ?3, ?4)")?;
        for decl in &decls {
            insert.execute(params![decl.doc.id, decl.name, folded_name(&decl.name), decl.file])?;
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
/// e cada chamada são também de toda declaração cujas linhas os contêm: a
/// mensagem escrita num método conta para ele e para o tipo que o traz; a
/// declaração sem a última linha gravada cobre só a primeira. A documentação
/// inteira que o scan não guardou à parte é a mesma de `doc`. A declaração é
/// de teste quando o arquivo dela é de teste ou quando a primeira linha dela
/// cai num trecho de teste do arquivo. A de teste e o parâmetro escrito no
/// cabeçalho do dono ([`HEADER_PARAMETER_KIND`]) ficam fora do nível das
/// declarações.
fn documents(conn: &Connection, normalizer: &mut Normalizer) -> Result<(Vec<Doc>, Vec<Decl>)> {
    let mut files: Vec<Doc> = Vec::new();
    // As palavras que o arquivo já tem nos nomes, na documentação e em cada
    // campo dos textos.
    let mut seen: Vec<[HashSet<Vec<String>>; 5]> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();
    // De cada arquivo, se ele é de teste e os trechos de teste dele.
    let mut tests: Vec<(bool, Vec<(u64, u64)>)> = Vec::new();
    let mut stmt = conn.prepare("SELECT rowid, path, file_class, test_lines FROM files ORDER BY rowid")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        if !text(row, 2)?.is_empty() {
            continue;
        }
        let path = text(row, 1)?;
        tests.push((is_test_path(&path), serde_json::from_str(&text(row, 3)?).unwrap_or_default()));
        let words = normalizer.forms(&path);
        at.insert(path, files.len());
        let mut fields = vec![Vec::new(); FILE_LEVEL.columns().count()];
        fields[1] = words;
        files.push(Doc { id: row.get(0)?, fields });
        seen.push(Default::default());
    }
    let mut rows_of: Vec<DeclRow> = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT rowid, file, name, signature, doc, line, end_line, whole_doc, body_comment, body_names, kind \
         FROM decls ORDER BY rowid",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let Some(&owner) = at.get(&text(row, 1)?) else { continue };
        let line = |at: usize| -> Result<u64> { Ok(row.get::<_, Option<i64>>(at)?.unwrap_or(0).max(0) as u64) };
        let (test_file, test_lines) = &tests[owner];
        let first = line(5)?;
        rows_of.push(DeclRow {
            id: row.get(0)?,
            owner,
            unlisted: *test_file
                || test_lines.iter().any(|&(start, end)| (start..=end).contains(&first))
                || text(row, 10)? == HEADER_PARAMETER_KIND,
            name: text(row, 2)?,
            signature: text(row, 3)?,
            doc: text(row, 4)?,
            lines: (first, line(6)?),
            texts: vec![Vec::new(); TEXT_FIELDS.len()],
            whole_doc: text(row, 7)?,
            body_comment: text(row, 8)?,
            body_names: text(row, 9)?,
        });
    }
    // Os textos de cada arquivo: as palavras vão para o arquivo e para a
    // declaração dele que contém a linha.
    let mut decls_of: Vec<Vec<usize>> = vec![Vec::new(); files.len()];
    for (at, row) in rows_of.iter().enumerate() {
        decls_of[row.owner].push(at);
    }
    let comments = FILE_LEVEL.fields.len();
    for FileText { path, written, file_doc, file_comment, file_doc_in_body } in written_texts(conn)? {
        let Some(&owner) = at.get(&path) else { continue };
        files[owner].fields[comments] = normalizer.forms(&file_doc);
        files[owner].fields[comments + 1] =
            file_comments(normalizer, &file_comment, file_doc_in_body, &rows_of, &decls_of[owner]);
        for written in written {
            let words = normalizer.forms(&written.value);
            let field = written.field();
            add_new(&mut files[owner].fields[3 + field], &mut seen[owner][2 + field], &words);
            for &decl in &decls_of[owner] {
                let (first, last) = rows_of[decl].lines;
                if (first..=last.max(first)).contains(&written.line) {
                    rows_of[decl].texts[field].push(words.clone());
                }
            }
        }
    }
    let calls = written_calls(conn, &at, normalizer)?;
    let mut decls = Vec::new();
    for row in rows_of {
        let (name_words, doc_words) = (normalizer.forms(&row.name), normalizer.forms(&row.doc));
        let whole_doc = if row.whole_doc.is_empty() { doc_words.clone() } else { normalizer.forms(&row.whole_doc) };
        let file = &mut files[row.owner];
        add_new(&mut file.fields[0], &mut seen[row.owner][0], &name_words);
        add_new(&mut file.fields[2], &mut seen[row.owner][1], &doc_words);
        let mut fields = vec![name_words, file.fields[1].clone(), normalizer.forms(&row.signature), doc_words];
        for texts in row.texts {
            let mut words: Words = Vec::new();
            add_new(&mut words, &mut HashSet::new(), &texts.concat());
            fields.push(words);
        }
        let mut called: Words = Vec::new();
        let (first, last) = row.lines;
        let file_calls = calls.get(row.owner).map(Vec::as_slice).unwrap_or_default();
        let start = file_calls.partition_point(|(line, _)| *line < first);
        let end = file_calls.partition_point(|(line, _)| *line <= last.max(first));
        let mut seen_calls = HashSet::new();
        for (_, words) in &file_calls[start..end.max(start)] {
            add_new(&mut called, &mut seen_calls, words);
        }
        fields.extend([whole_doc, normalizer.forms(&row.body_comment), normalizer.forms(&row.body_names), called]);
        decls.push(Decl { doc: Doc { id: row.id, fields }, name: row.name, file: file.id, unlisted: row.unlisted });
    }
    Ok((files, decls))
}

/// As chamadas de cada arquivo do índice, pela posição dele em `at`, cada
/// uma com a linha e as palavras dela — o qualificador e o nome, separados —,
/// em ordem de linha. A chamada que não se lê como `nome:linha` fica de fora.
fn written_calls(
    conn: &Connection,
    at: &HashMap<String, usize>,
    normalizer: &mut Normalizer,
) -> Result<Vec<Vec<(u64, Words)>>> {
    let mut out: Vec<Vec<(u64, Words)>> = vec![Vec::new(); at.len()];
    let mut stmt = conn.prepare("SELECT path, calls FROM links ORDER BY rowid")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let Some(&owner) = at.get(&text(row, 0)?) else { continue };
        let sites: Vec<String> = serde_json::from_str(&text(row, 1)?).unwrap_or_default();
        let calls = &mut out[owner];
        for site in sites {
            let Some((head, line)) = site.rsplit_once(':') else { continue };
            let Ok(line) = line.parse::<u64>() else { continue };
            calls.push((line, normalizer.forms(&head.replace('.', " "))));
        }
        calls.sort_by_key(|(line, _)| *line);
    }
    Ok(out)
}

/// O tipo de declaração que o scan dá ao parâmetro escrito no cabeçalho do
/// tipo dono, como o do construtor primário do C#. A assinatura do dono já o
/// traz, e ele à parte só repetiria, entre os candidatos, o nome do
/// parâmetro — também quando o teto da assinatura do dono o cortou.
const HEADER_PARAMETER_KIND: &str = "parameter";

/// Uma declaração lida do mapa para o índice: o número dela, o arquivo, o
/// nome, a assinatura, a documentação, a primeira e a última linha, e as
/// palavras dos textos fixos que caem nela, por campo.
struct DeclRow {
    id: i64,
    owner: usize,
    /// A declaração fica fora do nível das declarações: é de teste, ou é o
    /// parâmetro escrito no cabeçalho do dono.
    unlisted: bool,
    name: String,
    signature: String,
    doc: String,
    lines: (u64, u64),
    texts: Vec<Vec<Words>>,
    /// A documentação inteira, quando o teto de `doc` a cortou.
    whole_doc: String,
    body_comment: String,
    body_names: String,
}

/// Acrescenta a `into` cada palavra de `words` que `seen` ainda não tem.
fn add_new(into: &mut Words, seen: &mut HashSet<Vec<String>>, words: &Words) {
    for word in words {
        if seen.insert(word.clone()) {
            into.push(word.clone());
        }
    }
}

/// As palavras dos comentários de um arquivo fora os do começo, cada uma
/// uma vez, como as de todo campo: as dos que o scan guardou fora de toda
/// declaração (`outside`) e as dos comentários de cada declaração de fora
/// dele (`of`, pela regra de [`outer_declarations`]) — os de uma declaração
/// de dentro estão também nos da que a contém. Da primeira declaração de
/// fora, os `doc_in_body` bytes do começo são comentários do começo do
/// arquivo e ficam de fora.
fn file_comments(
    normalizer: &mut Normalizer,
    outside: &str,
    doc_in_body: usize,
    rows: &[DeclRow],
    of: &[usize],
) -> Words {
    let mut words = normalizer.forms(outside);
    let mut seen: HashSet<Vec<String>> = words.iter().cloned().collect();
    let lines: Vec<(usize, usize)> = of.iter().map(|&at| (rows[at].lines.0 as usize, rows[at].lines.1 as usize)).collect();
    for (nth, outer) in outer_declarations(&lines).into_iter().enumerate() {
        let body = rows[of[outer]].body_comment.as_str();
        let body = if nth == 0 { body.get(doc_in_body..).unwrap_or_default() } else { body };
        add_new(&mut words, &mut seen, &normalizer.forms(body));
    }
    words
}

/// O texto de um arquivo como o scan o gravou: os textos fixos, os
/// comentários do começo, os outros que caem fora das declarações e quantos
/// bytes do começo dos comentários da primeira declaração de fora são do
/// começo do arquivo.
struct FileText {
    path: String,
    written: Vec<Written>,
    file_doc: String,
    file_comment: String,
    file_doc_in_body: usize,
}

/// O texto de cada arquivo. O arquivo cuja coluna dos textos fixos não se lê
/// fica sem eles.
fn written_texts(conn: &Connection) -> Result<Vec<FileText>> {
    let mut stmt =
        conn.prepare("SELECT path, texts, file_doc, file_comment, file_doc_in_body FROM texts ORDER BY rowid")?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(FileText {
            path: text(row, 0)?,
            written: serde_json::from_str(&text(row, 1)?).unwrap_or_default(),
            file_doc: text(row, 2)?,
            file_comment: text(row, 3)?,
            file_doc_in_body: row.get::<_, Option<i64>>(4)?.unwrap_or(0).max(0) as usize,
        });
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
/// médio de cada campo na tabela de números do nível.
fn fill<'d>(conn: &Connection, level: &Level, docs: impl IntoIterator<Item = &'d Doc>) -> Result<()> {
    let names: Vec<&str> = level.columns().collect();
    let columns = names.join(", ");
    let slots: Vec<String> = (2..=names.len() + 1).map(|at| format!("?{at}")).collect();
    let slots = slots.join(", ");
    let mut words = conn.prepare(&format!("INSERT INTO {}(rowid, {columns}) VALUES (?1, {slots})", level.fts))?;
    let mut lengths = conn.prepare(&format!("INSERT INTO {}(id, {columns}) VALUES (?1, {slots})", level.lengths))?;
    let mut total = vec![0u64; names.len()];
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
    let mut meta = conn.prepare(&format!("INSERT OR REPLACE INTO {}(key, value) VALUES (?1, ?2)", level.meta))?;
    meta.execute(params![format!("{}.docs", level.fts), count as i64])?;
    for (field, name) in names.iter().enumerate() {
        let avg = if count == 0 { 0.0 } else { total[field] as f64 / count as f64 };
        meta.execute(params![format!("{}.{name}", level.fts), avg])?;
    }
    Ok(())
}

/// Os arquivos que mais casam com a pergunta `query` no mapa do projeto em
/// `root`, até `limit`, com as palavras cortadas nas línguas `languages`:
/// primeiro os que o pedaço do nome acha, depois os das palavras, da nota
/// mais alta para a mais baixa. As recusas são as de todo leitor do mapa —
/// sem o arquivo, [`MapRefusal::MapMissing`] — e, com um bloco de que o
/// índice lê ainda vazio depois de uma troca de formato,
/// [`MapRefusal::MapUnfilled`].
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
    let db = indexed(model, languages, &SEARCHED)?;
    found(db.conn(), query, languages, limit).map_err(unreadable)
}

/// O banco em `model`, com o índice feito nas línguas `languages`: o feito
/// em outras se refaz antes da busca. O bloco de `read`, os que a busca lê,
/// que o scan ainda não encheu depois de uma troca de formato recusa a
/// busca, que sem ele responderia vazio: a mesma recusa das outras perguntas
/// ao mapa ([`map_fill::refuse`]).
fn indexed(model: &Path, languages: &Languages, read: &[&MapBlock]) -> std::result::Result<MapDb, MapRefusal> {
    let mut db = open_existing(model)?;
    map_fill::refuse(&db, read)?;
    if !made_in(db.conn(), languages).map_err(unreadable)? {
        db.write(|tx| if made_in(tx, languages)? { Ok(()) } else { rebuild(tx, languages) }).map_err(unreadable)?;
    }
    Ok(db)
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
    let by_words = by_words(conn, &FILE_LEVEL, FILE_LEVEL.fields, &words)?;
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
/// com as suas formas, contando só os campos `fields` do nível, cada um à
/// parte: a lista de cada forma e o tamanho dos campos de cada documento dela
/// saem numa consulta só.
fn by_words(conn: &Connection, level: &Level, fields: &[&str], words: &[Vec<String>]) -> Result<Vec<(i64, f64)>> {
    by_words_as(conn, level, fields, Texts::Apart, words)
}

/// Como os campos dos textos fixos entram na conta: cada marca no seu campo,
/// ou as três num campo só, com o tamanho e a média somados.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Texts {
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
    let indexed = as_indexed(conn, words)?;
    let sizes: Vec<String> = fields.iter().map(|field| format!("l.{field}")).collect();
    let mut lists = conn.prepare(&format!(
        "SELECT v.doc, v.col, {} FROM {} v JOIN {} l ON l.id = v.doc WHERE v.term = ?1",
        sizes.join(", "),
        level.vocab,
        level.lengths
    ))?;
    let slots = slots(fields, texts);
    let mut postings = indexed
        .iter()
        .map(|forms| forms.iter().map(|form| postings(&mut lists, fields, &slots, form)).collect::<Result<Vec<_>>>())
        .collect::<Result<Vec<_>>>()?;
    let mut numbers = fields_of(conn, level, fields, &slots)?;
    let Some(learned) = level.learned else { return Ok(bm25f(&postings, &numbers)) };
    let marked = map_glossary::marked(conn, learned, words)?;
    if marked.iter().all(Vec::is_empty) {
        return Ok(bm25f(&postings, &numbers));
    }
    let learned_slot = numbers.avg_len.len();
    numbers.avg_len.push(1.0);
    numbers.weights.push(LEARNED_WEIGHT);
    for (forms, docs) in postings.iter_mut().zip(&marked) {
        let extra: Vec<Posting> = docs.iter().map(|&doc| Posting { doc, field: learned_slot, field_len: 1 }).collect();
        if extra.is_empty() {
            continue;
        }
        if forms.is_empty() {
            forms.push(extra);
        } else {
            for list in forms.iter_mut() {
                list.extend(extra.iter().copied());
            }
        }
    }
    let name_slot = fields.iter().position(|field| *field == "name").map(|at| slots[at]);
    Ok(behind_the_names(bm25f(&postings, &numbers), &postings, name_slot, learned_slot))
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
    for (name, slot) in fields.iter().zip(slots) {
        avg_len[*slot] += number(format!("{}.{name}", level.fts))?;
    }
    Ok(Fields { docs, avg_len, weights: vec![FIELD_WEIGHT; count] })
}

/// `true` quando o índice dos itens das specs foi feito nas línguas
/// `languages`.
pub(crate) fn specs_indexed_in(conn: &Connection, languages: &Languages) -> Result<bool> {
    let stored: Option<String> = conn
        .query_row(&format!("SELECT value FROM {} WHERE key = ?1", SPEC_LEVEL.meta), [LANGUAGES_KEY], |row| row.get(0))
        .optional()?;
    Ok(stored.is_some_and(|stored| stored == languages.codes().join(",")))
}

/// Tira do índice dos itens das specs os documentos `ids`, os das linhas que
/// saíram.
pub(crate) fn unindex_specs(conn: &Connection, ids: &[i64]) -> Result<()> {
    let mut words = conn.prepare(&format!("DELETE FROM {} WHERE rowid = ?1", SPEC_LEVEL.fts))?;
    let mut lengths = conn.prepare(&format!("DELETE FROM {} WHERE id = ?1", SPEC_LEVEL.lengths))?;
    for id in ids {
        words.execute([id])?;
        lengths.execute([id])?;
    }
    Ok(())
}

/// Põe no índice dos itens das specs as linhas da spec `spec` — de todas,
/// sem ela —, com as palavras nas línguas `languages`, e refaz a contagem e
/// as médias do nível pela tabela dos tamanhos. Roda na transação de quem
/// grava as linhas.
pub(crate) fn index_specs(conn: &Connection, languages: &Languages, spec: Option<&str>) -> Result<()> {
    let mut normalizer = Normalizer::new(languages);
    let docs: Vec<Doc> = {
        let filter = if spec.is_some() { "WHERE spec = ?1" } else { "" };
        let mut stmt = conn.prepare(&format!("SELECT rowid, title, text, search FROM spec_items {filter} ORDER BY rowid"))?;
        let mut rows = match spec {
            Some(spec) => stmt.query([spec])?,
            None => stmt.query([])?,
        };
        let mut docs = Vec::new();
        while let Some(row) = rows.next()? {
            let fields = (1..=3).map(|at| Ok(normalizer.forms(&text(row, at)?))).collect::<Result<Vec<Words>>>()?;
            docs.push(Doc { id: row.get(0)?, fields });
        }
        docs
    };
    let names: Vec<&str> = SPEC_LEVEL.columns().collect();
    let slots: Vec<String> = (2..=names.len() + 1).map(|at| format!("?{at}")).collect();
    let (columns, slots) = (names.join(", "), slots.join(", "));
    let mut words = conn.prepare(&format!("INSERT INTO {}(rowid, {columns}) VALUES (?1, {slots})", SPEC_LEVEL.fts))?;
    let mut lengths = conn.prepare(&format!("INSERT INTO {}(id, {columns}) VALUES (?1, {slots})", SPEC_LEVEL.lengths))?;
    for doc in &docs {
        let mut texts = vec![Sql::Integer(doc.id)];
        let mut sizes = vec![Sql::Integer(doc.id)];
        for prepared in &doc.fields {
            sizes.push(Sql::Integer(prepared.len() as i64));
            texts.push(Sql::Text(prepared.iter().flatten().map(String::as_str).collect::<Vec<_>>().join(" ")));
        }
        words.execute(params_from_iter(texts))?;
        lengths.execute(params_from_iter(sizes))?;
    }
    let averages: Vec<String> = names.iter().map(|name| format!("coalesce(avg({name}), 0)")).collect();
    let (count, means) = conn.query_row(
        &format!("SELECT count(*), {} FROM {}", averages.join(", "), SPEC_LEVEL.lengths),
        [],
        |row| Ok((row.get::<_, i64>(0)?, (1..=names.len()).map(|at| row.get::<_, f64>(at)).collect::<rusqlite::Result<Vec<f64>>>()?)),
    )?;
    let mut meta = conn.prepare(&format!("INSERT OR REPLACE INTO {}(key, value) VALUES (?1, ?2)", SPEC_LEVEL.meta))?;
    meta.execute(params![format!("{}.docs", SPEC_LEVEL.fts), count])?;
    for (name, mean) in names.iter().zip(means) {
        meta.execute(params![format!("{}.{name}", SPEC_LEVEL.fts), mean])?;
    }
    meta.execute(params![LANGUAGES_KEY, languages.codes().join(",")])?;
    Ok(())
}

/// Refaz o índice inteiro dos itens das specs nas línguas `languages`.
fn reindex_specs(conn: &Connection, languages: &Languages) -> Result<()> {
    conn.execute_batch(&format!(
        "INSERT INTO {fts}({fts}) VALUES ('delete-all'); DELETE FROM {lengths}; DELETE FROM {meta};",
        fts = SPEC_LEVEL.fts,
        lengths = SPEC_LEVEL.lengths,
        meta = SPEC_LEVEL.meta
    ))?;
    index_specs(conn, languages, None)?;
    conn.execute(&format!("INSERT INTO {fts}({fts}) VALUES ('optimize')", fts = SPEC_LEVEL.fts), [])?;
    Ok(())
}

/// Os itens das specs que mais casam com a pergunta `query` no mapa do
/// projeto em `root`, até `limit`, da nota mais alta para a mais baixa, cada
/// um com a spec, o código, o título, a linha da parte do usuário que mais
/// casa e até `limit` lugares ligados a ele. Lê só o mapa: nenhum arquivo de
/// spec se abre. As recusas são as de [`search`].
pub fn search_specs(root: &Path, query: &str, languages: &Languages, limit: usize) -> std::result::Result<Vec<FoundItem>, MapRefusal> {
    search_specs_at(&model_path(root), query, languages, limit)
}

/// A busca de [`search_specs`] no mapa gravado em `model`.
pub fn search_specs_at(
    model: &Path,
    query: &str,
    languages: &Languages,
    limit: usize,
) -> std::result::Result<Vec<FoundItem>, MapRefusal> {
    let mut db = open_existing(model)?;
    if !specs_indexed_in(db.conn(), languages).map_err(unreadable)? {
        db.write(|tx| if specs_indexed_in(tx, languages)? { Ok(()) } else { reindex_specs(tx, languages) })
            .map_err(unreadable)?;
    }
    found_items(db.conn(), query, languages, limit).map_err(unreadable)
}

fn found_items(conn: &Connection, query: &str, languages: &Languages, limit: usize) -> Result<Vec<FoundItem>> {
    let mut normalizer = Normalizer::new(languages);
    let words = normalizer.query(query);
    let mut item = conn.prepare("SELECT spec, id, code, title, text, files FROM spec_items WHERE rowid = ?1")?;
    let mut out = Vec::new();
    for (rowid, _) in by_words(conn, &SPEC_LEVEL, SPEC_LEVEL.fields, &words)?.into_iter().take(limit) {
        let Some((spec, id, code, title, body, files)) = item
            .query_row([rowid], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    row.get::<_, Option<String>>(5)?,
                ))
            })
            .optional()?
        else {
            continue;
        };
        let files: Vec<String> = files.and_then(|files| serde_json::from_str(&files).ok()).unwrap_or_default();
        let title = if title.trim().is_empty() { spec_sentence(&body) } else { title.trim().to_string() };
        out.push(FoundItem {
            links: crate::io::map_specs::links_of(conn, &spec, id, &files, limit)?,
            line: best_line(&body, &words, &mut normalizer),
            spec,
            code,
            title,
        });
    }
    Ok(out)
}

/// A linha de `body` que tem mais palavras da pergunta, por alguma das
/// formas, até o teto da frase de um item; empatadas, a mais de cima.
/// `None` quando nenhuma tem nenhuma.
fn best_line(body: &str, words: &[Vec<String>], normalizer: &mut Normalizer) -> Option<String> {
    let mut best: Option<(usize, &str)> = None;
    for line in body.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let forms: HashSet<String> = normalizer.forms(line).into_iter().flatten().collect();
        let hits = words.iter().filter(|word| word.iter().any(|form| forms.contains(form))).count();
        if hits > 0 && best.is_none_or(|(most, _)| hits > most) {
            best = Some((hits, line));
        }
    }
    best.map(|(_, line)| crate::domain::spec_index::cut(line, SPEC_SENTENCE_CHARS))
}

// ---------------------------------------------------------------------------
// Os candidatos da busca com filtro
// ---------------------------------------------------------------------------

/// Os campos da declaração que a lista de base lê: o nome, o caminho, a
/// assinatura e a documentação.
const BASE_FIELDS: &[&str] = &["name", "path", "signature", "doc"];

/// Quantos títulos de commit do arquivo vão com cada candidato, os mais
/// novos.
const FILE_COMMITS: usize = 3;

/// Os tipos de declaração que, entre os membros de um tipo, são métodos.
const METHOD_KINDS: [&str; 2] = ["function", "method"];

/// Os candidatos da busca com filtro: a lista inteira do rodízio e os
/// primeiros dela, até o teto, cada um com o que o mapa guarda dele.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterCandidates {
    /// A lista inteira: as quatro listas juntadas por rodízio, sem teto.
    pub whole: Vec<i64>,
    /// Os primeiros da lista inteira, na ordem dela.
    pub candidates: Vec<FilterCandidate>,
}

/// Os candidatos do filtro no mapa do projeto em `root`: as palavras de
/// `query` e a frase de `intent`, com as palavras cortadas nas línguas
/// `languages`, até `limit` candidatos. As recusas são as de [`search`] e,
/// como os candidatos levam os títulos dos commits do arquivo, também a da
/// história da base ainda vazia depois de uma troca de formato
/// ([`map_fill::READ_BY_CANDIDATES`]).
pub fn candidates(
    root: &Path,
    query: &str,
    intent: &str,
    languages: &Languages,
    limit: usize,
) -> std::result::Result<FilterCandidates, MapRefusal> {
    candidates_at(&model_path(root), query, intent, languages, limit)
}

/// Os candidatos de [`candidates`] no mapa gravado em `model`.
pub fn candidates_at(
    model: &Path,
    query: &str,
    intent: &str,
    languages: &Languages,
    limit: usize,
) -> std::result::Result<FilterCandidates, MapRefusal> {
    let db = indexed(model, languages, &map_fill::READ_BY_CANDIDATES)?;
    let whole = whole_list(db.conn(), query, intent, languages).map_err(unreadable)?;
    let first: Vec<i64> = whole.iter().take(limit).copied().collect();
    let candidates = declarations_in(db.conn(), &first).map_err(unreadable)?;
    Ok(FilterCandidates { whole, candidates })
}

/// As declarações `ids` do mapa do projeto em `root`, na ordem pedida, com
/// o que o mapa guarda de cada uma; o id que o mapa não tem fica de fora.
pub fn declarations(root: &Path, ids: &[i64]) -> std::result::Result<Vec<FilterCandidate>, MapRefusal> {
    let db = open_existing(&model_path(root))?;
    declarations_in(db.conn(), ids).map_err(unreadable)
}

/// As ligações das declarações `ids` no mapa do projeto em `root`: o tipo,
/// o caminho, os métodos de cada tipo e as implementações de cada método de
/// contrato.
pub fn links(root: &Path, ids: &[i64]) -> std::result::Result<Links, MapRefusal> {
    let db = open_existing(&model_path(root))?;
    links_in(db.conn(), ids).map_err(unreadable)
}

/// A lista inteira: o rodízio das listas de base, dos nomes, de tudo e dos
/// arquivos, nesta ordem. As listas de palavras leem a `query` seguida da
/// `intent`; a dos nomes, só as palavras da `query`. Na lista de tudo, os
/// textos fixos da declaração contam como um campo só, como no laboratório
/// que afinou a busca com filtro: cada marca num campo à parte dava ao texto
/// de erro, que quase nenhuma declaração tem, uma média perto de zero, e a
/// palavra dele quase não pesava. A pergunta leva só a raiz da primeira
/// língua ([`Normalizer::query_in_text_language`]), como no laboratório.
fn whole_list(conn: &Connection, query: &str, intent: &str, languages: &Languages) -> Result<Vec<i64>> {
    let words = Normalizer::new(languages).query_in_text_language(format!("{query} {intent}").trim());
    let base = base_list(conn, &words)?;
    let every_decl_field: Vec<&str> = DECL_LEVEL.columns().collect();
    let everything = by_words_as(conn, &DECL_LEVEL, &every_decl_field, Texts::Together, &words)?;
    let every_file_field: Vec<&str> = FILE_LEVEL.columns().collect();
    let file_scores: HashMap<i64, f64> = by_words(conn, &FILE_LEVEL, &every_file_field, &words)?.into_iter().collect();
    let base_scores: HashMap<i64, f64> = base.iter().copied().collect();
    let names = name_list(&name_hits(conn, query)?, fields_of(conn, &DECL_LEVEL, &[], &[])?.docs);
    let files = file_list(&decl_files(conn)?, &file_scores, &base_scores);
    let ids = |list: Vec<(i64, f64)>| list.into_iter().map(|(id, _)| id).collect::<Vec<_>>();
    Ok(round_robin(&[ids(base), ids(names), ids(everything), files]))
}

/// A lista de base: o BM25F no nível das declarações, sobre o nome, o
/// caminho, a assinatura e a documentação.
fn base_list(conn: &Connection, words: &[Vec<String>]) -> Result<Vec<(i64, f64)>> {
    by_words(conn, &DECL_LEVEL, BASE_FIELDS, words)
}

/// De cada palavra de nome da `query`, as declarações do nível cujo nome
/// dobrado a contém, pela tabela trigram.
fn name_hits(conn: &Connection, query: &str) -> Result<Vec<NameHits>> {
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
fn decl_files(conn: &Connection) -> Result<Vec<(i64, i64)>> {
    let mut stmt = conn.prepare("SELECT l.id, t.file FROM decl_lengths l JOIN decl_trigram t ON t.rowid = l.id")?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Uma declaração como a tabela a guarda, com as ligações ainda em texto.
struct Stored {
    candidate: FilterCandidate,
    owner: Vec<String>,
    members: Vec<DeclAt>,
    implemented_by: Vec<DeclAt>,
}

/// As declarações `ids`, na ordem pedida, como a tabela as guarda.
fn stored(conn: &Connection, ids: &[i64]) -> Result<Vec<Stored>> {
    let mut stmt = conn.prepare(
        "SELECT file, kind, name, line, end_line, signature, doc, body_comment, owner, contract, members, implemented_by \
         FROM decls WHERE rowid = ?1",
    )?;
    let list = |row: &Row<'_>, at: usize| -> Result<Vec<String>> {
        Ok(serde_json::from_str(&text(row, at)?).unwrap_or_default())
    };
    let places = |row: &Row<'_>, at: usize| -> Result<Vec<DeclAt>> {
        Ok(serde_json::from_str(&text(row, at)?).unwrap_or_default())
    };
    let mut out = Vec::new();
    for &id in ids {
        let mut rows = stmt.query([id])?;
        let Some(row) = rows.next()? else { continue };
        let line = |at: usize| -> Result<u32> { Ok(u32::try_from(row.get::<_, Option<i64>>(at)?.unwrap_or(0)).unwrap_or(0)) };
        let mut owner = list(row, 8)?;
        owner.extend(list(row, 9)?);
        out.push(Stored {
            candidate: FilterCandidate {
                id,
                kind: text(row, 1)?,
                name: text(row, 2)?,
                path: text(row, 0)?,
                line: line(3)?,
                end_line: line(4)?,
                signature: text(row, 5)?,
                documentation: text(row, 6)?,
                owner: String::new(),
                members: Vec::new(),
                body_comments: text(row, 7)?,
                file_commits: Vec::new(),
            },
            owner,
            members: places(row, 10)?,
            implemented_by: places(row, 11)?,
        });
    }
    Ok(out)
}

/// As declarações `ids`, na ordem pedida, com o dono e o contrato em texto,
/// os membros (os métodos com `()` no fim) e os títulos dos commits mais
/// novos do arquivo.
fn declarations_in(conn: &Connection, ids: &[i64]) -> Result<Vec<FilterCandidate>> {
    let stored = stored(conn, ids)?;
    let kinds = kinds_of(conn, stored.iter().flat_map(|decl| &decl.members))?;
    let paths: HashSet<&str> = stored.iter().map(|decl| decl.candidate.path.as_str()).collect();
    let titles = newest_titles(conn, &paths)?;
    Ok(stored
        .iter()
        .map(|decl| {
            let mut candidate = decl.candidate.clone();
            candidate.owner = decl.owner.join(" ");
            candidate.members = decl
                .members
                .iter()
                .map(|member| match kinds.get(member) {
                    Some((_, kind)) if METHOD_KINDS.contains(&kind.as_str()) => format!("{}()", member.name),
                    _ => member.name.clone(),
                })
                .collect();
            candidate.file_commits = titles.get(candidate.path.as_str()).cloned().unwrap_or_default();
            candidate
        })
        .collect())
}

/// As ligações de cada declaração de `ids` que o mapa tem. A implementação
/// entra só quando pode ser candidata: a de teste, como o dublê escrito num
/// arquivo de teste ou no trecho de teste de outro arquivo, fica de fora,
/// pela mesma regra que a tira dos candidatos.
fn links_in(conn: &Connection, ids: &[i64]) -> Result<Links> {
    let stored = stored(conn, ids)?;
    let kinds = kinds_of(conn, stored.iter().flat_map(|decl| decl.members.iter().chain(&decl.implemented_by)))?;
    let implementations = stored.iter().flat_map(|decl| &decl.implemented_by).filter_map(|place| kinds.get(place));
    let eligible = in_decl_level(conn, implementations.map(|(id, _)| *id))?;
    Ok(stored
        .into_iter()
        .map(|decl| {
            let methods = decl
                .members
                .iter()
                .filter_map(|member| kinds.get(member))
                .filter(|(_, kind)| METHOD_KINDS.contains(&kind.as_str()))
                .map(|(id, _)| *id)
                .collect();
            let implementations = decl
                .implemented_by
                .iter()
                .filter_map(|place| kinds.get(place).map(|(id, _)| (*id, place.file.clone())))
                .filter(|(id, _)| eligible.contains(id))
                .collect();
            let linked =
                Linked { kind: decl.candidate.kind, path: decl.candidate.path, methods, implementations };
            (decl.candidate.id, linked)
        })
        .collect())
}

/// Das declarações `ids`, as que estão no nível das declarações do índice,
/// de onde saem os candidatos da busca com filtro.
fn in_decl_level(conn: &Connection, ids: impl Iterator<Item = i64>) -> Result<HashSet<i64>> {
    let ids: Vec<i64> = ids.collect::<HashSet<_>>().into_iter().collect();
    if ids.is_empty() {
        return Ok(HashSet::new());
    }
    let slots: Vec<String> = (1..=ids.len()).map(|at| format!("?{at}")).collect();
    let mut stmt = conn.prepare(&format!("SELECT id FROM decl_lengths WHERE id IN ({})", slots.join(", ")))?;
    let rows = stmt.query_map(params_from_iter(&ids), |row| row.get::<_, i64>(0))?;
    Ok(rows.collect::<rusqlite::Result<HashSet<_>>>()?)
}

/// O id e o tipo de cada declaração apontada por `places`, lidos numa
/// passada só pelos arquivos delas.
fn kinds_of<'p>(conn: &Connection, places: impl Iterator<Item = &'p DeclAt>) -> Result<HashMap<DeclAt, (i64, String)>> {
    let wanted: HashSet<&DeclAt> = places.collect();
    let files: Vec<&str> = wanted.iter().map(|place| place.file.as_str()).collect::<HashSet<_>>().into_iter().collect();
    let mut out = HashMap::new();
    if files.is_empty() {
        return Ok(out);
    }
    let slots: Vec<String> = (1..=files.len()).map(|at| format!("?{at}")).collect();
    let mut stmt =
        conn.prepare(&format!("SELECT rowid, file, line, name, kind FROM decls WHERE file IN ({})", slots.join(", ")))?;
    let mut rows = stmt.query(params_from_iter(&files))?;
    while let Some(row) = rows.next()? {
        let place = DeclAt {
            file: text(row, 1)?,
            line: usize::try_from(row.get::<_, Option<i64>>(2)?.unwrap_or(0)).unwrap_or(0),
            name: text(row, 3)?,
        };
        if wanted.contains(&place) {
            out.entry(place).or_insert((row.get(0)?, text(row, 4)?));
        }
    }
    Ok(out)
}

/// Os títulos dos [`FILE_COMMITS`] commits mais novos que mudaram cada
/// arquivo de `paths`, do mais novo ao mais velho. O mapa sem a história do
/// git não dá título nenhum.
fn newest_titles(conn: &Connection, paths: &HashSet<&str>) -> Result<HashMap<String, Vec<String>>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    let mut stmt = conn.prepare("SELECT path FROM history_paths ORDER BY rowid")?;
    let listed = stmt.query_map([], |row| row.get::<_, Option<String>>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let wanted: HashMap<usize, String> = listed
        .into_iter()
        .enumerate()
        .filter_map(|(at, path)| path.filter(|path| paths.contains(path.as_str())).map(|path| (at, path)))
        .collect();
    if wanted.is_empty() {
        return Ok(out);
    }
    let mut stmt = conn.prepare("SELECT title, added, changed FROM commits ORDER BY at DESC, rowid")?;
    let mut rows = stmt.query([])?;
    let mut full = 0;
    while let Some(row) = rows.next()? {
        let title = text(row, 0)?;
        if title.trim().is_empty() {
            continue;
        }
        let touched: HashSet<usize> = [1, 2]
            .into_iter()
            .map(|at| Ok(serde_json::from_str::<Vec<usize>>(&text(row, at)?).unwrap_or_default()))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();
        for path in touched.iter().filter_map(|at| wanted.get(at)) {
            let titles = out.entry(path.clone()).or_default();
            if titles.len() < FILE_COMMITS {
                titles.push(title.clone());
                if titles.len() == FILE_COMMITS {
                    full += 1;
                }
            }
        }
        if full == wanted.len() {
            break;
        }
    }
    Ok(out)
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
    use crate::domain::search::{CANDIDATES, TOP};
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
        by_words(db.conn(), &FILE_LEVEL, FILE_LEVEL.fields, &words)
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

    // -- os candidatos da busca com filtro -----------------------------------

    /// Um projeto com o mapa `map`, em JSON, gravado pela porta.
    fn saved_json(map: &Value) -> TempDir {
        let dir = tempdir().unwrap();
        store::save_at(&model_path(dir.path()), map, "scan 1", &languages()).unwrap();
        dir
    }

    /// O id da declaração `name` no mapa.
    fn id_of(dir: &Path, name: &str) -> i64 {
        let db = open_existing(&model_path(dir)).unwrap();
        db.conn().query_row("SELECT rowid FROM decls WHERE name = ?1", [name], |row| row.get(0)).unwrap()
    }

    #[test]
    fn a_word_only_in_a_signature_puts_that_declaration_in_the_base_list() {
        let dir = saved_json(&json!({"modules": [
            {"path": "src/relogio.rs", "declarations": [
                {"kind": "function", "name": "agora", "line": 1, "signature": "pub fn agora() -> Timestamp"}]},
            {"path": "src/pedido.rs", "declarations": [
                {"kind": "function", "name": "gravar", "line": 1, "signature": "pub fn gravar()"}]}
        ]}));
        let db = open_existing(&model_path(dir.path())).unwrap();
        let words = Normalizer::new(&languages()).query("timestamp");
        let base: Vec<i64> = base_list(db.conn(), &words).unwrap().into_iter().map(|(id, _)| id).collect();
        assert_eq!(base, vec![id_of(dir.path(), "agora")]);
    }

    #[test]
    fn a_fresh_map_has_no_case_free_name_index() {
        let dir = saved(LARGER);
        let db = open_existing(&model_path(dir.path())).unwrap();
        let found: i64 = db
            .conn()
            .query_row("SELECT count(*) FROM sqlite_master WHERE name = 'decls_name_nocase'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(found, 0);
    }

    /// O mapa de um contrato e da implementação dele, em arquivos separados,
    /// com a história de quatro commits.
    fn contract_map() -> Value {
        json!({
          "modules": [
            {"path": "src/pay/port.rs", "declarations": [
              {"kind": "trait", "name": "PaymentPort", "line": 1, "end_line": 4, "signature": "pub trait PaymentPort",
               "doc": "Cobra o pedido.", "members": ["src/pay/port.rs:2:charge", "src/pay/port.rs:3:LIMIT"]},
              {"kind": "method", "name": "charge", "line": 2, "end_line": 2, "signature": "fn charge(&self, total: u32)",
               "owner": ["PaymentPort"], "implemented_by": ["src/pay/card.rs:3:charge"]},
              {"kind": "constant", "name": "LIMIT", "line": 3, "end_line": 3, "owner": ["PaymentPort"]}
            ]},
            {"path": "src/pay/card.rs", "declarations": [
              {"kind": "struct", "name": "CardGateway", "line": 1, "end_line": 1},
              {"kind": "method", "name": "charge", "line": 3, "end_line": 9, "signature": "fn charge(&self, total: u32)",
               "owner": ["CardGateway"], "contract": ["PaymentPort"], "body_comment": "manda ao banco do cartão",
               "implements": ["src/pay/port.rs:2:charge"]}
            ]}
          ],
          "history": {
            "paths": ["src/pay/card.rs", "src/pay/port.rs"],
            "commits": [
              {"id": "c1", "at": 100, "title": "Primeiro cartão", "added": [0, 1]},
              {"id": "c2", "at": 300, "title": "Cartão com parcela", "changed": [0]},
              {"id": "c3", "at": 200, "title": "Limite do cartão", "changed": [0]},
              {"id": "c4", "at": 400, "title": "Cartão sem juros", "changed": [0]}
            ]
          }
        })
    }

    /// Cada candidato leva o que o mapa guarda: o dono e o contrato, os
    /// membros com os métodos marcados, os comentários do corpo e os três
    /// títulos de commit mais novos do arquivo.
    #[test]
    fn each_candidate_carries_the_owner_the_members_the_body_comments_and_the_three_newest_commits() {
        let dir = saved_json(&contract_map());
        let found = candidates(dir.path(), "charge PaymentPort", "cobrar o pedido no cartão", &languages(), 100).unwrap();
        assert_eq!(found.whole.len(), 5, "{found:?}");
        let ids: Vec<i64> = found.candidates.iter().map(|c| c.id).collect();
        assert_eq!(ids, found.whole, "under the cap every declaration of the whole list is a candidate");

        let port = found.candidates.iter().find(|c| c.name == "PaymentPort").unwrap();
        assert_eq!(port.members, vec!["charge()".to_string(), "LIMIT".to_string()]);
        assert_eq!(port.file_commits, vec!["Primeiro cartão".to_string()]);
        assert_eq!(port.documentation, "Cobra o pedido.");

        let card = found.candidates.iter().find(|c| c.name == "charge" && c.path == "src/pay/card.rs").unwrap();
        assert_eq!(card.owner, "CardGateway PaymentPort");
        assert_eq!(card.body_comments, "manda ao banco do cartão");
        assert_eq!((card.line, card.end_line), (3, 9));
        assert_eq!(
            card.file_commits,
            vec!["Cartão sem juros".to_string(), "Cartão com parcela".to_string(), "Limite do cartão".to_string()]
        );

        let shown = declarations(dir.path(), &[card.id, 999]).unwrap();
        assert_eq!(shown, vec![card.clone()], "an id the map does not have is left out");
    }

    /// A declaração de teste nunca é candidata da busca com filtro: nem a de
    /// um arquivo de teste nem a escrita no trecho de teste de outro arquivo,
    /// por nenhuma das quatro listas. A busca sem filtro ainda acha o arquivo
    /// de teste pelo pedaço do nome.
    #[test]
    fn a_test_declaration_is_never_a_filter_candidate() {
        let dir = saved_json(&json!({"modules": [
            {"path": "src/pedido.rs", "test_lines": [[8, 20]], "declarations": [
                {"kind": "function", "name": "gravar_pedido", "line": 1, "end_line": 5,
                 "signature": "pub fn gravar_pedido()", "doc": "Grava o pedido."},
                {"kind": "function", "name": "grava_o_pedido_no_teste", "line": 10, "end_line": 14,
                 "signature": "fn grava_o_pedido_no_teste()", "doc": "Grava o pedido."}]},
            {"path": "tests/pedido_test.rs", "declarations": [
                {"kind": "function", "name": "gravar_pedido_de_teste", "line": 1, "end_line": 4,
                 "signature": "fn gravar_pedido_de_teste()", "doc": "Grava o pedido."}]}
        ]}));
        let found = candidates(dir.path(), "gravar pedido", "gravar o pedido", &languages(), 100).unwrap();
        let names: Vec<&str> = found.candidates.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["gravar_pedido"], "{found:?}");
        assert_eq!(found.whole, vec![id_of(dir.path(), "gravar_pedido")], "{found:?}");
        assert!(paths(dir.path(), "gravar_pedido_de_teste").contains(&"tests/pedido_test.rs".to_string()));
    }

    /// O parâmetro escrito no cabeçalho do tipo, que o scan grava com o tipo
    /// de declaração próprio dele, não é candidato: nem o que a assinatura do
    /// tipo traz, nem o que o teto dela cortou. O campo escrito no corpo do
    /// tipo continua candidato, e o arquivo continua achado pelo nome do
    /// parâmetro.
    #[test]
    fn a_parameter_written_in_the_header_of_its_owner_is_never_a_filter_candidate() {
        let dir = saved_json(&json!({"modules": [
            {"path": "src/Validator.cs", "declarations": [
                {"kind": "class", "name": "BlockingValidator", "line": 1, "end_line": 9,
                 "signature": "public sealed class BlockingValidator(string slugOwner, string reason"},
                {"kind": "parameter", "name": "slugOwner", "line": 1, "end_line": 1, "signature": "string slugOwner",
                 "owner": ["BlockingValidator"]},
                {"kind": "parameter", "name": "blockedSlug", "line": 1, "end_line": 1, "signature": "string blockedSlug",
                 "owner": ["BlockingValidator"]},
                {"kind": "field", "name": "slugCache", "line": 3, "end_line": 3,
                 "signature": "private readonly string slugCache", "owner": ["BlockingValidator"]}
            ]}
        ]}));
        let found = candidates(dir.path(), "slug", "", &languages(), 100).unwrap();
        let names: Vec<&str> = found.candidates.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"slugCache") && names.contains(&"BlockingValidator"), "{names:?}");
        assert!(!names.contains(&"slugOwner") && !names.contains(&"blockedSlug"), "{names:?}");
        assert_eq!(paths(dir.path(), "blocked slug"), ["src/Validator.cs"]);
    }

    /// A palavra aprendida soma peso, mas a declaração que só ela acha fica
    /// atrás da que casa pelo nome, mesmo quando o nome comprido dá a esta
    /// uma nota menor.
    #[test]
    fn a_learned_word_alone_does_not_pass_a_declaration_whose_name_matches() {
        let named = "sobra_do_caixa_do_mes_anterior_ja_consolidada_no_fechamento";
        let dir = saved_json(&json!({"modules": [
            {"path": "src/rounds.rs", "declarations": [
                {"kind": "function", "name": "collect_leftover", "line": 1, "end_line": 5}]},
            {"path": "src/cash.rs", "declarations": [
                {"kind": "function", "name": named, "line": 1, "end_line": 5}]}
        ]}));
        let model = model_path(dir.path());
        let words = Normalizer::new(&languages()).query("sobra");
        let base = |dir: &Path| -> Vec<i64> {
            let db = open_existing(&model_path(dir)).unwrap();
            base_list(db.conn(), &words).unwrap().into_iter().map(|(id, _)| id).collect()
        };
        assert_eq!(base(dir.path()), vec![id_of(dir.path(), named)]);

        map_glossary::record_search(&model, "s1", "sobra", &languages(), &[]).unwrap();
        let wait = std::time::Duration::from_secs(1);
        let marks = map_glossary::confirm_edit(&model, "s1", "src/rounds.rs", &[(2, 2)], &languages(), wait).unwrap();
        assert_eq!(marks.len(), 1, "{marks:?}");
        assert_eq!(base(dir.path()), vec![id_of(dir.path(), named), id_of(dir.path(), "collect_leftover")]);
    }

    #[test]
    fn the_links_give_the_methods_of_a_type_and_the_implementations_of_a_contract_method() {
        let dir = saved_json(&contract_map());
        let (port, contract) = (id_of(dir.path(), "PaymentPort"), id_of(dir.path(), "LIMIT") - 1);
        let db = open_existing(&model_path(dir.path())).unwrap();
        let implementation: i64 = db
            .conn()
            .query_row("SELECT rowid FROM decls WHERE name = 'charge' AND file = 'src/pay/card.rs'", [], |row| row.get(0))
            .unwrap();
        let found = links(dir.path(), &[port, contract]).unwrap();
        assert_eq!(found[&port].kind, "trait");
        assert_eq!(found[&port].methods, vec![contract], "the constant is a member, not a method");
        assert_eq!(found[&contract].path, "src/pay/port.rs");
        assert_eq!(found[&contract].implementations, vec![(implementation, "src/pay/card.rs".to_string())]);
    }

    /// O método de contrato cumprido pelo cartão, longe dele, e por dois
    /// dublês de teste: um no trecho de teste do próprio arquivo, de caminho
    /// igual ao dele, e outro num arquivo de teste. As ligações só trazem o
    /// cartão, e é ele que o método puxa quando nenhuma implementação está
    /// entre os candidatos, mesmo com o caminho do dublê mais parecido.
    #[test]
    fn a_test_double_is_never_the_implementation_a_contract_method_pulls() {
        let dir = saved_json(&json!({"modules": [
            {"path": "src/pay/port.rs", "test_lines": [[10, 20]], "declarations": [
                {"kind": "method", "name": "charge", "line": 2, "end_line": 2, "signature": "fn charge(&self)",
                 "owner": ["PaymentPort"],
                 "implemented_by": ["src/pay/port.rs:12:charge", "tests/fakes.rs:3:charge", "src/bank/card.rs:3:charge"]},
                {"kind": "method", "name": "charge", "line": 12, "end_line": 14, "signature": "fn charge(&self)",
                 "owner": ["FakeGateway"], "implements": ["src/pay/port.rs:2:charge"]}]},
            {"path": "tests/fakes.rs", "declarations": [
                {"kind": "method", "name": "charge", "line": 3, "end_line": 5, "signature": "fn charge(&self)",
                 "owner": ["FakeCard"], "implements": ["src/pay/port.rs:2:charge"]}]},
            {"path": "src/bank/card.rs", "declarations": [
                {"kind": "method", "name": "charge", "line": 3, "end_line": 9, "signature": "fn charge(&self)",
                 "owner": ["CardGateway"], "implements": ["src/pay/port.rs:2:charge"]}]}
        ]}));
        let db = open_existing(&model_path(dir.path())).unwrap();
        let at = |file: &str, line: i64| -> i64 {
            db.conn()
                .query_row("SELECT rowid FROM decls WHERE file = ?1 AND line = ?2", params![file, line], |row| row.get(0))
                .unwrap()
        };
        let (contract, card) = (at("src/pay/port.rs", 2), at("src/bank/card.rs", 3));
        let found = links(dir.path(), &[contract]).unwrap();
        assert_eq!(found[&contract].implementations, vec![(card, "src/bank/card.rs".to_string())]);
        let picks = crate::domain::map_select::select(&[contract], &[], &[], &found);
        let pulled: Vec<i64> = picks
            .iter()
            .filter(|pick| pick.source == crate::domain::map_select::Source::Pulled)
            .map(|pick| pick.id)
            .collect();
        assert_eq!(pulled, [card]);
    }

    #[test]
    fn the_name_list_finds_a_word_of_the_query_inside_a_glued_name() {
        let dir = saved(LARGER);
        let db = open_existing(&model_path(dir.path())).unwrap();
        let hits = name_hits(db.conn(), "pagamento de pedido").unwrap();
        let named = |hit: &NameHits| -> Vec<String> {
            let mut names: Vec<String> = hit
                .names
                .iter()
                .map(|(id, _)| {
                    db.conn().query_row("SELECT name FROM decls WHERE rowid = ?1", [id], |row| row.get(0)).unwrap()
                })
                .collect();
            names.sort();
            names
        };
        assert_eq!(hits.len(), 2, "the three-letter word looks for no name");
        assert_eq!(named(&hits[0]), ["estornarPagamento"]);
        // O nome do arquivo escrito por máquina fica fora do índice.
        assert_eq!(named(&hits[1]), ["buscarPedido", "listarPedidos", "pagarPedido"]);
        assert_eq!(hits[0].word_chars, 9);
        assert_eq!(hits[0].names[0].1, "estornarpagamento".chars().count());
    }

    /// Na lista de todos os campos, os textos fixos de uma declaração contam
    /// como um campo só: a marca do texto não muda o peso da palavra, e o
    /// texto mais curto pesa mais. Com uma marca por campo, o texto de erro,
    /// que quase nenhuma declaração tem, ficava com a média perto de zero, e a
    /// declaração do erro curto vinha atrás da do texto comum mais longo.
    #[test]
    fn a_short_error_text_weighs_like_any_text_of_its_size_in_the_list_of_every_field() {
        let module = |path: &str, name: &str, kind: &str, value: &str| {
            json!({"path": path,
                   "declarations": [{"kind": "function", "name": name, "line": 1, "end_line": 5,
                                     "signature": format!("fn {name}()")}],
                   "texts": [{"line": 3, "kind": kind, "value": value, "owner": name}]})
        };
        let mut modules = vec![
            module("src/baixa.rs", "baixar", "error", "estoque vazio"),
            module("src/aviso.rs", "avisar", "text", "estoque em falta agora"),
        ];
        // Oito declarações com um texto comum de quatro palavras, sem erro:
        // a média do texto comum é 3,6 palavras, e a do erro, 0,2.
        for n in 0..8 {
            modules.push(module(&format!("src/outro{n}.rs"), &format!("fazer{n}"), "text", "algo bem diferente aqui"));
        }
        let dir = saved_json(&json!({ "modules": modules }));
        let found = candidates(dir.path(), "estoque", "", &languages(), CANDIDATES).unwrap();
        // Só a lista de tudo e a dos arquivos acham a palavra, e a de tudo
        // entra primeiro no rodízio: o primeiro da lista inteira é o dela.
        assert_eq!(found.whole, vec![id_of(dir.path(), "baixar"), id_of(dir.path(), "avisar")], "{found:?}");
    }

    /// O texto fixo escrito num método conta também para o tipo que o traz,
    /// como a chamada: o tipo cujo método escreve o texto mais curto com a
    /// palavra vem à frente das funções dos outros arquivos, de texto mais
    /// longo. Só com o método, o tipo ficava para depois delas, trazido
    /// apenas pela lista dos arquivos, e a pergunta pela mensagem não o
    /// achava entre os primeiros.
    #[test]
    fn a_text_written_in_a_method_also_counts_for_the_type_that_contains_it() {
        let text = |line: u64, value: &str, owner: &str| json!({"line": line, "kind": "text", "value": value, "owner": owner});
        let mut modules = vec![json!({"path": "src/pedidos.rs",
            "declarations": [
                {"kind": "class", "name": "Pedidos", "line": 1, "end_line": 20, "signature": "pub struct Pedidos"},
                {"kind": "method", "name": "gravar", "line": 3, "end_line": 10, "signature": "fn gravar(&self)",
                 "owner": ["Pedidos"]},
                {"kind": "function", "name": "resumir", "line": 22, "end_line": 30, "signature": "fn resumir()"}],
            "texts": [text(5, "fornecedor bloqueado", "gravar"),
                      text(25, "total do dia somado por loja com desconto frete imposto taxa e troco devolvido", "resumir")]})];
        // Três funções de outros arquivos com a palavra num texto de quatro
        // palavras: o arquivo delas pesa mais que o dos pedidos, cujo texto
        // inteiro é longo.
        for n in 1..=3 {
            modules.push(json!({"path": format!("src/aviso{n}.rs"),
                "declarations": [{"kind": "function", "name": format!("avisar{n}"), "line": 1, "end_line": 5,
                                  "signature": format!("fn avisar{n}()")}],
                "texts": [text(3, "fornecedor em falta hoje", &format!("avisar{n}"))]}));
        }
        let dir = saved_json(&json!({ "modules": modules }));
        let found = candidates(dir.path(), "fornecedor", "", &languages(), CANDIDATES).unwrap();
        let id = |name: &str| id_of(dir.path(), name);
        assert_eq!(
            found.whole,
            vec![id("Pedidos"), id("avisar1"), id("gravar"), id("avisar2"), id("avisar3"), id("resumir")],
            "{found:?}"
        );
    }

    /// A pergunta da busca com filtro leva só a raiz da língua do texto: o
    /// plural `commands` acha a declaração que escreve `commands` e não a que
    /// escreve `command`, que a raiz inglesa da pergunta traria junto. O
    /// singular `command` acha as duas, porque o índice guarda as duas raízes
    /// de `commands`.
    #[test]
    fn a_plural_word_of_the_filter_question_does_not_reach_the_declaration_that_writes_the_singular() {
        let dir = saved(&[
            ("src/plural.rs", "", &[("varios", "Runs the commands of the queue")]),
            ("src/singular.rs", "", &[("unico", "Runs one command of the queue")]),
            ("src/outro.rs", "", &[("outro", "Draws the page")]),
        ]);
        let (plural, singular) = (id_of(dir.path(), "varios"), id_of(dir.path(), "unico"));
        let found = candidates(dir.path(), "commands", "", &languages(), CANDIDATES).unwrap();
        assert_eq!(found.whole, vec![plural], "{found:?}");
        let mut both = candidates(dir.path(), "command", "", &languages(), CANDIDATES).unwrap().whole;
        both.sort_unstable();
        assert_eq!(both, vec![plural.min(singular), plural.max(singular)]);
    }

    /// Duas palavras da pergunta que dividem uma forma contam essa forma uma
    /// vez só: a declaração que só a tem (`simulação`, com a forma `simul` das
    /// duas palavras) não passa à frente da que tem a outra palavra da
    /// pergunta (`pasta`) por causa da contagem em dobro.
    #[test]
    fn two_words_of_the_filter_question_with_a_shared_form_weigh_it_once() {
        let dir = saved(&[
            ("src/pasta.rs", "", &[("primeira", "pasta")]),
            ("src/simular.rs", "", &[("segunda", "simulação")]),
        ]);
        let found = candidates(dir.path(), "simula simulação pasta", "", &languages(), CANDIDATES).unwrap();
        let (first, second) = (id_of(dir.path(), "primeira"), id_of(dir.path(), "segunda"));
        assert_eq!(found.whole, vec![first, second], "{found:?}");
    }

    /// A medida do primeiro elo da busca com filtro: em quantas buscas de
    /// uma régua a declaração certa está entre os 100 candidatos, e quanto
    /// tempo a etapa leva. A régua vem de um arquivo JSON apontado por
    /// `MAP_FIRST_LINK_RULER`: `expected` (as buscas que o laboratório
    /// acertou) e `searches`, cada uma com `key`, o `model` do projeto, a
    /// `query`, a `intent` e os `targets` (caminho, nome e linha). A
    /// diferença acima de 3 buscas falha, com a lista das perdidas. Também
    /// diz em quantas o certo é o primeiro, está entre os 3, os 12, os 50 e os
    /// 100 primeiros da lista inteira.
    #[test]
    #[ignore = "mede com os mapas dos projetos de prova"]
    fn the_first_link_puts_the_right_declaration_among_the_hundred_candidates() {
        let Ok(ruler) = std::env::var("MAP_FIRST_LINK_RULER") else {
            panic!("MAP_FIRST_LINK_RULER points to the ruler file");
        };
        let ruler: Value = serde_json::from_str(&std::fs::read_to_string(ruler).unwrap()).unwrap();
        let languages = Languages::new(["pt-BR", "en-US"]);
        let searches = ruler["searches"].as_array().unwrap();
        // A primeira busca de cada mapa refaz o índice nas línguas dela: fica
        // fora do tempo.
        let mut warmed = HashSet::new();
        for search in searches {
            let model = search["model"].as_str().unwrap();
            if warmed.insert(model.to_string()) {
                candidates_at(Path::new(model), "warm", "", &languages, CANDIDATES).unwrap();
            }
        }
        let (mut found, mut millis, mut lost) = (0usize, Vec::new(), Vec::new());
        let mut ranks: Vec<usize> = Vec::new();
        for search in searches {
            let text = |key: &str| search[key].as_str().unwrap().to_string();
            let started = std::time::Instant::now();
            let got = candidates_at(Path::new(&text("model")), &text("query"), &text("intent"), &languages, CANDIDATES)
                .unwrap();
            millis.push(started.elapsed().as_secs_f64() * 1000.0);
            let targets: Vec<(String, String, u64)> = search["targets"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| (t[0].as_str().unwrap().to_string(), t[1].as_str().unwrap().to_string(), t[2].as_u64().unwrap()))
                .collect();
            let place = got.candidates.iter().position(|c| {
                targets.iter().any(|(path, name, line)| &c.path == path && &c.name == name && u64::from(c.line) == *line)
            });
            let hit = place.is_some();
            ranks.extend(place.map(|at| at + 1));
            found += usize::from(hit);
            if !hit {
                lost.push(format!("{} (lab: {})", text("key"), search["lab_in_100"]));
            }
        }
        millis.sort_by(f64::total_cmp);
        let expected = usize::try_from(ruler["expected"].as_u64().unwrap()).unwrap();
        println!(
            "first link: {found} of {} with the right one among {CANDIDATES}; lab {expected}; median {:.0} ms, worst {:.0} ms",
            searches.len(),
            millis[millis.len() / 2],
            millis[millis.len() - 1]
        );
        let within = |top: usize| ranks.iter().filter(|&&rank| rank <= top).count();
        println!(
            "first link: the right one is within the first 1/3/12/50/100: {}/{}/{}/{}/{}",
            within(1),
            within(3),
            within(12),
            within(50),
            within(100)
        );
        println!("lost: {lost:#?}");
        assert!(found.abs_diff(expected) <= 3, "{found} against {expected}: {lost:#?}");
    }
}
