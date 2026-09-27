//! Incremental reading and the git history the map keeps.
//!
//! A pass reads again only the files whose content changed since the
//! previous one: each file carries the git blob id of the content the pass
//! read, and a file whose blob now is the same is taken from the previous map
//! as it was, on any branch, committed or not. Outside git, without a
//! previous map, when a part of the previous map the pass takes from was
//! written by another scanner build — or came back empty because its format
//! changed —, or when a file that changes how every other one is read
//! changed, every file is read.
//!
//! The history comes from the local repository (`git log`), never from the
//! network: the first pass reads it whole, and the next ones read only the
//! commits after the one the previous pass stopped at.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use mustard_core::platform::git as git_exec;

use mustard_core::domain::project_map::{History, RawCommit, MAX_COMMITS};
use mustard_core::io::project_map::{Listing, MapBlock, BLOCKS, CENSUS, DECLS, FILES, GRAPH};

use crate::model::{Module, ProjectModel};

/// The scanner build tag written into the map, as the mark of every block the
/// pass writes. A map written by another build is read again in full, because
/// what a file yields may have changed. The part after `+map-` is a digest of
/// the scan's own sources, worked out by the build script, so any change to
/// the scan changes it.
pub(crate) const FORMAT: &str = concat!(env!("CARGO_PKG_VERSION"), "+map-", env!("SCAN_MAP_DIGEST"));

/// The blocks of the map a pass that reads only what changed takes from the
/// previous one: the state and the manifests, the files, their declarations
/// and their links. The history is read on its own, from the commits after
/// the one the previous pass stopped at.
const REUSED: [&MapBlock; 4] = [&CENSUS, &FILES, &DECLS, &GRAPH];

/// The block `block` of the previous map was filled by this scanner build.
/// A block rebuilt because its format changed comes back empty and without a
/// mark, so it is never taken as fresh.
fn fresh(prev: &ProjectModel, block: &MapBlock) -> bool {
    prev.marks.get(block.name()).is_some_and(|mark| mark == FORMAT)
}

/// Files whose change alters how every other file is classified: when one of
/// them changed, everything is read again. Os arquivos de configuração de
/// apelidos de pasta que o registro declara entram pela mesma porta (ver
/// [`rereads_everything`]): um apelido novo liga citações que a passada
/// anterior deixou fora do mapa.
const GLOBAL_INPUTS: &[&str] = &[".gitattributes", ".editorconfig"];

/// What a pass has to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Plan {
    /// Every file.
    Full,
    /// Only these files (relative to the scanned root), plus any file the
    /// previous map does not know.
    Only(BTreeSet<String>),
}

/// Run git in `root`, with paths printed as they are (no octal quoting of
/// accented names). `None` when git is missing or the command fails.
fn git(root: &Path, args: &[&str]) -> Option<String> {
    let mut full: Vec<&str> = vec!["-c", "core.quotePath=false"];
    full.extend(args);
    let out = git_exec::run(root, &full);
    out.ok.then_some(out.stdout)
}

