//! Cloning test scenery instead of rebuilding it.
//!
//! ## The cost this exists to remove
//!
//! A git-backed test has to build a world before it can assert anything: a bare
//! origin, a checkout, configs, commits, a remote, worktrees. `git_settle`'s
//! fixture makes EIGHTEEN `git` invocations to do it, and each invocation is a
//! process — 0,037s of pure birth-and-death on Windows even for `git --version`.
//! Measured 2026-08-14: the fixture costs 1,18s of a test's 1,74s, so two thirds
//! of what those 28 tests spend goes into scenery they throw away.
//!
//! The scenery is IDENTICAL every time, so it can be built once per test process
//! and copied. Measured: 0,28s per clone against 1,18s per build — **4,2×**.
//!
//! ## The trap, and why the whole tree is copied
//!
//! Git records the remote URL and each worktree registration as an ABSOLUTE
//! path. Copy only the checkout and the clone still resolves to the TEMPLATE's
//! origin and the TEMPLATE's worktree — so tests running in parallel write over
//! one another. That trades wall clock for intermittent failures, which is a
//! worse defect than the one being fixed.
//!
//! Worse still, `git worktree repair` does not notice: while the template
//! directory exists, nothing looks broken from git's point of view. Observed
//! directly — the first attempt produced clones whose `remote -v` and
//! `worktree list` both pointed back at the template.
//!
//! So [`clone_of`] copies the WHOLE tree, origin included, and the caller rewires
//! the paths its own layout knows about. Three git calls (one `remote set-url`,
//! one `worktree repair` per worktree) against eighteen — and
//! `a_cloned_fixture_is_independent_of_its_template` asserts the rewiring held.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};

/// Copy a directory tree recursively, including empty directories.
///
/// `std::fs` has no such call, and a fixture template is small (tens of files),
/// so the naive walk is the right shape. Panics on IO error: this runs only
/// under `#[cfg(test)]`, where a failed copy must fail the test loudly rather
/// than degrade into a half-built repository that fails somewhere confusing.
/// A SYMLINK is copied as its target's CONTENT, not re-linked, and a symlink
/// whose target is gone is skipped rather than fatal. That asymmetry is not
/// cosmetic: `entry.file_type()` does NOT follow links (it is `lstat`) while
/// `fs::copy` DOES, so a dangling link reads as a plain file and then fails to
/// copy. It cost a red CI on macOS and Linux — `copy template file: NotFound` —
/// while Windows, which has no such links in a git tree, stayed green. Found by
/// the three-OS matrix, 2026-08-14.
pub(crate) fn copy_dir_all(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("create clone dir");
    for entry in std::fs::read_dir(src).expect("read template dir") {
        let entry = entry.expect("template entry");
        let target = dst.join(entry.file_name());
        let from = entry.path();
        if entry.file_type().expect("entry type").is_dir() {
            copy_dir_all(&from, &target);
        } else if from.exists() {
            // `exists()` FOLLOWS the link, so this is exactly the question
            // `fs::copy` is about to ask. Naming the path in the message keeps
            // a future failure diagnosable instead of anonymous.
            std::fs::copy(&from, &target)
                .unwrap_or_else(|e| panic!("copy template file {}: {e}", from.display()));
        }
    }
}

/// A fresh temporary directory holding a copy of `template`.
///
/// The caller is responsible for rewiring whatever absolute paths its layout
/// records — see the module doc: git stores the remote URL and each worktree
/// registration absolutely, and a clone that skips the rewiring silently shares
/// the template's state.
pub(crate) fn clone_of(template: &Path) -> tempfile::TempDir {
    let dest = tempfile::tempdir().expect("clone tempdir");
    copy_dir_all(template, dest.path());
    dest
}

/// O que o repositório semente guarda no `.git/config`: o fim de linha fixo
/// (no Windows o git converteria os arquivos ao criar uma cópia de trabalho) e
/// a identidade de quem comita.
const SEED_CONFIG: &str = "[core]\n\tautocrlf = false\n\teol = lf\n[user]\n\temail = t@t\n\tname = t\n[commit]\n\tgpgsign = false\n";

