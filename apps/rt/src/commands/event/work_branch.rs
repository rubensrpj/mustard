//! The work unit's branch: its NAME, and the CUT that creates it.
//!
//! Two halves, one subject. The naming half is pure: given a spec or intent
//! plus the project's integration base, compute the `{base}_{slug}` work-branch
//! name the unit lives on (the only I/O is reading `mustard.json` for the slug
//! locale). The cutting half runs git: refresh the integration bases from
//! `origin`, then check the branch out, creating it off its base.
//!
//! Both halves live here because three callers must agree about them and a
//! second spelling is how they stop agreeing:
//! [`crate::hooks::write::work_branch_gate`] (the first file mutation of a
//! session), [`crate::commands::spec::spec_draft`] (the draft, which cuts the
//! branch so the spec is written INSIDE the unit rather than on the base), and
//! [`super::emit_pipeline`] (which pre-computes the name into the pending
//! marker). The base set itself is never re-derived here — it comes from
//! [`mustard_core::domain::config::GitConfig`], the single owner.

use std::path::Path;
use std::process::Command;

/// Resolve the effective integration base for the auto-branch prefix.
///
/// - `--base` omitted (or blank) → the project's primary base
///   (`config.git.primary_base()`), as before.
/// - `--base` naming one of `config.git.integration_bases()` → used verbatim.
/// - `--base` naming anything else → `Err` with a didactic message. An
///   explicit base is caller INTENT — silently coercing it to the primary
///   base once sent `--base dev` work onto a `main_*` branch in the field.
///
/// Agnostic — both the accepted set and the fallback come from `git.flow`; no
/// branch name is hardcoded here. Do NOT re-derive the base set ad hoc: the
/// core owns that derivation so `work_branch_gate` and this emitter agree.
pub(crate) fn resolve_base(
    requested: Option<&str>,
    config: &mustard_core::ProjectConfig,
) -> Result<String, String> {
    let bases = config.git.integration_bases();
    match requested.map(str::trim).filter(|b| !b.is_empty()) {
        None => Ok(config.git.primary_base()),
        Some(b) if bases.contains(b) => Ok(b.to_string()),
        Some(b) => Err(format!(
            "base '{b}' não é uma base de integração deste projeto (bases: {}). \
             Declare-a em mustard.json#git.flow ou use uma das bases existentes.",
            bases.iter().cloned().collect::<Vec<_>>().join(", ")
        )),
    }
}

/// A short, ref-safe fallback token from the session id. `unknown`/empty →
/// `work` so the branch always has a non-empty tail.
fn short_sid(sid: &str) -> String {
    let s = sid.trim();
    if s.is_empty() || s == "unknown" {
        return "work".to_string();
    }
    s.chars().take(8).collect()
}

