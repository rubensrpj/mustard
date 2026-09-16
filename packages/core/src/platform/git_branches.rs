//! `git_branches` — the branches a repository REALLY has, and the one it
//! refuses to be committed on.
//!
//! ## Why this exists
//!
//! Both answers used to come from `mustard.json#git.flow`, a list written once
//! at `mustard init`. That made them as old as the install: a `release/2026-Q3`
//! cut last Tuesday was refused as "not an integration base" — a sentence about
//! a file, delivered as if it were a sentence about the repository. In a client
//! repository, where the operator does not own the branch convention, the only
//! offered way out was to edit the configuration for every project opened.
//!
//! So the two questions are separated here, and each is asked of the thing that
//! actually knows:
//!
//! | Question | Answered by | Shape |
//! |---|---|---|
//! | where may a unit be cut from? | [`branch_catalog`] — git, after a fetch | OPEN |
//! | does this branch still exist? | [`remote_branch_names`] — git, local refs | OPEN |
//! | where is a direct commit forbidden? | [`protected_branches`] — `git.flow` | CLOSED |
//!
//! The middle row is the same source as the first, narrowed and made free: a
//! base a unit was really cut from is checked for EXISTENCE, and the check is
//! asked often enough (once per cut, once per hook invocation) that it must not
//! touch the network.
//!
//! Opening the first is only safe because the second stays closed. They are one
//! module so that can never drift into two.
//!
//! ## Finding the default branch (measured, 2026-08-19)
//!
//! `git symbolic-ref refs/remotes/origin/HEAD` is the cheap local probe, and it
//! is NOT sufficient. Against real git:
//!
//! | clone | probe | result |
//! |---|---|---|
//! | ordinary `git clone` | `symbolic-ref` | `refs/remotes/origin/main`, exit 0 |
//! | `origin/HEAD` absent (shallow / CI) | `symbolic-ref` | **exit 128**, "not a symbolic ref" |
//! | `origin/HEAD` absent | `ls-remote --symref` | `ref: refs/heads/main HEAD`, exit 0 |
//!
//! A CI clone commonly has no `origin/HEAD` at all, so a single local probe
//! would answer "unknown" where the answer is still wanted. The ladder is
//! therefore local first (free) and remote second (one round trip, and the
//! caller is fetching anyway). There is no third rung: a name this repository
//! never stated is not an answer.
//!
//! ## Contracts honoured
//!
//! - **Nothing here errors and nothing here blocks.** No git, no repository, a
//!   failed invocation: each degrades to "not measured".
//! - **No branch name is written down here.** [`protected_branches`] answers
//!   from the project's own declaration alone; the other two ask git. A literal
//!   in this module is a sentence about somebody else's repository.
//! - No `unwrap`/`expect` outside tests; no `println!` — this is a library seam
//!   and callers render.

use std::collections::BTreeSet;
use std::path::Path;

use crate::domain::config::GitConfig;
use crate::platform::git;

/// One branch a unit could be cut from, as git reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchEntry {
    /// Short name, with no `origin/` prefix (`dev`, `release/2026-Q3`).
    pub name: String,
    /// Unix seconds of the branch tip's commit — what the catalog is ordered
    /// by, and what a picker shows so a stale branch is recognisable as one.
    pub committed_at: i64,
    /// `true` when this branch refuses a direct commit, so the picker can mark
    /// it without asking a second question.
    pub protected: bool,
    /// `true` when `git.flow` names it — the row a picker opens ON. A hint
    /// about where the cursor starts, never a restriction on the rest.
    pub preselected: bool,
}

/// Trimmed stdout of a git command in `root`, or `None` for every failure —
/// no git, not a repository, a non-zero exit.
fn git_out(root: &Path, args: &[&str]) -> Option<String> {
    git::run(root, args).out()
}

