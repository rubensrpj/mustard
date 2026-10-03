//! `map_meaning` — o sentido de cada declaração e de cada palavra do projeto,
//! guardado no mapa como vetores.
//!
//! O modelo é estático e mora dentro do programa (`assets/meaning/`): uma
//! tabela de vetores por pedaço de palavra, e o vetor de um texto é a média
//! dos vetores dos pedaços, normalizada. Ele lê o projeto inteiro em
//! segundos, sem internet e sem serviço pago.
//!
//! O bloco `meaning` do mapa guarda duas tabelas, ambas em int8, 256 números
//! cada vetor:
//!
//! - `decl_vectors`: um vetor por declaração, do texto compilado dela
//!   ([`compiled_text`]), sem corte. A chave é o arquivo, o nome e a ordem
//!   entre as de mesmo nome; a marca do texto ([`fingerprint`]) diz se a
//!   declaração mudou, e só a que mudou é lida de novo;
//! - `word_vectors`: um vetor por palavra distinta do compilado, antes da raiz,
//!   com as formas que o índice de busca grava para ela
//!   ([`crate::domain::normalize`]), para achar as palavras que o projeto usa
//!   perto de uma palavra que ele não usa.
//!
//! Quem grava é o scan, depois do mapa e depois de cada leitura da história,
//! que traz ao compilado os títulos dos commits de cada declaração
//! ([`fill_at`]). Mapa sem estas tabelas abre e se lê como sempre: quem lê os
//! vetores ([`ranked_declarations`]) recebe uma lista vazia.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;

use model2vec_rs::model::StaticModel;
use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::normalize::{plain_words, split_identifier, Languages, Normalizer};
use crate::io::map_db::{table_exists, Block, Kind, MapDb};
use crate::io::map_notes_fresh;
use crate::io::map_revision;
use crate::platform::error::Result;

/// O nome do bloco na tabela de blocos do mapa.
pub const BLOCK_NAME: &str = "meaning";

/// A versão do formato deste bloco. Sobe quando o modelo embutido troca ou
/// quando o texto compilado muda de forma: o bloco refeito volta vazio, e o
/// scan seguinte calcula todos os vetores de novo.
const BLOCK_VERSION: u32 = 1;

/// Quantos textos o modelo lê por vez.
const BATCH: usize = 512;

/// A palavra de uma letra só e a que passa deste tamanho não entram na
/// tabela de palavras: uma não diz nada, a outra é um código colado.
const WORD_LENGTHS: std::ops::RangeInclusive<usize> = 2..=40;

/// Quantas confirmações do commit entram no compilado quando a declaração
/// não tem história própria: os títulos mais novos do arquivo.
const FILE_TITLES: usize = 5;

const TOKENIZER: &[u8] = include_bytes!("../../assets/meaning/tokenizer.json");
const WEIGHTS: &[u8] = include_bytes!("../../assets/meaning/model.safetensors");
const CONFIG: &[u8] = include_bytes!("../../assets/meaning/config.json");

/// As tabelas do bloco.
const SCHEMA: &str = "CREATE TABLE decl_vectors(file TEXT NOT NULL, name TEXT NOT NULL, nth INTEGER NOT NULL, \
     hash INTEGER NOT NULL, vector BLOB NOT NULL, PRIMARY KEY(file, name, nth)) WITHOUT ROWID;\
   CREATE TABLE word_vectors(word TEXT NOT NULL PRIMARY KEY, forms TEXT NOT NULL, vector BLOB NOT NULL) WITHOUT ROWID;\
   CREATE TABLE meaning_meta(key TEXT NOT NULL PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;";

/// O bloco `meaning`: refeito a partir do que o scan lê, então o formato novo
/// apaga as tabelas velhas e a passada seguinte as enche.
pub const BLOCK: Block = Block {
    name: BLOCK_NAME,
    version: BLOCK_VERSION,
    tables: &["decl_vectors", "word_vectors", "meaning_meta"],
    schema: SCHEMA,
    kind: Kind::Rebuilt(nothing_to_rebuild),
};

/// O bloco refeito volta vazio: quem o enche é [`fill_at`].
#[allow(clippy::unnecessary_wraps)] // a assinatura é a de todo bloco refeito
fn nothing_to_rebuild(_: &Connection, _: &Path) -> Result<()> {
    Ok(())
}

/// O modelo embutido, carregado uma vez. `None` só se os bytes embutidos não
/// forem um modelo, o que o teste do modelo pega.
fn model() -> Option<&'static StaticModel> {
    static MODEL: OnceLock<Option<StaticModel>> = OnceLock::new();
    MODEL.get_or_init(|| StaticModel::from_bytes(TOKENIZER, WEIGHTS, CONFIG, None).ok()).as_ref()
}

/// O vetor de um texto: a média dos vetores dos pedaços, de comprimento 1.
/// Um texto sem nenhum pedaço conhecido dá o vetor nulo. `None` quando o
/// modelo não carrega.
#[must_use]
pub fn text_vector(text: &str) -> Option<Vec<f32>> {
    let model = model()?;
    let owned = [text.to_string()];
    model.encode_with_args(&owned, None, 1).into_iter().next()
}

/// O vetor em int8: cada número vezes 127, arredondado. O vetor tem
/// comprimento 1, então cabe.
fn quantize(vector: &[f32]) -> Vec<i8> {
    vector.iter().map(|value| (value * 127.0).round().clamp(-127.0, 127.0) as i8).collect()
}

