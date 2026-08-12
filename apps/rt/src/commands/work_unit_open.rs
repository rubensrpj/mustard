//! `mustard-rt run work-unit-open` — the ENTRY RITUAL of a work unit: create
//! (idempotently) the unit's isolated worktree so the orchestrator can switch
//! the session into it (`EnterWorktree path=<returned path>`) instead of
//! mutating the main checkout with an in-place `checkout -b`.
//!
//! Counterpart of [`crate::commands::git_settle`] (the exit ritual): open cuts
//! `.claude/worktrees/{base}_{slug}` from a fresh `origin/{base}`; settle
//! verifies the merge and prunes the same worktree. Cleanup of these worktrees
//! is git-settle's job EXCLUSIVELY — `worktree-gc` collects only worktrees
//! that are NOT work units ([`is_unit_worktree_name`]) and never touches one.
//!
//! Branch naming reuses [`super::event::work_branch`] so the worktree branch
//! is byte-identical to the `pending-work-branch` marker `emit-pipeline`
//! wrote; inside the worktree the gate then finds the branch already checked
//! out and stays silent.
//!
//! Machine-local settings are NOT copied in: since Claude Code v2.1.211 the
//! repo's `.claude/settings.local.json` is resolved to the MAIN checkout from
//! inside any worktree — a per-worktree copy would only shadow it (undocumented
//! precedence) and freeze arrangements at open time.
//!
//! The project's own ENVIRONMENT does travel, but only what the project
//! DECLARES: `mustard.json#worktree.carry` is copied in and `.link` is pointed
//! at the main checkout ([`carry_environment`]). A fresh cut carries versioned
//! files and submodules only, so without that step every git-ignored path the
//! project needs to run — `.env`, `node_modules` — is simply missing.
//!
//! Error posture: config/user/state errors are LOUD (`ok:false` + exit 1) —
//! an unknown `--base` here is the same disease `resolve_base` now rejects at
//! emit time. Only the network is forgiving: a failed `git fetch origin` never
//! blocks, the cut degrades to the local base ref (`fetched:false` reports it).

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::commands::git_settle::{git_ok, git_out, main_checkout_root, parse_worktrees};

/// Where Claude Code itself puts worktrees, relative to the main checkout.
/// Mirrored so a hook-managed worktree lands exactly where the native one would
/// — the `WorktreeCreate` event names the worktree but never says where to put
/// it, leaving the layout to whoever replaces the native `git worktree add`.
const WORKTREES_RELDIR: &str = ".claude/worktrees";

/// The declared integration base a worktree NAME belongs to, read from its
/// longest `{base}_` prefix — `None` when the name is not a work unit's.
///
/// This is the ONE criterion that separates a work unit's worktree from every
/// other worktree the harness may cut, and it is derived from the project's own
/// `git.flow` rather than from any name SHAPE. There is no `agent-` prefix to
/// key on: `WorktreeCreate` documents `name` as a slug identifier — a
/// user-supplied one (`feature-auth`), a `pr-<number>`, or an auto-generated
/// `bright-running-fox` — and this repository's only harness-cut worktree is
/// `.claude/worktrees/recursing-benz-063389`. Keying on a prefix that never
/// appears made `worktree-gc` match nothing at all, so the collector and this
/// engine now ask the one same question, of the same declared bases.
pub(crate) fn unit_base_of_name(name: &str, bases: &[String]) -> Option<String> {
    bases
        .iter()
        .filter(|b| name.starts_with(&format!("{b}_")))
        .max_by_key(|b| b.len())
        .cloned()
}

/// Whether a worktree NAME is a work unit's — see [`unit_base_of_name`].
/// Everything else (a subagent's isolated checkout, a background session's, a
/// desktop one) is not a unit, and is what `worktree-gc` may collect.
pub(crate) fn is_unit_worktree_name(name: &str, bases: &[String]) -> bool {
    unit_base_of_name(name, bases).is_some()
}

/// How many dirty paths the refusal message spells out before summarising.
const MAX_DIRTY_SHOWN: usize = 20;

/// Options for `mustard-rt run work-unit-open`.
pub struct WorkUnitOpenOpts {
    /// Any directory inside the repo (worktrees welcome — the command resolves
    /// the main checkout itself). Defaults to the current dir.
    pub root: PathBuf,
    /// Full work-branch name override (e.g. `dev_my-spec`). Its `{base}_`
    /// prefix MUST name a declared integration base.
    pub branch: Option<String>,
    /// Spec slug — used verbatim as the branch slug (parity with emit-pipeline).
    pub spec: Option<String>,
    /// Free-form intent, slugified when `--spec` is absent (parity with
    /// emit-pipeline).
    pub intent: Option<String>,
    /// Integration base; STRICT — must name a declared base. Omitted → primary.
    pub base: Option<String>,
}

/// Run `git` in `dir`, `Err(stderr)` on failure — for the calls whose failure
/// text the orchestrator must see (worktree add conflicts).
fn git_try(dir: &Path, args: &[&str]) -> Result<(), String> {
    match std::process::Command::new("git").args(args).current_dir(dir).output() {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => Err(e.to_string()),
    }
}

/// Whether a git ref exists (branch or remote-tracking), quiet.
fn ref_exists(dir: &Path, full_ref: &str) -> bool {
    git_ok(dir, &["rev-parse", "--verify", "--quiet", full_ref])
}

/// The checkout that ALREADY has `branch` out — the main checkout, a linked
/// worktree, anywhere git knows about. `None` when no tree holds it.
///
/// It parses the RAW porcelain instead of going through
/// [`git_settle::parse_worktrees`], and that is the whole point: that parser
/// keeps only entries under `.claude/worktrees/`, so the tree this question is
/// really about — the MAIN checkout — is invisible to it. Since `spec-draft`
/// cuts the unit's branch in the main checkout at approval, the branch is
/// normally already out there by the time EXECUTE asks to isolate, and
/// `git worktree add` refuses a branch another tree holds with exit 128.
///
/// One probe, two callers ([`open_at`] and [`hook_create`]), so the manual face
/// and the `WorktreeCreate` hook cannot disagree about whether a unit is already
/// isolated.
pub(crate) fn checkout_holding_branch(main: &Path, branch: &str) -> Option<String> {
    let porcelain = git_out(main, &["worktree", "list", "--porcelain"])?;
    let mut path: Option<String> = None;
    for line in porcelain.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(p.trim().replace('\\', "/"));
        } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
            if b.trim() == branch {
                return path;
            }
        } else if line.trim().is_empty() {
            path = None;
        }
    }
    None
}

/// The work-unit branch the INVOKING tree sits on, `None` when it sits on
/// anything else (an integration base, a detached HEAD, an unreadable repo).
///
/// Read from the TREE, never from the requested worktree name: a non-unit name
/// (a slug such as `recursing-benz-063389`) carries no base and no slug, so the
/// unit can only come from where the hook was invoked. The shape and the
/// longest-match rule mirror
/// `work_branch_gate::base_for` — the parser of what
/// [`super::event::work_branch`] produces.
fn current_unit_branch(cwd: &Path, bases: &[String]) -> Option<String> {
    let branch = git_out(cwd, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let branch = branch.trim();
    // `HEAD` is git's answer for a detached checkout — not a branch name.
    if branch.is_empty() || branch == "HEAD" {
        return None;
    }
    bases
        .iter()
        .any(|b| branch.starts_with(&format!("{b}_")))
        .then(|| branch.to_string())
}

/// Start ref for a NON-UNIT worktree name (a subagent's or a desktop session's
/// slug), as an explicit cascade — each step tried ONLY when the one before it
/// does not resolve:
///
/// 1. the CURRENT work unit's HEAD, when the invoking tree sits on one. A
///    checkout cut from inside a unit must see that unit's commits; cutting
///    from an integration base would silently hand the agent older code.
/// 2. `origin/{primary_base}` from `mustard.json#git.flow`. `origin/HEAD` is
///    the REMOTE's opinion of a default (`origin/main` here) while the project
///    declares `"*": "dev"` — the declared flow is the authority, and it is
///    already loaded config, not a new source of truth.
/// 3. `origin/HEAD` when resolvable, else the local `HEAD` — today's behaviour,
///    kept verbatim for a project that declares no flow.
///
/// Freshness is best-effort exactly like `work_branch_gate`'s
/// `refresh_integration_bases`: the fetch may fail (offline, no remote,
/// diverged) and the cut then degrades to the LOCAL base. A stale-but-local
/// base is a worse cut, never a failure — a non-zero exit here would ABORT the
/// worktree creation.
fn non_unit_start(
    main: &Path,
    cwd: &Path,
    config: &mustard_core::ProjectConfig,
    bases: &[String],
) -> String {
    if let Some(unit) = current_unit_branch(cwd, bases) {
        return unit;
    }
    // An ABSENT `git.flow` has no declared primary — `primary_base()` would
    // answer its hardcoded last resort, which is exactly the guess step 3
    // already makes better. Only a DECLARED flow speaks here.
    if !config.git.flow.is_empty() {
        let primary = config.git.primary_base();
        git_ok(main, &["fetch", "origin", &primary]);
        if ref_exists(main, &format!("refs/remotes/origin/{primary}")) {
            return format!("origin/{primary}");
        }
        if ref_exists(main, &format!("refs/heads/{primary}")) {
            return primary;
        }
    }
    git_ok(main, &["fetch", "origin"]);
    git_out(main, &["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"])
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "HEAD".to_string())
}