/// The commit checked out in `root`, or `None` outside git and on a branch
/// with no commit yet.
pub(crate) fn head(root: &Path) -> Option<String> {
    git(root, &["rev-parse", "HEAD"]).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// The scanned root as the map records it.
pub(crate) fn canonical(root: &Path) -> String {
    root.canonicalize().unwrap_or_else(|_| root.to_path_buf()).to_string_lossy().to_string()
}

/// Decide what this pass reads, given the previous map and what git says
/// each file holds now (`None` outside git).
pub(crate) fn plan(root: &Path, prev: Option<&ProjectModel>, listing: Option<&Listing>) -> Plan {
    let (Some(prev), Some(listing)) = (prev, listing) else {
        return Plan::Full;
    };
    if !REUSED.iter().all(|block| fresh(prev, block))
        || prev.root != canonical(root)
        || rereads_everything(&prev.state.inputs) != rereads_everything(&listing.blobs)
    {
        return Plan::Full;
    }
    // Cada arquivo que a passada anterior guardou, com o blob do que ela leu:
    // os de código, os manifestos e os que não se decodificaram. O blob vazio
    // é o de um arquivo lido fora do git, que se relê sempre.
    let stored_input = |path: &str| prev.state.inputs.get(path).map_or("", String::as_str);
    let stored = prev
        .modules
        .iter()
        .map(|m| (m.path.as_str(), m.blob.as_str()))
        .chain(prev.manifests.iter().map(|m| (m.path.as_str(), stored_input(&m.path))))
        .chain(prev.state.non_utf8.iter().map(|path| (path.as_str(), stored_input(path))));
    let mut changed: BTreeSet<String> = stored
        .filter(|(path, blob)| blob.is_empty() || listing.blobs.get(*path).map(String::as_str) != Some(*blob))
        .map(|(path, _)| path.to_string())
        .collect();
    let under = under_changed_manifests(root, prev, listing, &changed);
    changed.extend(under);
    Plan::Only(changed)
}

/// Os arquivos de código que um manifesto mudado faz reler. A dependência do
/// manifesto mais próximo acima liga regras de rota nos arquivos sob a pasta
/// dele (ver [`crate::graph::nearest_manifest_deps`]): o manifesto cujas
/// dependências mudaram, que apareceu ou que sumiu relê os arquivos de
/// código dessa pasta. O que apareceu ou sumiu muda também o alcance do
/// import global escrito sob a pasta dele (ver [`crate::graph::global_reach`]):
/// aí os arquivos da língua desse import se releem todos.
fn under_changed_manifests(
    root: &Path,
    prev: &ProjectModel,
    listing: &Listing,
    changed: &BTreeSet<String>,
) -> BTreeSet<String> {
    let name_of = |path: &str| path.rsplit('/').next().unwrap_or(path).to_string();
    let folder_of = |path: &str| path.rfind('/').map_or(String::new(), |at| path[..at].to_string());
    let is_under = |path: &str, dir: &str| dir.is_empty() || path.strip_prefix(dir).is_some_and(|rest| rest.starts_with('/'));
    // Cada pasta de manifesto mudado, e se ele apareceu ou sumiu.
    let mut folders: Vec<(String, bool)> = Vec::new();
    for manifest in prev.manifests.iter().filter(|m| changed.contains(&m.path)) {
        let now = std::fs::read_to_string(root.join(&manifest.path))
            .ok()
            .and_then(|content| crate::manifests::parse(&manifest.path, &name_of(&manifest.path), &content));
        let before: BTreeSet<&String> = manifest.dependencies.iter().collect();
        match now {
            None => folders.push((folder_of(&manifest.path), true)),
            Some(parsed) if parsed.deps.iter().collect::<BTreeSet<_>>() != before => {
                folders.push((folder_of(&manifest.path), false));
            }
            Some(_) => {}
        }
    }
    let known: BTreeSet<&str> = prev.manifests.iter().map(|m| m.path.as_str()).collect();
    for path in listing.blobs.keys().filter(|path| !known.contains(path.as_str())) {
        if crate::manifests::is_manifest(&name_of(path)) {
            folders.push((folder_of(path), true));
        }
    }

    let mut again: BTreeSet<String> = BTreeSet::new();
    let mut global_languages: BTreeSet<&str> = BTreeSet::new();
    for (folder, came_or_went) in &folders {
        for m in prev.modules.iter().filter(|m| is_under(&m.path, folder)) {
            again.insert(m.path.clone());
            if *came_or_went && !m.global_imports.is_empty() {
                global_languages.insert(m.language.as_str());
            }
        }
    }
    again.extend(prev.modules.iter().filter(|m| global_languages.contains(m.language.as_str())).map(|m| m.path.clone()));
    again
}

/// A passada não tem arquivo a reler: todo bloco do mapa anterior `prev` é
/// desta versão do scan, o commit é o mesmo que ele leu — um commit novo
/// pode mudar a história e os testes que mudam juntos — e o plano não pede
/// arquivo nenhum. Falta só conferir que não entrou nem saiu arquivo de
/// código ou manifesto (ver [`crate::ingest::same_sources`]).
pub(crate) fn nothing_to_read(root: &Path, prev: &ProjectModel, listing: &Listing) -> bool {
    BLOCKS.iter().all(|block| fresh(prev, block))
        && prev.state.head == listing.head
        && plan(root, Some(prev), Some(listing)) == Plan::Only(BTreeSet::new())
}

/// Os arquivos de `blobs` que, mudando, aparecendo ou sumindo, fazem a
/// passada ler tudo: os de [`GLOBAL_INPUTS`] e as configurações de apelidos
/// de pasta, com o nome vindo do registro. A citação que só o apelido novo
/// liga saiu do módulo na passada anterior, e só a leitura do arquivo a traz
/// de volta.
fn rereads_everything(blobs: &BTreeMap<String, String>) -> BTreeMap<&str, &str> {
    let alias_configs = crate::extract::alias_config_names();
    blobs
        .iter()
        .filter(|(path, _)| {
            let name = path.rsplit('/').next().unwrap_or(path);
            GLOBAL_INPUTS.contains(&name) || alias_configs.contains(name)
        })
        .map(|(path, blob)| (path.as_str(), blob.as_str()))
        .collect()
}

/// O blob, pelo caminho, de cada arquivo que decide a releitura sem ser
/// código: os de [`rereads_everything`], os manifestos `manifests` e os que
/// não se decodificaram, `undecodable`. É o que a passada guarda para a
/// seguinte comparar.
pub(crate) fn inputs(listing: &Listing, manifests: &[&str], undecodable: &[String]) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> =
        rereads_everything(&listing.blobs).into_iter().map(|(path, blob)| (path.to_string(), blob.to_string())).collect();
    for path in manifests.iter().copied().chain(undecodable.iter().map(String::as_str)) {
        if let Some(blob) = listing.blobs.get(path) {
            out.insert(path.to_string(), blob.clone());
        }
    }
    out
}

