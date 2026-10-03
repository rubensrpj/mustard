//! `map_db` — o mapa do projeto como um arquivo SQLite só, em blocos, como um
//! banco de dados. O nome e o lugar do arquivo moram na porta do mapa
//! (`io::project_map`); aqui se abre o arquivo que ela der.
//!
//! Cada bloco é um grupo de tabelas com dono: o módulo que o declara diz o
//! nome, a versão do formato, as tabelas, o esquema e o tipo ([`Block`]). A
//! tabela `blocks` guarda, de cada bloco, o nome e a versão com que ele foi
//! gravado. Ao abrir ([`MapDb::open`]), cada bloco declarado cuja versão falta
//! ou difere da do programa é posto em dia sozinho, só ele, numa transação
//! própria:
//!
//! - o bloco refeito ([`Kind::Rebuilt`]) perde as tabelas dele e é refeito a
//!   partir do código, do git ou das specs, pela função que o dono passa;
//! - o bloco escrito por agente ([`Kind::Written`]) nunca é apagado: a função
//!   de conversão do dono leva as linhas da versão encontrada para a nova.
//!
//! A tabela de blocos guarda também a marca de quem encheu cada bloco: quem
//! grava a põe junto das linhas ([`set_mark`]), e o bloco refeito volta sem
//! ela. Assim quem lê sabe se o bloco foi enchido pela mesma versão dele.
//!
//! A tabela que o programa não conhece — de um bloco que um programa mais
//! novo gravou — fica como está: não se apaga, e o resto do mapa se lê.
//!
//! Toda gravação passa pela mesma porta ([`MapDb::write`]), numa transação
//! só, e quem lê nunca vê um bloco pela metade. O diário é o `WAL` do SQLite:
//! quem lê não espera quem grava, e quem grava não derruba quem lê, que é o
//! que a busca precisa enquanto o scan grava a história do projeto em lotes.
//! Com o diário `DELETE`, o leitor que chega entre dois commits seguidos podia
//! voltar com o banco travado sem passar pela espera de trava. Os arquivos que
//! o `WAL` põe ao lado do mapa existem só enquanto há conexão aberta: o último
//! a fechar os junta ao mapa e os apaga. Duas gravações ao mesmo tempo esperam
//! a vez pela espera de trava, em vez de uma recusar a outra.
//!
//! Bloco novo se declara no módulo dono, e este módulo não muda.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};

use crate::platform::error::{Error, Result};

/// A pasta do projeto onde o mapa mora.
pub(crate) const MAP_DIR: &str = ".claude";

/// Onde o scan grava o mapa, a partir da raiz do projeto, com barras normais.
/// É o texto que as recusas, os avisos e as listas do censo citam.
pub const MAP_FILE: &str = ".claude/grain.db";

/// O nome do arquivo do mapa, sem a pasta: o que as listas de arquivos de
/// dentro de `.claude/` citam. Sai de [`MAP_FILE`], que é o único lugar do
/// nome.
pub const MAP_FILE_NAME: &str = MAP_FILE.split_at(MAP_DIR.len() + 1).1;

/// Onde o scan grava o mapa, dentro da raiz do projeto.
#[must_use]
pub fn model_path(root: &Path) -> PathBuf {
    root.join(MAP_DIR).join(MAP_FILE_NAME)
}

/// A tabela que guarda o nome e a versão do formato de cada bloco. Nenhum
/// bloco pode usar este nome.
const BLOCKS_TABLE: &str = "blocks";

/// Quanto uma gravação espera a trava de outra antes de desistir. O scan
/// inteiro regrava o mapa em cerca de um segundo num projeto grande.
const BUSY_WAIT: Duration = Duration::from_secs(5);

/// Refaz um bloco a partir do código, do git ou das specs do projeto em
/// `root`. Roda dentro da transação que já apagou e recriou as tabelas do
/// bloco; um erro desfaz tudo, e o bloco fica como estava.
pub type Rebuild = fn(&Connection, &Path) -> Result<()>;

/// Converte as linhas de um bloco escrito por agente da versão encontrada
/// (o segundo argumento) para a versão do programa, sem apagá-las. Roda
/// dentro de uma transação; um erro desfaz tudo, e o bloco fica como estava.
/// A versão encontrada é 0 quando as tabelas existem sem linha na tabela de
/// blocos.
pub type Convert = fn(&Connection, u32) -> Result<()>;