/// O vetor em bytes, como a tabela o guarda.
fn to_blob(vector: &[i8]) -> Vec<u8> {
    vector.iter().map(|value| value.to_le_bytes()[0]).collect()
}

/// O produto escalar e as somas dos quadrados de duas listas de números em
/// int8. Cada vetor tem 256 números de no máximo 127, então as somas cabem
/// em `i32`, que o compilador lê em blocos.
fn products(pairs: impl Iterator<Item = (i8, i8)>) -> (i32, i32, i32) {
    let (mut dot, mut norm_a, mut norm_b) = (0_i32, 0_i32, 0_i32);
    for (x, y) in pairs {
        let (x, y) = (i32::from(x), i32::from(y));
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    (dot, norm_a, norm_b)
}

/// Uma palavra do projeto perto da palavra perguntada.
#[derive(Debug, Clone, PartialEq)]
pub struct Neighbor {
    /// A palavra como o projeto a escreve, em minúsculas.
    pub word: String,
    /// As formas que o índice de busca grava para ela, separadas por espaço.
    pub forms: String,
    /// O cosseno com a palavra perguntada.
    pub cosine: f32,
}

/// O vetor em int8 de um texto, como as tabelas do bloco o guardam. `None`
/// quando o modelo não carrega.
#[must_use]
pub fn quantized_vector(text: &str) -> Option<Vec<i8>> {
    text_vector(text).map(|vector| quantize(&vector))
}

/// O cosseno de um vetor em int8 com um vetor em bytes da tabela, sem
/// montar o segundo: a conta de [`cosine`], lida direto dos bytes.
fn cosine_with_blob(asked: &[i8], asked_norm: f64, blob: &[u8]) -> f32 {
    if asked.len() != blob.len() || asked_norm == 0.0 {
        return 0.0;
    }
    let (dot, _, norm) = products(asked.iter().zip(blob).map(|(x, byte)| (*x, i8::from_le_bytes([*byte]))));
    if norm == 0 {
        return 0.0;
    }
    (f64::from(dot) / (asked_norm * f64::from(norm).sqrt())) as f32
}

/// O tamanho de um vetor em int8.
fn norm_of(vector: &[i8]) -> f64 {
    f64::from(vector.iter().map(|value| i32::from(*value) * i32::from(*value)).sum::<i32>()).sqrt()
}

/// Uma declaração perto do texto perguntado.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Similar {
    /// O número da declaração no mapa (o `rowid` de `decls`).
    pub id: i64,
    /// O cosseno do vetor dela com o do texto.
    pub cosine: f32,
}

/// As declarações do mapa da mais perto para a mais longe do sentido de
/// `text`: o cosseno do vetor do texto com o de cada declaração, e no empate
/// o número menor primeiro. Só entra a declaração com cosseno acima de zero.
/// Sem a tabela de vetores, sem o modelo ou com um texto sem nenhuma palavra
/// que o modelo conheça, a lista é vazia.
pub fn ranked_declarations(conn: &Connection, text: &str) -> Result<Vec<Similar>> {
    if !table_exists(conn, "decl_vectors")? {
        return Ok(Vec::new());
    }
    let Some(asked) = quantized_vector(text) else { return Ok(Vec::new()) };
    let norm = norm_of(&asked);
    if norm == 0.0 {
        return Ok(Vec::new());
    }
    // A ordem entre as declarações de mesmo nome no arquivo é a da gravação
    // dos vetores: a do `rowid`.
    let mut statement = conn.prepare(
        "SELECT d.id, v.vector FROM \
           (SELECT rowid AS id, file, name, row_number() OVER (PARTITION BY file, name ORDER BY rowid) - 1 AS nth \
            FROM decls) d \
         JOIN decl_vectors v ON v.file = d.file AND v.name = d.name AND v.nth = d.nth",
    )?;
    let mut rows = statement.query([])?;
    let mut found: Vec<Similar> = Vec::new();
    while let Some(row) = rows.next()? {
        let (id, blob): (i64, Vec<u8>) = (row.get(0)?, row.get(1)?);
        let cosine = cosine_with_blob(&asked, norm, &blob);
        if cosine > 0.0 {
            found.push(Similar { id, cosine });
        }
    }
    found.sort_by(|a, b| b.cosine.total_cmp(&a.cosine).then(a.id.cmp(&b.id)));
    Ok(found)
}

/// As palavras do projeto com o vetor de cada uma, lidas uma vez para
/// perguntar por várias palavras.
pub struct ProjectWords {
    rows: Vec<(String, String, Vec<u8>)>,
}

impl ProjectWords {
    /// As palavras da tabela do mapa; vazia no mapa sem vetores.
    pub fn read(conn: &Connection) -> Result<Self> {
        if !table_exists(conn, "word_vectors")? {
            return Ok(Self { rows: Vec::new() });
        }
        let mut statement = conn.prepare("SELECT word, forms, vector FROM word_vectors")?;
        let rows = statement
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Vec<u8>>(2)?)))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(Self { rows })
    }

    /// Se a tabela não tem palavra nenhuma.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// As palavras do projeto com cosseno de pelo menos `min_cosine` com
    /// `asked`, o vetor de `word`, da mais perto para a mais longe. A própria
    /// palavra não entra.
    #[must_use]
    pub fn near(&self, word: &str, asked: &[i8], min_cosine: f32) -> Vec<Neighbor> {
        let norm = norm_of(asked);
        let mut found: Vec<Neighbor> = self
            .rows
            .iter()
            .filter(|(candidate, _, _)| candidate != word)
            .filter_map(|(candidate, forms, blob)| {
                let cosine = cosine_with_blob(asked, norm, blob);
                (cosine >= min_cosine).then(|| Neighbor { word: candidate.clone(), forms: forms.clone(), cosine })
            })
            .collect();
        found.sort_by(|a, b| b.cosine.total_cmp(&a.cosine).then_with(|| a.word.cmp(&b.word)));
        found
    }
}

