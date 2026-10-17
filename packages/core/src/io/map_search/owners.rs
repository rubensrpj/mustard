//! As declarações do mapa donas das linhas que uma busca de texto achou: a
//! resposta da busca pelo mapa aponta o arquivo e a função, e deixa de fora o
//! arquivo que o mapa não tem, como o de documentação e o de dados.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rusqlite::{Connection, OptionalExtension};

use crate::domain::project_map::MapRefusal;
use crate::io::map_fill;
use crate::io::project_map::{model_path, open_reading, unreadable, DECLS, FILES};
use crate::platform::error::Result;

/// Uma declaração do mapa e as linhas achadas dentro dela, em ordem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineOwner {
    pub kind: String,
    pub name: String,
    pub line: u64,
    pub end_line: u64,
    pub lines: Vec<u64>,
}

/// Um arquivo do mapa com linhas achadas: as declarações donas delas, pela
/// linha de começo, e as linhas que caem fora de toda declaração.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapFile {
    pub path: String,
    pub owners: Vec<LineOwner>,
    pub loose: Vec<u64>,
    /// Quantas linhas achadas o arquivo tem, sem repetir.
    pub found: usize,
}

/// As linhas achadas, separadas pelo mapa: os arquivos dele, do que tem mais
/// linhas achadas ao que tem menos, e, pelo caminho, os que ele não tem.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OwnedLines {
    pub files: Vec<MapFile>,
    pub outside: Vec<String>,
}

/// As linhas achadas `hits` (o caminho a partir da raiz do projeto e a
/// linha) separadas pelo mapa do projeto em `root`. Cada linha de um arquivo
/// do mapa vai para a declaração mais justa que a contém — a do método, e não
/// a do tipo dele —; a que nenhuma contém fica solta. As recusas são as de
/// todo leitor do mapa, com a dos blocos dos arquivos e das declarações ainda
/// vazios depois de uma troca de formato.
pub fn line_owners(root: &Path, hits: &[(String, u64)]) -> std::result::Result<OwnedLines, MapRefusal> {
    let blocks = [&FILES, &DECLS];
    let db = open_reading(&model_path(root), &blocks)?;
    map_fill::refuse(&db, &blocks)?;
    owned(db.conn(), hits).map_err(unreadable)
}

fn owned(conn: &Connection, hits: &[(String, u64)]) -> Result<OwnedLines> {
    let mut by_file: BTreeMap<&str, BTreeSet<u64>> = BTreeMap::new();
    for (file, line) in hits {
        by_file.entry(file.as_str()).or_default().insert(*line);
    }
    let mut in_map = conn.prepare("SELECT 1 FROM files WHERE path = ?1")?;
    let mut decls = conn.prepare("SELECT kind, name, line, end_line FROM decls WHERE file = ?1")?;
    let mut out = OwnedLines::default();
    for (path, lines) in by_file {
        if in_map.query_row([path], |_| Ok(())).optional()?.is_none() {
            out.outside.push(path.to_string());
            continue;
        }
        let declared: Vec<LineOwner> = decls
            .query_map([path], |row| {
                let at = |index: usize| -> rusqlite::Result<u64> { Ok(u64::try_from(row.get::<_, Option<i64>>(index)?.unwrap_or(0)).unwrap_or(0)) };
                Ok(LineOwner { kind: row.get(0)?, name: row.get(1)?, line: at(2)?, end_line: at(3)?, lines: Vec::new() })
            })?
            .collect::<rusqlite::Result<_>>()?;
        out.files.push(placed(path, &lines, declared));
    }
    out.files.sort_by(|a, b| b.found.cmp(&a.found).then_with(|| a.path.cmp(&b.path)));
    Ok(out)
}

/// As linhas `lines` de `path` em cada declaração `declared` que é a mais
/// justa a contê-las; a declaração sem linha achada fica de fora.
fn placed(path: &str, lines: &BTreeSet<u64>, mut declared: Vec<LineOwner>) -> MapFile {
    let mut loose = Vec::new();
    for &line in lines {
        let tightest = declared
            .iter()
            .enumerate()
            .filter(|(_, decl)| decl.line > 0 && decl.line <= line && line <= decl.end_line)
            .min_by_key(|(_, decl)| (decl.end_line - decl.line, std::cmp::Reverse(decl.line)))
            .map(|(at, _)| at);
        match tightest {
            Some(at) => declared[at].lines.push(line),
            None => loose.push(line),
        }
    }
    declared.retain(|decl| !decl.lines.is_empty());
    declared.sort_by(|a, b| a.line.cmp(&b.line).then_with(|| a.name.cmp(&b.name)));
    MapFile { path: path.to_string(), owners: declared, loose, found: lines.len() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::normalize::Languages;
    use crate::io::project_map::save_at;
    use serde_json::json;

    /// A linha dentro de um método vai para ele, e não para a classe; a de
    /// fora de toda declaração fica solta; o arquivo que o mapa não tem fica
    /// de fora; o arquivo com mais linhas achadas vem primeiro.
    #[test]
    fn each_line_goes_to_the_tightest_declaration_and_files_outside_the_map_are_apart() {
        let dir = tempfile::tempdir().unwrap();
        let map = json!({"modules": [
            {"path": "web/src/clinic.ts", "declarations": [
                {"kind": "class", "name": "Clinic", "line": 3, "end_line": 12},
                {"kind": "method", "name": "visit", "line": 5, "end_line": 8}
            ]},
            {"path": "web/src/order.ts", "declarations": [{"kind": "function", "name": "payOrder", "line": 1, "end_line": 4}]}
        ]});
        save_at(&model_path(dir.path()), &map, "scan 1", &Languages::new([])).unwrap();
        let hit = |file: &str, line: u64| (file.to_string(), line);
        let hits = [hit("web/src/clinic.ts", 6), hit("web/src/clinic.ts", 10), hit("web/src/clinic.ts", 1), hit("docs/clinic.md", 2), hit("web/src/order.ts", 2), hit("web/src/clinic.ts", 6)];

        let owned = line_owners(dir.path(), &hits).unwrap();

        assert_eq!(owned.outside, ["docs/clinic.md"]);
        let paths: Vec<&str> = owned.files.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(paths, ["web/src/clinic.ts", "web/src/order.ts"]);
        let clinic = &owned.files[0];
        let owners: Vec<(&str, Vec<u64>)> = clinic.owners.iter().map(|decl| (decl.name.as_str(), decl.lines.clone())).collect();
        assert_eq!(owners, [("Clinic", vec![10]), ("visit", vec![6])]);
        assert_eq!(clinic.loose, [1]);
        assert_eq!(clinic.found, 3);
    }
}
