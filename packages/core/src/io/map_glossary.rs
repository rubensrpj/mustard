//! `map_glossary` — as marcas do glossário que o mapa aprendeu do uso: a
//! palavra da pergunta ligada ao nome da declaração que o Claude editou logo
//! depois de buscá-la.
//!
//! A busca por palavra (`io::map_search`) lê as marcas como mais um campo. Um
//! mapa antigo guarda as que aprendeu; a marca se prende ao arquivo e ao nome
//! da declaração e sai quando o scan refaz as declarações e esse nome não está
//! mais lá.

use rusqlite::Connection;

use crate::platform::error::Result;

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
