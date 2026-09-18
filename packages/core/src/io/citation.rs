//! A conferência das fontes pelo disco: o arquivo citado é procurado nas
//! raízes, em ordem, e o nome citado, no mapa do projeto. A regra mora em
//! `domain::citation`; aqui ficam só as respostas do disco.
//!
//! Num worktree, a citação é procurada a partir de onde o comando roda, e o
//! mapa vem do checkout principal, onde o scan o grava, fora do git.

use std::cell::OnceCell;
use std::path::{Path, PathBuf};

use crate::domain::citation::{self, CitationWorld, Finding};
use crate::domain::project_map::ProjectMap;

/// As raízes em que uma citação é procurada, a partir de onde o comando roda:
/// a própria pasta, cada pasta acima dela e, por último, a raiz das specs.
/// Assim a citação confere de uma subpasta, de um submódulo e de um worktree.
#[must_use]
pub fn citation_roots(start: &Path, spec_root: &Path) -> Vec<PathBuf> {
    let start = std::path::absolute(start).unwrap_or_else(|_| start.to_path_buf());
    let mut roots: Vec<PathBuf> = start.ancestors().map(Path::to_path_buf).collect();
    roots.push(spec_root.to_path_buf());
    roots
}

/// O disco, como a conferência o pergunta: os arquivos nas raízes e os nomes
/// no mapa, que só é lido quando um nome é conferido.
pub struct DiskWorld {
    roots: Vec<PathBuf>,
    map_root: Option<PathBuf>,
    map: OnceCell<Option<ProjectMap>>,
}

impl DiskWorld {
    /// Os arquivos citados são procurados em `roots`, em ordem, e o mapa é o
    /// do projeto em `map_root`. Sem `map_root`, ou com um mapa que falta ou
    /// não se entende, os nomes ficam sem conferência.
    #[must_use]
    pub fn new(roots: Vec<PathBuf>, map_root: Option<&Path>) -> Self {
        Self { roots, map_root: map_root.map(Path::to_path_buf), map: OnceCell::new() }
    }

    fn map(&self) -> Option<&ProjectMap> {
        self.map
            .get_or_init(|| self.map_root.as_deref().and_then(|root| crate::io::project_map::read(root).ok()))
            .as_ref()
    }
}

impl CitationWorld for DiskWorld {
    fn file_lines(&self, path: &str) -> Option<u64> {
        let bytes = self.roots.iter().find_map(|root| crate::io::fs::read(root.join(path)).ok())?;
        Some(count_lines(&bytes))
    }

    fn declared(&self, name: &str) -> Vec<(String, u64)> {
        self.map().map(|map| map.declared(name)).unwrap_or_default()
    }

    fn has_map(&self) -> bool {
        self.map().is_some()
    }
}

/// Confere uma fonte e o texto que ela sustenta pelo disco: o arquivo citado
/// em `roots` (veja [`citation_roots`]) e os nomes no mapa do projeto em
/// `map_root`. É a mesma conferência da gravação de um ponto do levantamento
/// e a que o plano chama para as tarefas.
#[must_use]
pub fn check_at(roots: &[PathBuf], map_root: &Path, source: &str, text: &str) -> Vec<Finding> {
    citation::check(&DiskWorld::new(roots.to_vec(), Some(map_root)), source, text)
}

/// Quantas linhas o arquivo tem: a última conta mesmo sem `\n` no fim.
fn count_lines(bytes: &[u8]) -> u64 {
    if bytes.is_empty() {
        return 0;
    }
    let pieces = bytes.split(|b| *b == b'\n').count() as u64;
    if bytes.ends_with(b"\n") { pieces - 1 } else { pieces }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::project_map::model_path;
    use crate::io::spec_events::spec_root;

    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .output()
            .expect("spawn git");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    #[test]
    fn a_citation_is_found_from_a_linked_worktree_and_the_map_comes_from_the_main_checkout() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main");
        std::fs::create_dir_all(main.join("src")).unwrap();
        std::fs::write(main.join("src/a.rs"), "struct Tipo;\nfn run() {}\n").unwrap();
        git(&main, &["init", "-q"]);
        git(&main, &["add", "src"]);
        git(&main, &["commit", "-q", "-m", "seed"]);
        // O Mustard e o mapa ficam fora do git, então o worktree não tem nenhum dos dois.
        std::fs::write(main.join("mustard.json"), "{}").unwrap();
        std::fs::create_dir_all(main.join(".claude")).unwrap();
        std::fs::write(
            model_path(&main),
            r#"{"modules":[{"path":"src/a.rs","declarations":[
                {"kind":"struct","name":"Tipo","line":1},{"kind":"function","name":"run","line":2}]}]}"#,
        )
        .unwrap();
        let worktree = dir.path().join("wt");
        git(&main, &["worktree", "add", "-q", "-b", "work", &worktree.to_string_lossy()]);
        assert!(!model_path(&worktree).exists());
        // Um arquivo novo, ainda sem commit, que só o worktree tem.
        std::fs::write(worktree.join("src/novo.rs"), "fn novo() {}\n").unwrap();

        let map_root = spec_root(&worktree);
        let roots = citation_roots(&worktree.join("src"), &map_root);
        assert_eq!(check_at(&roots, &map_root, "src/a.rs:2", "o `Tipo` e o `run()`"), Vec::new());
        assert_eq!(check_at(&roots, &map_root, "src/novo.rs:1", "sem nome"), Vec::new());
        assert_eq!(
            check_at(&roots, &map_root, "src/a.rs:2", "o `Outro`"),
            vec![Finding::NameUnknown { name: "Outro".into() }],
            "the names were checked against the main checkout's map"
        );
        assert_eq!(
            check_at(&roots, &map_root, "src/a.rs:3", ""),
            vec![Finding::MissingLine { path: "src/a.rs".into(), line: 3, lines: 2 }]
        );
    }

    #[test]
    fn a_missing_or_broken_map_leaves_the_names_unchecked() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let roots = vec![root.clone()];
        assert_eq!(check_at(&roots, &root, "cargo test → ok", "o `SpecLog`"), vec![Finding::NoMap]);
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(model_path(&root), "{quebrado").unwrap();
        assert_eq!(check_at(&roots, &root, "cargo test → ok", "o `SpecLog`"), vec![Finding::NoMap]);
        let world = DiskWorld::new(roots, None);
        assert!(!world.has_map());
    }
}
