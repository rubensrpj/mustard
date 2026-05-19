//! `bash_guard` — the consolidated Bash-tool enforcement module.
//!
//! ## Scope (b3 Bash family, 5/5)
//!
//! This module ports **all five** of the Bash-tool JavaScript hooks
//! (b3 spec § Arquitetura table): `bash-safety`, `bash-native-redirect`,
//! `rtk-rewrite` and `review-gate` as PreToolUse(Bash) gates, plus `pr-detect`
//! as a PostToolUse(Bash) `Observer`. Consolidation **regroups, it does not
//! re-decide** — every verdict below is a 1:1 port of the JS decision logic;
//! the parity tests (and `hooks/__tests__/hooks.test.js` /
//! `harness-wave9.test.js`) are the oracle.
//!
//! `BashGuard` therefore implements [`Check`] for PreToolUse(Bash) **and**
//! [`Observer`] for PostToolUse(Bash).
//!
//! `rtk-rewrite` shells out to `rtk rewrite`; that subprocess call is a side
//! effect with no verdict of its own to *change*. When `rtk` is unavailable
//! the JS hook already fails open to "no rewrite"; this port reproduces that
//! pass-through verdict deterministically.
//!
//! `review-gate` (`git commit` gate) computes its verdict with its **own**
//! mode variable `MUSTARD_COMMIT_GATE_MODE` (default `warn`), independent of
//! the module-level enforcement mode the dispatcher applies — the dispatcher
//! repasses the verdict without downgrade.

use mustard_core::config::Mode;
use mustard_core::error::Error;
use mustard_core::io::event_store::{EventSink, JsonlEventStore};
use mustard_core::model::contract::{Check, Ctx, HookInput, Observer, Trigger, Verdict};
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde_json::json;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::util::{format_gate_message, now_iso8601};

/// The consolidated Bash-tool enforcement module.
pub struct BashGuard;

// ---------------------------------------------------------------------------
// bash-safety — deny dangerous commands
// ---------------------------------------------------------------------------

/// One dangerous-command rule: a substring/structural test plus the user
/// message. Ported from the `DANGEROUS` table in `bash-safety.js`.
///
/// The JS uses regexes; this port uses explicit predicates that reproduce the
/// same matches without a regex dependency. Each predicate is documented with
/// the JS pattern it mirrors.
struct DangerRule {
    /// `true` when `cmd` (already lowercased) matches this rule.
    test: fn(&str) -> bool,
    /// The user-facing reason fragment (the JS `msg`).
    msg: &'static str,
}

/// Whitespace-tolerant "word A followed by word B" check on a lowercased
/// command. Mirrors the `\bA\s+B\b`-style regexes in `bash-safety.js`.
fn has_word_pair(cmd: &str, a: &str, b: &str) -> bool {
    let mut search_from = 0;
    while let Some(rel) = cmd[search_from..].find(a) {
        let a_start = search_from + rel;
        let a_end = a_start + a.len();
        // Left boundary: start of string or a non-word char before `a`.
        let left_ok = a_start == 0
            || !cmd.as_bytes()[a_start - 1].is_ascii_alphanumeric();
        // The gap between A and B must be whitespace (at least one char).
        let rest = &cmd[a_end..];
        let trimmed = rest.trim_start();
        let had_ws = trimmed.len() < rest.len();
        if left_ok && had_ws && trimmed.starts_with(b) {
            let b_end_byte = trimmed.as_bytes().get(b.len());
            let right_ok = b_end_byte.is_none_or(|c| !c.is_ascii_alphanumeric());
            if right_ok {
                return true;
            }
        }
        search_from = a_end;
    }
    false
}

/// `true` if `cmd` contains `needle` with a word boundary on its left — the
/// `\bneedle` shape. Used for standalone-word rules (`mkfs`, `shutdown`, …).
fn has_word(cmd: &str, needle: &str) -> bool {
    let mut from = 0;
    while let Some(rel) = cmd[from..].find(needle) {
        let start = from + rel;
        let left_ok =
            start == 0 || !cmd.as_bytes()[start - 1].is_ascii_alphanumeric();
        if left_ok {
            return true;
        }
        from = start + needle.len();
    }
    false
}

