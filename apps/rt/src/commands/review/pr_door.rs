//! `mustard-rt run pr-list` / `pr-review` / `pr-merge` — the engine of the
//! `/mustard:pr` door.
//!
//! ONE module for three commands, because they are one ritual over one seam:
//! the link between a pull request and the work unit behind it. A PR's head
//! branch is `{kind}/{slug}` (or the older `{base}_{slug}`, still recognised)
//! and that slug IS the spec — `pr-review` prints the brief of the unit under
//! it and `pr-merge` reads back the verdicts the round recorded there. Written
//! once here, the link cannot drift into three spellings across three files.
//!
//! ## What each command answers
//!
//! - **`pr-list`** — the base gate first: it refuses from INSIDE a work unit,
//!   because "which PRs are open" is a question about the BASE, not about one
//!   unit. The test is the unit, never a declared list: a branch that is
//!   somebody's unit and is not a base by the project's one base reading
//!   ([`crate::commands::event::work_branch::on_integration_base`]) refuses and
//!   names the base to
//!   switch to, touching nothing; anything else is a base as far as this
//!   question goes. On a base it answers one row per open PR:
//!   number, title, whether the provider calls it mergeable, whether it is a
//!   draft, and the head branch its unit lives on.
//! - **`pr-review`** — resolves the PR to its unit and prints the review brief:
//!   the spec the unit belongs to, the subproject its tasks' files name, and the
//!   SAME skill shelf the implementer was dispatched with — so "reviewed
//!   against the project patterns" means the very molds the work was written
//!   to, never a second list that can drift. `--verdict` no longer records
//!   anything: it refuses at the door and says the round records each wave's
//!   verdict in the spec file. The merge step reads those verdicts from the
//!   spec's `spec.ndjson`, one per wave.
//!
//! ## The spec is read out of the PR's OWN branch
//!
//! A review runs from an integration base — that is the door's design. The spec
//! is its event file, `.claude/spec/{slug}/spec.ndjson`, never a rendered
//! `spec.md`: the binary writes no `spec.md` any more. It is read from the head
//! ref itself — `git show {head}:.claude/spec/{slug}/spec.ndjson` — for a
//! project that commits its specs, with the remote-tracking ref and then the
//! main checkout's own spec folder as fallbacks ([`spec_text_of_unit`]). The
//! subproject comes from the files the spec's tasks name.
//!
//! The verdict the merge reads needs no such hop: `.claude/` is redirected
//! state, resolved to the MAIN checkout from inside any linked worktree, so the
//! `spec.ndjson` the merge reads is the main checkout's, whatever branch
//! happens to be out, and the round that wrote it adds nothing tracked to the
//! base's tree.
//! - **`pr-merge`** — the merge and the tidying up, in ONE call. It merges,
//!   then hands the pruning to [`git_settle::settle_at`] — returning to the
//!   base, pulling it, removing the worktree and deleting the local branch IS
//!   the exit ritual, already written and already covering the in-place unit
//!   and the per-repo report. The branch on the SERVER is never touched unless
//!   `mustard.json#git.deleteRemoteBranch` says so: many teams may not delete
//!   it, because the merge is another area's and the branch is theirs.
//!
//!   A merge records three things, in this order: the `pr.merged` event, the
//!   spec's `delivered` phase — written through the spec file's own phase
//!   door, which is what ARMS the pending charge at the end of the answer —
//!   and the closing of the pending item whose note says it became this spec.
//!   The report returns `pendingClosed` and `pendingOpen` — the pending items
//!   born in the spec that stay open — so the delivery asks only about them,
//!   never about the whole list.
//!
//!   O merge feito por outra pessoa passa pelo mesmo caminho ([`land`]): o
//!   início da sessão, com a spec atual em "pull request aberto", pergunta ao
//!   provedor só pelo pull request dela ([`merged_elsewhere`]) e, se ele
//!   entrou, grava a entrega e faz a arrumação sem ter feito o merge.
//!
//! ## The unreviewed merge WARNS and ASKS — it never refuses
//!
//! A merge requested without an `approved` verdict answers `action:"confirm"`
//! with `ok:true` and touches NOTHING: not a refusal (the operator decides case
//! by case) and not a silent merge. `--confirm` is that answer coming back —
//! the same hand-back shape `git-settle` uses for `exit-and-rerun`. The rule is
//! deliberately one rule, not two: an absent verdict and a recorded rejection
//! are both "not approved", and both are ASKED about rather than forked into
//! separate behaviours.
//!
//! Fail-open everywhere `gh` is involved: an absent CLI or an unreachable
//! provider degrades to an honest `gh_error` field and exit 0. The consent rule
//! above is the one thing that never degrades — no evidence means ASK.
//!
//! ## The WRITE path already left `gh` — this module keeps only reads
//!
//! Every pull-request WRITE (create/edit/ready) now goes through the provider
//! port ([`crate::shared::pr_provider::PrProvider`], spoken by
//! [`crate::commands::review::pr_publish`]). The `gh pr list`/`view` reads
//! below (and the [`gh_out`]/[`gh_json`] helpers other modules import) migrate
//! behind the same port in their own unit; they stay direct shell-outs here
//! until then.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use serde_json::Value;

use crate::commands::agent::render::skills::build_skills_list;
use crate::commands::event::pending::{became_of, close_pending, open_pending_born_in, OpenPending};
use crate::commands::event::work_branch::on_integration_base;
use crate::commands::git_settle::{git_out, main_checkout_root, settle_at, settle_unit_at, superproject_of};
use crate::commands::review::pr_publish::{spec_pr, submodules_landed, SubmodulePrs};
use crate::shared::branch_state::PrStatus;
use crate::shared::pr_provider::{provider_for, provider_in, PrChecks};
use crate::shared::work_kind::BaseFlow;

/// O subprojeto que um conjunto de arquivos aponta, ou nada quando eles se
/// espalham por mais de um.
///
/// Lê o par `apps/<nome>` ou `packages/<nome>` de cada caminho; dois pares
/// diferentes no mesmo conjunto não têm subprojeto comum, e a resposta é nada.
/// Veio da checagem de dependência quando ela saiu: era a única função dela
/// com chamador vivo.
fn detect_subproject(files: &[String], repo_root: &Path) -> Option<PathBuf> {
    let mut chosen: Option<(String, String)> = None;
    for raw in files {
        let normalized = raw.replace('\\', "/");
        let segments: Vec<&str> = normalized.split('/').filter(|s| !s.is_empty()).collect();
        let mut found: Option<(String, String)> = None;
        let bases = ["apps", "packages"];
        for (i, seg) in segments.iter().enumerate() {
            if bases.contains(seg)
                && let Some(name) = segments.get(i + 1) {
                    found = Some(((*seg).to_string(), (*name).to_string()));
                    break;
                }
        }
        match (&chosen, &found) {
            (None, Some(f)) => chosen = Some(f.clone()),
            (Some(c), Some(f)) if c != f => return None,
            _ => {}
        }
    }
    chosen.map(|(base, name)| repo_root.join(base).join(name))
}


// ---------------------------------------------------------------------------
// Shared plumbing — the provider, the bases, and the PR↔unit link
// ---------------------------------------------------------------------------

/// Run `gh` in `root` and return its trimmed stdout, or the reason it did not
/// answer.
///
/// Same shape the antigo review-prefetch usava (the `cmd /C`
/// hop is how a `gh.cmd` shim is found on Windows) plus one addition that
/// matters here: the working directory. `gh` resolves the repository from the
/// cwd, and every command in this module asks about THIS project's pull
/// requests — inheriting the process cwd would ask about whichever repository
/// the session happens to sit in.
pub(crate) fn gh_out(root: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", "gh"]);
        c
    } else {
        Command::new("gh")
    };
    let Ok(out) = cmd.args(args).current_dir(root).output() else {
        return Err("gh-not-found".to_string());
    };
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        if stderr.contains("command not found") || out.status.code() == Some(127) {
            return Err("gh-not-found".to_string());
        }
        return Err(if stderr.is_empty() { "gh-failed".to_string() } else { stderr });
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// [`gh_out`] plus a JSON parse — an unparseable body is `parse-error`, never a
/// panic.
pub(crate) fn gh_json(root: &Path, args: &[&str]) -> Result<Value, String> {
    let text = gh_out(root, args)?;
    serde_json::from_str(&text).map_err(|_| "parse-error".to_string())
}

/// The repository root every command here works from: the MAIN checkout when
/// `root` sits inside a linked worktree, `root` itself otherwise. `mustard.json`
/// and `.claude/` live there, and so does the repository `gh` must resolve.
/// `pub(crate)` because [`super::pr_publish`] resolves the SAME root for the
/// same reason — one spelling, not two that can drift.
pub(crate) fn project_root(root: &Path) -> PathBuf {
    main_checkout_root(root).unwrap_or_else(|| root.to_path_buf())
}

/// The project's base model (derived from `git.flow`) and the branch the
/// checkout is standing on. No branch name is ever hardcoded — the core owns
/// that derivation so this door and the work-branch gate agree.
///
/// ROOTED ([`BaseFlow::of_at`]), never the pure derivation: every caller here
/// hands in [`project_root`], the main checkout where `.claude/` lives, and this
/// door resolves REAL branches of that repository. A rootless model cannot read
/// the base a unit's own directory RECORDED, so in a project declaring several
/// emergency bases an in-flight `hotfix/…` answered
/// [`crate::shared::work_kind::UnitBase::Ambiguous`] here and the refusal below
/// fell back to the primary base — naming a base the operator never chose.
fn bases_and_branch(root: &Path) -> (BaseFlow, String) {
    let cfg = mustard_core::ProjectConfig::load(root);
    let flow = BaseFlow::of_at(&cfg.git, root);
    let branch = mustard_core::current_branch(root).unwrap_or_default();
    (flow, branch)
}

/// The spec slug a work branch names — [`BaseFlow::slug_of`], the crate's ONE
/// spelling of the question, shared with the per-branch notebook.
///
/// `None` when the branch is nobody's work unit — a PR opened by hand or a
/// base→base promotion has no unit, and therefore no spec to review against or
/// verdict to read.
fn spec_of_branch(branch: &str, flow: &BaseFlow) -> Option<String> {
    flow.slug_of(branch)
}

/// Where a unit's spec lives, relative to the repository root: its event file.
fn spec_rel_path(slug: &str) -> String {
    format!(".claude/spec/{slug}/spec.ndjson")
}

