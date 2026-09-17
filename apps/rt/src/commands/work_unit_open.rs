//! `mustard-rt run work-unit-open` — the ENTRY RITUAL of a work unit: create
//! (idempotently) the unit's isolated worktree so the orchestrator can switch
//! the session into it (`EnterWorktree path=<returned path>`) instead of
//! mutating the main checkout with an in-place `checkout -b`.
//!
//! Counterpart of [`crate::commands::git_settle`] (the exit ritual): open cuts
//! `.claude/worktrees/{kind}/{slug}` from a fresh `origin/{base}`; settle
//! verifies the merge and prunes the same worktree. Cleanup of these worktrees
//! is git-settle's job EXCLUSIVELY — nothing else may touch a worktree that
//! reads as a work unit.
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
//! Nothing ELSE is planted either: a cut receives what git tracks, and nothing
//! the harness invented. Carrying or
//! LINKING the project's git-ignored environment (`.env`, `node_modules`) was
//! tried and withdrawn: a directory junction inside the worktree is DESCENDED by
//! `git worktree remove`, which deleted the main checkout's own directory (with
//! and without `--force`), so the removal of a worktree destroyed the tree it
//! pointed at. A worktree therefore lacks whatever git ignores, by design; the
//! second unit that would need one is REFUSED instead
//! ([`crate::commands::event::work_branch::cut_pending_work_branch`]).
//!
//! Error posture: config/user/state errors are LOUD (`ok:false` + exit 1) —
//! an unknown `--base` here is the same disease `resolve_base` now rejects at
//! emit time. Only the network is forgiving: a failed `git fetch origin` never
//! blocks, the cut degrades to the local base ref (`fetched:false` reports it).

use std::path::{Path, PathBuf};

use mustard_core::platform::git;

use serde_json::{json, Value};

use crate::commands::git_settle::{git_ok, git_out, main_checkout_root, parse_worktrees};
use crate::shared::work_kind::{BaseFlow, UnitBase, WorkKind};

/// Options for `mustard-rt run work-unit-open`.
pub struct WorkUnitOpenOpts {
    /// Any directory inside the repo (worktrees welcome — the command resolves
    /// the main checkout itself). Defaults to the current dir.
    pub root: PathBuf,
    /// Full work-branch name override (e.g. `feature/my-spec`). Its prefix
    /// names a work kind, or — for a unit still in the `{base}_{slug}` shape —
    /// a branch `origin` really has.
    pub branch: Option<String>,
    /// Spec slug — used verbatim as the branch slug (parity with emit-pipeline).
    pub spec: Option<String>,
    /// Free-form intent, slugified when `--spec` is absent (parity with
    /// emit-pipeline).
    pub intent: Option<String>,
    /// What the unit IS (`feature`/`fix`/`hotfix`) — names the branch and, via
    /// `git.flow`, its base. Omitted → the ordinary unit, never the emergency.
    pub work_kind: Option<String>,
    /// Integration base; STRICT — must name a declared base, and must not be
    /// the work base for a hotfix. Omitted → the base the kind implies.
    pub base: Option<String>,
}