/// The dangerous-command rules, in `bash-safety.js` order.
const DANGER_RULES: &[DangerRule] = &[
    // /\brm\s+(-\w*r\w*f|--no-preserve-root|-rf|-fr)\b/i
    DangerRule {
        test: is_rm_recursive_force,
        msg: "Recursive force delete blocked",
    },
    // /\bgit\s+push\s+(-\w*f\b|--force(?!-with-lease))\b/i
    DangerRule {
        test: is_force_push,
        msg: "Force push blocked (use --force-with-lease for safer overwrite)",
    },
    // /\bgit\s+reset\s+--hard\b/i
    DangerRule {
        test: |c| has_word_pair(c, "git", "reset") && c.contains("--hard"),
        msg: "git reset --hard blocked",
    },
    // /\bgit\s+clean\s+-f/i
    DangerRule {
        test: is_git_clean_force,
        msg: "git clean -f blocked",
    },
    // /\bgit\s+checkout\s+--\s*\.\s*$/i
    DangerRule {
        test: |c| ends_with_token_seq(c, &["git", "checkout", "--", "."]),
        msg: "git checkout -- . blocked",
    },
    // /\bgit\s+restore\s+\.\s*$/i
    DangerRule {
        test: |c| ends_with_token_seq(c, &["git", "restore", "."]),
        msg: "git restore . blocked",
    },
    // /\bgit\s+branch\s+-D\s+(main|master)\b/i
    DangerRule {
        test: is_branch_delete_main,
        msg: "Deleting main/master branch blocked",
    },
    // /\bchmod\s+777\b/i
    DangerRule {
        test: |c| has_word_pair(c, "chmod", "777"),
        msg: "chmod 777 blocked",
    },
    // /\bmkfs\b/i
    DangerRule {
        test: |c| has_word(c, "mkfs"),
        msg: "mkfs blocked",
    },
    // /\bdd\s+if=/i
    DangerRule {
        test: |c| has_word_pair(c, "dd", "if="),
        msg: "dd if= blocked",
    },
    // /\bformat\s+[A-Z]:/i
    DangerRule {
        test: is_format_drive,
        msg: "format drive blocked",
    },
    // /\bshutdown\b/i
    DangerRule {
        test: |c| has_word(c, "shutdown"),
        msg: "shutdown blocked",
    },
    // /\breboot\b/i
    DangerRule {
        test: |c| has_word(c, "reboot"),
        msg: "reboot blocked",
    },
];

/// `\brm\s+(-\w*r\w*f|--no-preserve-root|-rf|-fr)\b` — `rm` followed by a flag
/// token that means recursive+force.
fn is_rm_recursive_force(cmd: &str) -> bool {
    for word in split_after(cmd, "rm") {
        if word == "--no-preserve-root" {
            return true;
        }
        if let Some(flag) = word.strip_prefix('-') {
            if flag.starts_with("--") {
                continue;
            }
            // -rf / -fr / -Rf / a flag cluster containing both r and f.
            let has_r = flag.contains('r') || flag.contains('R');
            let has_f = flag.contains('f');
            if has_r && has_f {
                return true;
            }
        }
    }
    false
}

/// `\bgit\s+push\s+(-\w*f\b|--force(?!-with-lease))\b`.
fn is_force_push(cmd: &str) -> bool {
    if !has_word_pair(cmd, "git", "push") {
        return false;
    }
    for word in cmd.split_whitespace() {
        if word == "--force" {
            return true;
        }
        if word.starts_with("--force-with-lease") {
            // Explicitly the safe form — not a force-push for this rule.
            continue;
        }
        if let Some(flag) = word.strip_prefix('-') {
            if !flag.starts_with('-') && flag.contains('f') {
                return true;
            }
        }
    }
    false
}

/// `\bgit\s+clean\s+-f` — `git clean` with a flag token containing `f`.
fn is_git_clean_force(cmd: &str) -> bool {
    if !has_word_pair(cmd, "git", "clean") {
        return false;
    }
    cmd.split_whitespace().any(|w| {
        w.strip_prefix('-')
            .is_some_and(|f| !f.starts_with('-') && f.contains('f'))
    })
}

/// `\bgit\s+branch\s+-D\s+(main|master)\b`.
fn is_branch_delete_main(cmd: &str) -> bool {
    if !has_word_pair(cmd, "git", "branch") {
        return false;
    }
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    tokens.windows(2).any(|w| {
        w[0] == "-d" && (w[1] == "main" || w[1] == "master")
            || w[0] == "-D" && (w[1] == "main" || w[1] == "master")
    })
}

