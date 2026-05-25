//! Persistent `[[wikilink]]` index — the read/write layer for the wave-network
//! spec (`2026-05-20-mustard-wave-network-standard`).
//!
//! The harness extracts Obsidian-style `[[name]]` references from spec markdown
//! and persists them as edges in a small `wikilinks` table that lives next to
//! the event log inside `.claude/.harness/mustard.db`. Downstream consumers
//! (the dashboard "Network" tab, `metrics wave-status`) join on the columns to
//! render the parent/child graph.
//!
//! ## Schema
//!
//! ```sql
//! CREATE TABLE IF NOT EXISTS wikilinks (
//!   from TEXT, to TEXT, file TEXT, line INTEGER,
//!   PRIMARY KEY (from, to, file)
//! );
//! ```
//!
//! `from` is the spec that *contains* the wikilink (typically a directory
//! name); `to` is the wikilink target; `file` is the relative path of the
//! markdown source; `line` is the 1-based line of the match. The primary key
//! is `(from, to, file)` so re-extracting a spec REPLACEs every previous row
//! for that combination idempotently.
//!
//! ## Connection sharing
//!
//! Functions take a `&rusqlite::Connection` instead of opening a fresh handle.
//! The standard call shape — used by `apps/rt/src/run/wikilink.rs` and the
//! Wave-6b memory writers — is:
//!
//! ```ignore
//! let store = SqliteEventStore::for_project(cwd)?;
//! let conn  = rusqlite::Connection::open(store.path())?;
//! wikilinks::ensure_schema(&conn)?;
//! wikilinks::upsert_batch(&conn, &links)?;
//! ```
//!
//! Every operation is fail-open from the caller's view: errors propagate as
//! [`crate::error::Error::Sqlite`] and the harness discards them.

use crate::error::Result;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

/// One row in the `wikilinks` table — a single `[[target]]` occurrence inside
/// a spec markdown file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Wikilink {
    /// Spec that contains the link (typically the parent directory name of
    /// the markdown file, or the file name minus `.md` when the file sits at
    /// the root of a `--spec-dir`).
    pub from: String,
    /// Wikilink target — the text between `[[` and `]]`.
    pub to: String,
    /// Source markdown file, relative to the `--spec-dir` that was scanned.
    pub file: String,
    /// 1-based line number where the match was found.
    pub line: u32,
}

/// Idempotent `CREATE TABLE IF NOT EXISTS` for the `wikilinks` table.
///
/// Safe to call on every open of the store — both `ensure_schema` and the
/// embedded `sqlite_schema.sql` use `IF NOT EXISTS`, so two callers cannot
/// collide.
///
/// # Errors
///
/// Returns [`crate::error::Error::Sqlite`] when the `CREATE TABLE` statement
/// fails (extremely rare — the only realistic cause is a corrupt database).
pub fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS wikilinks (\
           \"from\" TEXT NOT NULL,\
           \"to\"   TEXT NOT NULL,\
           file    TEXT NOT NULL,\
           line    INTEGER NOT NULL,\
           PRIMARY KEY (\"from\", \"to\", file)\
         );\
         CREATE INDEX IF NOT EXISTS idx_wikilinks_from ON wikilinks(\"from\");\
         CREATE INDEX IF NOT EXISTS idx_wikilinks_to   ON wikilinks(\"to\");",
    )?;
    Ok(())
}

/// Upsert every link in `links`. Uses `INSERT OR REPLACE` keyed on the table's
/// primary key `(from, to, file)`, so re-extracting a spec is idempotent.
///
/// Returns the number of rows touched (every input row counts once — REPLACE
/// always touches the row).
///
/// # Errors
///
/// Returns [`crate::error::Error::Sqlite`] on a prepare/execute failure.
pub fn upsert_batch(conn: &Connection, links: &[Wikilink]) -> Result<usize> {
    if links.is_empty() {
        return Ok(0);
    }
    let mut stmt = conn.prepare(
        "INSERT OR REPLACE INTO wikilinks (\"from\", \"to\", file, line) \
         VALUES (?1, ?2, ?3, ?4)",
    )?;
    let mut total = 0usize;
    for link in links {
        stmt.execute(params![link.from, link.to, link.file, link.line])?;
        total += 1;
    }
    Ok(total)
}

