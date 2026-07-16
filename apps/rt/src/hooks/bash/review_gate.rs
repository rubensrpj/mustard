//! `review_gate` — validate before `git commit`.
//!
//! Computes its verdict with its **own** mode variable
//! `MUSTARD_COMMIT_GATE_MODE` (default `warn`), independent of the
//! module-level enforcement mode the dispatcher applies — the dispatcher
//! repasses the verdict without downgrade. 1:1 port of `review-gate.js`.

use mustard_core::ClaudePaths;
use mustard_core::platform::config::Mode;
use mustard_core::platform::process::rtk_command;
use mustard_core::domain::model::contract::{Ctx, Verdict};
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::time::now_iso8601;
use serde_json::json;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::shared::context::current_spec;
use crate::util::format_gate_message;

use super::lex::{has_word_pair, is_cmd_separator, mask_quoted_operators, strip_leading_rtk, truncate};

/// Build timeout for the strict-mode build check (`BUILD_TIMEOUT_MS` in
/// `review-gate.js`): 5 minutes.
const BUILD_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// `\bgit\s+commit\b` — `git commit` anywhere in the command (tolerates an
/// `rtk` prefix). Mirrors `isGitCommit` in `review-gate.js`.
fn is_git_commit(cmd: &str) -> bool {
    let lower = cmd.to_ascii_lowercase();
    has_word_pair(&lower, "git", "commit")
}

/// `true` when a `git commit` stages its changes **as part of the commit** —
/// `-a`/`-am`/`--all` (all tracked) or an explicit `-- <pathspec>` separator.
///
/// For these forms the index is legitimately empty at `PreToolUse` time (the
/// staging happens inside the commit), so the "No staged changes detected"
/// advisory is a false positive — the commit will, in fact, record changes.
/// A plain `git commit` (relying on a pre-staged index) is NOT inline-staging,
/// so it still warns. Detection is conservative: a short-flag cluster is only
/// matched when it is all letters and contains `a` (so `-am` matches, `-m` and
/// the long `--amend` do not), avoiding a false suppression on `git commit -m`.
fn commit_stages_inline(cmd: &str) -> bool {
    cmd.split_whitespace().any(|tok| {
        tok == "--all"
            || tok == "--" // explicit pathspec separator: `git commit -- <paths>`
            || (tok.len() >= 2
                && tok.starts_with('-')
                && !tok.starts_with("--")
                && tok[1..].bytes().all(|b| b.is_ascii_alphabetic())
                && tok[1..].contains('a'))
    })
}

/// `true` when `seg` — one command from a compound line, tolerating a leading
/// `rtk` — is `git <sub>` (e.g. `git add`, `git commit`). Anchored at the
/// segment start so a `git add` mentioned *inside a commit message* does not
/// count as a staging command.
fn seg_is_git_subcmd(seg: &str, sub: &str) -> bool {
    let stripped = strip_leading_rtk(seg.trim());
    let lower = stripped.trim_start().to_ascii_lowercase();
    let Some(rest) = lower.strip_prefix("git") else {
        return false;
    };
    let Some(rest) = rest.strip_prefix(|c: char| c.is_whitespace()) else {
        return false;
    };
    match rest.trim_start().strip_prefix(sub) {
        Some(after) => after.is_empty() || after.starts_with(char::is_whitespace),
        None => false,
    }
}

/// `true` when the same compound command runs a `git add` **before** the
/// `git commit` — `git add -A && git commit -m x`, `git add . ; git commit …`.
///
/// At `PreToolUse` the chained `git add` has not run yet, so the index the gate
/// inspects is still empty even though the commit *will* record the files the
/// earlier `git add` stages. Recognising the chained add keeps the "No staged
/// changes detected" advisory off the most common commit idiom, while a plain
/// `git commit` against an empty index still warns. Segment boundaries are read
/// on the quote-masked view so a `;`/`&` inside a commit message is not
/// mistaken for a command separator.
fn chained_git_add_precedes_commit(cmd: &str) -> bool {
    let masked = mask_quoted_operators(cmd);
    let mut saw_git_add = false;
    for seg in masked.split(is_cmd_separator) {
        let seg = seg.trim();
        if seg.is_empty() {
            continue;
        }
        if seg_is_git_subcmd(seg, "commit") && saw_git_add {
            return true;
        }
        if seg_is_git_subcmd(seg, "add") {
            saw_git_add = true;
        }
    }
    false
}