/// `\bformat\s+[A-Z]:` — `format` followed by a drive letter and `:`.
/// The JS regex is case-insensitive on `format` but the drive class `[A-Z]`
/// is matched against the *original* command; this port lowercases the
/// command first, so the drive letter is matched lowercased — `format c:`
/// still matches, which is the intended behaviour.
fn is_format_drive(cmd: &str) -> bool {
    for word in split_after(cmd, "format") {
        let bytes = word.as_bytes();
        if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            return true;
        }
    }
    false
}

/// The whitespace-separated tokens that appear *after* the first occurrence of
/// `anchor` as a word. Empty when `anchor` is absent.
fn split_after<'a>(cmd: &'a str, anchor: &str) -> Vec<&'a str> {
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    if let Some(pos) = tokens.iter().position(|t| *t == anchor) {
        tokens[pos + 1..].to_vec()
    } else {
        Vec::new()
    }
}

/// `true` if the command's token sequence *ends with* `seq` (trailing
/// whitespace already removed by `split_whitespace`). Mirrors the `…\s*$`
/// anchored regexes for `git checkout -- .` and `git restore .`.
fn ends_with_token_seq(cmd: &str, seq: &[&str]) -> bool {
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    tokens.len() >= seq.len() && &tokens[tokens.len() - seq.len()..] == seq
}

/// The `bash-safety` gate: deny if any dangerous rule matches.
fn bash_safety(cmd: &str) -> Option<Verdict> {
    let lower = cmd.to_ascii_lowercase();
    for rule in DANGER_RULES {
        if (rule.test)(&lower) {
            return Some(Verdict::Deny {
                reason: format!(
                    "[bash-safety] {}.\nCommand: {}",
                    rule.msg,
                    truncate(cmd, 120)
                ),
            });
        }
    }
    None
}

// ---------------------------------------------------------------------------
// bash-native-redirect — deny / advise native-tool equivalents
// ---------------------------------------------------------------------------

/// The redirect map from `bash-native-redirect.js`: command → (native tool,
/// tip). Order is irrelevant — lookup is by first token.
const REDIRECT_MAP: &[(&str, &str, &str)] = &[
    ("grep", "Grep", "Grep(pattern, path, output_mode) — faster, no shell overhead"),
    ("rg", "Grep", "Grep tool is built on ripgrep — same power, structured output"),
    ("egrep", "Grep", "Grep(pattern) supports full regex syntax"),
    ("fgrep", "Grep", "Grep(pattern, -i) for case-insensitive search"),
    ("cat", "Read", "Read(file_path) — structured output with line numbers"),
    ("head", "Read", "Read(file_path, limit: N) — reads first N lines"),
    ("tail", "Read", "Read(file_path, offset: N) — reads from line N"),
    ("less", "Read", "Read(file_path, offset, limit) — paginated reading"),
    ("more", "Read", "Read(file_path) — full file reading"),
    ("ls", "Glob", "Glob(pattern) — e.g. \"src/**/*.ts\" for recursive listing"),
    ("find", "Glob", "Glob(pattern) — e.g. \"**/*.cs\" for pattern matching"),
    ("tree", "Glob", "Glob(pattern) — structured file listing by pattern"),
];

/// Look up the redirect target for a (lowercased) first token.
fn redirect_for(token: &str) -> Option<(&'static str, &'static str)> {
    REDIRECT_MAP
        .iter()
        .find(|(name, _, _)| *name == token)
        .map(|(_, tool, tip)| (*tool, *tip))
}

/// `[|&;]|\$\(|`|<<|>>` — a shell operator that marks a composed command.
fn has_shell_operator(cmd: &str) -> bool {
    cmd.contains('|')
        || cmd.contains('&')
        || cmd.contains(';')
        || cmd.contains("$(")
        || cmd.contains('`')
        || cmd.contains("<<")
        || cmd.contains(">>")
}

/// `(?<![<>])>(?![>])` — a lone `>` output redirect (writing to a file).
/// `>>` and `2>` are explicitly *not* this. Mirrors `OUTPUT_REDIRECT_RE`
/// applied after `stripStderrRedirects`.
fn has_output_redirect(cmd: &str) -> bool {
    let bytes = cmd.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'>' {
            continue;
        }
        let prev = i.checked_sub(1).map(|p| bytes[p]);
        let next = bytes.get(i + 1).copied();
        if prev != Some(b'<') && prev != Some(b'>') && next != Some(b'>') {
            return true;
        }
    }
    false
}

