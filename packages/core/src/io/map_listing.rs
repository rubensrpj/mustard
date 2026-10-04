//! O conteúdo do projeto como o git o vê agora: o commit do checkout, o blob
//! de cada arquivo e a branch de partida. O scan grava isso no censo, e
//! [`super::project_map::is_behind`] compara com o que o git diz hoje para
//! saber que o mapa ficou atrás. Só se lê o git; nada se grava.
//!
//! A leitura faz quatro chamadas ao git: o índice (`ls-files`), a situação
//! (`status`, que traz também o commit e a branch do checkout), as referências
//! (`for-each-ref`, que traz a branch padrão do servidor e as pontas) e o
//! cálculo dos blobs dos arquivos novos (`hash-object`). A pasta de dentro de
//! um repositório pede mais uma, a do caminho dela a partir do topo.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::io::map_db::MAP_DIR;
use crate::io::project_map::{MAP_FILE_NAME, MAP_JOURNAL_FILE_NAME, MAP_SHARED_FILE_NAME, MAP_WAL_FILE_NAME};

/// Quantos caminhos vão numa chamada só ao git que calcula blobs.
const HASH_BATCH: usize = 256;

/// O modo que o índice do git dá a um submódulo.
const SUBMODULE_MODE: &str = "160000";

/// A referência que aponta para a branch padrão do servidor.
const SERVER_HEAD: &str = "refs/remotes/origin/HEAD";

/// Cada arquivo do projeto como está agora, pelo git: o commit do checkout e
/// o id do blob do conteúdo de cada arquivo, pelo caminho relativo à pasta
/// lida. O arquivo comitado e intocado vem do índice; o mudado, o novo e o
/// que só está no índice vêm do mesmo cálculo sobre o conteúdo de agora. O
/// próprio mapa e o diário dele ficam de fora: senão cada gravação dele
/// mudaria a listagem.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    /// O commit do checkout; vazio num repositório sem commit.
    pub head: String,
    /// O blob de cada arquivo, pelo caminho.
    pub blobs: BTreeMap<String, String>,
    /// Os caminhos que o índice do git guarda, com os dos submódulos
    /// iniciados: só o que foi adicionado ao git, sem o arquivo novo que
    /// ninguém adicionou.
    pub indexed: BTreeSet<String>,
    /// A branch de partida do projeto e o commit da ponta dela.
    pub base: Base,
}

/// A branch de partida que o projeto declara no `mustard.json` e o commit da
/// ponta dela, de onde vem a história do git que o mapa guarda.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Base {
    /// O nome declarado; vazio quando o projeto não declara nenhum.
    pub name: String,
    /// O commit da ponta: a do servidor (`origin/<nome>`) quando o clone a
    /// tem, senão a local; vazio quando nenhuma das duas existe.
    pub tip: String,
}

impl Listing {
    /// Uma marca curta e estável de todos os pares caminho e blob: duas
    /// listagens com a mesma marca têm os mesmos arquivos com os mesmos
    /// conteúdos.
    #[must_use]
    pub fn digest(&self) -> String {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for (path, blob) in &self.blobs {
            for byte in path.bytes().chain([0]).chain(blob.bytes()).chain([0]) {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0100_0000_01b3);
            }
        }
        format!("{hash:016x}-{}", self.blobs.len())
    }
}

/// O git em `root`, com os caminhos escritos como são; `None` quando ele
/// falha ou falta.
fn git_out(root: &Path, args: &[&str]) -> Option<String> {
    let mut full = vec!["-c", "core.quotePath=false"];
    full.extend(args);
    let run = crate::platform::git::run(root, &full);
    run.ok.then_some(run.stdout)
}

/// O conteúdo do projeto em `root` agora, pelo git. `None` fora do git.
#[must_use]
pub fn listing(root: &Path) -> Option<Listing> {
    let own = [MAP_FILE_NAME, MAP_JOURNAL_FILE_NAME, MAP_WAL_FILE_NAME, MAP_SHARED_FILE_NAME]
        .map(|name| format!("{MAP_DIR}/{name}"));
    let Files { blobs, indexed, checkout } = files_under(root, &own)?;
    Some(Listing { head: checkout.head, blobs, indexed, base: base_with(root, || checkout.branch) })
}

