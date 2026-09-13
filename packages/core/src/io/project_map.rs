//! A leitura do mapa do projeto (`.claude/grain.model.json`), gravado pelo
//! scan. As perguntas moram em `domain::project_map`.

use std::path::{Path, PathBuf};

use crate::domain::project_map::{MapRefusal, ProjectMap};

/// Onde o scan grava o mapa, dentro da raiz do projeto.
#[must_use]
pub fn model_path(root: &Path) -> PathBuf {
    root.join(".claude").join("grain.model.json")
}

/// O mapa do projeto em `root`. Sem o arquivo, [`MapRefusal::MapMissing`];
/// com um arquivo que não se entende, [`MapRefusal::MapUnreadable`].
pub fn read(root: &Path) -> Result<ProjectMap, MapRefusal> {
    let path = model_path(root);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(MapRefusal::MapMissing),
        Err(e) => return Err(MapRefusal::MapUnreadable { detail: e.to_string() }),
    };
    serde_json::from_str(&text).map_err(|e| MapRefusal::MapUnreadable { detail: e.to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn a_missing_map_and_a_broken_map_are_told_apart() {
        let dir = tempdir().unwrap();
        assert_eq!(read(dir.path()).unwrap_err(), MapRefusal::MapMissing);
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        std::fs::write(model_path(dir.path()), "{not json").unwrap();
        assert_eq!(read(dir.path()).unwrap_err().reason(), "map-unreadable");
        std::fs::write(model_path(dir.path()), r#"{"modules":[{"path":"a.rs","deps":["b.rs"]}],"other":1}"#).unwrap();
        let map = read(dir.path()).unwrap();
        assert_eq!(map.modules[0].deps, vec!["b.rs".to_string()]);
    }
}
