//! O que a pasta de trabalho tem a mais que o último commit: se está suja e um
//! resumo curto do que a suja.
//!
//! "Suja" é qualquer arquivo rastreado diferente do commit ou qualquer arquivo
//! novo que o git não ignora. O resumo é o começo do SHA-256 de `git diff HEAD`
//! seguido do caminho e do conteúdo de cada arquivo novo, em ordem de caminho:
//! o mesmo código dá o mesmo resumo, e dois códigos sujos de jeitos diferentes
//! dão resumos diferentes. Pasta limpa tem resumo vazio.
//!
//! O arquivo só usa a `std` e o `sha256` ao lado dele, de propósito: o
//! `build.rs` do `mustard-rt` e o do `mustard` o incluem por caminho, para o
//! carimbo de versão e a prova de cada medida virem da mesma conta. Por isso
//! nada aqui usa o resto do núcleo, nem recurso mais novo que a edição 2015.
//! O que roda o git é de quem chama ([`Git`]).

use super::sha256::Sha256;

/// Quantos caracteres do SHA-256 o resumo guarda.
const DIGEST_LEN: usize = 12;

/// O estado da pasta de trabalho em relação ao último commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeState {
    /// Há arquivo rastreado mudado ou arquivo novo não ignorado.
    pub dirty: bool,
    /// O resumo do que muda; vazio quando a pasta está limpa.
    pub diff: String,
}

/// Roda `git <args>` na pasta de trabalho de quem pergunta e devolve o que ele
/// escreveu, sem os espaços das pontas; `None` quando o git não roda ou sai
/// com erro, e `Some("")` quando sai bem sem escrever nada.
pub type Git<'a> = &'a dyn Fn(&[&str]) -> Option<String>;

/// O estado da pasta de trabalho, lido pelo `git` que `git` roda. `None`
/// quando o git não roda, a pasta não está num repositório ou o repositório
/// não tem commit: quem pergunta não afirma nada sobre a pasta.
///
/// Quem roda o git é de quem chama: o código do Mustard o roda pela porta
/// única (`platform::git`), e o script de build, que não enxerga o núcleo, o
/// roda ele mesmo.
#[must_use]
pub fn tree_state(git: Git<'_>) -> Option<TreeState> {
    let top = std::path::PathBuf::from(git(&["rev-parse", "--show-toplevel"])?.trim());
    let tracked = git(&["-c", "color.ui=never", "diff", "--binary", "--no-ext-diff", "--no-textconv", "HEAD"])?;
    let tracked = tracked.trim();
    let listed = git(&["ls-files", "--others", "--exclude-standard", "--full-name", "-z", "--", ":/"])?;
    let mut fresh: Vec<&str> = listed.split('\0').filter(|name| !name.is_empty()).collect();
    fresh.sort_unstable();
    if tracked.is_empty() && fresh.is_empty() {
        return Some(TreeState { dirty: false, diff: String::new() });
    }
    let mut hasher = Sha256::new();
    hasher.update(b"tracked\0");
    hasher.update(tracked.as_bytes());
    for name in fresh {
        hasher.update(b"\0untracked\0");
        hasher.update(name.as_bytes());
        hasher.update(b"\0");
        if let Ok(body) = std::fs::read(top.join(name)) {
            hasher.update(&body);
        }
    }
    let digest = hasher.hex_digest();
    Some(TreeState { dirty: true, diff: digest[..DIGEST_LEN].to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::{TempDir, tempdir};

    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// O estado de `dir`, com o git rodado pela porta única do núcleo.
    fn state_of(dir: &Path) -> Option<TreeState> {
        tree_state(&|args| crate::platform::git::run(dir, args).out())
    }

    /// Um repositório com um commit e o arquivo `a.txt`.
    fn committed() -> TempDir {
        let dir = tempdir().unwrap();
        git(dir.path(), &["init", "-q"]);
        std::fs::write(dir.path().join("a.txt"), "um\n").unwrap();
        git(dir.path(), &["add", "-A"]);
        git(dir.path(), &["commit", "-q", "-m", "primeiro"]);
        dir
    }

    #[test]
    fn a_clean_tree_is_not_dirty_and_has_no_digest() {
        let dir = committed();
        assert_eq!(state_of(dir.path()), Some(TreeState { dirty: false, diff: String::new() }));
    }

    #[test]
    fn a_changed_tracked_file_and_a_new_file_are_dirty_with_different_digests() {
        let changed = committed();
        std::fs::write(changed.path().join("a.txt"), "dois\n").unwrap();
        let changed = state_of(changed.path()).unwrap();

        let added = committed();
        std::fs::write(added.path().join("novo.txt"), "dois\n").unwrap();
        let added = state_of(added.path()).unwrap();

        assert!(changed.dirty && added.dirty);
        assert_eq!(changed.diff.len(), DIGEST_LEN);
        assert_eq!(added.diff.len(), DIGEST_LEN);
        assert_ne!(changed.diff, added.diff);
    }

    #[test]
    fn the_same_change_gives_the_same_digest_and_another_change_another() {
        let dir = committed();
        std::fs::write(dir.path().join("novo.txt"), "x\n").unwrap();
        let first = state_of(dir.path()).unwrap();
        assert_eq!(state_of(dir.path()).unwrap(), first);
        std::fs::write(dir.path().join("novo.txt"), "y\n").unwrap();
        assert_ne!(state_of(dir.path()).unwrap().diff, first.diff);
    }

    /// Visto de uma pasta de dentro do projeto, o arquivo novo de outra pasta
    /// também suja: o resumo é do repositório, não da pasta onde se pergunta.
    #[test]
    fn a_new_file_outside_the_folder_asked_from_still_counts() {
        let dir = committed();
        std::fs::create_dir_all(dir.path().join("fundo")).unwrap();
        std::fs::write(dir.path().join("fundo").join("b.txt"), "um\n").unwrap();
        git(dir.path(), &["add", "-A"]);
        git(dir.path(), &["commit", "-q", "-m", "segundo"]);
        std::fs::write(dir.path().join("novo.txt"), "x\n").unwrap();
        let from_inside = state_of(&dir.path().join("fundo")).unwrap();
        assert_eq!(from_inside, state_of(dir.path()).unwrap());
        assert!(from_inside.dirty);
    }

    #[test]
    fn a_folder_outside_a_repository_has_no_state() {
        let dir = tempdir().unwrap();
        assert_eq!(state_of(dir.path()), None);
    }
}