/// The files a pass that read only what changed has to read again, so that
/// their citations link as a pass reading every file would link them.
///
/// A file keeps in the map only the citations that linked (see
/// [`crate::graph::link_declarations`]): a name it cites that no file in its
/// sight declared was dropped. A file that did not change still gains a link
/// when that changes, and the name it cites is then no longer in the map. So
/// a file taken from the previous map is read again when:
///
/// - its text has, as a whole word, a name some file now declares or no
///   longer declares, as a constant or a type, where the previous map said
///   otherwise — a file read now that declares a name it did not, a new file,
///   a file whose namespaces changed (all it declares is then seen by other
///   files), a file gone. Losing a declaration counts too: a name declared
///   too many times links nowhere, and one fewer may make it link;
/// - it imports a file of the project it did not import before;
/// - a global import of its language was written, changed or removed, since
///   it may put in sight files that did not change.
///
/// `fresh` holds the files this pass read; `modules`, what this pass has, with
/// the imports already resolved.
pub(crate) fn stale_citers(
    root: &Path,
    prev: &ProjectModel,
    modules: &[Module],
    fresh: &BTreeSet<String>,
) -> BTreeSet<String> {
    let before: HashMap<&str, &Module> = prev.modules.iter().map(|m| (m.path.as_str(), m)).collect();
    let now: BTreeSet<&str> = modules.iter().map(|m| m.path.as_str()).collect();
    let cited = |m: &Module| -> BTreeSet<String> {
        m.declarations
            .iter()
            .filter(|d| crate::graph::CITED_KINDS.contains(&d.kind.as_str()))
            .map(|d| d.name.clone())
            .collect()
    };

    let mut names: BTreeSet<String> = BTreeSet::new();
    let mut global_languages: BTreeSet<&str> = BTreeSet::new();
    for m in modules.iter().filter(|m| fresh.contains(&m.path)) {
        match before.get(m.path.as_str()) {
            Some(old) if old.namespaces == m.namespaces && old.language == m.language => {
                names.extend(cited(m).symmetric_difference(&cited(old)).cloned());
            }
            _ => names.extend(cited(m)),
        }
        let old_globals = before.get(m.path.as_str()).map_or(&[][..], |old| old.global_imports.as_slice());
        if old_globals != m.global_imports.as_slice() {
            global_languages.insert(m.language.as_str());
        }
    }
    for gone in prev.modules.iter().filter(|m| !now.contains(m.path.as_str())) {
        names.extend(cited(gone));
        if !gone.global_imports.is_empty() {
            global_languages.insert(gone.language.as_str());
        }
    }

    modules
        .iter()
        .filter(|m| !fresh.contains(&m.path))
        .filter(|m| {
            global_languages.contains(m.language.as_str())
                || before.get(m.path.as_str()).is_none_or(|old| m.deps.iter().any(|d| !old.deps.contains(d)))
                || (!names.is_empty()
                    && std::fs::read_to_string(root.join(&m.path)).is_ok_and(|text| names.iter().any(|n| has_word(&text, n))))
        })
        .map(|m| m.path.clone())
        .collect()
}

/// `word` is written in `text` as a whole name: not inside a longer one.
fn has_word(text: &str, word: &str) -> bool {
    let part_of_name = |c: char| c.is_alphanumeric() || c == '_';
    text.match_indices(word).any(|(at, _)| {
        !text[..at].chars().next_back().is_some_and(part_of_name)
            && !text[at + word.len()..].chars().next().is_some_and(part_of_name)
    })
}

