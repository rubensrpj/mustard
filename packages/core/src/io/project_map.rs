//! O mapa do projeto, gravado pelo scan: a porta única de todo acesso ao
//! arquivo dele — o nome, o caminho, a pergunta "o mapa existe", a leitura e
//! a gravação. Ninguém mais escreve o nome do arquivo nem o abre direto. As
//! perguntas ao mapa moram em `domain::project_map`.

use std::path::{Path, PathBuf};

use crate::domain::project_map::{MapRefusal, ProjectMap};
use crate::platform::error::Result;

/// A pasta do projeto onde o mapa mora.
const MAP_DIR: &str = ".claude";

/// Onde o scan grava o mapa, a partir da raiz do projeto, com barras normais.
/// É o texto que as recusas, os avisos e as listas do censo citam.
pub const MAP_FILE: &str = ".claude/grain.model.json";

/// O nome do arquivo do mapa, sem a pasta: o que as listas de arquivos de
/// dentro de `.claude/` citam. Sai de [`MAP_FILE`], que é o único lugar do
/// nome.
pub const MAP_FILE_NAME: &str = MAP_FILE.split_at(MAP_DIR.len() + 1).1;

/// Onde o scan grava o mapa, dentro da raiz do projeto.
#[must_use]
pub fn model_path(root: &Path) -> PathBuf {
    root.join(MAP_DIR).join(MAP_FILE_NAME)
}

/// `true` quando há um mapa gravado em `model`: o caminho de [`model_path`],
/// ou o que o scan recebeu para gravar.
#[must_use]
pub fn exists_at(model: &Path) -> bool {
    model.is_file()
}

/// O mapa do projeto em `root`. Sem o arquivo, [`MapRefusal::MapMissing`];
/// com um arquivo que não se entende, [`MapRefusal::MapUnreadable`].
pub fn read(root: &Path) -> std::result::Result<ProjectMap, MapRefusal> {
    read_at(&model_path(root))
}

/// O mapa gravado em `model`, com as mesmas recusas de [`read`]. É a leitura
/// do mapa que o scan acabou de gravar num caminho escolhido por quem o
/// chamou.
pub fn read_at(model: &Path) -> std::result::Result<ProjectMap, MapRefusal> {
    let text = match std::fs::read_to_string(model) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(MapRefusal::MapMissing),
        Err(e) => return Err(MapRefusal::MapUnreadable { detail: e.to_string() }),
    };
    serde_json::from_str(&text).map_err(|e| MapRefusal::MapUnreadable { detail: e.to_string() })
}

/// Grava `map` como o mapa do projeto em `root`, criando a pasta quando ela
/// falta. É como os testes deixam um mapa pronto sem rodar o scan.
///
/// # Errors
///
/// A falha de gravação do disco.
pub fn write(root: &Path, map: &ProjectMap) -> Result<()> {
    write_text(root, &serde_json::to_string(map)?)
}

/// Grava `text` como o mapa do projeto em `root`, do jeito que veio, criando a
/// pasta quando ela falta: é o mapa escrito à mão num teste, com o formato
/// inteiro do scan, ou um que não se entende.
///
/// # Errors
///
/// A falha de gravação do disco.
pub fn write_text(root: &Path, text: &str) -> Result<()> {
    crate::io::fs::write_atomic(model_path(root), text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::project_map::{MapModule, MapState};
    use tempfile::tempdir;

    #[test]
    fn a_missing_map_and_a_broken_map_are_told_apart() {
        let dir = tempdir().unwrap();
        assert!(!exists_at(&model_path(dir.path())), "nothing was written yet");
        assert_eq!(read(dir.path()).unwrap_err(), MapRefusal::MapMissing);
        write_text(dir.path(), "{not json").unwrap();
        assert!(exists_at(&model_path(dir.path())), "a broken map still exists");
        assert_eq!(read(dir.path()).unwrap_err().reason(), "map-unreadable");
        write_text(dir.path(), r#"{"modules":[{"path":"a.rs","deps":["b.rs"]}],"other":1}"#).unwrap();
        let map = read(dir.path()).unwrap();
        assert_eq!(map.modules[0].deps, vec!["b.rs".to_string()]);
    }

    /// Um mapa gravado pela porta volta igual pela leitura dela, e só passa a
    /// existir depois da gravação.
    #[test]
    fn a_written_map_reads_back_the_same() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("projeto");
        assert!(!exists_at(&model_path(&root)));
        let map = ProjectMap {
            modules: vec![MapModule {
                path: "src/a.rs".to_string(),
                deps: vec!["src/b.rs".to_string()],
                ..MapModule::default()
            }],
            state: MapState { head: "abc123".to_string() },
            ..ProjectMap::default()
        };
        write(&root, &map).unwrap();
        assert!(exists_at(&model_path(&root)), "the map exists after it is written");
        let back = read(&root).unwrap();
        assert_eq!(back.modules.len(), 1);
        assert_eq!(back.modules[0].path, "src/a.rs");
        assert_eq!(back.modules[0].deps, vec!["src/b.rs".to_string()]);
        assert_eq!(back.state.head, "abc123");
        assert_eq!(read_at(&model_path(&root)).unwrap().state.head, "abc123");
    }

    /// O nome do arquivo e o caminho saem do mesmo texto.
    #[test]
    fn the_file_name_and_the_path_come_from_one_text() {
        assert_eq!(format!("{MAP_DIR}/{MAP_FILE_NAME}"), MAP_FILE);
        assert!(model_path(Path::new("raiz")).ends_with(MAP_FILE));
    }
}
