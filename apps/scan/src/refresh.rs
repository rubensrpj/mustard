//! Incremental reading and the git history the map keeps.
//!
//! A pass reads again only what can have changed since the previous one: the
//! files git says changed between the commit the previous pass read and the
//! one checked out now, the files not committed now, and the files that were
//! not committed then (they may have been put back since). Everything else is
//! taken from the previous map as it was. Outside git, without a previous
//! map, or when the previous map was written by another scanner build, every
//! file is read.
//!
//! The history comes from the local repository (`git log`), never from the
//! network: the first pass reads it whole, and the next ones read only the
//! commits after the one the previous pass stopped at.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use mustard_core::platform::git as git_exec;

use mustard_core::domain::project_map::{History, RawCommit, MAX_COMMITS};

use crate::model::{Module, ProjectModel};

/// The scanner build tag written into the map. A map written by another
/// build is read again in full, because what a file yields may have changed.
/// The part after `+map-` is a digest of the scan's own sources, worked out by
/// the build script, so any change to the scan changes it.
pub(crate) const FORMAT: &str = concat!(env!("CARGO_PKG_VERSION"), "+map-", env!("SCAN_MAP_DIGEST"));

/// Files whose change alters how every other file is classified: when one of
/// them changed, everything is read again.
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

/// Where `root` sits inside its repository (`apps/scan/`), empty at the top.
fn prefix(root: &Path) -> Option<String> {
    git(root, &["rev-parse", "--show-prefix"]).map(|s| s.trim().to_string())
}

/// The files under `root` that are not committed — changed, staged or new —
/// relative to `root`, in name order.
pub(crate) fn dirty(root: &Path) -> Option<Vec<String>> {
    let prefix = prefix(root)?;
    let out = git(root, &["status", "--porcelain=v1", "-z", "--untracked-files=all", "--", "."])?;
    let mut paths = BTreeSet::new();
    let mut fields = out.split('\0');
    while let Some(entry) = fields.next() {
        if entry.len() < 4 {
            continue;
        }
        let (code, path) = entry.split_at(3);
        // A rename or a copy carries the old path in the next field.
        if code[..2].contains(['R', 'C']) {
            let _ = fields.next();
        }
        // The status paths are relative to the top of the repository.
        if let Some(rel) = path.strip_prefix(prefix.as_str()) {
            paths.insert(rel.to_string());
        }
    }
    Some(paths.into_iter().collect())
}

/// The files under `root` that differ between two commits, relative to
/// `root`. `None` when git cannot compare them (a commit that no longer
/// exists, for one).
fn changed_between(root: &Path, from: &str, to: &str) -> Option<Vec<String>> {
    let out = git(root, &["diff", "--name-only", "-z", "--no-renames", "--relative", from, to])?;
    Some(out.split('\0').filter(|p| !p.is_empty()).map(str::to_string).collect())
}

/// The scanned root as the map records it.
pub(crate) fn canonical(root: &Path) -> String {
    root.canonicalize().unwrap_or_else(|_| root.to_path_buf()).to_string_lossy().to_string()
}

/// The `state.head` a pass writes when the repository has no commit at all —
/// distinct from the empty string, which still means "nothing to compare
/// against, read everything" (a map from before this constant existed, or a
/// scanner build that never recorded a head). Without the distinction, a
/// repository with zero commits paid a full read on every single pass: `head`
/// is always absent there, so `plan` below saw `state.head.is_empty()`
/// forever and never let a later pass read only what changed since.
pub(crate) const NO_COMMIT_HEAD: &str = "no-commit";

/// Decide what this pass reads, given the previous map.
pub(crate) fn plan(root: &Path, prev: Option<&ProjectModel>) -> Plan {
    let Some(prev) = prev else {
        return Plan::Full;
    };
    let no_commit_before = prev.state.head == NO_COMMIT_HEAD;
    if prev.state.format != FORMAT || (prev.state.head.is_empty() && !no_commit_before) || prev.root != canonical(root)
    {
        return Plan::Full;
    }
    let now = head(root);
    // Sem commit então: não há um "de" válido para comparar (`changed_between`
    // exige duas revisões reais). Tudo que pode ter mudado já está coberto
    // pelo que está aberto agora, unido ao que já estava aberto na passada
    // anterior — o mesmo raciocínio do ramo comum, sem a metade comitada.
    if no_commit_before {
        let Some(open) = dirty(root) else {
            return Plan::Full;
        };
        return only_or_full(open.into_iter().chain(prev.state.dirty.iter().cloned()).collect());
    }
    let Some(now) = now else {
        return Plan::Full;
    };
    let Some(committed) = changed_between(root, &prev.state.head, &now) else {
        return Plan::Full;
    };
    let Some(open) = dirty(root) else {
        return Plan::Full;
    };
    let changed: BTreeSet<String> = committed.into_iter().chain(open).chain(prev.state.dirty.iter().cloned()).collect();
    only_or_full(changed)
}