/// Quantas linhas cada tabela passou a ter e quantos vetores esta passada
/// calculou: o que o scan conta depois de encher o bloco.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Report {
    /// As declarações com vetor.
    pub declarations: usize,
    /// Os vetores de declaração que esta passada calculou: os das
    /// declarações novas ou mudadas.
    pub computed: usize,
    /// As palavras com vetor.
    pub words: usize,
}

/// Uma declaração como o mapa a guarda, com o que entra no compilado dela.
struct Declaration {
    file: String,
    name: String,
    nth: i64,
    text: String,
}

/// Enche o bloco `meaning` do mapa em `map`, lendo as declarações, os textos
/// e a história que o scan já gravou. Só a declaração nova ou mudada tem o
/// vetor calculado; a que saiu do mapa perde o dela. As palavras só se
/// refazem quando alguma declaração mudou ou saiu, ou quando as línguas do
/// `mustard.json` de `root` mudaram, porque delas vêm as formas de cada
/// palavra. O modelo só é carregado quando há vetor a calcular.
///
/// Passada em que nada foi gravado no mapa desde o último preenchimento, com
/// as mesmas línguas, não relê nem compara declaração nenhuma: o carimbo das
/// gravações ([`map_revision::stamp`]) que o último preenchimento guardou
/// ainda é o do mapa.
///
/// Mapa sem declarações, ou sem o modelo carregado, fica como está.
pub fn fill_at(map: &Path, root: &Path) -> Result<Report> {
    let mut db = MapDb::open(map, root, &[BLOCK])?;
    if !table_exists(db.conn(), "decls")? {
        return Ok(Report::default());
    }
    let languages = Languages::of_project(root);
    let language_mark = languages.codes().join(",");
    let stamp = map_revision::stamp(db.conn())?;
    if stored_stamp(db.conn())?.as_deref() == Some(stamp.as_str())
        && stored_language_mark(db.conn())?.as_deref() == Some(language_mark.as_str())
    {
        return Ok(Report {
            declarations: count_rows(db.conn(), "decl_vectors")?,
            computed: 0,
            words: count_rows(db.conn(), "word_vectors")?,
        });
    }
    let declarations = read_declarations(db.conn())?;
    let known = stored_hashes(db.conn())?;
    let changed: Vec<usize> = (0..declarations.len())
        .filter(|at| {
            let d = &declarations[*at];
            known.get(&(d.file.clone(), d.name.clone(), d.nth)) != Some(&fingerprint(&d.text))
        })
        .collect();
    let present: HashSet<(&str, &str, i64)> =
        declarations.iter().map(|d| (d.file.as_str(), d.name.as_str(), d.nth)).collect();
    let gone: Vec<&(String, String, i64)> =
        known.keys().filter(|(file, name, nth)| !present.contains(&(file.as_str(), name.as_str(), *nth))).collect();
    let words_stale = !changed.is_empty()
        || !gone.is_empty()
        || stored_language_mark(db.conn())?.as_deref() != Some(language_mark.as_str());

    let vectors = if changed.is_empty() {
        Vec::new()
    } else {
        let Some(model) = model() else { return Ok(Report::default()) };
        encode(model, changed.iter().map(|at| declarations[*at].text.as_str()))
    };
    let plan = if words_stale { Some(plan_words(db.conn(), &declarations, &languages)?) } else { None };

    db.write(|tx| {
        let mut drop_declaration = tx.prepare("DELETE FROM decl_vectors WHERE file = ?1 AND name = ?2 AND nth = ?3")?;
        for (file, name, nth) in gone {
            drop_declaration.execute(params![file, name, nth])?;
        }
        let mut put = tx.prepare(
            "INSERT OR REPLACE INTO decl_vectors(file, name, nth, hash, vector) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for (at, vector) in changed.iter().zip(&vectors) {
            let d = &declarations[*at];
            put.execute(params![d.file, d.name, d.nth, fingerprint(&d.text), to_blob(vector)])?;
        }
        if let Some(plan) = &plan {
            let current: HashSet<&str> = plan.words.iter().map(|(word, _)| word.as_str()).collect();
            let mut drop_word = tx.prepare("DELETE FROM word_vectors WHERE word = ?1")?;
            for word in plan.stored.iter().filter(|word| !current.contains(word.as_str())) {
                drop_word.execute(params![word])?;
            }
            let mut update = tx.prepare("UPDATE word_vectors SET forms = ?2 WHERE word = ?1 AND forms != ?2")?;
            for (word, forms) in plan.words.iter().filter(|(word, _)| plan.stored.contains(word)) {
                update.execute(params![word, forms])?;
            }
            let mut insert = tx.prepare("INSERT INTO word_vectors(word, forms, vector) VALUES (?1, ?2, ?3)")?;
            for ((word, forms), vector) in plan.fresh.iter().zip(&plan.fresh_vectors) {
                insert.execute(params![word, forms, to_blob(vector)])?;
            }
            tx.execute(
                "INSERT OR REPLACE INTO meaning_meta(key, value) VALUES ('languages', ?1)",
                params![language_mark],
            )?;
        }
        tx.execute("INSERT OR REPLACE INTO meaning_meta(key, value) VALUES ('stamp', ?1)", params![stamp])?;
        Ok(())
    })?;
    let words = count_rows(db.conn(), "word_vectors")?;
    Ok(Report { declarations: declarations.len(), computed: changed.len(), words })
}