/// The event file of `slug`'s spec, as the PR's OWN branch carries it when the
/// project commits its specs, else as this machine keeps it.
///
/// `git show <head>:.claude/spec/<slug>/spec.ndjson` first — the local ref for
/// the author reviewing their own unit, the remote-tracking ref for the
/// reviewer who only ever fetched it — because `pr-review` runs from an
/// integration base by design. The main checkout's own spec folder is the last
/// fallback: specs usually stay out of git, and for a spec that was never
/// committed it is the only copy there is.
fn spec_text_of_unit(root: &Path, head: &str, slug: &str) -> Option<String> {
    let rel = spec_rel_path(slug);
    for reference in [format!("{head}:{rel}"), format!("origin/{head}:{rel}")] {
        if let Some(text) = git_out(root, &["show", &reference]).filter(|t| !t.trim().is_empty()) {
            return Some(text);
        }
    }
    let on_disk = mustard_core::ClaudePaths::for_project(root)
        .ok()
        .and_then(|p| p.for_spec(slug).ok())
        .map(|p| p.spec_ndjson_path())?;
    std::fs::read_to_string(on_disk).ok().filter(|t| !t.trim().is_empty())
}

/// The pull request a command was pointed at, reduced to the two facts every
/// step here needs: which PR, and which branch carries its unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrFacts {
    pub number: u64,
    /// The PR's head branch — the work unit's branch.
    pub head: String,
}