/// A manutenção automática desligada: o `git commit` abriria mais um processo
/// (`git maintenance run --auto`) a cada commit de uma pasta que o teste joga
/// fora logo depois.
const NO_AUTO_MAINTENANCE: &str = "[gc]\n\tauto = 0\n[maintenance]\n\tauto = false\n";

/// Acrescenta `text` ao `.git/config` do repositório em `repo`.
fn append_to_config(repo: &Path, text: &str) {
    let mut config = std::fs::OpenOptions::new().append(true).open(repo.join(".git").join("config")).expect("open config");
    std::io::Write::write_all(&mut config, text.as_bytes()).expect("write config");
}

/// Desliga a manutenção automática do repositório em `repo`, se ele é um
/// repositório comum (com `.git/config`).
pub(crate) fn quiet_maintenance(repo: &Path) {
    if repo.join(".git").join("config").is_file() {
        append_to_config(repo, NO_AUTO_MAINTENANCE);
    }
}

/// Roda o `git` em `dir` e falha o teste se o git recusar.
fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git").args(args).current_dir(dir).output().expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// Escreve `files` (caminho e texto) em `root`, inicia o repositório com o
/// [`SEED_CONFIG`] e comita tudo como `semente`.
fn build_in_place(root: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let at = root.join(path);
        std::fs::create_dir_all(at.parent().expect("a file inside the repository")).expect("create parent");
        std::fs::write(&at, text).expect("write seed file");
    }
    git(root, &["init", "-q"]);
    append_to_config(root, SEED_CONFIG);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "semente"]);
}

/// Um repositório git em `root`, com `files` (caminho e texto) comitados como
/// `semente`: o que um teste que precisa de um projeto com histórico monta com
/// `init`, configuração, `add` e `commit`, um processo do git para cada.
///
/// O repositório nasce de uma cópia (ver [`repo_from_template`]); o conjunto de
/// arquivos é a chave do modelo.
pub(crate) fn seeded_repo(root: &Path, files: &[(&str, &str)]) {
    let key: BTreeMap<&str, &str> = files.iter().copied().collect();
    repo_from_template(root, &format!("seeded:{key:?}"), |dir| {
        let files: Vec<(&str, &str)> = key.iter().map(|(path, text)| (*path, *text)).collect();
        build_in_place(dir, &files);
    });
}

