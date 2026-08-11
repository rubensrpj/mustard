//! `work_branch_gate` — auto-branch per work unit on the first file mutation.
//!
//! ## Contract (slim)
//!
//! A `PreToolUse(Write|Edit|MultiEdit)` [`Check`] with exactly two behaviors:
//!
//! 1. **Pending work-unit marker** → cut `{base}_{slug}` off a freshly
//!    fetched base and check it out (fail-open: any git failure warns, never
//!    blocks — unless staying would leave the edit on a protected branch).
//!    The dirty tree is pre-checked with the worktree door's probe so a
//!    refused checkout names the paths that likely refused it; when the run
//!    then continues on the previous branch, the marker is RECONCILED to the
//!    branch actually active (both names in the warning), never silently
//!    cleared — and a protected-branch deny keeps the marker so the next
//!    attempt retries the cut.
//!    A cut that lands IN-PLACE on the MAIN checkout answers `Warn` with the
//!    isolation hint (`EnterWorktree name=…`) — informative only, the edit
//!    proceeds; a cut inside a linked worktree (already isolated) or a
//!    submodule (a superproject-scoped hint would be wrong there) stays a
//!    silent `Allow`.
//!    The router pre-computed the branch name and stored it as the session's
//!    `pending-work-branch` marker via `emit-pipeline --kind pipeline.kind`
//!    (see [`crate::commands::event::emit_pipeline`]); this hook is the
//!    consumer. Read-only requests never Write/Edit, so the marker is never
//!    consumed and no branch is ever created.
//! 2. **No marker** → deny only a direct edit on a BARE integration branch
//!    (an exact member of `git.flow`'s bases); any work branch edits freely.
//!    `.claude/spec/…` is NOT exempt from this (it was, once): the spec belongs
//!    to the unit, so it is written after the branch exists, never on the base.
//!
//! ## The two harness carve-outs ([`is_harness_carve_out`])
//!
//! Branch protection guards the REPO's tree. Two paths under `.claude/` are
//! carved out of it because both are written BEFORE a unit exists and neither
//! is repo work — they are allowed on a bare base, cut NO branch, and leave a
//! pending marker intact for the first real edit:
//!
//! - `.claude/plans/…` — native plan-mode artifacts. Planning precedes the
//!   work unit, so blocking them deadlocks plan mode on a protected branch:
//!   the session cannot even write the plan it needs approved.
//! - `.claude/scratch/…` — sanctioned SCRATCH EVIDENCE: the throwaway code a
//!   diagnosis runs to decide between two hypotheses by executing them, before
//!   there is any unit to open. Refusing it did not prevent the work, it only
//!   made the agent deduce over several rounds what one run would have shown.
//!   The seeded `.claude/.gitignore` ignores `scratch/`, so it never reaches a
//!   diff either.
//!
//! **The limit of the scratch carve-out.** It serves evidence a runner can
//! execute IN PLACE — shell scripts, data fixtures, `mustard-rt run …` probes.
//! Evidence that must COMPILE inside a crate cannot live there: cargo does not
//! compile files under `.claude/`, so no carve-out here can make a throwaway
//! Rust integration test work. For that case the honest path is opening the
//! unit early — cheap once the diagnosis rides into the spec through
//! `spec-draft --material` instead of being retyped by hand.
//!
//! The git primitives this gate performs the cut with — the base refresh, the
//! checkout, the `{base}_` base recovery, the protected-branch predicate — live
//! in [`crate::commands::event::work_branch`], because `spec-draft` performs the
//! SAME cut before it writes. A second spelling is how two doors that must agree
//! about a branch stop agreeing.
//!
//! ## Two roots: state vs. the local tree (the nested-worktree fix)
//!
//! Inside a linked worktree (`.claude/worktrees/{slug}`) the workspace
//! resolver redirects every `.claude/` path to the MAIN checkout — the
//! worktree carries code only (see `tests/worktree_redirect.rs`). That is
//! correct for STATE (the pending-branch marker, `mustard.json` git flow),
//! but this gate used to read the git BRANCH from that redirected root too,
//! so inside a linked worktree it judged the MAIN checkout's branch (`dev`)
//! and false-blocked every edit. Branch detection and the branch cut now run
//! against the LOCAL tree of the edit ([`local_tree_of`]: the edited file's
//! own directory, else the process cwd), while state keeps the main-checkout
//! resolution. A per-session worktree is its own tree, so cutting the work
//! branch there collides with nobody.
//!
//! ## Agnostic base model
//!
//! Everything derives from `mustard.json#git.flow` (via
//! [`mustard_core::domain::config::GitConfig`]). The project's **integration
//! bases** are `git.flow`'s non-`*` keys ∪ values (`{"*":"dev","dev":"main"}` →
//! `{dev, main}`; `{"*":"develop","develop":"master"}` → `{develop, master}`);
//! nothing hardcodes `dev`/`main`. A work branch's base is recovered from its
//! NAME: the LONGEST integration base `B` with `target == "{B}_…"`
//! ([`base_for`], falling back to `config.git.primary_base()`).
//!
//! ## Contract (apps/rt/CLAUDE.md)
//!
//! Never panics — no `unwrap`/`expect` outside tests; every git call is
//! `Command::…output()` matched into a `Result`, and an `Err` from the `Check`
//! itself folds to `Allow` in the dispatcher. It raises `Deny` ONLY to keep an
//! edit off a bare integration branch; otherwise it `Allow`s or `Warn`s.

use mustard_core::platform::error::Error;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::io::workspace::is_git_repo_root;
use mustard_core::platform::i18n::{translate, Locale};
use mustard_core::ProjectConfig;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::commands::event::work_branch::{
    base_for, checkout_work_branch, current_branch, is_protected, refresh_integration_bases,
};
use crate::commands::work_unit_open::dirty_paths;
use crate::shared::context;

/// How many dirty paths a checkout-failure verdict spells out before
/// summarising — a hook message is one line in the transcript, so the cap is
/// smaller than the worktree door's 20-path refusal.
const MAX_DIRTY_NAMED: usize = 5;

