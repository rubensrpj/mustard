//! `map_glossary` — o glossário que o mapa aprende do uso: a palavra da
//! pergunta ligada ao nome da declaração que o Claude editou logo depois de
//! buscá-la.
//!
//! A busca acha pelas palavras escritas no código. Quem pergunta usa as
//! dele: a pergunta em português não chega à função de nome em inglês. O
//! glossário guarda o par que o trabalho confirmou.
//!
//! Cada busca do mapa grava, por sessão, as palavras da pergunta, com as
//! formas da normalização de toda busca, e os lugares que ela entregou; a
//! leitura de uma declaração e a de quem a usa acrescentam os lugares delas à
//! mesma busca. Depois de cada edição, o gancho passa o arquivo e as linhas
//! que ela mudou: a declaração mudada que a última busca da sessão entregou
//! ganha a marca de cada palavra da pergunta que o nome dela ainda não tem.
//! Depois de uma busca sem resultado, a primeira declaração editada conta
//! também. Abrir um arquivo nunca ensina: só a edição chega aqui.
//!
//! A busca por palavra (`io::map_search`) lê as marcas como mais um campo. A
//! marca se prende ao arquivo e ao nome da declaração e sai quando o scan
//! refaz as declarações e esse nome não está mais lá.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::domain::normalize::{Languages, Normalizer};
use crate::domain::project_map::MapRefusal;
use crate::io::map_db::MapDb;
use crate::io::project_map::{open_existing, open_existing_waiting};
use crate::platform::error::{Error, Result};

/// Por quanto tempo a busca de uma sessão ainda ensina: um dia. A mais velha
/// sai na gravação da busca seguinte, de qualquer sessão.
const ASK_KEPT_SECS: i64 = 86_400;

/// Um lugar que a busca entregou: o arquivo e as linhas da declaração; o
/// arquivo inteiro tem a linha 0.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Place {
    pub file: String,
    #[serde(default)]
    pub line: u64,
    #[serde(default)]
    pub end_line: u64,
}

impl Place {
    /// O arquivo `file` inteiro.
    #[must_use]
    pub fn whole(file: impl Into<String>) -> Self {
        Self { file: file.into(), line: 0, end_line: 0 }
    }

    /// `true` quando a declaração do arquivo `file` que vai da linha `line`
    /// à `end_line` cai neste lugar. A última linha 0 é a que não se sabe: o
    /// lugar vai até o fim do arquivo, e a declaração é só a linha dela.
    fn holds(&self, file: &str, line: u64, end_line: u64) -> bool {
        if self.file != file {
            return false;
        }
        if self.line == 0 {
            return true;
        }
        let end = if self.end_line == 0 { u64::MAX } else { self.end_line };
        self.line <= line && end_line.max(line) <= end
    }
}

/// Uma marca: as formas da palavra da pergunta e a declaração, pelo arquivo
/// e pelo nome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mark {
    pub forms: Vec<String>,
    pub file: String,
    pub name: String,
}

/// O trecho que uma edição mudou, nas linhas do arquivo de antes dela: da
/// primeira à última, as duas incluídas. A linha acrescentada entre duas
/// outras é o trecho dessas duas.
pub type Touched = (u64, u64);

/// O nível da busca que lê as marcas: a declaração marcada, ou o arquivo
/// dela.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Learned {
    Decls,
    Files,
}

/// Grava no mapa em `model` a busca que a sessão `session` acabou de fazer,
/// no lugar da anterior dela: as palavras de `query`, cortadas nas línguas
/// `languages` como a busca as corta, e os lugares `found` que ela entregou.
/// Sem nenhum lugar, a primeira declaração editada em seguida ensina.
///
/// # Errors
///
/// Sem o mapa, com ele ilegível ou quando a gravação falha.
pub fn record_search(model: &Path, session: &str, query: &str, languages: &Languages, found: &[Place]) -> Result<()> {
    let words = Normalizer::new(languages).query(query);
    let now = now();
    let mut db = opened(open_existing(model))?;
    db.write(|tx| {
        tx.execute("DELETE FROM glossary_asks WHERE session = ?1 OR at < ?2", params![session, now - ASK_KEPT_SECS])?;
        tx.execute(
            "INSERT INTO glossary_asks(session, words, found, first_edit, at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![session, json(&words)?, json(found)?, found.is_empty(), now],
        )?;
        Ok(())
    })
}