/// Monta em `root` o repositório que `build` descreve, sem abrir o git quando
/// já se montou um igual no processo do teste.
///
/// A primeira chamada de cada `key`, no processo, roda `build` numa pasta
/// modelo; as seguintes copiam a pasta. Os repositórios têm o mesmo commit, e a
/// cópia só serve a um `build` sem caminho absoluto gravado no `.git` (nada de
/// remoto nem de `worktree`): quem precisa disso reescreve o caminho depois,
/// como `clone_of` pede. Numa `root` que já tem algo (outro repositório,
/// arquivos escritos antes), `build` roda ali mesmo, como o teste faria à mão.
pub(crate) fn repo_from_template(root: &Path, key: &str, build: impl FnOnce(&Path)) {
    static TEMPLATES: OnceLock<Mutex<HashMap<String, Arc<OnceLock<tempfile::TempDir>>>>> = OnceLock::new();

    if std::fs::read_dir(root).is_ok_and(|mut entries| entries.next().is_some()) {
        build(root);
        return quiet_maintenance(root);
    }
    let cell = TEMPLATES.get_or_init(Mutex::default).lock().expect("templates lock").entry(key.to_string()).or_default().clone();
    let template = cell.get_or_init(|| {
        let dir = tempfile::tempdir().expect("template tempdir");
        build(dir.path());
        quiet_maintenance(dir.path());
        dir
    });
    copy_dir_all(template.path(), root);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A saída do `git` em `dir`, sem as quebras de linha das pontas.
    fn git_text(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git").args(args).current_dir(dir).output().expect("git");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// O repositório nasce com os arquivos pedidos, tudo num commit só, e sem
    /// nada por comitar.
    #[test]
    fn a_seeded_repo_holds_its_files_in_one_clean_seed_commit() {
        let dir = tempfile::tempdir().unwrap();
        seeded_repo(dir.path(), &[("mustard.json", "{}"), ("src/a.rs", "fn a() {}\n")]);
        assert_eq!(git_text(dir.path(), &["log", "--format=%s"]), "semente");
        assert_eq!(git_text(dir.path(), &["ls-files"]), "mustard.json\nsrc/a.rs");
        assert_eq!(git_text(dir.path(), &["status", "--porcelain"]), "", "nothing left to commit");
        assert_eq!(std::fs::read_to_string(dir.path().join("src/a.rs")).unwrap(), "fn a() {}\n");
    }

    /// Dois repositórios do mesmo conjunto partem do mesmo commit e não se
    /// enxergam: o que um comita ou configura não chega ao outro.
    #[test]
    fn two_seeded_repos_of_one_set_start_equal_and_never_see_each_other() {
        let files = [("a.txt", "a\n")];
        let (first, second) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        seeded_repo(first.path(), &files);
        seeded_repo(second.path(), &files);
        let seed = git_text(first.path(), &["rev-parse", "HEAD"]);
        assert_eq!(git_text(second.path(), &["rev-parse", "HEAD"]), seed, "the same seed commit");

        std::fs::write(first.path().join("b.txt"), "b\n").unwrap();
        git(first.path(), &["add", "-A"]);
        git(first.path(), &["commit", "-q", "-m", "so no primeiro"]);
        git(first.path(), &["config", "user.name", "outro"]);

        assert_eq!(git_text(second.path(), &["rev-parse", "HEAD"]), seed, "the second kept its seed");
        assert_eq!(git_text(second.path(), &["config", "user.name"]), "t", "the second kept its identity");
        assert!(!second.path().join("b.txt").exists());
    }

    /// O modelo de cada `key` se monta uma vez; a segunda pasta vazia só ganha
    /// a cópia, e uma `key` nova monta de novo.
    #[test]
    fn the_template_of_a_key_is_built_once_and_the_next_empty_folder_is_a_copy() {
        static BUILDS: AtomicUsize = AtomicUsize::new(0);
        let build = |dir: &Path| {
            BUILDS.fetch_add(1, Ordering::SeqCst);
            std::fs::write(dir.join("f.txt"), "f\n").unwrap();
        };
        let (a, b, c) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        repo_from_template(a.path(), "template-test:one", build);
        repo_from_template(b.path(), "template-test:one", build);
        assert_eq!(BUILDS.load(Ordering::SeqCst), 1, "the second folder is a copy");
        assert_eq!(std::fs::read_to_string(b.path().join("f.txt")).unwrap(), "f\n");
        repo_from_template(c.path(), "template-test:another", build);
        assert_eq!(BUILDS.load(Ordering::SeqCst), 2, "a new key builds its own template");
    }

    /// O repositório sai com a manutenção automática do git desligada, na
    /// primeira chamada da `key` e na cópia, e também quando se monta no lugar:
    /// é o que tira do commit de cada teste o processo da manutenção.
    #[test]
    fn a_repo_from_a_template_has_the_automatic_maintenance_off() {
        let build = |dir: &Path| git(dir, &["init", "-q"]);
        let (first, copy, in_place) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        std::fs::write(in_place.path().join("antes.txt"), "antes\n").unwrap();
        repo_from_template(first.path(), "template-test:maintenance", build);
        repo_from_template(copy.path(), "template-test:maintenance", build);
        repo_from_template(in_place.path(), "template-test:maintenance", build);
        for (case, dir) in [("the first", &first), ("the copy", &copy), ("in place", &in_place)] {
            assert_eq!(git_text(dir.path(), &["config", "--get", "maintenance.auto"]), "false", "{case}");
            assert_eq!(git_text(dir.path(), &["config", "--get", "gc.auto"]), "0", "{case}");
        }
    }

    /// Numa pasta que já tem algo, o repositório se monta ali mesmo, com o que
    /// o teste escreveu antes dentro do commit da semente.
    #[test]
    fn a_folder_that_already_holds_something_is_built_in_place() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("antes.txt"), "antes\n").unwrap();
        seeded_repo(dir.path(), &[("a.txt", "a\n")]);
        assert_eq!(git_text(dir.path(), &["ls-files"]), "a.txt\nantes.txt");
        assert_eq!(git_text(dir.path(), &["log", "--format=%s"]), "semente");
    }
}