/// The `MUSTARD_COMMIT_GATE_MODE` mode for the commit gate.
///
/// Default is `warn` (retro-compat with `getCommitGateMode` in
/// `review-gate.js` — *not* the crate-wide strict default). An unrecognised
/// value also falls back to `warn`.
pub(super) fn commit_gate_mode() -> Mode {
    std::env::var("MUSTARD_COMMIT_GATE_MODE")
        .ok()
        .and_then(|raw| Mode::parse(&raw))
        .unwrap_or(Mode::Warn)
}

/// `true` when the hook profile is `strict` — mirrors `isStrictMode()` in
/// `_lib/hook-env.js`. Used by `review-gate.js` to decide `deny` vs `allow`
/// in warn-mode.
fn is_strict_profile() -> bool {
    std::env::var("MUSTARD_HOOK_PROFILE")
        .is_ok_and(|v| v.trim().eq_ignore_ascii_case("strict"))
}

const SENSITIVE_EXT: &[&str] = &[
    ".env", ".pem", ".key", ".secret", ".p12", ".pfx", ".cer", ".crt",
];

/// `true` if a staged path matches a sensitive-file pattern. Mirrors the
/// `sensitiveFiles` filter in `review-gate.js`.
fn is_sensitive_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    if SENSITIVE_EXT.iter().any(|ext| normalized.ends_with(ext)) {
        return true;
    }
    // /credentials/i and /\.env\./i — substring matches.
    normalized.contains("credentials") || normalized.contains(".env.")
}

/// `true` if a staged path lives under a generated/build output directory.
/// Mirrors the `generated` filter in `review-gate.js`.
fn is_generated_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    ["dist/", "node_modules/", "obj/", "bin/"]
        .iter()
        .any(|prefix| normalized.starts_with(prefix))
}

/// Read `buildCommand` from the project-root `mustard.json` through the single
/// config owner. Fail-open to `None` when the file is absent or the key unset.
fn read_build_command(project_dir: &str) -> Option<String> {
    mustard_core::ProjectConfig::load(Path::new(project_dir)).build_command()
}

/// The outcome of a build run. `env_error` marks a fail-open condition
/// (`ENOENT` / timeout) — the JS port never blocks on those.
struct BuildOutcome {
    ok: bool,
    env_error: bool,
    output: String,
}

/// Run the staged build command under [`BUILD_TIMEOUT`].
///
/// `std::process::Command` has no native timeout, so the child is spawned and
/// waited on in a thread; if the wait does not finish inside the budget the
/// child is killed and the run is reported as an `env_error` (fail-open,
/// matching the JS `SIGTERM` branch). A spawn failure (`ENOENT`) is likewise
/// an `env_error`.
fn run_build(cmd: &str, project_dir: &str) -> BuildOutcome {
    // Shell out so the command string is interpreted the same way the JS
    // `execSync` does. `cmd /C` on Windows, `sh -c` elsewhere.
    let mut command = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", cmd]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", cmd]);
        c
    };
    command
        .current_dir(project_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match command.spawn() {
        Ok(child) => child,
        // Spawn failure (missing shell / ENOENT) → fail-open.
        Err(err) => {
            return BuildOutcome {
                ok: false,
                env_error: true,
                output: err.to_string(),
            };
        }
    };

    let (tx, rx) = std::sync::mpsc::channel();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    // The wait runs on a worker thread so the caller can apply a timeout.
    std::thread::spawn(move || {
        let status = child.wait();
        let _ = tx.send((status, child));
    });

    match rx.recv_timeout(BUILD_TIMEOUT) {
        Ok((Ok(status), _child)) => {
            let mut output = String::new();
            if let Some(mut out) = stdout {
                use std::io::Read;
                let _ = out.read_to_string(&mut output);
            }
            if let Some(mut err) = stderr {
                use std::io::Read;
                let _ = err.read_to_string(&mut output);
            }
            BuildOutcome {
                ok: status.success(),
                env_error: false,
                output: output.trim().to_string(),
            }
        }
        // Wait itself failed → fail-open.
        Ok((Err(err), _child)) => BuildOutcome {
            ok: false,
            env_error: true,
            output: err.to_string(),
        },
        // Timed out — kill the child and fail open (the JS `SIGTERM` branch).
        Err(_) => {
            if let Ok((_, mut child)) = rx.recv_timeout(Duration::from_millis(0)) {
                let _ = child.kill();
            }
            BuildOutcome {
                ok: false,
                env_error: true,
                output: format!("[timeout] {cmd}"),
            }
        }
    }
}

