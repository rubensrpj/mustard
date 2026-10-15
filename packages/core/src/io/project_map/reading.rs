//! A abertura do mapa para quem só lê: a pergunta ao mapa não espera a
//! atualização de um bloco que ela não lê.

use std::path::Path;

use super::{MapBlock, open_existing, outdated_block, read_only_existing};
use crate::domain::project_map::MapRefusal;
use crate::io::map_db::MapDb;

/// O mapa em `model` aberto para ler só os blocos `among`. Quando algum bloco
/// do mapa está noutra versão do formato, mas nenhum dos de `among`, a
/// abertura é só de leitura: ela não refaz o bloco velho sob a trava de
/// gravação, que é da passada do scan, nem espera a trava de outra gravação.
/// Com um bloco de `among` noutra versão, a abertura é a de sempre
/// ([`open_existing`]), que o põe em dia antes da leitura.
pub(crate) fn open_reading(model: &Path, among: &[&MapBlock]) -> std::result::Result<MapDb, MapRefusal> {
    if let Ok(db) = read_only_existing(model)
        && outdated_block(&db).unwrap_or(false)
        && among.iter().all(|block| db.version(block.name()).is_ok_and(|found| found == Some(block.block.version)))
    {
        return Ok(db);
    }
    open_existing(model)
}

/// Se o mapa em `model` tem bloco noutra versão do formato, lido sem gravar
/// nem esperar trava: `None` quando o mapa não se abre assim.
pub(crate) fn has_outdated_block(model: &Path) -> Option<bool> {
    read_only_existing(model).ok().map(|db| outdated_block(&db).unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use serde_json::json;
    use tempfile::tempdir;

    use crate::domain::normalize::Languages;
    use crate::domain::project_map::MapRefusal;
    use crate::io::project_map::{Need, model_path, read_for_at, save_at};

    /// Um bloco que a pergunta não lê, noutra versão do formato, com outra
    /// gravação segurando a trava: as partes de um arquivo saem na hora, sem
    /// a espera da trava (cinco segundos) e sem refazer o bloco. Com a trava
    /// solta, o bloco que a pergunta lê, noutra versão, é posto em dia antes
    /// da leitura, como sempre.
    #[test]
    fn a_read_does_not_wait_for_a_block_it_does_not_read() {
        let dir = tempdir().unwrap();
        let model = model_path(dir.path());
        let map = json!({"modules": [{"path": "src/a.ts", "declarations": [{"name": "alpha", "kind": "function", "line": 1, "end_line": 3}]}]});
        save_at(&model, &map, "scan 1", &Languages::new([])).unwrap();
        let version = |name: &str| -> i64 {
            let conn = rusqlite::Connection::open(&model).unwrap();
            conn.query_row("SELECT version FROM blocks WHERE name = ?1", [name], |row| row.get(0)).unwrap()
        };
        let current = version("pulls");
        let writer = rusqlite::Connection::open(&model).unwrap();
        writer.execute("UPDATE blocks SET version = version - 1 WHERE name = 'pulls'", []).unwrap();
        writer.execute_batch("BEGIN IMMEDIATE").unwrap();

        let started = Instant::now();
        let read = read_for_at(&model, Need::Parts("src/a.ts")).unwrap();
        assert!(started.elapsed() < Duration::from_secs(1), "the read waited {:?}", started.elapsed());
        let parts = crate::domain::project_map::parts(&read, "src/a.ts").unwrap();
        assert_eq!(parts.parts.len(), 1, "{parts:?}");
        writer.execute_batch("COMMIT").unwrap();
        assert_eq!(version("pulls"), current - 1, "the block nobody read stays for the scan pass");

        writer.execute("UPDATE blocks SET version = version - 1 WHERE name = 'files'", []).unwrap();
        let files = version("files");
        let refused = read_for_at(&model, Need::Parts("src/a.ts")).unwrap_err();
        assert_eq!(refused, MapRefusal::MapUnfilled { blocks: vec!["files".to_string()] }, "rebuilt empty, for the scan to fill");
        assert_eq!(version("files"), files + 1, "the block the question reads is brought up to date");
    }
}