/// List every wikilink whose `from` matches `spec_name`. Useful for rendering
/// a single spec's outgoing edges in the dashboard "Network" tab.
///
/// Results are ordered by `(file, line)` so the caller can preserve the
/// authoring order.
///
/// # Errors
///
/// Returns [`crate::error::Error::Sqlite`] on a query failure.
pub fn list_for_spec(conn: &Connection, spec_name: &str) -> Result<Vec<Wikilink>> {
    let mut stmt = conn.prepare(
        "SELECT \"from\", \"to\", file, line FROM wikilinks \
         WHERE \"from\" = ?1 ORDER BY file, line",
    )?;
    let rows = stmt.query_map(params![spec_name], |row| {
        Ok(Wikilink {
            from: row.get(0)?,
            to: row.get(1)?,
            file: row.get(2)?,
            // cast_sign_loss/cast_possible_truncation: line numbers are non-negative and fit in u32.
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            line: row.get::<_, i64>(3)? as u32,
        })
    })?;
    Ok(rows.filter_map(std::result::Result::ok).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn open(path: &std::path::Path) -> Connection {
        Connection::open(path).unwrap()
    }

    #[test]
    fn ensure_schema_is_idempotent() {
        let dir = tempdir().unwrap();
        let conn = open(&dir.path().join("w.db"));
        ensure_schema(&conn).unwrap();
        // Second call must succeed — `IF NOT EXISTS` clauses.
        ensure_schema(&conn).unwrap();
        // Table exists?
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master \
                 WHERE type='table' AND name='wikilinks'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn upsert_batch_replaces_on_conflict() {
        let dir = tempdir().unwrap();
        let conn = open(&dir.path().join("w.db"));
        ensure_schema(&conn).unwrap();
        let link = Wikilink {
            from: "spec-a".into(),
            to: "spec-b".into(),
            file: "spec.md".into(),
            line: 12,
        };
        assert_eq!(upsert_batch(&conn, std::slice::from_ref(&link)).unwrap(), 1);
        // Same primary key, different `line` — must REPLACE, not duplicate.
        let updated = Wikilink { line: 42, ..link };
        assert_eq!(upsert_batch(&conn, &[updated]).unwrap(), 1);
        let rows = list_for_spec(&conn, "spec-a").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].line, 42);
    }

    #[test]
    fn list_for_spec_orders_by_file_then_line() {
        let dir = tempdir().unwrap();
        let conn = open(&dir.path().join("w.db"));
        ensure_schema(&conn).unwrap();
        let links = vec![
            Wikilink { from: "p".into(), to: "z".into(), file: "b.md".into(), line: 5 },
            Wikilink { from: "p".into(), to: "y".into(), file: "a.md".into(), line: 9 },
            Wikilink { from: "p".into(), to: "x".into(), file: "a.md".into(), line: 3 },
        ];
        upsert_batch(&conn, &links).unwrap();
        let rows = list_for_spec(&conn, "p").unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].file, "a.md");
        assert_eq!(rows[0].line, 3);
        assert_eq!(rows[1].file, "a.md");
        assert_eq!(rows[1].line, 9);
        assert_eq!(rows[2].file, "b.md");
    }

    #[test]
    fn list_for_spec_returns_empty_for_unknown() {
        let dir = tempdir().unwrap();
        let conn = open(&dir.path().join("w.db"));
        ensure_schema(&conn).unwrap();
        let rows = list_for_spec(&conn, "ghost").unwrap();
        assert!(rows.is_empty());
    }
}