/// List staged file paths via `git diff --cached --name-only`.
///
/// Fail-open: `None` when git is unavailable or the command fails (the JS
/// `catch` branch — no staged-file warnings produced). Goes through
/// [`rtk_command`] so the subprocess follows Mustard's Golden Rule.
fn staged_files(project_dir: &str) -> Option<Vec<String>> {
    let output = rtk_command("git", &["diff", "--cached", "--name-only"])
        .current_dir(project_dir)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Some(
        text.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect(),
    )
}

/// List active pipeline names under `.claude/.pipeline-states/*.json`.
fn active_pipelines(project_dir: &str) -> Vec<String> {
    let Ok(paths) = ClaudePaths::for_project(Path::new(project_dir)) else {
        return Vec::new();
    };
    let dir = paths.pipeline_states_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    entries
        .filter_map(std::result::Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.strip_suffix(".json").map(str::to_string)
        })
        .collect()
}

/// Emit the `commit-gate.check` harness event. Best-effort — telemetry is
/// never load-bearing, so any failure is swallowed.
fn emit_commit_gate_event(
    project_dir: &str,
    session_id: Option<&str>,
    mode: Mode,
    warnings: usize,
    blocking_findings: &[&str],
    has_sensitive: bool,
    build_ok: Option<bool>,
) {
    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id.unwrap_or("unknown").to_string(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("review-gate".to_string()),
            actor_type: None,
        },
        event: "commit-gate.check".to_string(),
        payload: json!({
            "mode": mode.as_str(),
            "warnings": warnings,
            "blockingFindings": blocking_findings,
            "hasSensitive": has_sensitive,
            "buildOk": build_ok,
        }),
        spec: current_spec(project_dir),
    };
    // `commit-gate.check` is non-pipeline → per-spec NDJSON via W5 router.
    let _ = crate::shared::events::route::emit(project_dir, &event);
}