/// As palavras do projeto como a passada as deixa: cada uma com as formas
/// dela, as que já tinham vetor e as novas, com o vetor calculado agora.
struct WordPlan {
    words: Vec<(String, String)>,
    stored: HashSet<String>,
    fresh: Vec<(String, String)>,
    fresh_vectors: Vec<Vec<i8>>,
}

/// As palavras distintas do compilado das declarações, antes da raiz, sem as
/// de ligação, as de uma letra e os números, cada uma com as formas do
/// índice, e o vetor das que ainda não têm.
fn plan_words(conn: &Connection, declarations: &[Declaration], languages: &Languages) -> Result<WordPlan> {
    let mut normalizer = Normalizer::new(languages);
    let mut words: Vec<(String, String)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for declaration in declarations {
        for word in plain_words(&declaration.text) {
            if WORD_LENGTHS.contains(&word.chars().count())
                && !word.chars().all(|c| c.is_ascii_digit())
                && !normalizer.is_function_word(&word)
                && seen.insert(word.clone())
            {
                let forms = normalizer.word_forms(&word).join(" ");
                words.push((word, forms));
            }
        }
    }
    let stored = stored_words(conn)?;
    let fresh: Vec<(String, String)> = words.iter().filter(|(word, _)| !stored.contains(word)).cloned().collect();
    let fresh_vectors = if fresh.is_empty() {
        Vec::new()
    } else {
        model().map_or_else(Vec::new, |model| encode(model, fresh.iter().map(|(word, _)| word.as_str())))
    };
    // Sem o modelo, nenhuma palavra nova entra: o par de listas segue do mesmo tamanho.
    let fresh = fresh.into_iter().take(fresh_vectors.len()).collect();
    Ok(WordPlan { words, stored, fresh, fresh_vectors })
}

/// Quantas linhas tem a tabela.
fn count_rows(conn: &Connection, table: &str) -> Result<usize> {
    let rows: i64 = conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0))?;
    Ok(usize::try_from(rows).unwrap_or_default())
}

/// O carimbo das gravações do mapa de quando o último preenchimento leu as
/// declarações.
fn stored_stamp(conn: &Connection) -> Result<Option<String>> {
    Ok(conn.query_row("SELECT value FROM meaning_meta WHERE key = 'stamp'", [], |row| row.get(0)).optional()?)
}

/// As línguas com que as formas das palavras foram gravadas.
fn stored_language_mark(conn: &Connection) -> Result<Option<String>> {
    Ok(conn.query_row("SELECT value FROM meaning_meta WHERE key = 'languages'", [], |row| row.get(0)).optional()?)
}

/// Os vetores de `texts`, em int8, na mesma ordem, lidos em lotes.
fn encode<'a>(model: &StaticModel, texts: impl Iterator<Item = &'a str>) -> Vec<Vec<i8>> {
    let owned: Vec<String> = texts.map(str::to_string).collect();
    owned
        .chunks(BATCH)
        .flat_map(|batch| model.encode_with_args(batch, None, BATCH))
        .map(|vector| quantize(&vector))
        .collect()
}

/// A marca de um texto compilado: o FNV-1a de 64 bits sobre o texto,
/// misturado com a versão do bloco. É estável entre versões do Rust, ao
/// contrário do hasher da biblioteca padrão.
fn fingerprint(text: &str) -> i64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in BLOCK_VERSION.to_le_bytes().iter().chain(text.as_bytes()) {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
    }
    hash as i64
}

