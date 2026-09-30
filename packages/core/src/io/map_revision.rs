//! `map_revision` — o contador das gravações do conteúdo do mapa.
//!
//! Quem lê o mapa para calcular algo caro (os vetores de sentido de
//! `map_meaning`) precisa saber, antes de reler tudo, se alguma passada
//! gravou depois da última vez. Cada gravação de conteúdo (as declarações,
//! os textos, a história e os pull requests) soma 1 ao contador, que mora no
//! próprio arquivo do mapa (`PRAGMA user_version`), na mesma transação da
//! gravação: o contador e o conteúdo entram juntos ou nenhum dos dois.
//!
//! A [`stamp`] junta o contador com a versão e a marca de cada bloco, para a
//! troca de formato de um bloco também valer como gravação.

use std::fmt::Write as _;

use rusqlite::{Connection, OptionalExtension};

use crate::platform::error::Result;

/// O contador de gravações do mapa; zero no mapa que nenhuma gravação
/// passou por aqui.
///
/// # Errors
///
/// Quando o mapa não pode ser lido.
pub fn current(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("PRAGMA user_version", [], |row| row.get(0))?)
}

/// Soma 1 ao contador; vale dentro da transação da gravação.
pub(crate) fn bump(conn: &Connection) -> Result<()> {
    let next = current(conn)?.saturating_add(1);
    conn.pragma_update(None, "user_version", next)?;
    Ok(())
}

/// O estado do conteúdo do mapa numa palavra só: o contador e, de cada bloco,
/// o nome, a versão e a marca. Duas leituras iguais dizem que nenhuma
/// gravação de conteúdo passou entre elas.
///
/// # Errors
///
/// Quando o mapa não pode ser lido.
pub fn stamp(conn: &Connection) -> Result<String> {
    let mut out = current(conn)?.to_string();
    let listed: Option<i64> =
        conn.query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'blocks'", [], |row| row.get(0)).optional()?;
    if listed.is_some() {
        let mut statement = conn.prepare("SELECT name, version, mark FROM blocks ORDER BY name")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))
        })?;
        for row in rows {
            let (name, version, mark) = row?;
            let _ = write!(out, "|{name}:{version}:{mark}");
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_bump_adds_one_to_the_counter_and_changes_the_stamp() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE blocks(name TEXT PRIMARY KEY, version INTEGER NOT NULL, mark TEXT NOT NULL DEFAULT '')")
            .unwrap();
        conn.execute("INSERT INTO blocks(name, version, mark) VALUES ('decls', 13, 'scan 1')", []).unwrap();
        assert_eq!(current(&conn).unwrap(), 0);
        let before = stamp(&conn).unwrap();
        assert_eq!(stamp(&conn).unwrap(), before, "a read changes nothing");
        bump(&conn).unwrap();
        assert_eq!(current(&conn).unwrap(), 1);
        assert_ne!(stamp(&conn).unwrap(), before);
        let after = stamp(&conn).unwrap();
        conn.execute("UPDATE blocks SET version = 14 WHERE name = 'decls'", []).unwrap();
        assert_ne!(stamp(&conn).unwrap(), after, "a block converted to another version is a write");
    }

    #[test]
    fn a_map_without_the_block_table_has_the_counter_only() {
        let conn = Connection::open_in_memory().unwrap();
        assert_eq!(stamp(&conn).unwrap(), "0");
    }
}