/// Working-tree paths `git add -A` would stage, EXCLUDING `.claude/`.
///
/// `.claude/` is redirected state, not code (`io::workspace` remaps it to the
/// MAIN checkout from inside any worktree), so its dirt never belongs to the
/// code a fresh checkout runs against — the same carve-out the harness's other
/// gates make. Untracked files DO count: a new source file that never travels
/// is the same defect as a modified one.
///
/// Fail-open: no git, not a repository, or a failed probe yields an EMPTY list
/// (read as "clean"). The refusal only ever stands on a positive observation.
pub(crate) fn dirty_paths(dir: &Path) -> Vec<String> {
    let Some(out) = git_out(dir, &["status", "--porcelain"]) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    for line in out.lines() {
        // `XY <path>`, but NEVER sliced at a fixed column: `git_out` trims the
        // whole output, so the FIRST entry loses its leading status space
        // (`" M a.txt"` arrives as `"M a.txt"`). Split on the first space
        // instead — the status codes never contain one, the path may.
        let Some((code, rest)) = line.trim_start().split_once(' ') else { continue };
        if code.len() > 2 || !code.chars().all(|c| "MADRCU?!".contains(c)) {
            continue; // not a status entry we understand — skip, never guess
        }
        // A rename reports `old -> new`; the destination is the live path.
        let rest = rest.trim_start();
        let path = rest.rsplit(" -> ").next().unwrap_or(rest).trim().trim_matches('"');
        if path.is_empty() || path == ".claude" || path.starts_with(".claude/") {
            continue;
        }
        paths.push(path.to_string());
    }
    paths
}

/// The open pass — the testable core of [`run`]. Never panics.
pub(crate) fn open_at(opts: &WorkUnitOpenOpts) -> Value {
    let Some(main) = main_checkout_root(&opts.root) else {
        return json!({ "ok": false, "reason": "not-a-git-repo" });
    };
    let config = mustard_core::ProjectConfig::load(&main);
    let bases: Vec<String> = config.git.integration_bases().into_iter().collect();

    // Resolve the target branch + its base — every mismatch is loud, never a
    // silent fallback (an explicit input is caller intent).
    let (target, base) = match opts.branch.as_deref().map(str::trim).filter(|b| !b.is_empty()) {
        Some(b) => {
            // Longest declared `{B}_` prefix — the gate's rule, minus its
            // primary-base fallback: a branch without a base prefix is not a
            // work unit and is refused (mirrors git-settle's `no-base-prefix`).
            let Some(prefix) = unit_base_of_name(b, &bases) else {
                return json!({ "ok": false, "reason": "no-base-prefix", "branch": b });
            };
            if let Some(req) = opts.base.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                if req != prefix {
                    return json!({
                        "ok": false,
                        "reason": "base-mismatch",
                        "branch": b,
                        "prefix": prefix,
                        "base": req,
                    });
                }
            }
            (b.to_string(), prefix)
        }
        None => {
            let base = match super::event::work_branch::resolve_base(opts.base.as_deref(), &config)
            {
                Ok(b) => b,
                Err(msg) => return json!({ "ok": false, "reason": "unknown-base", "error": msg }),
            };
            let spec = opts.spec.as_deref().map(str::trim).unwrap_or("");
            let intent = opts.intent.as_deref().map(str::trim).filter(|s| !s.is_empty());
            // The date+session fallback of `compute_work_branch` would NOT
            // reproduce the marker emit-pipeline wrote in another session —
            // determinism over convenience: require an explicit slug source.
            if spec.is_empty() && intent.is_none() {
                return json!({
                    "ok": false,
                    "reason": "missing-slug",
                    "hint": "pass --spec, --intent or --branch",
                });
            }
            let main_str = main.to_string_lossy().to_string();
            let target = super::event::work_branch::compute_work_branch(
                &base,
                spec,
                intent,
                &crate::shared::context::session_id(),
                &mustard_core::time::now_iso8601(),
                &main_str,
            );
            (target, base)
        }
    };

    let Ok(paths) = mustard_core::io::claude_paths::ClaudePaths::for_project(&main) else {
        return json!({ "ok": false, "reason": "invalid-project-root" });
    };
    let wt_path = paths.claude_dir().join("worktrees").join(&target);
    let wt_str = wt_path.to_string_lossy().replace('\\', "/");

    // Idempotency FIRST: an already-registered worktree for this branch is the
    // answer, wherever it lives — the registration is the source of truth.
    let entries = git_out(&main, &["worktree", "list", "--porcelain"])
        .map(|s| parse_worktrees(&s))
        .unwrap_or_default();
    if let Some(e) = entries.iter().find(|e| e.branch == target) {
        return json!({
            "ok": true,
            "path": e.path,
            "branch": target,
            "base": base,
            "created": false,
            "fetched": false,
        });
    }
    // The branch may already be CHECKED OUT somewhere else — normally the MAIN
    // checkout, because `spec-draft` cuts the unit's branch there at approval
    // and the whole unit (spec, waves, ceremony, code) is written inside it.
    // That branch IS the isolation, so the unit is already isolated in place:
    // report it and touch nothing. Attempting the cut here is what git refuses
    // with exit 128, which turned the EXECUTE step into a hard failure on the
    // arrangement that is now the DEFAULT.
    //
    // A worktree is still cut whenever the branch is NOT already out — that is
    // the parallel-work case, and it keeps several units in flight at once.
    if let Some(path) = checkout_holding_branch(&main, &target) {
        return json!({
            "ok": true,
            "path": path,
            "branch": target,
            "base": base,
            "created": false,
            "inPlace": true,
            "fetched": false,
        });
    }
    if wt_path.exists() {
        // Unregistered leftover dir — never clobber someone's files.
        return json!({ "ok": false, "reason": "path-occupied", "path": wt_str });
    }

    // Freshness — the ONLY forgiving step: offline cuts from the local ref.
    let fetched = git_ok(&main, &["fetch", "origin", &base]);

    let add = if ref_exists(&main, &format!("refs/heads/{target}")) {
        // The branch already exists (e.g. the gate cut it in-place earlier):
        // attach it, never re-cut — its commits are the unit's history.
        git_try(&main, &["worktree", "add", &wt_str, &target])
    } else {
        let origin_ref = format!("origin/{base}");
        let start = if ref_exists(&main, &format!("refs/remotes/origin/{base}")) {
            origin_ref.as_str()
        } else if ref_exists(&main, &format!("refs/heads/{base}")) {
            base.as_str()
        } else {
            return json!({ "ok": false, "reason": "base-not-found", "base": base, "fetched": fetched });
        };
        git_try(&main, &["worktree", "add", "-b", &target, &wt_str, start])
    };
    if let Err(error) = add {
        // A state conflict (branch checked out elsewhere, locked path…) the
        // orchestrator must see — loud, unlike the network step above.
        return json!({
            "ok": false,
            "reason": "worktree-add-failed",
            "branch": target,
            "error": error,
            "fetched": fetched,
        });
    }

    json!({
        "ok": true,
        "path": wt_str,
        "branch": target,
        "base": base,
        "created": true,
        "fetched": fetched,
    })
}

