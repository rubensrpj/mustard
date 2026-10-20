//! As specs de antes desta versão do Mustard no projeto.
//!
//! Uma spec é antiga quando já existia no dia em que esta versão do Mustard
//! foi montada no projeto pela primeira vez. A marca é uma lista só, gravada
//! uma vez, pela montagem (`mustard init` e `run upsert`), num arquivo da
//! pasta do Mustard no projeto ([`LEGACY_SPECS_FILE`]): o nome de cada pasta
//! de `.claude/spec/` naquela hora, e a versão que a gravou. Com o arquivo já
//! gravado, a montagem seguinte não mexe nele. A spec fora da lista é atual,
//! e o projeto sem o arquivo não tem spec antiga.
//!
//! A spec antiga só se lê: some do painel, da publicação, do levantamento e
//! das conferências do `doctor`, e toda obra nela é recusada
//! ([`refuse_legacy`]).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::domain::spec_events::Refusal;
use crate::io::claude_paths::ClaudePaths;
use crate::io::spec_index::DISCARDED_DIR;

/// O arquivo da lista, relativo à raiz do projeto.
pub const LEGACY_SPECS_FILE: &str = ".claude/mustard/legacy-specs.json";

/// O arquivo da lista no projeto `root`.
#[must_use]
pub fn path(root: &Path) -> PathBuf {
    root.join(LEGACY_SPECS_FILE)
}

/// A lista das specs antigas do projeto, lida uma vez.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LegacySpecs {
    names: BTreeSet<String>,
}

impl LegacySpecs {
    /// A lista do projeto `root`. Sem o arquivo, ou com ele ilegível, a lista
    /// é vazia: nenhuma spec é antiga.
    #[must_use]
    pub fn read(root: &Path) -> Self {
        let Ok(text) = crate::io::fs::read_to_string(path(root)) else {
            return Self::default();
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            return Self::default();
        };
        let names = value["specs"].as_array().into_iter().flatten().filter_map(serde_json::Value::as_str).map(str::to_string).collect();
        Self { names }
    }

    /// A spec `name` é antiga.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.names.contains(name.trim())
    }

    /// Nenhuma spec é antiga.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// O nome de cada spec antiga, em ordem.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(String::as_str)
    }
}

/// A spec `name` do projeto `root` é antiga. Lê a lista a cada chamada; quem
/// pergunta por muitas specs lê a lista uma vez com [`LegacySpecs::read`].
#[must_use]
pub fn is_legacy(root: &Path, name: &str) -> bool {
    LegacySpecs::read(root).contains(name)
}

/// A recusa da obra na spec antiga `name` do projeto `root`.
///
/// # Errors
///
/// [`Refusal::LegacySpec`] quando a spec é antiga.
pub fn refuse_legacy(root: &Path, name: &str) -> Result<(), Refusal> {
    if is_legacy(root, name) {
        return Err(Refusal::LegacySpec { spec: name.trim().to_string() });
    }
    Ok(())
}

/// Grava a lista do projeto `root` quando ela ainda não existe: o nome de
/// cada pasta de spec que há agora em `.claude/spec/`, as descartadas
/// também, e a versão `version`. O projeto sem spec ganha a lista vazia.
/// `Ok(true)` quando gravou; `Ok(false)` quando a lista já existia.
///
/// # Errors
///
/// A falha de leitura da pasta das specs ou de gravação do arquivo.
pub fn mark_once(root: &Path, version: &str) -> crate::platform::error::Result<bool> {
    let file = path(root);
    if crate::io::fs::exists(&file) {
        return Ok(false);
    }
    let spec_dir = ClaudePaths::compose_unchecked(root).spec_dir();
    let mut specs = folders(&spec_dir)?;
    specs.extend(folders(&spec_dir.join(DISCARDED_DIR))?);
    let body = json!({ "since": version, "specs": specs.into_iter().collect::<BTreeSet<_>>() });
    crate::io::fs::write_atomic(&file, format!("{body}\n").as_bytes())?;
    Ok(true)
}

/// O nome de cada pasta de spec em `dir`: a pasta das descartadas e a que
/// começa com ponto não são specs. A pasta que falta não tem nenhuma.
fn folders(dir: &Path) -> crate::platform::error::Result<Vec<String>> {
    let entries = match crate::io::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(crate::platform::error::Error::NotFound(_)) => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    Ok(entries.into_iter().filter(|entry| entry.is_dir && !entry.file_name.starts_with('.')).map(|entry| entry.file_name).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec_folder(root: &Path, name: &str) {
        std::fs::create_dir_all(root.join(".claude/spec").join(name)).unwrap();
        std::fs::write(root.join(".claude/spec").join(name).join("spec.ndjson"), "").unwrap();
    }

    /// A montagem grava a lista com as specs que existem, as descartadas
    /// também, e a versão; a montagem seguinte não regrava, mesmo com spec
    /// nova no projeto, que fica atual.
    #[test]
    fn the_list_is_written_once_with_the_specs_that_exist() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        spec_folder(root, "antiga");
        std::fs::create_dir_all(root.join(".claude/spec").join(DISCARDED_DIR).join("jogada")).unwrap();

        assert!(mark_once(root, "0.2.25").unwrap(), "the first mount writes the list");
        let written: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path(root)).unwrap()).unwrap();
        assert_eq!(written, json!({"since": "0.2.25", "specs": ["antiga", "jogada"]}));

        spec_folder(root, "nova");
        let before = std::fs::read_to_string(path(root)).unwrap();
        assert!(!mark_once(root, "0.2.26").unwrap(), "the list already exists");
        assert_eq!(std::fs::read_to_string(path(root)).unwrap(), before, "the list is never rewritten");

        let legacy = LegacySpecs::read(root);
        assert!(legacy.contains("antiga") && legacy.contains("jogada"));
        assert!(!legacy.contains("nova"), "a spec born after the mount is current");
        assert!(is_legacy(root, " antiga "));
    }

    /// O projeto novo, sem spec, ganha a lista vazia: nada é antigo, e a spec
    /// que nascer depois é atual.
    #[test]
    fn a_project_without_specs_gets_an_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert!(mark_once(root, "0.2.25").unwrap());
        let written: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path(root)).unwrap()).unwrap();
        assert_eq!(written, json!({"since": "0.2.25", "specs": []}));
        spec_folder(root, "nova");
        assert!(!is_legacy(root, "nova"));
    }

    /// Sem a lista, nenhuma spec é antiga; a spec da lista recusa a obra com
    /// o código próprio, e a atual passa.
    #[test]
    fn only_a_listed_spec_refuses_the_work() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        spec_folder(root, "antiga");
        assert!(LegacySpecs::read(root).is_empty(), "without the list nothing is old");
        assert!(refuse_legacy(root, "antiga").is_ok());

        mark_once(root, "0.2.25").unwrap();
        spec_folder(root, "nova");
        let refused = refuse_legacy(root, "antiga").unwrap_err();
        assert_eq!(refused, Refusal::LegacySpec { spec: "antiga".into() });
        assert_eq!(refused.reason(), "legacy-spec");
        assert!(refuse_legacy(root, "nova").is_ok());
    }
}
