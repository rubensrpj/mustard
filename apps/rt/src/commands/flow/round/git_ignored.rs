//! O arquivo que o `.gitignore` do projeto ignora é mudança local: nenhuma onda
//! o entrega, porque o git recusa o commit que o leva
//! ([`super::commit::ignored_by_git`]). A sobra e a tarefa que só mexem em
//! arquivos assim não podem virar onda; a conferência é a do próprio git, a
//! mesma da volta, e vale antes de a tarefa existir.

use std::collections::BTreeSet;
use std::path::Path;

use mustard_core::io::wave_prompt::local_file_inside;
use mustard_core::platform::git;

use super::commit::repo_of;
use crate::commands::git_settle::submodules_of;

/// `true` quando `files` tem algum arquivo e o git do projeto `root` ignora
/// todos eles, cada um no repositório que o guarda: o principal ou um
/// submódulo. O arquivo que o git já rastreia não é ignorado, mesmo com uma
/// regra que o casa. Na dúvida — o caminho absoluto ou que sai do projeto, o
/// git que não responde — o arquivo vale como não ignorado: segue o caminho de
/// sempre, e a volta da onda recusa o que o git recusar.
pub(crate) fn all_ignored(root: &Path, files: &BTreeSet<String>) -> bool {
    if files.is_empty() {
        return false;
    }
    let subs = submodules_of(root);
    files.iter().all(|file| {
        let (repo, inner) = repo_of(root, &subs, file);
        local_file_inside(file) && git::run(&repo, &["check-ignore", "-q", "--", &inner]).ok
    })
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    fn git_in(root: &Path, args: &[&str]) {
        let out = std::process::Command::new("git").args(args).current_dir(root).output().expect("git runs");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    fn files(listed: &[&str]) -> BTreeSet<String> {
        listed.iter().map(|file| (*file).to_string()).collect()
    }

    /// Só o conjunto cujos arquivos o git ignora todos conta: um arquivo
    /// versionado no meio, a lista vazia e o caminho que sai do projeto
    /// deixam o conjunto no caminho de sempre, e o arquivo que o git rastreia
    /// não é ignorado, mesmo casando com uma regra.
    #[test]
    fn only_a_set_that_git_ignores_entirely_counts() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        git_in(root, &["init", "-q"]);
        git_in(root, &["config", "user.email", "t@example.com"]);
        git_in(root, &["config", "user.name", "t"]);
        std::fs::write(root.join(".gitignore"), ".env\n*.log\n").unwrap();
        std::fs::write(root.join("kept.log"), "versionado\n").unwrap();
        git_in(root, &["add", "-A", "-f"]);
        git_in(root, &["commit", "-q", "-m", "init"]);

        assert!(all_ignored(root, &files(&[".env"])), "um arquivo ignorado");
        assert!(all_ignored(root, &files(&[".env", "logs/app.log"])), "todos ignorados, mesmo o que ainda não existe");
        assert!(!all_ignored(root, &files(&[".env", "src/main.rs"])), "um arquivo versionado no meio");
        assert!(!all_ignored(root, &files(&["kept.log"])), "o rastreado não é ignorado, ainda que a regra o case");
        assert!(!all_ignored(root, &files(&[])), "sem arquivo nenhum");
        assert!(!all_ignored(root, &files(&["../fora/.env"])), "o caminho que sai do projeto");
    }
}