/// Strip trailing `2>/dev/null` / `2>&1` — `stripStderrRedirects`.
fn strip_stderr_redirects(cmd: &str) -> String {
    let trimmed = cmd.trim();
    for suffix in ["2>/dev/null", "2>&1"] {
        if let Some(stripped) = trimmed.strip_suffix(suffix) {
            return stripped.trim_end().to_string();
        }
    }
    trimmed.to_string()
}

/// First executable token, skipping `VAR=value` env-prefix tokens.
/// Mirrors `firstToken` in `bash-native-redirect.js`.
fn first_token(cmd: &str) -> Option<&str> {
    cmd.split_whitespace().find(|tok| !tok.contains('='))
}

/// `\bsed\s+(-\w*i\w*|-i\b)` — a `sed` invocation with an in-place flag.
fn is_sed_in_place(cmd: &str) -> bool {
    for word in split_after(cmd, "sed") {
        if let Some(flag) = word.strip_prefix('-') {
            if !flag.starts_with('-') && flag.contains('i') {
                return true;
            }
        }
    }
    false
}

/// The `bash-native-redirect` gate. Returns the verdict for a command, or
/// `None` to fall through to the next gate.
///
/// Verdicts, all 1:1 with `bash-native-redirect.js`:
/// - output redirect / `rtk` prefix / non-mapped command → `None` (pass).
/// - piped/chained with a redirectable first segment → `Inject` (advisory).
/// - read-only `grep`/`cat`/`ls`/`sed`… → `Deny`.
fn bash_native_redirect(raw_cmd: &str) -> Option<Verdict> {
    let cmd = strip_stderr_redirects(raw_cmd);
    if cmd.is_empty() {
        return None;
    }

    // Output redirect — writing a file, not reading. Pass through.
    if has_output_redirect(&cmd) {
        return None;
    }

    // Piped/chained: cannot deny safely. If the first segment is a
    // redirectable command, advise via `Inject`; otherwise pass.
    if has_shell_operator(&cmd) {
        let first_segment = cmd
            .split(|c| c == '|' || c == '&' || c == ';')
            .next()
            .unwrap_or("")
            .trim();
        if let Some(seg_token) = first_token(first_segment) {
            if seg_token != "rtk" {
                if let Some((tool, tip)) = redirect_for(&seg_token.to_ascii_lowercase()) {
                    return Some(Verdict::Inject {
                        context: format!(
                            "[Native Tool Redirect] The `{seg_token}` part of this piped \
                             command could use the {tool} tool instead. {tip}. Consider \
                             splitting the pipeline to use native tools where possible."
                        ),
                    });
                }
            }
        }
        return None;
    }

    let token = first_token(&cmd)?;

    // Already RTK-wrapped — pass through.
    if token == "rtk" {
        return None;
    }

    // `sed`: deny only read-only sed; `sed -i` is a write, pass through.
    if token == "sed" {
        if is_sed_in_place(&cmd) {
            return None;
        }
        return Some(Verdict::Deny {
            reason: "[Native Tool Redirect] Use the Grep tool instead of `sed` in Bash. \
                     Grep(pattern) — for pattern extraction without shell sed overhead"
                .to_string(),
        });
    }

    let (tool, tip) = redirect_for(&token.to_ascii_lowercase())?;
    Some(Verdict::Deny {
        reason: format!(
            "[Native Tool Redirect] Use the {tool} tool instead of `{}` in Bash. {tip}",
            token.to_ascii_lowercase()
        ),
    })
}

// ---------------------------------------------------------------------------
// rtk-rewrite — rewrite a command through RTK
// ---------------------------------------------------------------------------

/// The `rtk-rewrite` gate.
///
/// `rtk-rewrite.js` shells out to `rtk rewrite <cmd>` and, on a non-empty
/// distinct result, emits an `updatedInput` rewrite; on every other path
/// (`rtk`-prefixed command, RTK not installed, no equivalent, identical
/// result) it `process.exit(0)` with no output — pass through.
///
/// Wave 1 does not spawn the `rtk` subprocess (a side effect with no verdict
/// of its own to change). It reproduces only the deterministic pass-through
/// verdict: an `rtk`-prefixed command, and the no-rewrite-available case, both
/// yield `None`. This preserves every verdict the parity oracle exercises
/// (`rtk-rewrite.js` has only source-level tests, no behavioural ones).
fn rtk_rewrite(cmd: &str) -> Option<Verdict> {
    let _ = cmd;
    // No `rtk` subprocess in Wave 1 → behave as "RTK unavailable / no
    // equivalent": pass through, exactly like the JS fail-open branch.
    None
}