/// Run `git` in `dir`, `Err(stderr)` on failure — for the calls whose failure
/// text the orchestrator must see (worktree add conflicts).
fn git_try(dir: &Path, args: &[&str]) -> Result<(), String> {
    git::run(dir, args).result().map(|_| ())
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

/// What the caller said the unit IS, or the ordinary unit when they said
/// nothing. `None` only for a value that names no kind — refused rather than
/// defaulted, so a mistyped `--type hotifx` never cuts into the ordinary queue.
fn resolve_work_kind(requested: Option<&str>) -> Option<WorkKind> {
    match requested.map(str::trim).filter(|s| !s.is_empty()) {
        Some(value) => WorkKind::parse(value),
        None => Some(WorkKind::suggested_default()),
    }
}

/// The open pass — the testable core of [`run`]. Never panics.
pub(crate) fn open_at(opts: &WorkUnitOpenOpts) -> Value {
    let Some(main) = main_checkout_root(&opts.root) else {
        return json!({ "ok": false, "reason": "not-a-git-repo" });
    };
    let config = mustard_core::ProjectConfig::load(&main);
    let flow = BaseFlow::of_at(&config.git, &main);

    // Resolve the target branch + its base — every mismatch is loud, never a
    // silent fallback (an explicit input is caller intent).
    let (target, base) = match opts.branch.as_deref().map(str::trim).filter(|b| !b.is_empty()) {
        Some(b) => {
            let requested = opts.base.as_deref().map(str::trim).filter(|s| !s.is_empty());
            // The crate's one reading of a work-branch name — minus the gate's
            // work-base fallback: a name that is nobody's unit is refused
            // (mirrors git-settle's `no-base-prefix`).
            match flow.base_of(b) {
                UnitBase::NotAUnit => {
                    return json!({ "ok": false, "reason": "no-base-prefix", "branch": b })
                }
                UnitBase::Known(prefix) => {
                    if let Some(req) = requested
                        && req != prefix {
                            return json!({
                                "ok": false,
                                "reason": "base-mismatch",
                                "branch": b,
                                "prefix": prefix,
                                "base": req,
                            });
                        }
                    (b.to_string(), prefix)
                }
                // The name IS a unit's, but nothing ever established which base
                // it came from — an emergency in a project with several
                // candidates, cut by a door that did not record it. `--base` is
                // the operator saying it; without one there is no honest answer,
                // and picking the outermost would cut the emergency somewhere
                // they did not choose.
                UnitBase::Ambiguous(candidates) => {
                    // The operator's `--base` is validated against the branches
                    // this repository REALLY has, never against the declared
                    // list. `candidates` is `preselected_bases()`, which falls
                    // back to the hardcoded `{main, master}` when no `git.flow`
                    // is written — the shape `mustard init` produces today — so
                    // filtering here refused a base the remote carries and
                    // answered with a verdict about a configuration file
                    // ("this project declares several bases") over a repository
                    // nobody asked about. Existence is the honest test, and it
                    // is the same one `--base`'s own help text promises.
                    let real = |req: &&str| {
                        ref_exists(&main, &format!("refs/heads/{req}"))
                            || ref_exists(&main, &format!("refs/remotes/origin/{req}"))
                    };
                    match requested.filter(real) {
                        Some(req) => (b.to_string(), req.to_string()),
                        None => {
                            let asked = requested.unwrap_or_default();
                            return json!({
                                "ok": false,
                                "reason": "ambiguous-base",
                                "branch": b,
                                "candidates": candidates,
                                "hint": if asked.is_empty() {
                                    format!(
                                        "nothing recorded which base '{b}' was cut from — pass \
                                         --base with a branch this repository has"
                                    )
                                } else {
                                    format!(
                                        "--base '{asked}' names no branch this repository has, \
                                         locally or on origin — check it with `git branch -a`"
                                    )
                                },
                            })
                        }
                    }
                }
            }
        }
        None => {
            let Some(kind) = resolve_work_kind(opts.work_kind.as_deref()) else {
                return json!({
                    "ok": false,
                    "reason": "unknown-type",
                    "type": opts.work_kind.clone(),
                    "hint": WorkKind::SUGGESTED.join(", "),
                });
            };
            let base = match super::event::work_branch::resolve_kind_base(
                &main,
                opts.base.as_deref(),
                &config,
            ) {
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
                kind,
                spec,
                intent,
                &crate::shared::context::session::session_id(),
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
        // Even with nothing to cut, the CALLER's base answer is still worth
        // writing down: git-settle's ambiguous-base hint sends the operator
        // to THIS command to record it, and an early return that skipped the
        // record made that hint a circle (measured 2026-08-19: ok:true, then
        // the same refusal again). A no-op unless the flow cannot re-derive.
        flow.record_cut_base(&target, &base);
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
        // Same record as above: in-place is the DEFAULT arrangement now, and
        // it was the one place the base answer silently evaporated.
        flow.record_cut_base(&target, &base);
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

    // The cut happened HERE, so this is where its base becomes a fact — and the
    // only door that can still write it down. A no-op unless the flow cannot
    // re-derive the answer (see `record_cut_base`).
    flow.record_cut_base(&target, &base);

    json!({
        "ok": true,
        "path": wt_str,
        "branch": target,
        "base": base,
        "created": true,
        "fetched": fetched,
    })
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
    use tempfile::tempdir;

    fn git(dir: &Path, args: &[&str]) {
        let out = git::run(dir, args);
        assert!(out.ok, "git {args:?} failed: {}", out.stderr);
    }

    fn opts(main: &Path) -> WorkUnitOpenOpts {
        WorkUnitOpenOpts {
            root: main.to_path_buf(),
            branch: None,
            spec: None,
            intent: None,
            work_kind: None,
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

    /// A mistyped base is still refused loudly — but the refusal now names what
    /// the REMOTE really has instead of pointing at a configuration file. Same
    /// loudness, opposite source: that swap is the whole unit.
    #[test]
    fn strict_base_error_names_the_real_branches() {
        let (_dir, main) = fixture();
        let v = open_at(&WorkUnitOpenOpts { spec: Some("x".into()), base: Some("hml".into()), ..opts(&main) });
        assert_eq!(v["ok"], json!(false), "{v}");
        assert_eq!(v["reason"], json!("unknown-base"));
        let err = v["error"].as_str().unwrap_or_default();
        assert!(err.contains("hml"), "names the rejected base: {err}");
        assert!(err.contains("dev"), "and lists a branch that really exists: {err}");
        assert!(
            !err.contains("git.flow"),
            "and no longer sends the operator to a configuration file: {err}",
        );
        assert!(!main.join(".claude").join("worktrees").exists(), "nothing created");
    }

    #[test]
    fn creates_worktree_from_origin_base() {
        let (_dir, main) = fixture();
        let head_before = git_out(&main, &["rev-parse", "HEAD"]).expect("head");
        let v = open_at(&WorkUnitOpenOpts { spec: Some("my-unit".into()), ..opts(&main) });
        assert_eq!(v["ok"], json!(true), "{v}");
        // Named by what the unit IS; the base follows from that through the
        // declared flow, and is reported separately.
        assert_eq!(v["branch"], json!("feature/my-unit"));
        assert_eq!(v["base"], json!("dev"));
        assert_eq!(v["created"], json!(true));
        let path = v["path"].as_str().expect("path");
        assert!(path.ends_with(".claude/worktrees/feature/my-unit"), "{path}");
        // Cut from origin/dev (the AHEAD commit), not the stale local dev.
        let wt_head = git_out(Path::new(path), &["rev-parse", "HEAD"]).expect("wt head");
        let origin = git_out(&main, &["rev-parse", "origin/dev"]).expect("origin");
        assert_eq!(wt_head, origin, "worktree cut from a fresh origin/dev");
        // The main checkout was not moved.
        assert_eq!(git_out(&main, &["rev-parse", "HEAD"]).expect("head"), head_before);
        assert_eq!(
            mustard_core::current_branch(&main).expect("branch"),
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
        let count =
            parse_worktrees(&porcelain).iter().filter(|e| e.branch == "feature/twice").count();
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
    /// so the command must report the checkout that already holds it instead
    /// of failing.
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

        // The base the caller answered is RECORDED even though nothing was
        // cut — settle's ambiguous-base hint sends the operator here to write
        // exactly this down, and the early return used to skip it (measured
        // 2026-08-19: ok:true, then the same refusal again). The fixture's
        // flow declares two bases, so the answer is not derivable and the
        // record is due.
        std::fs::write(
            main.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#,
        )
        .expect("declare an ambiguous two-tier flow");
        // A `{kind}/{slug}` branch — the shape whose base is NOT derivable
        // (the flow above declares two bases), so the record is due.
        git(&main, &["checkout", "-b", "fix/inplace"]);
        let again = open_at(&WorkUnitOpenOpts {
            branch: Some("fix/inplace".into()),
            base: Some("dev".into()),
            ..opts(&main)
        });
        assert_eq!(again["inPlace"], json!(true), "{again}");
        let record = main.join(".claude").join("spec").join("inplace").join(".cut-base");
        let body = std::fs::read_to_string(&record).expect("the in-place return records the base");
        assert_eq!(body.trim(), "dev", "the caller's answer, written where settle reads");
        git(&main, &["checkout", "dev_inplace"]);

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
