//! `census_settlement` — THE question every door that is about to move a dirty
//! tree has to answer: asked once, answered in one place, and ACTED ON here
//! rather than by the door.
//!
//! ## The question, stated whole
//!
//! It has two inputs and exactly one answer.
//!
//! 1. **What is dirty** — harness scratch (dropped from the measurement),
//!    census artefacts (the tool's own output), the operator's work. Measured by
//!    [`checkout_work`], EXACTLY ONCE per settlement, and carried from there.
//! 2. **Where the checkout stands** ([`CheckoutPosition`]) — the branch the tree
//!    sits on, the branch about to be cut (none at the explicit open) and the
//!    resolved base.
//!
//! One answer, of two shapes ([`CensusSettlement`]):
//!
//! - **Refuse**, naming what is in the way and what unblocks it
//!   ([`RefusalCause`]).
//! - **Proceed**, with the base already refreshed from `origin` when there was
//!   a base to refresh.
//!
//! Nothing here writes a commit. The scan never writes to git, so the census is
//! nobody's work and there is nowhere to record it: a tree dirty only with it
//! ([`CheckoutWork::CensusOnly`]) never refuses anything. The census is still
//! told apart from the operator's work, so that their files are the only ones
//! that can refuse a move and the only ones a refusal names.
//!
//! ## The table
//!
//! Every combination of (what is dirty × the state of the index × the shape
//! of the root a door passed × where the checkout stands) has a row; a hole in
//! the table is a defect of the table, never a condition to add at a door. Two
//! of the four axes collapse at the entrance, before any row is taken, and the
//! collapse is stated here so nobody re-derives it at a door:
//!
//! - **root shape** — a door may pass the toplevel, a subdirectory, a linked
//!   worktree or a submodule. `settle` resolves `git rev-parse --show-toplevel`
//!   ONCE at entry and every git call below uses that root. Every row is
//!   therefore written for ONE shape; a door cannot get it wrong because a door
//!   no longer chooses. (A submodule keeps its own toplevel: it is a repository
//!   of its own.)
//! - **index state** — a dirty path may be unstaged (` M`), staged (`M `,
//!   `A `), both (`MM`) or untracked (`??`). [`checkout_work`] accepts all four
//!   into the same reading, and the one step that touches those paths — the
//!   set-aside — is a `stash push --include-untracked` restricted to them,
//!   which holds all four states alike. No row below reads the index state
//!   again: there is no state a row handles differently.
//!
//! The rows, in the order the body takes them:
//!
//! | dirty         | position                              | row |
//! |---------------|---------------------------------------|-----|
//! | any           | (vcs opted out)                       | Proceed, nothing measured |
//! | `Holds`/`Unproven` | another unit's branch, cutting   | **Refuse** — work would travel |
//! | `Holds`/`Unproven` | the base, protected, cutting     | fall through: their work rides into the unit, by design (the first unit cuts off the base in place) |
//! | `Holds`/`Unproven` | any, not cutting                 | fall through: nothing moves |
//! | `ProvenClean`/`CensusOnly` | any                      | fall through: nothing of anyone's can travel |
//! | `Holds`       | the base, the advance overwrites THEIR paths | **Refuse** — names their files, prescribes the stash; nothing touched |
//! | `CensusOnly`/`Holds` | the base, the advance overwrites census paths | set those paths aside — stashed, never discarded — then advance |
//! | any (fell through) | base known, `origin` answers, base cannot be advanced | **Refuse** — stale base, git's words; what was set aside is put back first, index state included |
//! | (set aside)   | the base, advanced                    | account for each path: miner output → `origin`'s stands; authored mold → kept BESIDE `origin`'s; then drop the stash |
//! | any           | anything else                         | Proceed |
//!
//! Every row that touches the tree has a rollback: the set-aside is the only
//! one, and its rollback is the put-back on the refusal row that follows it.
//!
//! ## Why this is ONE function and not a condition at each door
//!
//! Two doors take this decision: the explicit open and
//! the cut that follows the spec's approval. While each carried a condition of its own, the next
//! review always found the door that had missed one, or that took the steps in
//! another order. So the doors stopped deciding AND stopped acting: a door
//! states where the checkout stands and obeys the answer, and the base refresh
//! happens HERE, once, in the order this body states. The doors differ only in
//! the position they state: the explicit open cuts nothing (no target).
//!
//! ## Fail-open where nothing was measured
//!
//! An unreachable remote, a tree git cannot place: each answers `Proceed`.
//! Only a POSITIVE observation refuses: somebody's work that would travel, or a
//! base that `origin` proved stale and git could not advance.

use std::path::Path;

use mustard_core::ProjectConfig;

use super::work_branch::{
    checkout_work, fast_forward_base, fetch_origin, holds_other_work,
    paths_the_advance_overwrites, toplevel_of, BaseRefresh, BusyCheckout, CensusSetAside,
    CheckoutWork, RefusalCause,
};

/// WHERE THE CHECKOUT STANDS — the second input, stated by the door and never
/// re-derived here.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CheckoutPosition<'a> {
    /// The branch the tree really sits on — `None` on a detached HEAD or a
    /// probe that did not answer. Used both to judge whose work the tree holds
    /// and to drive the git steps.
    current: Option<&'a str>,
    /// The branch about to be cut, when one is. `None` at a door that cuts
    /// nothing — the explicit open — where no work can ride anywhere and so
    /// nothing in the TREE is ever refused.
    target: Option<&'a str>,
    /// The base this open or cut resolved to. `None` when the door could not
    /// establish it: a base nobody knows cannot be refreshed, and nothing is
    /// going to move from it either.
    base: Option<&'a str>,
}

impl<'a> CheckoutPosition<'a> {
    /// The ordinary position: where the tree sits, what is about to be cut (if
    /// anything), and the base that was resolved for it.
    pub(crate) fn at(
        current: Option<&'a str>,
        target: Option<&'a str>,
        base: Option<&'a str>,
    ) -> Self {
        Self { current, target, base }
    }

    /// `true` when taking this checkout would carry work that is not this
    /// unit's onto the branch about to be cut — the plain `git checkout -b`
    /// this settlement stands in front of moves everything uncommitted with it.
    ///
    /// `false` wherever nothing is going to be checked out (no target).
    fn would_carry_work_off(&self, root: &Path, config: &ProjectConfig) -> bool {
        self.target.is_some_and(|target| holds_other_work(root, self.current, target, config))
    }
}

/// THE ANSWER — two shapes and no more. Everything the answer describes has
/// ALREADY HAPPENED when it is returned; the caller only obeys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CensusSettlement {
    /// Do NOT proceed. Something is in the way of this move, and the refusal
    /// carries the measured paths and the cause so each door can say the same
    /// sentence in its own shape ([`BusyCheckout::reason`]).
    ///
    /// A refusal for the TREE (another unit's work that would travel) happens
    /// before any fetch: the tree is left exactly as it was found. A refusal
    /// for the BASE necessarily comes after the fetch that proved it stale, and
    /// it leaves the tree exactly as it was found too: the operator's files in
    /// the advance's way ([`RefusalCause::BaseBlockedByWork`]) are refused
    /// before any path is set aside, and a fast-forward that still fails
    /// ([`RefusalCause::BaseStale`]) puts back — index state included — the
    /// census paths that had been set aside for it.
    Refuse(BusyCheckout),
    /// Proceed. The base was refreshed from `origin` when there was a base to
    /// refresh and a remote that answered; nothing else was owed.
    Proceed,
}