/// Sanitise `{base}_{slug}` into a valid git ref: keep `[A-Za-z0-9-_./]`,
/// map everything else to `-`, collapse `..` runs (git forbids them), and trim
/// leading `-`/`.`/`/` and trailing `/`/`.`. Never empty — floors to `work`.
fn sanitize_git_ref(raw: &str) -> String {
    let mut out: String = raw
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '/' => ch,
            _ => '-',
        })
        .collect();
    while out.contains("..") {
        out = out.replace("..", "-");
    }
    let trimmed = out
        .trim_start_matches(|c| c == '-' || c == '.' || c == '/')
        .trim_end_matches(|c| c == '/' || c == '.');
    if trimmed.is_empty() {
        "work".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Compute the auto-branch name for a `pipeline.kind` work-type signal:
/// `{base}_{slug}`, sanitised to a valid git ref. The `{base}_` prefix records
/// the integration branch the work is cut from, so the gate (and `/git`) can
/// recover the PR-target from the name alone. Slug precedence:
/// 1. `--spec` when present (already a slug — at the base gate that is the
///    name `emit_pipeline` just MINTED, so this leg carries the canonical one);
/// 2. else `--intent` through the canonical derivation
///    ([`crate::commands::spec::spec_slug::canonical_for_project`]) — the SAME
///    one `spec-draft` names the spec directory with, so the branch half and
///    the directory cannot be two different spellings of one unit;
/// 3. else a date-based fallback (`YYYY-MM-DD` from the event `ts`) suffixed
///    with a short session id for uniqueness.
/// Never fails — every branch degrades to a valid ref.
pub(crate) fn compute_work_branch(
    base: &str,
    spec: &str,
    intent: Option<&str>,
    sid: &str,
    ts: &str,
    project: &str,
) -> String {
    let slug = if !spec.trim().is_empty() {
        spec.trim().to_string()
    } else if let Some(intent) = intent.map(str::trim).filter(|s| !s.is_empty()) {
        crate::commands::spec::spec_slug::canonical_for_project(intent, Path::new(project))
    } else {
        // Date-based fallback from the shared event timestamp, plus a short
        // session id so two spec-less/intent-less runs on the same day differ.
        let date = ts.split('T').next().unwrap_or("").trim();
        if date.is_empty() {
            short_sid(sid)
        } else {
            format!("{date}-{}", short_sid(sid))
        }
    };
    sanitize_git_ref(&format!("{base}_{slug}"))
}

// ---------------------------------------------------------------------------
// The cut — git primitives shared by the hook gate and the draft
// ---------------------------------------------------------------------------

/// The current branch name (`git rev-parse --abbrev-ref HEAD`), or `None` on
/// any failure (not a repo, detached HEAD reported as `"HEAD"`, git absent).
pub(crate) fn current_branch(vcs: &str, root: &str) -> Option<String> {
    let out = Command::new(vcs)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(root)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let name = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// `true` when a local branch `refs/heads/<branch>` exists.
fn local_branch_exists(vcs: &str, root: &str, branch: &str) -> bool {
    Command::new(vcs)
        .args([
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .current_dir(root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Run one git subcommand in `root`, mapping a non-zero exit (or spawn error)
/// to `Err(<stderr|io error>)`. Never panics.
fn run_git(vcs: &str, root: &str, args: &[&str]) -> Result<(), String> {
    let out = Command::new(vcs)
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let msg = stderr.trim();
        Err(if msg.is_empty() {
            format!("git exited with status {}", out.status)
        } else {
            msg.to_string()
        })
    }
}

/// Check out `target`, creating it off `base` when it does not yet exist.
/// Carries the working-tree changes along (a plain `checkout`, no stash). If
/// `base` is absent locally, branch off the current HEAD instead. Returns the
/// git error string on failure.
pub(crate) fn checkout_work_branch(
    vcs: &str,
    root: &str,
    target: &str,
    base: &str,
) -> Result<(), String> {
    if local_branch_exists(vcs, root, target) {
        return run_git(vcs, root, &["checkout", target]);
    }
    if local_branch_exists(vcs, root, base) {
        return run_git(vcs, root, &["checkout", "-b", target, base]);
    }
    // Base branch not present locally — branch off the current HEAD.
    run_git(vcs, root, &["checkout", "-b", target])
}

/// Refresh the project's integration bases (`git.flow`) to their `origin`
/// remotes BEFORE a work branch is cut, so the branch is always based on the
/// latest `dev`/`main`. Fire-and-forget: it returns nothing the caller must
/// act on, and every git failure is swallowed. Offline, no remote, or a
/// diverged base never blocks the cut and never panics.
///
/// 1. `git fetch origin` — on failure (offline / no remote) RETURN early and
///    do nothing else; the branch is still cut from the local base.
/// 2. For each integration base `B`:
///    - when `B` is the checked-out branch (`Some(B) == current`) →
///      `git merge --ff-only origin/B` fast-forwards it in place;
///    - otherwise → `git fetch origin B:B`, a refspec fetch git refuses to
///      make non-ff, so it safely fast-forwards the local ref without a
///      checkout.
///    Every per-base error (no matching origin ref, a diverged base, a base
///    checked out in another worktree, …) is ignored — best-effort, keep going.
pub(crate) fn refresh_integration_bases(
    vcs: &str,
    root: &str,
    config: &mustard_core::ProjectConfig,
    current: Option<&str>,
) {
    // Offline / no remote → nothing to refresh; the branch is cut from the
    // local base as before. Do NOT propagate the error.
    if run_git(vcs, root, &["fetch", "origin"]).is_err() {
        return;
    }
    for base in config.git.integration_bases() {
        // Best-effort per base — drop the result either way.
        let _ = if current == Some(base.as_str()) {
            run_git(vcs, root, &["merge", "--ff-only", &format!("origin/{base}")])
        } else {
            run_git(vcs, root, &["fetch", "origin", &format!("{base}:{base}")])
        };
    }
}

/// Recover the integration base a work branch was cut from, from its NAME:
/// among the project's integration bases (`git.flow`), the LONGEST base `B`
/// such that `target` starts with `"{B}_"`. When none match, the project's
/// primary base (`config.git.primary_base()`).
///
/// Longest-match disambiguates nested bases (a `dev_release` base wins over
/// `dev` for `dev_release_x`). Agnostic — the base set and the primary both
/// come from `git.flow`; no branch name is hardcoded.
pub(crate) fn base_for(target: &str, config: &mustard_core::ProjectConfig) -> String {
    let bases = config.git.integration_bases();
    let mut best: Option<&str> = None;
    for b in &bases {
        if target.starts_with(&format!("{b}_")) && best.is_none_or(|cur| b.len() > cur.len()) {
            best = Some(b.as_str());
        }
    }
    best.map_or_else(|| config.git.primary_base(), str::to_string)
}

/// The SLUG half of a work branch — `dev_my-unit` → `my-unit` — read against
/// the project's declared integration bases. The inverse of
/// [`compute_work_branch`]'s `{base}_{slug}` join.
///
/// This is the DURABLE record of a unit's name. The `pending-work-branch`
/// marker that carried the name from the gate is consumed and deleted by the
/// first checkout ([`cut_pending_work_branch`]), so after that moment the
/// branch itself is the only thing that still remembers what the unit is
/// called — which is what lets `spec-draft` consume the gate's name instead of
/// deriving a second one.
///
/// `None` when the name carries no declared `{base}_` prefix: it is then not a
/// work unit's branch at all, and inventing a slug out of it would mint the
/// very third name this module exists to prevent. The longest-match rule is
/// [`crate::commands::work_unit_open::unit_base_of_name`] — the same question
/// the worktree engine asks, never a second parser.
pub(crate) fn slug_of_work_branch(
    branch: &str,
    config: &mustard_core::ProjectConfig,
) -> Option<String> {
    let bases: Vec<String> = config.git.integration_bases().into_iter().collect();
    let base = crate::commands::work_unit_open::unit_base_of_name(branch, &bases)?;
    let slug = branch.strip_prefix(&format!("{base}_"))?.trim();
    (!slug.is_empty()).then(|| slug.to_string())
}

/// `true` when `branch` is a bare integration branch that must never be
/// developed on directly — an exact member of `config.git.integration_bases()`
/// (`dev`, `main`/`master`, `develop`, … whatever `git.flow` declares). The
/// `{base}_*` work branches (`dev_rubens`, `main_close-gate`, …) are NOT
/// protected.
pub(crate) fn is_protected(branch: &str, config: &mustard_core::ProjectConfig) -> bool {
    config.git.integration_bases().contains(branch)
}

/// `true` when the tree HOLDS work that is not this unit's — its HEAD names a
/// branch that is neither `target` nor a bare integration base, i.e. ANOTHER
/// unit's branch.
///
/// The partition is the one both doors already use: an integration base is
/// nobody's work (the ordinary first unit cuts off it in place), and everything
/// else is somebody's. It is deliberately not narrowed to a `{base}_` shape — a
/// hand-made `feature/x` carries edits exactly the same way, and taking its
/// checkout costs exactly the same.
///
/// `None` (unreadable HEAD) and git's `"HEAD"` (a detached checkout) are NOT
/// measurements of a position, so neither counts: an unmeasured HEAD keeps
/// today's cut rather than triggering a refusal the operator did not ask for.
pub(crate) fn holds_other_work(
    current: Option<&str>,
    target: &str,
    config: &mustard_core::ProjectConfig,
) -> bool {
    let Some(branch) = current.filter(|b| *b != "HEAD") else {
        return false;
    };
    branch != target && !is_protected(branch, config)
}

/// How many dirty paths a verdict spells out before summarising — a hook
/// message is one line in the transcript.
const MAX_DIRTY_NAMED: usize = 5;

/// `{paths}` and `{more}` for a dirty-path list: the first
/// [`MAX_DIRTY_NAMED`] names, then ` (+N)` for whatever is left (empty when
/// nothing is). ONE truncation, rendered into two different sentences — the
/// refusal below and the gate's checkout-failure note.
pub(crate) fn name_dirty_paths(dirty: &[String]) -> (String, String) {
    let shown: Vec<&str> = dirty.iter().take(MAX_DIRTY_NAMED).map(String::as_str).collect();
    let more = dirty.len().saturating_sub(shown.len());
    let tail = if more == 0 { String::new() } else { format!(" (+{more})") };
    (shown.join(", "), tail)
}

/// The checkout is BUSY: it holds ANOTHER unit's branch AND that unit's work is
/// still uncommitted, so [`checkout_work_branch`] — a plain checkout, no stash —
/// would carry those edits onto this unit's branch and leave the session already
/// working here on a branch it never asked for.
///
/// The measured facts, kept apart from the sentence built out of them, so the
/// gate and the draft REPORT the same refusal in their own shapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BusyCheckout {
    /// The branch the checkout is on — another unit's.
    pub(crate) current: String,
    /// The branch that was going to be cut here.
    pub(crate) target: String,
    /// The uncommitted paths that would have ridden along.
    pub(crate) dirty: Vec<String>,
}

impl BusyCheckout {
    /// The one sentence both doors say: WHERE the checkout is, WHAT is
    /// uncommitted there, and WHAT to do about it. Catalogue-rendered in the
    /// project's configured language.
    pub(crate) fn reason(&self, lang: mustard_core::platform::i18n::Locale) -> String {
        let (paths, more) = name_dirty_paths(&self.dirty);
        mustard_core::platform::i18n::translate("workbranch.busy.refusal", lang)
            .replace("{current}", &self.current)
            .replace("{target}", &self.target)
            .replace("{paths}", &paths)
            .replace("{more}", &more)
    }
}

/// THE decision — taken once, for both doors. `Some` when cutting `target` in
/// `root` would take a checkout that belongs to another unit AND destroy nothing
/// less than its uncommitted work; `None` when the cut is safe to make.
///
/// Both conditions are required and neither is enough alone: another unit's
/// branch with a CLEAN tree loses nothing (the branch keeps its commits, and the
/// cut still comes off the integration base), while a dirty tree on THIS unit's
/// branch or on a bare base is the ordinary first-unit case.
///
/// The dirt is measured with [`crate::commands::work_unit_open::dirty_paths`] —
/// the same probe the worktree door uses, `.claude/` carved out because it is
/// redirected state rather than code.
pub(crate) fn busy_checkout(
    root: &Path,
    current: Option<&str>,
    target: &str,
    config: &mustard_core::ProjectConfig,
) -> Option<BusyCheckout> {
    if !holds_other_work(current, target, config) {
        return None;
    }
    let dirty = crate::commands::work_unit_open::dirty_paths(root);
    if dirty.is_empty() {
        return None;
    }
    Some(BusyCheckout {
        current: current.unwrap_or_default().to_string(),
        target: target.to_string(),
        dirty,
    })
}

/// What [`cut_pending_work_branch`] did — the closed set, so a caller that must
/// decide (refuse? warn? say nothing?) reads a state instead of guessing from a
/// bool. `NoPending` and `AlreadyThere` are deliberately apart: "no work unit
/// was signalled" and "the unit's branch is already the checkout" both leave
/// git untouched and mean opposite things to the caller.
///
/// No serde derive — the JSON shape belongs to whichever command reports it
/// (`spec-draft` folds it into its own document).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CutOutcome {
    /// No `pending-work-branch` marker for this session (or no VCS at all):
    /// nothing was ever promised, so nothing was cut.
    NoPending,
    /// The checkout was ALREADY the pending branch; the marker was consumed and
    /// git was not touched.
    AlreadyThere(String),
    /// The branch was created (or checked out) by this call. Carries its name.
    Cut(String),
    /// REFUSED: the checkout holds another unit's branch with uncommitted work,
    /// so cutting here would carry that work off ([`busy_checkout`]). Nothing
    /// was touched and the marker is KEPT — the unit was never started, so
    /// there is nothing to consume.
    Refused(BusyCheckout),
    /// Git refused the checkout. Carries the branch that was wanted, the branch
    /// the tree actually sits on (`None` on a detached HEAD / probe failure),
    /// and git's own message. The JUDGEMENT of how bad that is lives in the
    /// caller: staying on an integration base is a refusal for one caller and
    /// merely a warning for another.
    Failed {
        target: String,
        current: Option<String>,
        error: String,
    },
}

/// Consume this session's `pending-work-branch` marker and check that branch
/// out in `project`, creating it off its base.
///
/// The non-hook door to the SAME cut [`crate::hooks::write::work_branch_gate`]
/// performs on the first file mutation. `spec-draft` calls it because the spec
/// must be written INSIDE the unit: the draft is the first thing the work
/// produces, and it used to land on the integration base (a `.claude/spec/`
/// carve-out existed precisely to let it). Cutting here moves the draft, the
/// wave layout and the negative proof onto the branch, in that one call.
///
/// Idempotent by construction: the marker is cleared on every outcome that
/// leaves the tree on the target branch, so a second call answers `NoPending`.
/// The marker is KEPT on a failure and on a refusal — the intent survives for a
/// retry, exactly as the hook gate keeps it.
///
/// The refusal is the point the review found missing: this door opens FIRST
/// (`spec-draft` calls it at approval, before any `Write` reaches the hook
/// gate), so a guard living only in the gate never ran. The decision is
/// [`busy_checkout`], the same one the gate takes — one predicate, one message,
/// two doors.
pub(crate) fn cut_pending_work_branch(project: &Path, session: &str) -> CutOutcome {
    let config = mustard_core::ProjectConfig::load(project);
    // An explicit `vcs: ""` opt-out (or a non-git tree) means there is no
    // branch to cut and nothing to guard.
    let Some(vcs) = config.vcs() else {
        return CutOutcome::NoPending;
    };
    let root = project.to_string_lossy().into_owned();
    let Some(target) = crate::shared::context::pending_branch_for(&root, session) else {
        return CutOutcome::NoPending;
    };

    let current = current_branch(&vcs, &root);
    if current.as_deref() == Some(target.as_str()) {
        crate::shared::context::clear_pending_branch(&root, session);
        return CutOutcome::AlreadyThere(target);
    }

    // The checkout may belong to ANOTHER unit that has not committed yet:
    // refuse before touching git, so its work stays where its author left it.
    if let Some(busy) = busy_checkout(project, current.as_deref(), &target, &config) {
        return CutOutcome::Refused(busy);
    }

    // Refresh from origin FIRST so the unit is cut from the latest base.
    refresh_integration_bases(&vcs, &root, &config, current.as_deref());
    let base = base_for(&target, &config);
    match checkout_work_branch(&vcs, &root, &target, &base) {
        Ok(()) => {
            crate::shared::context::clear_pending_branch(&root, session);
            CutOutcome::Cut(target)
        }
        Err(error) => CutOutcome::Failed {
            target,
            current,
            error,
        },
    }
}

#[cfg(test)]
mod tests {
    // -----------------------------------------------------------------------
    // Auto-branch name computation (porta-unica)
    // -----------------------------------------------------------------------

    #[test]
    fn compute_work_branch_prefers_spec_slug_off_primary_base() {
        // base = the primary/`*` base → `{base}_{slug}`, kind dropped from name.
        let b = super::compute_work_branch("dev", "2026-07-02-my-spec", None, "sess-abcdef12", "2026-07-02T10:00:00.000Z", "/no/project");
        assert_eq!(b, "dev_2026-07-02-my-spec");
        // Task example.
        let b2 = super::compute_work_branch("dev", "parcelas-virtuais", None, "sess-abcdef12", "2026-07-02T10:00:00.000Z", "/no/project");
        assert_eq!(b2, "dev_parcelas-virtuais");
    }

    #[test]
    fn compute_work_branch_off_non_primary_base() {
        // base = a non-primary integration base (e.g. `main`) → prefix records it.
        let b = super::compute_work_branch("main", "close-gate-windows", None, "sess-abcdef12", "2026-07-02T10:00:00.000Z", "/no/project");
        assert_eq!(b, "main_close-gate-windows");
    }

    #[test]
    fn compute_work_branch_falls_back_to_intent_slug() {
        // No spec → the intent is slugified (pt-BR strips accents by default).
        let b = super::compute_work_branch("main", "", Some("Corrigir botão de login"), "sess-abcdef12", "2026-07-02T10:00:00.000Z", "/no/project");
        assert_eq!(b, "main_corrigir-botao-login");
    }

    #[test]
    fn compute_work_branch_date_fallback_when_no_spec_or_intent() {
        // No spec, no intent → date-from-ts + short session id.
        let b = super::compute_work_branch("dev", "", None, "sess-abcdef1234", "2026-07-02T10:00:00.000Z", "/no/project");
        assert_eq!(b, "dev_2026-07-02-sess-abc");
    }

    #[test]
    fn compute_work_branch_sanitizes_unsafe_slug() {
        // A spec with unsafe chars is sanitised into a valid ref.
        let b = super::compute_work_branch("dev", "weird ..slug/", None, "unknown", "2026-07-02T10:00:00.000Z", "/no/project");
        // ".." collapsed, spaces mapped to '-', trailing '/' trimmed.
        assert_eq!(b, "dev_weird--slug");
        assert!(!b.contains(".."), "no `..` runs in a git ref");
        assert!(!b.starts_with('-'), "no leading dash");
    }

    #[test]
    fn resolve_base_honours_requested_when_in_bases() {
        // Standard two-tier flow → integration bases {dev, main}, primary = dev.
        let mut config = mustard_core::ProjectConfig::default();
        config.git.flow.insert("*".to_string(), "dev".to_string());
        config.git.flow.insert("dev".to_string(), "main".to_string());
        // A requested base that IS an integration base is used verbatim.
        assert_eq!(super::resolve_base(Some("main"), &config), Ok("main".to_string()));
        assert_eq!(super::resolve_base(Some("dev"), &config), Ok("dev".to_string()));
        // No request → primary. Blank counts as omitted.
        assert_eq!(super::resolve_base(None, &config), Ok("dev".to_string()));
        assert_eq!(super::resolve_base(Some("  "), &config), Ok("dev".to_string()));
    }

    #[test]
    fn resolve_base_errors_loudly_on_unknown_explicit_base() {
        let mut config = mustard_core::ProjectConfig::default();
        config.git.flow.insert("*".to_string(), "dev".to_string());
        config.git.flow.insert("dev".to_string(), "main".to_string());
        // An EXPLICIT base outside the declared set is an error, never a
        // silent coercion to the primary.
        let err = super::resolve_base(Some("feature/x"), &config).unwrap_err();
        assert!(err.contains("feature/x"), "names the rejected base: {err}");
        assert!(err.contains("git.flow"), "points at the config: {err}");
        assert!(err.contains("dev") && err.contains("main"), "lists declared bases: {err}");

        // Agnostic: a develop/master project resolves against ITS bases —
        // the exact field bug: `--base dev` on an undeclared flow must error,
        // not silently become the primary base.
        let mut dm = mustard_core::ProjectConfig::default();
        dm.git.flow.insert("*".to_string(), "develop".to_string());
        dm.git.flow.insert("develop".to_string(), "master".to_string());
        assert_eq!(super::resolve_base(Some("master"), &dm), Ok("master".to_string()));
        assert!(super::resolve_base(Some("dev"), &dm).is_err(), "unknown base → loud error");
    }

    // -----------------------------------------------------------------------
    // The cut itself — the door that opens FIRST
    // -----------------------------------------------------------------------

    /// Run git in `root`, asserting success — test scaffolding only.
    fn git(root: &std::path::Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(ok, "git {args:?} failed");
    }

    /// AC-11 — the CUT itself refuses a busy checkout.
    ///
    /// This test deliberately drives [`super::cut_pending_work_branch`] and NOT
    /// `WorkBranchGate::evaluate`: the previous round's tests all went through
    /// the gate and passed while the real defect sat here. `spec-draft` calls
    /// this function at APPROVAL — before any `Write` exists for a PreToolUse
    /// hook to see — so a guard living only in the gate was a guard on the door
    /// that opens second.
    #[test]
    fn the_branch_cut_itself_refuses_a_busy_checkout() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let root_s = root.to_string_lossy().to_string();
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#,
        )
        .expect("cfg");
        git(root, &["init"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["checkout", "-b", "dev"]);
        std::fs::write(root.join("f.txt"), "seed").expect("seed");
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "init"]);

        // A FIRST unit holds the checkout, with its work still uncommitted.
        git(root, &["checkout", "-b", "dev_first"]);
        std::fs::write(root.join("f.txt"), "first unit, uncommitted").expect("dirty");

        // A SECOND unit is signalled — this is what `spec-draft` consumes.
        let sid = "sess-cut-refuses";
        crate::shared::context::set_pending_branch(&root_s, sid, "dev_second");

        let outcome = super::cut_pending_work_branch(root, sid);
        let super::CutOutcome::Refused(busy) = outcome else {
            panic!("the cut must refuse a busy checkout, got {outcome:?}");
        };
        assert_eq!(busy.current, "dev_first");
        assert_eq!(busy.target, "dev_second");
        assert!(busy.dirty.contains(&"f.txt".to_string()), "{:?}", busy.dirty);
        let reason = busy.reason(mustard_core::platform::i18n::Locale::EnUs);
        assert!(
            reason.contains("dev_first") && reason.contains("dev_second") && reason.contains("f.txt"),
            "the refusal names both branches and the work at risk: {reason}",
        );

        // Nothing was touched: the checkout still holds the first unit, its
        // uncommitted work is intact, and the second branch does not exist.
        assert_eq!(super::current_branch("git", &root_s).as_deref(), Some("dev_first"));
        assert_eq!(
            std::fs::read_to_string(root.join("f.txt")).expect("read"),
            "first unit, uncommitted",
        );
        assert!(
            !super::local_branch_exists("git", &root_s, "dev_second"),
            "a refused cut creates no branch",
        );
        // The marker SURVIVES — the unit was never started, so nothing was
        // consumed and the next attempt retries after the commit or stash.
        assert_eq!(
            crate::shared::context::pending_branch_for(&root_s, sid).as_deref(),
            Some("dev_second"),
            "a refusal consumes no intent",
        );
    }

    /// The counterweight: with the SAME arrangement minus the uncommitted work,
    /// the cut proceeds. Nothing rides along from a clean tree, so refusing
    /// there would be friction with no defect behind it.
    #[test]
    fn a_clean_checkout_lets_the_cut_through() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let root_s = root.to_string_lossy().to_string();
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#,
        )
        .expect("cfg");
        git(root, &["init"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["checkout", "-b", "dev"]);
        std::fs::write(root.join("f.txt"), "seed").expect("seed");
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "init"]);
        git(root, &["checkout", "-b", "dev_first"]);
        std::fs::write(root.join("f.txt"), "first unit, committed").expect("work");
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "first unit work"]);

        let sid = "sess-cut-clean";
        crate::shared::context::set_pending_branch(&root_s, sid, "dev_second");
        let outcome = super::cut_pending_work_branch(root, sid);
        assert_eq!(outcome, super::CutOutcome::Cut("dev_second".to_string()), "{outcome:?}");
        assert_eq!(super::current_branch("git", &root_s).as_deref(), Some("dev_second"));
    }
}