/// A marca de cada declaração que já tem vetor: arquivo, nome e ordem.
fn stored_hashes(conn: &Connection) -> Result<HashMap<(String, String, i64), i64>> {
    let mut statement = conn.prepare("SELECT file, name, nth, hash FROM decl_vectors")?;
    let rows = statement.query_map([], |row| {
        Ok(((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?), row.get::<_, i64>(3)?))
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// As palavras que já têm vetor.
fn stored_words(conn: &Connection) -> Result<HashSet<String>> {
    let mut statement = conn.prepare("SELECT word FROM word_vectors")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// As palavras de um texto de código: o nome colado separado, em minúsculas,
/// e tudo o que não é letra nem número vira espaço.
fn words_of(text: &str) -> String {
    split_identifier(text)
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// O texto compilado de uma declaração, sem corte, nesta ordem: o nome em
/// palavras, o tipo, a documentação, os textos de log, erro e texto de que ela
/// é dona, o comentário do corpo, a assinatura, a documentação inteira, os
/// membros, o dono em palavras, os supertipos, as palavras do caminho, os
/// títulos de commit e as notas de sentido (a da declaração e a do arquivo).
/// Os espaços se juntam em um só.
fn compiled_text(parts: &Parts<'_>) -> String {
    let path_words = words_of(parts.file);
    let pieces = [
        words_of(parts.name),
        parts.kind.to_string(),
        parts.doc.to_string(),
        parts.owned_texts.join(" "),
        parts.body_comment.to_string(),
        parts.signature.to_string(),
        parts.whole_doc.to_string(),
        parts.members.to_string(),
        words_of(parts.owner),
        parts.supertypes.to_string(),
        path_words,
        parts.titles.join(" "),
        parts.notes.to_string(),
    ];
    pieces.join(" ").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// O que entra no compilado de uma declaração.
struct Parts<'a> {
    file: &'a str,
    kind: &'a str,
    name: &'a str,
    doc: &'a str,
    signature: &'a str,
    whole_doc: &'a str,
    body_comment: &'a str,
    members: &'a str,
    owner: &'a str,
    supertypes: &'a str,
    owned_texts: &'a [String],
    titles: &'a [String],
    notes: &'a str,
}

/// As declarações do mapa, na ordem do mapa, cada uma com o texto compilado.
fn read_declarations(conn: &Connection) -> Result<Vec<Declaration>> {
    let texts = owned_texts(conn)?;
    let file_titles = newest_titles_by_file(conn)?;
    let lineage = lineage_titles(conn)?;
    let notes = map_notes_fresh::Texts::read(conn)?;
    let mut statement = conn.prepare(
        "SELECT file, kind, name, signature, doc, whole_doc, body_comment, supertypes, owner, members FROM decls ORDER BY rowid",
    )?;
    let rows = statement.query_map([], |row| {
        let text = |at: usize| row.get::<_, Option<String>>(at).map(Option::unwrap_or_default);
        Ok((text(0)?, text(1)?, text(2)?, text(3)?, text(4)?, text(5)?, text(6)?, text(7)?, text(8)?, text(9)?))
    })?;
    let mut counted: HashMap<(String, String), i64> = HashMap::new();
    let mut out = Vec::new();
    let none: Vec<String> = Vec::new();
    for row in rows {
        let (file, kind, name, signature, doc, whole_doc, body_comment, supertypes, owner, members) = row?;
        let nth = counted.entry((file.clone(), name.clone())).or_insert(0);
        let this = *nth;
        *nth += 1;
        let own = texts.get(&(file.clone(), name.clone())).unwrap_or(&none);
        let titles = lineage.get(&(file.clone(), name.clone(), this)).or_else(|| file_titles.get(&file)).unwrap_or(&none);
        let text = compiled_text(&Parts {
            file: &file,
            kind: &kind,
            name: &name,
            doc: &doc,
            signature: &signature,
            whole_doc: &whole_doc,
            body_comment: &body_comment,
            members: &members,
            owner: &owner,
            supertypes: &supertypes,
            owned_texts: own,
            titles,
            notes: &notes.of(&file, &name),
        });
        out.push(Declaration { file, name, nth: this, text });
    }
    Ok(out)
}

/// Os textos que o código escreve, por arquivo e dono: os de log, erro e
/// texto solto que cada declaração escreve.
fn owned_texts(conn: &Connection) -> Result<HashMap<(String, String), Vec<String>>> {
    let mut out: HashMap<(String, String), Vec<String>> = HashMap::new();
    if !table_exists(conn, "texts")? {
        return Ok(out);
    }
    let mut statement = conn.prepare("SELECT path, texts FROM texts")?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)))?;
    for row in rows {
        let (path, json) = row?;
        let Some(json) = json else { continue };
        let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(&json) else { continue };
        for entry in entries {
            let owner = entry.get("owner").and_then(serde_json::Value::as_str);
            let value = entry.get("value").and_then(serde_json::Value::as_str);
            if let (Some(owner), Some(value)) = (owner, value) {
                out.entry((path.clone(), owner.to_string())).or_default().push(value.to_string());
            }
        }
    }
    Ok(out)
}

/// Os títulos de commit dos `FILE_TITLES` commits mais novos que criaram ou
/// mudaram cada arquivo, do mais novo para o mais antigo, sem repetir.
fn newest_titles_by_file(conn: &Connection) -> Result<HashMap<String, Vec<String>>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    if !table_exists(conn, "commits")? || !table_exists(conn, "history_paths")? {
        return Ok(out);
    }
    let paths: HashMap<i64, String> = {
        let mut statement = conn.prepare("SELECT rowid, path FROM history_paths")?;
        let rows = statement.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    let mut statement = conn.prepare("SELECT title, added, changed FROM commits ORDER BY at DESC")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, Option<String>>(2)?))
    })?;
    for row in rows {
        let (title, added, changed) = row?;
        for list in [added, changed].into_iter().flatten() {
            let Ok(ids) = serde_json::from_str::<Vec<i64>>(&list) else { continue };
            for id in ids {
                // O número no commit conta de 0; o `rowid` da tabela, de 1.
                let Some(path) = paths.get(&(id + 1)) else { continue };
                let titles = out.entry(path.clone()).or_default();
                if titles.len() < FILE_TITLES && !titles.contains(&title) {
                    titles.push(title.clone());
                }
            }
        }
    }
    Ok(out)
}