/// The `WorktreeCreate` hook engine: create the worktree the harness NAMED and
/// return the path to echo. Naming decides the cut:
///
/// - `{base}_…` with a DECLARED base → work unit: fetch + cut from a fresh
///   `origin/{base}` (attach the branch if it already exists).
/// - `prefix_…` with an UNDECLARED prefix → `Err` (didactic — almost certainly
///   a mistyped base; silent coercion is the disease this crate just cured).
/// - anything else — the slug the harness actually hands over
///   (`recursing-benz-063389`, `feature-auth`, `pr-1234`) → [`non_unit_start`]'s
///   cascade: the current work unit's HEAD, else `origin/{primary_base}` from
///   the DECLARED `git.flow`, else the historical `origin/HEAD`/local `HEAD`.
///   Background isolation must never break.
///
/// A NON-UNIT name additionally requires a CLEAN tree. A fresh checkout
/// carries only COMMITTED code, so uncommitted work in the invoking tree would
/// not travel and the agent would run against the older version — most visibly
/// after `/scan`, which rewrites each subproject's `CLAUDE.md` `## Guards` and
/// its `{role}-pattern` skills next to the code. This is the ONE place here
/// that blocks, and it blocks by `Err`: a non-zero exit ABORTS creation with
/// stderr shown to the user, which IS this event's protocol (the same way a
/// `Deny` is a gate's). It mirrors `scan_clean_gate`, which already refuses
/// `/scan` on a dirty tree for the `add -A` reason. Unit worktrees are
/// untouched — nothing outside them depends on their tree. The precondition is
/// keyed on [`is_unit_worktree_name`], the SAME question the cut below asks, so
/// the two cannot drift; the earlier `agent-` prefix was a shape the platform
/// never emits, which left this refusal permanently silent.
///
/// The event hands over a NAME, never a path (`worktree_path` is the *Remove*
/// twin's field), so placing the worktree is this engine's call: it mirrors the
/// harness layout, `{main}/.claude/worktrees/{name}`, anchored at the MAIN
/// checkout rather than `cwd` — the hook may be invoked from any subdirectory,
/// and a worktree cut relative to the caller would land outside the repo.
///
/// An already-registered branch returns its registered path (idempotent).
pub(crate) fn hook_create(worktree_name: &str, cwd: &Path) -> Result<String, String> {
    let name = worktree_name.trim().to_string();
    if name.is_empty() {
        return Err("WorktreeCreate: `name` empty in hook input".to_string());
    }
    if name.contains('/') || name.contains('\\') {
        return Err(format!("WorktreeCreate: `name` must not contain a path separator: {name}"));
    }
    let Some(main) = main_checkout_root(cwd) else {
        return Err("WorktreeCreate: not a git repository".to_string());
    };
    let requested = format!("{}/{}/{}", main.display(), WORKTREES_RELDIR, name).replace('\\', "/");
    let wt_path = PathBuf::from(&requested);

    // Idempotency: a registration for this branch is the answer.
    let entries = git_out(&main, &["worktree", "list", "--porcelain"])
        .map(|s| parse_worktrees(&s))
        .unwrap_or_default();
    if let Some(e) = entries.iter().find(|e| e.branch == name) {
        return Ok(e.path.clone());
    }
    // Already checked out somewhere — the main checkout after `spec-draft` cut
    // the unit's branch there at approval. That tree IS the isolation, so name
    // it and let the session enter it. Falling through would run `git worktree
    // add` on a branch another tree holds, which git refuses with exit 128 —
    // and a non-zero exit here ABORTS the whole `EnterWorktree` (see the
    // module doc of [`crate::hooks::worktree_create`]). Degrade, never fail.
    if let Some(path) = checkout_holding_branch(&main, &name) {
        return Ok(path);
    }
    if wt_path.exists() {
        return Err(format!("WorktreeCreate: path already occupied: {requested}"));
    }

    let wt_str = requested.replace('\\', "/");
    let config = mustard_core::ProjectConfig::load(&main);
    let bases: Vec<String> = config.git.integration_bases().into_iter().collect();
    let unit_base = unit_base_of_name(&name, &bases);

    // Not a work unit → a mistyped base is refused BEFORE anything else looks
    // at the tree: it is a naming error, and blaming a dirty tree for it would
    // be misleading. (Such a name never becomes a worktree at all.)
    if unit_base.is_none() {
        if let Some((prefix, _)) = name.split_once('_') {
            return Err(format!(
                "WorktreeCreate: '{prefix}' (from '{name}') is not an integration base of this \
                 project (bases: {}). Declare it in mustard.json#git.flow or use a name without \
                 '_'.",
                bases.join(", ")
            ));
        }
        // Clean-tree precondition — NON-UNIT worktrees only, and the one
        // refusal in this engine that is deliberate. Checked on the INVOKING
        // tree, which is the tree the cascade derives the cut from.
        let dirty = dirty_paths(cwd);
        if !dirty.is_empty() {
            let shown: Vec<&str> = dirty.iter().take(MAX_DIRTY_SHOWN).map(String::as_str).collect();
            let more = dirty.len().saturating_sub(shown.len());
            let tail =
                if more == 0 { String::new() } else { format!("\n  … and {more} more path(s)") };
            return Err(format!(
                "WorktreeCreate: refusing to cut the isolated worktree '{name}' — the working tree \
                 has uncommitted changes:\n  {}{tail}\n\
                 A fresh worktree carries only COMMITTED code, so this work would NOT travel and \
                 the agent would run against the older version — most visibly after /scan, which \
                 rewrites each subproject's CLAUDE.md ## Guards and its {{role}}-pattern skills \
                 next to the code.\n\
                 Commit or stash the paths above, then re-run. (`.claude/` is exempt — it is \
                 redirected state, not code.)",
                shown.join("\n  ")
            ));
        }
    }

    let start = if let Some(base) = unit_base {
        // Work unit: freshness first (network is the one forgiving step).
        git_ok(&main, &["fetch", "origin", &base]);
        if ref_exists(&main, &format!("refs/remotes/origin/{base}")) {
            format!("origin/{base}")
        } else if ref_exists(&main, &format!("refs/heads/{base}")) {
            base
        } else {
            return Err(format!("WorktreeCreate: base '{base}' not found in the repository"));
        }
    } else {
        non_unit_start(&main, cwd, &config, &bases)
    };

    let add = if ref_exists(&main, &format!("refs/heads/{name}")) {
        git_try(&main, &["worktree", "add", &wt_str, &name])
    } else {
        git_try(&main, &["worktree", "add", "-b", &name, &wt_str, &start])
    };
    add.map_err(|e| format!("WorktreeCreate: git worktree add failed: {e}"))?;
    let _ = init_submodules(&wt_path);
    let _ = carry_environment(&main, &wt_path, &config);
    Ok(wt_str)
}