/// Settle the question for one position and one tree.
///
/// The whole decision AND the whole effect. In order, and the order is the
/// point:
///
/// 0. **Resolve the root, once** ([`toplevel_of`]). Whatever a door passed —
///    the project root, the edited file's directory, a worktree — every git
///    call below runs from the repository's toplevel, because the paths git
///    ANSWERS (`status`, `diff --name-only`) are toplevel-relative and the
///    pathspecs it is HANDED (`stash push -- p`) are CWD-relative. From any
///    other directory the two disagree in silence. Resolving the root is not a
///    measurement of the tree: step 1 is still the only one.
/// 1. **Measure the tree, once** ([`checkout_work`]). Every later question
///    reads this one value; nothing measures again.
/// 2. **Refuse for the tree**: work that is not this unit's would ride along.
///    Refusing FIRST is what keeps a refused move from leaving a fetch or an
///    advance behind it.
/// 3. **Fetch `origin`** — the measurement the next two steps read. Offline,
///    nothing below happens and the cut takes the local base, as it always did.
/// 4. **Set the census aside where it stands in the way of the advance** —
///    and refuse, before touching anything, where the OPERATOR's files do.
///    Standing on the base with dirty paths that `origin`'s advance
///    overwrites, the fast-forward refuses on them ("local changes would be
///    overwritten"). Two kinds of path can stand in the way, and they get
///    different sentences:
///    - THEIRS (a [`CheckoutWork::Holds`] tree on the base — the first unit
///      cutting off the base in place, by design): the move is refused naming
///      those files and prescribing the stash that unblocks the advance
///      ([`RefusalCause::BaseBlockedByWork`]). Not a `git pull`, which fails
///      on the same files; and nothing was touched, so there is nothing to
///      undo.
///    - THE CENSUS (in a `CensusOnly` tree, or beside their work in a `Holds`
///      one — it is the tool's whatever else is dirty): those paths, and only
///      those, are SET ASIDE with a stash restricted to them
///      ([`CensusSetAside::push`]) — staged, unstaged and untracked alike.
///      Never discarded: an authored mold is not regenerable, and a base that
///      then fails to advance owes the tree back exactly as it was.
/// 5. **Fast-forward the base** — THE base this settlement is about, and no
///    other ref ([`fast_forward_base`]). If it still trails `origin` afterwards,
///    **put back what step 4 set aside, then refuse, loudly**, with git's
///    words: never swallowed, never a cut from a stale base. It is the
///    invariant the root `CLAUDE.md` states in its own words: `--ff-only` only
///    passes while the integration base carries no commit of its own. When it
///    advanced, what was set aside is accounted for path by path
///    ([`CensusSetAside::settle_after_advance`]): miner output yields to
///    `origin`'s version, an authored mold is kept beside `origin`'s and said
///    so — nobody's text is deleted.
///
/// The caller does none of those steps and cannot reorder them.
pub(crate) fn settle(
    root: &Path,
    position: CheckoutPosition<'_>,
    config: &ProjectConfig,
) -> CensusSettlement {
    // An explicit `vcs: ""` opt-out (or a tree git does not manage) has no
    // base to refresh and nothing to refuse.
    if config.vcs().is_none() {
        return CensusSettlement::Proceed;
    }
    // 0. THE ROOT — the repository's toplevel, whatever the door passed. A
    //    tree git cannot place keeps the door's root: the measurement below
    //    then answers `Unproven`, which authorises nothing.
    let toplevel = toplevel_of(root);
    let root: &Path = toplevel.as_deref().unwrap_or(root);
    let base = position.base.map(str::trim).filter(|b| !b.is_empty());

    // 1. WHAT IS DIRTY — measured here and NOWHERE else in this settlement.
    let work = checkout_work(root);

    // 2. REFUSE FOR THE TREE. Only the OPERATOR's work refuses (or a tree that
    //    could not be measured — an unmeasured tree is not an empty one, and
    //    reading it as empty is how another unit's work rides off in silence),
    //    and only where it is another unit's, i.e. where `holds_other_work`
    //    says so. Riding off a protected base into the first unit is by
    //    design. The census is nobody's work and never refuses.
    let refusal = match &work {
        CheckoutWork::ProvenClean | CheckoutWork::CensusOnly(_) => None,
        CheckoutWork::Holds { .. } | CheckoutWork::Unproven => position
            .would_carry_work_off(root, config)
            .then_some(RefusalCause::WorkWouldTravel),
    };
    if let Some(cause) = refusal {
        return CensusSettlement::Refuse(BusyCheckout {
            current: position.current.unwrap_or("HEAD").to_string(),
            target: position.target.unwrap_or_default().to_string(),
            work,
            cause,
        });
    }

    // 3–5. THE BASE — fetched, with the tool's own output moved out of the
    //      advance's way where it stands in it, and fast-forwarded. A base
    //      nobody established cannot be refreshed; offline, nothing can be
    //      measured and the local base is taken as before.
    if let Some(base) = base
        && fetch_origin(root) {
            let mut aside = CensusSetAside::none();
            // 4. SET ASIDE — only on the base, only the paths the advance
            //    overwrites, and only when the advance IS a fast-forward (a
            //    diverged base refuses below without a single path touched).
            //    Their files first, because those refuse before anything is
            //    set aside; then the census, in whichever reading it came.
            if position.current == Some(base) {
                let (theirs, census): (&[String], &[String]) = match &work {
                    CheckoutWork::Holds { theirs, census } => (theirs, census),
                    CheckoutWork::CensusOnly(census) => (&[], census),
                    CheckoutWork::ProvenClean | CheckoutWork::Unproven => (&[], &[]),
                };
                let blocking = paths_the_advance_overwrites(root, base, theirs);
                if !blocking.is_empty() {
                    return CensusSettlement::Refuse(BusyCheckout {
                        current: position.current.unwrap_or("HEAD").to_string(),
                        target: position.target.unwrap_or_default().to_string(),
                        work,
                        cause: RefusalCause::BaseBlockedByWork {
                            base: base.to_string(),
                            paths: blocking,
                        },
                    });
                }
                let overwritten = paths_the_advance_overwrites(root, base, census);
                aside = CensusSetAside::push(root, base, &overwritten);
                if !aside.paths().is_empty() {
                    eprintln!(
                        "base-gate: census output set aside (stashed) so '{base}' can advance \
                         to origin/{base} — {}",
                        aside.paths().join(", ")
                    );
                }
            }
            // 5. FAST-FORWARD, and READ the answer. A refusal puts back what
            //    was set aside FIRST: the tree is returned exactly as found.
            match fast_forward_base(root, position.current, base) {
                BaseRefresh::Stale { base, error } => {
                    aside.put_back(root);
                    return CensusSettlement::Refuse(BusyCheckout {
                        current: position.current.unwrap_or("HEAD").to_string(),
                        target: position.target.unwrap_or_default().to_string(),
                        work,
                        cause: RefusalCause::BaseStale { base, error },
                    });
                }
                BaseRefresh::Current => {
                    let report = aside.settle_after_advance(root);
                    if !report.origin_kept.is_empty() {
                        eprintln!(
                            "base-gate: miner output rewritten on origin/{base} — origin's \
                             version stands, the local one was regenerable: {}",
                            report.origin_kept.join(", ")
                        );
                    }
                    for (path, sibling) in &report.kept_both {
                        eprintln!(
                            "base-gate: authored mold {path} was rewritten on origin/{base} \
                             too — origin's text is at {path}, the text found here is kept \
                             at {sibling}; both survive, reconcile them by hand"
                        );
                    }
                }
            }
        }

    CensusSettlement::Proceed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::git_settle::git_out;
    use mustard_core::io::project_map;
    use std::process::Command;

    /// A pergunta inteira, feita como a porta de CORTE a faz.
    ///
    /// As fixtures deste módulo medem pelo MESMO ponto de entrada que o produto
    /// usa, e não por uma metade dele.
    fn settle_cut(
        root: &Path,
        current: Option<&str>,
        target: &str,
        base: Option<&str>,
        config: &ProjectConfig,
    ) -> CensusSettlement {
        settle(root, CheckoutPosition::at(current, Some(target), base), config)
    }

    /// …e como a porta EXPLÍCITA de abertura a faz: sem alvo, porque ali
    /// nada é checado out e portanto nada pode viajar.
    fn settle_open(
        root: &Path,
        current: Option<&str>,
        base: Option<&str>,
        config: &ProjectConfig,
    ) -> CensusSettlement {
        settle(root, CheckoutPosition::at(current, None, base), config)
    }

    /// Run a git command in `root`, asserting success — test scaffolding only.
    fn git(root: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(ok, "git {args:?} failed");
    }

    /// A `dev`/`main` project config — the base set is derived, never hardcoded.
    fn flow_config() -> ProjectConfig {
        let mut config = ProjectConfig::default();
        config.git.flow.insert("*".to_string(), "dev".to_string());
        config.git.flow.insert("dev".to_string(), "main".to_string());
        config
    }

    /// Init a repo whose single commit lives on `base`.
    ///
    /// The line-ending config is not cosmetic. These fixtures assert the BYTES
    /// git puts back on disk after a `reset --hard`, a fast-forward or a stash
    /// pop, and the Windows runner carries `core.autocrlf=true` globally — so
    /// the same commit checks out with CRLF there and every byte comparison
    /// fails while the content is identical. Pinning both keys makes the
    /// fixture answer the same on every platform. Writing git config is
    /// confined to `#[cfg(test)]` by the root `CLAUDE.md` guard; this is that
    /// carve-out, not an exception to it.
    fn init_repo_on(root: &Path, base: &str) {
        crate::shared::test_fixture::repo_from_template(root, &format!("census.init_repo_on:{base}"), |root| {
            git(root, &["init"]);
            git(root, &["config", "core.autocrlf", "false"]);
            git(root, &["config", "core.eol", "lf"]);
            git(root, &["config", "user.email", "t@example.com"]);
            git(root, &["config", "user.name", "t"]);
            git(root, &["checkout", "-b", base]);
            std::fs::write(root.join("f.txt"), "hi").unwrap();
            git(root, &["add", "."]);
            git(root, &["commit", "-m", "init"]);
        });
    }

    /// `git status --porcelain` for `root` — the tree as the NEXT command's
    /// clean-tree guard will read it.
    fn porcelain(root: &Path) -> String {
        let out = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(root)
            .output()
            .expect("git status");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A repo on `dev` whose project map (`.claude/grain.db`) is TRACKED and
    /// committed — the shape where a re-mined census shows up as a dirty tree
    /// at all.
    fn repo_tracking_the_census(root: &Path) {
        init_repo_on(root, "dev");
        project_map::write_text(root, "{\"projects\":[]}\n").unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "track the census"]);
        assert_eq!(porcelain(root), "", "the fixture must start clean");
    }

    /// What a scan writes: the model of the tree at `root`.
    fn remine(root: &Path) {
        project_map::write_text(root, "{\"projects\":[{\"dir\":\"apps/rt\"}]}\n").unwrap();
    }

    /// Deixa na árvore, e só na árvore, a saída da passagem de ENRIQUECIMENTO —
    /// o mapa de um subprojeto e um molde `{papel}-pattern`, que o mine
    /// determinístico não escreve.
    fn leftover_enrichment(root: &Path) {
        let claude = root.join("apps").join("rt").join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(claude.join("scan-map.md"), "Tipo: cargo · 307 arquivos\n").unwrap();
        let mold = claude.join("skills").join("rt-gate-pattern");
        std::fs::create_dir_all(&mold).unwrap();
        // `source: scan` é o ÚNICO marcador que declara o molde como saída da
        // ferramenta — a regra canônica de `scan_patterns::origin`, que é
        // também a que a passagem de enriquecimento carimba em tudo que escreve.
        std::fs::write(
            mold.join("SKILL.md"),
            "---\nname: rt-gate-pattern\nsource: scan\n---\n",
        )
        .unwrap();
    }

    /// A abertura ORDINÁRIA: o operador parado NA base, a árvore suja só com o
    /// censo, e o corte da próxima unidade NÃO é recusado — e nenhum commit é
    /// criado. O censo não é trabalho de ninguém e não entra no git: ele segue
    /// sujo para a branch nova, sem ser gravado.
    ///
    /// Medido pela porta REAL (`cut_pending_work_branch`).
    #[test]
    fn a_census_only_dirty_tree_is_cut_without_a_commit() {
        use crate::commands::event::work_branch::{cut_pending_work_branch, CutOutcome};

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_string_lossy().to_string();
        // Escrito ANTES do `git init` da fixture, para entrar no commit inicial:
        // um `mustard.json` solto seria trabalho do operador na árvore.
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#,
        )
        .unwrap();
        // A árvore fica em `dev`, que é a base de onde `dev_second` sai.
        repo_tracking_the_census(root);

        remine(root);
        leftover_enrichment(root);
        assert_ne!(porcelain(root), "", "a passagem de enriquecimento sujou a árvore");

        let commits_before = git_out(root, &["rev-list", "--count", "HEAD"]).expect("count");

        // A porta real, e só ela: nada disso é trabalho de ninguém, então o
        // corte acontece de verdade.
        let sid = "sess-census-only-open";
        crate::shared::context::pending_branch::set_pending_branch(&root_s, sid, "dev_second", None);
        let outcome = cut_pending_work_branch(root, sid);
        assert_eq!(
            outcome,
            CutOutcome::Cut("dev_second".to_string()),
            "a abertura ordinária não é recusada: {outcome:?}",
        );
        assert_eq!(
            git_out(root, &["rev-list", "--count", "HEAD"]).expect("count"),
            commits_before,
            "o corte não cria commit nenhum",
        );
        assert!(
            matches!(checkout_work(root), CheckoutWork::CensusOnly(_)),
            "e o censo segue sujo na branch nova, sem ser gravado",
        );
    }

    /// A base é ATUALIZADA a partir do `origin` com a árvore suja só com o
    /// censo, e nenhum commit é escrito nela: depois do corte a base local é
    /// exatamente a do `origin`. É a invariante que o Guard do `CLAUDE.md` da
    /// raiz enuncia: `--ff-only` só passa enquanto a base de integração não
    /// tem commit próprio, e o `git pull --ff-only origin {base}` que a recusa
    /// deste portão prescreve continua passando.
    ///
    /// O commit do `origin` é VAZIO de propósito: o avanço não depende da
    /// árvore suja. O caso em que o commit do `origin` TOCA o censo sujo é
    /// medido à parte, em
    /// `a_census_in_the_way_of_the_advance_is_set_aside_and_nothing_is_committed`.
    #[test]
    fn the_base_advances_under_a_census_only_tree_without_a_commit() {
        use crate::commands::event::work_branch::{cut_pending_work_branch, CutOutcome};

        let tmp = tempfile::tempdir().unwrap();
        // A árvore e o `origin` vivem LADO A LADO: um repositório DENTRO da
        // árvore seria trabalho não versionado do operador, e o corte seria
        // recusado por isso em vez de medir o que este teste mede.
        let root = tmp.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.as_path();
        let root_s = root.to_string_lossy().to_string();
        let origin = tmp.path().join("origin.git");
        let origin_s = origin.to_string_lossy().to_string();
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#,
        )
        .unwrap();
        repo_tracking_the_census(root);

        // Um `origin` cuja `dev` está UM commit à frente da base local. O commit
        // é VAZIO de propósito: assim o fast-forward não depende da árvore suja.
        git(root, &["init", "--bare", "-q", &origin_s]);
        git(root, &["remote", "add", "origin", &origin_s]);
        git(root, &["push", "-q", "origin", "dev"]);
        git(root, &["commit", "-q", "--allow-empty", "-m", "origin moved"]);
        git(root, &["push", "-q", "origin", "dev"]);
        let ahead = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");
        git(root, &["reset", "-q", "--hard", "HEAD~1"]);
        assert!(
            !git_out(root, &["rev-list", "dev"]).expect("rev-list").contains(&ahead),
            "a fixture tem de começar com a base ATRÁS do origin",
        );

        // A abertura ordinária: a árvore suja só com o censo.
        remine(root);
        leftover_enrichment(root);
        assert_ne!(porcelain(root), "", "a passagem de enriquecimento sujou a árvore");

        let sid = "sess-stale-base";
        crate::shared::context::pending_branch::set_pending_branch(&root_s, sid, "dev_second", None);
        let outcome = cut_pending_work_branch(root, sid);
        assert_eq!(
            outcome,
            CutOutcome::Cut("dev_second".to_string()),
            "o corte tem de acontecer: {outcome:?}",
        );

        assert_eq!(
            git_out(root, &["rev-parse", "dev"]).expect("dev"),
            ahead,
            "a base é exatamente a do origin: avançou, e nada foi commitado nela",
        );
        assert!(
            matches!(checkout_work(root), CheckoutWork::CensusOnly(_)),
            "e o censo segue sujo, sem ser gravado",
        );
    }

    /// Um corte RECUSADO por base desconhecida não deixa nada para trás. Com
    /// vários candidatos declarados e nada dizendo de qual base a emergência
    /// saiu, o corte devolve `BaseUnknown` e não toca no git: nenhum commit,
    /// nenhuma branch, a árvore como estava.
    #[test]
    fn a_cut_denied_for_an_unknown_base_touches_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_string_lossy().to_string();
        // Dois candidatos declarados e nenhum registro: `hotfix/…` não tem base
        // derivável, então a resolução responde `Ambiguous`.
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#,
        )
        .unwrap();
        repo_tracking_the_census(root);

        remine(root);
        leftover_enrichment(root);
        assert_ne!(porcelain(root), "", "a passagem de enriquecimento sujou a árvore");
        let head_before = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");

        let sid = "sess-base-unknown";
        crate::shared::context::pending_branch::set_pending_branch(&root_s, sid, "hotfix/urgente", None);
        // Amostrado DEPOIS do marcador, que também escreve na árvore: o que
        // este teste mede é o que o corte faz, não o que o marcador fez.
        let dirty_before = porcelain(root);
        let outcome = crate::commands::event::work_branch::cut_pending_work_branch(root, sid);
        assert!(
            matches!(
                outcome,
                crate::commands::event::work_branch::CutOutcome::BaseUnknown { .. }
            ),
            "a base não foi estabelecida, então nada é cortado: {outcome:?}",
        );
        assert_eq!(
            git_out(root, &["rev-parse", "HEAD"]).expect("HEAD"),
            head_before,
            "e nenhum commit fica para trás de um corte que não houve",
        );
        assert_eq!(
            porcelain(root),
            dirty_before,
            "a árvore fica exatamente como estava, para o corte que vier de fato",
        );
        assert!(
            git_out(root, &["rev-parse", "--verify", "hotfix/urgente"]).is_none(),
            "e nenhuma branch foi criada",
        );
    }

    /// Numa base PROTEGIDA, com a árvore suja só com o censo, o corte segue e
    /// não cria commit: o censo não é trabalho de ninguém, então não há o que
    /// recusar nem onde gravá-lo. A posição NÃO MEDIDA (`HEAD` destacado, ou
    /// ilegível) responde igual.
    #[test]
    fn a_census_only_tree_on_a_protected_base_proceeds_without_a_commit() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // `git.protected` nomeia a branch em que a árvore está parada.
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev"},"protected":["dev"]}}"#,
        )
        .unwrap();
        repo_tracking_the_census(root);

        remine(root);
        leftover_enrichment(root);
        assert_ne!(porcelain(root), "", "a passagem de enriquecimento sujou a árvore");
        let head_before = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");

        let config = ProjectConfig::load(root);
        assert!(
            mustard_core::protected_branches(&config.git).contains("dev"),
            "a fixture precisa de uma posição realmente protegida",
        );
        let dirty_before = porcelain(root);
        for current in [Some("dev"), Some("HEAD"), None] {
            let settled = settle_cut(root, current, "dev_second", Some("dev"), &config);
            assert_eq!(
                settled,
                CensusSettlement::Proceed,
                "posição {current:?}: o censo sozinho não recusa nada",
            );
        }
        assert_eq!(
            git_out(root, &["rev-parse", "HEAD"]).expect("HEAD"),
            head_before,
            "nenhum commit foi criado",
        );
        assert_eq!(porcelain(root), dirty_before, "e a árvore fica exatamente como estava");
    }

    /// Monta a árvore e um `origin` LADO A LADO, com a `dev` local UM commit
    /// atrás do `origin` — e o commit do `origin` TOCANDO o modelo do censo,
    /// que é o caso que o commit vazio da fixture irmã contorna. Devolve o
    /// commit à frente e o conteúdo que o `origin` tem para o modelo.
    fn origin_ahead_touching_the_census(root: &Path) -> (String, Vec<u8>) {
        let origin = root.parent().expect("tmp").join("origin.git");
        let origin_s = origin.to_string_lossy().to_string();
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#,
        )
        .unwrap();
        repo_tracking_the_census(root);
        git(root, &["init", "--bare", "-q", &origin_s]);
        git(root, &["remote", "add", "origin", &origin_s]);
        git(root, &["push", "-q", "origin", "dev"]);
        // A máquina A re-minerou e publicou.
        const ORIGINS_CENSUS: &str = "{\"projects\":[{\"dir\":\"apps/rt\"},{\"dir\":\"apps/cli\"}]}\n";
        project_map::write_text(root, ORIGINS_CENSUS).unwrap();
        // O mapa é um banco: o que se compara são os bytes do arquivo.
        let origins_census = std::fs::read(project_map::model_path(root)).unwrap();
        git(root, &["commit", "-q", "-am", "another machine re-mined the census"]);
        git(root, &["push", "-q", "origin", "dev"]);
        let ahead = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");
        // A máquina B ainda não puxou.
        git(root, &["reset", "-q", "--hard", "HEAD~1"]);
        assert!(
            !git_out(root, &["rev-list", "dev"]).expect("rev-list").contains(&ahead),
            "a fixture tem de começar com a base ATRÁS do origin",
        );
        (ahead, origins_census)
    }

    /// O censo sujo no caminho do avanço — o modelo que o `origin` também
    /// reescreveu — é posto de lado, a base avança, e nada é commitado: a base
    /// local fica exatamente a do `origin`, e o `git pull --ff-only origin dev`
    /// que o portão prescreve continua passando. O modelo é o do `origin`, e o
    /// resto do censo segue sujo, sem ser gravado.
    #[test]
    fn a_census_in_the_way_of_the_advance_is_set_aside_and_nothing_is_committed() {
        use crate::commands::event::work_branch::{cut_pending_work_branch, CutOutcome};

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.as_path();
        let root_s = root.to_string_lossy().to_string();
        let (ahead, origins_census) = origin_ahead_touching_the_census(root);

        // A máquina B com o censo sujo — o modelo INCLUSIVE, que é o arquivo
        // que o avanço sobrescreve.
        remine(root);
        leftover_enrichment(root);
        assert!(
            matches!(checkout_work(root), CheckoutWork::CensusOnly(_)),
            "precondição: só o censo está sujo",
        );

        let sid = "sess-census-in-the-way";
        crate::shared::context::pending_branch::set_pending_branch(&root_s, sid, "dev_second", None);
        let outcome = cut_pending_work_branch(root, sid);
        assert_eq!(outcome, CutOutcome::Cut("dev_second".to_string()), "{outcome:?}");

        assert!(
            git_out(root, &["rev-list", "dev"]).expect("rev-list").contains(&ahead),
            "a base avançou até o origin: o censo no caminho não a prendeu",
        );
        assert_eq!(
            std::fs::read(project_map::model_path(root)).unwrap(),
            origins_census,
            "o modelo é o do origin — o local velho foi posto de lado, não gravado por cima",
        );
        assert_eq!(
            git_out(root, &["rev-parse", "dev"]).expect("dev"),
            ahead,
            "a base é exatamente a do origin — nenhum commit foi escrito nela",
        );
        assert!(
            matches!(checkout_work(root), CheckoutWork::CensusOnly(_)),
            "e o resto do censo segue sujo, sem ser gravado",
        );
    }

    /// …e quando o avanço NÃO tem como passar — a base local divergiu —, a
    /// resposta é RECUSAR, alto, com as palavras do git: nunca engolir e nunca
    /// cortar de uma base velha. E a recusa vem ANTES de qualquer ação: nada
    /// posto de lado, a árvore como estava.
    #[test]
    fn a_base_that_cannot_advance_refuses_loudly_instead_of_cutting_stale() {
        use crate::commands::event::work_branch::RefusalCause;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.as_path();
        let (ahead, _) = origin_ahead_touching_the_census(root);
        // A `dev` local com um commit PRÓPRIO: divergiu do origin.
        git(root, &["commit", "-q", "--allow-empty", "-m", "a commit of its own"]);

        remine(root);
        leftover_enrichment(root);
        let head_before = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");
        let dirty_before = porcelain(root);

        let settled = settle_cut(root, Some("dev"), "dev_second", Some("dev"), &flow_config());
        let CensusSettlement::Refuse(busy) = settled else {
            panic!("uma base que não avança não recebe corte nem commit: {settled:?}");
        };
        let RefusalCause::BaseStale { base, error } = &busy.cause else {
            panic!("a causa é a base, não a árvore: {:?}", busy.cause);
        };
        assert_eq!(base, "dev");
        assert!(!error.is_empty(), "as palavras do git viajam na recusa");
        let reason = busy.reason(mustard_core::platform::i18n::Locale::EnUs);
        assert!(reason.contains("origin/dev"), "a frase nomeia o remoto: {reason}");

        assert_eq!(git_out(root, &["rev-parse", "HEAD"]).expect("HEAD"), head_before);
        assert!(
            !git_out(root, &["rev-list", "dev"]).expect("rev-list").contains(&ahead),
            "a base não foi rebobinada nem mesclada",
        );
        assert_eq!(porcelain(root), dirty_before, "nada foi posto de lado antes de recusar");
    }

    /// A porta EXPLÍCITA só move a base sobre a qual abre — e nenhuma outra.
    ///
    /// Um passo antigo avançava TODA base pré-selecionada do fluxo
    /// (`fetch origin main:main`, `release/*`…), atrás do operador. Mover outras
    /// refs locais nunca foi trabalho desta decisão.
    #[test]
    fn the_explicit_open_advances_the_base_it_opens_on_and_no_other_ref() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.as_path();
        let origin = tmp.path().join("origin.git");
        let origin_s = origin.to_string_lossy().to_string();
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#,
        )
        .unwrap();
        repo_tracking_the_census(root);
        // `main` local, parada no commit inicial.
        git(root, &["branch", "main"]);
        let main_before = git_out(root, &["rev-parse", "main"]).expect("main");
        git(root, &["init", "--bare", "-q", &origin_s]);
        git(root, &["remote", "add", "origin", &origin_s]);
        git(root, &["push", "-q", "origin", "dev", "main"]);
        // O origin avança AS DUAS; a local só a `dev` vai puxar.
        git(root, &["commit", "-q", "--allow-empty", "-m", "moved"]);
        git(root, &["push", "-q", "origin", "dev", "dev:main"]);
        let ahead = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");
        git(root, &["reset", "-q", "--hard", "HEAD~1"]);

        let config = ProjectConfig::load(root);
        assert!(
            config.git.declared_bases().contains("main"),
            "a fixture precisa de uma base pré-selecionada que NÃO é a desta abertura",
        );
        let _ = settle_open(root, Some("dev"), Some("dev"), &config);
        assert!(
            git_out(root, &["rev-list", "dev"]).expect("rev-list").contains(&ahead),
            "a base desta abertura avançou",
        );
        assert_eq!(
            git_out(root, &["rev-parse", "main"]).expect("main"),
            main_before,
            "e a `main` local, que o operador não mencionou, não se mexeu",
        );
    }

    /// A árvore que TODAS as portas recebem neste módulo: o censo re-minerado
    /// e a saída da passagem de enriquecimento, e mais nada do operador.
    /// `stand_on` põe a árvore fora da base quando é `Some`.
    fn a_tree_dirty_only_with_the_census(root: &Path, stand_on: Option<&str>) {
        // Escrito ANTES do `git init` da fixture, para entrar no commit inicial:
        // um `mustard.json` solto seria trabalho do operador na árvore.
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#,
        )
        .unwrap();
        repo_tracking_the_census(root);
        if let Some(branch) = stand_on {
            git(root, &["checkout", "-b", branch]);
        }
        remine(root);
        leftover_enrichment(root);
        assert_ne!(porcelain(root), "", "a passagem de enriquecimento sujou a árvore");
    }

    /// Um molde ADOTADO (`source: manual`) é escrita do OPERADOR, e o caminho
    /// dele é igualzinho ao de um molde gerado — o frontmatter é o que separa.
    ///
    /// Lê-lo como censo faria o corte parar de recusar por causa da edição à
    /// mão de alguém, e ela viajaria para a unidade nova. A recusa NOMEIA o
    /// molde adotado, e devolve antes de qualquer fetch.
    #[test]
    fn an_adopted_mold_is_the_operators_writing_not_the_census() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        repo_tracking_the_census(root);
        git(root, &["checkout", "-b", "dev_first"]);

        remine(root);
        leftover_enrichment(root);
        // O molde curado, adotado: a partir do `source: manual` quem escreve
        // ali é o operador, e o próprio molde documenta isso.
        let adopted = root
            .join("apps")
            .join("rt")
            .join(".claude")
            .join("skills")
            .join("rt-verdict-pattern");
        std::fs::create_dir_all(&adopted).unwrap();
        std::fs::write(
            adopted.join("SKILL.md"),
            "---\nname: rt-verdict-pattern\nsource: manual\n---\n\n## Purpose\n",
        )
        .unwrap();
        let head_before = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");

        let CensusSettlement::Refuse(busy) =
            settle_cut(root, Some("dev_first"), "dev_second", Some("dev"), &flow_config())
        else {
            panic!("a edição à mão do operador recusa o corte");
        };
        let CheckoutWork::Holds { theirs: dirty, .. } = &busy.work else {
            panic!("os caminhos foram observados, veio {:?}", busy.work);
        };
        assert_eq!(
            dirty,
            &vec!["apps/rt/.claude/skills/rt-verdict-pattern/SKILL.md".to_string()],
            "a recusa nomeia o molde adotado e só ele: {dirty:?}",
        );
        assert_eq!(
            git_out(root, &["rev-parse", "HEAD"]).expect("HEAD"),
            head_before,
            "e nada foi commitado",
        );
    }

    /// Uma RECUSA não deixa nada para trás — nem um fetch, nem um avanço da
    /// base, nem um commit.
    ///
    /// A resposta `Refuse` promete isso na própria documentação dela, e uma
    /// promessa sobre o que outra parte do código faz é exatamente o tipo de
    /// comentário que este trabalho encontrou desatualizado em três arquivos. A
    /// ordem que a sustenta — recusar ANTES de agir — não tem como ser lida do
    /// resultado: sem o `origin` adiantado desta fixture, agir primeiro e
    /// recusar depois passa despercebido.
    #[test]
    fn a_refusal_leaves_the_repository_exactly_as_it_found_it() {
        let tmp = tempfile::tempdir().unwrap();
        // Árvore e `origin` LADO A LADO: um repositório DENTRO da árvore seria
        // trabalho não versionado do operador.
        let root = tmp.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.as_path();
        let origin = tmp.path().join("origin.git");
        let origin_s = origin.to_string_lossy().to_string();
        a_tree_dirty_only_with_the_census(root, None);

        // Um `origin` cuja `dev` está um commit VAZIO à frente da base local.
        git(root, &["init", "--bare", "-q", &origin_s]);
        git(root, &["remote", "add", "origin", &origin_s]);
        git(root, &["push", "-q", "origin", "dev"]);
        git(root, &["commit", "-q", "--allow-empty", "-m", "origin moved"]);
        git(root, &["push", "-q", "origin", "dev"]);
        let ahead = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");
        git(root, &["reset", "-q", "--hard", "HEAD~1"]);
        // …e a árvore parada na branch de OUTRA unidade, com trabalho dela não
        // commitado ao lado do censo: o corte é recusado porque esse trabalho
        // viajaria.
        git(root, &["checkout", "-q", "-b", "feature/outra-unidade"]);
        std::fs::write(root.join("theirs.txt"), "mine, not yours\n").unwrap();
        let head_before = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");

        let settled = settle_cut(
            root,
            Some("feature/outra-unidade"),
            "dev_second",
            Some("dev"),
            &flow_config(),
        );
        let CensusSettlement::Refuse(busy) = settled else {
            panic!("a precondição é a recusa: {settled:?}");
        };
        assert_eq!(
            busy.cause,
            crate::commands::event::work_branch::RefusalCause::WorkWouldTravel,
            "a recusa é pelo trabalho que viajaria",
        );
        assert!(
            !git_out(root, &["rev-list", "dev"]).expect("rev-list").contains(&ahead),
            "a base NÃO foi avançada: quem recusa não age antes de recusar",
        );
        assert_eq!(
            git_out(root, &["rev-parse", "HEAD"]).expect("HEAD"),
            head_before,
            "e nada foi commitado onde a árvore estava parada",
        );

        // A linha NA BASE da mesma promessa: o censo já foi posto de lado para
        // o avanço, e o avanço falha mesmo assim — aqui, num rascunho do
        // harness que o `origin` passou a versionar (rascunho não entra na
        // medição, então nada o pôs de lado). A recusa devolve o censo exatamente
        // como estava, ÍNDICE incluído, e não deixa entrada nenhuma no stash.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.as_path();
        let (ahead, _) = origin_ahead_touching_the_census(root);
        // O `origin` também passou a versionar um rascunho do harness (em cima
        // do commit à frente, e a máquina B volta dois)…
        git(root, &["reset", "-q", "--hard", &ahead]);
        std::fs::write(root.join(".claude").join("feature-digest.json"), "{}\n").unwrap();
        git(root, &["add", "-f", ".claude/feature-digest.json"]);
        git(root, &["commit", "-q", "-m", "a scratch file, versioned by mistake"]);
        git(root, &["push", "-q", "origin", "dev"]);
        git(root, &["reset", "-q", "--hard", "HEAD~2"]);
        // …que nesta máquina existe, não rastreado, e vai barrar o avanço.
        std::fs::write(root.join(".claude").join("feature-digest.json"), "{\"local\":1}\n")
            .unwrap();
        remine(root);
        leftover_enrichment(root);
        // O modelo ENCENADO no índice: o estado que o descarte antigo não via.
        git(root, &["add", project_map::MAP_FILE]);
        assert!(
            matches!(checkout_work(root), CheckoutWork::CensusOnly(_)),
            "precondição: só o censo (e um rascunho) está sujo",
        );
        let ours = std::fs::read(project_map::model_path(root)).unwrap();
        let dirty_before = porcelain(root);
        let head_before = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");

        let settled = settle_cut(root, Some("dev"), "dev_second", Some("dev"), &flow_config());
        let CensusSettlement::Refuse(busy) = settled else {
            panic!("o avanço barrado pelo rascunho recusa: {settled:?}");
        };
        assert!(
            matches!(busy.cause, crate::commands::event::work_branch::RefusalCause::BaseStale { .. }),
            "a causa é a base: {:?}",
            busy.cause
        );
        assert!(
            !git_out(root, &["rev-list", "dev"]).expect("rev-list").contains(&ahead),
            "a base não avançou",
        );
        assert_eq!(git_out(root, &["rev-parse", "HEAD"]).expect("HEAD"), head_before);
        assert_eq!(
            porcelain(root),
            dirty_before,
            "o censo posto de lado voltou exatamente como estava — o modelo encenado inclusive",
        );
        assert_eq!(
            std::fs::read(project_map::model_path(root)).unwrap(),
            ours,
            "e com o conteúdo local, não o do origin",
        );
        assert!(
            git_out(root, &["rev-parse", "--verify", "--quiet", "refs/stash"]).is_none(),
            "nenhuma entrada de stash ficou para trás",
        );
    }

    /// Um censo ENCENADO no índice (`M `) é posto de lado do mesmo jeito que um
    /// só modificado (` M`): o avanço passa e o modelo é o do origin.
    ///
    /// O descarte antigo restaurava do ÍNDICE, então uma mudança encenada
    /// continuava na frente do fast-forward — recusa com um remédio que falhava
    /// do mesmo jeito. O stash guarda os dois estados.
    #[test]
    fn a_staged_census_change_is_set_aside_and_the_base_advances() {
        use crate::commands::event::work_branch::{cut_pending_work_branch, CutOutcome};

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.as_path();
        let root_s = root.to_string_lossy().to_string();
        let (ahead, origins_census) = origin_ahead_touching_the_census(root);

        remine(root);
        leftover_enrichment(root);
        git(root, &["add", project_map::MAP_FILE]);
        assert!(
            porcelain(root).lines().any(|l| l.starts_with("M ")),
            "precondição: o modelo está ENCENADO: {}",
            porcelain(root)
        );

        let sid = "sess-staged-census";
        crate::shared::context::pending_branch::set_pending_branch(&root_s, sid, "dev_second", None);
        let outcome = cut_pending_work_branch(root, sid);
        assert_eq!(outcome, CutOutcome::Cut("dev_second".to_string()), "{outcome:?}");
        assert!(
            git_out(root, &["rev-list", "dev"]).expect("rev-list").contains(&ahead),
            "a base avançou apesar do censo encenado",
        );
        assert_eq!(std::fs::read(project_map::model_path(root)).unwrap(), origins_census);
        assert_eq!(
            git_out(root, &["rev-parse", "dev"]).expect("dev"),
            ahead,
            "a base é exatamente a do origin — nenhum commit foi escrito nela",
        );
        assert!(
            matches!(checkout_work(root), CheckoutWork::CensusOnly(_)),
            "e o resto do censo segue sujo, sem ser gravado",
        );
        assert!(
            git_out(root, &["rev-parse", "--verify", "--quiet", "refs/stash"]).is_none(),
            "a entrada de stash foi consumida",
        );
    }

    /// Um molde AUTORADO (`source: scan`, escrito pela passagem de
    /// enriquecimento e não regenerado pelo mine) que o `origin` também
    /// reescreveu: os DOIS textos sobrevivem — o do origin no lugar dele, o
    /// local ao lado — e o stderr diz onde. O descarte antigo apagava o local
    /// em silêncio.
    #[test]
    fn an_authored_mold_rewritten_on_origin_too_is_kept_beside_origins() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.as_path();
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#,
        )
        .unwrap();
        repo_tracking_the_census(root);
        // O molde, rastreado, escrito pela passagem de enriquecimento.
        let mold = root.join("apps").join("rt").join(".claude").join("skills").join("rt-gate-pattern");
        std::fs::create_dir_all(&mold).unwrap();
        let mold_rel = "apps/rt/.claude/skills/rt-gate-pattern/SKILL.md";
        std::fs::write(mold.join("SKILL.md"), "---\nname: rt-gate-pattern\nsource: scan\n---\n\nA\n")
            .unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "the mold"]);
        let origin = tmp.path().join("origin.git");
        let origin_s = origin.to_string_lossy().to_string();
        git(root, &["init", "--bare", "-q", &origin_s]);
        git(root, &["remote", "add", "origin", &origin_s]);
        git(root, &["push", "-q", "origin", "dev"]);
        // A máquina A re-autorou o molde e publicou.
        const THEIRS: &str = "---\nname: rt-gate-pattern\nsource: scan\n---\n\nB (origin)\n";
        std::fs::write(mold.join("SKILL.md"), THEIRS).unwrap();
        git(root, &["commit", "-q", "-am", "re-authored on origin"]);
        git(root, &["push", "-q", "origin", "dev"]);
        git(root, &["reset", "-q", "--hard", "HEAD~1"]);
        // A máquina B também, sem ter puxado.
        const OURS: &str = "---\nname: rt-gate-pattern\nsource: scan\n---\n\nC (local)\n";
        std::fs::write(mold.join("SKILL.md"), OURS).unwrap();
        assert!(
            matches!(checkout_work(root), CheckoutWork::CensusOnly(_)),
            "precondição: o molde `source: scan` é censo",
        );

        let settled = settle_open(root, Some("dev"), Some("dev"), &flow_config());
        assert!(
            !matches!(settled, CensusSettlement::Refuse(_)),
            "o molde no caminho não prende a base: {settled:?}",
        );
        assert_eq!(
            std::fs::read_to_string(mold.join("SKILL.md")).unwrap(),
            THEIRS,
            "o texto do origin está no lugar dele",
        );
        assert_eq!(
            std::fs::read_to_string(mold.join("SKILL.set-aside.md")).unwrap(),
            OURS,
            "e o texto local foi mantido AO LADO, não apagado",
        );
        assert!(
            git_out(root, &["rev-parse", "--verify", "--quiet", "refs/stash"]).is_none(),
            "com os dois textos em casa, a entrada de stash foi consumida",
        );
        let CheckoutWork::Holds { theirs, .. } = checkout_work(root) else {
            panic!("o texto mantido ao lado é do operador reconciliar");
        };
        assert_eq!(theirs, vec![mold_rel.replace("SKILL.md", "SKILL.set-aside.md")]);
    }

    /// Uma base PROTEGIDA com o censo re-minerado E uma edição do operador —
    /// a primeira unidade cortando no lugar, por desenho — cujo `origin` tocou
    /// o censo. Devolve o commit à frente e o conteúdo do origin para o modelo.
    fn protected_main_behind_origin(root: &Path) -> (String, Vec<u8>) {
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"main"},"protected":["main"]}}"#,
        )
        .unwrap();
        init_repo_on(root, "main");
        project_map::write_text(root, "{\"projects\":[]}\n").unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "track the census"]);
        let origin = root.parent().expect("tmp").join("origin.git");
        let origin_s = origin.to_string_lossy().to_string();
        git(root, &["init", "--bare", "-q", &origin_s]);
        git(root, &["remote", "add", "origin", &origin_s]);
        git(root, &["push", "-q", "origin", "main"]);
        const ORIGINS_CENSUS: &str = "{\"projects\":[{\"dir\":\"apps/rt\"},{\"dir\":\"apps/cli\"}]}\n";
        project_map::write_text(root, ORIGINS_CENSUS).unwrap();
        // O mapa é um banco: o que se compara são os bytes do arquivo.
        let origins_census = std::fs::read(project_map::model_path(root)).unwrap();
        git(root, &["commit", "-q", "-am", "another machine re-mined the census"]);
        git(root, &["push", "-q", "origin", "main"]);
        let ahead = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");
        git(root, &["reset", "-q", "--hard", "HEAD~1"]);
        (ahead, origins_census)
    }

    /// `Holds` numa base protegida: o trabalho do operador segue para a
    /// primeira unidade por desenho, mas o CENSO ao lado dele continua sendo da
    /// ferramenta — e é posto de lado para a base avançar, exatamente como
    /// numa árvore só de censo. Antes, a leitura `Holds` descartava os caminhos
    /// do censo, o fast-forward abortava neles e toda escrita da sessão era
    /// negada prescrevendo um `git pull` que abortava do mesmo jeito.
    #[test]
    fn a_holds_tree_on_a_protected_base_sets_its_census_aside_and_advances() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.as_path();
        let (ahead, origins_census) = protected_main_behind_origin(root);
        let config = ProjectConfig::load(root);
        remine(root);
        std::fs::write(root.join("theirs.txt"), "mine, not yours\n").unwrap();
        let CheckoutWork::Holds { theirs, census } = checkout_work(root) else {
            panic!("precondição: trabalho do operador E censo");
        };
        assert_eq!(theirs, vec!["theirs.txt".to_string()]);
        assert!(census.iter().any(|p| p.ends_with(project_map::MAP_FILE_NAME)), "{census:?}");

        let settled = settle_cut(root, Some("main"), "feature/first", Some("main"), &config);
        assert!(
            !matches!(settled, CensusSettlement::Refuse(_)),
            "o censo ao lado do trabalho deles não prende a base: {settled:?}",
        );
        assert!(
            git_out(root, &["rev-list", "main"]).expect("rev-list").contains(&ahead),
            "a base avançou",
        );
        assert_eq!(std::fs::read(project_map::model_path(root)).unwrap(), origins_census);
        assert_eq!(
            std::fs::read_to_string(root.join("theirs.txt")).unwrap(),
            "mine, not yours\n",
            "e o arquivo deles não foi tocado",
        );
    }

    /// …e quando é o arquivo DELES que o avanço sobrescreveria, a recusa nomeia
    /// esse arquivo e prescreve o stash — não um `git pull` que falha nele do
    /// mesmo jeito — e não toca em nada: nem o censo é posto de lado.
    #[test]
    fn their_file_in_the_way_of_the_advance_is_named_and_the_stash_prescribed() {
        use crate::commands::event::work_branch::RefusalCause;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.as_path();
        let (ahead, _) = protected_main_behind_origin(root);
        // O origin também tocou `f.txt` (em cima do commit à frente; a máquina
        // B volta dois)…
        git(root, &["reset", "-q", "--hard", &ahead]);
        std::fs::write(root.join("f.txt"), "theirs on origin").unwrap();
        git(root, &["commit", "-q", "-am", "f on origin"]);
        git(root, &["push", "-q", "origin", "main"]);
        git(root, &["reset", "-q", "--hard", "HEAD~2"]);
        // …que o operador editou aqui, sem commitar.
        std::fs::write(root.join("f.txt"), "edited here").unwrap();
        remine(root);
        let config = ProjectConfig::load(root);
        let dirty_before = porcelain(root);
        let head_before = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");

        let settled = settle_cut(root, Some("main"), "feature/first", Some("main"), &config);
        let CensusSettlement::Refuse(busy) = settled else {
            panic!("o arquivo deles no caminho recusa: {settled:?}");
        };
        let RefusalCause::BaseBlockedByWork { base, paths } = &busy.cause else {
            panic!("a causa nomeia o trabalho deles: {:?}", busy.cause);
        };
        assert_eq!(base, "main");
        assert_eq!(paths, &vec!["f.txt".to_string()], "só o arquivo no caminho, não todo o sujo");
        let reason = busy.reason(mustard_core::platform::i18n::Locale::EnUs);
        assert!(reason.contains("f.txt"), "a frase nomeia o arquivo: {reason}");
        assert!(reason.contains("stash"), "e prescreve o stash: {reason}");
        assert!(
            !git_out(root, &["rev-list", "main"]).expect("rev-list").contains(&ahead),
            "a base não avançou",
        );
        assert_eq!(git_out(root, &["rev-parse", "HEAD"]).expect("HEAD"), head_before);
        assert_eq!(porcelain(root), dirty_before, "nada foi tocado, nem o censo");
        assert!(
            git_out(root, &["rev-parse", "--verify", "--quiet", "refs/stash"]).is_none(),
            "e nada foi posto de lado",
        );
    }

    /// …e a outra metade da mesma regra: com trabalho do operador junto, a
    /// recusa continua valendo, nomeando SÓ o que é dele — e nada é commitado.
    #[test]
    fn operator_work_beside_the_census_still_refuses_and_names_only_theirs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        repo_tracking_the_census(root);
        git(root, &["checkout", "-b", "dev_first"]);

        remine(root);
        leftover_enrichment(root);
        std::fs::write(root.join("theirs.txt"), "mine, not yours\n").unwrap();
        let head_before = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");

        let CensusSettlement::Refuse(busy) =
            settle_cut(root, Some("dev_first"), "dev_second", Some("dev"), &flow_config())
        else {
            panic!("o trabalho do operador ainda recusa o corte");
        };
        let CheckoutWork::Holds { theirs: dirty, .. } = &busy.work else {
            panic!("os caminhos foram observados, veio {:?}", busy.work);
        };
        assert_eq!(
            dirty,
            &vec!["theirs.txt".to_string()],
            "a recusa nomeia só o que é do operador: {dirty:?}",
        );
        assert_eq!(
            git_out(root, &["rev-parse", "HEAD"]).expect("HEAD"),
            head_before,
            "com trabalho do operador na árvore o portão não commita nada",
        );
        assert!(
            porcelain(root).contains("theirs.txt"),
            "e o arquivo dele segue sendo dele para commitar",
        );
    }
}