/// A branch de partida do projeto em `root`, pela configuração dele, com a
/// ponta que o clone tem: a do servidor antes da local, numa chamada só ao
/// git. O projeto que não declara nenhuma parte da branch padrão do servidor
/// (`refs/remotes/origin/HEAD`) e, sem servidor, da branch em que o checkout
/// está; só se lê o git, nada se grava. Sem declaração, sem servidor e com o
/// checkout solto de branch, não há base.
#[must_use]
pub fn base_of(root: &Path) -> Base {
    base_with(root, || {
        let out = git_out(root, &["symbolic-ref", "-q", "HEAD"])?;
        out.trim().strip_prefix("refs/heads/").filter(|name| !name.is_empty()).map(str::to_string)
    })
}

/// [`base_of`] com a branch do checkout vinda de quem já a leu, para a
/// listagem não perguntá-la de novo ao git. `checked_out` só é chamada quando
/// nem o projeto nem o servidor dizem a base.
fn base_with(root: &Path, checked_out: impl FnOnce() -> Option<String>) -> Base {
    let listed = git_out(root, &["for-each-ref", "--format=%(objectname) %(refname) %(symref)", "refs/remotes/origin", "refs/heads"])
        .unwrap_or_default();
    // Cada referência: o commit, o nome e, na que aponta para outra, o destino.
    let refs: Vec<(&str, &str, &str)> = listed
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, ' ');
            Some((parts.next()?, parts.next()?, parts.next().unwrap_or("")))
        })
        .collect();
    let server_default = refs
        .iter()
        .find(|(_, name, _)| *name == SERVER_HEAD)
        .and_then(|(_, _, target)| target.strip_prefix("refs/remotes/origin/"))
        .filter(|name| !name.is_empty())
        .map(str::to_string);
    let Some(name) = crate::domain::config::ProjectConfig::load(root).git.primary_base().or(server_default).or_else(checked_out) else {
        return Base::default();
    };
    let tip_of = |wanted: &str| refs.iter().find(|(_, refname, _)| *refname == wanted).map(|(tip, _, _)| (*tip).to_string());
    let tip = tip_of(&format!("refs/remotes/origin/{name}")).or_else(|| tip_of(&format!("refs/heads/{name}"))).unwrap_or_default();
    Base { name, tip }
}

/// O que a situação do git diz do checkout de uma pasta.
#[derive(Default)]
struct Checkout {
    /// O commit; vazio num repositório sem commit.
    head: String,
    /// A branch; `None` com o checkout solto de branch.
    branch: Option<String>,
}

/// Os arquivos de uma pasta e o checkout em que ela está.
struct Files {
    blobs: BTreeMap<String, String>,
    indexed: BTreeSet<String>,
    checkout: Checkout,
}