/// The branch the checkout is ON, or `None` when git could not answer (absent
/// binary, no repository) — and on a detached HEAD, where `--abbrev-ref` says
/// `HEAD` and there is no branch to name.
///
/// Reported, never enforced: the one caller is the install's stamp line, which
/// names where its commit landed so a commit on a protected branch is a stated
/// outcome instead of a silent one. Deciding anything on this value would be a
/// second branch policy beside [`protected_branches`], which is the module's
/// one.
#[must_use]
pub fn current_branch(root: &Path) -> Option<String> {
    let name = git_out(root, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    (!name.is_empty() && name != "HEAD").then_some(name)
}

/// The remote's default branch — `main`, `master`, `develop`, whatever THIS
/// repository declares — or `None` when git could not answer.
///
/// Two probes, in this order and for the reason in the module doc: the local
/// ref costs nothing and covers an ordinary clone; `ls-remote` costs one round
/// trip and covers the clone that has no `origin/HEAD`, which is the common CI
/// shape. Never a literal — a project whose default branch is `trunk` is
/// answered correctly by both probes and would be answered wrongly by any list.
#[must_use]
pub fn default_branch(root: &Path) -> Option<String> {
    if let Some(local) = git_out(root, &["symbolic-ref", "refs/remotes/origin/HEAD"])
        && let Some(name) = local.strip_prefix("refs/remotes/origin/")
            && !name.is_empty() {
                return Some(name.to_string());
            }
    let remote = git_out(root, &["ls-remote", "--symref", "origin", "HEAD"])?;
    remote.lines().find_map(|line| {
        let rest = line.strip_prefix("ref:")?;
        let refname = rest.split_whitespace().next()?;
        let name = refname.strip_prefix("refs/heads/")?;
        (!name.is_empty()).then(|| name.to_string())
    })
}

/// The branches that refuse a direct commit or merge.
///
/// **Everything here was DECLARED**: the bases of `git.flow`, plus whatever
/// `git.protected` adds. Nothing is probed and nothing is assumed — the answer
/// is the project's own statement about itself, and a project that states
/// nothing protects nothing.
///
/// That last sentence used to read the other way. Unmeasured degraded to a
/// hardcoded `{main, master}`, which is why a repository whose branches are
/// `develop` and `master` was protected on a name it does not promote through
/// while `develop` stayed open, and why a fresh install — which writes no flow
/// — protected two literals and nothing it actually has. The harness now says
/// what it knows: where the project has declared its bases, those refuse a
/// direct commit; where it has not, the door that needs the answer says so
/// instead of inventing one.
#[must_use]
pub fn protected_branches(config: &GitConfig) -> BTreeSet<String> {
    let mut out = config.declared_bases();
    out.extend(
        config
            .protected
            .iter()
            .map(|b| b.trim())
            .filter(|b| !b.is_empty())
            .map(str::to_string),
    );
    out
}

/// Every branch `origin` really has, as `(short name, tip unix seconds)`,
/// newest commit first — read from the LOCAL remote-tracking refs, with no
/// fetch and no round trip of any kind.
///
/// The module's ONE parse of `for-each-ref` output. [`branch_catalog`]
/// annotates what this returns and [`remote_branch_names`] narrows it to names,
/// so the two answers can never disagree about which refs count as branches.
///
/// `None` when git could not be asked — no git, not a repository, an
/// unreadable invocation. That is "not measured", and it is deliberately kept
/// apart from `Some(empty)`, which is a repository that answered and named
/// nothing.
fn origin_refs(root: &Path) -> Option<Vec<(String, i64)>> {
    let listing = git_out(
        root,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:short)\t%(committerdate:unix)",
            "refs/remotes/origin",
        ],
    )?;
    Some(
        listing
            .lines()
            .filter_map(|line| {
                let (raw, when) = line.split_once('\t')?;
                let name = raw.strip_prefix("origin/")?;
                // `origin/HEAD` is a pointer at another row, not a branch of
                // its own — listing it would offer the same branch twice under
                // two names.
                if name.is_empty() || name == "HEAD" {
                    return None;
                }
                Some((name.to_string(), when.trim().parse().unwrap_or(0)))
            })
            .collect(),
    )
}

/// The NAMES of every branch on `origin` — the reading for the one question
/// *"does this branch still exist?"*.
///
/// Free by construction: local refs only, no `fetch` and no `ls-remote`. That
/// is a requirement of the callers, not an optimisation — the recorded-base
/// check runs on the cut path AND inside a `PreToolUse` hook, so a round trip
/// here would be paid on every file write of every session. The refs are as
/// fresh as the last fetch, which is what the question needs: a branch deleted
/// on the remote weeks ago is already gone from them once anything has pruned.
///
/// `None` is *"could not measure"*, never *"there are none"* — see
/// [`origin_refs`]. A caller that folds the two together turns an offline
/// machine into a repository with no branches.
#[must_use]
pub fn remote_branch_names(root: &Path) -> Option<BTreeSet<String>> {
    Some(origin_refs(root)?.into_iter().map(|(name, _)| name).collect())
}