/// The `review-gate` gate: validate a `git commit` command.
///
/// `mode` is the commit-gate's **own** [`Mode`] (`MUSTARD_COMMIT_GATE_MODE`,
/// default `warn`), resolved by the caller — passing it in keeps the gate
/// testable without mutating process environment.
///
/// Returns `None` for every non-commit command and for `Mode::Off`.
/// Otherwise reproduces `review-gate.js` 1:1:
/// - strict mode + a blocking finding (staged secret / broken build) → `Deny`;
/// - any warnings → `Warn` (or `Deny` when the hook profile is `strict`);
/// - no warnings → `None` (pass).
// review_gate contains a single sequential logic block; splitting it would
// require threading many local variables through helper fns with no clarity gain.
#[allow(clippy::too_many_lines)]
pub(super) fn review_gate(cmd: &str, ctx: &Ctx, mode: Mode) -> Option<Verdict> {
    // Mode `off` — skip entirely.
    if mode == Mode::Off {
        return None;
    }
    if !is_git_commit(cmd) {
        return None;
    }

    let project_dir = ctx.project_dir.as_str();
    let mut warnings: Vec<String> = Vec::new();
    // Strict-blocking findings: `secrets` or `build`.
    let mut blocking: Vec<(&'static str, String)> = Vec::new();
    let mut has_sensitive = false;

    // Check 1-4: staged changes — sensitive / generated / large.
    match staged_files(project_dir) {
        // An empty index here is expected — not a missing-changes problem —
        // when the commit stages its own changes (`-a`/`-am`/`commit -- <paths>`)
        // or when a `git add` is chained *before* the commit in the same command
        // (`git add -A && git commit -m x`; that add has not run yet at
        // PreToolUse). Only a plain `git commit` relying on a pre-staged index
        // warns; the self/chained-staging forms fall through to the (empty)
        // file-scan arm below, a harmless no-op.
        Some(files)
            if files.is_empty()
                && !commit_stages_inline(cmd)
                && !chained_git_add_precedes_commit(cmd) =>
        {
            warnings.push("No staged changes detected".to_string());
        }
        Some(files) => {
            let sensitive: Vec<&String> =
                files.iter().filter(|f| is_sensitive_path(f)).collect();
            if !sensitive.is_empty() {
                let list = sensitive
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let msg = format!("Sensitive files staged: {list}");
                warnings.push(msg.clone());
                blocking.push(("secrets", msg));
                has_sensitive = true;
            }
            let generated: Vec<&String> =
                files.iter().filter(|f| is_generated_path(f)).collect();
            if !generated.is_empty() {
                let list = generated
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                warnings.push(format!("Generated/build files staged: {list}"));
            }
            if files.len() > 30 {
                warnings.push(format!(
                    "Large commit: {} files staged. Consider splitting.",
                    files.len()
                ));
            }
        }
        // git unavailable — fail open, no staged warnings.
        None => {}
    }

    // Check 5: build integrity — strict mode only.
    let mut build_ok: Option<bool> = None;
    if mode == Mode::Strict {
        if let Some(build_cmd) = read_build_command(project_dir) {
            let result = run_build(&build_cmd, project_dir);
            if !result.ok && !result.env_error {
                build_ok = Some(false);
                let out = truncate(&result.output, 300);
                let suffix = if result.output.len() > 300 { "…" } else { "" };
                let msg = format!("Build broken: {out}{suffix}");
                warnings.push(msg.clone());
                blocking.push(("build", msg));
            } else if result.ok {
                build_ok = Some(true);
            }
            // env_error → fail-open: leave `build_ok` as `None`, no warning.
        }
    }

    // Check 6: active pipeline advisory.
    let pipelines = active_pipelines(project_dir);
    if !pipelines.is_empty() {
        warnings.push(format!(
            "Active pipeline(s): {}. Ensure changes match spec.",
            pipelines.join(", ")
        ));
    }

    // Emit the harness event (best-effort).
    let blocking_types: Vec<&str> = blocking.iter().map(|(t, _)| *t).collect();
    emit_commit_gate_event(
        project_dir,
        ctx_session_id(ctx),
        mode,
        warnings.len(),
        &blocking_types,
        has_sensitive,
        build_ok,
    );

    // Strict mode: block on real sensor failures.
    if mode == Mode::Strict && !blocking.is_empty() {
        let what = blocking
            .iter()
            .map(|(_, m)| m.as_str())
            .collect::<Vec<_>>()
            .join(" | ");
        return Some(Verdict::Deny {
            reason: format_gate_message(
                "Commit Gate",
                &what,
                "committing secrets or a broken build is unrecoverable",
                "unstage the flagged files / fix the build, or set MUSTARD_COMMIT_GATE_MODE=warn",
            ),
        });
    }

    // Warn mode (or strict with no blocking finding): advisory on warnings.
    if !warnings.is_empty() {
        let reason = format_gate_message(
            "Review Gate",
            &warnings.join(" | "),
            "these may not belong in the commit",
            "review the staged changes before committing",
        );
        // `review-gate.js`: `permissionDecision: isStrictMode() ? 'deny' : 'allow'`.
        return Some(if is_strict_profile() {
            Verdict::Deny { reason }
        } else {
            Verdict::Warn { message: reason }
        });
    }

    None
}

/// `Ctx` carries no session id today, so the commit-gate event uses a
/// placeholder. Kept as a helper so a future `Ctx` field is a one-line change.
fn ctx_session_id(_ctx: &Ctx) -> Option<&str> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::model::contract::Trigger;
    use tempfile::tempdir;

    // --- review-gate parity (harness-wave9.test.js, tests 7-9) --------------

    /// `review-gate` only fires on a `git commit` command.
    #[test]
    fn review_gate_detects_git_commit() {
        assert!(is_git_commit("git commit -m \"feat: x\""));
        assert!(is_git_commit("rtk git commit -m \"feat: x\""));
        assert!(!is_git_commit("git add ."));
        assert!(!is_git_commit("git push origin dev"));
    }

    /// Regression (#3): a commit that stages its own changes (`-a`/`-am`/`--all`,
    /// or `commit -- <paths>`) must NOT trip the "No staged changes" advisory —
    /// the index is legitimately empty at PreToolUse time. A plain `git commit`
    /// (and `--amend`) still relies on a pre-staged index, so it is not inline.
    #[test]
    fn commit_stages_inline_detects_self_staging_forms() {
        assert!(commit_stages_inline("git commit -am \"msg\""));
        assert!(commit_stages_inline("rtk git commit -a -m \"msg\""));
        assert!(commit_stages_inline("git commit --all -m x"));
        assert!(commit_stages_inline("git commit -- src/a.ts"));
        // Plain index-driven commits are NOT inline-staging.
        assert!(!commit_stages_inline("git commit -m \"msg\""));
        assert!(!commit_stages_inline("git commit"));
        assert!(!commit_stages_inline("git commit --amend -m x"));
        // `-m"attached"` is not an all-letters cluster → not misread as `-a`.
        assert!(!commit_stages_inline("git commit -m\"add auth\""));
    }

    /// T6 (AC7): a `git add` chained *before* the commit pre-stages it, so the
    /// empty index at PreToolUse must NOT trip the "No staged changes" advisory.
    /// A plain commit with no chained add still warns.
    #[test]
    fn chained_git_add_before_commit_is_detected() {
        assert!(chained_git_add_precedes_commit("git add -A && git commit -m x"));
        assert!(chained_git_add_precedes_commit("git add . ; git commit -m x"));
        assert!(chained_git_add_precedes_commit("rtk git add -A && rtk git commit -m x"));
        // No add before the commit → the empty-index warning must stand.
        assert!(!chained_git_add_precedes_commit("git commit -m x"));
        // An add *after* the commit does not pre-stage it.
        assert!(!chained_git_add_precedes_commit("git commit -m x && git add -A"));
        // `git add` only inside the commit message is not a staging command.
        assert!(!chained_git_add_precedes_commit("git commit -m \"git add stuff\""));
    }

    /// A non-commit command never triggers the review gate.
    #[test]
    fn review_gate_ignores_non_commit_commands() {
        let ctx = Ctx {
            project_dir: String::new(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        assert_eq!(review_gate("git status", &ctx, Mode::Warn), None);
        assert_eq!(review_gate("npm run build", &ctx, Mode::Warn), None);
    }

    /// `Mode::Off` skips the gate entirely — even on a `git commit`.
    #[test]
    fn review_gate_off_mode_returns_none() {
        let ctx = Ctx {
            project_dir: String::new(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        assert_eq!(review_gate("git commit -m x", &ctx, Mode::Off), None);
    }

    /// With no git repo, the gate self-passes — git unavailable → no warnings.
    #[test]
    fn review_gate_fails_open_without_git_repo() {
        let dir = tempdir().unwrap();
        let ctx = Ctx {
            project_dir: dir.path().to_string_lossy().into_owned(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        // No `.git`, no `.pipeline-states` → no warnings → no verdict.
        assert_eq!(review_gate("git commit -m x", &ctx, Mode::Warn), None);
    }

    /// In a real git repo with a staged `.env`, the gate denies in strict mode
    /// (wave9 test 7) and only warns in warn mode (wave9 test 9).
    #[test]
    fn review_gate_strict_denies_staged_secret() {
        let dir = tempdir().unwrap();
        let repo = dir.path();
        // Skip gracefully if git is unavailable, mirroring the JS test.
        if Command::new("git")
            .args(["init"])
            .current_dir(repo)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| !s.success())
            .unwrap_or(true)
        {
            return;
        }
        // `staged_files()` runs git THROUGH rtk (the Golden Rule). On a clean CI
        // runner without rtk the staged-file probe fails open and the gate sees
        // nothing — so skip when rtk is absent, mirroring the git-unavailable
        // skip above, rather than asserting a verdict that cannot be produced.
        if Command::new("rtk")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| !s.success())
            .unwrap_or(true)
        {
            return;
        }
        std::fs::write(repo.join(".env"), "SECRET=abc123").unwrap();
        let _ = Command::new("git")
            .args(["add", ".env"])
            .current_dir(repo)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        let ctx = Ctx {
            project_dir: repo.to_string_lossy().into_owned(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        let warn = review_gate("git commit -m \"feat: x\"", &ctx, Mode::Warn);
        let strict = review_gate("git commit -m \"feat: x\"", &ctx, Mode::Strict);
        // Warn mode → non-blocking advisory; strict → blocking deny.
        assert!(
            matches!(warn, Some(Verdict::Warn { .. })),
            "warn-mode verdict: {warn:?}"
        );
        match strict {
            Some(Verdict::Deny { reason }) => {
                assert!(
                    reason.to_lowercase().contains("sensitive"),
                    "reason: {reason}"
                );
            }
            other => panic!("expected strict Deny, got {other:?}"),
        }
    }

    /// `format_gate_message` reproduces the `formatGateMessage` shape.
    #[test]
    fn gate_message_format_matches_js() {
        let msg = format_gate_message(
            "Review Gate",
            "Sensitive files staged: .env",
            "these may not belong in the commit",
            "review the staged changes before committing",
        );
        assert!(msg.starts_with("[Review Gate] "));
        assert!(msg.contains("Saída: "));
        assert!(msg.ends_with('.'));
    }
}