/// Como um bloco volta quando a versão gravada difere da do programa.
#[derive(Debug, Clone, Copy)]
pub enum Kind {
    /// Refeito: o que ele guarda se tira de novo do código, do git ou das
    /// specs, então as tabelas velhas saem inteiras.
    Rebuilt(Rebuild),
    /// Escrito por agente (notas de sentido, palavras aprendidas): o que ele
    /// guarda não se refaz, então é convertido e nunca apagado.
    Written(Convert),
}

/// Um bloco do mapa, declarado pelo módulo dono.
#[derive(Debug, Clone, Copy)]
pub struct Block {
    /// O nome do bloco na tabela de blocos.
    pub name: &'static str,
    /// A versão do formato que este programa grava.
    pub version: u32,
    /// As tabelas do bloco, inclusive as virtuais: são elas que o bloco
    /// refeito apaga antes de rodar o esquema.
    pub tables: &'static [&'static str],
    /// Os `CREATE` do bloco, na versão deste programa.
    pub schema: &'static str,
    /// Refeito ou escrito por agente.
    pub kind: Kind,
}

impl From<rusqlite::Error> for Error {
    fn from(err: rusqlite::Error) -> Self {
        Self::Io(std::io::Error::other(err))
    }
}

/// O mapa aberto, com os blocos declarados já em dia.
#[derive(Debug)]
pub struct MapDb {
    conn: Connection,
}

impl MapDb {
    /// Abre o mapa gravado em `path`, criando o arquivo se faltar, e põe em
    /// dia cada bloco de `blocks` cuja versão falta ou difere: só aquele
    /// bloco, numa transação própria, com `root` — a raiz do projeto — para o
    /// bloco refeito. O bloco que não está em `blocks` fica como está.
    pub fn open(path: &Path, root: &Path, blocks: &[Block]) -> Result<Self> {
        Self::open_waiting(path, root, blocks, BUSY_WAIT)
    }

    /// [`Self::open`] com outra espera pela trava: quem não pode segurar a
    /// ação de quem chamou — o gancho que roda depois de cada edição — espera
    /// pouco e desiste, em vez de esperar a regravação inteira do scan.
    pub fn open_waiting(path: &Path, root: &Path, blocks: &[Block], wait: Duration) -> Result<Self> {
        for block in blocks {
            if block.name == BLOCKS_TABLE || block.tables.contains(&BLOCKS_TABLE) {
                return Err(Error::config(format!("map block `{}` uses the reserved name `{BLOCKS_TABLE}`", block.name)));
            }
        }
        if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            crate::io::fs::create_dir_all(dir)?;
        }
        let mut conn = Connection::open(path)?;
        // A espera vem antes de tudo: até trocar o diário pede a trava.
        conn.busy_timeout(wait)?;
        // Um mapa em outro diário passa ao `WAL`, que fica gravado no arquivo:
        // as aberturas seguintes não pedem trava nenhuma. O disco que não
        // guarda o `WAL` (memória compartilhada que ele não dá) deixa o mapa
        // no `DELETE`, que também vale; qualquer outro diário é recusado.
        let mode: String = conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") && !mode.eq_ignore_ascii_case("delete") {
            return Err(Error::Io(std::io::Error::other(format!("map journal stayed `{mode}`"))));
        }
        for block in blocks {
            keep_up(&mut conn, block, root)?;
        }
        Ok(Self { conn })
    }

    /// A conexão, para ler. Cada consulta vê só transações inteiras.
    #[must_use]
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// A versão com que o bloco `name` está gravado; `None` quando ele falta.
    #[cfg(test)]
    pub(crate) fn version(&self, name: &str) -> Result<Option<u32>> {
        stored_version(&self.conn, name)
    }

    /// A marca de quem encheu o bloco `name` ([`set_mark`]): vazia quando
    /// ninguém marcou depois que ele nasceu ou foi refeito; `None` quando o
    /// bloco falta.
    pub fn mark(&self, name: &str) -> Result<Option<String>> {
        if !table_exists(&self.conn, BLOCKS_TABLE)? {
            return Ok(None);
        }
        let mark = self
            .conn
            .query_row(&format!("SELECT mark FROM {BLOCKS_TABLE} WHERE name = ?1"), params![name], |row| row.get(0))
            .optional()?;
        Ok(mark)
    }

    /// A porta de toda gravação: `write` roda numa transação só, que pega a
    /// trava de gravação logo no começo e espera a vez se outra gravação a
    /// segura. Com `Ok`, grava tudo; com erro, nada.
    pub fn write<T>(&mut self, write: impl FnOnce(&Transaction<'_>) -> Result<T>) -> Result<T> {
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let value = write(&tx)?;
        tx.commit()?;
        Ok(value)
    }
}