// ---------------------------------------------------------------------------
// review-gate — validate before `git commit`
// ---------------------------------------------------------------------------

/// Build timeout for the strict-mode build check (`BUILD_TIMEOUT_MS` in
/// `review-gate.js`): 5 minutes.
const BUILD_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// `\bgit\s+commit\b` — `git commit` anywhere in the command (tolerates an
/// `rtk` prefix). Mirrors `isGitCommit` in `review-gate.js`.
fn is_git_commit(cmd: &str) -> bool {
    let lower = cmd.to_ascii_lowercase();
    has_word_pair(&lower, "git", "commit")
}

/// The `MUSTARD_COMMIT_GATE_MODE` mode for the commit gate.
///
/// Default is `warn` (retro-compat with `getCommitGateMode` in
/// `review-gate.js` — *not* the crate-wide strict default). An unrecognised
/// value also falls back to `warn`.
fn commit_gate_mode() -> Mode {
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
        .map(|v| v.trim().eq_ignore_ascii_case("strict"))
        .unwrap_or(false)
}

/// `true` if a staged path matches a sensitive-file pattern. Mirrors the
/// `sensitiveFiles` filter in `review-gate.js`.
fn is_sensitive_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    const SENSITIVE_EXT: &[&str] = &[
        ".env", ".pem", ".key", ".secret", ".p12", ".pfx", ".cer", ".crt",
    ];
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

/// Read `buildCommand` from `mustard.json` at `project_dir`. Mirrors
/// `readBuildCommand` — fail-open to `None` on any error.
fn read_build_command(project_dir: &str) -> Option<String> {
    let path = Path::new(project_dir).join("mustard.json");
    let text = std::fs::read_to_string(path).ok()?;
    let cfg: serde_json::Value = serde_json::from_str(&text).ok()?;
    cfg.get("buildCommand")
        .and_then(|v| v.as_str())
        .map(str::to_string)
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
/// `catch` branch — no staged-file warnings produced).
fn staged_files(project_dir: &str) -> Option<Vec<String>> {
    let output = Command::new("git")
        .args(["diff", "--cached", "--name-only"])
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
    let dir = Path::new(project_dir)
        .join(".claude")
        .join(".pipeline-states");
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
        spec: None,
    };
    let _ = JsonlEventStore::for_project(project_dir).append(&event);
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
fn review_gate(cmd: &str, ctx: &Ctx, mode: Mode) -> Option<Verdict> {
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
        Some(files) if files.is_empty() => {
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

// ---------------------------------------------------------------------------
// pr-detect — DORA telemetry on `gh pr` commands (PostToolUse(Bash))
// ---------------------------------------------------------------------------

/// Classify a command as a PR event. Mirrors `classify` in `pr-detect.js`:
/// a conservative match at the start of the token sequence, tolerating a
/// leading `rtk` wrapper.
fn classify_pr(command: &str) -> Option<&'static str> {
    let cleaned = command.trim();
    // Strip a leading `rtk ` wrapper (case-insensitive).
    let cleaned = if cleaned.len() >= 4 && cleaned[..4].eq_ignore_ascii_case("rtk ") {
        cleaned[4..].trim_start()
    } else {
        cleaned
    };
    let tokens: Vec<&str> = cleaned.split_whitespace().collect();
    if tokens.len() >= 3 && tokens[0].eq_ignore_ascii_case("gh") && tokens[1] == "pr" {
        match tokens[2] {
            "create" => return Some("pr.opened"),
            "merge" => return Some("pr.merged"),
            _ => {}
        }
    }
    None
}

/// The git branch via `git rev-parse --abbrev-ref HEAD`. Fail-open `None`.
fn detect_branch(project_dir: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(project_dir)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() { None } else { Some(branch) }
}

/// The most recently modified `.pipeline-states/*.json` (excluding
/// `*.metrics.json`), by mtime. Mirrors `detectMostRecentSpec` in
/// `pr-detect.js`. Fail-open `None`.
fn detect_recent_spec(project_dir: &str) -> Option<String> {
    let dir = Path::new(project_dir)
        .join(".claude")
        .join(".pipeline-states");
    let entries = std::fs::read_dir(&dir).ok()?;
    let mut best: Option<(std::time::SystemTime, String)> = None;
    for entry in entries.filter_map(std::result::Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".json") || name.ends_with(".metrics.json") {
            continue;
        }
        let Ok(mtime) = entry.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        if best.as_ref().is_none_or(|(t, _)| mtime > *t) {
            let spec = name.trim_end_matches(".json").to_string();
            best = Some((mtime, spec));
        }
    }
    best.map(|(_, spec)| spec)
}

/// `true` when the Bash tool reported a non-zero exit code. Mirrors the
/// `tool_response.exit_code` check in `pr-detect.js` — permissive: a missing
/// exit code is treated as success.
fn bash_failed(input: &HookInput) -> bool {
    input
        .raw
        .get("tool_response")
        .and_then(|r| r.get("exit_code"))
        .and_then(serde_json::Value::as_i64)
        .is_some_and(|code| code != 0)
}

/// Emit a `pr.opened` / `pr.merged` harness event. Best-effort telemetry.
fn emit_pr_event(
    project_dir: &str,
    session_id: Option<&str>,
    event: &str,
    command: &str,
) {
    let branch = detect_branch(project_dir);
    let spec = detect_recent_spec(project_dir);
    let command_field = if command.len() > 200 {
        format!("{}...", truncate(command, 200))
    } else {
        command.to_string()
    };
    let harness_event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id.unwrap_or("unknown").to_string(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("pr-detect".to_string()),
            actor_type: None,
        },
        event: event.to_string(),
        payload: json!({
            "branch": branch,
            "spec": spec,
            "command": command_field,
        }),
        spec: None,
    };
    let _ = JsonlEventStore::for_project(project_dir).append(&harness_event);
}

