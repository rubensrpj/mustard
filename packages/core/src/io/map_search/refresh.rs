//! A atualização do índice de busca por arquivo: o que mudou nas linhas de
//! alguns arquivos — a história das declarações deles, por exemplo — refaz só
//! os documentos deles nos dois níveis, e não o índice inteiro.
//!
//! Os documentos saem da mesma leitura da montagem inteira
//! ([`super::documents`]) restrita aos arquivos, com as línguas em que o
//! índice foi feito; entram pela mesma gravação ([`super::insert_docs`]), e os
//! números de cada nível — os documentos e o tamanho médio de cada campo —
//! saem da conta exata das somas, o que dá os mesmos números da montagem
//! inteira. A tabela trigram não muda: o nome da declaração e o arquivo dela
//! ficam como estavam.

use rusqlite::{params_from_iter, Connection, OptionalExtension};

use super::{documents, insert_docs, write_meta, Doc, Level, DECL_LEVEL, FILE_LEVEL, LANGUAGES_KEY};
use crate::domain::normalize::{Languages, Normalizer};
use crate::platform::error::Result;

/// Refaz no índice os documentos dos arquivos `paths` e os das declarações
/// deles, nas línguas em que o índice foi feito, e acerta os números dos dois
/// níveis. Roda na transação de quem grava. O índice esvaziado — sem línguas —
/// fica como está, para a primeira busca o refazer inteiro; o feito em línguas
/// que não se reconstroem dos códigos guardados é esvaziado pela mesma razão.
/// Diz se atualizou.
pub(crate) fn refresh_files(conn: &Connection, paths: &[&str]) -> Result<bool> {
    if paths.is_empty() {
        return Ok(true);
    }
    let stored: Option<String> = conn
        .query_row("SELECT value FROM search_meta WHERE key = ?1", [LANGUAGES_KEY], |row| row.get(0))
        .optional()?;
    let Some(stored) = stored else { return Ok(false) };
    let languages = Languages::new(stored.split(',').filter(|code| !code.is_empty()));
    if languages.codes().join(",") != stored {
        super::forget(conn)?;
        return Ok(false);
    }
    let (files, decls) = documents(conn, &mut Normalizer::new(&languages), Some(paths))?;
    let of_files = ids(conn, "SELECT rowid FROM files WHERE path IN", paths)?;
    swap(conn, &FILE_LEVEL, &of_files, files.iter())?;
    let of_decls = ids(conn, "SELECT rowid FROM decls WHERE file IN", paths)?;
    swap(conn, &DECL_LEVEL, &of_decls, decls.iter().filter(|decl| !decl.unlisted).map(|decl| &decl.doc))?;
    Ok(true)
}

/// Os números das linhas que `select` acha para os caminhos `paths`.
fn ids(conn: &Connection, select: &str, paths: &[&str]) -> Result<Vec<i64>> {
    let slots: Vec<String> = (1..=paths.len()).map(|at| format!("?{at}")).collect();
    let mut stmt = conn.prepare(&format!("{select} ({})", slots.join(", ")))?;
    let rows = stmt.query_map(params_from_iter(paths), |row| row.get::<_, i64>(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Troca no nível os documentos de números `old` pelos de `fresh`: tira os
/// antigos da tabela FTS5 e da dos tamanhos, grava os novos e acerta os
/// números do nível pela diferença das somas.
fn swap<'d>(conn: &Connection, level: &Level, old: &[i64], fresh: impl IntoIterator<Item = &'d Doc>) -> Result<()> {
    let columns: Vec<&str> = level.columns().collect();
    let (mut count, mut total) = stored_meta(conn, level, &columns)?;
    let mut sizes = conn.prepare(&format!("SELECT {} FROM {} WHERE id = ?1", columns.join(", "), level.lengths))?;
    let mut forget_words = conn.prepare(&format!("DELETE FROM {} WHERE rowid = ?1", level.fts))?;
    let mut forget_sizes = conn.prepare(&format!("DELETE FROM {} WHERE id = ?1", level.lengths))?;
    for &id in old {
        let present = sizes
            .query_row([id], |row| (0..columns.len()).map(|at| row.get::<_, Option<i64>>(at)).collect::<rusqlite::Result<Vec<_>>>())
            .optional()?;
        if let Some(present) = present {
            count = count.saturating_sub(1);
            for (sum, size) in total.iter_mut().zip(present) {
                *sum = sum.saturating_sub(u64::try_from(size.unwrap_or(0)).unwrap_or(0));
            }
        }
        forget_words.execute([id])?;
        forget_sizes.execute([id])?;
    }
    let (added, added_total) = insert_docs(conn, level, fresh)?;
    count += added;
    for (sum, size) in total.iter_mut().zip(added_total) {
        *sum += size;
    }
    write_meta(conn, level, count, &total)
}

/// O número de documentos do nível e a soma dos tamanhos de cada campo, como
/// a tabela de números guarda: o número e a média, de que a soma sai exata.
fn stored_meta(conn: &Connection, level: &Level, columns: &[&str]) -> Result<(u64, Vec<u64>)> {
    let read = |key: String| -> Result<f64> {
        let value = conn
            .query_row(&format!("SELECT value FROM {} WHERE key = ?1", level.meta), [key], |row| row.get::<_, f64>(0))
            .optional()?;
        Ok(value.unwrap_or(0.0))
    };
    let count = read(format!("{}.docs", level.fts))?.max(0.0).round() as u64;
    let mut total = Vec::with_capacity(columns.len());
    for name in columns {
        total.push((read(format!("{}.{name}", level.fts))? * count as f64).max(0.0).round() as u64);
    }
    Ok((count, total))
}