/// Marca o bloco `name` como enchido por `mark`, dentro da transação da
/// gravação ([`MapDb::write`]): a marca e as linhas entram juntas. O bloco
/// tem de estar declarado na abertura; o que falta não ganha marca.
pub fn set_mark(conn: &Connection, name: &str, mark: &str) -> Result<()> {
    conn.execute(&format!("UPDATE {BLOCKS_TABLE} SET mark = ?2 WHERE name = ?1"), params![name, mark])?;
    Ok(())
}

/// Põe `block` em dia, quando a versão gravada falta ou difere da do
/// programa. A versão é conferida de novo depois de pegar a trava: outro
/// processo pode ter feito o mesmo enquanto este esperava.
fn keep_up(conn: &mut Connection, block: &Block, root: &Path) -> Result<()> {
    if stored_version(conn, block.name)? == Some(block.version) {
        return Ok(());
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS {BLOCKS_TABLE}(name TEXT PRIMARY KEY, version INTEGER NOT NULL, mark TEXT NOT NULL DEFAULT '')"
    ))?;
    let found = stored_version(&tx, block.name)?;
    if found == Some(block.version) {
        return Ok(());
    }
    // O bloco refeito perde a marca de quem o encheu; o convertido guarda as
    // linhas e a marca delas.
    let keep_mark = match block.kind {
        Kind::Rebuilt(rebuild) => {
            for table in block.tables {
                tx.execute_batch(&format!("DROP TABLE IF EXISTS {}", quoted(table)))?;
            }
            tx.execute_batch(block.schema)?;
            rebuild(&tx, root)?;
            false
        }
        Kind::Written(convert) => {
            match found {
                Some(from) => convert(&tx, from)?,
                None if any_table_exists(&tx, block.tables)? => convert(&tx, 0)?,
                None => tx.execute_batch(block.schema)?,
            }
            true
        }
    };
    tx.execute(
        &format!(
            "INSERT INTO {BLOCKS_TABLE}(name, version) VALUES (?1, ?2) ON CONFLICT(name) DO UPDATE SET version = excluded.version, \
             mark = CASE WHEN ?3 THEN mark ELSE '' END"
        ),
        params![block.name, block.version, keep_mark],
    )?;
    tx.commit()?;
    Ok(())
}

/// A versão gravada do bloco `name`; `None` quando a tabela de blocos ou a
/// linha dele faltam.
fn stored_version(conn: &Connection, name: &str) -> Result<Option<u32>> {
    if !table_exists(conn, BLOCKS_TABLE)? {
        return Ok(None);
    }
    let version = conn
        .query_row(&format!("SELECT version FROM {BLOCKS_TABLE} WHERE name = ?1"), params![name], |row| row.get(0))
        .optional()?;
    Ok(version)
}

/// Se o banco aberto em `conn` tem a tabela `name`.
pub(crate) fn table_exists(conn: &Connection, name: &str) -> Result<bool> {
    let found = conn
        .query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1", params![name], |_| Ok(()))
        .optional()?;
    Ok(found.is_some())
}