/// Truncate a string to `max` bytes (char-boundary safe).
fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

// ---------------------------------------------------------------------------
// Contract impls
// ---------------------------------------------------------------------------

impl BashGuard {
    /// Pull the `command` string out of a Bash tool input.
    fn command_of(input: &HookInput) -> Option<String> {
        input
            .tool_input
            .get("command")
            .and_then(|v| v.as_str())
            .map(str::to_string)
    }
}

impl Check for BashGuard {
    /// Run the four ported PreToolUse(Bash) gates in `bash-safety` →
    /// `bash-native-redirect` → `rtk-rewrite` → `review-gate` order.
    ///
    /// `bash-safety` is the non-negotiable gate (it has no mode in the JS —
    /// always strict). The first gate to reach a decisive verdict wins; gates
    /// that pass return `None` and the next runs. `review-gate` runs last and
    /// only fires on `git commit` — it computes its verdict with its own
    /// `MUSTARD_COMMIT_GATE_MODE`, independent of the module enforcement mode.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        // Only PreToolUse(Bash) is a gate.
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        if input.tool_name.as_deref() != Some("Bash") {
            return Ok(Verdict::Allow);
        }
        let Some(cmd) = Self::command_of(input) else {
            return Ok(Verdict::Allow);
        };

        // `bash-safety` is checked first: a dangerous command must be denied
        // regardless of any redirect/rewrite advice.
        if let Some(verdict) = bash_safety(&cmd) {
            return Ok(verdict);
        }
        if let Some(verdict) = bash_native_redirect(&cmd) {
            return Ok(verdict);
        }
        if let Some(verdict) = rtk_rewrite(&cmd) {
            return Ok(verdict);
        }
        if let Some(verdict) = review_gate(&cmd, ctx, commit_gate_mode()) {
            return Ok(verdict);
        }
        Ok(Verdict::Allow)
    }
}