/// `Plan::Only(changed)`, unless a file whose change alters how every other
/// file is classified is in it — then the whole project is read again.
fn only_or_full(changed: BTreeSet<String>) -> Plan {
    let global = changed.iter().any(|p| {
        let name = p.rsplit('/').next().unwrap_or(p);
        GLOBAL_INPUTS.contains(&name)
    });
    if global {
        Plan::Full
    } else {
        Plan::Only(changed)
    }
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
        &["log", "--no-merges", "--no-renames", "--relative", "--name-status", "--format=%x00%H %ct", &max, range],
    )?;
    Some(parse_log(&out))
}

/// Parse `git log --name-status --format=%x00%H %ct` into commits, oldest
/// first. A commit that touches nothing under the scanned root is left out.
pub(crate) fn parse_log(text: &str) -> Vec<RawCommit> {
    let mut commits = Vec::new();
    for block in text.split('\0').filter(|b| !b.trim().is_empty()) {
        let mut lines = block.lines();
        let Some(header) = lines.next() else {
            continue;
        };
        let mut parts = header.split_whitespace();
        let (Some(sha), Some(at)) = (parts.next(), parts.next()) else {
            continue;
        };
        let mut commit =
            RawCommit { id: sha.chars().take(10).collect(), at: at.parse().unwrap_or(0), ..RawCommit::default() };
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
        let text = "\0bbbbbbbbbbbbbbbb 20\n\nM\tsrc/a.rs\nD\tsrc/old.rs\nA\t\"src/with\\\"quote.rs\"\n\
                    \0cccccccccccc 15\n\n\
                    \0aaaaaaaaaaaaaaaa 10\n\nA\tsrc/a.rs\n";
        let commits = parse_log(text);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].id, "aaaaaaaaaa");
        assert_eq!(commits[0].added, vec!["src/a.rs".to_string()]);
        assert_eq!(commits[1].at, 20);
        assert_eq!(commits[1].changed, vec!["src/a.rs".to_string()]);
        assert_eq!(commits[1].added, vec!["src/with\"quote.rs".to_string()]);
    }

    #[test]
    fn without_a_previous_map_everything_is_read() {
        assert_eq!(plan(Path::new("."), None), Plan::Full);
        let other_build = ProjectModel {
            state: crate::model::ScanState { format: "0.0.0+old".to_string(), head: "x".to_string(), ..Default::default() },
            ..Default::default()
        };
        assert_eq!(plan(Path::new("."), Some(&other_build)), Plan::Full);
    }

    /// Um repositório sem nenhum commit não relê o projeto inteiro para
    /// sempre: a passada seguinte, ainda sem commit, lê só o que a anterior já
    /// tinha marcado como não comitado, pelo selo distinto do vazio que a
    /// passada sem commit grava.
    #[test]
    fn a_repository_with_no_commit_yet_is_not_read_in_full_forever() {
        let dir = std::env::temp_dir().join(format!("scan-refresh-no-commit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        let root = dir.as_path();
        let _ = git_exec::run(root, &["init", "-q"]);
        std::fs::write(root.join("a.rs"), "fn a() {}\n").expect("a.rs");
        std::fs::write(root.join("b.rs"), "fn b() {}\n").expect("b.rs");

        let prev = ProjectModel {
            root: canonical(root),
            state: crate::model::ScanState {
                format: FORMAT.to_string(),
                head: NO_COMMIT_HEAD.to_string(),
                dirty: vec!["a.rs".to_string(), "b.rs".to_string()],
                ..Default::default()
            },
            ..Default::default()
        };
        match plan(root, Some(&prev)) {
            Plan::Only(changed) => {
                assert_eq!(changed, BTreeSet::from(["a.rs".to_string(), "b.rs".to_string()]));
            }
            Plan::Full => panic!("sem commit também é um estado válido: devia ler só o não comitado, não tudo de novo"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