/// Os títulos dos commits que mudaram cada declaração, quando a história dela
/// já foi lida: por arquivo, nome e ordem entre as de mesmo nome.
fn lineage_titles(conn: &Connection) -> Result<HashMap<(String, String, i64), Vec<String>>> {
    let mut out = HashMap::new();
    if !table_exists(conn, "lineage_decls")? || !table_exists(conn, "lineage_commits")? {
        return Ok(out);
    }
    let titles: HashMap<(String, String), String> = {
        let mut statement = conn.prepare("SELECT path, id, title FROM lineage_commits")?;
        let rows = statement
            .query_map([], |row| Ok(((row.get::<_, String>(0)?, row.get::<_, String>(1)?), row.get::<_, String>(2)?)))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    let mut statement = conn.prepare("SELECT path, name, nth, commits FROM lineage_decls")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?, row.get::<_, Option<String>>(3)?))
    })?;
    for row in rows {
        let (path, name, nth, commits) = row?;
        let Some(commits) = commits else { continue };
        let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(&commits) else { continue };
        let mut found: Vec<String> = Vec::new();
        for entry in entries {
            let Some(id) = entry.get("id").and_then(serde_json::Value::as_str) else { continue };
            if let Some(title) = titles.get(&(path.clone(), id.to_string()))
                && !found.contains(title)
            {
                found.push(title.clone());
            }
        }
        if !found.is_empty() {
            out.insert((path, name, nth), found);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::project_map;
    use serde_json::{json, Value};
    use tempfile::TempDir;

    /// Um projeto com este mapa, gravado como o scan grava.
    fn saved(modules: &Value) -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        let map = project_map::model_path(dir.path());
        project_map::save_at(&map, &json!({ "modules": modules }), "scan 1", &Languages::of_project(dir.path())).unwrap();
        dir
    }

    /// Uma função com esta documentação.
    fn function(name: &str, line: u64, doc: &str) -> Value {
        json!({"kind": "function", "name": name, "line": line, "end_line": line + 5,
               "signature": format!("fn {name}()"), "doc": doc})
    }

    /// Dois arquivos: um que remove e outro que apaga, na língua do código.
    fn two_files() -> TempDir {
        saved(&json!([
            {"path": "src/folders.rs", "declarations": [
                function("remove_folder", 1, "Remove the folder and everything inside it.")]},
            {"path": "src/files.rs", "declarations": [
                function("delete_file", 1, "Delete the file from the disk."),
                function("charge_invoice", 8, "Charge the customer invoice with the tax.")]}
        ]))
    }

    /// Grava de novo, pela porta do scan, o mapa de `two_files` com esta
    /// documentação em `delete_file` e, ou não, a função `charge_invoice`.
    fn rewrite(dir: &TempDir, doc: &str, with_charge: bool) {
        let mut declarations = vec![function("delete_file", 1, doc)];
        if with_charge {
            declarations.push(function("charge_invoice", 8, "Charge the customer invoice with the tax."));
        }
        let modules = json!([
            {"path": "src/folders.rs", "declarations": [
                function("remove_folder", 1, "Remove the folder and everything inside it.")]},
            {"path": "src/files.rs", "declarations": declarations}
        ]);
        let languages = Languages::of_project(dir.path());
        project_map::save_at(&map_of(dir), &json!({ "modules": modules }), "scan 1", &languages).unwrap();
    }

    fn map_of(dir: &TempDir) -> std::path::PathBuf {
        project_map::model_path(dir.path())
    }

    fn opened(dir: &TempDir) -> MapDb {
        MapDb::open(&map_of(dir), dir.path(), &[]).unwrap()
    }

    /// A primeira coluna, inteira, de cada linha da consulta.
    fn numbers(dir: &TempDir, sql: &str) -> Vec<i64> {
        let db = opened(dir);
        let mut statement = db.conn().prepare(sql).unwrap();
        let rows = statement.query_map([], |row| row.get::<_, i64>(0)).unwrap();
        rows.collect::<std::result::Result<_, _>>().unwrap()
    }

    fn count(dir: &TempDir, table: &str) -> i64 {
        opened(dir).conn().query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0)).unwrap()
    }

    /// O modelo embutido carrega sem rede e lê um texto: o vetor tem
    /// comprimento 1.
    #[test]
    fn the_embedded_model_loads_offline_and_reads_a_text() {
        let vector = text_vector("delete the file").expect("the embedded model loads");
        let length = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((length - 1.0).abs() < 1e-3, "unit length: {length}");
    }

    /// Os arquivos do modelo cabem no teto de 30 MB do binário.
    #[test]
    fn the_embedded_model_stays_under_thirty_megabytes() {
        assert!(TOKENIZER.len() + WEIGHTS.len() + CONFIG.len() <= 30_000_000);
    }

    /// O mapa sem vetores continua abrindo e lendo como sempre, e quem pergunta
    /// pelas declarações perto de um texto recebe uma lista vazia.
    #[test]
    fn a_map_without_vectors_opens_and_reads_as_before() {
        let dir = two_files();
        let db = opened(&dir);
        assert!(!table_exists(db.conn(), "decl_vectors").unwrap() && !table_exists(db.conn(), "word_vectors").unwrap());
        assert!(ranked_declarations(db.conn(), "apagar").unwrap().is_empty());
        assert!(project_map::read(dir.path()).is_ok());
    }

    /// Depois de encher o bloco, o mapa continua lendo como mapa, cada
    /// declaração tem o vetor dela e cada vetor tem 256 números.
    #[test]
    fn filling_the_block_gives_each_declaration_a_vector_and_keeps_the_map_readable() {
        let dir = two_files();
        let report = fill_at(&map_of(&dir), dir.path()).unwrap();
        assert_eq!((report.declarations, report.computed), (3, 3), "{report:?}");
        assert!(report.words > 10, "{report:?}");
        assert_eq!(count(&dir, "decl_vectors"), 3);
        let lengths = numbers(&dir, "SELECT length(vector) FROM decl_vectors");
        assert_eq!(lengths, vec![256; 3]);
        assert!(project_map::read(dir.path()).is_ok(), "the map still reads");
    }

    /// A tabela de palavras guarda, de cada palavra, as formas que o índice de
    /// busca grava para ela: a raiz, a mesma de `domain::normalize`.
    #[test]
    fn each_word_keeps_the_forms_the_search_index_writes() {
        let dir = saved(&json!([{"path": "src/a.rs", "declarations": [function("charge", 1, "Charging the invoices")]}]));
        fill_at(&map_of(&dir), dir.path()).unwrap();
        let db = opened(&dir);
        let forms: String =
            db.conn().query_row("SELECT forms FROM word_vectors WHERE word = 'invoices'", [], |row| row.get(0)).unwrap();
        let mut normalizer = Normalizer::new(&Languages::of_project(dir.path()));
        assert_eq!(forms, normalizer.word_forms("invoices").join(" "));
        assert!(forms.contains("invoic"), "{forms}");
    }

    /// Só a declaração nova ou mudada tem o vetor calculado de novo: a segunda
    /// passada não calcula nenhum, a doc mudada de uma calcula uma, e a que sai
    /// do mapa perde o vetor e as palavras que só ela tinha.
    #[test]
    fn only_a_changed_or_new_declaration_gets_a_new_vector() {
        let dir = two_files();
        let map = map_of(&dir);
        assert_eq!(fill_at(&map, dir.path()).unwrap().computed, 3);
        assert_eq!(fill_at(&map, dir.path()).unwrap().computed, 0, "nothing changed");

        rewrite(&dir, "Delete the file and its backup.", true);
        assert_eq!(fill_at(&map, dir.path()).unwrap().computed, 1, "one declaration changed");
        assert_eq!(fill_at(&map, dir.path()).unwrap().computed, 0);

        let words_before = count(&dir, "word_vectors");
        rewrite(&dir, "Delete the file and its backup.", false);
        let report = fill_at(&map, dir.path()).unwrap();
        assert_eq!((report.declarations, report.computed), (2, 0), "{report:?}");
        assert_eq!(count(&dir, "decl_vectors"), 2);
        assert!(count(&dir, "word_vectors") < words_before, "the words only that declaration had are gone");
        let db = opened(&dir);
        let left: i64 =
            db.conn().query_row("SELECT count(*) FROM word_vectors WHERE word = 'invoice'", [], |row| row.get(0)).unwrap();
        assert_eq!(left, 0);
    }

    /// Trocar a língua do texto no `mustard.json` refaz as formas de cada
    /// palavra na passada seguinte, sem calcular vetor de declaração de novo;
    /// sem troca, a passada não mexe nas palavras.
    /// A passada que não gravou nada no mapa não relê nem compara declaração
    /// nenhuma: o preenchimento guarda o carimbo das gravações e, com o mesmo
    /// carimbo e as mesmas línguas, devolve os números de antes sem tocar nas
    /// tabelas. A passada que gravou algo, mesmo uma só declaração, faz a
    /// conta inteira; e quem apagou o carimbo (o preenchimento que não
    /// terminou) faz também.
    #[test]
    fn a_pass_that_wrote_nothing_does_not_reread_the_declarations() {
        let dir = two_files();
        let map = map_of(&dir);
        let first = fill_at(&map, dir.path()).unwrap();
        assert_eq!((first.declarations, first.computed), (3, 3));

        // Uma linha de vetor trocada por fora não é gravação de conteúdo: o
        // carimbo é o mesmo, e a passada nem lê as declarações para ver.
        let edit = Connection::open(&map).unwrap();
        edit.execute("UPDATE decl_vectors SET hash = 1 WHERE name = 'delete_file'", []).unwrap();
        drop(edit);
        let quiet = fill_at(&map, dir.path()).unwrap();
        assert_eq!((quiet.declarations, quiet.computed, quiet.words), (3, 0, first.words), "{quiet:?}");
        let hash: i64 = opened(&dir)
            .conn()
            .query_row("SELECT hash FROM decl_vectors WHERE name = 'delete_file'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(hash, 1, "the quiet pass did not compare the fingerprints");

        // A gravação pela porta do scan muda o carimbo, e a conta se refaz.
        rewrite(&dir, "Delete the file and keep a copy of it.", true);
        let after_write = fill_at(&map, dir.path()).unwrap();
        assert_eq!(after_write.computed, 1, "the tampered fingerprint is found again: {after_write:?}");

        // Sem o carimbo guardado, a passada faz a conta.
        let edit = Connection::open(&map).unwrap();
        edit.execute("DELETE FROM meaning_meta WHERE key = 'stamp'", []).unwrap();
        edit.execute("UPDATE decl_vectors SET hash = 1 WHERE name = 'delete_file'", []).unwrap();
        drop(edit);
        assert_eq!(fill_at(&map, dir.path()).unwrap().computed, 1, "no stamp, no shortcut");
    }

    #[test]
    fn a_changed_project_language_rewrites_the_word_forms_and_nothing_else() {
        let dir = saved(&json!([{"path": "src/a.rs", "declarations": [
            function("charge", 1, "Charging the invoices and faturamentos")]}]));
        let map = map_of(&dir);
        let forms_of = |word: &str| -> String {
            opened(&dir)
                .conn()
                .query_row("SELECT forms FROM word_vectors WHERE word = ?1", [word], |row| row.get(0))
                .unwrap()
        };
        let words = ["invoices", "charging", "faturamentos"];
        fill_at(&map, dir.path()).unwrap();
        let before: Vec<String> = words.iter().map(|word| forms_of(word)).collect();
        std::fs::write(dir.path().join("mustard.json"), r#"{"language":{"text":"en-US","code":"en-US"}}"#).unwrap();
        let report = fill_at(&map, dir.path()).unwrap();
        assert_eq!(report.computed, 0, "no declaration changed");
        let mut normalizer = Normalizer::new(&Languages::of_project(dir.path()));
        let after: Vec<String> = words.iter().map(|word| forms_of(word)).collect();
        for (word, forms) in words.iter().zip(&after) {
            assert_eq!(forms, &normalizer.word_forms(word).join(" "), "{word}");
        }
        assert_ne!(after, before, "the forms follow the language of the project");
    }

    /// Duas declarações de mesmo nome no mesmo arquivo têm um vetor cada, pela
    /// ordem entre elas.
    #[test]
    fn declarations_with_the_same_name_keep_one_vector_each() {
        let dir = saved(&json!([{"path": "src/a.rs", "declarations": [
            function("new", 1, "Builds the reader."), function("new", 9, "Builds the writer.")]}]));
        fill_at(&map_of(&dir), dir.path()).unwrap();
        let nths = numbers(&dir, "SELECT nth FROM decl_vectors ORDER BY nth");
        assert_eq!(nths, vec![0, 1]);
    }

    /// O texto compilado junta, sem corte e nesta ordem: o nome em palavras, o
    /// tipo, a documentação, os textos de que a declaração é dona, o comentário
    /// do corpo, a assinatura, a documentação inteira, os membros, o dono em
    /// palavras, os supertipos, as palavras do caminho, os títulos de commit e a
    /// nota escrita.
    #[test]
    fn the_compiled_text_joins_the_fields_in_the_agreed_order() {
        let owned = vec!["creating directory".to_string()];
        let titles = vec!["Fix the backup".to_string()];
        let text = compiled_text(&Parts {
            file: "src/CopyDir.rs",
            kind: "function",
            name: "copyDir",
            doc: "Copies the folder.",
            signature: "fn copy_dir()",
            whole_doc: "Copies the folder. Slowly.",
            body_comment: "walks the tree",
            members: "[\"a\"]",
            owner: "FileOps",
            supertypes: "[\"Drop\"]",
            owned_texts: &owned,
            titles: &titles,
            notes: "Copies a tree for the backup.",
        });
        assert_eq!(
            text,
            "copy dir function Copies the folder. creating directory walks the tree fn copy_dir() \
             Copies the folder. Slowly. [\"a\"] file ops [\"Drop\"] src copy dir rs Fix the backup \
             Copies a tree for the backup."
        );
    }

    /// Os títulos de commit do compilado são os da história da declaração,
    /// quando o mapa já a leu; sem ela, os cinco mais novos do arquivo.
    #[test]
    fn the_commit_titles_come_from_the_declaration_history_or_from_the_newest_of_the_file() {
        let dir = saved(&json!([{"path": "src/a.rs", "declarations": [
            function("with_history", 1, "One."), function("without_history", 9, "Two.")]}]));
        let edit = Connection::open(map_of(&dir)).unwrap();
        edit.execute_batch("INSERT INTO history_paths(path) VALUES ('src/a.rs');").unwrap();
        for (n, title) in ["one", "two", "three", "four", "five", "six", "seven"].iter().enumerate() {
            edit.execute(
                "INSERT INTO commits(id, at, title, pr, added, changed) VALUES (?1, ?2, ?3, 0, '[]', '[0]')",
                params![format!("c{n}"), n as i64, format!("title {title}")],
            )
            .unwrap();
        }
        edit.execute("INSERT INTO lineage_commits(path, id, at, title, pr, files) VALUES ('src/a.rs', 'c1', 1, 'title two', 0, '{}')", [])
            .unwrap();
        edit.execute(
            "INSERT INTO lineage_decls(path, name, nth, commits, comments) VALUES ('src/a.rs', 'with_history', 0, '[{\"id\":\"c1\"}]', NULL)",
            [],
        )
        .unwrap();
        drop(edit);
        let db = opened(&dir);
        let read = read_declarations(db.conn()).unwrap();
        let with = read.iter().find(|d| d.name == "with_history").unwrap();
        assert!(with.text.ends_with("title two"), "{}", with.text);
        let without = read.iter().find(|d| d.name == "without_history").unwrap();
        assert!(without.text.ends_with("title seven title six title five title four title three"), "{}", without.text);
    }

    /// O bloco entra na tabela de blocos com a versão do programa, e um mapa
    /// sem declarações não ganha vetor nenhum.
    #[test]
    fn the_block_is_recorded_and_an_empty_map_gets_no_vectors() {
        let dir = saved(&json!([]));
        let report = fill_at(&map_of(&dir), dir.path()).unwrap();
        assert_eq!(report, Report::default());
        assert_eq!(opened(&dir).version(BLOCK_NAME).unwrap(), Some(BLOCK_VERSION));
    }
}