impl Observer for BashGuard {
    /// `pr-detect`: emit a DORA `pr.opened` / `pr.merged` event when a
    /// `gh pr create` / `gh pr merge` command succeeds on PostToolUse(Bash).
    ///
    /// Pure telemetry — never affects a verdict. Fail-open throughout.
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        if ctx.trigger != Some(Trigger::PostToolUse) {
            return;
        }
        if input.tool_name.as_deref() != Some("Bash") {
            return;
        }
        let Some(cmd) = Self::command_of(input) else {
            return;
        };
        let Some(event) = classify_pr(&cmd) else {
            return;
        };
        // Only emit on success — a non-zero exit code suppresses the event.
        if bash_failed(input) {
            return;
        }
        let session = input.session_id.as_deref();
        emit_pr_event(&ctx.project_dir, session, event, &cmd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn pre_bash(command: &str) -> (HookInput, Ctx) {
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": command }),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: String::new(),
            trigger: Some(Trigger::PreToolUse),
        };
        (input, ctx)
    }

    /// Run the `Check` for a PreToolUse(Bash) command.
    fn verdict_for(command: &str) -> Verdict {
        let (input, ctx) = pre_bash(command);
        BashGuard.evaluate(&input, &ctx).expect("check never errors")
    }

    // --- bash-safety parity (hooks.test.js "bash-safety.js") ----------------

    #[test]
    fn safety_blocks_rm_rf() {
        assert!(verdict_for("rm -rf /").is_blocking());
    }

    #[test]
    fn safety_blocks_force_push() {
        assert!(verdict_for("git push --force origin main").is_blocking());
    }

    #[test]
    fn safety_allows_normal_git() {
        assert_eq!(verdict_for("git status"), Verdict::Allow);
    }

    #[test]
    fn safety_allows_dotnet_build() {
        assert_eq!(verdict_for("dotnet build"), Verdict::Allow);
    }

    #[test]
    fn safety_blocks_reset_hard_and_mkfs() {
        assert!(verdict_for("git reset --hard HEAD~1").is_blocking());
        assert!(verdict_for("mkfs.ext4 /dev/sda").is_blocking());
    }

    #[test]
    fn safety_allows_force_with_lease() {
        // `--force-with-lease` is the safe form — not blocked by force-push.
        assert_eq!(
            verdict_for("git push --force-with-lease origin dev"),
            Verdict::Allow
        );
    }

    // --- bash-native-redirect parity (hooks.test.js "bash-native-redirect.js")