/// Populate a FRESHLY CUT worktree's submodules — `git worktree add` registers
/// every gitlink but leaves its directory EMPTY, and nothing downstream knows
/// the difference between "submodule not checked out" and "subtree that does
/// not exist": the scan's walk visits the dir, finds zero files, and mines a
/// model missing that whole subtree without a word. Whatever a project keeps in
/// a submodule, it loses entirely.
///
/// This is the other half of plugging the native `git worktree add` — the
/// native cut populates submodules, so replacing it without this step is a
/// regression the hook could not exhibit while it was dead (it aborted every
/// creation by reading the *Remove* twin's field).
///
/// Trigger is the worktree's own `.gitmodules` — the FACT on disk — not
/// `mustard.json#git.submodules`, which is a declaration made at `init` time
/// and goes stale the moment a submodule is added. No `.gitmodules` → no-op.
///
/// Called ONLY on the fresh-cut path, never on the idempotent one: `submodule
/// update` checks out the recorded commit, which in an ALREADY-populated
/// worktree could move a submodule the caller is working in. A new worktree has
/// no work to lose.
///
/// Forgiving like `fetch` — the network is the one step allowed to fail. A
/// submodule that cannot be fetched must not abort the creation (a non-zero
/// exit here kills the whole `EnterWorktree`), so the failure degrades to a
/// loud WARN: the worktree is usable, and the operator is told the one thing
/// that matters — a scan run here would mine an INCOMPLETE model.
///
/// Returns the decision so a test can observe it: `None` = nothing declared,
/// `Some(Ok)` = populated, `Some(Err)` = attempted and failed (degraded). That
/// the populating command itself works is a property of git, verified out of
/// band; a local-path submodule cannot be cloned in-process without relaxing
/// `protocol.file.allow` (CVE-2022-39253), and this crate does not mutate the
/// environment (`std::env::set_var` is `unsafe` under edition 2024).
fn init_submodules(worktree: &Path) -> Option<Result<(), String>> {
    if !worktree.join(".gitmodules").exists() {
        return None;
    }
    let outcome = git_try(worktree, &["submodule", "update", "--init", "--recursive"]);
    if let Err(e) = &outcome {
        eprintln!(
            "WorktreeCreate: WARN: submodules not populated in {}: {e}\n\
             The worktree exists and is usable, but the submodule directories are EMPTY — \
             a /scan from here would mine an INCOMPLETE model (silently). \
             Run `git submodule update --init --recursive` before scanning.",
            worktree.display()
        );
    }
    Some(outcome)
}

/// How a declared path travels into a fresh worktree — the two verbs of
/// `mustard.json#worktree`, whose costs are OPPOSITE (see
/// [`mustard_core::WorktreeConfig`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verb {
    /// A real COPY. Small, environment-specific, and authored in place: a link
    /// would leak the worktree's edits back into the main checkout.
    Carry,
    /// A POINTER at the main checkout's directory. Heavy and regenerable, so
    /// duplicating it is the ten-minute wait that makes worktrees unusable.
    Link,
}

impl Verb {
    fn as_str(self) -> &'static str {
        match self {
            Self::Carry => "carry",
            Self::Link => "link",
        }
    }
}

/// Populate a FRESHLY CUT worktree with the environment the project DECLARED —
/// `mustard.json#worktree.carry` (copied) and `.link` (pointed at the main
/// checkout).
///
/// `git worktree add` writes only VERSIONED files, so every git-ignored path the
/// project needs to run is absent: the operator is dropped into a directory that
/// cannot build. Which paths those are is not knowable from here — copying every
/// ignored path drags `node_modules`/`target` along, and picking a subset is a
/// guess — so the project states them once and this step obeys. A project that
/// declares nothing does exactly what it did before (no declaration, no work).
///
/// Same posture as [`init_submodules`]: the filesystem is FORGIVING. Every
/// failure degrades to a loud WARN and the creation continues — a non-zero exit
/// here kills the whole `EnterWorktree`, which would cost the operator the
/// isolation itself over a missing `.env`.
///
/// Returns the paths that did NOT travel (one line each, empty when everything
/// did) so the operator learns the gap BEFORE hitting it mid-work, and a test
/// can observe the decision.
fn carry_environment(
    main: &Path,
    worktree: &Path,
    config: &mustard_core::ProjectConfig,
) -> Vec<String> {
    let declared = config
        .worktree
        .carried()
        .into_iter()
        .map(|rel| (rel, Verb::Carry))
        .chain(config.worktree.linked().into_iter().map(|rel| (rel, Verb::Link)));

    let mut stranded: Vec<String> = Vec::new();
    for (rel, verb) in declared {
        if let Err(reason) = plant(main, worktree, &rel, verb) {
            stranded.push(format!("{rel} ({}): {reason}", verb.as_str()));
        }
    }
    if !stranded.is_empty() {
        eprintln!(
            "WorktreeCreate: WARN: the declared environment did NOT fully travel into {}:\n  {}\n\
             The worktree exists and is usable, but whatever needs those paths fails HERE and \
             not in the main checkout. Fix the declaration in mustard.json#worktree (`carry` \
             copies a file, `link` points a heavy directory at the main checkout) or provide \
             them by hand before working in this worktree.",
            worktree.display(),
            stranded.join("\n  "),
        );
    }
    stranded
}

/// Plant ONE declared path in the worktree. `Err(reason)` is what the operator
/// is told about a path that stayed behind — never a fatal error.
///
/// Nothing is ever OVERWRITTEN: a path already in the worktree (a tracked file
/// the declaration also names, a leftover) is left exactly as git wrote it and
/// reported, because clobbering versioned content to plant an ignored one is a
/// worse defect than the missing file.
fn plant(main: &Path, worktree: &Path, rel: &str, verb: Verb) -> Result<(), String> {
    if is_repo_machinery(rel) {
        return Err("refused — `.claude/` already resolves to the MAIN checkout from inside a \
                    worktree (that redirect is what keeps the harness's state single) and `.git` \
                    is git's own"
            .to_string());
    }
    let src = main.join(rel);
    let dst = worktree.join(rel);
    // `symlink_metadata` on both sides: it does not follow links, so a link
    // whose target is gone still reads as PRESENT — creating over it fails.
    if src.symlink_metadata().is_err() {
        return Err("declared, but absent from the main checkout — nothing to take".to_string());
    }
    if dst.symlink_metadata().is_ok() {
        return Err("already in the worktree — left untouched".to_string());
    }
    if verb == Verb::Link && !src.is_dir() {
        return Err("`link` points a DIRECTORY at the main checkout, and this is a file — \
                    declare it under `carry` instead (a linked file would leak edits back)"
            .to_string());
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    match verb {
        Verb::Carry => copy_tree(&src, &dst).map_err(|e| format!("copy failed: {e}")),
        Verb::Link => link_dir(&src, &dst).map_err(|e| format!("link failed: {e}")),
    }
}

/// Whether a declared path is the repository's own machinery rather than the
/// project's environment — `.claude/` (redirected harness state) or `.git`.
fn is_repo_machinery(rel: &str) -> bool {
    matches!(rel, ".claude" | ".git")
        || rel.starts_with(".claude/")
        || rel.starts_with(".git/")
}

/// Recursive copy of `src` onto `dst`, which must not exist yet.
///
/// Directory-ness is read from `symlink_metadata`, so a symlink INSIDE a carried
/// tree is copied as the file it points at rather than descended into — a link
/// cycle would otherwise recurse forever.
fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(src)?;
    if !meta.is_dir() {
        std::fs::copy(src, dst)?;
        return Ok(());
    }
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        copy_tree(&entry.path(), &dst.join(entry.file_name()))?;
    }
    Ok(())
}

