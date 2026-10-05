//! As marcas de versão do projeto no disco: uma linha por compilação do
//! Mustard que abriu uma sessão nele, com a linha de versão e o instante.
//!
//! O arquivo mora em `.claude/mustard/` do checkout principal, que o rastro
//! do Mustard já deixa fora do git, e só cresce: a marca nova entra no fim
//! quando a linha de versão difere da última.

use std::path::{Path, PathBuf};

use crate::domain::measure::Mark;
use crate::io::fs::lock::{read_shared, LockedFile};
use crate::io::workspace::linked_worktree_main;
use crate::platform::error::Error;

/// O arquivo das marcas dentro de `.claude/mustard/`.
const MARKS: &str = "version-marks.ndjson";

/// O caminho do arquivo das marcas do projeto de `root`, no checkout
/// principal também quando `root` é a cópia de onda dele.
#[must_use]
pub fn marks_path(root: &Path) -> PathBuf {
    let home = linked_worktree_main(root).unwrap_or_else(|| root.to_path_buf());
    home.join(".claude").join("mustard").join(MARKS)
}

/// As marcas do projeto de `root`, da mais velha à mais nova. A linha que não
/// se lê fica de fora; sem arquivo, nenhuma.
#[must_use]
pub fn marks(root: &Path) -> Vec<Mark> {
    parse(&read_shared(&marks_path(root)).unwrap_or_default())
}

fn parse(text: &str) -> Vec<Mark> {
    text.lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
}

/// Acrescenta a marca da linha de versão `version` no projeto de `root`, com
/// o instante de agora, só quando ela difere da última marca. Com o arquivo
/// preso por outra sessão, que grava a mesma marca agora, nada: quem chama
/// não espera.
///
/// # Errors
///
/// [`Error::Io`] quando a pasta não se cria ou o arquivo não se lê nem grava.
pub fn record(root: &Path, version: &str) -> Result<(), Error> {
    let Some(mut file) = LockedFile::exclusive_if_free(&marks_path(root))? else { return Ok(()) };
    if parse(&file.read_to_string()?).last().is_some_and(|last| last.version == version) {
        return Ok(());
    }
    let mark = Mark { version: version.to_string(), at: crate::io::spec_events::now() };
    file.append_line(&serde_json::to_string(&mark)?)
}