/// `true` when `ancestor` is in the history of `head`.
fn is_ancestor(root: &Path, ancestor: &str, head: &str) -> bool {
    git_exec::run(root, &["merge-base", "--is-ancestor", ancestor, head]).ok
}

/// The history at `head`: the previous one when nothing was committed since,
/// the previous one plus the new commits when `head` descends from where the
/// previous pass stopped, and the whole log otherwise (another branch, a
/// rewritten history, a first pass).
pub(crate) fn history(root: &Path, prev: Option<&History>, prev_head: &str, head: &str) -> History {
    let prev = prev.filter(|h| !h.is_empty() && !prev_head.is_empty());
    if let Some(prev) = prev {
        if prev_head == head {
            return prev.clone();
        }
        if is_ancestor(root, prev_head, head) {
            return match log(root, &format!("{prev_head}..{head}")) {
                Some(newer) => prev.extended(newer),
                None => prev.clone(),
            };
        }
    }
    log(root, head).map(History::from_raw).unwrap_or_default()
}

/// The commits of `range` that touch `root`, oldest first.
fn log(root: &Path, range: &str) -> Option<Vec<RawCommit>> {
    let max = format!("--max-count={MAX_COMMITS}");
    let out = git(
        root,
        &["log", "--no-merges", "--no-renames", "--relative", "--name-status", "--format=%x00%H %ct %s", &max, range],
    )?;
    Some(parse_log(&out))
}

/// Parse `git log --name-status --format=%x00%H %ct %s` into commits, oldest
/// first, each with its title. A commit that touches nothing under the
/// scanned root is left out.
pub(crate) fn parse_log(text: &str) -> Vec<RawCommit> {
    let mut commits = Vec::new();
    for block in text.split('\0').filter(|b| !b.trim().is_empty()) {
        let mut lines = block.lines();
        let Some(header) = lines.next() else {
            continue;
        };
        let mut parts = header.splitn(3, ' ');
        let (Some(sha), Some(at)) = (parts.next(), parts.next()) else {
            continue;
        };
        let title = parts.next().unwrap_or_default().trim().to_string();
        let mut commit = RawCommit {
            id: sha.chars().take(10).collect(),
            at: at.trim().parse().unwrap_or(0),
            title,
            ..RawCommit::default()
        };
        for line in lines {
            let Some((status, path)) = line.split_once('\t') else {
                continue;
            };
            let path = unquote(path);
            match status.chars().next() {
                Some('A') => commit.added.push(path),
                Some('D') | None => {}
                Some(_) => commit.changed.push(path),
            }
        }
        if !commit.added.is_empty() || !commit.changed.is_empty() {
            commits.push(commit);
        }
    }
    commits.reverse();
    commits
}