/// O blob de cada arquivo sob `root`, com os de dentro dos submódulos
/// iniciados, pelo caminho relativo a `root`, fora os caminhos `skip`, que
/// nem se calculam; os caminhos que o índice do git guarda, da mesma leitura
/// do índice; e o commit e a branch do checkout, da mesma leitura da situação.
fn files_under(root: &Path, skip: &[String]) -> Option<Files> {
    let staged = git_out(root, &["ls-files", "-s", "-z"])?;
    let mut blobs = BTreeMap::new();
    let mut indexed = BTreeSet::new();
    let mut nested = Vec::new();
    for entry in staged.split('\0') {
        let Some((meta, path)) = entry.split_once('\t') else { continue };
        let mut parts = meta.split(' ');
        let (Some(mode), Some(blob)) = (parts.next(), parts.next()) else { continue };
        if mode == SUBMODULE_MODE {
            nested.push(path.to_string());
        } else if !skip.iter().any(|own| own == path) {
            blobs.insert(path.to_string(), blob.to_string());
            indexed.insert(path.to_string());
        }
    }
    // A pasta que tem `.git` é o topo do repositório, ou do submódulo: o
    // caminho dela é vazio. Só a pasta de dentro de um repositório pergunta ao
    // git onde está.
    let prefix = if root.join(".git").exists() {
        String::new()
    } else {
        git_out(root, &["rev-parse", "--show-prefix"])?.trim().to_string()
    };
    let status = git_out(root, &["status", "--porcelain=v2", "--branch", "-z", "--untracked-files=all", "--", "."])?;
    let mut checkout = Checkout::default();
    let mut fresh = Vec::new();
    let mut fields = status.split('\0');
    while let Some(entry) = fields.next() {
        if let Some(header) = entry.strip_prefix("# ") {
            if let Some(oid) = header.strip_prefix("branch.oid ") {
                checkout.head = if oid == "(initial)" { String::new() } else { oid.to_string() };
            } else if let Some(name) = header.strip_prefix("branch.head ") {
                checkout.branch = (name != "(detached)").then(|| name.to_string());
            }
            continue;
        }
        let Some((path, renamed)) = changed_path(entry) else { continue };
        // A troca de nome e a cópia trazem o caminho antigo no campo seguinte.
        if renamed {
            let _ = fields.next();
        }
        // Os caminhos da situação partem do topo do repositório.
        let Some(rel) = path.strip_prefix(prefix.as_str()) else { continue };
        if skip.iter().any(|own| own == rel) {
            blobs.remove(rel);
        } else if root.join(rel).is_file() {
            fresh.push(rel.to_string());
        } else {
            blobs.remove(rel);
        }
    }
    for batch in fresh.chunks(HASH_BATCH) {
        let mut args = vec!["hash-object", "--"];
        args.extend(batch.iter().map(String::as_str));
        let hashed = git_out(root, &args)?;
        for (path, blob) in batch.iter().zip(hashed.lines()) {
            blobs.insert(path.clone(), blob.trim().to_string());
        }
    }
    for sub in nested {
        let dir = root.join(&sub);
        if !dir.join(".git").exists() {
            continue;
        }
        let Files { blobs: inner, indexed: inner_indexed, .. } =
            files_under(&dir, &[]).unwrap_or(Files { blobs: BTreeMap::new(), indexed: BTreeSet::new(), checkout: Checkout::default() });
        for (path, blob) in inner {
            blobs.insert(format!("{sub}/{path}"), blob);
        }
        indexed.extend(inner_indexed.into_iter().map(|path| format!("{sub}/{path}")));
    }
    Some(Files { blobs, indexed, checkout })
}

/// O caminho de um item da situação do git (`status --porcelain=v2 -z`) e se
/// ele é uma troca de nome ou cópia, que traz o caminho antigo no campo
/// seguinte. `None` no item que não nomeia arquivo.
fn changed_path(entry: &str) -> Option<(&str, bool)> {
    // Os campos separados por espaço que vêm antes do caminho.
    let (before, renamed) = match entry.chars().next()? {
        '1' => (8, false),
        '2' => (9, true),
        'u' => (10, false),
        '?' => (1, false),
        _ => return None,
    };
    entry.splitn(before + 1, ' ').nth(before).map(|path| (path, renamed))
}