/// The dirty-path note appended to a checkout-failure verdict, built from the
/// SAME probe the worktree door uses ([`dirty_paths`]) — measured BEFORE the
/// attempt, so the verdict can name the usual reason git refused instead of
/// leaving only the raw git error. Empty when the tree was clean.
/// Catalogue-rendered in the project's configured language.
fn dirty_note(dirty: &[String], lang: Locale) -> String {
    if dirty.is_empty() {
        return String::new();
    }
    let shown: Vec<&str> = dirty.iter().take(MAX_DIRTY_NAMED).map(String::as_str).collect();
    let more = dirty.len().saturating_sub(shown.len());
    let tail = if more == 0 { String::new() } else { format!(" (+{more})") };
    translate("workbranch.dirty.note", lang)
        .replace("{paths}", &shown.join(", "))
        .replace("{more}", &tail)
}

/// The auto-branch gate. Stateless — every invocation rebuilds from the hook
/// input and the on-disk marker.
pub struct WorkBranchGate;

/// `true` when `rel` is one of the two harness paths carved out of branch
/// protection: native plan-mode artifacts and sanctioned scratch evidence.
/// Both are harness state written BEFORE a unit exists, so both are allowed on
/// a bare integration base, cut no branch and keep the pending marker — see the
/// module doc for why each is carved out and where the scratch carve-out stops.
///
/// `rel` is the project-relative, forward-slash path
/// [`super::boundary_gate::relative_to_cwd`] produces, so the same prefixes
/// match on every platform.
fn is_harness_carve_out(rel: &str) -> bool {
    rel.starts_with(".claude/plans/") || rel.starts_with(".claude/scratch/")
}

/// Resolve the session id for this invocation: the harness-provided
/// [`HookInput::session_id`] when present, else the env/filesystem fallback in
/// [`context::session_id`].
fn session_id_from(input: &HookInput) -> String {
    if let Some(s) = input.session_id.as_deref().filter(|s| !s.is_empty()) {
        return s.to_string();
    }
    context::session_id()
}

/// The working tree that HOSTS the edit — branch detection runs here.
///
/// Priority: the edited file's own directory (nearest EXISTING ancestor, so a
/// Write that creates parent directories still resolves) → the process cwd →
/// the state root. An absolute edit into another tree (session cwd in a
/// worktree, target in the main checkout) is judged by the TARGET's tree.
fn local_tree_of(input: &HookInput, state_root: &str) -> String {
    if let Some(fp) = input.file_path() {
        let p = Path::new(&fp);
        if p.is_absolute() {
            let mut dir = p.parent();
            while let Some(d) = dir {
                if d.is_dir() {
                    return d.to_string_lossy().into_owned();
                }
                dir = d.parent();
            }
        }
        // Relative path → anchored at the process cwd; fall through.
    }
    if let Some(cwd) = input.cwd.as_deref().filter(|c| !c.is_empty()) {
        return cwd.to_string();
    }
    state_root.to_string()
}

