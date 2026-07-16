//! `mustard-rt run work-unit-open` — the ENTRY RITUAL of a work unit: create
//! (idempotently) the unit's isolated worktree so the orchestrator can switch
//! the session into it (`EnterWorktree path=<returned path>`) instead of
//! mutating the main checkout with an in-place `checkout -b`.
//!
//! Counterpart of [`crate::commands::git_settle`] (the exit ritual): open cuts
//! `.claude/worktrees/{base}_{slug}` from a fresh `origin/{base}`; settle
//! verifies the merge and prunes the same worktree. Cleanup of these worktrees
//! is git-settle's job EXCLUSIVELY — `worktree-gc` collects `agent-*` dirs
//! only and never touches work units.
//!
//! Branch naming reuses [`super::event::work_branch`] so the worktree branch
//! is byte-identical to the `pending-work-branch` marker `emit-pipeline`
//! wrote; inside the worktree the gate then finds the branch already checked
//! out and stays silent.
//!
//! Error posture: config/user/state errors are LOUD (`ok:false` + exit 1) —
//! an unknown `--base` here is the same disease `resolve_base` now rejects at
//! emit time. Only the network is forgiving: a failed `git fetch origin` never
//! blocks, the cut degrades to the local base ref (`fetched:false` reports it).

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::commands::git_settle::{git_ok, git_out, main_checkout_root, parse_worktrees};

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
            let Some(prefix) = bases
                .iter()
                .filter(|c| b.starts_with(&format!("{c}_")))
                .max_by_key(|c| c.len())
                .cloned()
            else {
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
                    "hint": "passe --spec, --intent ou --branch",
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
