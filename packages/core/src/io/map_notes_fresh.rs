//! `map_notes_fresh` — a leitura das notas de sentido em dia, o que a busca
//! por palavras e o sentido pelos vetores usam.
//!
//! A nota vale enquanto o blob do arquivo no mapa for o que ela guardou
//! (`io::map_notes`). Quem lê as notas para a busca fica aqui, num módulo que
//! só conhece o banco: a busca ([`crate::io::map_search`]) e o sentido
//! ([`crate::io::map_meaning`]) o usam, e `io::map_notes`, que grava a nota e
//! refaz o índice e os vetores, fica em cima dos dois.
//!
//! - no índice de palavras ([`fresh`]), a nota entra como mais um texto fixo
//!   do arquivo e da declaração que ela nomeia, com o peso de qualquer texto
//!   fixo;
//! - no texto compilado de cada declaração ([`Texts`]), de onde saem os
//!   vetores de sentido: a nota da declaração, e a do arquivo em todas as
//!   dele.
//!
//! Mapa sem nenhuma nota, ou sem a tabela, responde como sempre.

use std::collections::HashMap;

use rusqlite::{params_from_iter, Connection};

use crate::io::map_db::table_exists;
use crate::platform::error::Result;

/// Uma nota em dia como o índice a lê: o arquivo, a declaração, o texto e a
/// linha da declaração no arquivo (`u64::MAX` na nota do arquivo inteiro, que
/// não cai nas linhas de declaração nenhuma).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fresh {
    pub file: String,
    pub name: String,
    pub text: String,
    pub line: u64,
}

/// As notas em dia do mapa aberto em `conn`, na ordem em que foram escritas:
/// a cujo arquivo ainda tem o blob que ela guardou e, quando nomeia uma
/// declaração, cuja declaração o arquivo ainda tem — uma linha por declaração
/// de mesmo nome. Com `scope`, só as dos arquivos dados. A nota velha, a de
/// arquivo que saiu do mapa e a da declaração que sumiu ficam de fora. Sem a
/// tabela das notas, a lista é vazia.
pub(crate) fn fresh(conn: &Connection, scope: Option<&[&str]>) -> Result<Vec<Fresh>> {
    if !["notes", "files", "decls"].iter().all(|table| table_exists(conn, table).unwrap_or(false)) {
        return Ok(Vec::new());
    }
    let only = scope.map_or_else(String::new, |paths| {
        let slots: Vec<String> = (1..=paths.len()).map(|at| format!("?{at}")).collect();
        format!(" AND n.file IN ({})", slots.join(", "))
    });
    let mut statement = conn.prepare(&format!(
        "SELECT n.file, n.name, n.text, d.line, d.rowid FROM notes n \
         JOIN files f ON f.path = n.file AND f.blob IS n.blob \
         LEFT JOIN decls d ON d.file = n.file AND d.name = n.name AND n.name <> '' \
         WHERE (n.name = '' OR d.rowid IS NOT NULL){only} ORDER BY n.rowid, d.rowid"
    ))?;
    let mut rows = statement.query(params_from_iter(scope.unwrap_or_default()))?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        let line = if name.is_empty() {
            u64::MAX
        } else {
            u64::try_from(row.get::<_, Option<i64>>(3)?.unwrap_or(0)).unwrap_or(0)
        };
        out.push(Fresh { file: row.get(0)?, name, text: row.get(2)?, line });
    }
    Ok(out)
}

/// O texto das notas em dia por dono, para o texto compilado das declarações.
#[derive(Debug, Default)]
pub(crate) struct Texts {
    of_declaration: HashMap<(String, String), String>,
    of_file: HashMap<String, String>,
}

impl Texts {
    /// As notas em dia do mapa aberto em `conn`; vazio sem nenhuma.
    pub(crate) fn read(conn: &Connection) -> Result<Self> {
        let mut out = Self::default();
        for note in fresh(conn, None)? {
            if note.name.is_empty() {
                out.of_file.insert(note.file, note.text);
            } else {
                out.of_declaration.insert((note.file, note.name), note.text);
            }
        }
        Ok(out)
    }

    /// O que as notas dizem da declaração `name` do arquivo `file`: a dela e,
    /// depois, a do arquivo. Vazio quando nenhuma existe.
    pub(crate) fn of(&self, file: &str, name: &str) -> String {
        let own = self.of_declaration.get(&(file.to_string(), name.to_string())).map_or("", String::as_str);
        let whole = self.of_file.get(file).map_or("", String::as_str);
        format!("{own} {whole}").trim().to_string()
    }
}