/// Undo git's C-style quoting of a path with unusual characters.
fn unquote(path: &str) -> String {
    let Some(inner) = path.strip_prefix('"').and_then(|p| p.strip_suffix('"')) else {
        return path.to_string();
    };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_is_read_oldest_first_without_deletions_or_empty_commits() {
        let text = "\0bbbbbbbbbbbbbbbb 20 Troca o  leitor\n\nM\tsrc/a.rs\nD\tsrc/old.rs\nA\t\"src/with\\\"quote.rs\"\n\
                    \0cccccccccccc 15 Vazio\n\n\
                    \0aaaaaaaaaaaaaaaa 10\n\nA\tsrc/a.rs\n";
        let commits = parse_log(text);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].id, "aaaaaaaaaa");
        assert_eq!(commits[0].added, vec!["src/a.rs".to_string()]);
        assert_eq!(commits[1].at, 20);
        assert_eq!(commits[1].title, "Troca o  leitor");
        assert_eq!(commits[0].title, "", "a commit written with no title keeps none");
        assert_eq!(commits[1].changed, vec!["src/a.rs".to_string()]);
        assert_eq!(commits[1].added, vec!["src/with\"quote.rs".to_string()]);
    }

    /// Todo bloco que a passada reaproveita, marcado por esta versão do scan.
    fn marked_by_this_build() -> std::collections::BTreeMap<String, String> {
        REUSED.iter().map(|block| (block.name().to_string(), FORMAT.to_string())).collect()
    }

    /// A listagem do git com estes arquivos e blobs.
    fn listing_of(files: &[(&str, &str)]) -> Listing {
        Listing { head: String::new(), blobs: files.iter().map(|(path, blob)| (path.to_string(), blob.to_string())).collect() }
    }

    /// O mapa anterior desta versão do scan, com estes arquivos de código e
    /// os blobs que ela leu.
    fn previous(root: &Path, files: &[(&str, &str)]) -> ProjectModel {
        ProjectModel {
            root: canonical(root),
            modules: files
                .iter()
                .map(|(path, blob)| Module { path: path.to_string(), blob: blob.to_string(), ..Module::default() })
                .collect(),
            marks: marked_by_this_build(),
            ..Default::default()
        }
    }

    #[test]
    fn without_a_previous_map_or_outside_git_everything_is_read() {
        let here = Path::new(".");
        let now = listing_of(&[("a.rs", "1")]);
        assert_eq!(plan(here, None, Some(&now)), Plan::Full);
        let prev = previous(here, &[("a.rs", "1")]);
        assert_eq!(plan(here, Some(&prev), Some(&now)), Plan::Only(BTreeSet::new()));
        assert_eq!(plan(here, Some(&prev), None), Plan::Full, "fora do git não há blob para comparar");
        let other_build = ProjectModel {
            marks: REUSED.iter().map(|block| (block.name().to_string(), "0.0.0+old".to_string())).collect(),
            ..prev
        };
        assert_eq!(plan(here, Some(&other_build), Some(&now)), Plan::Full);
    }

    /// Só se relê o arquivo cujo blob de agora não é o que a passada anterior
    /// leu: o de código, o manifesto e o que não se decodificou. O lido fora
    /// do git, sem blob, se relê sempre; o arquivo novo entra pela caminhada.
    /// O manifesto que mudou sem mudar as dependências relê só ele.
    #[test]
    fn only_the_files_whose_blob_changed_are_read() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("Cargo.toml"), "[package]\nname = \"outro\"\n").unwrap();
        let here = temp.path();
        let mut prev = previous(here, &[("a.rs", "1"), ("b.rs", "2"), ("c.rs", "")]);
        prev.manifests.push(crate::model::Manifest { path: "Cargo.toml".to_string(), ..Default::default() });
        prev.state.non_utf8 = vec!["latin.rs".to_string()];
        prev.state.inputs = [("Cargo.toml", "9"), ("latin.rs", "7")].map(|(p, b)| (p.to_string(), b.to_string())).into();
        let only = |files: &[&str]| Plan::Only(files.iter().map(|f| f.to_string()).collect());

        let now = [("a.rs", "1"), ("b.rs", "3"), ("c.rs", "4"), ("d.rs", "5"), ("Cargo.toml", "9"), ("latin.rs", "7")];
        assert_eq!(plan(here, Some(&prev), Some(&listing_of(&now))), only(&["b.rs", "c.rs"]));

        let mut touched = now;
        touched[4] = ("Cargo.toml", "8");
        touched[5] = ("latin.rs", "6");
        assert_eq!(plan(here, Some(&prev), Some(&listing_of(&touched))), only(&["Cargo.toml", "b.rs", "c.rs", "latin.rs"]));
    }

    /// O manifesto cujas dependências mudaram, que apareceu ou que sumiu relê
    /// os arquivos de código sob a pasta dele, e só eles; o que apareceu ou
    /// sumiu acima de um import global relê também os arquivos da língua dele.
    #[test]
    fn a_manifest_whose_dependencies_changed_reads_the_code_under_it_again() {
        let temp = tempfile::tempdir().unwrap();
        let here = temp.path();
        let csproj = |sdk: &str| format!("<Project Sdk=\"{sdk}\">\n</Project>\n");
        for (dir, sdk) in [("api", "Microsoft.NET.Sdk"), ("web", "Microsoft.NET.Sdk.Web"), ("novo", "Microsoft.NET.Sdk")] {
            std::fs::create_dir_all(here.join(dir)).unwrap();
            let name = format!("{}.csproj", dir);
            std::fs::write(here.join(dir).join(name), csproj(sdk)).unwrap();
        }
        let code = [("api/a.cs", "1"), ("web/b.cs", "2"), ("web/sub/c.cs", "3"), ("novo/d.cs", "4"), ("foi/e.cs", "5"), ("x.rs", "6")];
        let mut prev = previous(here, &code);
        for m in &mut prev.modules {
            m.language = if m.path.ends_with(".cs") { "csharp" } else { "rust" }.to_string();
        }
        let manifest = |path: &str, dep: &str| crate::model::Manifest {
            path: path.to_string(),
            dependencies: vec![dep.to_string()],
            ..Default::default()
        };
        prev.manifests = vec![
            manifest("api/api.csproj", "Microsoft.NET.Sdk"),
            manifest("web/web.csproj", "Microsoft.NET.Sdk"),
            manifest("foi/foi.csproj", "Microsoft.NET.Sdk"),
        ];
        prev.state.inputs = [("api/api.csproj", "a"), ("web/web.csproj", "w"), ("foi/foi.csproj", "f")]
            .map(|(p, b)| (p.to_string(), b.to_string()))
            .into();
        let only = |files: &[&str]| Plan::Only(files.iter().map(|f| f.to_string()).collect());
        let mut now: Vec<(&str, &str)> = code.to_vec();
        now.extend([("api/api.csproj", "a2"), ("web/web.csproj", "w2"), ("foi/foi.csproj", "f")]);
        assert_eq!(
            plan(here, Some(&prev), Some(&listing_of(&now))),
            only(&["api/api.csproj", "web/b.cs", "web/sub/c.cs", "web/web.csproj"]),
            "o SDK do web mudou; o do api é o mesmo"
        );

        let mut came_and_went: Vec<(&str, &str)> = code.to_vec();
        came_and_went.extend([("api/api.csproj", "a"), ("web/web.csproj", "w"), ("novo/novo.csproj", "n")]);
        assert_eq!(
            plan(here, Some(&prev), Some(&listing_of(&came_and_went))),
            only(&["foi/e.cs", "foi/foi.csproj", "novo/d.cs"]),
            "o manifesto novo e o que sumiu, sem import global embaixo"
        );

        prev.modules.iter_mut().find(|m| m.path == "novo/d.cs").unwrap().global_imports = vec!["Algo".to_string()];
        assert_eq!(
            plan(here, Some(&prev), Some(&listing_of(&came_and_went))),
            only(&["api/a.cs", "foi/e.cs", "foi/foi.csproj", "novo/d.cs", "web/b.cs", "web/sub/c.cs"]),
            "o manifesto novo acima de um import global relê a língua dele"
        );
    }

    /// Basta um dos blocos reaproveitados sem a marca desta versão — o que
    /// volta vazio quando o formato dele muda — para a passada ler tudo de
    /// novo.
    #[test]
    fn a_block_not_filled_by_this_build_makes_the_pass_read_everything() {
        let here = Path::new(".");
        let prev = previous(here, &[("a.rs", "1")]);
        let now = listing_of(&[("a.rs", "2")]);
        assert_eq!(plan(here, Some(&prev), Some(&now)), Plan::Only(BTreeSet::from(["a.rs".to_string()])));
        for block in REUSED {
            let mut stale = prev.clone();
            stale.marks.insert(block.name().to_string(), String::new());
            assert_eq!(plan(here, Some(&stale), Some(&now)), Plan::Full, "{}", block.name());
            stale.marks.remove(block.name());
            assert_eq!(plan(here, Some(&stale), Some(&now)), Plan::Full, "{}", block.name());
        }
    }

    /// O arquivo que muda a leitura de todos os outros — o de atributos do
    /// git, o do editor e a configuração dos apelidos de pasta — faz a
    /// passada ler tudo quando muda, aparece ou some, em qualquer pasta.
    #[test]
    fn a_file_that_changes_every_reading_makes_the_pass_read_everything() {
        let here = Path::new(".");
        let mut prev = previous(here, &[("src/a.ts", "1")]);
        let now = [("src/a.ts", "1"), ("web/.editorconfig", "5")];
        prev.state.inputs = inputs(&listing_of(&now), &[], &[]);
        assert_eq!(prev.state.inputs.len(), 1, "{:?}", prev.state.inputs);
        assert_eq!(plan(here, Some(&prev), Some(&listing_of(&now))), Plan::Only(BTreeSet::new()));

        let changed = [("src/a.ts", "1"), ("web/.editorconfig", "6")];
        assert_eq!(plan(here, Some(&prev), Some(&listing_of(&changed))), Plan::Full, "mudou");
        assert_eq!(plan(here, Some(&prev), Some(&listing_of(&now[..1]))), Plan::Full, "sumiu");
        let alias = [("src/a.ts", "1"), ("web/.editorconfig", "5"), ("tsconfig.json", "8")];
        assert_eq!(plan(here, Some(&prev), Some(&listing_of(&alias))), Plan::Full, "a configuração de apelidos apareceu");
    }
}