/// Acrescenta os lugares `found` à última busca da sessão `session` no mapa
/// em `model`: a leitura de uma declaração ou de quem a usa entrega lugares
/// pelas mesmas palavras. Sem busca gravada da sessão, não há palavra a
/// ligar, e nada muda.
///
/// # Errors
///
/// Sem o mapa, com ele ilegível ou quando a gravação falha.
pub fn add_delivered(model: &Path, session: &str, found: &[Place]) -> Result<()> {
    if found.is_empty() {
        return Ok(());
    }
    let mut db = opened(open_existing(model))?;
    db.write(|tx| {
        let Some(stored) = tx
            .query_row("SELECT found FROM glossary_asks WHERE session = ?1", [session], |row| row.get::<_, String>(0))
            .optional()?
        else {
            return Ok(());
        };
        let mut places: Vec<Place> = serde_json::from_str(&stored).unwrap_or_default();
        for place in found {
            if !places.contains(place) {
                places.push(place.clone());
            }
        }
        tx.execute("UPDATE glossary_asks SET found = ?2 WHERE session = ?1", params![session, json(&places)?])?;
        Ok(())
    })
}

/// A última busca de uma sessão, como o gancho da edição a lê.
struct Ask {
    words: Vec<Vec<String>>,
    found: Vec<Place>,
    first_edit: bool,
}

/// Uma declaração do nível das declarações que a edição mudou.
struct Edited {
    name: String,
    line: u64,
    end_line: u64,
}

/// Confirma a edição dos trechos `touched` do arquivo `file` — o caminho
/// dele a partir da raiz do projeto — pela sessão `session`, no mapa em
/// `model`, e devolve as marcas novas. Cada trecho é da declaração mais
/// interna do mapa que o contém inteiro. A declaração mudada que a última
/// busca da sessão entregou ganha a marca de cada palavra da pergunta que
/// ainda não casa com o nome dela, cortado nas línguas `languages`; depois de
/// uma busca sem resultado, a primeira declaração mudada ganha, e a busca
/// deixa de ensinar assim. A trava de outra gravação se espera por `wait`, e
/// não mais.
///
/// # Errors
///
/// Sem o mapa, com ele ilegível, travado além de `wait`, ou quando a
/// gravação falha.
pub fn confirm_edit(
    model: &Path,
    session: &str,
    file: &str,
    touched: &[Touched],
    languages: &Languages,
    wait: Duration,
) -> Result<Vec<Mark>> {
    if touched.is_empty() {
        return Ok(Vec::new());
    }
    let mut db = opened(open_existing_waiting(model, wait))?;
    let Some(ask) = last_ask(db.conn(), session)? else { return Ok(Vec::new()) };
    let edited = edited_in(db.conn(), file, touched)?;
    let Some(first) = edited.first() else { return Ok(Vec::new()) };
    let mut taught: Vec<&Edited> =
        edited.iter().filter(|decl| ask.found.iter().any(|place| place.holds(file, decl.line, decl.end_line))).collect();
    if taught.is_empty() && ask.first_edit {
        taught.push(first);
    }
    if taught.is_empty() {
        return Ok(Vec::new());
    }
    let mut normalizer = Normalizer::new(languages);
    let mut marks: Vec<Mark> = Vec::new();
    for decl in taught {
        let named: HashSet<String> = normalizer.forms(&decl.name).into_iter().flatten().collect();
        for forms in &ask.words {
            if forms.is_empty() || forms.iter().any(|form| named.contains(form)) {
                continue;
            }
            let mark = Mark { forms: forms.clone(), file: file.to_string(), name: decl.name.clone() };
            if !marks.contains(&mark) {
                marks.push(mark);
            }
        }
    }
    db.write(|tx| {
        if ask.first_edit {
            tx.execute("UPDATE glossary_asks SET first_edit = 0 WHERE session = ?1", [session])?;
        }
        let mut added = Vec::new();
        for mark in marks {
            let forms = json(&mark.forms)?;
            let known = tx
                .query_row(
                    "SELECT 1 FROM glossary_marks WHERE forms = ?1 AND file = ?2 AND name = ?3",
                    params![forms, mark.file, mark.name],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if !known {
                tx.execute(
                    "INSERT INTO glossary_marks(forms, file, name) VALUES (?1, ?2, ?3)",
                    params![forms, mark.file, mark.name],
                )?;
                added.push(mark);
            }
        }
        Ok(added)
    })
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

/// Tira as marcas cuja declaração não está mais nas declarações do mapa:
/// mudou de nome, de arquivo ou sumiu. Roda na transação de quem refaz as
/// declarações.
pub(crate) fn drop_stale(conn: &Connection) -> Result<()> {
    conn.execute(
        "DELETE FROM glossary_marks WHERE rowid NOT IN \
         (SELECT m.rowid FROM glossary_marks m JOIN decls d ON d.file = m.file AND d.name = m.name)",
        [],
    )?;
    Ok(())
}

/// A última busca da sessão `session`, enquanto ela ainda ensina.
fn last_ask(conn: &Connection, session: &str) -> Result<Option<Ask>> {
    let found = conn
        .query_row(
            "SELECT words, found, first_edit FROM glossary_asks WHERE session = ?1 AND at >= ?2",
            params![session, now() - ASK_KEPT_SECS],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, bool>(2)?)),
        )
        .optional()?;
    Ok(found.map(|(words, found, first_edit)| Ask {
        words: serde_json::from_str(&words).unwrap_or_default(),
        found: serde_json::from_str(&found).unwrap_or_default(),
        first_edit,
    }))
}