/// Ask the provider which PR is meant. `None` = the one for the current branch,
/// which is what the door uses from inside a unit.
fn resolve_pr(root: &Path, pr: Option<u64>) -> Result<PrFacts, String> {
    let number = pr.map(|n| n.to_string());
    let mut args: Vec<&str> = vec!["pr", "view"];
    if let Some(n) = number.as_deref() {
        args.push(n);
    }
    args.extend_from_slice(&["--json", "number,headRefName"]);
    let value = gh_json(root, &args)?;
    let Some(number) = value.get("number").and_then(Value::as_u64) else {
        return Err("no-pr-for-branch".to_string());
    };
    Ok(PrFacts {
        number,
        head: value
            .get("headRefName")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

/// The subproject the files of a spec's tasks name, relative to the repo root
/// (`apps/rt`, `packages/core`, …), read from the spec's event file
/// `spec_text`: every task the reading shows, each file once. `None` when the
/// paths disagree or name no `apps/<x>` / `packages/<x>` segment.
///
/// Derived through [`detect_subproject`], the ONE discovery the dispatch plan
/// already uses — joined onto an empty root so the answer comes back relative,
/// which is the form both the skill shelf and `review.result` want.
fn spec_subproject(spec_text: &str) -> Option<String> {
    let log = mustard_core::domain::spec_events::parse_log(spec_text);
    let mut files: Vec<String> = Vec::new();
    for task in log.visible().into_iter().filter(|e| e.event_type == "task") {
        let declared = task.fields.get("files").and_then(Value::as_array).into_iter().flatten();
        for path in declared.filter_map(|file| file.get("path").and_then(Value::as_str)) {
            if !files.iter().any(|known| known == path) {
                files.push(path.to_string());
            }
        }
    }
    detect_subproject(&files, Path::new(""))
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .filter(|s| !s.is_empty())
}

// ---------------------------------------------------------------------------
// `pr-list`
// ---------------------------------------------------------------------------

/// One open pull request, as the door lists it.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub(crate) struct PrEntry {
    pub number: u64,
    pub title: String,
    /// The provider's own word, verbatim (`MERGEABLE` / `CONFLICTING` /
    /// `UNKNOWN`). Never re-spelled into a bool: `UNKNOWN` means the provider
    /// has not finished computing it, which is not the same answer as "no".
    pub mergeable: String,
    /// A draft cannot be merged even when it is mergeable — the parent of a
    /// monorepo unit opens as a draft while any submodule PR is still open, so
    /// omitting this would show a row the merge step will refuse.
    pub draft: bool,
    /// The head branch — the work unit `pr-review` and `pr-merge` act on.
    pub head: String,
}

/// The `pr-list` document.
#[derive(Debug, Serialize)]
pub(crate) struct PrListReport {
    /// False only when the base gate refused. An unreachable provider is
    /// `ok: true` with `gh_error` set — the checkout was fine, the network was
    /// not.
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
    pub branch: String,
    /// What `git.flow` really DECLARES — reported so the operator sees the hint
    /// the project wrote down, and EMPTY when it wrote none (the installer
    /// writes no flow). It decides nothing here: the refusal below is measured
    /// against the unit and the protected set, never against this list — and it
    /// is [`mustard_core::ProjectConfig`]'s declared set rather than its
    /// pre-selected one, so a report never names the `{main, master}` fallback
    /// as branches this repository has.
    pub bases: Vec<String>,
    /// Sorted by number, so two runs over the same state print the same bytes.
    pub prs: Vec<PrEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gh_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// Read one `gh pr list` row into a [`PrEntry`]. A row without a number is not
/// a pull request and is dropped rather than reported as number 0.
fn pr_entry(row: &Value) -> Option<PrEntry> {
    Some(PrEntry {
        number: row.get("number").and_then(Value::as_u64)?,
        title: row.get("title").and_then(Value::as_str).unwrap_or_default().to_string(),
        mergeable: row
            .get("mergeable")
            .and_then(Value::as_str)
            .unwrap_or("UNKNOWN")
            .to_string(),
        draft: row.get("isDraft").and_then(Value::as_bool).unwrap_or(false),
        head: row
            .get("headRefName")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

/// List the open pull requests of the base `root` is standing on.
///
/// **What the refusal measures.** It used to ask whether the checkout's branch
/// appears in `git.flow`'s declared set, which refused a real integration base
/// for the sole reason that a file written at install time does not list it —
/// and the installer writes no flow at all. The question this command actually
/// asks is the opposite one: *am I standing INSIDE a unit?* So it refuses on a
/// positive reading — the branch is somebody's work unit
/// ([`crate::shared::work_kind::BaseFlow::base_of`], the crate's one parser) —
/// and lets a branch the project's one base reading
/// ([`crate::commands::event::work_branch::on_integration_base`]) measures as a
/// base through even when its name reads like a unit's.
#[must_use]
pub(crate) fn list_at(root: &Path) -> PrListReport {
    let repo = project_root(root);
    let (flow, branch) = bases_and_branch(&repo);
    let config = mustard_core::ProjectConfig::load(&repo);
    let bases: Vec<String> = config.git.declared_bases().into_iter().collect();
    let unit = flow.base_of(&branch);
    // The project's own RECORD of the unit, not the name's shape: an undeclared
    // base like `release/2026-Q3` splits into a kind and a slug exactly like a
    // unit branch does, and `pr list` was measured refusing to run from it.
    // A pergunta "esta branch é base de integração" tem uma resposta só, a
    // mesma que as outras portas fazem: o que o projeto declarou MAIS o que o
    // próprio remoto chama de padrão. Sem a segunda metade, um projeto
    // recém-instalado — que não declara fluxo nenhum — respondia aqui o
    // contrário do que responde lá.
    if flow.has_unit_record(&branch) && !on_integration_base(&repo, &branch, &config) {
        // Name the base rather than the rule. The unit's OWN record answers
        // first — it is a measurement of where the branch really came from —
        // and the remote's own default (`origin/HEAD`) is the last resort, so
        // the refusal ends with something the operator can type without this
        // module ever spelling a branch name of its own.
        // Three sources, in the order their authority runs out. The unit's own
        // record is a measurement of where this branch really came from. Next,
        // when — and only when — the project DECLARES a flow, its primary base
        // is the project's own stated answer: naming `origin/HEAD` there sent a
        // unit that integrates into `dev` off to `main`, a regression measured
        // in a repo whose flow says exactly that. With no flow declared there is
        // nothing to state, and the remote's own default is the last resort, so
        // this module never spells a branch name of its own.
        let target = unit
            .known()
            .map(str::to_string)
            .or_else(|| config.git.primary_base())
            .or_else(|| mustard_core::default_branch(&repo));
        let hint = match &target {
            Some(base) => format!(
                "`pr list` asks about a BASE, not about one unit — switch to `{base}` \
                 (`git checkout {base}`) and run it again"
            ),
            // Nothing recorded the base and git named no default: say what to
            // do without inventing a branch nobody measured.
            None => "`pr list` asks about a BASE, not about one unit — switch to the branch \
                     this unit integrates into and run it again"
                .to_string(),
        };
        return PrListReport {
            ok: false,
            reason: Some("not-on-integration-base"),
            hint: Some(hint),
            branch,
            bases,
            prs: Vec::new(),
            gh_error: None,
        };
    }

    let (prs, gh_error) = match gh_json(
        &repo,
        &["pr", "list", "--state", "open", "--json", "number,title,mergeable,isDraft,headRefName"],
    ) {
        Ok(value) => {
            let mut rows: Vec<PrEntry> = value
                .as_array()
                .map(|arr| arr.iter().filter_map(pr_entry).collect())
                .unwrap_or_default();
            rows.sort_by_key(|p| p.number);
            (rows, None)
        }
        Err(e) => (Vec::new(), Some(e)),
    };
    PrListReport { ok: true, reason: None, branch, bases, prs, gh_error, hint: None }
}

// ---------------------------------------------------------------------------
// `pr-review`
// ---------------------------------------------------------------------------

/// The `pr-review` document — the review brief, plus what was recorded when a
/// verdict was supplied.
#[derive(Debug, Serialize)]
pub(crate) struct PrReviewReport {
    /// False only when the PR could not be resolved. A unit with no spec is a
    /// legitimate answer (`spec: null`), not a failure.
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
    pub pr: u64,
    pub head: String,
    /// The spec slug the head branch names — the unit under review.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec: Option<String>,
    /// Where that spec lives, when it is on disk in this checkout. The spec is
    /// materialized ON the work branch, so a base checkout legitimately reports
    /// `null` here while the branch itself reports the path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subproject: Option<String>,
    /// The subproject's skill shelf, verbatim — the same block the implementer
    /// was dispatched with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patterns: Option<String>,
}

/// Build the review brief for a resolved PR.
///
/// It records nothing. The door refuses `--verdict` before anything gets
/// here, so a recorder reachable from this step would be a recorder nobody
/// can reach: the per-wave verdicts `pr-merge` reads are the ones the round
/// writes into the spec's `spec.ndjson`.
#[must_use]
fn review_brief(root: &Path, facts: &PrFacts, flow: &BaseFlow) -> PrReviewReport {
    let spec = spec_of_branch(&facts.head, flow);
    let spec_text = spec
        .as_deref()
        .and_then(|slug| spec_text_of_unit(root, &facts.head, slug));
    // Named only when the spec was really found — a path on an `ok:true` report
    // is a promise that something is there.
    let spec_path = spec
        .as_deref()
        .filter(|_| spec_text.is_some())
        .map(spec_rel_path);
    let subproject = spec_subproject(spec_text.as_deref().unwrap_or_default());
    let patterns = subproject
        .as_deref()
        .map(|sub| build_skills_list(root, sub))
        .filter(|shelf| !shelf.is_empty());

    PrReviewReport {
        ok: true,
        reason: None,
        pr: facts.number,
        head: facts.head.clone(),
        spec,
        // Repo-relative, forward slashes: one report reads the same on every
        // platform and carries no machine path.
        spec_path,
        subproject,
        patterns,
    }
}

// ---------------------------------------------------------------------------
// `pr-merge`
// ---------------------------------------------------------------------------

/// What the merge step is allowed to do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MergeConsent {
    /// Merge now.
    Proceed,
    /// WARN and ASK — nothing is touched. Never a refusal: the operator's
    /// answer decides, case by case.
    Ask { reason: &'static str },
}

/// What the provider's own checks say about merging — `None` when they do not
/// stand in the way.
///
/// The three refusing answers are one shape on purpose: a run still in flight,
/// a run that failed, and an answer that could not be read are all "no evidence
/// that this tree is green", and the door's response to each is the same
/// question. They keep separate REASONS because the operator's next move
/// differs — wait, fix, or look at why the provider went quiet.
fn checks_reason(checks: &Result<PrChecks, String>) -> Option<&'static str> {
    match checks {
        // A project with no CI measured zero runs; that is an answer, not a
        // silence, and it merges like it always did.
        Ok(PrChecks::Passed | PrChecks::Absent) => None,
        Ok(PrChecks::Running) => Some("provider-checks-running"),
        Ok(PrChecks::Failed) => Some("provider-checks-failed"),
        Ok(PrChecks::Unknown(_)) | Err(_) => Some("provider-checks-unreadable"),
    }
}

/// The ONE rule the merge step applies before it merges: a merge with no
/// evidence behind it is ASKED about — never refused, never merged silently.
///
/// Two independent sources of evidence, one rule. The review verdict is an
/// opinion Mustard recorded; the provider's checks are a result the provider
/// observed. Reading only the first is how a pull request came to be merged
/// while its CI run was still in flight, and the run then answered a question
/// nobody had any more.
///
/// The checks are read FIRST because they are the fact that expires: a verdict
/// recorded yesterday still describes the same code, while a run that was
/// pending a second ago decides something the door is about to make permanent.
///
/// Every refusing case takes the SAME branch on purpose. An absent verdict, a
/// recorded rejection, a pending run and a failed one are one fact ("not
/// evidently mergeable") and the answer to all of them is the same question;
/// forking them into separate behaviours would add decisions the operator never
/// asked for. `confirmed` is that operator's answer coming back — the one way
/// through the gate, deliberately.
///
/// `promotion` é a rota que não tem veredito nenhum a ter: levar uma base de
/// integração para outra não é obra de ninguém, então não existe spec, não
/// existe revisão e a pergunta pela falta do veredito nunca poderia ser
/// respondida com um veredito. Quem usa aprendia a confirmar sem ler. As
/// verificações do provedor continuam valendo na promoção, porque ali há um
/// resultado real a esperar.
#[must_use]
pub(crate) fn merge_consent(
    verdict: Option<&str>,
    checks: &Result<PrChecks, String>,
    confirmed: bool,
    promotion: bool,
) -> MergeConsent {
    if confirmed {
        return MergeConsent::Proceed;
    }
    if let Some(reason) = checks_reason(checks) {
        return MergeConsent::Ask { reason };
    }
    if promotion || verdict == Some("approved") {
        return MergeConsent::Proceed;
    }
    MergeConsent::Ask {
        reason: if verdict.is_none() { "no-review-verdict" } else { "review-not-approved" },
    }
}

/// A rota da promoção entre bases: o pull request cujo head é uma base que o
/// próprio projeto declarou (`dev` para `main`, por exemplo).
///
/// Uma leitura só, usada pelo portão antes de decidir e por [`land`] depois do
/// merge. Eram duas antes — a poda já reconhecia a rota, o portão não — e o
/// portão pedia confirmação por falta de um veredito que a rota nunca pode ter.
fn is_base_promotion(head: &str, flow: &BaseFlow) -> bool {
    flow.is_declared_base(head)
}

/// The review verdict of `spec`, read from its `spec.ndjson`: `approved` when
/// the last verdict of every wave approved, `rejected` when some wave's
/// rejected. `None` = the spec has no verdict at all, or no event file.
///
/// Per wave, because one wave's approval must not hide another's rejection.
/// The merge's answer to a rejection is a QUESTION: a cautious verdict costs
/// one confirmation, and ignoring it would cost a silent merge over a
/// rejection.
fn recorded_verdict(root: &Path, spec: &str) -> Option<String> {
    use mustard_core::domain::spec_state::SpecState as _;
    let log = crate::shared::spec_state::DiskSpecState::new(root).log(spec)?;
    mustard_core::domain::spec_state::review(&log).word().map(str::to_string)
}

/// The `pr-merge` document.
#[derive(Debug, Serialize)]
pub(crate) struct PrMergeReport {
    /// True for `merged` (as far as the exit ritual got) AND for `confirm` —
    /// an ASK is an instruction, not a failure, exactly like `git-settle`'s
    /// `exit-and-rerun`. Downgrading it would teach the caller to stop where it
    /// must continue.
    pub ok: bool,
    /// `confirm` (asked, nothing touched) · `merged` (merged, then settled) ·
    /// `merge-failed` (the provider refused; nothing was pruned).
    pub action: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
    pub pr: u64,
    /// The head branch — the unit this merge retires.
    pub head: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    /// What the PROVIDER'S own checks answered — its closed vocabulary
    /// (`passed` · `running` · `failed` · `absent`) or, when the query itself
    /// failed, the reason verbatim. Always present: it is the evidence for
    /// what this command did with it, including on the paths that merged.
    pub checks: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// The `git-settle` report, folded verbatim — the pruning half of this
    /// command IS that ritual, so its answer is not re-spelled here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settle: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// The pending item THIS merge closed — the one carrying in the list the
    /// note "became the spec X" of this spec, recorded at the opening
    /// (`emit-pipeline --pending`). Absent when there was no link.
    #[serde(rename = "pendingClosed", skip_serializing_if = "Option::is_none")]
    pub pending_closed: Option<String>,
    /// The pending items born in the merge's spec that stay open: the delivery
    /// asks only about them. Present on every `merged` (empty when nothing was
    /// born in the spec, and on a promotion, which has no spec) and absent when
    /// nothing was merged.
    #[serde(rename = "pendingOpen", skip_serializing_if = "Option::is_none")]
    pub pending_open: Option<Vec<OpenPending>>,
    /// Os pull requests dos submódulos da spec, depois do merge do pull
    /// request de um deles.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub submodules: Option<SubmodulePrs>,
}

/// Merge a resolved PR, with all three external effects injected: `checks` asks
/// the provider what its own runs say, `merge` asks it to merge, `settle` runs
/// the exit ritual. Injected so the consent rule can be exercised without a
/// provider, a network or a repository — and so a test can prove that a merge
/// the door refuses calls NEITHER of the other two.
///
/// The checks are read on EVERY path, including the confirmed one that ignores
/// the answer: one call site instead of two, and the report then carries what
/// the provider said even when the operator overrode it.
///
/// `session` is the one who asked for the merge, read from the environment by
/// the `run` entry: it is the session the new merge door's pending charge will
/// wait for.
#[must_use]
#[allow(clippy::too_many_arguments)]
fn merge_core(
    root: &Path,
    facts: &PrFacts,
    flow: &BaseFlow,
    confirmed: bool,
    checks: &dyn Fn(&Path, u64) -> Result<PrChecks, String>,
    merge: &dyn Fn(&Path, u64) -> Result<(), String>,
    settle: &dyn Fn(&Path, &str) -> Value,
    session: Option<&str>,
) -> PrMergeReport {
    let MergedPr { spec, verdict, checks_word } = match merge_or_ask(root, facts, flow, confirmed, checks, merge) {
        Ok(merged) => merged,
        Err(report) => return *report,
    };
    merged_report(root, facts, flow, spec, verdict, checks_word, settle, session)
}

/// O que o merge de um pull request deixou para o resto da porta.
struct MergedPr {
    spec: Option<String>,
    verdict: Option<String>,
    checks_word: String,
}

/// A metade de [`merge_core`] que pergunta e faz o merge: a pergunta ao
/// operador quando falta veredito aprovado ou as verificações do provedor não
/// deixam, e a recusa do provedor, voltam como `Err` com a resposta pronta.
fn merge_or_ask(
    root: &Path,
    facts: &PrFacts,
    flow: &BaseFlow,
    confirmed: bool,
    checks: &dyn Fn(&Path, u64) -> Result<PrChecks, String>,
    merge: &dyn Fn(&Path, u64) -> Result<(), String>,
) -> Result<MergedPr, Box<PrMergeReport>> {
    let spec = spec_of_branch(&facts.head, flow);
    let verdict = spec.as_deref().and_then(|slug| recorded_verdict(root, slug));
    let checks = checks(root, facts.number);
    let checks_word = match &checks {
        Ok(state) => state.word().to_string(),
        Err(reason) => reason.clone(),
    };

    let promotion = is_base_promotion(&facts.head, flow);

    if let MergeConsent::Ask { reason } = merge_consent(verdict.as_deref(), &checks, confirmed, promotion) {
        let unit = spec.as_deref().unwrap_or(&facts.head);
        return Err(Box::new(PrMergeReport {
            ok: true,
            action: "confirm",
            reason: Some(reason),
            pr: facts.number,
            head: facts.head.clone(),
            warning: Some(match reason {
                "provider-checks-running" => format!(
                    "the provider's own checks for `{unit}` are still running — nothing was \
                     merged, so their verdict still has something to stop."
                ),
                // NOT "FAILING": a CANCELLED run reduces to `Failed` too, and this
                // repository produces those routinely (`concurrency:
                // cancel-in-progress` kills the superseded run on every re-push).
                // Being conservative about a cancelled run is right; telling the
                // operator it FAILED sends them hunting a failure that never
                // happened. Say what is actually known — it did not come back green.
                "provider-checks-failed" => format!(
                    "the provider's own checks for `{unit}` did not come back green (failed or \
                     cancelled) — nothing was merged."
                ),
                "provider-checks-unreadable" => format!(
                    "the provider's own checks for `{unit}` could not be read \
                     ({checks_word}) — nothing was merged."
                ),
                _ => match verdict.as_deref() {
                    None => {
                        format!("`{unit}` carries no recorded review verdict — nothing was merged.")
                    }
                    Some(v) => format!(
                        "the last review of `{unit}` came back `{v}` — nothing was merged."
                    ),
                },
            }),
            // One hint per reason, because the three checks cases ask for three
            // different moves. Grouping them under "is it a checks reason?" gave the
            // UNREADABLE case the advice to WAIT — and waiting is the one thing that
            // cannot help when nobody is running anything and the provider simply did
            // not answer. It also named `gh pr checks`, one provider's command line,
            // inside the door that goes through the port precisely so it never has to
            // name one.
            hint: Some(match reason {
                "provider-checks-running" => {
                    "wait for them to finish and run `pr merge` again, or re-run with `--confirm` \
                     to merge without waiting"
                        .to_string()
                }
                "provider-checks-failed" => {
                    "fix what they reported and push again, or re-run with `--confirm` to merge \
                     anyway"
                        .to_string()
                }
                "provider-checks-unreadable" => {
                    "the provider did not answer — check that its tooling is installed and \
                     authenticated, then run `pr merge` again; `--confirm` merges without it"
                        .to_string()
                }
                // No line that records a verdict: this door records none, and
                // the verdict read here is the one the round writes into the
                // spec, wave by wave. Naming a recording command the binary
                // refuses spent the reader's next call on a refusal.
                _ => "ask the operator, then re-run with `--confirm` to merge anyway — the verdict \
                      read here is the one `mustard-rt run round` records for each wave"
                    .to_string(),
            }),
            spec,
            verdict,
            checks: checks_word,
            settle: None,
            pending_closed: None,
            pending_open: None,
            submodules: None,
        }));
    }

    if let Err(e) = merge(root, facts.number) {
        return Err(Box::new(PrMergeReport {
            ok: false,
            action: "merge-failed",
            reason: Some("provider-refused"),
            pr: facts.number,
            head: facts.head.clone(),
            spec,
            verdict,
            checks: checks_word,
            warning: Some(e),
            settle: None,
            hint: Some(
                "nothing was pruned — the unit is untouched; resolve the refusal (conflicts, \
                 draft state, required checks) and run `pr merge` again"
                    .to_string(),
            ),
            pending_closed: None,
            pending_open: None,
            submodules: None,
        }));
    }
    Ok(MergedPr { spec, verdict, checks_word })
}

/// A metade de [`merge_core`] depois do merge: o fechamento gravado, a
/// arrumação e a resposta.
#[allow(clippy::too_many_arguments)]
fn merged_report(
    root: &Path,
    facts: &PrFacts,
    flow: &BaseFlow,
    spec: Option<String>,
    verdict: Option<String>,
    checks_word: String,
    settle: &dyn Fn(&Path, &str) -> Value,
    session: Option<&str>,
) -> PrMergeReport {

    // O aviso dos critérios viaja com o merge que aconteceu. Ele morava numa
    // etapa que olhava o `gh pr merge` digitado à mão, e por isso só alcançava
    // quem digitava a linha de comando do provedor; esta porta, que é a que
    // realmente faz o merge, não dizia nada.
    let qa_warning = spec
        .as_deref()
        .and_then(|slug| crate::commands::review::pr_publish::qa_warning(root, slug));
    let Landed { pending_closed, pending_open, settle: settled } =
        land(root, facts, spec.as_deref(), flow, settle, session);

    let Some(settled) = settled else {
        return PrMergeReport {
            ok: true,
            action: "merged",
            reason: Some("base-to-base-promotion"),
            pr: facts.number,
            head: facts.head.clone(),
            spec,
            verdict,
            checks: checks_word,
            warning: None,
            settle: None,
            hint: Some(format!(
                "`{head}` é uma base, não uma unidade: a promoção termina no merge e não há \
                 poda a fazer. Atualize as bases locais com \
                 `git fetch origin <base>:<base>` — nenhuma branch foi apagada.",
                head = facts.head,
            )),
            pending_closed,
            pending_open: Some(pending_open),
            submodules: None,
        };
    };

    PrMergeReport {
        ok: settled.get("ok") == Some(&Value::Bool(true)),
        action: "merged",
        reason: None,
        pr: facts.number,
        head: facts.head.clone(),
        spec,
        verdict,
        checks: checks_word,
        warning: qa_warning,
        settle: Some(settled),
        hint: None,
        pending_closed,
        pending_open: Some(pending_open),
        submodules: None,
    }
}

/// O merge do pull request de um submódulo da spec: a mesma pergunta e o
/// mesmo merge de [`merge_core`], com o veredito lido da spec no principal
/// `principal`; depois, no lugar da entrega e da arrumação, a conferência dos
/// pull requests dos submódulos ([`submodules_landed`]), que leva o ponteiro
/// ao principal, envia e deixa o principal pronto quando nenhum falta.
fn merge_submodule(
    principal: &Path,
    facts: &PrFacts,
    flow: &BaseFlow,
    confirmed: bool,
    checks: &dyn Fn(&Path, u64) -> Result<PrChecks, String>,
    merge: &dyn Fn(&Path, u64) -> Result<(), String>,
) -> PrMergeReport {
    let MergedPr { spec, verdict, checks_word } = match merge_or_ask(principal, facts, flow, confirmed, checks, merge) {
        Ok(merged) => merged,
        Err(report) => return *report,
    };
    let lang = mustard_core::ProjectConfig::load(principal).language().text_or_default();
    let found = spec.as_deref().and_then(|slug| {
        let view = provider_for(principal).view(spec_pr(principal, slug)?.as_ref()).ok()?;
        submodules_landed(principal, slug, &view)
    });
    PrMergeReport {
        ok: found.as_ref().is_none_or(|found| found.problem.is_none()),
        action: "merged",
        reason: None,
        pr: facts.number,
        head: facts.head.clone(),
        spec,
        verdict,
        checks: checks_word,
        warning: None,
        settle: None,
        hint: found.as_ref().and_then(|found| found.text(lang)),
        pending_closed: None,
        pending_open: None,
        submodules: found,
    }
}


/// O que um pull request que entrou deixa, pelo merge desta porta ou pelas
/// mãos de outra pessoa: o fechamento gravado ([`after_merge`]) e, numa
/// unidade, a arrumação.
struct Landed {
    /// A pendência que virou a spec, fechada por este merge.
    pending_closed: Option<String>,
    /// As pendências nascidas na spec que seguem abertas: a entrega pergunta
    /// só delas.
    pending_open: Vec<OpenPending>,
    /// O relatório da arrumação; `None` numa promoção de base para base, que
    /// não tem unidade para arrumar.
    settle: Option<Value>,
}

/// O caminho único de um pull request que entrou: o merge desta porta e o
/// merge feito por outra pessoa, que o início da sessão encontra, passam os
/// dois por aqui.
///
/// O fechamento se grava ANTES da arrumação, nos dois casos: a promoção também
/// é um pull request mergeado.
///
/// **A promotion has no unit, so it has nothing to settle.** `dev` → `main`
/// is the ordinary end of a cycle and its HEAD is a declared BASE; handing
/// that name to the prune asks it to delete the project's own integration
/// branch, here and on the server. The prune refuses this too, but the refusal
/// is the second line of defence: this path knows it is promoting and must not
/// ask.
///
/// The rest — back to the base, pull it, remove the worktree, delete the local
/// branch, and the remote one only with `git.deleteRemoteBranch` — IS
/// `git-settle`, called rather than rewritten: it already verifies the merge
/// landed, already advances every base and already handles the in-place unit
/// that has no worktree to leave.
fn land(
    root: &Path,
    facts: &PrFacts,
    spec: Option<&str>,
    flow: &BaseFlow,
    settle: &dyn Fn(&Path, &str) -> Value,
    session: Option<&str>,
) -> Landed {
    let (pending_closed, pending_open) = after_merge(root, facts, spec, session);
    let settle = (!is_base_promotion(&facts.head, flow)).then(|| settle(root, &facts.head));
    Landed { pending_closed, pending_open, settle }
}

/// O pull request da spec atual, perguntado no início da sessão.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MergedElsewhere {
    /// Entrou pelas mãos de outra pessoa, e o mesmo caminho do merge desta
    /// porta rodou: a spec gravada como entregue, a arrumação da branch e a
    /// pergunta das pendências nascidas na spec.
    Landed {
        /// O número do pull request.
        pr: u64,
        /// A branch da unidade.
        branch: String,
        /// O relatório da arrumação, ou nada numa promoção.
        settle: Option<Value>,
        /// As pendências nascidas na spec que seguem abertas.
        pending_open: Vec<OpenPending>,
    },
    /// O provedor não respondeu, com o motivo que ele deu; nada foi mudado.
    Unanswered { reason: String },
    /// O pull request do principal segue aberto, e a spec mexe em
    /// submódulo: o que a conferência dos pull requests deles achou e fez.
    Submodules(SubmodulePrs),
}

/// No início da sessão, com a spec `spec` em "pull request aberto": pergunta
/// ao provedor só pelo pull request dela — o número gravado quando ele abriu
/// ou, sem o número, a branch da spec — e, se ele entrou, roda o mesmo caminho
/// do merge desta porta ([`land`]). A arrumação pergunta ao provedor só por
/// essa unidade; as outras ficam com o que o git prova.
///
/// Uma pergunta por branch, num repositório com as branches dos colegas,
/// passou do prazo do início da sessão; por isso é um pull request só.
///
/// `None` quando a spec não está em "pull request aberto", quando não se sabe
/// qual é o pull request dela, e quando o provedor responde que ele segue
/// aberto ou foi fechado sem merge.
pub(crate) fn merged_elsewhere(root: &Path, spec: &str, session: Option<&str>) -> Option<MergedElsewhere> {
    use mustard_core::domain::spec_state::{SpecState as _, State};

    let repo = project_root(root);
    let log = crate::shared::spec_state::DiskSpecState::new(&repo).log(spec)?;
    let state = State::from_log(&log);
    if state.phase != Some("pr_open") {
        return None;
    }
    let asked = spec_pr(&repo, spec)?;
    let view = match provider_for(&repo).view(asked.as_ref()) {
        Ok(view) => view,
        Err(reason) => return Some(MergedElsewhere::Unanswered { reason }),
    };
    match view.status {
        PrStatus::Merged => {}
        PrStatus::Unknown(reason) => return Some(MergedElsewhere::Unanswered { reason: reason.to_string() }),
        // Com o principal aberto, os pull requests dos submódulos da spec são
        // conferidos: o que entrou leva o ponteiro ao principal, e o principal
        // fica pronto quando nenhum falta.
        PrStatus::Open => {
            let lang = mustard_core::ProjectConfig::load(&repo).language().text_or_default();
            return submodules_landed(&repo, spec, &view)
                .filter(|found| found.text(lang).is_some())
                .map(MergedElsewhere::Submodules);
        }
        PrStatus::Closed | PrStatus::Absent => return None,
    }
    let head = if view.head.is_empty() { state.branch.unwrap_or_default() } else { view.head };
    if head.is_empty() {
        return None;
    }
    let facts = PrFacts { number: view.number, head };
    let (flow, _) = bases_and_branch(&repo);
    let settle = |r: &Path, branch: &str| settle_unit_at(r, branch);
    let landed = land(&repo, &facts, Some(spec), &flow, &settle, session);
    Some(MergedElsewhere::Landed {
        pr: facts.number,
        branch: facts.head,
        settle: landed.settle,
        pending_open: landed.pending_open,
    })
}

/// What a merge leaves recorded besides the merge: the `pr.merged` event, the
/// spec's `delivered` phase and the closing of the pending item that became
/// the spec. Returns the closed id (if any) and the pending items born in the
/// spec that stay open: the delivery asks only about them, never about the
/// whole list.
///
/// **The phase goes through the spec file's own door, and that is the point.**
/// Writing `delivered` by hand here would record the fact and leave the pending
/// charge unarmed — which is exactly what happened while nothing recorded the
/// phase at all: a merge delivered the spec and the end of the answer asked
/// about nothing. The door that writes the phase is the door that arms the
/// charge, so the two can never come apart again.
///
/// The reason `PR #N mergeado` puts the pull request number in the ledger, so
/// whoever rereads the list knows what delivered the item.
///
/// Also runs on the `dev` → `main` promotion: the `pr.merged` is recorded. A
/// promotion has no spec, so it records no phase, arms no charge and asks
/// about no pending item.
fn after_merge(
    root: &Path,
    facts: &PrFacts,
    spec: Option<&str>,
    session: Option<&str>,
) -> (Option<String>, Vec<OpenPending>) {
    record_merge(root, facts, spec, session);
    if let Some(slug) = spec.map(str::trim).filter(|s| !s.is_empty()) {
        crate::commands::spec_events::write::record_phase(root, slug, "delivered", session);
    }
    let reason = format!("PR #{} mergeado", facts.number);
    // The note "became the spec X" in the list links the pending item to the
    // spec.
    let closed = spec
        .and_then(|slug| became_of(root, slug))
        .filter(|id| close_pending(root, id, &reason));
    let born = spec.map(|slug| open_pending_born_in(root, slug)).unwrap_or_default();
    (closed, born)
}

/// Records this door's `pr.merged`, and nothing else — the phase is
/// [`after_merge`]'s next step, through the spec file's own door. `pr_detect`
/// records the event of a `gh pr merge` typed in Bash, the same way.
///
/// Declared effect: the event also feeds `pr_metrics` — the merge count when
/// git does not answer and the opened → merged pairing. The merges made by
/// this door, invisible there before, start counting. They do not count
/// twice: `pr_detect` only records the `gh pr merge` typed in Bash.
fn record_merge(_root: &Path, _facts: &PrFacts, _spec: Option<&str>, _session: Option<&str>) {}

/// Ask the provider to merge. The strategy is explicit because it has to be: a
/// bare `gh pr merge` opens an interactive picker, and a `run`-face command has
/// no stdin to answer it with. A merge commit is what this project's history is
/// made of, so that is what is asked for.
fn gh_merge(root: &Path, pr: u64) -> Result<(), String> {
    gh_out(root, &["pr", "merge", &pr.to_string(), "--merge"]).map(|_| ())
}

// ---------------------------------------------------------------------------
// CLI faces — resolve the root, build the report, print it
// ---------------------------------------------------------------------------

/// Print one report as the single JSON document the command answers with.
fn emit<T: Serialize>(report: &T) {
    println!("{}", serde_json::to_string_pretty(report).unwrap_or_else(|_| "{}".to_string()));
}


/// Dispatch `mustard-rt run pr-review`. The brief still answers; recording a
/// verdict has left the flow, so `--verdict` refuses at the door with exit 1,
/// records nothing and says to wait for the round.
pub fn run_review(root: &Path, pr: Option<u64>, verdict: Option<&str>) {
    let repo = project_root(root);
    if verdict.is_some() {
        crate::commands::retired::refuse(
            &repo,
            "wait-for-round",
            "retired.wait_round",
            &[("{command}", "pr-review --verdict")],
        );
    }
    // Sem número, o comando LISTA os pull requests abertos em vez de adivinhar
    // um. Ele existe para revisar o de um colega, e o colega não está na branch
    // desta máquina: resolver pela branch do checkout devolvia o pull request
    // de quem chamou, que é justamente o que ninguém pediu.
    let Some(number) = pr else {
        emit(&list_at(&repo));
        return;
    };
    match resolve_pr(&repo, Some(number)) {
        Ok(facts) => {
            let (flow, _) = bases_and_branch(&repo);
            emit(&review_brief(&repo, &facts, &flow));
        }
        Err(e) => emit(&serde_json::json!({ "ok": false, "reason": e, "pr": pr })),
    }
}

/// Dispatch `mustard-rt run pr-merge`: the merge and the tidying up in one
/// call, the delivery recorded and the pending item that became the spec
/// closed.
pub fn run_merge(root: &Path, pr: Option<u64>, confirm: bool) {
    let started = std::time::Instant::now();
    let repo = project_root(root);
    // De dentro de um submódulo do projeto, o pull request é o do submódulo:
    // o veredito vem da spec no principal, e depois do merge o principal
    // recebe o ponteiro.
    if let Some(principal) = superproject_of(&repo).filter(|parent| mustard_core::ProjectConfig::exists(parent)) {
        let report = match resolve_pr(&repo, pr) {
            Ok(facts) => {
                let (flow, _) = bases_and_branch(&principal);
                let checks = |_: &Path, number: u64| provider_in(&principal, &repo).checks(number);
                let merge = |_: &Path, number: u64| gh_merge(&repo, number);
                let merged = merge_submodule(&principal, &facts, &flow, confirm, &checks, &merge);
                serde_json::to_value(&merged).unwrap_or_default()
            }
            Err(e) => serde_json::json!({ "ok": false, "reason": e, "pr": pr }),
        };
        let _ = crate::commands::spec_events::conversation::record_call(&principal, "pr-merge", None, started, &report);
        emit(&report);
        return;
    }
    let report = match resolve_pr(&repo, pr) {
        Ok(facts) => {
            let (flow, _) = bases_and_branch(&repo);
            let settle = |r: &Path, branch: &str| settle_at(r, Some(branch));
            // Through the PORT, not through `gh`: the question "did YOUR runs
            // finish?" is the same question on every provider, and the door
            // must not learn a second provider's vocabulary to ask it.
            let checks = |r: &Path, number: u64| provider_for(r).checks(number);
            let session = crate::shared::spec_state::session_from_env();
            let merged = merge_core(&repo, &facts, &flow, confirm, &checks, &gh_merge, &settle, session.as_deref());
            serde_json::to_value(&merged).unwrap_or_default()
        }
        Err(e) => serde_json::json!({ "ok": false, "reason": e, "pr": pr }),
    };
    let _ = crate::commands::spec_events::conversation::record_call(&repo, "pr-merge", None, started, &report);
    emit(&report);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use serde_json::json;
    use tempfile::tempdir;

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git").args(args).current_dir(dir).output().expect("spawn git");
        assert!(out.status.success(), "git {args:?} failed");
    }

    /// A repo with `git.flow` declaring `dev` (primary) and `main`, sitting on
    /// `dev`. No remote, no `gh` — the base gate is answered from local state
    /// alone, which is the whole point of testing it here.
    fn repo() -> tempfile::TempDir {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        git(root, &["init", "."]);
        git(root, &["config", "user.email", "t@t"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["checkout", "-b", "dev"]);
        std::fs::write(root.join("mustard.json"), r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#)
            .expect("cfg");
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "seed"]);
        dir
    }

    /// `pr list` from a work branch REFUSES and names the base to switch
    /// to; from the base it does not refuse (whatever the provider answers).
    #[test]
    fn pr_list_refuses_off_an_integration_base_and_names_it() {
        let dir = repo();
        let root = dir.path();

        // On the base: the gate passes. `gh` is absent/unauthenticated in the
        // test environment, which is reported as `gh_error` — never as a
        // refusal, because the checkout was fine.
        let on_base = list_at(root);
        assert!(on_base.ok, "the base itself is never refused: {:?}", on_base.reason);
        assert_eq!(on_base.reason, None);
        assert_eq!(on_base.branch, "dev");
        assert!(on_base.bases.contains(&"dev".to_string()), "bases: {:?}", on_base.bases);

        // On a work branch: refused, and the refusal NAMES the base. What makes
        // it a work branch is the project's RECORD of the unit, not the shape of
        // the name — the fixture used to create only the branch, so this case
        // was satisfied by anything that merely looked like a unit, which is how
        // a real release line ended up being refused here.
        git(root, &["checkout", "-b", "dev_some-unit"]);
        std::fs::create_dir_all(root.join(".claude").join("spec").join("some-unit"))
            .expect("unit record");
        let off_base = list_at(root);
        assert!(!off_base.ok, "a work branch must be refused");
        assert_eq!(off_base.reason, Some("not-on-integration-base"));
        assert_eq!(off_base.branch, "dev_some-unit");
        assert!(off_base.prs.is_empty(), "a refusal lists nothing");
        let hint = off_base.hint.unwrap_or_default();
        assert!(hint.contains("dev"), "the refusal must name the base: {hint}");

        // A branch that is NOBODY's unit is not refused any more. It used to
        // be, for the sole reason that `git.flow` does not list it — and the
        // installer writes no flow, so that refusal fired on every branch a
        // real project integrates through. The question here is whether the
        // checkout is inside a unit, and this one is not.
        git(root, &["checkout", "-b", "loose-branch"]);
        let loose = list_at(root);
        assert_eq!(loose.reason, None, "an undeclared base is still a base");
        assert!(loose.ok, "nothing about the checkout refuses: {:?}", loose.hint);
    }

    /// The rows survive the trip from `gh` shape to report shape, sorted, with
    /// the provider's own mergeable word kept verbatim.
    #[test]
    fn pr_list_rows_keep_the_provider_word_and_sort_by_number() {
        let rows = json!([
            {"number": 9, "title": "later", "mergeable": "UNKNOWN", "isDraft": true, "headRefName": "dev_b"},
            {"number": 2, "title": "earlier", "mergeable": "MERGEABLE", "isDraft": false, "headRefName": "dev_a"},
            {"number": null, "title": "not a pr"},
        ]);
        let mut got: Vec<PrEntry> =
            rows.as_array().map(|a| a.iter().filter_map(pr_entry).collect()).unwrap_or_default();
        got.sort_by_key(|p| p.number);
        assert_eq!(
            got,
            vec![
                PrEntry {
                    number: 2,
                    title: "earlier".into(),
                    mergeable: "MERGEABLE".into(),
                    draft: false,
                    head: "dev_a".into(),
                },
                PrEntry {
                    number: 9,
                    title: "later".into(),
                    mergeable: "UNKNOWN".into(),
                    draft: true,
                    head: "dev_b".into(),
                },
            ],
            "numbered rows only, sorted, provider word verbatim"
        );
    }

    /// A merge requested with NO recorded review verdict warns and asks:
    /// it does not refuse (`ok` stays true) and it does not merge (neither
    /// injected effect runs). `--confirm` is the answer coming back.
    #[test]
    fn pr_merge_without_verdict_warns_and_asks_instead_of_merging() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        let bases = door_flow();
        let facts = PrFacts { number: 42, head: "dev_unreviewed".to_string() };

        let merges = Cell::new(0u32);
        let settles = Cell::new(0u32);
        let merge = |_: &Path, _: u64| {
            merges.set(merges.get() + 1);
            Ok(())
        };
        let settle = |_: &Path, _: &str| {
            settles.set(settles.get() + 1);
            json!({ "ok": true })
        };
        // The provider is green here, so the ONLY thing left to ask about is
        // the missing verdict.
        let green = |_: &Path, _: u64| Ok(PrChecks::Passed);

        let asked = merge_core(root, &facts, &bases, false, &green, &merge, &settle, None);
        assert!(asked.ok, "an ASK is an instruction, never a failure");
        assert_eq!(asked.action, "confirm");
        assert_eq!(asked.reason, Some("no-review-verdict"));
        assert_eq!(asked.verdict, None);
        assert_eq!(asked.checks, "passed", "the provider's own answer is reported either way");
        assert!(asked.settle.is_none(), "nothing was pruned");
        assert!(
            asked.warning.unwrap_or_default().contains("unreviewed"),
            "the warning names the unit"
        );
        assert_eq!(merges.get(), 0, "nothing may be merged without an answer");
        assert_eq!(settles.get(), 0, "nothing may be pruned without an answer");

        // The operator's answer comes back as `--confirm`: now it merges and
        // settles. Still not a refusal at any point.
        let confirmed = merge_core(root, &facts, &bases, true, &green, &merge, &settle, None);
        assert!(confirmed.ok);
        assert_eq!(confirmed.action, "merged");
        assert_eq!(merges.get(), 1);
        assert_eq!(settles.get(), 1);
        assert_eq!(confirmed.settle, Some(json!({ "ok": true })));
    }

    /// A base→base promotion is merged and NEVER handed to the prune.
    ///
    /// `dev` → `main` is the ordinary end of a cycle, and the pull request's
    /// HEAD is then a declared BASE. The prune deletes whatever branch it is
    /// handed, here and on the server, so passing `dev` to it asks for the
    /// project's own integration branch to be destroyed. `spec_of_branch`
    /// already answers `None` for such a head — that answer was read for the
    /// verdict and then dropped.
    ///
    /// The assertion is on the CALL COUNT, not on the report: a version that
    /// called the prune and merely reported nicely would satisfy any check of
    /// the JSON alone.
    #[test]
    fn a_base_to_base_promotion_is_merged_but_never_pruned() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        let bases = door_flow();
        // The head IS a base — this is what a promotion looks like to this door.
        let facts = PrFacts { number: 231, head: "dev".to_string() };

        let merges = Cell::new(0u32);
        let settles = Cell::new(0u32);
        let merge = |_: &Path, _: u64| {
            merges.set(merges.get() + 1);
            Ok(())
        };
        let settle = |_: &Path, _: &str| {
            settles.set(settles.get() + 1);
            json!({ "ok": true })
        };

        let green = |_: &Path, _: u64| Ok(PrChecks::Passed);
        let done = merge_core(root, &facts, &bases, true, &green, &merge, &settle, None);

        assert!(done.ok, "a promotion is a success, not a refusal: {done:?}");
        assert_eq!(done.action, "merged");
        assert_eq!(done.reason, Some("base-to-base-promotion"));
        assert_eq!(merges.get(), 1, "the promotion IS merged");
        assert_eq!(settles.get(), 0, "and the prune is never even asked");
        assert!(done.settle.is_none(), "so there is no settle report to carry: {done:?}");
    }

    /// A promoção entre bases não pede veredito de revisão.
    ///
    /// Levar `dev` para `main` não é obra de ninguém: não há spec, não há
    /// revisão e o veredito que o portão cobrava nunca poderia existir. Sem
    /// `--confirm`, o portão reconhece a rota antes de decidir e o merge
    /// acontece sem pergunta nenhuma — antes ele respondia `confirm` com o
    /// motivo `no-review-verdict`, e quem usava aprendia a confirmar sem ler.
    ///
    /// A pergunta que continua de pé: as verificações do provedor. Elas são um
    /// resultado que a promoção pode mesmo ter, e uma que ainda está correndo
    /// segura o merge como sempre segurou. As contagens provam o efeito real,
    /// não o texto do relatório.
    #[test]
    fn a_promocao_entre_bases_nao_pede_veredito() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        let bases = door_flow();
        // O head É uma base declarada: é assim que a promoção chega à porta.
        let promotion = PrFacts { number: 231, head: "dev".to_string() };

        let merges = Cell::new(0u32);
        let settles = Cell::new(0u32);
        let merge = |_: &Path, _: u64| {
            merges.set(merges.get() + 1);
            Ok(())
        };
        let settle = |_: &Path, _: &str| {
            settles.set(settles.get() + 1);
            json!({ "ok": true })
        };
        let green = |_: &Path, _: u64| Ok(PrChecks::Passed);

        let done = merge_core(root, &promotion, &bases, false, &green, &merge, &settle, None);
        assert_eq!(done.action, "merged", "sem `--confirm` a promoção passa: {done:?}");
        assert_eq!(done.reason, Some("base-to-base-promotion"));
        assert_eq!(done.verdict, None, "a rota não tem veredito a ter");
        assert_eq!(merges.get(), 1, "a promoção foi mesmo merjada sem pergunta");
        assert_eq!(settles.get(), 0, "e a poda segue fora da rota");

        // A mesma porta, com uma unidade de verdade no head e sem veredito
        // gravado, continua perguntando: o que saiu foi a pergunta impossível,
        // não a regra.
        let unit = PrFacts { number: 232, head: "dev_unreviewed".to_string() };
        let asked = merge_core(root, &unit, &bases, false, &green, &merge, &settle, None);
        assert_eq!(asked.action, "confirm");
        assert_eq!(asked.reason, Some("no-review-verdict"));
        assert_eq!(merges.get(), 1, "a unidade sem veredito não foi merjada");

        // E a proteção que resta na promoção: uma verificação do provedor ainda
        // correndo segura o merge, mesmo sem veredito nenhum em jogo.
        let running = |_: &Path, _: u64| Ok(PrChecks::Running);
        let held = merge_core(root, &promotion, &bases, false, &running, &merge, &settle, None);
        assert_eq!(held.action, "confirm");
        assert_eq!(held.reason, Some("provider-checks-running"));
        assert_eq!(merges.get(), 1, "nada foi merjado com a verificação em curso");
    }

    /// The consent rule itself, over BOTH sources of evidence: only an
    /// `approved` verdict on top of provider checks that are not in the way
    /// (or the operator's own answer) proceeds. Everything else ASKS, and
    /// nothing ever produces a refusal.
    #[test]
    fn pr_merge_consent_asks_for_anything_but_approved() {
        let green = Ok(PrChecks::Passed);
        assert_eq!(merge_consent(Some("approved"), &green, false, false), MergeConsent::Proceed);
        assert_eq!(merge_consent(None, &green, true, false), MergeConsent::Proceed);
        assert_eq!(merge_consent(Some("rejected"), &green, true, false), MergeConsent::Proceed);
        assert_eq!(
            merge_consent(None, &green, false, false),
            MergeConsent::Ask { reason: "no-review-verdict" }
        );
        assert_eq!(
            merge_consent(Some("rejected"), &green, false, false),
            MergeConsent::Ask { reason: "review-not-approved" }
        );

        // The provider's own checks, over an approved verdict: only a finished
        // green (or a measured absence of runs) lets the merge through.
        let asks = |checks: Result<PrChecks, String>, reason: &'static str| {
            assert_eq!(
                merge_consent(Some("approved"), &checks, false, false),
                MergeConsent::Ask { reason },
                "for {checks:?}",
            );
            assert_eq!(
                merge_consent(Some("approved"), &checks, true, false),
                MergeConsent::Proceed,
                "`--confirm` is the one way through, for {checks:?}",
            );
        };
        assert_eq!(
            merge_consent(Some("approved"), &Ok(PrChecks::Absent), false, false),
            MergeConsent::Proceed,
            "a project with no CI measured zero runs — that is an answer",
        );
        asks(Ok(PrChecks::Running), "provider-checks-running");
        asks(Ok(PrChecks::Failed), "provider-checks-failed");
        asks(Ok(PrChecks::Unknown("pr-unreadable")), "provider-checks-unreadable");
        asks(Err("gh-not-found".to_string()), "provider-checks-unreadable");
    }

    /// The run that was still in flight when PR 237 was merged. With an
    /// APPROVED verdict recorded (so the review half consents), a provider
    /// whose checks are still running stops the merge dead: the door asks, and
    /// neither injected effect is called.
    ///
    /// The assertion is on the CALL COUNT, not on the JSON: a version that
    /// merged and merely reported the pending run would satisfy any check of
    /// the document alone — and that is exactly the bug this AC pins.
    #[test]
    fn pr_merge_does_not_merge_while_provider_checks_run() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        let bases = door_flow();
        let facts = PrFacts { number: 237, head: "dev_still-running".to_string() };
        let criteria = crate::shared::spec_state::seed_runs(root, "still-running", &[None]);
        crate::shared::spec_state::seed_verdict(root, "still-running", 1, "approved", criteria[0]);

        let merges = Cell::new(0u32);
        let settles = Cell::new(0u32);
        let merge = |_: &Path, _: u64| {
            merges.set(merges.get() + 1);
            Ok(())
        };
        let settle = |_: &Path, _: &str| {
            settles.set(settles.get() + 1);
            json!({ "ok": true })
        };
        let running = |_: &Path, _: u64| Ok(PrChecks::Running);

        let asked = merge_core(root, &facts, &bases, false, &running, &merge, &settle, None);
        assert!(asked.ok, "an ASK is an instruction, never a failure: {asked:?}");
        assert_eq!(asked.action, "confirm");
        assert_eq!(asked.reason, Some("provider-checks-running"));
        assert_eq!(asked.verdict.as_deref(), Some("approved"), "the review half DID consent");
        assert_eq!(asked.checks, "running");
        assert_eq!(merges.get(), 0, "nothing may be merged while the provider is still deciding");
        assert_eq!(settles.get(), 0, "and nothing may be pruned");
        assert!(asked.settle.is_none());

        // `--confirm` remains the operator's deliberate way through the gate.
        let confirmed = merge_core(root, &facts, &bases, true, &running, &merge, &settle, None);
        assert_eq!(confirmed.action, "merged");
        assert_eq!(merges.get(), 1);
        assert_eq!(confirmed.checks, "running", "the override is recorded, not hidden");
    }

    /// Checks that came back FAILING do not merge either. Same one
    /// rule, its own reason: the operator's next move is to fix, not to wait.
    #[test]
    fn pr_merge_refuses_when_provider_checks_failed() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        let bases = door_flow();
        let facts = PrFacts { number: 238, head: "dev_red-ci".to_string() };
        let criteria = crate::shared::spec_state::seed_runs(root, "red-ci", &[None]);
        crate::shared::spec_state::seed_verdict(root, "red-ci", 1, "approved", criteria[0]);

        let merges = Cell::new(0u32);
        let settles = Cell::new(0u32);
        let merge = |_: &Path, _: u64| {
            merges.set(merges.get() + 1);
            Ok(())
        };
        let settle = |_: &Path, _: &str| {
            settles.set(settles.get() + 1);
            json!({ "ok": true })
        };
        let failed = |_: &Path, _: u64| Ok(PrChecks::Failed);

        let asked = merge_core(root, &facts, &bases, false, &failed, &merge, &settle, None);
        assert_eq!(asked.action, "confirm");
        assert_eq!(asked.reason, Some("provider-checks-failed"));
        assert_eq!(asked.checks, "failed");
        assert_eq!(merges.get(), 0, "a failing tree is never integrated by this door");
        assert_eq!(settles.get(), 0);
        // "did not come back green", never "FAILING": `Failed` also absorbs a
        // CANCELLED run, which this repository produces on every re-push
        // (`concurrency: cancel-in-progress`). The word has to cover both or it
        // sends the operator hunting a failure that never happened.
        assert!(
            asked
                .warning
                .unwrap_or_default()
                .contains("did not come back green"),
            "the warning says which of the two evidences refused",
        );

        // An unreadable answer takes the same branch: "the provider could not
        // be asked" is not evidence that its runs passed.
        let unreadable = |_: &Path, _: u64| Err("gh-not-found".to_string());
        let blind = merge_core(root, &facts, &bases, false, &unreadable, &merge, &settle, None);
        assert_eq!(blind.action, "confirm");
        assert_eq!(blind.reason, Some("provider-checks-unreadable"));
        assert_eq!(blind.checks, "gh-not-found", "the reason travels verbatim into the report");
        assert_eq!(merges.get(), 0);
    }

    /// O merge lê o veredito do `spec.ndjson` com a mesma regra do
    /// fechamento. A reprovação de uma onda faz o merge perguntar antes de
    /// juntar; a aprovação final da obra, gravada pelo agente de teste
    /// dedicado depois da entrega do conserto, quita essa reprovação, e o
    /// merge segue sem perguntar — é a mesma aprovação que deixou a obra
    /// fechar. Uma reprovação gravada DEPOIS dessa aprovação volta a fazer o
    /// merge perguntar: nada a quitou ainda.
    ///
    /// Em 20/09 o merge do pedido 281 parou para uma confirmação manual por
    /// causa da reprovação de uma onda antiga que a aprovação final da obra
    /// já tinha quitado.
    #[test]
    fn the_final_approval_of_the_work_pays_off_an_older_wave_rejection() {
        use crate::shared::spec_state::{seed_event, seed_runs, seed_verdict};
        use mustard_core::domain::spec_state::{final_approval, SpecState as _};
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        assert_eq!(recorded_verdict(root, "unit-a"), None, "nothing recorded yet");
        // A leitura do fechamento, ao lado da do portão: as duas vêem a mesma
        // quitação, ou nenhuma.
        let closing = || {
            let log = crate::shared::spec_state::DiskSpecState::new(root).log("unit-a").expect("the spec file");
            final_approval(&log).map(|event| event.id)
        };

        let criteria = seed_runs(root, "unit-a", &[None]);
        let delivery = |wave: u64| {
            let line = json!({ "wave": wave, "text": "a onda saiu", "files": ["src/unit.rs"] });
            seed_event(root, "unit-a", "delivered", line)
        };
        delivery(1);
        delivery(2);
        seed_verdict(root, "unit-a", 1, "rejected", criteria[0]);
        assert_eq!(recorded_verdict(root, "unit-a").as_deref(), Some("rejected"), "a onda 1 reprovou");
        assert_eq!(closing(), None, "sem conserto, o fechamento também não vê quitação");

        let merges = Cell::new(0u32);
        let merge = |_: &Path, _: u64| {
            merges.set(merges.get() + 1);
            Ok(())
        };
        let settle = |_: &Path, _: &str| json!({ "ok": true });
        let green = |_: &Path, _: u64| Ok(PrChecks::Passed);
        let facts = PrFacts { number: 281, head: "dev_unit-a".to_string() };
        let door = || merge_core(root, &facts, &door_flow(), false, &green, &merge, &settle, None);
        let asked = door();
        assert_eq!((asked.action, asked.reason), ("confirm", Some("review-not-approved")), "{asked:?}");
        assert_eq!(merges.get(), 0, "a onda reprovada nunca é juntada sem perguntar");

        // O conserto sai pela rodada e o agente de teste dedicado aprova a
        // obra: a aprovação fica na última onda, como o fechamento a grava.
        delivery(1);
        let quittance = seed_verdict(root, "unit-a", 2, "approved", criteria[0]);
        assert_eq!(closing(), Some(quittance), "o fechamento lê a aprovação final");
        assert_eq!(recorded_verdict(root, "unit-a").as_deref(), Some("approved"), "e o portão lê a mesma");
        let merged = door();
        assert_eq!(merged.action, "merged", "a reprovação quitada não pede confirmação: {merged:?}");
        assert_eq!(merges.get(), 1);

        // A reprovação que chega depois da aprovação final ainda barra.
        seed_verdict(root, "unit-a", 1, "rejected", criteria[0]);
        assert_eq!(recorded_verdict(root, "unit-a").as_deref(), Some("rejected"));
        let again = door();
        assert_eq!((again.action, again.reason), ("confirm", Some("review-not-approved")), "{again:?}");
        assert_eq!(merges.get(), 1, "e nada mais foi juntado");
    }

    /// A obra sem onda nenhuma — a de até três pontos, que o orquestrador faz
    /// — fecha com a aprovação final do agente de teste dedicado, que não
    /// aponta onda nenhuma. O portão do merge lê essa aprovação como
    /// veredito da obra, em vez de perguntar por falta de veredito.
    #[test]
    fn the_merge_reads_the_final_approval_of_a_work_with_no_waves() {
        use crate::shared::spec_state::seed_event;
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        seed_event(root, "unit-b", "delivered", json!({ "wave": 1, "text": "a obra saiu", "files": ["src/unit.rs"] }));
        assert_eq!(recorded_verdict(root, "unit-b"), None, "sem veredito nenhum");

        let merges = Cell::new(0u32);
        let merge = |_: &Path, _: u64| {
            merges.set(merges.get() + 1);
            Ok(())
        };
        let settle = |_: &Path, _: &str| json!({ "ok": true });
        let green = |_: &Path, _: u64| Ok(PrChecks::Passed);
        let facts = PrFacts { number: 282, head: "dev_unit-b".to_string() };
        let door = || merge_core(root, &facts, &door_flow(), false, &green, &merge, &settle, None);
        let asked = door();
        assert_eq!((asked.action, asked.reason), ("confirm", Some("no-review-verdict")), "{asked:?}");
        assert_eq!(merges.get(), 0);

        seed_event(
            root,
            "unit-b",
            "verdict",
            json!({ "final": true, "result": "approved", "text": "a obra está pronta" }),
        );
        assert_eq!(recorded_verdict(root, "unit-b").as_deref(), Some("approved"));
        let merged = door();
        assert_eq!(merged.action, "merged", "{merged:?}");
        assert_eq!(merges.get(), 1);
    }

    /// A `dev`/`main` flow project with the open pending items `titles`,
    /// numbered in order.
    fn project_with_items(titles: &[&str]) -> tempfile::TempDir {
        use crate::commands::event::pending::{pending_at, PendingOpts};
        let dir = tempdir().expect("tempdir");
        std::fs::write(dir.path().join("mustard.json"), r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#)
            .expect("cfg");
        for title in titles {
            let out = pending_at(&PendingOpts {
                root: dir.path().to_path_buf(),
                add: true,
                title: Some((*title).to_string()),
                detail: Some("combinado".to_string()),
                ..PendingOpts::default()
            });
            assert_eq!(out["ok"], json!(true), "seed: {out}");
        }
        dir
    }

    /// A merge with no checks, no provider and no pruning, through the real door.
    fn merged(root: &Path, number: u64, head: &str) -> PrMergeReport {
        let green = |_: &Path, _: u64| Ok(PrChecks::Passed);
        let merge = |_: &Path, _: u64| Ok(());
        let settle = |_: &Path, _: &str| json!({ "ok": true });
        let facts = PrFacts { number, head: head.to_string() };
        merge_core(root, &facts, &door_flow(), true, &green, &merge, &settle, None)
    }

    /// The delivery asks only about the pending items born in the merge's
    /// spec; the promotion, which has no spec, asks about none.
    #[test]
    fn the_merge_reports_only_the_items_born_in_its_spec() {
        use crate::hooks::task::pending_gate::seed_spec;
        let dir = project_with_items(&["Humanize", "HTML padrao", "Painel"]);
        let root = dir.path();
        seed_spec(root, "trava", &[2], "s-entrega");

        let done = merged(root, 310, "feature/trava");
        assert_eq!(done.action, "merged");
        assert_eq!(done.pending_open, Some(vec![OpenPending { id: "P-2".into(), title: "HTML padrao".into() }]));
        let promotion = merged(root, 311, "dev");
        assert_eq!(promotion.reason, Some("base-to-base-promotion"));
        assert_eq!(promotion.pending_open, Some(vec![]), "a promotion has no spec to ask about");
    }

    /// O merge entrega a spec e arma a cobrança pela mesma porta.
    ///
    /// As duas metades são uma coisa só, e por isso estão num teste só: a fase
    /// `delivered` é gravada pela porta do arquivo de eventos, e é essa porta
    /// que arma a cobrança das pendências no fim da resposta. Gravar a fase
    /// aqui à mão registraria o fato e deixaria a cobrança desarmada — que foi
    /// exatamente o que aconteceu enquanto ninguém gravava fase nenhuma: o
    /// merge entregava a spec e o fim da resposta não perguntava nada.
    ///
    /// A promoção de base não tem spec: não entrega e não arma.
    #[test]
    fn o_merge_entrega_a_spec_e_arma_a_cobranca_pela_mesma_porta() {
        use crate::commands::event::pending::armed_charges;
        use crate::hooks::task::pending_gate::seed_spec;
        let dir = project_with_items(&["Humanize", "HTML padrao"]);
        let root = dir.path();
        seed_spec(root, "trava", &[2], "s-entrega");
        assert!(armed_charges(root).is_empty(), "nada armado antes do merge");

        let green = |_: &Path, _: u64| Ok(PrChecks::Passed);
        let merge = |_: &Path, _: u64| Ok(());
        let settle = |_: &Path, _: &str| json!({ "ok": true });
        let facts = PrFacts { number: 330, head: "feature/trava".to_string() };
        let done = merge_core(
            root,
            &facts,
            &door_flow(),
            true,
            &green,
            &merge,
            &settle,
            Some("s-entrega"),
        );
        assert_eq!(done.action, "merged", "{done:?}");

        let events = std::fs::read_to_string(
            mustard_core::io::spec_events::spec_file(root, "trava").expect("caminho"),
        )
        .expect("arquivo de eventos");
        assert!(events.contains("\"delivered\""), "a spec não ficou entregue: {events}");

        let armed = armed_charges(root);
        assert_eq!(armed.len(), 1, "a cobrança não foi armada: {armed:?}");
        assert_eq!(armed[0].spec, "trava", "{armed:?}");

        // A promoção de base não tem spec: nada a entregar, nada a armar.
        let promotion = PrFacts { number: 331, head: "dev".to_string() };
        let _ = merge_core(
            root,
            &promotion,
            &door_flow(),
            true,
            &green,
            &merge,
            &settle,
            Some("s-entrega"),
        );
        assert_eq!(armed_charges(root).len(), 1, "a promoção armou uma cobrança");
    }

    /// The pending item that became the spec gets the note in the list, and
    /// that spec's merge closes it with the pull request number; another
    /// spec's note stays.
    #[test]
    fn a_merge_closes_the_item_that_became_its_spec() {
        use crate::commands::event::pending::{mark_became, pending_at, PendingOpts};
        let dir = project_with_items(&["Humanize", "Painel"]);
        let root = dir.path();
        assert!(mark_became(root, "P-1", "trava"), "the note is recorded");
        assert!(mark_became(root, "P-2", "outra"));
        let list = || pending_at(&PendingOpts { root: root.to_path_buf(), ..PendingOpts::default() });
        assert_eq!(list()["open"][0]["became"], json!("trava"), "the list shows the note");

        let done = merged(root, 320, "feature/trava");
        assert_eq!(done.pending_closed.as_deref(), Some("P-1"), "the item that became the spec is closed");
        let after = list();
        assert_eq!(after["closed"][0]["id"], json!("P-1"), "{after}");
        assert_eq!(after["closed"][0]["reason"], json!("PR #320 mergeado"), "{after}");
        assert_eq!(after["open"][0]["id"], json!("P-2"), "another spec's note stays: {after}");
        assert_eq!(after["open"][0]["became"], json!("outra"));
    }

    /// The whole pending criterion: twelve open, two born in the delivered
    /// spec and three idle for over 30 days. The session start shows one line
    /// with the count; the delivery asks only about the two; the end-of-answer
    /// lock charges only the two; the three idle ones come back in one
    /// question, once, and the unmarked ones leave as expired; and "Humanize"
    /// with an open "humanize" is refused, pointing at the existing one.
    #[test]
    fn twelve_open_items_follow_the_four_brakes() {
        use crate::commands::event::pending::{pending_at, PendingOpts};
        use crate::hooks::task::end_of_turn_check::run_rules;
        use crate::hooks::task::pending_gate::{seed_spec, PendingRule};
        use mustard_core::domain::model::contract::{Ctx, HookInput, Trigger, Verdict};

        let dir = project_with_items(&[]);
        let root = dir.path();
        let add = |title: &str, day: Option<&str>| {
            pending_at(&PendingOpts {
                root: root.to_path_buf(),
                add: true,
                title: Some(title.to_string()),
                detail: Some("combinado".to_string()),
                now: day.map(str::to_string),
                ..PendingOpts::default()
            })
        };
        for n in 1..=3 {
            assert_eq!(add(&format!("parada {n}"), Some("2020-01-01"))["ok"], json!(true));
        }
        for n in 4..=10 {
            assert_eq!(add(&format!("aberta {n}"), None)["ok"], json!(true));
        }
        assert_eq!(add("humanize", None)["id"], json!("P-11"));
        assert_eq!(add("html padrao", None)["id"], json!("P-12"));
        seed_spec(root, "entrega", &[11, 12], "s-doze");
        let lang = mustard_core::ProjectConfig::load(root).language().text_or_default();

        // The session start: one line with the count, and the idle ones counted.
        let notice = crate::hooks::session::session_start_inject::pending_notice(root, lang).expect("twelve open");
        assert!(notice.starts_with("[Mustard] 12 ") && !notice.contains('\n'), "{notice}");
        assert!(notice.contains(" 3 ") && !notice.contains("parada 1"), "{notice}");

        // The duplicate is refused, pointing at the existing one.
        let duplicate = add("Humanize", None);
        assert_eq!(duplicate["reason"], json!("duplicate"), "{duplicate}");
        assert_eq!(duplicate["id"], json!("P-11"));

        // The delivery asks only about the two — and records the delivery
        // itself, through the spec file's own phase door, which is what arms
        // the charge the end of the answer reads below.
        let green = |_: &Path, _: u64| Ok(PrChecks::Passed);
        let merge = |_: &Path, _: u64| Ok(());
        let settle = |_: &Path, _: &str| json!({ "ok": true });
        let facts = PrFacts { number: 400, head: "feature/entrega".to_string() };
        let done =
            merge_core(root, &facts, &door_flow(), true, &green, &merge, &settle, Some("s-doze"));
        assert_eq!(done.action, "merged");
        let asked: Vec<String> = done.pending_open.clone().unwrap_or_default().into_iter().map(|i| i.id).collect();
        assert_eq!(asked, vec!["P-11", "P-12"], "{done:?}");
        let ctx = Ctx::for_test(root.to_string_lossy().into_owned(), Some(Trigger::Stop));
        let stop = HookInput {
            hook_event_name: Some("Stop".to_string()),
            session_id: Some("s-doze".to_string()),
            raw: json!({ "last_assistant_message": "Entrega feita." }),
            ..HookInput::default()
        };
        match run_rules(&[&PendingRule], &stop, &ctx) {
            Verdict::Deny { reason } => {
                assert!(reason.contains("P-11") && reason.contains("P-12"), "{reason}");
                assert!(!reason.contains("\"parada") && !reason.contains("\"aberta"), "only those two: {reason}");
            }
            other => panic!("the delivery charges the two born in it, got {other:?}"),
        }

        // The three idle ones come back in one question, once; the unmarked
        // ones leave as expired.
        let sweep = |stale: bool, keep: Option<&str>| {
            pending_at(&PendingOpts {
                root: root.to_path_buf(),
                stale,
                expire: !stale,
                keep: keep.map(str::to_string),
                ..PendingOpts::default()
            })
        };
        let swept = sweep(true, None);
        assert_eq!(swept["stale"].as_array().map(Vec::len), Some(3), "{swept}");
        assert!(swept["question"].is_string(), "one question: {swept}");
        assert_eq!(sweep(true, None)["stale"], json!([]), "they come back once");
        let expired = sweep(false, Some("P-2"));
        assert_eq!(expired["expired"], json!(["P-1", "P-3"]), "{expired}");
        let reason = mustard_core::translate("pending.expired_reason", lang);
        let gone: Vec<&Value> = expired["closed"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
        assert_eq!(gone.len(), 2, "{expired}");
        assert!(gone.iter().all(|item| item["reason"] == json!(reason)), "{expired}");
    }

    /// The base model of a project declaring the ordinary two-tier flow.
    fn door_flow() -> BaseFlow {
        let mut git = mustard_core::domain::config::GitConfig::default();
        git.flow.insert("*".to_string(), "dev".to_string());
        git.flow.insert("dev".to_string(), "main".to_string());
        BaseFlow::of(&git)
    }

    /// The PR↔unit link: a head named by its kind names its spec, so does one
    /// still in the `{base}_{slug}` shape, and a bare base names none.
    #[test]
    fn spec_of_branch_reads_the_unit_out_of_the_head_ref() {
        let bases = door_flow();
        assert_eq!(spec_of_branch("feature/my-spec", &bases).as_deref(), Some("my-spec"));
        assert_eq!(spec_of_branch("hotfix/login", &bases).as_deref(), Some("login"));
        assert_eq!(spec_of_branch("dev_my-spec", &bases).as_deref(), Some("my-spec"));
        assert_eq!(spec_of_branch("worktree-dev_my-spec", &bases).as_deref(), Some("my-spec"));
        assert_eq!(spec_of_branch("main_hotfix", &bases).as_deref(), Some("hotfix"));
        assert_eq!(spec_of_branch("dev", &bases), None, "a bare base carries no unit");
        assert_eq!(spec_of_branch("feature_x", &bases), None, "a name of neither shape");
    }

    /// The brief points at the spec and hands back the SAME shelf the
    /// implementer got, and records nothing on the way: the verdicts
    /// `pr-merge` reads are the ones the round writes into the spec.
    #[test]
    fn pr_review_brief_names_the_spec_and_records_nothing() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        let spec_dir = root.join(".claude").join("spec").join("my-unit");
        std::fs::create_dir_all(&spec_dir).expect("spec dir");
        // A spec is its event file: the brief reads the files of its tasks,
        // and a rendered `spec.md` that says otherwise is not read.
        std::fs::write(spec_dir.join("spec.md"), "# demo\n\n## Files\n\n- `packages/core/src/lib.rs`\n")
            .expect("an old spec.md");
        let events = [
            json!({"v": 1, "id": 1, "at": "2026-09-18T10:00:00-03:00", "type": "task", "author": "assistant",
                "wave": 1, "text": "A.", "files": [{"path": "apps/rt/src/lib.rs"}], "origin": 1}),
            json!({"v": 1, "id": 2, "at": "2026-09-18T10:00:01-03:00", "type": "task", "author": "assistant",
                "wave": 2, "text": "B.", "files": [{"path": "apps/rt/src/main.rs"}, {"path": "apps/rt/src/lib.rs"}],
                "origin": 1}),
        ];
        let lines: Vec<String> = events.iter().map(Value::to_string).collect();
        std::fs::write(spec_dir.join("spec.ndjson"), format!("{}\n", lines.join("\n"))).expect("spec");
        let shelf = root.join("apps/rt/.claude/skills/rt-demo-pattern");
        std::fs::create_dir_all(&shelf).expect("shelf");
        std::fs::write(
            shelf.join("SKILL.md"),
            "---\nname: rt-demo-pattern\ndescription: Use when demoing.\n---\n\nbody\n",
        )
        .expect("skill");

        let bases = door_flow();
        let facts = PrFacts { number: 7, head: "dev_my-unit".to_string() };

        let brief = review_brief(root, &facts, &bases);
        assert!(brief.ok);
        assert_eq!(brief.spec.as_deref(), Some("my-unit"));
        assert_eq!(brief.subproject.as_deref(), Some("apps/rt"));
        assert!(
            brief.spec_path.unwrap_or_default().ends_with("my-unit/spec.ndjson"),
            "forward slashes on every platform"
        );
        assert!(
            brief.patterns.unwrap_or_default().contains("rt-demo-pattern"),
            "the review reads the implementer's own shelf"
        );
        assert!(
            !root.join(".claude").join("spec").join("my-unit").join("review").exists(),
            "the brief writes no verdict of its own"
        );
    }
}