/// Every branch on `origin`, newest commit first, annotated with what a picker
/// needs to render a row.
///
/// `fetch` refreshes the remote-tracking refs first. It is a parameter and not
/// a constant because the two callers genuinely differ: opening a unit wants
/// today's truth and pays the round trip, while a gate that merely reports
/// wants an answer offline. A failed fetch is not an error — the catalog is
/// then simply as fresh as the last one.
///
/// Empty when git could not be asked. An empty catalog means "unmeasured", and
/// a caller must never read it as "this repository has no branches".
#[must_use]
pub fn branch_catalog(root: &Path, config: &GitConfig, fetch: bool) -> Vec<BranchEntry> {
    if fetch {
        let _ = git_out(root, &["fetch", "--prune", "origin"]);
    }
    let Some(refs) = origin_refs(root) else {
        return Vec::new();
    };
    let protected = protected_branches(config);
    let preselected = config.declared_bases();
    refs.into_iter()
        .map(|(name, committed_at)| BranchEntry {
            protected: protected.contains(&name),
            preselected: preselected.contains(&name),
            name,
            committed_at,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn git(root: &Path, args: &[&str]) -> bool {
        git::run(root, args).ok
    }

    /// An upstream with one commit on `trunk` — deliberately NOT `main`, so a
    /// test that passes by accident on a hardcoded name cannot.
    fn upstream(dir: &Path) -> bool {
        if !git(dir, &["init", "-q", "-b", "trunk", "."]) {
            return false;
        }
        let _ = git(dir, &["config", "user.email", "t@t.t"]);
        let _ = git(dir, &["config", "user.name", "t"]);
        git(dir, &["commit", "-q", "--allow-empty", "-m", "seed"])
    }

    #[test]
    fn the_default_branch_comes_from_the_repository_not_from_a_list() {
        let tmp = tempfile::tempdir().unwrap();
        let up = tmp.path().join("up");
        std::fs::create_dir_all(&up).unwrap();
        if !upstream(&up) {
            return; // no usable git here — every path degrades anyway.
        }
        let clone = tmp.path().join("clone");
        if !git(tmp.path(), &["clone", "-q", &up.to_string_lossy(), "clone"]) {
            return;
        }
        assert_eq!(
            default_branch(&clone).as_deref(),
            Some("trunk"),
            "the probe must report what this repository declares, not a literal",
        );
    }

    /// The CI shape the module doc measured: `origin/HEAD` absent makes the
    /// local probe exit 128, and the remote probe must carry it.
    #[test]
    fn a_clone_without_origin_head_still_resolves_through_the_remote() {
        let tmp = tempfile::tempdir().unwrap();
        let up = tmp.path().join("up");
        std::fs::create_dir_all(&up).unwrap();
        if !upstream(&up) {
            return;
        }
        let clone = tmp.path().join("clone");
        if !git(tmp.path(), &["clone", "-q", &up.to_string_lossy(), "clone"]) {
            return;
        }
        if !git(&clone, &["symbolic-ref", "-d", "refs/remotes/origin/HEAD"]) {
            return;
        }
        assert_eq!(
            default_branch(&clone).as_deref(),
            Some("trunk"),
            "the local probe fails here; the ls-remote fallback is what answers",
        );
    }

    /// Um projeto que não declara nada não protege nada: a proteção é a
    /// declaração do próprio projeto, nunca uma lista escrita no código.
    #[test]
    fn um_projeto_que_nao_declara_nada_nao_protege_nome_nenhum() {
        let protected = protected_branches(&GitConfig::default());
        assert!(
            protected.is_empty(),
            "sem `git.flow` e sem `git.protected` não há base declarada: {protected:?}",
        );
    }

    /// As bases do fluxo são protegidas, e a lista `git.protected` se soma a
    /// elas — nada além do que o projeto escreveu entra.
    #[test]
    fn a_protecao_sai_do_fluxo_e_da_lista_declarada() {
        let mut flow = BTreeMap::new();
        flow.insert("*".to_string(), "develop".to_string());
        flow.insert("develop".to_string(), "master".to_string());
        let config = GitConfig {
            flow,
            protected: vec!["release/2026-Q3".to_string()],
            ..GitConfig::default()
        };
        let protected = protected_branches(&config);
        assert!(protected.contains("develop"), "a base do fluxo é protegida: {protected:?}");
        assert!(protected.contains("master"), "e a de cima também: {protected:?}");
        assert!(protected.contains("release/2026-Q3"), "a lista declarada se soma");
        assert_eq!(protected.len(), 3, "e mais nada entra: {protected:?}");
    }

    /// The catalog reports branches that no `git.flow` ever declared — the
    /// whole point — and marks the two hints without turning them into rules.
    #[test]
    fn flow_preselects_but_never_restricts() {
        let tmp = tempfile::tempdir().unwrap();
        let up = tmp.path().join("up");
        std::fs::create_dir_all(&up).unwrap();
        if !upstream(&up) {
            return;
        }
        if !git(&up, &["checkout", "-q", "-b", "release/2026-Q3"]) {
            return;
        }
        let _ = git(&up, &["commit", "-q", "--allow-empty", "-m", "later"]);
        let _ = git(&up, &["checkout", "-q", "trunk"]);
        let clone = tmp.path().join("clone");
        if !git(tmp.path(), &["clone", "-q", &up.to_string_lossy(), "clone"]) {
            return;
        }

        let mut flow = BTreeMap::new();
        flow.insert("*".to_string(), "trunk".to_string());
        let config = GitConfig { flow, ..GitConfig::default() };
        let catalog = branch_catalog(&clone, &config, false);

        let names: Vec<&str> = catalog.iter().map(|b| b.name.as_str()).collect();
        assert!(
            names.contains(&"release/2026-Q3"),
            "a branch cut after the install is still a base: {names:?}",
        );
        assert!(!names.contains(&"HEAD"), "origin/HEAD is a pointer, not a row: {names:?}");
        assert_eq!(names.first(), Some(&"release/2026-Q3"), "newest commit first: {names:?}");

        let trunk = catalog.iter().find(|b| b.name == "trunk").expect("trunk listed");
        assert!(trunk.protected, "the remote default is marked");
        assert!(trunk.preselected, "and git.flow names it");
        let release =
            catalog.iter().find(|b| b.name == "release/2026-Q3").expect("release listed");
        assert!(!release.protected, "an ordinary branch is not protected");
        assert!(!release.preselected, "and being undeclared does not exclude it");
    }

    /// The existence probe answers about the REMOTE, and answers `None` where
    /// it could not look — the distinction its callers turn a recorded base on.
    #[test]
    fn the_name_probe_separates_absent_from_unmeasured() {
        assert_eq!(
            remote_branch_names(Path::new("/no/such/repository")),
            None,
            "a place git cannot answer for is UNMEASURED, never a branchless repository",
        );

        let tmp = tempfile::tempdir().unwrap();
        let up = tmp.path().join("up");
        std::fs::create_dir_all(&up).unwrap();
        if !upstream(&up) {
            return;
        }
        if !git(&up, &["checkout", "-q", "-b", "release/2026-Q3"]) {
            return;
        }
        let _ = git(&up, &["commit", "-q", "--allow-empty", "-m", "later"]);
        let _ = git(&up, &["checkout", "-q", "trunk"]);
        let clone = tmp.path().join("clone");
        if !git(tmp.path(), &["clone", "-q", &up.to_string_lossy(), "clone"]) {
            return;
        }

        let names = remote_branch_names(&clone).expect("a clone is measurable");
        assert!(names.contains("release/2026-Q3"), "a branch no config declares exists: {names:?}");
        assert!(names.contains("trunk"), "and so does the default one: {names:?}");
        assert!(!names.contains("HEAD"), "origin/HEAD is a pointer, not a branch: {names:?}");
        assert!(!names.contains("release/2025-Q1"), "one nobody cut does not: {names:?}");
    }
}
