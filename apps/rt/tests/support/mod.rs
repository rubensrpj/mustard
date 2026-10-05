//! O que os testes que rodam o binário de verdade dividem entre si.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

/// Os projetos de teste desta linha de execução e a pasta das cópias de cada
/// um ([`mustard_core::io::wave_prompt::copies_dir`]). Quando a linha termina
/// — o teste passou ou falhou —, cada pasta sai do disco e, com o repositório
/// ainda no lugar, o git dele esquece as cópias que sumiram; o repositório
/// que já saiu com a pasta temporária levou o registro junto.
struct TestCopies(Vec<(PathBuf, PathBuf)>);

impl Drop for TestCopies {
    fn drop(&mut self) {
        for (root, copies) in &self.0 {
            let _ = std::fs::remove_dir_all(copies);
            if root.join(".git").exists() {
                let _ = std::process::Command::new("git").args(["worktree", "prune"]).current_dir(root).output();
            }
        }
    }
}

thread_local! {
    static TEST_COPIES: RefCell<TestCopies> = const { RefCell::new(TestCopies(Vec::new())) };
}

/// As cópias que o binário cria para o projeto `root` saem no fim do teste,
/// também quando ele falha, e o git do projeto deixa de listá-las. Elas moram
/// fora da pasta temporária do teste — na pasta que `MUSTARD_COPIES_DIR`
/// indica, a mesma para o teste e para o binário que ele roda —, e a pasta
/// temporária, quando sai, não as leva. Chame da linha de execução do próprio
/// teste.
pub fn copies_leave_with_the_test(root: &Path) {
    let copies = mustard_core::io::wave_prompt::copies_dir(root);
    TEST_COPIES.with(|made| made.borrow_mut().0.push((root.to_path_buf(), copies)));
}