/// `true` quando `root` está dentro da árvore de trabalho de um repositório
/// git, a condição para o mapa se ler do projeto.
pub(super) fn inside_work_tree(root: &Path) -> bool {
    git_out(root, &["rev-parse", "--is-inside-work-tree"]).is_some_and(|out| out.trim() == "true")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// O git de `repo`, com a identidade de teste; devolve o que ele imprimiu.
    fn git_in(repo: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A base do mapa é a que o projeto declara; sem declaração, a que o
    /// servidor aponta como padrão; sem servidor, a do checkout; o checkout
    /// solto de branch, sem servidor, não tem base. O git só é lido: nem a
    /// configuração dele nem a do projeto ganham uma linha.
    #[test]
    fn the_base_of_the_map_is_the_declared_one_then_the_default_of_the_server_then_the_checkout() {
        let dir = tempdir().unwrap();
        let repo = dir.path();
        let git = |args: &[&str]| git_in(repo, args);
        git(&["init", "-q", "-b", "trunk"]);
        std::fs::write(repo.join("a.txt"), "a\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "first"]);
        let first = git(&["rev-parse", "HEAD"]);
        std::fs::write(repo.join("a.txt"), "b\n").unwrap();
        git(&["commit", "-q", "-am", "second"]);
        let second = git(&["rev-parse", "HEAD"]);
        git(&["branch", "develop", &first]);
        let config_before = std::fs::read_to_string(repo.join(".git/config")).unwrap();
        let named = |base: Base| (base.name, base.tip);

        assert_eq!(named(base_of(repo)), ("trunk".into(), second.clone()), "no declaration, no server: the branch of the checkout");

        git(&["update-ref", "refs/remotes/origin/main", &first]);
        git(&["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"]);
        assert_eq!(
            named(base_of(repo)),
            ("main".into(), first.clone()),
            "no declaration: the default branch of the server, at the tip the server has"
        );

        std::fs::write(repo.join("mustard.json"), r#"{"git": {"flow": {"*": "develop"}}}"#).unwrap();
        assert_eq!(named(base_of(repo)), ("develop".into(), first.clone()), "the declared base wins over the server default");
        std::fs::remove_file(repo.join("mustard.json")).unwrap();

        git(&["symbolic-ref", "--delete", "refs/remotes/origin/HEAD"]);
        git(&["checkout", "-q", "--detach"]);
        assert_eq!(named(base_of(repo)), (String::new(), String::new()), "a loose checkout with no server has no base");

        assert_eq!(std::fs::read_to_string(repo.join(".git/config")).unwrap(), config_before, "the git configuration was only read");
        assert!(!repo.join("mustard.json").exists(), "the project configuration was only read");
    }

    /// A listagem dá, pelo caminho relativo à pasta lida, o blob do que cada
    /// arquivo guarda agora: o do índice para o intocado, o do conteúdo para
    /// o editado e para o novo, nada para o apagado; o próprio mapa e os
    /// arquivos que o SQLite põe ao lado dele ficam de fora, e a marca muda
    /// com o conteúdo.
    #[test]
    fn the_listing_gives_the_blob_of_what_each_file_holds_now() {
        let dir = tempdir().unwrap();
        let repo = dir.path();
        let git = |args: &[&str]| git_in(repo, args);
        git(&["init", "-q"]);
        std::fs::create_dir_all(repo.join("sub")).unwrap();
        std::fs::write(repo.join("top.txt"), "fora\n").unwrap();
        std::fs::write(repo.join("sub/a.txt"), "a\n").unwrap();
        std::fs::write(repo.join("sub/gone.txt"), "g\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "first"]);
        let root = repo.join("sub");

        let first = listing(&root).expect("dentro do git");
        assert_eq!(first.head, git(&["rev-parse", "HEAD"]));
        assert_eq!(first.blobs.keys().collect::<Vec<_>>(), ["a.txt", "gone.txt"], "só o que mora na pasta lida");
        assert_eq!(first.blobs["a.txt"], git(&["hash-object", "sub/a.txt"]));

        std::fs::write(root.join("a.txt"), "a mudado\n").unwrap();
        std::fs::write(root.join("new.txt"), "novo\n").unwrap();
        std::fs::remove_file(root.join("gone.txt")).unwrap();
        std::fs::create_dir_all(root.join(MAP_DIR)).unwrap();
        std::fs::write(root.join(MAP_DIR).join(MAP_FILE_NAME), "mapa").unwrap();
        for beside in [MAP_JOURNAL_FILE_NAME, MAP_WAL_FILE_NAME, MAP_SHARED_FILE_NAME] {
            std::fs::write(root.join(MAP_DIR).join(beside), "ao lado").unwrap();
        }
        let now = listing(&root).expect("dentro do git");
        assert_eq!(now.blobs.keys().collect::<Vec<_>>(), ["a.txt", "new.txt"]);
        assert_eq!(now.blobs["a.txt"], git(&["hash-object", "sub/a.txt"]));
        assert_eq!(now.blobs["new.txt"], git(&["hash-object", "sub/new.txt"]));
        assert_ne!(now.digest(), first.digest());
        assert_eq!(now.digest(), listing(&root).unwrap().digest(), "a mesma listagem, a mesma marca");

        assert_eq!(listing(tempdir().unwrap().path()), None, "fora do git não há listagem");
    }

    /// Um arquivo que mudou de nome pelo índice aparece só com o nome novo, e
    /// o nome com espaço não se parte.
    #[test]
    fn a_renamed_file_and_a_name_with_a_space_are_listed_by_their_new_paths() {
        let dir = tempdir().unwrap();
        let repo = dir.path();
        let git = |args: &[&str]| git_in(repo, args);
        git(&["init", "-q"]);
        std::fs::write(repo.join("old.txt"), "conteudo antigo\n").unwrap();
        std::fs::write(repo.join("stays.txt"), "fica\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "first"]);
        git(&["mv", "old.txt", "new name.txt"]);
        std::fs::write(repo.join("with space.txt"), "novo\n").unwrap();

        let now = listing(repo).expect("dentro do git");
        assert_eq!(now.blobs.keys().collect::<Vec<_>>(), ["new name.txt", "stays.txt", "with space.txt"]);
        assert_eq!(now.blobs["new name.txt"], git(&["hash-object", "new name.txt"]));
        assert_eq!(now.blobs["with space.txt"], git(&["hash-object", "with space.txt"]));
    }

    /// O commit e a base da listagem seguem o checkout em cada estado dele —
    /// sem commit, com commit, com servidor, solto de branch —, e são os
    /// mesmos que `base_of` dá a quem só pergunta a base.
    #[test]
    fn the_head_and_the_base_of_the_listing_follow_the_checkout_like_the_base_alone() {
        let dir = tempdir().unwrap();
        let repo = dir.path();
        let git = |args: &[&str]| git_in(repo, args);
        let side_by_side = |head: &str, name: &str, tip: &str| {
            let now = listing(repo).expect("dentro do git");
            assert_eq!(now.head, head, "head");
            assert_eq!((now.base.name.as_str(), now.base.tip.as_str()), (name, tip), "base");
            assert_eq!(now.base, base_of(repo), "the listing and the base alone say the same");
        };
        git(&["init", "-q", "-b", "trunk"]);
        side_by_side("", "trunk", "");

        std::fs::write(repo.join("a.txt"), "a\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "first"]);
        let first = git(&["rev-parse", "HEAD"]);
        side_by_side(&first, "trunk", &first);

        git(&["update-ref", "refs/remotes/origin/main", &first]);
        git(&["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"]);
        side_by_side(&first, "main", &first);

        git(&["symbolic-ref", "--delete", "refs/remotes/origin/HEAD"]);
        git(&["checkout", "-q", "--detach"]);
        side_by_side(&first, "", "");
    }

    /// A leitura do estado do git custa quatro chamadas: o índice, a
    /// situação, o cálculo do blob dos arquivos novos e as referências. O
    /// programa que o projeto aponta como git é um que anota o que lhe pedem e
    /// repassa ao git de verdade.
    #[cfg(unix)]
    #[test]
    fn the_listing_asks_the_git_four_times() {
        let dir = tempdir().unwrap();
        let repo = dir.path();
        let git = |args: &[&str]| git_in(repo, args);
        git(&["init", "-q", "-b", "trunk"]);
        std::fs::create_dir_all(repo.join(".claude")).unwrap();
        std::fs::create_dir_all(repo.join("sub")).unwrap();
        std::fs::write(repo.join("sub/a.txt"), "a\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "first"]);
        std::fs::write(repo.join("sub/new.txt"), "novo\n").unwrap();

        let (spy, log) = (repo.join("spy"), repo.join("calls.log"));
        let program = format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexec git \"$@\"\n", log.display());
        // O programa é gravado por um shell à parte: os testes rodam em paralelo
        // no mesmo processo, e o arquivo que este processo mantém aberto para
        // escrita o Linux recusa rodar ("Text file busy").
        let written = std::process::Command::new("/bin/sh")
            .args(["-c", "printf '%s' \"$2\" > \"$1\" && chmod 755 \"$1\"", "sh"])
            .arg(&spy)
            .arg(&program)
            .status()
            .unwrap();
        assert!(written.success());
        let pinned = serde_json::json!({"vcs": spy.to_string_lossy()});
        std::fs::write(repo.join("mustard.json"), pinned.to_string()).unwrap();
        let calls = || -> Vec<String> {
            let text = std::fs::read_to_string(&log).unwrap_or_default();
            let words: Vec<String> =
                text.lines().map(|line| line.split(' ').filter(|word| !word.starts_with('-') && *word != "core.quotePath=false").next().unwrap_or("").to_string()).collect();
            let _ = std::fs::write(&log, "");
            words
        };

        let top = listing(repo).expect("dentro do git");
        assert_eq!(calls(), ["ls-files", "status", "hash-object", "for-each-ref"], "from the top of the repository");
        assert_eq!(top.blobs.keys().filter(|path| path.starts_with("sub/")).count(), 2);

        let inside = listing(&repo.join("sub")).expect("dentro do git");
        assert_eq!(
            calls(),
            ["ls-files", "rev-parse", "status", "hash-object", "for-each-ref"],
            "a folder inside the repository also asks where it is"
        );
        assert_eq!(inside.blobs.keys().collect::<Vec<_>>(), ["a.txt", "new.txt"]);
    }
}
