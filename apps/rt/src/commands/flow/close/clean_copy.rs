//! A cópia limpa da obra: a máquina do fechamento roda nela — a compilação,
//! o lint, a suíte e a prova de cada critério saem de um checkout do commit
//! da obra, sem o que o git ignora e a esteira do servidor não tem —, e o
//! revisor final confere a obra nela, no mesmo commit. O comando que lê um
//! arquivo local (`.env`) cai aqui como cairia lá, e não só depois do PR.
//!
//! A pasta ([`check_copy_path`]) fica fora das vagas das ondas e, como elas,
//! guarda a compilação e as dependências entre um fechamento e o seguinte: o
//! git não as apaga, e a compilação refaz só o que mudou. Os arquivos locais
//! do projeto (`localFiles`) nunca são levados a ela.

use std::path::Path;

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::io::wave_prompt::{check_copy_path, shown};
use mustard_core::platform::git;

use super::CloseRefusal;
use crate::commands::flow::round::Kept;
use crate::commands::git_settle::CopiesLock;

/// Prepara a cópia limpa da obra `spec` no commit atual de `root`, com a
/// trava das cópias presa por quem chama (`_copies`): a pasta zerada no
/// commit, sem arquivo local, e as dependências declaradas instaladas como na
/// cópia de onda. Devolve o código que a limpeza guardou antes de zerá-la,
/// como o que um revisor deixou nela.
pub(super) fn prepare(root: &Path, spec: &str, log: &SpecLog, _copies: &CopiesLock) -> Result<Vec<Kept>, CloseRefusal> {
    let path = check_copy_path(root, spec);
    let failed = |detail: String| CloseRefusal::CleanCopyFailed { copy: shown(&path), detail };
    let head = git::run(root, &["rev-parse", "HEAD"]).result().map_err(&failed)?;
    let owner = crate::commands::flow::round::slot_owner(spec, log, &path);
    let kept = crate::commands::flow::round::clean_copy(root, &path, &head, &owner).map_err(&failed)?;
    crate::commands::flow::round::prepare_copy_dependencies(root, &path).map_err(&failed)?;
    Ok(kept)
}
