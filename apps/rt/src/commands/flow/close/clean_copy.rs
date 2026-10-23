//! A cópia limpa em que a máquina do fechamento roda: a compilação, o lint, a
//! suíte e a prova de cada critério saem de um checkout do commit da obra,
//! sem o que o git ignora e a esteira do servidor não tem. O comando que lê
//! um arquivo local (`.env`) cai aqui como cairia lá, e não só depois do PR.
//!
//! A pasta é uma vaga da obra, com nome que nenhuma onda usa, e como as das
//! ondas ela guarda a compilação e as dependências entre um fechamento e o
//! seguinte: o git não as apaga, e a compilação refaz só o que mudou. Os
//! arquivos locais do projeto (`localFiles`) nunca são levados a ela.

use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::io::wave_prompt::{shown, spec_copies_dir};
use mustard_core::platform::git;

use super::CloseRefusal;
use crate::commands::git_settle::CopiesLock;

/// O nome da pasta da cópia limpa dentro da pasta das cópias da obra: as
/// vagas das ondas são `a` a `z` e, depois, números.
const FOLDER: &str = "check";

/// Prepara a cópia limpa da obra `spec` no commit atual de `root`, com a
/// trava das cópias presa por quem chama (`_copies`), e devolve a pasta dela:
/// a vaga zerada no commit, sem arquivo local, e as dependências declaradas
/// instaladas como na cópia de onda.
pub(super) fn prepare(root: &Path, spec: &str, log: &SpecLog, _copies: &CopiesLock) -> Result<PathBuf, CloseRefusal> {
    let path = spec_copies_dir(root, spec).join(FOLDER);
    let failed = |detail: String| CloseRefusal::CleanCopyFailed { copy: shown(&path), detail };
    let head = git::run(root, &["rev-parse", "HEAD"]).result().map_err(&failed)?;
    let owner = crate::commands::flow::round::slot_owner(spec, log, &path);
    crate::commands::flow::round::clean_copy(root, &path, &head, &owner).map_err(&failed)?;
    crate::commands::flow::round::prepare_copy_dependencies(root, &path).map_err(&failed)?;
    Ok(path)
}