/// Canonicalise `p`, falling back to the path as-given on error — so relative
/// and absolute spellings of the same directory compare equal without panicking.
fn canonicalize_or(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// The nearest ancestor of `start` (inclusive) that is a git repository root
/// (`.git` dir or pointer file) AND a PROPER descendant of `state_root` — a
/// nested repo sitting BETWEEN the superproject and the edited file. `None` when
/// the first git root reached is the state root itself (the superproject) or no
/// git root is found at/below it. Purely a filesystem walk (reuses the single
/// [`is_git_repo_root`] probe); never climbs above the state root.
fn nested_git_root_between(state_root: &Path, start: &Path) -> Option<PathBuf> {
    let state = canonicalize_or(state_root);
    for dir in start.ancestors() {
        let here = canonicalize_or(dir);
        if is_git_repo_root(dir) {
            // A `.git` AT the state root is the SUPERproject — not a nested repo.
            return (here != state).then(|| dir.to_path_buf());
        }
        if here == state {
            return None; // reached the state root with no nested `.git` below it
        }
    }
    None
}

/// The absolute path `git rev-parse` reports for `flag` (`--git-dir`,
/// `--git-common-dir`, …) in `root`. A LINKED worktree's common dir is the
/// MAIN checkout's while a SUBMODULE reports its own — and a linked worktree's
/// git-dir differs from its common dir — these two probes tell every tree
/// shape apart. `None` on any git failure.
fn git_abs_path(vcs: &str, root: &str, flag: &str) -> Option<PathBuf> {
    let out = Command::new(vcs)
        .args(["rev-parse", "--path-format=absolute", flag])
        .current_dir(root)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if s.is_empty() { None } else { Some(PathBuf::from(s)) }
}

/// `true` when `nested` is a DISTINCT repository from the state root's — a real
/// submodule, not a linked worktree of the SAME repo (which shares the main
/// checkout's git-common-dir). Fail-closed to `false` (keep today's behavior)
/// when either probe fails, so only a proven-distinct repo ever swaps the base.
fn is_distinct_repo(vcs: &str, state_root: &str, nested: &Path) -> bool {
    match (
        git_abs_path(vcs, &nested.to_string_lossy(), "--git-common-dir"),
        git_abs_path(vcs, state_root, "--git-common-dir"),
    ) {
        (Some(a), Some(b)) => canonicalize_or(&a) != canonicalize_or(&b),
        _ => false,
    }
}

/// `true` when `root` hosts the repository's MAIN checkout: `--git-dir` and
/// `--git-common-dir` resolve to the SAME path (a linked worktree's git-dir
/// lives under `<common>/worktrees/<name>`, so they differ). Fail-closed to
/// `false` when a probe fails — the in-place nudge is informative only and
/// must never fire on guesswork.
fn is_main_checkout(vcs: &str, root: &str) -> bool {
    match (
        git_abs_path(vcs, root, "--git-dir"),
        git_abs_path(vcs, root, "--git-common-dir"),
    ) {
        (Some(a), Some(b)) => canonicalize_or(&a) == canonicalize_or(&b),
        _ => false,
    }
}

/// The submodule's OWN integration base: its remote default branch
/// (`git symbolic-ref --short refs/remotes/origin/HEAD`, `origin/` stripped),
/// falling back to its current branch. `None` when neither resolves (detached
/// HEAD / git failure) — the caller then keeps today's superproject base.
fn nested_default_base(vcs: &str, root: &str) -> Option<String> {
    if let Ok(out) = Command::new(vcs)
        .args(["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"])
        .current_dir(root)
        .output()
    {
        if out.status.success() {
            if let Ok(s) = String::from_utf8(out.stdout) {
                let b = s.trim().trim_start_matches("origin/").trim();
                if !b.is_empty() {
                    return Some(b.to_string());
                }
            }
        }
    }
    current_branch(vcs, root).filter(|b| b != "HEAD")
}

/// Resolve the effective `(target, base)` for a work branch when the edited
/// file's LOCAL tree sits inside a DISTINCT nested git repository (a submodule)
/// below the state root: the base becomes that repo's own default branch, and
/// the `{base}_{slug}` NAME is re-prefixed with it (never the superproject's).
///
/// `None` — keep today's superproject-derived pair — for every non-submodule
/// case: no nested root, a linked worktree of the same repo (shared
/// git-common-dir), or an unresolvable submodule default branch. Fully
/// fail-open, so a git-less / offline / detached submodule never breaks the cut.
fn nested_work_target_base(
    vcs: &str,
    state_root: &str,
    local: &str,
    target: &str,
    config: &ProjectConfig,
) -> Option<(String, String)> {
    let nested = nested_git_root_between(Path::new(state_root), Path::new(local))?;
    if !is_distinct_repo(vcs, state_root, &nested) {
        return None; // a linked worktree of the same repo — not a submodule
    }
    let sub_base = nested_default_base(vcs, &nested.to_string_lossy())?;
    // Re-prefix the name with the submodule base: strip the superproject base
    // recovered from the marker, re-attach the submodule's. When the marker does
    // not carry that prefix, keep the name and only swap the cut base.
    let super_base = base_for(target, config);
    let effective_target = match target.strip_prefix(&format!("{super_base}_")) {
        Some(slug) => format!("{sub_base}_{slug}"),
        None => target.to_string(),
    };
    Some((effective_target, sub_base))
}

impl Check for WorkBranchGate {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        // Defensive: only PreToolUse(Write|Edit|MultiEdit) should reach us.
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        // STATE root: the main checkout (worktree redirect) — markers, config.
        let project = ctx.project_dir_or_cwd(input);
        let sid = session_id_from(input);

        // Branch protection guards the REPO's tree only. A mutation whose
        // target lives outside the project root (`~/.claude` memory files,
        // temp dirs, …) is not repo work: never block it, never cut a branch
        // for it, and keep the pending marker for the first IN-repo edit.
        if let Some(fp) = input.file_path() {
            match super::boundary_gate::relative_to_cwd(&project, &fp) {
                None => return Ok(Verdict::Allow),
                // The harness carve-outs (`.claude/plans/…` plan-mode
                // artifacts, `.claude/scratch/…` scratch evidence) are harness
                // state, not repo work — both are written BEFORE the work unit
                // exists. Same contract as out-of-repo: allow, cut nothing,
                // keep the marker. See [`is_harness_carve_out`].
                Some(rel) if is_harness_carve_out(&rel) => return Ok(Verdict::Allow),
                // `.claude/spec/…` used to be carved out here too, so a spec
                // could be authored on the protected base before any branch
                // existed. It is NOT carved out any more: the spec is the first
                // thing the work produces and it belongs to the unit, so the
                // branch is cut BEFORE the draft
                // ([`crate::commands::event::work_branch::cut_pending_work_branch`],
                // called by `spec-draft`) and the whole layout — spec, waves,
                // ceremony, proof — is written inside it. A spec write on a bare
                // base is therefore refused like any other repo write.
                Some(_) => {}
            }
        }

        // VCS binary policy: default `git`; an explicit `""` opt-out (or a
        // non-git tree) means we cannot branch and there is nothing to guard.
        let config = crate::shared::context::project_config_cached(Path::new(&project));
        let Some(vcs) = config.vcs() else {
            context::clear_pending_branch(&project, &sid);
            return Ok(Verdict::Allow);
        };

        // LOCAL tree of the edit — branch detection and the cut run HERE, not
        // on the (possibly redirected) state root. See module doc.
        let local = local_tree_of(input, &project);

        let current = current_branch(&vcs, &local);
        let on_protected = current
            .as_deref()
            .map(|b| is_protected(b, &config))
            .unwrap_or(false);

        // 1. No pending work unit signalled for this session.
        let Some(target) = context::pending_branch_for(&project, &sid) else {
            // Hard block: never develop directly on a bare integration
            // branch. On a work branch (or non-git tree) there is nothing to do.
            if on_protected {
                return Ok(Verdict::Deny {
                    reason: format!(
                        "Você está na branch de integração protegida '{}'. O Mustard não \
                         desenvolve direto aqui — descreva o trabalho para eu criar a branch \
                         {{base}}_{{slug}}, ou crie uma branch manualmente antes de editar.",
                        current.as_deref().unwrap_or("?")
                    ),
                });
            }
            return Ok(Verdict::Allow);
        };

        // Nested-git-root (submodule) base resolution: when the edited file's
        // LOCAL tree sits inside a DISTINCT nested repo below the state root, the
        // work branch must be based on the submodule's OWN default branch and its
        // `{base}_{slug}` name must carry that base, never the superproject's.
        // Fail-open to today's (target, prefix-recovered base) pair otherwise.
        let nested = nested_work_target_base(&vcs, &project, &local, &target, &config);
        let in_submodule = nested.is_some();
        let (target, base) =
            nested.unwrap_or_else(|| (target.clone(), base_for(&target, &config)));

        // 2. Already on the target branch → clear and allow.
        if current.as_deref() == Some(target.as_str()) {
            context::clear_pending_branch(&project, &sid);
            return Ok(Verdict::Allow);
        }

        // 3. Refresh the integration bases from origin FIRST so the branch is
        //    cut from the latest dev/main. Fail-open: offline / no remote /
        //    non-ff never blocks the edit (see refresh_integration_bases).
        refresh_integration_bases(&vcs, &local, &config, current.as_deref());

        // 3.5 Pre-check the dirty tree with the SAME probe the worktree door
        //     uses, BEFORE the attempt. The cut itself still carries changes
        //     along (a plain checkout, no stash) — the measurement exists so a
        //     refused checkout can NAME the paths that likely refused it
        //     instead of asserting nothing beyond the raw git error.
        let dirty = dirty_paths(Path::new(&local));

        // 4. Check out the (possibly submodule-rebased) target IN THE LOCAL TREE
        //    (a per-session worktree is isolated; the shared main checkout keeps
        //    the historical single-session behavior).
        match checkout_work_branch(&vcs, &local, &target, &base) {
            Ok(()) => {
                context::clear_pending_branch(&project, &sid);
                // The cut landed IN-PLACE when the hosting tree is the MAIN
                // checkout — legitimate (cheap, off the protected base), but
                // un-isolated: say so ONCE, naming the native step that
                // isolates the unit. A linked worktree is already isolated; a
                // submodule cut would name a branch the superproject-scoped
                // EnterWorktree cannot attach. Informative only — Warn lets
                // the edit through; never a mode, never a block.
                if !in_submodule && is_main_checkout(&vcs, &local) {
                    return Ok(Verdict::Warn {
                        message: format!(
                            "branch '{target}' criada in-place no checkout principal — o \
                             trabalho segue aqui. Para isolar esta unidade numa worktree \
                             própria: EnterWorktree name={target}"
                        ),
                    });
                }
                Ok(Verdict::Allow)
            }
            Err(e) => {
                let lang = config.i18n().lang;
                let note = dirty_note(&dirty, lang);
                if on_protected {
                    // We could not leave the protected branch — refuse rather
                    // than let the edit land directly on it. The marker is
                    // KEPT: the deny blocks this edit anyway, and the next
                    // attempt (after the user resolves git) retries the cut —
                    // clearing it here destroyed the intent and left the
                    // session stuck on the protected branch with no retry.
                    Ok(Verdict::Deny {
                        reason: format!(
                            "Não consegui sair da branch protegida '{}' para '{target}': {e}.{note} \
                             O Mustard não desenvolve direto na branch protegida — resolva o \
                             git e tente de novo.",
                            current.as_deref().unwrap_or("?")
                        ),
                    })
                } else {
                    // On a work branch already: the run continues on the branch
                    // actually active, so RECONCILE the record with reality —
                    // rewrite the marker to that branch (the next edit's
                    // `current == target` fast path then consumes it) and name
                    // BOTH branches in the warning. Clearing the marker here
                    // destroyed the intent: the recorded branch stayed a name
                    // that never existed, and nothing retried or reconciled.
                    match current.as_deref() {
                        Some(actual) => context::set_pending_branch(&project, &sid, actual),
                        // No branch to record (detached HEAD / probe failure)
                        // — clearing is the honest residue.
                        None => context::clear_pending_branch(&project, &sid),
                    }
                    let actual = current.as_deref().unwrap_or("?");
                    Ok(Verdict::Warn {
                        message: translate("workbranch.reconcile.warn", lang)
                            .replace("{target}", &target)
                            .replace("{error}", &e)
                            .replace("{actual}", actual)
                            .replace("{note}", &note),
                    })
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::model::contract::{Ctx, HookInput, Trigger};
    use serde_json::json;

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

    fn pre_edit_input(root: &str, sid: &str) -> (HookInput, Ctx) {
        pre_edit_input_for(root, sid, "f.txt")
    }

    /// Like [`pre_edit_input`] but with an explicit mutation target path.
    fn pre_edit_input_for(root: &str, sid: &str, file_path: &str) -> (HookInput, Ctx) {
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({ "file_path": file_path, "content": "x" }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(root.to_string()),
            session_id: Some(sid.to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: root.to_string(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        (input, ctx)
    }

    /// Write a `mustard.json` declaring the given `git.flow` so the gate derives
    /// this project's integration bases from it (agnostic — no hardcoded flow).
    fn seed_flow(root: &Path, flow_json: &str) {
        std::fs::write(
            root.join("mustard.json"),
            format!(r#"{{"git":{{"flow":{flow_json}}}}}"#),
        )
        .unwrap();
    }

    /// Init a git repo whose sole commit lives on `base` (created before the
    /// first commit so it exists regardless of the platform's default branch).
    fn init_repo_on(root: &Path, base: &str) {
        git(root, &["init"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["checkout", "-b", base]);
        std::fs::write(root.join("f.txt"), "hi").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "init"]);
    }

    /// Marker `dev_my-thing` on a repo whose base is `dev` → the gate recovers
    /// `dev` from the `dev_` prefix and checks the work branch out off `dev`.
    #[test]
    fn creates_work_branch_off_prefix_base_dev() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();

        seed_flow(root, r#"{"*":"dev","dev":"main"}"#);
        init_repo_on(root, "dev");

        // The router pre-computed this branch for the work unit.
        let sid = "sess-branch-test";
        context::set_pending_branch(root_s, sid, "dev_my-thing");

        // First Write of the work unit fires the gate. The cut lands in-place
        // on the MAIN checkout → the informative Warn nudge (EnterWorktree
        // hint); the edit still proceeds.
        let (input, ctx) = pre_edit_input(root_s, sid);
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(matches!(verdict, Verdict::Warn { .. }), "in-place cut nudges: {verdict:?}");

        // HEAD is now on the target branch, created off `dev` (from the prefix)...
        assert_eq!(
            current_branch("git", root_s).as_deref(),
            Some("dev_my-thing"),
            "checked out the pre-computed branch off dev",
        );
        // ...and the marker was cleared so subsequent edits do not re-fire.
        assert!(
            context::pending_branch_for(root_s, sid).is_none(),
            "marker cleared after checkout",
        );
    }

    /// Marker whose target is ALREADY checked out (by hand) → clear and allow,
    /// no git mutation.
    #[test]
    fn already_on_target_clears_marker_and_allows() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();
        seed_flow(root, r#"{"*":"dev","dev":"main"}"#);
        init_repo_on(root, "dev");
        git(root, &["checkout", "-b", "dev_manual"]);

        let sid = "sess-already";
        context::set_pending_branch(root_s, sid, "dev_manual");
        let (input, ctx) = pre_edit_input(root_s, sid);
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(matches!(verdict, Verdict::Allow), "got {verdict:?}");
        assert_eq!(current_branch("git", root_s).as_deref(), Some("dev_manual"));
        assert!(context::pending_branch_for(root_s, sid).is_none());
    }

    /// AC-2 — spec authoring is NO LONGER carved out of branch protection.
    ///
    /// `.claude/spec/…` used to return `Allow` here whatever the branch, so a
    /// spec was authored on the protected base and the unit's own artefacts
    /// lived apart from the unit. The spec is the first thing the work produces,
    /// so it is refused on a bare base exactly like any other repo write — and
    /// WITH a pending marker the same write CUTS the branch, which is how the
    /// draft comes to live inside the unit.
    #[test]
    fn spec_authoring_on_protected_base_is_refused_then_cuts_the_branch() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();
        seed_flow(root, r#"{"*":"dev","dev":"main"}"#);
        init_repo_on(root, "dev");

        // Author a spec.md under `.claude/spec/…` with NO work unit pending.
        let spec_dir = root.join(".claude").join("spec").join("notif");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), "# spec").unwrap();
        let spec = spec_dir.join("spec.md");
        let spec_s = spec.to_str().unwrap();

        let (input, ctx) = pre_edit_input_for(root_s, "sess-spec", spec_s);
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(
            matches!(verdict, Verdict::Deny { .. }),
            "a spec write on the bare integration base is refused like any other \
             repo write — the carve-out is gone: {verdict:?}",
        );
        assert_eq!(
            current_branch("git", root_s).as_deref(),
            Some("dev"),
            "a refusal cuts nothing",
        );

        // The same write, with the work unit signalled, lands INSIDE the unit:
        // the marker is consumed and the branch is cut before the spec exists.
        let sid = "sess-spec-marked";
        context::set_pending_branch(root_s, sid, "dev_notif");
        let (marked, ctx2) = pre_edit_input_for(root_s, sid, spec_s);
        let verdict2 = WorkBranchGate.evaluate(&marked, &ctx2).expect("no error");
        assert!(
            matches!(verdict2, Verdict::Warn { .. }),
            "the in-place cut nudges, it never refuses the spec write: {verdict2:?}",
        );
        assert_eq!(
            current_branch("git", root_s).as_deref(),
            Some("dev_notif"),
            "the spec is written on the unit's own branch",
        );
        assert!(
            context::pending_branch_for(root_s, sid).is_none(),
            "the marker is consumed by the spec write, like any first mutation",
        );
    }

    /// REGRESSION (nested worktree — the bug that false-blocked every edit):
    /// the harness state-root redirect hands the gate the MAIN checkout as
    /// `project_dir` even though the session edits inside a linked worktree
    /// (modelled on `tests/worktree_redirect.rs`). Branch detection must run
    /// against the WORKTREE (a work branch → free to edit), never against the
    /// main checkout's integration branch — while the same state root still
    /// hard-blocks a direct edit in the main checkout itself.
    #[test]
    fn nested_worktree_edit_judged_by_local_tree_not_state_root() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("repo");
        std::fs::create_dir_all(&main).unwrap();
        seed_flow(&main, r#"{"*":"dev","dev":"main"}"#);
        init_repo_on(&main, "dev");
        // The harness cuts worktrees at <main>/.claude/worktrees/<slug>, on a
        // WORK branch.
        git(&main, &["worktree", "add", ".claude/worktrees/dev_x", "-b", "dev_x"]);
        let wt = main.join(".claude").join("worktrees").join("dev_x");
        let main_s = main.to_str().unwrap();

        // Edit a file INSIDE the worktree; ctx.project_dir is the MAIN
        // checkout (what the workspace redirect resolves for state).
        let file_in_wt = wt.join("f.txt");
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({ "file_path": file_in_wt.to_str().unwrap(), "content": "x" }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(wt.to_str().unwrap().to_string()),
            session_id: Some("sess-nested".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: main_s.to_string(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(
            matches!(verdict, Verdict::Allow),
            "a worktree on a work branch edits freely even when state resolves \
             to a main checkout sitting on an integration branch: {verdict:?}",
        );

        // The SAME state root judged at the main checkout itself still blocks.
        let (input2, ctx2) = pre_edit_input(main_s, "sess-nested-main");
        let verdict2 = WorkBranchGate.evaluate(&input2, &ctx2).expect("no error");
        assert!(
            matches!(verdict2, Verdict::Deny { .. }),
            "a bare-integration-branch edit in the main checkout stays blocked: {verdict2:?}",
        );
    }

    /// A pending marker consumed INSIDE a linked worktree cuts the target
    /// branch in the WORKTREE — the main checkout's HEAD is untouched.
    #[test]
    fn nested_worktree_marker_cuts_branch_locally() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("repo");
        std::fs::create_dir_all(&main).unwrap();
        seed_flow(&main, r#"{"*":"dev","dev":"main"}"#);
        init_repo_on(&main, "dev");
        git(&main, &["worktree", "add", ".claude/worktrees/dev_x", "-b", "dev_x"]);
        let wt = main.join(".claude").join("worktrees").join("dev_x");
        let wt_s = wt.to_str().unwrap();
        let main_s = main.to_str().unwrap();

        // Marker lives in the STATE root (the main checkout).
        let sid = "sess-nested-marker";
        context::set_pending_branch(main_s, sid, "dev_other");

        let file_in_wt = wt.join("f.txt");
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({ "file_path": file_in_wt.to_str().unwrap(), "content": "x" }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(wt_s.to_string()),
            session_id: Some(sid.to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: main_s.to_string(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(matches!(verdict, Verdict::Allow), "got {verdict:?}");
        assert_eq!(
            current_branch("git", wt_s).as_deref(),
            Some("dev_other"),
            "the work branch is cut in the LOCAL worktree",
        );
        assert_eq!(
            current_branch("git", main_s).as_deref(),
            Some("dev"),
            "the main checkout's HEAD is untouched",
        );
        assert!(
            context::pending_branch_for(main_s, sid).is_none(),
            "marker consumed from the state root",
        );
    }

    /// FAIL-OPEN: with NO `origin` remote, the base refresh's `git fetch origin`
    /// fails — that must NOT break branch creation. The gate still checks the
    /// work branch out off the local base and Allows the edit.
    #[test]
    fn base_refresh_is_fail_open_without_origin_remote() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();

        seed_flow(root, r#"{"*":"dev","dev":"main"}"#);
        init_repo_on(root, "dev");

        // No `origin` remote is configured — `git fetch origin` will fail.
        let has_origin = Command::new("git")
            .args(["remote", "get-url", "origin"])
            .current_dir(root)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(!has_origin, "precondition: repo has no origin remote");

        let sid = "sess-offline";
        context::set_pending_branch(root_s, sid, "dev_offline-thing");

        let (input, ctx) = pre_edit_input(root_s, sid);
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(
            matches!(verdict, Verdict::Warn { .. }),
            "a failing `git fetch origin` must not block the edit (in-place cut → nudge): {verdict:?}",
        );
        assert_eq!(
            current_branch("git", root_s).as_deref(),
            Some("dev_offline-thing"),
            "the work branch is still cut from the local base when offline",
        );
        assert!(
            context::pending_branch_for(root_s, sid).is_none(),
            "marker cleared after checkout",
        );
    }

    /// A local bare-remote fixture whose `dev` is BEHIND origin: the base refresh
    /// fast-forwards the local `dev` before the work branch is cut, so the branch
    /// carries the newest commit. Proves the refresh actually advances the base.
    #[test]
    fn base_refresh_fast_forwards_base_behind_origin() {
        let tmp = tempfile::tempdir().unwrap();

        // A bare "remote" repo whose HEAD points at `dev` (set explicitly so we
        // do not depend on the git version's default-branch flag).
        let remote = tmp.path().join("remote.git");
        std::fs::create_dir_all(&remote).unwrap();
        let remote_s = remote.to_str().unwrap();
        git(&remote, &["init", "--bare"]);
        git(&remote, &["symbolic-ref", "HEAD", "refs/heads/dev"]);

        // A working clone that publishes the first `dev` commit.
        let seed = tmp.path().join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        git(&seed, &["init"]);
        git(&seed, &["config", "user.email", "t@example.com"]);
        git(&seed, &["config", "user.name", "t"]);
        git(&seed, &["checkout", "-b", "dev"]);
        std::fs::write(seed.join("f.txt"), "one").unwrap();
        git(&seed, &["add", "."]);
        git(&seed, &["commit", "-m", "one"]);
        git(&seed, &["remote", "add", "origin", remote_s]);
        git(&seed, &["push", "origin", "dev"]);

        // The project clone — its local `dev` starts at the first commit.
        let proj = tmp.path().join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        let proj_s = proj.to_str().unwrap();
        git(&proj, &["clone", remote_s, "."]);
        git(&proj, &["config", "user.email", "t@example.com"]);
        git(&proj, &["config", "user.name", "t"]);
        seed_flow(&proj, r#"{"*":"dev","dev":"main"}"#);

        // Now advance origin/dev with a SECOND commit the project has not seen.
        std::fs::write(seed.join("f.txt"), "two").unwrap();
        git(&seed, &["add", "."]);
        git(&seed, &["commit", "-m", "two"]);
        git(&seed, &["push", "origin", "dev"]);
        let ahead = Command::new("git")
            .args(["rev-parse", "dev"])
            .current_dir(&seed)
            .output()
            .unwrap();
        let ahead_sha = String::from_utf8(ahead.stdout).unwrap().trim().to_string();

        // First edit of the work unit fires the gate; base refresh must ff `dev`.
        let sid = "sess-behind";
        context::set_pending_branch(proj_s, sid, "dev_new-thing");
        let (input, ctx) = pre_edit_input(proj_s, sid);
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(matches!(verdict, Verdict::Warn { .. }), "edit proceeds with nudge: {verdict:?}");
        assert_eq!(
            current_branch("git", proj_s).as_deref(),
            Some("dev_new-thing"),
            "the work branch is checked out",
        );

        // The work branch was cut from the fast-forwarded `dev` (second commit).
        let head = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&proj)
            .output()
            .unwrap();
        let head_sha = String::from_utf8(head.stdout).unwrap().trim().to_string();
        assert_eq!(
            head_sha, ahead_sha,
            "the work branch is based on the fast-forwarded dev (latest origin commit)",
        );
    }

    /// Agnostic: a `develop`/`master` project cuts `develop_feature` off
    /// `develop` — nothing in the gate assumes `dev`/`main`.
    #[test]
    fn creates_work_branch_off_prefix_base_develop() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();

        seed_flow(root, r#"{"*":"develop","develop":"master"}"#);
        init_repo_on(root, "develop");

        let sid = "sess-develop";
        context::set_pending_branch(root_s, sid, "develop_feature");

        let (input, ctx) = pre_edit_input(root_s, sid);
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(matches!(verdict, Verdict::Warn { .. }), "in-place cut nudges: {verdict:?}");
        assert_eq!(
            current_branch("git", root_s).as_deref(),
            Some("develop_feature"),
            "checked out off develop (its prefix base)",
        );
    }

    #[test]
    fn no_marker_is_a_silent_noop() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();
        // No git repo, no marker: the gate must simply Allow (never panic).
        let (input, ctx) = pre_edit_input(root_s, "sess-none");
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(matches!(verdict, Verdict::Allow));
    }

    /// A repo on a protected branch (`main`) with NO pending-branch marker and
    /// NO `mustard.json`: a direct edit must be BLOCKED — the harness never
    /// develops on an integration branch. With no `git.flow`, the base set
    /// falls back to `{main, master}`, so `main` is protected.
    #[test]
    fn blocks_direct_edit_on_protected_branch_without_marker() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();
        git(root, &["init"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["checkout", "-b", "main"]);
        std::fs::write(root.join("f.txt"), "hi").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "init"]);

        let (input, ctx) = pre_edit_input(root_s, "sess-protected");
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(
            matches!(verdict, Verdict::Deny { .. }),
            "direct edit on main is blocked: {verdict:?}",
        );
    }

    /// A repo already on a work branch (`feature/x`) with NO marker: editing is
    /// fine — the block only guards protected integration branches.
    #[test]
    fn allows_direct_edit_on_work_branch_without_marker() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();
        git(root, &["init"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["checkout", "-b", "feature/x"]);
        std::fs::write(root.join("f.txt"), "hi").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "init"]);

        let (input, ctx) = pre_edit_input(root_s, "sess-work");
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(
            matches!(verdict, Verdict::Allow),
            "editing on a work branch proceeds: {verdict:?}",
        );
    }

    /// A `dev`/`main` flow makes the BARE `dev` branch protected (derived from
    /// `git.flow`, not hardcoded): a direct edit on `dev` with no marker → Deny.
    #[test]
    fn blocks_direct_edit_on_bare_dev_from_flow() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();
        seed_flow(root, r#"{"*":"dev","dev":"main"}"#);
        init_repo_on(root, "dev");

        let (input, ctx) = pre_edit_input(root_s, "sess-bare-dev");
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(
            matches!(verdict, Verdict::Deny { .. }),
            "direct edit on the bare integration branch `dev` is blocked: {verdict:?}",
        );
    }

    /// A `dev_*` WORK branch is NOT protected: editing on `dev_thing` with no
    /// marker proceeds (the block only guards the bare integration bases).
    #[test]
    fn allows_direct_edit_on_dev_work_branch_without_marker() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();
        seed_flow(root, r#"{"*":"dev","dev":"main"}"#);
        init_repo_on(root, "dev_thing");

        let (input, ctx) = pre_edit_input(root_s, "sess-dev-work");
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(
            matches!(verdict, Verdict::Allow),
            "editing on a `dev_*` work branch proceeds: {verdict:?}",
        );
    }

    /// A Write targeting a path OUTSIDE the repo (e.g. `~/.claude` memory
    /// files) on a bare protected branch with NO marker: Allow — branch
    /// protection guards the repo's tree only.
    #[test]
    fn allows_out_of_repo_write_on_protected_branch() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();
        seed_flow(root, r#"{"*":"dev","dev":"main"}"#);
        init_repo_on(root, "dev");

        let outside = tempfile::tempdir().unwrap();
        let outside_file = outside.path().join("memo.md");
        let (input, ctx) =
            pre_edit_input_for(root_s, "sess-outside", outside_file.to_str().unwrap());
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(
            matches!(verdict, Verdict::Allow),
            "an out-of-repo write is not repo work — never blocked: {verdict:?}",
        );
    }

    /// A Write targeting the native plan-mode artifact (`.claude/plans/…`) on a
    /// bare protected branch: Allow, no branch cut, marker kept — planning
    /// precedes the work unit, and blocking it deadlocks plan mode (the session
    /// cannot even write the plan it needs approved).
    #[test]
    fn allows_plan_file_write_on_protected_branch() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();
        seed_flow(root, r#"{"*":"dev","dev":"main"}"#);
        init_repo_on(root, "dev");

        // Even with a pending work unit, a plan write must not consume it.
        let sid = "sess-plan-file";
        context::set_pending_branch(root_s, sid, "dev_planned-thing");

        let (input, ctx) = pre_edit_input_for(root_s, sid, ".claude/plans/my-plan.md");
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(matches!(verdict, Verdict::Allow), "plan file writes freely: {verdict:?}");
        assert_eq!(
            current_branch("git", root_s).as_deref(),
            Some("dev"),
            "no branch is cut for a plan-file mutation",
        );
        assert_eq!(
            context::pending_branch_for(root_s, sid).as_deref(),
            Some("dev_planned-thing"),
            "the marker survives for the first code edit",
        );
    }

    /// AC-6 — sanctioned scratch evidence (`.claude/scratch/…`) is writable on
    /// a bare integration base: the diagnosis that decides whether there is any
    /// work at all happens BEFORE a unit exists, and the cheapest way to choose
    /// between two hypotheses is to RUN them. The carve-out has the same
    /// contract as the plan file — Allow, no branch cut, marker kept for the
    /// first real edit — because scratch is never part of the unit (the seeded
    /// `.claude/.gitignore` ignores it).
    #[test]
    fn scratch_evidence_is_writable_on_a_protected_base() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();
        seed_flow(root, r#"{"*":"dev","dev":"main"}"#);
        init_repo_on(root, "dev");

        // With NO pending unit — the diagnosis precedes the unit entirely.
        let (bare, bare_ctx) =
            pre_edit_input_for(root_s, "sess-scratch-bare", ".claude/scratch/probe.sh");
        let verdict = WorkBranchGate.evaluate(&bare, &bare_ctx).expect("no error");
        assert!(
            matches!(verdict, Verdict::Allow),
            "scratch evidence writes freely on the bare integration base: {verdict:?}",
        );
        assert_eq!(
            current_branch("git", root_s).as_deref(),
            Some("dev"),
            "no branch is cut for a scratch mutation",
        );

        // …and WITH a pending unit the marker is NOT consumed: the first real
        // edit still gets its branch.
        let sid = "sess-scratch-marked";
        context::set_pending_branch(root_s, sid, "dev_diagnosed-thing");
        let (marked, ctx) = pre_edit_input_for(root_s, sid, ".claude/scratch/data/case.json");
        let verdict2 = WorkBranchGate.evaluate(&marked, &ctx).expect("no error");
        assert!(matches!(verdict2, Verdict::Allow), "got {verdict2:?}");
        assert_eq!(
            current_branch("git", root_s).as_deref(),
            Some("dev"),
            "a pending marker is not consumed by scratch evidence",
        );
        assert_eq!(
            context::pending_branch_for(root_s, sid).as_deref(),
            Some("dev_diagnosed-thing"),
            "the marker survives for the first real edit",
        );

        // The carve-out is a PREFIX, not a substring: a real repo file whose
        // name merely mentions scratch is judged like any other repo write.
        let (decoy, decoy_ctx) =
            pre_edit_input_for(root_s, "sess-scratch-decoy", "src/scratch_notes.rs");
        let verdict3 = WorkBranchGate.evaluate(&decoy, &decoy_ctx).expect("no error");
        assert!(
            matches!(verdict3, Verdict::Deny { .. }),
            "only `.claude/scratch/` is carved out, never a lookalike path: {verdict3:?}",
        );
    }

    /// A Write targeting a path OUTSIDE the repo with a PENDING marker: Allow,
    /// no branch is cut, and the marker survives for the first in-repo edit.
    #[test]
    fn out_of_repo_write_keeps_marker_and_branch() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();
        seed_flow(root, r#"{"*":"dev","dev":"main"}"#);
        init_repo_on(root, "dev");

        let sid = "sess-outside-marker";
        context::set_pending_branch(root_s, sid, "dev_pending-thing");

        let outside = tempfile::tempdir().unwrap();
        let outside_file = outside.path().join("memo.md");
        let (input, ctx) = pre_edit_input_for(root_s, sid, outside_file.to_str().unwrap());
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(matches!(verdict, Verdict::Allow), "out-of-repo write proceeds: {verdict:?}");
        assert_eq!(
            current_branch("git", root_s).as_deref(),
            Some("dev"),
            "no branch is cut for an out-of-repo mutation",
        );
        assert_eq!(
            context::pending_branch_for(root_s, sid).as_deref(),
            Some("dev_pending-thing"),
            "the marker survives for the first in-repo edit",
        );
    }

    #[test]
    fn is_protected_matches_integration_bases_only() {
        let mut config = ProjectConfig::default();
        config.git.flow.insert("*".into(), "develop".into());
        config.git.flow.insert("develop".into(), "master".into());
        assert!(is_protected("develop", &config), "bare integration base protected");
        assert!(is_protected("master", &config), "bare integration base protected");
        assert!(!is_protected("develop_x", &config), "work branch not protected");
        assert!(!is_protected("main", &config), "not a base of THIS project");
    }

    /// AC-5 (submodule base): a marker consumed while editing a file INSIDE a
    /// DISTINCT nested git repo (a submodule) cuts the work branch off the
    /// SUBMODULE's own default branch and names it with the submodule's base —
    /// never the superproject's. Here the superproject sits on `dev` but the
    /// submodule's base is `main`, so `dev_thing` becomes `main_thing` off `main`,
    /// and the superproject checkout is untouched.
    #[test]
    fn own_git_root_submodule_resolves_nested_base() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("super");
        std::fs::create_dir_all(&main).unwrap();
        let main_s = main.to_str().unwrap();
        seed_flow(&main, r#"{"*":"dev","dev":"main"}"#);
        init_repo_on(&main, "dev"); // superproject sits on the bare integration base

        // A DISTINCT nested repository (a submodule) whose OWN base branch is
        // `main` (its own `.git` dir ⇒ a different git-common-dir than the super).
        let sub = main.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let sub_s = sub.to_str().unwrap();
        git(&sub, &["init"]);
        git(&sub, &["config", "user.email", "t@example.com"]);
        git(&sub, &["config", "user.name", "t"]);
        git(&sub, &["checkout", "-b", "main"]);
        std::fs::write(sub.join("f.txt"), "hi").unwrap();
        git(&sub, &["add", "."]);
        git(&sub, &["commit", "-m", "init"]);

        // The router precomputed the SUPERPROJECT-named branch for the work unit.
        let sid = "sess-submodule";
        context::set_pending_branch(main_s, sid, "dev_thing");

        // Edit a file INSIDE the submodule; the state root is the superproject.
        let file_in_sub = sub.join("f.txt");
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({ "file_path": file_in_sub.to_str().unwrap(), "content": "x" }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(sub_s.to_string()),
            session_id: Some(sid.to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: main_s.to_string(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(matches!(verdict, Verdict::Allow), "the edit proceeds: {verdict:?}");

        // The work branch was cut IN THE SUBMODULE, named with the SUBMODULE's base.
        assert_eq!(
            current_branch("git", sub_s).as_deref(),
            Some("main_thing"),
            "the work branch uses the submodule's base `main`, not the superproject's `dev`",
        );
        // The superproject HEAD is untouched (still on its own base).
        assert_eq!(
            current_branch("git", main_s).as_deref(),
            Some("dev"),
            "the superproject checkout is untouched",
        );
        assert!(
            context::pending_branch_for(main_s, sid).is_none(),
            "marker consumed after the submodule checkout",
        );
    }

    /// AC-13 — when the work branch cannot be created and the run continues on
    /// the previous branch, the RECORD is rewritten to the real branch and the
    /// warning names BOTH. The marker used to be cleared on failure, so the
    /// intent was destroyed: the only record of the computed branch was a name
    /// that never came to exist. The failure here is deterministic — the
    /// marker carries a ref name git refuses (`..`) — and the tree is dirty,
    /// so the warning also names the paths the pre-check measured.
    #[test]
    fn work_branch_record_reconciles_with_the_real_branch() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();
        seed_flow(root, r#"{"*":"dev","dev":"main"}"#);
        // A WORK branch (not protected) so the failure path warns + proceeds.
        init_repo_on(root, "dev_current");
        // Dirty the tree so the pre-check has something to name.
        std::fs::write(root.join("f.txt"), "dirty").unwrap();

        let sid = "sess-reconcile";
        context::set_pending_branch(root_s, sid, "dev_bad..name");

        let (input, ctx) = pre_edit_input(root_s, sid);
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        let Verdict::Warn { message } = verdict else {
            panic!("a failed cut on a work branch must Warn, got {verdict:?}");
        };
        assert!(
            message.contains("dev_bad..name") && message.contains("dev_current"),
            "the warning must name BOTH the intended and the real branch: {message}"
        );
        assert!(
            message.contains("f.txt"),
            "the warning must name the dirty paths the pre-check measured: {message}"
        );
        // The record now matches reality: rewritten to the branch actually
        // active, not cleared, not the branch that never existed.
        assert_eq!(
            context::pending_branch_for(root_s, sid).as_deref(),
            Some("dev_current"),
            "the marker is reconciled to the real branch",
        );
        assert_eq!(current_branch("git", root_s).as_deref(), Some("dev_current"));

        // The next edit consumes the reconciled record via the
        // already-on-target fast path: silent Allow, marker cleared.
        let (input2, ctx2) = pre_edit_input(root_s, sid);
        let verdict2 = WorkBranchGate.evaluate(&input2, &ctx2).expect("no error");
        assert!(matches!(verdict2, Verdict::Allow), "got {verdict2:?}");
        assert!(
            context::pending_branch_for(root_s, sid).is_none(),
            "the reconciled record is consumed on the next edit",
        );
    }

    /// The in-place nudge: a cut that lands on the MAIN checkout answers Warn
    /// naming the exact isolation step (`EnterWorktree name=<target>`) — and
    /// the edit still proceeds (branch cut, marker consumed). The two carved
    /// exemptions stay silent Allows: a linked worktree
    /// (`nested_worktree_marker_cuts_branch_locally`) and a submodule
    /// (`own_git_root_submodule_resolves_nested_base`).
    #[test]
    fn in_place_cut_warns_with_enter_worktree_hint() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();
        seed_flow(root, r#"{"*":"dev","dev":"main"}"#);
        init_repo_on(root, "dev");

        let sid = "sess-nudge";
        context::set_pending_branch(root_s, sid, "dev_nudged-thing");
        let (input, ctx) = pre_edit_input(root_s, sid);
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        let Verdict::Warn { message } = verdict else {
            panic!("in-place cut on the main checkout must Warn, got {verdict:?}");
        };
        assert!(
            message.contains("EnterWorktree name=dev_nudged-thing"),
            "the nudge names the exact native isolation step: {message}",
        );
        assert!(message.contains("in-place"), "the nudge says why it fired: {message}");
        // Informative, not a refusal: the cut happened and the marker is gone.
        assert_eq!(current_branch("git", root_s).as_deref(), Some("dev_nudged-thing"));
        assert!(context::pending_branch_for(root_s, sid).is_none(), "marker consumed");
    }
}