/// Point `dst` at the directory `src`, WITHOUT requiring an elevated user.
///
/// `std::os::windows::fs::symlink_dir` asks for
/// `SYMBOLIC_LINK_FLAG_ALLOW_UNPRIVILEGED_CREATE`, which Windows honours only
/// when Developer Mode is on; otherwise it needs `SeCreateSymbolicLinkPrivilege`
/// or an administrator, and fails with `ERROR_PRIVILEGE_NOT_HELD` (1314). A
/// directory JUNCTION needs no privilege at all, so it is the fallback — and it
/// has to go through `mklink /J`, because std's own `junction_point` is still
/// unstable (rust-lang/rust#121709) and this crate builds on stable.
///
/// `mklink` is a `cmd.exe` BUILTIN, not a program, hence `cmd /S /C`. The
/// command line is assembled with `raw_arg` for the reason
/// [`crate::util::platform::build_shell_command`] documents: `std`'s own
/// quoting follows `CommandLineToArgvW` rules that `cmd.exe` does not, which
/// would corrupt a path carrying spaces. `"` cannot appear in a Windows path,
/// so the inner quotes cannot be escaped out of.
#[cfg(windows)]
fn link_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    let Err(symlink_err) = std::os::windows::fs::symlink_dir(src, dst) else {
        return Ok(());
    };
    let mut cmd = std::process::Command::new("cmd");
    cmd.raw_arg(format!(
        "/S /C \"mklink /J \"{}\" \"{}\"\"",
        dst.display(),
        src.display()
    ));
    match cmd.output() {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => Err(std::io::Error::other(format!(
            "symlink refused ({symlink_err}) and the junction fallback failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))),
        Err(e) => Err(std::io::Error::other(format!(
            "symlink refused ({symlink_err}) and `cmd /C mklink /J` could not run: {e}"
        ))),
    }
}

/// See the `#[cfg(windows)]` variant for why that one needs a fallback. Here a
/// symlink is unprivileged by construction.
#[cfg(not(windows))]
fn link_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(src, dst)
}

/// Run `work-unit-open` from `opts.root`, print the single-line JSON report,
/// and exit 1 when `ok:false` (every failure here is a user/config/state
/// error the caller must handle; the network never produces one).
pub fn run(opts: WorkUnitOpenOpts) {
    let result = open_at(&opts);
    let ok = result.get("ok").and_then(Value::as_bool).unwrap_or(false);
    println!("{}", serde_json::to_string(&result).unwrap_or_else(|_| "{}".into()));
    if !ok {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::tempdir;

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git").args(args).current_dir(dir).output().expect("spawn git");
        assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    }

    fn opts(main: &Path) -> WorkUnitOpenOpts {
        WorkUnitOpenOpts {
            root: main.to_path_buf(),
            branch: None,
            spec: None,
            intent: None,
            base: None,
        }
    }

    /// Bare origin + main checkout on `dev` (flow `{*: dev, dev: main}`,
    /// `.claude/` gitignored). `origin/dev` is pushed one commit AHEAD of the
    /// local `dev` so a cut from `origin/dev` is distinguishable from a stale
    /// local cut.
    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().expect("tempdir");
        let bare = dir.path().join("origin.git");
        let main = dir.path().join("repo");
        std::fs::create_dir_all(&bare).expect("mkdir bare");
        std::fs::create_dir_all(&main).expect("mkdir main");
        git(&bare, &["init", "--bare", "."]);
        git(&main, &["init", "."]);
        git(&main, &["config", "user.email", "t@t"]);
        git(&main, &["config", "user.name", "t"]);
        git(&main, &["checkout", "-b", "dev"]);
        std::fs::write(main.join("mustard.json"), r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#)
            .expect("cfg");
        std::fs::write(main.join(".gitignore"), ".claude/\n").expect("ignore");
        std::fs::write(main.join("a.txt"), "a").expect("seed");
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-m", "seed"]);
        git(&main, &["remote", "add", "origin", bare.to_string_lossy().as_ref()]);
        git(&main, &["push", "-u", "origin", "dev"]);
        // origin/dev advances one commit past the local dev.
        std::fs::write(main.join("a.txt"), "ahead").expect("ahead");
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-m", "ahead"]);
        git(&main, &["push", "origin", "dev"]);
        git(&main, &["reset", "--hard", "HEAD~1"]);
        (dir, main)
    }

    #[test]
    fn strict_base_error_names_flow() {
        let (_dir, main) = fixture();
        let v = open_at(&WorkUnitOpenOpts { spec: Some("x".into()), base: Some("hml".into()), ..opts(&main) });
        assert_eq!(v["ok"], json!(false), "{v}");
        assert_eq!(v["reason"], json!("unknown-base"));
        let err = v["error"].as_str().unwrap_or_default();
        assert!(err.contains("hml") && err.contains("git.flow"), "{err}");
        assert!(!main.join(".claude").join("worktrees").exists(), "nothing created");
    }

    #[test]
    fn creates_worktree_from_origin_base() {
        let (_dir, main) = fixture();
        let head_before = git_out(&main, &["rev-parse", "HEAD"]).expect("head");
        let v = open_at(&WorkUnitOpenOpts { spec: Some("my-unit".into()), ..opts(&main) });
        assert_eq!(v["ok"], json!(true), "{v}");
        assert_eq!(v["branch"], json!("dev_my-unit"));
        assert_eq!(v["base"], json!("dev"));
        assert_eq!(v["created"], json!(true));
        let path = v["path"].as_str().expect("path");
        assert!(path.ends_with(".claude/worktrees/dev_my-unit"), "{path}");
        // Cut from origin/dev (the AHEAD commit), not the stale local dev.
        let wt_head = git_out(Path::new(path), &["rev-parse", "HEAD"]).expect("wt head");
        let origin = git_out(&main, &["rev-parse", "origin/dev"]).expect("origin");
        assert_eq!(wt_head, origin, "worktree cut from a fresh origin/dev");
        // The main checkout was not moved.
        assert_eq!(git_out(&main, &["rev-parse", "HEAD"]).expect("head"), head_before);
        assert_eq!(
            git_out(&main, &["rev-parse", "--abbrev-ref", "HEAD"]).expect("branch"),
            "dev",
            "main checkout stays on its branch"
        );
    }

    #[test]
    fn idempotent_rerun_returns_existing() {
        let (_dir, main) = fixture();
        let first = open_at(&WorkUnitOpenOpts { spec: Some("twice".into()), ..opts(&main) });
        assert_eq!(first["created"], json!(true), "{first}");
        let second = open_at(&WorkUnitOpenOpts { spec: Some("twice".into()), ..opts(&main) });
        assert_eq!(second["ok"], json!(true), "{second}");
        assert_eq!(second["created"], json!(false));
        assert_eq!(second["path"], first["path"], "same registered path");
        let porcelain = git_out(&main, &["worktree", "list", "--porcelain"]).expect("list");
        let count = parse_worktrees(&porcelain).iter().filter(|e| e.branch == "dev_twice").count();
        assert_eq!(count, 1, "exactly one registration");
    }

    #[test]
    fn existing_branch_is_attached_not_recreated() {
        let (_dir, main) = fixture();
        // A pre-existing branch at the (rewound) local dev — distinguishable
        // from origin/dev, which is one commit ahead.
        git(&main, &["branch", "dev_pre"]);
        let pre_sha = git_out(&main, &["rev-parse", "dev_pre"]).expect("sha");
        let v = open_at(&WorkUnitOpenOpts { branch: Some("dev_pre".into()), ..opts(&main) });
        assert_eq!(v["ok"], json!(true), "{v}");
        assert_eq!(v["created"], json!(true));
        let path = v["path"].as_str().expect("path");
        let wt_head = git_out(Path::new(path), &["rev-parse", "HEAD"]).expect("wt head");
        assert_eq!(wt_head, pre_sha, "existing branch reused, not re-cut from origin");
    }

    /// The EXECUTE isolation step DEGRADES when the unit's branch is already
    /// checked out in the main checkout — which is now the DEFAULT arrangement,
    /// because `spec-draft` cuts that branch at approval and writes the whole
    /// unit inside it. `git worktree add` refuses such a branch with exit 128,
    /// so both faces (the manual command and the `WorktreeCreate` hook) must
    /// report the checkout that already holds it instead of failing.
    #[test]
    fn a_branch_already_checked_out_in_place_is_reported_not_re_cut() {
        let (_dir, main) = fixture();
        git(&main, &["checkout", "-b", "dev_inplace"]);
        let root = main_checkout_root(&main).expect("main checkout").to_string_lossy().replace('\\', "/");

        // The manual face: `ok:true`, `inPlace:true`, nothing created.
        let v = open_at(&WorkUnitOpenOpts { branch: Some("dev_inplace".into()), ..opts(&main) });
        assert_eq!(v["ok"], json!(true), "the step degrades, it never fails: {v}");
        assert_eq!(v["inPlace"], json!(true), "{v}");
        assert_eq!(v["created"], json!(false), "nothing was cut");
        assert_eq!(v["branch"], json!("dev_inplace"));
        assert_eq!(v["path"], json!(root), "the checkout that already holds the branch");
        assert!(
            !main.join(".claude").join("worktrees").join("dev_inplace").exists(),
            "no worktree is added over a branch another tree holds",
        );

        // The hook face: the same answer, as the path EnterWorktree enters. A
        // non-zero exit here would abort the isolation altogether.
        let got = hook_create("dev_inplace", &main).expect("degrades instead of aborting");
        assert_eq!(got.replace('\\', "/"), root);

        // And the parallel-work case still cuts: a branch NOT already out gets
        // its own worktree, which is what keeps several units in flight.
        let other = open_at(&WorkUnitOpenOpts { spec: Some("parallel".into()), ..opts(&main) });
        assert_eq!(other["created"], json!(true), "{other}");
        assert!(other.get("inPlace").is_none(), "a fresh cut is not in place: {other}");
    }

    #[test]
    fn offline_falls_back_to_local_base() {
        // No remote at all: fetch degrades, the cut comes from the local base.
        let dir = tempdir().expect("tempdir");
        let main = dir.path().join("repo");
        std::fs::create_dir_all(&main).expect("mkdir");
        git(&main, &["init", "."]);
        git(&main, &["config", "user.email", "t@t"]);
        git(&main, &["config", "user.name", "t"]);
        git(&main, &["checkout", "-b", "dev"]);
        std::fs::write(main.join("mustard.json"), r#"{"git":{"flow":{"*":"dev"}}}"#).expect("cfg");
        std::fs::write(main.join(".gitignore"), ".claude/\n").expect("ignore");
        std::fs::write(main.join("a.txt"), "a").expect("seed");
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-m", "seed"]);
        let v = open_at(&WorkUnitOpenOpts { spec: Some("solo".into()), ..opts(&main) });
        assert_eq!(v["ok"], json!(true), "{v}");
        assert_eq!(v["fetched"], json!(false), "offline never blocks");
        let path = v["path"].as_str().expect("path");
        let wt_head = git_out(Path::new(path), &["rev-parse", "HEAD"]).expect("wt head");
        let dev = git_out(&main, &["rev-parse", "dev"]).expect("dev");
        assert_eq!(wt_head, dev, "cut from the local base ref");
    }

    #[test]
    fn hook_create_unit_name_cuts_from_fresh_origin_base() {
        // The event hands over a NAME, never a path — `worktree_path` is the
        // Remove twin's field. Placing the worktree is this engine's job.
        let (_dir, main) = fixture();
        let got = hook_create("dev_hooked", &main).expect("creates");
        assert!(got.ends_with(".claude/worktrees/dev_hooked"), "harness layout, from a name: {got}");
        let wt_head = git_out(Path::new(&got), &["rev-parse", "HEAD"]).expect("wt head");
        let origin = git_out(&main, &["rev-parse", "origin/dev"]).expect("origin");
        assert_eq!(wt_head, origin, "unit name cut from fresh origin/dev, not stale local");
        // Idempotent: second call returns the registered path, creates nothing.
        let again = hook_create("dev_hooked", &main).expect("idempotent");
        assert_eq!(again.replace('\\', "/"), got.replace('\\', "/"));
    }

    #[test]
    fn unit_name_is_decided_by_the_declared_bases_not_by_a_prefix_shape() {
        // The criterion, stated on its own: `{base}_` and nothing else. The
        // slug shapes the platform really emits (`WorktreeCreate#name`:
        // user-given, `pr-<n>`, or auto-generated) are all NON-units — there is
        // no `agent-` prefix anywhere in the documented contract, and this
        // repository's own harness worktree is `recursing-benz-063389`.
        let bases = vec!["dev".to_string(), "main".to_string()];
        assert!(is_unit_worktree_name("dev_my-spec", &bases));
        assert!(is_unit_worktree_name("main_hotfix", &bases));
        assert!(!is_unit_worktree_name("recursing-benz-063389", &bases));
        assert!(!is_unit_worktree_name("bright-running-fox", &bases));
        assert!(!is_unit_worktree_name("feature-auth", &bases));
        assert!(!is_unit_worktree_name("pr-1234", &bases));
        assert!(!is_unit_worktree_name("agent-w1", &bases), "no special shape survives");
        assert!(!is_unit_worktree_name("hml_x", &bases), "an UNDECLARED prefix is not a base");
        // Longest declared prefix wins, exactly like the branch gate.
        let nested = vec!["dev".to_string(), "dev_rc".to_string()];
        assert_eq!(unit_base_of_name("dev_rc_thing", &nested).as_deref(), Some("dev_rc"));
    }

    #[test]
    fn hook_create_non_unit_name_still_cuts_its_own_branch() {
        // A harness slug (no declared `{base}_`) must never break — background
        // isolation. `recursing-benz-063389` is the real shape, taken from this
        // repository's own `.claude/worktrees/`.
        let (_dir, main) = fixture();
        let got = hook_create("recursing-benz-063389", &main).expect("creates");
        assert!(Path::new(&got).is_dir(), "worktree materialized");
        assert_eq!(
            git_out(Path::new(&got), &["rev-parse", "--abbrev-ref", "HEAD"]).expect("branch"),
            "recursing-benz-063389",
            "own branch, native-style"
        );
    }

    #[test]
    fn agent_worktree_cuts_from_unit_head() {
        // Step 1 of the cascade: a checkout cut from INSIDE a work unit must
        // see that unit's commits. Cutting from the integration base would hand
        // the agent code that predates the unit — silently.
        let (_dir, main) = fixture();
        git(&main, &["checkout", "-b", "dev_unit"]);
        std::fs::write(main.join("unit.txt"), "unit work").expect("unit file");
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-m", "unit work"]);
        let unit_head = git_out(&main, &["rev-parse", "HEAD"]).expect("unit head");

        let got = hook_create("agent-w1", &main).expect("creates");
        let wt_head = git_out(Path::new(&got), &["rev-parse", "HEAD"]).expect("wt head");
        assert_eq!(wt_head, unit_head, "cut from the CURRENT unit's HEAD");
        assert!(Path::new(&got).join("unit.txt").is_file(), "the unit's commit travelled");
        let origin_dev = git_out(&main, &["rev-parse", "origin/dev"]).expect("origin/dev");
        assert_ne!(wt_head, origin_dev, "the unit beats the integration base");
    }

    /// Bare origin carrying BOTH `main` and `dev` at DIFFERENT tips, with
    /// `origin/HEAD` pointing at `main` (the remote's opinion of a default),
    /// and a main checkout parked on `dev` — which is an integration base, so
    /// no work unit is in play. `flow` is the `mustard.json` body, so the same
    /// shape serves the declared and the undeclared case.
    fn fixture_remote_head_on_main(flow: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().expect("tempdir");
        let bare = dir.path().join("origin.git");
        let main = dir.path().join("repo");
        std::fs::create_dir_all(&bare).expect("mkdir bare");
        std::fs::create_dir_all(&main).expect("mkdir main");
        git(&bare, &["init", "--bare", "."]);
        git(&main, &["init", "."]);
        git(&main, &["config", "user.email", "t@t"]);
        git(&main, &["config", "user.name", "t"]);
        git(&main, &["checkout", "-b", "main"]);
        std::fs::write(main.join("mustard.json"), flow).expect("cfg");
        std::fs::write(main.join(".gitignore"), ".claude/\n").expect("ignore");
        std::fs::write(main.join("a.txt"), "on main").expect("seed");
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-m", "main seed"]);
        git(&main, &["remote", "add", "origin", bare.to_string_lossy().as_ref()]);
        git(&main, &["push", "-u", "origin", "main"]);
        git(&main, &["checkout", "-b", "dev"]);
        std::fs::write(main.join("a.txt"), "on dev").expect("dev file");
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-m", "dev commit"]);
        git(&main, &["push", "-u", "origin", "dev"]);
        // Explicit, offline: `set-head <remote> <branch>` never contacts origin.
        git(&main, &["remote", "set-head", "origin", "main"]);
        (dir, main)
    }

    #[test]
    fn agent_worktree_falls_back_to_primary_base_not_remote_head() {
        // Step 2: `origin/HEAD` is the REMOTE's opinion of a default; the
        // project's DECLARED flow is the authority.
        let (_dir, main) =
            fixture_remote_head_on_main(r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#);
        assert_eq!(
            git_out(&main, &["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"])
                .expect("origin/HEAD"),
            "origin/main",
            "the fixture really does point origin/HEAD at main",
        );
        let got = hook_create("agent-declared", &main).expect("creates");
        let wt_head = git_out(Path::new(&got), &["rev-parse", "HEAD"]).expect("wt head");
        assert_eq!(
            wt_head,
            git_out(&main, &["rev-parse", "origin/dev"]).expect("origin/dev"),
            "cut from the DECLARED primary base",
        );
        assert_ne!(
            wt_head,
            git_out(&main, &["rev-parse", "origin/main"]).expect("origin/main"),
            "never from origin/HEAD when a flow is declared",
        );
    }

    #[test]
    fn agent_worktree_without_declared_flow_keeps_the_remote_head_base() {
        // Companion of the test above: with NO `git.flow`, step 2 is silent and
        // the historical `origin/HEAD` cut stands, byte for byte.
        let (_dir, main) = fixture_remote_head_on_main("{}");
        let got = hook_create("agent-undeclared", &main).expect("creates");
        let wt_head = git_out(Path::new(&got), &["rev-parse", "HEAD"]).expect("wt head");
        assert_eq!(
            wt_head,
            git_out(&main, &["rev-parse", "origin/main"]).expect("origin/main"),
            "undeclared flow → today's origin/HEAD behaviour",
        );
    }

    #[test]
    fn agent_worktree_refuses_dirty_tree() {
        // A fresh checkout carries only COMMITTED code: uncommitted work would
        // not travel and the agent would run against the older version. Driven
        // through the SLUG the harness really hands over — the previous
        // `agent-dirty` fixture was the only reason this refusal looked alive.
        let (_dir, main) = fixture();
        std::fs::write(main.join("a.txt"), "uncommitted").expect("dirty");
        let err = hook_create("recursing-benz-063389", &main).unwrap_err();
        assert!(err.contains("a.txt"), "names the offending path: {err}");
        assert!(
            !main.join(".claude").join("worktrees").join("recursing-benz-063389").exists(),
            "nothing created on refusal",
        );
    }

    #[test]
    fn every_non_unit_slug_shape_refuses_a_dirty_tree() {
        // The refusal is keyed on "not a work unit", so it holds for every name
        // the platform can generate — not for one prefix.
        for name in ["bright-running-fox", "feature-auth", "pr-1234", "agent-w1"] {
            let (_dir, main) = fixture();
            std::fs::write(main.join("a.txt"), "uncommitted").expect("dirty");
            let Err(err) = hook_create(name, &main) else {
                panic!("'{name}' must refuse a dirty tree");
            };
            assert!(err.contains("a.txt"), "{name} names the offending path: {err}");
        }
    }

    #[test]
    fn agent_worktree_ignores_dirt_confined_to_dot_claude() {
        // `.claude/` is redirected state, not code — the same carve-out the
        // harness's other gates make.
        let (_dir, main) = fixture();
        std::fs::create_dir_all(main.join(".claude")).expect("mkdir .claude");
        std::fs::write(main.join(".claude").join("settings.json"), "{}").expect("seed state");
        git(&main, &["add", "-f", ".claude/settings.json"]);
        git(&main, &["commit", "-m", "track harness state"]);
        std::fs::write(main.join(".claude").join("settings.json"), "{\"x\":1}").expect("edit");
        assert!(
            git_out(&main, &["status", "--porcelain"])
                .expect("status")
                .contains(".claude/settings.json"),
            "git really does report the change — the carve-out is what excludes it",
        );
        let got = hook_create("agent-stateonly", &main).expect("`.claude/` dirt never blocks");
        assert!(Path::new(&got).is_dir(), "worktree materialized");
    }

    #[test]
    fn dirty_tree_still_opens_a_unit_worktree() {
        // The precondition is scoped to AGENT worktrees: nothing outside a unit
        // worktree depends on its tree state, so its behaviour is untouched.
        let (_dir, main) = fixture();
        std::fs::write(main.join("a.txt"), "uncommitted").expect("dirty");
        let got = hook_create("dev_stillopens", &main).expect("unit worktrees keep behaviour");
        assert!(Path::new(&got).is_dir(), "worktree materialized");
    }

    #[test]
    fn hook_create_anchors_the_worktree_at_the_main_checkout() {
        // The hook may be invoked from ANY subdirectory: the harness reports the
        // caller's cwd, not the repo root. Deriving the layout from cwd would cut
        // the worktree outside the repository.
        let (_dir, main) = fixture();
        let deep = main.join("nested").join("deeper");
        std::fs::create_dir_all(&deep).expect("subdir");
        let got = hook_create("dev_fromdeep", &deep).expect("creates").replace('\\', "/");

        // Resolve the expectation THROUGH GIT, exactly as the engine does. A
        // tempdir path is not canonical — macOS hands out `/var/…`, a symlink
        // to `/private/var/…`, and a Windows runner hands out the short 8.3
        // `RUNNER~1` form — while git always answers with the resolved path.
        // Comparing a raw tempdir path against git's would fail on those two
        // platforms for a reason that has nothing to do with anchoring.
        let root = main_checkout_root(&deep).expect("main checkout");
        let expected = format!("{}/.claude/worktrees/dev_fromdeep", root.display()).replace('\\', "/");
        assert_eq!(got, expected, "anchored at the main checkout, not at cwd");
        // The claim itself, independent of any path formatting: never under the
        // caller's cwd.
        assert!(!got.contains("/nested/"), "never cut under the caller's cwd: {got}");
    }

    #[test]
    fn hook_create_undeclared_prefix_is_loud() {
        let (_dir, main) = fixture();
        let err = hook_create("hml_x", &main).unwrap_err();
        assert!(err.contains("hml") && err.contains("git.flow"), "didactic: {err}");
        assert!(!main.join(".claude/worktrees/hml_x").exists(), "nothing created on refusal");
    }

    #[test]
    fn hook_create_rejects_a_name_carrying_a_path_separator() {
        // `name` is a name. A separator would escape the worktrees dir.
        let (_dir, main) = fixture();
        let err = hook_create("../../etc/evil", &main).unwrap_err();
        assert!(err.contains("separator"), "refused: {err}");
    }

    /// [`fixture`] + a REAL git submodule at `vendor/lib`, committed on `dev`
    /// and pushed, so a cut from `origin/dev` carries the gitlink.
    ///
    /// Local-path submodules are refused by default since CVE-2022-39253, and
    /// the allowance is honoured ONLY from the command line or the global scope
    /// — a repo-local `protocol.file.allow` is ignored on purpose (a repo must
    /// not be able to authorise itself). So the fixture's own `submodule add`
    /// passes `-c`, while the ENGINE never does: relaxing that guard in
    /// production to make a test pass would trade a real protection for
    /// convenience.
    fn fixture_with_submodule() -> (tempfile::TempDir, PathBuf) {
        let (dir, main) = fixture();
        let up = dir.path().join("sublib");
        std::fs::create_dir_all(&up).expect("mkdir sublib");
        git(&up, &["init", "."]);
        git(&up, &["config", "user.email", "t@t"]);
        git(&up, &["config", "user.name", "t"]);
        std::fs::write(up.join("lib.txt"), "lib").expect("seed sub");
        git(&up, &["add", "-A"]);
        git(&up, &["commit", "-m", "sub seed"]);

        // The base fixture parks the local branch one commit BEHIND origin/dev;
        // realign so the submodule commit fast-forwards onto the pushed tip.
        git(&main, &["reset", "--hard", "origin/dev"]);
        let url = up.to_string_lossy().replace('\\', "/");
        git(&main, &["-c", "protocol.file.allow=always", "submodule", "add", &url, "vendor/lib"]);
        git(&main, &["commit", "-m", "add submodule"]);
        git(&main, &["push", "origin", "dev"]);
        (dir, main)
    }

    #[test]
    fn a_fresh_worktree_of_a_superproject_declares_its_submodules() {
        // The regression this guards, stated as the fact it rests on: `git
        // worktree add` registers the gitlink but leaves its directory EMPTY.
        // Nothing downstream distinguishes "not checked out" from "does not
        // exist" — the scan's walk finds zero files and mines a model missing
        // that whole subtree, with no error and no coverage entry.
        let (_dir, main) = fixture_with_submodule();
        let got = hook_create("dev_withsub", &main).expect("creates");
        let wt = Path::new(&got);
        assert!(wt.join(".gitmodules").is_file(), "the cut carries the declaration");
        // Hence the engine must ASK to populate — proven by the attempt below.
        assert!(init_submodules(wt).is_some(), "a declared submodule is always acted on");
    }

    #[test]
    fn a_worktree_without_submodules_is_left_alone() {
        // No `.gitmodules` → no git call at all. The trigger is the fact on
        // disk, never `mustard.json#git.submodules` (a declaration made at init
        // time that goes stale the moment a submodule is added).
        let (_dir, main) = fixture();
        let got = hook_create("dev_nosub", &main).expect("creates");
        assert_eq!(init_submodules(Path::new(&got)), None, "plain repo: nothing to do");
    }

    #[test]
    fn hook_create_survives_an_unfetchable_submodule() {
        // The network is the ONE forgiving step: a submodule that cannot be
        // fetched must not abort the creation — a non-zero exit here kills the
        // whole EnterWorktree. Loud WARN, usable worktree.
        let (_dir, main) = fixture_with_submodule();
        std::fs::write(
            main.join(".gitmodules"),
            "[submodule \"vendor/lib\"]\n\tpath = vendor/lib\n\turl = ../nope-does-not-exist\n",
        )
        .expect("break url");
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-m", "break submodule url"]);
        git(&main, &["push", "origin", "dev"]);
        let got = hook_create("dev_broken", &main).expect("worktree still created");
        assert!(Path::new(&got).is_dir(), "creation survives a failing submodule");
        assert!(
            matches!(init_submodules(Path::new(&got)), Some(Err(_))),
            "the failure is reported, never swallowed and never fatal"
        );
    }

    /// [`fixture`] whose `.gitignore` really ignores the environment paths, so
    /// the premise the whole block rests on is the fixture's own state: these
    /// are git-IGNORED, hence absent from any fresh cut. `declaration` is the
    /// `mustard.json` body (loaded from the MAIN checkout, never from the
    /// worktree).
    fn fixture_with_environment(declaration: &str) -> (tempfile::TempDir, PathBuf) {
        let (dir, main) = fixture();
        // The base fixture parks the local branch one commit BEHIND origin/dev;
        // realign so the ignore rule fast-forwards onto the pushed tip and the
        // cut carries it.
        git(&main, &["reset", "--hard", "origin/dev"]);
        std::fs::write(main.join(".gitignore"), ".claude/\n.env\nnode_modules/\n").expect("ignore");
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-m", "ignore the environment"]);
        git(&main, &["push", "origin", "dev"]);
        std::fs::write(main.join("mustard.json"), declaration).expect("cfg");
        std::fs::write(main.join(".env"), "TOKEN=main").expect(".env");
        std::fs::create_dir_all(main.join("node_modules").join("pkg")).expect("node_modules");
        std::fs::write(main.join("node_modules/pkg/index.js"), "one").expect("pkg");
        (dir, main)
    }

    /// The arrangement both halves below rest on, cut once: a project that
    /// declares a small git-ignored file under `carry` and a heavy regenerable
    /// directory under `link`, and a worktree freshly cut from it. Returns the
    /// fixture's guard (dropping it deletes the tree), the main checkout and
    /// the fresh worktree.
    ///
    /// The fact the block exists for: `git worktree add` writes only VERSIONED
    /// files, so a fresh cut has neither `.env` nor `node_modules` — the
    /// operator lands in a directory that cannot run.
    fn fresh_worktree_with_declared_environment(
        branch: &str,
    ) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let (dir, main) = fixture_with_environment(
            r#"{"git":{"flow":{"*":"dev","dev":"main"}},
                "worktree":{"carry":[".env"],"link":["node_modules"]}}"#,
        );
        let got = hook_create(branch, &main).expect("creates");
        let wt = PathBuf::from(got);
        (dir, main, wt)
    }

    #[test]
    fn a_declared_carry_path_lands_in_a_fresh_worktree() {
        let (_dir, main, wt) = fresh_worktree_with_declared_environment("dev_withcarry");

        // `carry` is a REAL copy: an edit inside the worktree must NOT reach
        // the main checkout (which is exactly what a link would do).
        assert_eq!(std::fs::read_to_string(wt.join(".env")).expect("carried"), "TOKEN=main");
        std::fs::write(wt.join(".env"), "TOKEN=worktree").expect("edit the copy");
        assert_eq!(
            std::fs::read_to_string(main.join(".env")).expect("main .env"),
            "TOKEN=main",
            "a carried file is a copy — edits never leak back into the main checkout",
        );
    }

    #[test]
    fn a_declared_link_path_reaches_the_main_checkout() {
        let (_dir, main, wt) = fresh_worktree_with_declared_environment("dev_withlink");

        // `link` is the main checkout's directory, seen from here: what changes
        // there is visible here, because nothing was duplicated.
        assert_eq!(
            std::fs::read_to_string(wt.join("node_modules/pkg/index.js")).expect("through link"),
            "one",
        );
        std::fs::write(main.join("node_modules/pkg/index.js"), "two").expect("regenerate");
        assert_eq!(
            std::fs::read_to_string(wt.join("node_modules/pkg/index.js")).expect("through link"),
            "two",
            "a linked directory IS the main checkout's — never a copy of it",
        );
    }

    #[test]
    fn an_undeclared_project_receives_exactly_what_it_did_before() {
        // Both lists default to empty, so a project that declares nothing gets
        // no population step at all — today's behaviour, byte for byte.
        let (_dir, main) = fixture_with_environment(r#"{"git":{"flow":{"*":"dev"}}}"#);
        let config = mustard_core::ProjectConfig::load(&main);
        let got = hook_create("dev_nodecl", &main).expect("creates");
        assert!(
            carry_environment(&main, Path::new(&got), &config).is_empty(),
            "nothing declared, nothing attempted and nothing reported",
        );
        assert!(!Path::new(&got).join(".env").exists(), "an undeclared path never travels");
    }

    #[test]
    fn what_did_not_travel_is_named_and_never_aborts_the_creation() {
        // A silent partial environment is the defect: the operator must learn
        // the gap HERE, not mid-work. And the filesystem is forgiving like the
        // network — a non-zero exit would kill the whole EnterWorktree.
        let (dir, main) = fixture_with_environment(
            r#"{"git":{"flow":{"*":"dev"}},
                "worktree":{"carry":[".env.production",".claude/settings.local.json"],
                            "link":[".env","node_modules"]}}"#,
        );
        let config = mustard_core::ProjectConfig::load(&main);
        let got = hook_create("dev_gaps", &main).expect("the environment never aborts creation");
        let wt = Path::new(&got);
        assert!(wt.is_dir(), "the worktree exists and is usable");
        assert!(wt.join("node_modules/pkg/index.js").is_file(), "the sound entry still travelled");

        // The report itself, observed on a destination in the state a FRESH cut
        // hands over (the worktree above has already been populated once).
        let fresh = dir.path().join("fresh-cut");
        std::fs::create_dir_all(&fresh).expect("fresh dst");
        let stranded = carry_environment(&main, &fresh, &config);
        let report = stranded.join("\n");
        assert_eq!(stranded.len(), 3, "one line per path that stayed behind: {report}");
        assert!(
            report.contains(".env.production") && report.contains("absent from the main checkout"),
            "a declared path the main checkout does not have is named: {report}",
        );
        assert!(
            report.contains(".claude/settings.local.json") && report.contains("refused"),
            "`.claude/` keeps resolving to the main checkout — never populated: {report}",
        );
        assert!(
            report.contains(".env (link)") && report.contains("carry"),
            "a FILE declared under `link` is refused with the verb that fits it: {report}",
        );
        assert!(
            !Path::new(&got).join(".claude").join("settings.local.json").exists(),
            "the refusal is real — nothing was written under .claude/",
        );
    }

    #[test]
    fn missing_slug_and_bad_prefix_are_loud() {
        let (_dir, main) = fixture();
        let v = open_at(&opts(&main));
        assert_eq!(v["reason"], json!("missing-slug"), "{v}");
        let v = open_at(&WorkUnitOpenOpts { branch: Some("feature_x".into()), ..opts(&main) });
        assert_eq!(v["reason"], json!("no-base-prefix"), "{v}");
        let v = open_at(&WorkUnitOpenOpts {
            branch: Some("dev_pre".into()),
            base: Some("main".into()),
            ..opts(&main)
        });
        assert_eq!(v["reason"], json!("base-mismatch"), "{v}");
    }
}