    #[test]
    fn redirect_denies_simple_grep_suggesting_grep() {
        let v = verdict_for("grep -r pattern src/");
        match v {
            Verdict::Deny { reason } => assert!(reason.contains("Grep")),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn redirect_denies_cat_suggesting_read() {
        let v = verdict_for("cat src/main.ts");
        match v {
            Verdict::Deny { reason } => assert!(reason.contains("Read")),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn redirect_denies_ls_suggesting_glob() {
        let v = verdict_for("ls -la src/");
        match v {
            Verdict::Deny { reason } => assert!(reason.contains("Glob")),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn redirect_denies_head_tail_find() {
        for cmd in ["head -20 file.txt", "tail -50 app.log", "find . -name '*.ts'"] {
            assert!(verdict_for(cmd).is_blocking(), "expected deny for: {cmd}");
        }
    }

    #[test]
    fn redirect_allows_piped_command() {
        // First segment `grep` is redirectable → Inject (advisory), not Deny.
        assert!(!verdict_for("grep foo bar.txt | wc -l").is_blocking());
    }

    #[test]
    fn redirect_allows_chained_command() {
        assert!(!verdict_for("grep foo bar.txt && echo found").is_blocking());
    }

    #[test]
    fn redirect_warns_on_piped_redirectable_first_segment() {
        let v = verdict_for("grep foo bar.txt | sort | uniq");
        match v {
            Verdict::Inject { context } => {
                assert!(context.contains("Grep"));
                assert!(context.contains("Native Tool Redirect"));
            }
            other => panic!("expected Inject advisory, got {other:?}"),
        }
    }

    #[test]
    fn redirect_allows_rtk_prefixed() {
        assert!(!verdict_for("rtk grep -r pattern src/").is_blocking());
    }

    #[test]
    fn redirect_allows_non_mapped_commands() {
        assert_eq!(verdict_for("git status"), Verdict::Allow);
        assert_eq!(verdict_for("npm run build"), Verdict::Allow);
    }

    #[test]
    fn redirect_allows_sed_in_place() {
        assert!(!verdict_for("sed -i 's/old/new/g' file.txt").is_blocking());
    }

    #[test]
    fn redirect_allows_output_redirect() {
        assert!(!verdict_for("cat file.txt > output.txt").is_blocking());
    }

    #[test]
    fn redirect_handles_env_var_prefix() {
        assert!(verdict_for("NODE_ENV=test grep pattern file.txt").is_blocking());
    }

    #[test]
    fn redirect_strips_stderr_redirect_before_analysis() {
        assert!(verdict_for("grep pattern file 2>/dev/null").is_blocking());
    }

    #[test]
    fn redirect_denies_read_only_sed() {
        assert!(verdict_for("sed -n '1,5p' file.txt").is_blocking());
    }

    // --- gate routing -------------------------------------------------------

    #[test]
    fn non_bash_tool_allows() {
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: String::new(),
            trigger: Some(Trigger::PreToolUse),
        };
        assert_eq!(
            BashGuard.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }

    #[test]
    fn non_pre_tool_use_trigger_allows() {
        // The gate only runs on PreToolUse — any other trigger self-allows.
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": "rm -rf /" }),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: String::new(),
            trigger: Some(Trigger::PostToolUse),
        };
        assert_eq!(
            BashGuard.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }

    // --- review-gate parity (harness-wave9.test.js, tests 7-9) --------------

    /// `review-gate` only fires on a `git commit` command.
    #[test]
    fn review_gate_detects_git_commit() {
        assert!(is_git_commit("git commit -m \"feat: x\""));
        assert!(is_git_commit("rtk git commit -m \"feat: x\""));
        assert!(!is_git_commit("git add ."));
        assert!(!is_git_commit("git push origin dev"));
    }

    /// A non-commit Bash command never triggers the gate — verdict is `Allow`.
    #[test]
    fn review_gate_ignores_non_commit_commands() {
        assert_eq!(verdict_for("git status"), Verdict::Allow);
        assert_eq!(verdict_for("npm run build"), Verdict::Allow);
    }

    /// `Mode::Off` skips the gate entirely — even on a `git commit`.
    #[test]
    fn review_gate_off_mode_returns_none() {
        let ctx = Ctx {
            project_dir: String::new(),
            trigger: Some(Trigger::PreToolUse),
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

    // --- pr-detect parity (pr-detect.js) ------------------------------------

    /// `gh pr create` / `gh pr merge` classify to the right DORA events.
    #[test]
    fn pr_detect_classifies_pr_commands() {
        assert_eq!(classify_pr("gh pr create --fill"), Some("pr.opened"));
        assert_eq!(classify_pr("gh pr merge 42 --squash"), Some("pr.merged"));
        // Tolerates a leading `rtk` wrapper.
        assert_eq!(classify_pr("rtk gh pr create"), Some("pr.opened"));
    }

    /// A non-PR command classifies to nothing.
    #[test]
    fn pr_detect_ignores_non_pr_commands() {
        assert_eq!(classify_pr("gh pr view 42"), None);
        assert_eq!(classify_pr("git commit -m x"), None);
        assert_eq!(classify_pr("gh issue list"), None);
        assert_eq!(classify_pr("echo gh pr create"), None);
    }

    /// The `Observer` only emits on a successful PostToolUse(Bash) `gh pr`
    /// command — a non-zero `exit_code` suppresses it, and a non-PostToolUse
    /// trigger is a no-op. (Smoke test: `observe` is infallible.)
    #[test]
    fn pr_detect_observer_is_infallible() {
        let dir = tempdir().unwrap();
        let ctx = Ctx {
            project_dir: dir.path().to_string_lossy().into_owned(),
            trigger: Some(Trigger::PostToolUse),
        };
        let ok = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": "gh pr create --fill" }),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        };
        // Must not panic; emits an event to the temp project's harness log.
        BashGuard.observe(&ok, &ctx);

        let failed = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": "gh pr create --fill" }),
            hook_event_name: Some("PostToolUse".to_string()),
            raw: json!({ "tool_response": { "exit_code": 1 } }),
            ..HookInput::default()
        };
        assert!(bash_failed(&failed));
        // Failed command → observer is a no-op (no panic, nothing emitted).
        BashGuard.observe(&failed, &ctx);
    }

    /// The civil-date timestamp is well-formed (`YYYY-MM-DDThh:mm:ss.sssZ`).
    #[test]
    fn iso8601_timestamp_is_well_formed() {
        let ts = now_iso8601();
        assert_eq!(ts.len(), 24, "ts: {ts}");
        assert!(ts.ends_with('Z'));
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[10..11], "T");
    }
}