/// As declarações do nível das declarações que os trechos `touched` do
/// arquivo `file` mudaram, na ordem dos trechos, sem repetir: de cada trecho,
/// a mais interna que o contém inteiro — a que começa mais abaixo. A
/// declaração sem a última linha gravada cobre só a linha dela.
fn edited_in(conn: &Connection, file: &str, touched: &[Touched]) -> Result<Vec<Edited>> {
    let mut stmt = conn.prepare(
        "SELECT d.name, d.line, d.end_line FROM decls d JOIN decl_lengths l ON l.id = d.rowid WHERE d.file = ?1",
    )?;
    let decls = stmt
        .query_map([file], |row| {
            let line = |at: usize| -> rusqlite::Result<u64> {
                Ok(u64::try_from(row.get::<_, Option<i64>>(at)?.unwrap_or(0)).unwrap_or(0))
            };
            Ok(Edited { name: row.get(0)?, line: line(1)?, end_line: line(2)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out: Vec<Edited> = Vec::new();
    for &(first, last) in touched {
        let inner = decls
            .iter()
            .filter(|decl| decl.line <= first && decl.end_line.max(decl.line) >= last)
            .max_by_key(|decl| decl.line);
        if let Some(decl) = inner
            && !out.iter().any(|seen| seen.name == decl.name && seen.line == decl.line)
        {
            out.push(Edited { name: decl.name.clone(), line: decl.line, end_line: decl.end_line });
        }
    }
    Ok(out)
}

/// O banco aberto, ou a recusa como erro de gravação.
fn opened(db: std::result::Result<MapDb, MapRefusal>) -> Result<MapDb> {
    db.map_err(|refusal| Error::Parse(format!("{refusal:?}")))
}

/// O valor em JSON, como a coluna o guarda.
fn json<T: Serialize + ?Sized>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|err| Error::Parse(err.to_string()))
}

/// A hora de agora, em segundos desde 1970.
fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| i64::try_from(since.as_secs()).unwrap_or(i64::MAX))
}