fn any_table_exists(conn: &Connection, tables: &[&str]) -> Result<bool> {
    for table in tables {
        if table_exists(conn, table)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// O nome de tabela entre aspas duplas, com as aspas de dentro dobradas.
fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::path::PathBuf;
    use tempfile::tempdir;

    /// O nome do mapa nos testes: qualquer um serve, o módulo abre o que
    /// recebe.
    const MAP_FILE_NAME: &str = "map.db";

    /// Onde os testes gravam o mapa do projeto em `root`.
    fn map_in(root: &Path) -> PathBuf {
        root.join(".claude").join(MAP_FILE_NAME)
    }

    fn rows(db: &MapDb, sql: &str) -> Vec<String> {
        let mut stmt = db.conn().prepare(sql).unwrap();
        stmt.query_map([], |row| row.get::<_, String>(0)).unwrap().map(|row| row.unwrap()).collect()
    }

    fn put(db: &mut MapDb, sql: &'static str) {
        db.write(|tx| {
            tx.execute_batch(sql)?;
            Ok(())
        })
        .unwrap();
    }

    const DECLS_V1: Block = Block {
        name: "decls",
        version: 1,
        tables: &["decls", "decls_fts"],
        schema: "CREATE TABLE decls(name TEXT NOT NULL);
                 CREATE VIRTUAL TABLE decls_fts USING fts5(name, tokenize='unicode61 remove_diacritics 2');",
        kind: Kind::Rebuilt(|conn, _| {
            conn.execute_batch("INSERT INTO decls VALUES ('from code v1'); INSERT INTO decls_fts VALUES ('buscar pedido');")?;
            Ok(())
        }),
    };

    const DECLS_V2: Block = Block {
        name: "decls",
        version: 2,
        tables: &["decls", "decls_fts"],
        schema: "CREATE TABLE decls(name TEXT NOT NULL, words TEXT NOT NULL);
                 CREATE VIRTUAL TABLE decls_fts USING fts5(name, words, tokenize='unicode61 remove_diacritics 2');",
        kind: Kind::Rebuilt(|conn, _| {
            conn.execute_batch("INSERT INTO decls VALUES ('from code v2', 'from code');")?;
            Ok(())
        }),
    };

    const HISTORY_V1: Block = Block {
        name: "history",
        version: 1,
        tables: &["history"],
        schema: "CREATE TABLE history(commit_message TEXT NOT NULL);",
        kind: Kind::Rebuilt(|conn, _| {
            conn.execute_batch("INSERT INTO history VALUES ('from git');")?;
            Ok(())
        }),
    };

    const NOTES_V1: Block = Block {
        name: "notes",
        version: 1,
        tables: &["notes"],
        schema: "CREATE TABLE notes(path TEXT NOT NULL, note TEXT NOT NULL);",
        kind: Kind::Written(|_, _| Ok(())),
    };

    const NOTES_V2: Block = Block {
        name: "notes",
        version: 2,
        tables: &["notes"],
        schema: "CREATE TABLE notes(path TEXT NOT NULL, note TEXT NOT NULL, stale INTEGER NOT NULL DEFAULT 0);",
        kind: Kind::Written(|conn, from| {
            if from < 2 {
                conn.execute_batch("ALTER TABLE notes ADD COLUMN stale INTEGER NOT NULL DEFAULT 0;")?;
            }
            Ok(())
        }),
    };

    const NOTES_BROKEN: Block = Block {
        name: "notes",
        version: 3,
        tables: &["notes"],
        schema: "CREATE TABLE notes(path TEXT NOT NULL);",
        kind: Kind::Written(|conn, _| {
            conn.execute_batch("ALTER TABLE notes RENAME TO notes_old;")?;
            Err(Error::config("the conversion does not understand this version"))
        }),
    };

    /// Um bloco que só um programa mais novo conhece.
    const ROUTES_NEWER: Block = Block {
        name: "routes",
        version: 1,
        tables: &["routes"],
        schema: "CREATE TABLE routes(route TEXT NOT NULL);",
        kind: Kind::Rebuilt(|conn, _| {
            conn.execute_batch("INSERT INTO routes VALUES ('/orders');")?;
            Ok(())
        }),
    };

    #[test]
    fn a_rebuilt_block_with_an_old_version_is_redone_without_touching_the_others() {
        let dir = tempdir().unwrap();
        let mut db = MapDb::open(&map_in(dir.path()), dir.path(), &[DECLS_V1, HISTORY_V1]).unwrap();
        put(&mut db, "INSERT INTO history VALUES ('written after the rebuild');");
        drop(db);

        let db = MapDb::open(&map_in(dir.path()), dir.path(), &[DECLS_V2, HISTORY_V1]).unwrap();
        assert_eq!(rows(&db, "SELECT name || '|' || words FROM decls"), vec!["from code v2|from code"]);
        assert!(rows(&db, "SELECT name FROM decls_fts WHERE decls_fts MATCH 'pedido'").is_empty());
        assert_eq!(db.version("decls").unwrap(), Some(2));
        // O outro bloco não foi refeito: a linha gravada depois continua.
        assert_eq!(rows(&db, "SELECT commit_message FROM history"), vec!["from git", "written after the rebuild"]);
        assert_eq!(db.version("history").unwrap(), Some(1));
    }

    /// A marca entra com as linhas; o bloco refeito volta sem ela, e o outro
    /// fica com a sua.
    #[test]
    fn a_rebuilt_block_loses_the_mark_of_who_filled_it_and_the_others_keep_theirs() {
        let dir = tempdir().unwrap();
        let mut db = MapDb::open(&map_in(dir.path()), dir.path(), &[DECLS_V1, HISTORY_V1]).unwrap();
        assert_eq!(db.mark("decls").unwrap().as_deref(), Some(""), "a block that was just born has no mark");
        assert_eq!(db.mark("routes").unwrap(), None, "a block that is not there has no mark either");
        db.write(|tx| {
            set_mark(tx, "decls", "scan 1")?;
            set_mark(tx, "history", "scan 1")
        })
        .unwrap();
        drop(db);

        let db = MapDb::open(&map_in(dir.path()), dir.path(), &[DECLS_V2, HISTORY_V1]).unwrap();
        assert_eq!(db.mark("decls").unwrap().as_deref(), Some(""), "the rebuilt block lost the mark");
        assert_eq!(db.mark("history").unwrap().as_deref(), Some("scan 1"), "the other block kept it");
    }

    #[test]
    fn an_agent_written_block_with_an_old_version_is_converted_and_its_rows_stay() {
        let dir = tempdir().unwrap();
        let mut db = MapDb::open(&map_in(dir.path()), dir.path(), &[NOTES_V1]).unwrap();
        put(&mut db, "INSERT INTO notes VALUES ('a.rs', 'reads the git log'), ('b.rs', 'writes the map');");
        drop(db);

        let db = MapDb::open(&map_in(dir.path()), dir.path(), &[NOTES_V2]).unwrap();
        assert_eq!(
            rows(&db, "SELECT path || '|' || note || '|' || stale FROM notes ORDER BY path"),
            vec!["a.rs|reads the git log|0", "b.rs|writes the map|0"]
        );
        assert_eq!(db.version("notes").unwrap(), Some(2));
    }

    #[test]
    fn a_failed_conversion_leaves_the_agent_written_block_as_it_was() {
        let dir = tempdir().unwrap();
        let mut db = MapDb::open(&map_in(dir.path()), dir.path(), &[NOTES_V1]).unwrap();
        put(&mut db, "INSERT INTO notes VALUES ('a.rs', 'reads the git log');");
        drop(db);

        assert!(MapDb::open(&map_in(dir.path()), dir.path(), &[NOTES_BROKEN]).is_err());
        let db = MapDb::open(&map_in(dir.path()), dir.path(), &[]).unwrap();
        assert_eq!(rows(&db, "SELECT note FROM notes"), vec!["reads the git log"]);
        assert_eq!(db.version("notes").unwrap(), Some(1));
    }

    #[test]
    fn an_unknown_table_survives_a_rewrite() {
        let dir = tempdir().unwrap();
        drop(MapDb::open(&map_in(dir.path()), dir.path(), &[DECLS_V1, ROUTES_NEWER]).unwrap());

        // Um programa que não conhece as rotas refaz as declarações e grava.
        let mut db = MapDb::open(&map_in(dir.path()), dir.path(), &[DECLS_V2]).unwrap();
        put(&mut db, "DELETE FROM decls; INSERT INTO decls VALUES ('rewritten', 'rewritten');");
        assert_eq!(rows(&db, "SELECT route FROM routes"), vec!["/orders"]);
        assert_eq!(db.version("routes").unwrap(), Some(1));
    }

    #[test]
    fn a_reader_opens_a_map_with_a_block_it_does_not_know_and_reads_the_others() {
        let dir = tempdir().unwrap();
        drop(MapDb::open(&map_in(dir.path()), dir.path(), &[DECLS_V1, HISTORY_V1, ROUTES_NEWER]).unwrap());

        let older = MapDb::open(&map_in(dir.path()), dir.path(), &[DECLS_V1, HISTORY_V1]).unwrap();
        assert_eq!(rows(&older, "SELECT name FROM decls"), vec!["from code v1"]);
        assert_eq!(rows(&older, "SELECT name FROM decls_fts WHERE decls_fts MATCH 'pedido'"), vec!["buscar pedido"]);
        assert_eq!(rows(&older, "SELECT commit_message FROM history"), vec!["from git"]);
        drop(older);

        // O programa mais novo acha o bloco dele como deixou.
        let newer = MapDb::open(&map_in(dir.path()), dir.path(), &[ROUTES_NEWER]).unwrap();
        assert_eq!(rows(&newer, "SELECT route FROM routes"), vec!["/orders"]);
        assert_eq!(newer.version("routes").unwrap(), Some(1));
    }

    #[test]
    fn two_writes_at_the_same_time_both_finish() {
        let dir = tempdir().unwrap();
        drop(MapDb::open(&map_in(dir.path()), dir.path(), &[NOTES_V1]).unwrap());
        let root = Arc::new(dir.path().to_path_buf());
        let start = Arc::new(Barrier::new(2));
        let writers: Vec<_> = ["a.rs", "b.rs"]
            .into_iter()
            .map(|path| {
                let (root, start) = (Arc::clone(&root), Arc::clone(&start));
                std::thread::spawn(move || {
                    let mut db = MapDb::open(&map_in(&root), &root, &[NOTES_V1]).unwrap();
                    start.wait();
                    db.write(|tx| {
                        tx.execute("INSERT INTO notes VALUES (?1, 'first')", params![path])?;
                        // Segura a trava para a outra gravação chegar no meio.
                        std::thread::sleep(Duration::from_millis(200));
                        tx.execute("INSERT INTO notes VALUES (?1, 'second')", params![path])?;
                        Ok(())
                    })
                })
            })
            .collect();
        for writer in writers {
            writer.join().unwrap().unwrap();
        }
        let db = MapDb::open(&map_in(dir.path()), dir.path(), &[]).unwrap();
        assert_eq!(
            rows(&db, "SELECT path || '|' || note FROM notes ORDER BY path, rowid"),
            vec!["a.rs|first", "a.rs|second", "b.rs|first", "b.rs|second"]
        );
    }

    /// O modo de diário que a conexão `conn` vê no arquivo do mapa.
    fn journal_of(conn: &Connection) -> String {
        conn.pragma_query_value(None, "journal_mode", |row| row.get(0)).unwrap()
    }

    #[test]
    fn a_map_left_in_the_rollback_journal_passes_to_wal_when_it_is_opened() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        let other = Connection::open(map_in(dir.path())).unwrap();
        let mode: String = other.pragma_update_and_check(None, "journal_mode", "DELETE", |row| row.get(0)).unwrap();
        assert_eq!(mode, "delete");
        drop(other);

        let mut db = MapDb::open(&map_in(dir.path()), dir.path(), &[DECLS_V1, NOTES_V1]).unwrap();
        put(&mut db, "INSERT INTO notes VALUES ('a.rs', 'reads the git log');");
        assert_eq!(journal_of(db.conn()), "wal");
        assert_eq!(journal_of(&Connection::open(map_in(dir.path())).unwrap()), "wal", "the mode stays in the file");
    }

    #[test]
    fn no_file_is_left_beside_the_map_once_the_last_connection_closes() {
        let dir = tempdir().unwrap();
        let mut db = MapDb::open(&map_in(dir.path()), dir.path(), &[DECLS_V1, NOTES_V1]).unwrap();
        put(&mut db, "INSERT INTO notes VALUES ('a.rs', 'reads the git log');");
        let beside = || -> Vec<String> {
            let mut files: Vec<String> = std::fs::read_dir(dir.path().join(".claude"))
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            files.sort();
            files
        };
        let reader = MapDb::open(&map_in(dir.path()), dir.path(), &[]).unwrap();
        drop(db);
        assert!(beside().len() > 1, "while a connection is open the log of the writes is beside the map: {:?}", beside());
        drop(reader);
        assert_eq!(beside(), vec![MAP_FILE_NAME], "the last one to close joins the log to the map and deletes it");
    }
}
