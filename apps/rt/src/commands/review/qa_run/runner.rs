//! qa-run acceptance-criteria execution engine: locate the spec file, run each
//! AC command (with per-AC timeouts and self-invocation guards), and emit the
//! `qa.result` event and metric. Split out of `qa_run` (F3 PERF-D).

use crate::shared::context::session_id;
use crate::shared::proc::{run_shell_with_deadline, ShellOutcome};
use mustard_core::io::fs;
use mustard_core::ClaudePaths;
use mustard_core::time::now_iso8601;
use mustard_core::platform::metrics::{emit_metric, MetricLine};
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use super::{AcResult, QaRunOptions};

/// Default per-AC timeout (2 min) for non-cargo commands, matching
/// `AC_TIMEOUT_MS` in `qa-run.js`.
const AC_TIMEOUT_SECS: u64 = 120;

/// The POSIX shell's "command not found" exit code.
///
/// `pub(crate)` because the judgement it enables belongs to a CALLER, not to
/// this module: [`crate::commands::review::ac_negative_check`] must not read an
/// unrunnable command as its red proof, while `qa-run` must still fail on one.
/// Both read the same record and reach opposite, correct verdicts — which only
/// works while the code is a shared constant instead of a literal each side
/// spells for itself.
pub(crate) const EXIT_COMMAND_NOT_FOUND: i64 = 127;

/// How a refusal names the running executable when no machine-independent name
/// can be produced for it. The reason still has to read as a sentence, and it
/// must never degrade into an absolute path — see [`running_binary_label`].
pub(super) const UNNAMEABLE_BINARY: &str = "the binary running this pass";

/// Per-AC timeout ceiling (10 min) for commands invoking `cargo `: a
/// `cargo build`/`cargo test` AC that runs right after an edit must recompile,
/// and a cold compile routinely exceeds the 120 s default (real case:
/// `cargo test -p mustard-rt` hit 120 s mid-recompile and degraded to a
/// silent `skip`). Mirrors `TIMEOUT_RUST_SECS` in `verify-pipeline`.
const AC_TIMEOUT_CARGO_SECS: u64 = 600;

/// The cargo target directory a build launched from `cwd` would write into:
/// `CARGO_TARGET_DIR` when it is set, otherwise `<workspace root>/target`,
/// where the workspace root is the TOPMOST ancestor of `cwd` carrying a
/// `Cargo.toml`.
///
/// `None` when `cwd` is not inside a cargo project at all — a build launched
/// there writes nothing this process could be executing from.
fn cargo_target_root(cwd: &Path) -> Option<PathBuf> {
    let root = cwd.ancestors().filter(|dir| dir.join("Cargo.toml").is_file()).last()?;
    let configured = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .filter(|dir| !dir.as_os_str().is_empty());
    Some(match configured {
        Some(dir) if dir.is_absolute() => dir,
        Some(dir) => root.join(dir),
        None => root.join("target"),
    })
}

/// `path` rendered for comparison: resolved through the filesystem when it
/// exists, with the Windows verbatim prefix, separators and case flattened.
///
/// Two spellings of the same file must compare equal; a path cargo has not
/// written yet simply compares as itself (canonicalization fails there, which
/// is the right answer — a file that does not exist is not the file this
/// process is executing from).
fn comparable(path: &Path) -> String {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let text = resolved.to_string_lossy().to_string();
    if cfg!(windows) {
        text.replace('\\', "/").trim_start_matches("//?/").to_ascii_lowercase()
    } else {
        text
    }
}

/// `true` when both paths name the same file on this machine.
fn same_file(a: &Path, b: &Path) -> bool {
    comparable(a) == comparable(b)
}

/// `true` for the two cargo subcommands that relink a crate's binary.
fn is_cargo_build_or_test(lower_command: &str) -> bool {
    lower_command.contains("cargo build") || lower_command.contains("cargo test")
}

/// The profile directory cargo writes into for this command: `release` under
/// `--release`, the `--profile` value otherwise (`dev`/`test` both land in
/// `debug`, `bench` in `release`), and `debug` when nothing is said.
fn profile_dir(tokens: &[&str]) -> String {
    let mut profile = "debug".to_string();
    for (i, token) in tokens.iter().enumerate() {
        if *token == "--release" {
            profile = "release".to_string();
        } else if let Some(name) = token
            .strip_prefix("--profile=")
            .or_else(|| (*token == "--profile").then(|| tokens.get(i + 1).copied()).flatten())
        {
            profile = match name {
                "dev" | "test" => "debug".to_string(),
                "bench" => "release".to_string(),
                other => other.to_string(),
            };
        }
    }
    profile
}

/// The packages `command` names DIRECTLY via `-p`/`--package`, in both the
/// split (`-p mustard-rt`) and glued (`-p=mustard-rt`) spellings, matched on
/// token boundaries so `-p mustard-rt-extras` is a different package.
fn named_packages<'a>(tokens: &[&'a str]) -> Vec<&'a str> {
    tokens
        .iter()
        .enumerate()
        .filter_map(|(i, token)| {
            if *token == "-p" || *token == "--package" {
                return tokens.get(i + 1).copied();
            }
            token.strip_prefix("-p=").or_else(|| token.strip_prefix("--package="))
        })
        .collect()
}

/// The package whose freshly-built binary WOULD BE the file `running`, for a
/// build described by `command` under `target_root` — the path question, asked
/// once and shared by both callers below.
///
/// `None` whenever the directory this command writes into is not the directory
/// the running binary lives in, which is the ordinary case: the process runs
/// from an installed path (`~/.cargo/bin/mustard-rt`) while cargo writes
/// `target/debug/mustard-rt.exe`. Two different files, no conflict to protect
/// against.
fn package_shadowing_running(command: &str, target_root: &Path, running: &Path) -> Option<String> {
    let lower = command.to_ascii_lowercase();
    if !is_cargo_build_or_test(&lower) {
        return None;
    }
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    let out_dir = target_root.join(profile_dir(&tokens));
    if !same_file(running.parent()?, &out_dir) {
        return None;
    }
    Some(running.file_stem()?.to_string_lossy().to_string())
}

/// THE question the self-invocation guard asks: would running `command`
/// overwrite the very file this process is executing from?
///
/// It compares PATHS — the build target the command would write against the
/// running executable — instead of matching crate names in the command text.
/// The text match refused `cargo test -p mustard-rt` by spelling alone, even
/// when the two files were plainly different, and one refusal is enough to
/// deny a whole QA run.
///
/// Only the DIRECT `-p`/`--package` form is answered here; the `--workspace`
/// form is salvaged instead of refused — see [`rewrite_workspace_exclusion`].
fn overwrites_running_binary(command: &str, target_root: &Path, running: &Path) -> bool {
    let Some(package) = package_shadowing_running(command, target_root, running) else {
        return false;
    };
    let lower = command.to_ascii_lowercase();
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    named_packages(&tokens).iter().any(|named| named.eq_ignore_ascii_case(&package))
}

/// The running binary named the way a refusal must name it: relative to the
/// project when it lives inside it, so the committed QA report carries
/// `target/debug/mustard-rt.exe` and never an absolute path that only exists on
/// one machine.
///
/// The binary can also sit OUTSIDE the project and still be refused: with
/// `CARGO_TARGET_DIR` pointing somewhere absolute, the build target and the
/// running file coincide out there. Falling back to the raw path would
/// interpolate a machine path into `stderr_excerpt` → `qa/report.md` and into
/// `reason` → `ac-proof.json`, both versioned files, breaking this crate's
/// "deterministic output, no volatile paths" guard. So the fallback keeps the
/// file NAME only — which is all the refusal needs to name what it protects.
pub(super) fn running_binary_label(cwd: &Path, running: &Path) -> String {
    match running.strip_prefix(cwd) {
        Ok(relative) => relative.to_string_lossy().replace('\\', "/"),
        Err(_) => running
            .file_name()
            .map_or_else(|| UNNAMEABLE_BINARY.to_string(), |name| name.to_string_lossy().to_string()),
    }
}

/// Per-AC timeout for `command`, env-aware.
///
/// `MUSTARD_QA_AC_TIMEOUT_SECS` (whole seconds, `u64`) overrides BOTH defaults
/// when set to a parseable value; an invalid value is ignored. Without the
/// override, commands containing `cargo ` get [`AC_TIMEOUT_CARGO_SECS`]
/// (compilation-bound) and everything else keeps [`AC_TIMEOUT_SECS`].
fn ac_timeout_secs(command: &str) -> u64 {
    let env = std::env::var("MUSTARD_QA_AC_TIMEOUT_SECS").ok();
    ac_timeout_secs_with_override(command, env.as_deref())
}

/// Deterministic core of [`ac_timeout_secs`]: the env value is injected as a
/// parameter so the decision is a pure function of its inputs (no wall-clock,
/// no globals) and unit-testable without mutating process env (which would
/// need `unsafe` under Rust 2024 — forbidden in this crate).
fn ac_timeout_secs_with_override(command: &str, env_override: Option<&str>) -> u64 {
    if let Some(secs) = env_override.and_then(|s| s.trim().parse::<u64>().ok()) {
        return secs;
    }
    if command.to_ascii_lowercase().contains("cargo ") {
        AC_TIMEOUT_CARGO_SECS
    } else {
        AC_TIMEOUT_SECS
    }
}

/// Locate the spec file. Tries, in order:
///   1. `.claude/specs/{spec}.md` (very-legacy single-file layout)
///   2. `.claude/spec/{spec}/spec.md` (canonical flat layout — single-spec mode)
///   3. `.claude/spec/{spec}/wave-plan.md` (flat layout — wave-plan mode where
///      the global ACs live in `wave-plan.md` and `spec.md` is absent)
///
/// Flat layout is the post-wave-2 contract of
/// `2026-05-21-flatten-spec-layout-and-multi-collab`: there are no
/// `active/` / `completed/` buckets anymore. The spec dir lives at the same
/// path for its entire lifecycle and the canonical status is in the SQLite
/// event store + the `### Status:` header.
pub(super) fn find_spec_file(cwd: &Path, spec: &str) -> Option<PathBuf> {
    let paths = ClaudePaths::for_project(cwd).ok()?;
    // `specs/<spec>.md` is the legacy pre-flat-layout fallback; that directory
    // is not in the documented `ClaudePaths` catalog (post-flat-layout) so
    // build it from the claude_dir root.
    let legacy = paths.claude_dir().join("specs").join(format!("{spec}.md"));
    let sp = paths.for_spec(spec).ok()?;
    let candidates = [legacy, sp.spec_md_path(), sp.wave_plan_md_path()];
    candidates.into_iter().find(|c| c.exists())
}

/// Rewrite a `cargo build/test --workspace` command so the workspace build
/// leaves out the one crate whose output IS the running binary.
///
/// **The catch-22 this solves:** `complete-spec` calls
/// [`run_for_spec_with_options`] which forks shell commands for each AC. When
/// this process is itself running from `target/debug`, an AC like
/// `cargo build --workspace` tries to relink the very executable in the
/// foreground — `Acesso negado. (os error 5)` on Windows.
///
/// The exclusion is appended ONLY when the paths say it is needed: a process
/// running from an installed path is not the file a workspace build writes, so
/// the command goes to the shell verbatim and the criterion is really
/// verified. Idempotent — it won't double-add an exclusion the AC already has.
///
/// This rewrite only covers the `--workspace` form. The DIRECT form
/// (`-p mustard-rt`) has no salvaging rewrite — the command's entire point is
/// rebuilding that crate — so [`run_ac_command`] refuses it outright via
/// [`overwrites_running_binary`], and only when the paths coincide.
fn rewrite_workspace_exclusion(command: &str, target_root: &Path, running: &Path) -> String {
    let lower = command.to_ascii_lowercase();
    if !is_cargo_build_or_test(&lower) || !lower.contains("--workspace") {
        return command.to_string();
    }
    let Some(package) = package_shadowing_running(command, target_root, running) else {
        return command.to_string();
    };
    let needle = package.to_ascii_lowercase();
    if lower.contains(&format!("--exclude {needle}")) || lower.contains(&format!("--exclude={needle}"))
    {
        return command.to_string();
    }
    // Append at the end — `cargo` accepts flags positionally after
    // `--workspace`. Adding to the tail keeps any post-`--` script args
    // (passed to the test binary) untouched.
    format!("{command} --exclude {package}")
}

/// `true` when `command` would overwrite the file THIS process is executing
/// from, resolving both sides itself: the running executable from
/// [`std::env::current_exe`], the build target from the cargo layout under
/// `cwd`.
///
/// `false` when either side cannot be resolved — an unanswerable path question
/// is not evidence of a conflict, and refusing on it would be the crate-name
/// guess this spec removed, wearing a different hat.
///
/// `pub(super)` so the CONFIRMATION pass can ask the SAME question BEFORE it
/// spawns anything: a confirmation taken from inside this binary must answer
/// "not taken here" for such a command, never "inexecutable" — see
/// [`crate::commands::review::ac_negative_check::confirm_in_process`].
pub(super) fn targets_running_binary(command: &str, cwd: &Path) -> bool {
    let (Ok(running), Some(target_root)) = (std::env::current_exe(), cargo_target_root(cwd)) else {
        return false;
    };
    overwrites_running_binary(command, &target_root, &running)
}

/// The verdict of evaluating an AC's optional `Expect:` evidence regex against
/// a passing command's captured output. Pure and panic-free (SRP: no process,
/// no I/O) so the matcher is unit-testable in isolation.
enum ExpectVerdict {
    /// No `Expect:` declared ⇒ the caller keeps the legacy exit-code verdict.
    NoExpectation,
    /// The pattern compiled and matched the output.
    Matched,
    /// The pattern compiled but did NOT match the output.
    Missed,
    /// The pattern is not a valid regex ⇒ fail-open to `skip`, never a panic.
    InvalidPattern,
}

/// Evaluate an optional `Expect:` regex against a command's combined output.
/// Total + pure: an absent expectation is [`ExpectVerdict::NoExpectation`], an
/// uncompilable pattern is [`ExpectVerdict::InvalidPattern`] (never a panic),
/// otherwise match/miss. The regex is compiled here (per-AC, once) — qa-run
/// runs a handful of ACs, so there is no hot loop to cache for.
fn evaluate_expect(expect: Option<&str>, output: &str) -> ExpectVerdict {
    let Some(pattern) = expect else {
        return ExpectVerdict::NoExpectation;
    };
    match regex::Regex::new(pattern) {
        Ok(re) if re.is_match(output) => ExpectVerdict::Matched,
        Ok(_) => ExpectVerdict::Missed,
        Err(_) => ExpectVerdict::InvalidPattern,
    }
}

/// First 100 chars of `s` — the bounded excerpt carried in `stderr_excerpt`.
fn excerpt(s: &str) -> String {
    s.chars().take(100).collect()
}

/// Run one AC command under its resolved per-AC timeout ([`ac_timeout_secs`],
/// computed from the ORIGINAL command so a self-invoked rewrite cannot shift
/// the ceiling).
///
/// Classification: `pass` (exit 0), `fail` (non-zero exit), `timeout` (killed
/// by the deadline), `skip` (the criterion could not be attempted at all).
///
/// `expect` is the AC's optional `Expect:` evidence regex. When present and the
/// command exits 0, the regex must match the command's combined stdout+stderr
/// or the "green" command is downgraded to `fail` (it printed no expected
/// evidence); an uncompilable pattern degrades to `skip` (fail-open). When
/// absent, the exit-code-only verdict is byte-for-byte the historical one.
pub(super) fn run_ac_command(command: &str, expect: Option<&str>, cwd: &Path) -> AcResult {
    let timeout = Duration::from_secs(ac_timeout_secs(command));
    let running = std::env::current_exe().ok();
    run_ac_command_with_timeout(command, expect, cwd, timeout, running.as_deref())
}

/// Deterministic core of [`run_ac_command`]: the deadline AND the running
/// executable are injected as parameters instead of being read from the
/// environment inside, so both the timeout branch and the self-invocation
/// branch are unit-testable without owning the process (env mutation would
/// need `unsafe` under Rust 2024 — forbidden in this crate). Mirrors the same
/// injected-override seam [`ac_timeout_secs_with_override`] already uses.
fn run_ac_command_with_timeout(
    command: &str,
    expect: Option<&str>,
    cwd: &Path,
    timeout: Duration,
    running: Option<&Path>,
) -> AcResult {
    run_ac_command_inner(command, expect, cwd, timeout, running, false)
}

/// Is the first word of `command` a program the shell can find?
///
/// Answers the ONE question that separates "this criterion names a program that
/// does not exist" from "the environment could not run it just now": the former
/// is a real defect and must stay `fail`, the latter is worth one retry.
///
/// `false` whenever the question cannot be answered — an empty command, a shell
/// builtin, an unreadable `PATH`. That direction keeps the historical behaviour
/// (no retry, verdict unchanged), which is the safe one.
fn first_program_is_on_path(command: &str) -> bool {
    let Some(word) = command.split_whitespace().next() else {
        return false;
    };
    // An absolute or relative path is checked directly; a bare name is looked
    // up the way the shell would.
    if word.contains('/') {
        return Path::new(word).is_file();
    }
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| dir.join(word).is_file())
}

/// [`run_ac_command_with_timeout`], with the retry flag the public face hides.
///
/// `is_retry` exists to bound the recursion at exactly one extra attempt: a
/// flaky environment gets a second chance, a genuinely missing program does not
/// spin.
fn run_ac_command_inner(
    command: &str,
    expect: Option<&str>,
    cwd: &Path,
    timeout: Duration,
    running: Option<&Path>,
    is_retry: bool,
) -> AcResult {
    let t0 = Instant::now();
    // Both halves of the path question, resolved once: where cargo would write
    // and what this process is executing from. `None` on either side means the
    // question cannot be answered here, and the command simply runs.
    let paths = cargo_target_root(cwd).zip(running.map(Path::to_path_buf));
    let opts = QA_OPTIONS.with(std::cell::Cell::get);
    // Self-invocation guard for the DIRECT `-p`/`--package` form: no rewrite
    // can save a command whose output file IS this executable (unlike
    // `--workspace`, which gets `--exclude`d below) — refuse immediately
    // instead of burning the timeout on a compile that dies relinking the
    // running exe (os error 5). Only when the two paths really coincide.
    if opts.self_invoked
        && paths
            .as_ref()
            .is_some_and(|(root, exe)| overwrites_running_binary(command, root, exe))
    {
        let label = paths
            .as_ref()
            .map(|(_, exe)| running_binary_label(cwd, exe))
            .unwrap_or_default();
        return AcResult {
            id: String::new(),
            status: "skip".to_string(),
            exit: None,
            duration_ms: t0.elapsed().as_millis(),
            stderr_excerpt: format!(
                "self-invocation: this command overwrites `{label}`, the file this process is \
                 executing from; run this AC externally"
            ),
        };
    }
    // POSIX-style AC commands assume a POSIX shell, and now GET one: the shared
    // runner resolves the shell that ships beside `git` on Windows
    // (`crate::util::platform::build_shell_command`). This used to be a
    // documented constraint on the AC author — "Windows AC are cross-shell-safe
    // (`node -e`, `bash -c`)" — which is not a guarantee, and under `cmd.exe`
    // the single quotes in `rg 'token' path` reached the program as literal
    // characters, so the criterion could never match and never go green.
    // Self-invoked rewrite first — see `rewrite_workspace_exclusion` for why.
    let rewritten = match (opts.self_invoked, paths.as_ref()) {
        (true, Some((root, exe))) => rewrite_workspace_exclusion(command, root, exe),
        _ => command.to_string(),
    };
    // The spawn + concurrent pipe drain + deadline poll live in ONE place
    // ([`crate::shared::proc::run_shell_with_deadline`]), shared with
    // `verify-pipeline`. The drain is load-bearing here too: an AC whose
    // command prints more than the ~64 KB OS pipe buffer used to block writing
    // and burn its whole timeout despite having already finished its work.
    let (status, stdout, stderr) = match run_shell_with_deadline(&rewritten, cwd, timeout) {
        ShellOutcome::Exited { status, stdout, stderr } => (status, stdout, stderr),
        // Killed by its deadline: a class of its own, NEVER `skip`. The
        // criterion WAS attempted and simply never finished, so it verified
        // nothing — `skip` (warn-and-allow) would let the run read green.
        ShellOutcome::TimedOut { after } => {
            return AcResult {
                id: String::new(),
                status: "timeout".to_string(),
                exit: None,
                duration_ms: t0.elapsed().as_millis(),
                stderr_excerpt: format!("timeout after {}ms", after.as_millis()),
            };
        }
        // Never ran ⇒ the criterion could not be attempted at all ⇒ `skip`.
        // Never attempted, or attempted and no status can ever arrive. Carry the
        // OS error instead of asserting "command not found": that reason is a
        // guess, and a criterion reported `skip` for the wrong cause is exactly
        // the misleading verdict this spec exists to remove.
        ShellOutcome::SpawnFailed { error } => {
            return AcResult {
                id: String::new(),
                status: "skip".to_string(),
                exit: None,
                duration_ms: t0.elapsed().as_millis(),
                stderr_excerpt: format!("could not run the command: {error}"),
            };
        }
    };

    let duration_ms = t0.elapsed().as_millis();
    // Full combined output (stderr first, then stdout): the haystack the
    // optional `Expect:` regex matches against AND the source of the bounded
    // excerpt shown on failure.
    let combined_full = [stderr.trim(), stdout.trim()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if status.success() {
        // Optional `Expect:` evidence gate. Absent ⇒ the legacy exit-0 pass
        // (byte-for-byte). Present ⇒ the regex must match the command's own
        // output, else the "green" command proved nothing (fail); an
        // uncompilable pattern degrades to skip, never a panic (fail-open).
        let pattern = expect.unwrap_or_default();
        return match evaluate_expect(expect, &combined_full) {
            ExpectVerdict::NoExpectation | ExpectVerdict::Matched => AcResult {
                id: String::new(),
                status: "pass".to_string(),
                exit: Some(0),
                duration_ms,
                stderr_excerpt: String::new(),
            },
            ExpectVerdict::Missed => AcResult {
                id: String::new(),
                status: "fail".to_string(),
                exit: Some(0),
                duration_ms,
                stderr_excerpt: format!(
                    "Expect `{pattern}` not found in command output: {}",
                    excerpt(&combined_full)
                ),
            },
            ExpectVerdict::InvalidPattern => AcResult {
                id: String::new(),
                status: "skip".to_string(),
                exit: Some(0),
                duration_ms,
                stderr_excerpt: format!(
                    "Expect `{pattern}` is not a valid regex; skipped (fail-open)"
                ),
            },
        };
    }
    // 127 is the POSIX shell's own "command not found". It is NAMED here and
    // still graded `fail`, and both halves of that are deliberate.
    //
    // NAMED, because the bare combined output is not always legible as a cause.
    //
    // Still `fail`, because this function answers for `qa-run` too, and a
    // criterion nobody could run must never let a QA run read green:
    // [`super::overall_verdict`] tolerates a `skip` beside a `pass` on the
    // EXTERNAL path, so grading it `skip` here turned an unrunnable criterion
    // into a passing CLOSE. That regression shipped once; this shape keeps it
    // out.
    //
    // The consumer that DOES need 127 apart is
    // [`crate::commands::review::ac_negative_check`], whose red rule is exit≠0
    // and which would otherwise stamp an unrunnable command `proven: red`. It
    // reads `exit` off this record and decides for itself — the discrimination
    // lives in the ONE caller that needs it, never in the shared status that
    // every caller reads differently.
    // **A 127 whose program DOES exist means "could not run it now", not "does
    // not exist" — and that is worth one retry, never a softer verdict.**
    //
    // Measured four times in one session: a second `cargo` compiling in
    // parallel makes the first lose the build lock, the shell answers 127, and
    // all fourteen criteria fail at once — every one of them passing again less
    // than a minute later. The Stop gate then names a healthy criterion and
    // asks for a fix to something that is not broken.
    //
    // The verdict stays `fail` if the retry also fails. Grading 127 `skip` is
    // the tempting fix and it is the wrong one: it shipped once and let a spec
    // CLOSE on a criterion nobody had ever run (see the note above). One retry
    // separates a flaky environment from a real defect without weakening what
    // the record claims.
    if status.code().map(i64::from) == Some(EXIT_COMMAND_NOT_FOUND)
        && !is_retry
        && first_program_is_on_path(command)
    {
        return run_ac_command_inner(command, expect, cwd, timeout, running, true);
    }
    if status.code().map(i64::from) == Some(EXIT_COMMAND_NOT_FOUND) {
        return AcResult {
            id: String::new(),
            status: "fail".to_string(),
            exit: Some(EXIT_COMMAND_NOT_FOUND),
            duration_ms,
            stderr_excerpt: format!(
                "the shell could not find the command (exit {EXIT_COMMAND_NOT_FOUND}): {}",
                excerpt(&combined_full)
            ),
        };
    }
    AcResult {
        id: String::new(),
        status: "fail".to_string(),
        exit: Some(status.code().map_or(1, i64::from)),
        duration_ms,
        stderr_excerpt: excerpt(&combined_full),
    }
}

/// Emit the `qa.result` harness event.
pub(super) fn emit_qa_event(cwd: &Path, spec: &str, overall: &str, criteria: &[Value]) {
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Cli,
            id: Some("qa-run".to_string()),
            actor_type: None,
        },
        event: "qa.result".to_string(),
        payload: json!({ "spec": spec, "overall": overall, "criteria": criteria }),
        spec: Some(spec.to_string()),
    };
    // `qa.result` is non-pipeline → per-spec NDJSON via the W5 router.
    let _ = crate::shared::events::route::emit(cwd.to_string_lossy().as_ref(), &ev);
}

/// Emit the `qa` metric (fail-silent).
pub(super) fn emit_qa_metric(cwd: &Path, spec: &str, overall: &str, criteria: &[AcResult]) {
    let (mut pass, mut fail, mut skip, mut timeout) = (0, 0, 0, 0);
    for c in criteria {
        match c.status.as_str() {
            "pass" => pass += 1,
            "fail" => fail += 1,
            "skip" => skip += 1,
            "timeout" => timeout += 1,
            _ => {}
        }
    }
    let line = MetricLine::new(now_iso8601(), "qa").note(overall).extras(json!({
        "spec": spec,
        "overall": overall,
        "passCount": pass,
        "failCount": fail,
        "skipCount": skip,
        "timeoutCount": timeout,
        "category": "verification",
    }));
    let _ = emit_metric(cwd, &line);
}

thread_local! {
    /// Active [`QaRunOptions`] for the current thread's qa-run.
    ///
    /// Set by [`run_for_spec_with_options`] and read by
    /// [`run_ac_command_with_timeout`]. A `thread_local!` Cell — not an env
    /// var — because `unsafe_code` is forbidden in this crate and Rust 2024
    /// requires `unsafe` for env mutation, but a Cell-backed `thread_local`
    /// is plain safe Rust.
    pub(super) static QA_OPTIONS: std::cell::Cell<QaRunOptions> = const {
        std::cell::Cell::new(QaRunOptions { self_invoked: false })
    };
}

/// Gather the executable ACs of every capability the spec links in its
/// `## Capabilities` section.
///
/// Reuses the SINGLE `## Capabilities` scanner
/// ([`crate::commands::capability::linked_capability_ids`]) — the same one
/// `complete-spec` uses on close — so qa-run and merge-on-close can never drift
/// on which capabilities a spec links. For each linked `cap.{slug}` whose
/// `.claude/capabilities/{slug}.md` exists, the doc is parsed
/// ([`crate::commands::capability::parse`]) and its command-bearing scenarios are
/// compiled into [`AcceptanceCriterion`]s via the EXISTING
/// [`mustard_core::domain::capability::Capability::acceptance_criteria`] (no
/// parallel AC type). The compiled ids are already stable + namespaced
/// (`cap.{slug}-{scenario}`), so they merge cleanly beside the spec's own AC ids.
///
/// Returns `(id, command)` pairs — exactly the two fields [`run_ac_command`]
/// needs — so the capability ACs run through the SAME execution path as the
/// spec's own. FAIL-OPEN: a linked-but-missing or unreadable / garbage
/// capability doc is skipped (never aborts QA), and a documentary scenario with
/// no command is naturally not compiled.
pub(super) fn gather_capability_acs(cwd: &Path, spec: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let linked = crate::commands::capability::linked_capability_ids(cwd, spec);
    if linked.is_empty() {
        return out; // no `## Capabilities` section (or no `cap.*` links) ⇒ none.
    }
    let Ok(caps_dir) = ClaudePaths::for_project(cwd).map(|p| p.capabilities_dir()) else {
        return out;
    };
    for id in linked {
        // `cap.{slug}` → `{slug}` (the doc file stem). A malformed id with no
        // slug after the prefix is skipped.
        let Some(slug) = id.strip_prefix("cap.").map(str::trim).filter(|s| !s.is_empty())
        else {
            continue;
        };
        let doc_path = caps_dir.join(format!("{slug}.md"));
        // Missing doc ⇒ skip (do NOT invent ACs); fail-open like complete-spec.
        let Ok(md) = fs::read_to_string(&doc_path) else {
            continue;
        };
        let cap = crate::commands::capability::parse(&md);
        for ac in cap.acceptance_criteria() {
            out.push((ac.id, ac.command));
        }
    }
    out
}

#[cfg(test)]
mod tests {

    /// A program that genuinely does not exist still FAILS — no retry softens
    /// it, and no retry spins on it.
    ///
    /// The retry exists only for the other 127: a program the shell CAN find
    /// that could not run just now (a second `cargo` holding the build lock).
    /// Measured four times in one session — fourteen criteria failing together
    /// with 127, all passing again under a minute later.
    #[test]
    fn a_missing_program_still_fails_and_one_that_exists_is_retried() {
        let dir = tempfile::tempdir().unwrap();

        // Not on PATH: no retry, verdict unchanged. Grading this `skip` is the
        // regression documented above — it once let a spec CLOSE on a criterion
        // nobody had run.
        assert!(!first_program_is_on_path("programa-que-nao-existe-xyz --flag"));
        let res = run_ac_command_with_timeout(
            "programa-que-nao-existe-xyz",
            None,
            dir.path(),
            Duration::from_secs(10),
            None,
        );
        assert_eq!(res.status, "fail", "a missing program must stay a failure");
        assert_eq!(res.exit, Some(EXIT_COMMAND_NOT_FOUND));

        // On PATH: the retry is eligible. The program has to be one that really
        // exists on every runner, and `sh` is not — Windows has no `sh`, which
        // is what broke CI there (measured, not guessed). `cargo` is on PATH by
        // construction: these tests only run because it invoked them.
        assert!(first_program_is_on_path("cargo --version"));
        // An absolute path is answered by the filesystem, without consulting
        // PATH at all — so the executable named here must also exist on every
        // platform. `current_exe` is this very test binary.
        if let Ok(me) = std::env::current_exe() {
            assert!(first_program_is_on_path(&me.to_string_lossy()));
        }
        assert!(!first_program_is_on_path("/nao/existe/em/lugar/nenhum"));
        assert!(!first_program_is_on_path(""));
    }

    use super::*;
    use tempfile::tempdir;

    /// Wave-plans keep their global ACs in `wave-plan.md` (no `spec.md` at the
    /// root). `find_spec_file` must fall back to `wave-plan.md` so qa-run
    /// closes wave-plans end-to-end without the operator copying/renaming.
    #[test]
    fn finds_wave_plan_md_when_spec_md_absent() {
        let dir = tempdir().unwrap();
        let spec_dir = ClaudePaths::for_project(dir.path()).unwrap().for_spec("plan-a").unwrap().dir().to_path_buf();
        std::fs::create_dir_all(&spec_dir).unwrap();
        let wp = spec_dir.join("wave-plan.md");
        std::fs::write(&wp, "# Plan A\n## Acceptance Criteria\n- [ ] AC-G1: ok — Command: `true`\n").unwrap();
        let found = find_spec_file(dir.path(), "plan-a").unwrap();
        assert_eq!(found, wp);
    }

    /// When both `spec.md` and `wave-plan.md` exist in the same dir, the
    /// `spec.md` path wins — preserves the single-spec contract for the rare
    /// case where an operator authored both (e.g. legacy migrations).
    #[test]
    fn spec_md_wins_over_wave_plan_md_when_both_exist() {
        let dir = tempdir().unwrap();
        let spec_dir = ClaudePaths::for_project(dir.path()).unwrap().for_spec("plan-b").unwrap().dir().to_path_buf();
        std::fs::create_dir_all(&spec_dir).unwrap();
        let sp = spec_dir.join("spec.md");
        let wp = spec_dir.join("wave-plan.md");
        std::fs::write(&sp, "# Spec B\n## Acceptance Criteria\n- [ ] AC-1: x — Command: `true`\n").unwrap();
        std::fs::write(&wp, "# Plan B\n## Acceptance Criteria\n- [ ] AC-G1: y — Command: `true`\n").unwrap();
        let found = find_spec_file(dir.path(), "plan-b").unwrap();
        assert_eq!(found, sp);
    }

    /// An AC-style command with quotes AND parentheses must survive intact to
    /// the shell. Under the old `cmd.arg("/C").arg(command)` path, `std`'s
    /// `CommandLineToArgvW`-style quoting corrupts the line (`node` sees a
    /// split string → "Unterminated string constant"); the `raw_arg`-based
    /// `build_shell_command` passes it verbatim, so this exits 0.
    #[cfg(windows)]
    #[test]
    fn ac_command_with_quotes_and_parens_runs_verbatim() {
        let dir = tempdir().unwrap();
        // node one-liner: a regex test inside parentheses, double-quoted -e arg.
        let cmd = r#"node -e "process.exit(/^(foo|bar)$/.test('bar') ? 0 : 1)""#;
        let res = run_ac_command(cmd, None, dir.path());
        assert_eq!(
            res.status, "pass",
            "quoted+parenthesized AC command must run verbatim (exit {:?}, stderr: {})",
            res.exit, res.stderr_excerpt
        );
        assert_eq!(res.exit, Some(0));
    }

    /// A `cmd.exe`-native command echoing a parenthesized, quoted string — the
    /// simplest case proving the outer quote pair is stripped and the inner
    /// `()` reach the program unmangled.
    #[cfg(windows)]
    #[test]
    fn ac_command_echoes_parenthesized_string() {
        let dir = tempdir().unwrap();
        let cmd = r#"node -e "console.log('(ok)')""#;
        let res = run_ac_command(cmd, None, dir.path());
        assert_eq!(res.status, "pass", "stderr: {}", res.stderr_excerpt);
        assert_eq!(res.exit, Some(0));
    }

    /// THE regression this fix exists for, in both senses. A criterion written
    /// POSIX-style — the shape the flow's own prose teaches — must be able to go
    /// GREEN when its evidence is there and RED when it is not. Under `cmd.exe`
    /// the apostrophes reached `echo` as literal characters, so the output was
    /// `'a b'`, the `Expect:` never matched, and BOTH senses came back red: a
    /// criterion that could not pass in any tree state, stamped `proven: red` by
    /// the negative test and blamed on the implementer at QA.
    #[test]
    fn an_ac_written_with_single_quotes_can_go_both_ways() {
        let dir = tempdir().unwrap();
        let green = run_ac_command("echo 'a b'", Some("^a b$"), dir.path());
        assert_eq!(
            green.status, "pass",
            "single-quoted AC must be able to pass, stderr: {}",
            green.stderr_excerpt
        );
        let red = run_ac_command("echo 'a b'", Some("^zzz$"), dir.path());
        assert_eq!(
            red.status, "fail",
            "and must still fail on absent evidence, stderr: {}",
            red.stderr_excerpt
        );
    }

    /// A command the shell cannot find is graded `fail`, and NAMED.
    ///
    /// It briefly shipped as `skip`, to keep `ac_negative_check` from reading it
    /// as red proof. That fixed one consumer and broke the other: `qa-run`
    /// tolerates a `skip` beside a `pass` on the external path, so a criterion
    /// whose program did not exist stopped blocking CLOSE and rode along as a
    /// pass. The discrimination moved to the caller that needs it — the exit
    /// code is what carries it — and the verdict here went back to `fail`.
    #[test]
    fn a_command_the_shell_cannot_find_fails_and_names_the_cause() {
        let dir = tempdir().unwrap();
        let res = run_ac_command("mustard-no-such-program-9f3c --version", None, dir.path());
        assert_eq!(
            res.status, "fail",
            "an unrunnable criterion must still block QA, stderr: {}",
            res.stderr_excerpt
        );
        assert_eq!(res.exit, Some(EXIT_COMMAND_NOT_FOUND), "the shell's own not-found code");
        assert!(
            res.stderr_excerpt.contains("could not find the command"),
            "the cause is named, not left to the raw output: {}",
            res.stderr_excerpt
        );
    }

    /// Commands invoking `cargo ` get the compile-aware ceiling (600 s): a
    /// build/test AC may need a full recompile, and the 120 s default turned
    /// such ACs into silent skips (the regression behind this fix).
    #[test]
    fn qa_timeout_cargo_command_gets_big_ceiling() {
        assert_eq!(
            ac_timeout_secs_with_override("cargo test -p mustard-rt", None),
            AC_TIMEOUT_CARGO_SECS
        );
        assert_eq!(
            ac_timeout_secs_with_override("cargo build --workspace", None),
            AC_TIMEOUT_CARGO_SECS
        );
        // Wrapped/chained invocations still contain `cargo ` → big ceiling.
        assert_eq!(
            ac_timeout_secs_with_override("rtk cargo test && echo ok", None),
            AC_TIMEOUT_CARGO_SECS
        );
    }

    /// Non-cargo commands keep the historical 120 s default.
    #[test]
    fn qa_timeout_non_cargo_keeps_default() {
        assert_eq!(
            ac_timeout_secs_with_override(r#"node -e "process.exit(0)""#, None),
            AC_TIMEOUT_SECS
        );
        assert_eq!(
            ac_timeout_secs_with_override("grep -q Modelo SKILL.md", None),
            AC_TIMEOUT_SECS
        );
    }

    /// `MUSTARD_QA_AC_TIMEOUT_SECS` overrides BOTH defaults when it parses as
    /// `u64`; an invalid value is ignored and the command-sensitive default
    /// applies. Exercised through the injected-override core (env mutation
    /// needs `unsafe` under Rust 2024, forbidden in this crate).
    #[test]
    fn qa_timeout_env_override_wins() {
        assert_eq!(
            ac_timeout_secs_with_override("cargo test -p mustard-rt", Some("300")),
            300
        );
        assert_eq!(ac_timeout_secs_with_override("echo ok", Some("300")), 300);
        // Surrounding whitespace is tolerated.
        assert_eq!(ac_timeout_secs_with_override("cargo build", Some(" 42 ")), 42);
        // Invalid values fall back to the command-sensitive defaults.
        assert_eq!(
            ac_timeout_secs_with_override("cargo build", Some("not-a-number")),
            AC_TIMEOUT_CARGO_SECS
        );
        assert_eq!(ac_timeout_secs_with_override("echo ok", Some("")), AC_TIMEOUT_SECS);
    }

    /// The file name cargo writes for `package` in `profile` — spelled once so
    /// the tests below read the same on Windows (`.exe`) and Unix.
    fn built(target_root: &Path, profile: &str, package: &str) -> PathBuf {
        target_root
            .join(profile)
            .join(format!("{package}{}", std::env::consts::EXE_SUFFIX))
    }

    /// AC-1 — the guard's question is about PATHS, not spelling. A command that
    /// rebuilds this crate while writing to a file OTHER than the one this
    /// process executes from is RUN, not refused: that is the shipped shape
    /// (installed binary, workspace `target/`), and refusing it by crate name
    /// denied a whole QA run over criteria that would have passed.
    #[test]
    fn guard_allows_when_build_target_is_not_the_running_binary() {
        let target_root = PathBuf::from("no-such-workspace").join("target");
        let installed = PathBuf::from("no-such-home")
            .join(".cargo")
            .join("bin")
            .join(format!("mustard-rt{}", std::env::consts::EXE_SUFFIX));
        // The shipped shape: two different files, so there is no conflict to
        // protect against — both spellings of the direct form simply run.
        assert!(!overwrites_running_binary("cargo test -p mustard-rt", &target_root, &installed));
        assert!(!overwrites_running_binary(
            "cargo build --package=mustard-rt",
            &target_root,
            &installed
        ));

        let debug_exe = built(&target_root, "debug", "mustard-rt");
        // Same directory tree, different profile: `--release` writes elsewhere.
        assert!(!overwrites_running_binary(
            "cargo build -p mustard-rt --release",
            &target_root,
            &debug_exe
        ));
        // Another package's output is another file.
        assert!(!overwrites_running_binary("cargo test -p mustard-core", &target_root, &debug_exe));
        // Token boundary: a prefix-sharing name is a different package.
        assert!(!overwrites_running_binary(
            "cargo test -p mustard-rt-extras",
            &target_root,
            &debug_exe
        ));
        // Only `build`/`test` relink a crate's binary.
        assert!(!overwrites_running_binary("cargo fmt -p mustard-rt", &target_root, &debug_exe));
        assert!(!overwrites_running_binary("echo -p mustard-rt", &target_root, &debug_exe));
        // The `--workspace` form is salvaged by an exclusion, never refused.
        assert!(!overwrites_running_binary("cargo test --workspace", &target_root, &debug_exe));
    }

    /// AC-2 — and the refusal STANDS when the two paths coincide: a harness
    /// started from its own build directory really would be overwritten. The
    /// reason names that file, relative to the project, so the committed QA
    /// report says `target/debug/mustard-rt` and not a path off one machine.
    #[test]
    fn guard_refuses_when_build_target_is_the_running_binary() {
        let project = PathBuf::from("no-such-workspace");
        let target_root = project.join("target");
        let running = built(&target_root, "debug", "mustard-rt");
        for command in [
            "cargo test -p mustard-rt qa_run",
            "cargo test -p=mustard-rt -- --nocapture",
            "cargo build --package mustard-rt",
            "cargo build --package=mustard-rt",
        ] {
            assert!(
                overwrites_running_binary(command, &target_root, &running),
                "must refuse: {command}"
            );
        }
        // A release build refuses too, when THAT is where the process runs from.
        let released = built(&target_root, "release", "mustard-rt");
        assert!(overwrites_running_binary(
            "cargo build -p mustard-rt --release",
            &target_root,
            &released
        ));
        assert_eq!(
            running_binary_label(&project, &running),
            format!("target/debug/mustard-rt{}", std::env::consts::EXE_SUFFIX)
        );
    }

    /// The refusal is really wired into the executor: with `self_invoked` set
    /// and the running binary sitting in the directory the command builds into,
    /// the AC is skipped without spawning and the reason names the file.
    #[test]
    fn qa_self_invoked_refusal_names_the_file_it_protects() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        let target_root = cargo_target_root(dir.path()).expect("a Cargo.toml makes a target root");
        let running = built(&target_root, "debug", "mustard-rt");
        QA_OPTIONS.with(|cell| cell.set(QaRunOptions { self_invoked: true }));
        let res = run_ac_command_with_timeout(
            "cargo test -p mustard-rt qa_run",
            None,
            dir.path(),
            Duration::from_secs(60),
            Some(&running),
        );
        QA_OPTIONS.with(|cell| cell.set(QaRunOptions::default()));
        assert_eq!(res.status, "skip");
        assert_eq!(res.exit, None);
        assert!(res.stderr_excerpt.contains("mustard-rt"), "{}", res.stderr_excerpt);
        assert!(
            res.stderr_excerpt.contains("the file this process is executing from"),
            "the reason names the file, not the crate: {}",
            res.stderr_excerpt
        );
    }

    /// The other side of the same wiring, taking the branch it names: the path
    /// question is ANSWERABLE here (the tempdir carries a `Cargo.toml`, so a
    /// target root resolves) and the answer is "different files" — the process
    /// runs from an installed-style path beside the target dir, not inside it.
    /// So nothing is refused and cargo really spawns.
    ///
    /// The earlier version of this test used a tempdir with no `Cargo.toml`,
    /// which made `cargo_target_root` return `None`: the guard short-circuited
    /// on the UNANSWERABLE branch and the path comparison was never exercised.
    #[test]
    fn qa_self_invoked_runs_the_command_when_the_paths_differ() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        let target_root = cargo_target_root(dir.path()).expect("a Cargo.toml makes a target root");
        let elsewhere = dir
            .path()
            .join("installed")
            .join(format!("mustard-rt{}", std::env::consts::EXE_SUFFIX));
        // The branch under test, named directly: the target root resolved, and
        // the file the build writes is NOT the file this process runs from.
        assert!(!overwrites_running_binary(
            "cargo test -p mustard-rt --offline",
            &target_root,
            &elsewhere
        ));
        QA_OPTIONS.with(|cell| cell.set(QaRunOptions { self_invoked: true }));
        let res = run_ac_command_with_timeout(
            "cargo test -p mustard-rt --offline",
            None,
            dir.path(),
            Duration::from_secs(120),
            Some(&elsewhere),
        );
        QA_OPTIONS.with(|cell| cell.set(QaRunOptions::default()));
        assert_eq!(res.status, "fail", "stderr: {}", res.stderr_excerpt);
        assert!(res.exit.is_some(), "the command really spawned: {}", res.stderr_excerpt);
        assert!(
            !res.stderr_excerpt.contains("self-invocation"),
            "nothing may be refused when the paths differ: {}",
            res.stderr_excerpt
        );
    }

    /// The label never leaks a machine path. When the running binary lives
    /// OUTSIDE the project — reachable with an absolute `CARGO_TARGET_DIR`,
    /// where a refusal is still possible — the reason falls back to the bare
    /// file name instead of interpolating an absolute path into `qa/report.md`
    /// and `ac-proof.json`, which are versioned.
    #[test]
    fn running_binary_label_never_leaks_an_absolute_path() {
        let project = PathBuf::from("no-such-workspace");
        let outside = if cfg!(windows) {
            PathBuf::from(r"D:\somewhere\else\target\debug")
        } else {
            PathBuf::from("/somewhere/else/target/debug")
        }
        .join(format!("mustard-rt{}", std::env::consts::EXE_SUFFIX));
        let label = running_binary_label(&project, &outside);
        assert_eq!(label, format!("mustard-rt{}", std::env::consts::EXE_SUFFIX));
        assert!(!label.contains("somewhere"), "no machine path may survive: {label}");
        // And a path with no file name at all still reads as a sentence.
        assert_eq!(running_binary_label(&project, Path::new("/")), UNNAMEABLE_BINARY);
    }

    /// With `self_invoked=false` (external invocation) the command runs
    /// untouched: cargo actually spawns and fails fast in the empty tempdir
    /// ("could not find Cargo.toml") → `fail`, proving no skip short-circuit
    /// and no rewrite fired.
    #[test]
    fn qa_self_invoked_false_runs_command_untouched() {
        let dir = tempdir().unwrap();
        // Thread-local default: self_invoked = false.
        let res = run_ac_command("cargo test -p mustard-rt --offline", None, dir.path());
        assert_eq!(res.status, "fail", "stderr: {}", res.stderr_excerpt);
        assert!(
            res.stderr_excerpt.contains("Cargo.toml"),
            "cargo must have actually run: {}",
            res.stderr_excerpt
        );
    }

    /// The `--workspace` salvage asks the SAME path question: the exclusion is
    /// appended only when the workspace build would overwrite the running
    /// binary. A process running from an installed path gets the command
    /// verbatim — which is how a `cargo build --workspace` criterion gets
    /// genuinely verified instead of quietly not building this crate.
    #[test]
    fn qa_workspace_rewrite_excludes_only_the_running_binary() {
        let target_root = PathBuf::from("no-such-workspace").join("target");
        let running = built(&target_root, "debug", "mustard-rt");
        assert_eq!(
            rewrite_workspace_exclusion("cargo test --workspace", &target_root, &running),
            "cargo test --workspace --exclude mustard-rt"
        );
        // Idempotent: an AC that already excludes it is left alone.
        assert_eq!(
            rewrite_workspace_exclusion(
                "cargo test --workspace --exclude mustard-rt",
                &target_root,
                &running
            ),
            "cargo test --workspace --exclude mustard-rt"
        );
        // The rewrite never touches non-workspace forms (the direct form is
        // handled upstream by the refusal, not by rewriting).
        assert_eq!(
            rewrite_workspace_exclusion("cargo test -p mustard-core", &target_root, &running),
            "cargo test -p mustard-core"
        );
        // Running from an installed path: nothing to exclude, the workspace
        // build really builds every crate.
        let installed = PathBuf::from("no-such-home")
            .join(".cargo")
            .join("bin")
            .join(format!("mustard-rt{}", std::env::consts::EXE_SUFFIX));
        assert_eq!(
            rewrite_workspace_exclusion("cargo test --workspace", &target_root, &installed),
            "cargo test --workspace"
        );
    }

    /// The pure `Expect:` matcher: absent ⇒ NoExpectation, a compiling pattern
    /// that matches ⇒ Matched, that misses ⇒ Missed, an uncompilable pattern ⇒
    /// InvalidPattern (never a panic). This is the SRP surface the exit-0 gate
    /// in `run_ac_command` delegates to.
    #[test]
    fn expect_regex_matcher_verdicts() {
        assert!(matches!(evaluate_expect(None, "anything"), ExpectVerdict::NoExpectation));
        assert!(matches!(
            evaluate_expect(Some("test result: ok"), "running 3 tests\ntest result: ok. 3 passed"),
            ExpectVerdict::Matched
        ));
        assert!(matches!(
            evaluate_expect(Some("0 passed"), "test result: ok. 3 passed"),
            ExpectVerdict::Missed
        ));
        // Unclosed character class ⇒ not a valid regex ⇒ fail-open, no panic.
        assert!(matches!(evaluate_expect(Some("[unterminated"), "x"), ExpectVerdict::InvalidPattern));
    }

    /// End-to-end: an exit-0 command whose output MATCHES the `Expect:` regex
    /// passes. `echo` is a builtin in both `cmd.exe` and `sh`, so this is
    /// cross-platform.
    #[test]
    fn expect_regex_exit0_match_passes() {
        let dir = tempdir().unwrap();
        let res = run_ac_command("echo evidence-token", Some("evidence-token"), dir.path());
        assert_eq!(res.status, "pass", "stderr: {}", res.stderr_excerpt);
        assert_eq!(res.exit, Some(0));
        assert!(res.stderr_excerpt.is_empty());
    }

    /// End-to-end: an exit-0 command whose output does NOT match the `Expect:`
    /// regex is downgraded to `fail` — a green command that printed no expected
    /// evidence proved nothing. The excerpt names the pattern and the output.
    #[test]
    fn expect_regex_exit0_no_match_fails() {
        let dir = tempdir().unwrap();
        let res = run_ac_command("echo evidence-token", Some("MISSING-TOKEN"), dir.path());
        assert_eq!(res.status, "fail", "stderr: {}", res.stderr_excerpt);
        // The command genuinely exited 0; the fail is the evidence gate.
        assert_eq!(res.exit, Some(0));
        assert!(res.stderr_excerpt.contains("MISSING-TOKEN"), "names the pattern: {}", res.stderr_excerpt);
        assert!(res.stderr_excerpt.contains("evidence-token"), "shows the output excerpt: {}", res.stderr_excerpt);
    }

    /// End-to-end: an exit-0 command with NO `Expect:` keeps the legacy pass,
    /// byte-for-byte (empty excerpt) — the unchanged-behaviour guarantee.
    #[test]
    fn expect_regex_absent_keeps_legacy_pass() {
        let dir = tempdir().unwrap();
        let res = run_ac_command("echo whatever", None, dir.path());
        assert_eq!(res.status, "pass", "stderr: {}", res.stderr_excerpt);
        assert_eq!(res.exit, Some(0));
        assert!(res.stderr_excerpt.is_empty(), "legacy pass carries an empty excerpt");
    }

    /// A shell command that prints ~90 KB — far past the ~64 KB OS pipe buffer
    /// — and then exits 3. The POSIX form is what runs whenever a POSIX shell
    /// was resolved, which on Windows is now the normal case; the `cmd.exe`
    /// form is kept for the fallback so the test still drives the REAL shell on
    /// a machine carrying no `git` install beside one.
    const BIG_OUTPUT_EXIT_3_POSIX: &str = "s=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAA; i=0; \
         while [ $i -lt 12 ]; do s=\"$s$s\"; i=$((i+1)); done; echo \"$s\"; exit 3";
    #[cfg(windows)]
    const BIG_OUTPUT_EXIT_3_CMD: &str =
        "(for /L %i in (1,1,3000) do @echo AAAAAAAAAAAAAAAAAAAAAAAAAAAAAA) & exit 3";

    /// The form matching the shell this process will actually spawn.
    fn big_output_exit_3() -> &'static str {
        #[cfg(windows)]
        if crate::util::platform::posix_shell().is_none() {
            return BIG_OUTPUT_EXIT_3_CMD;
        }
        BIG_OUTPUT_EXIT_3_POSIX
    }

    /// A command that stays alive ~3 s, so a 1 s deadline always fires first.
    #[cfg(windows)]
    const SLEEPS_SECONDS: &str = "ping -n 4 127.0.0.1";
    #[cfg(not(windows))]
    const SLEEPS_SECONDS: &str = "sleep 3";

    /// THE regression that must never come back: an AC printing far more than
    /// the OS pipe buffer still completes and is judged by its EXIT CODE. Under
    /// the old `wait_with_output`-after-`try_wait` shape the child blocked
    /// writing into a full pipe, never exited, and the AC degraded to a bogus
    /// timeout — a green-looking `skip` for a criterion that had already failed.
    #[test]
    fn ac_command_past_the_pipe_buffer_is_judged_by_exit_code() {
        let dir = tempdir().unwrap();
        let res = run_ac_command(big_output_exit_3(), None, dir.path());
        assert_eq!(
            res.status, "fail",
            "verdict comes from the exit code, stderr: {}",
            res.stderr_excerpt
        );
        assert_eq!(res.exit, Some(3), "the command's own code, not a timeout");
    }

    /// An AC killed by its deadline reports `timeout` — its OWN class, never
    /// `skip`. `skip` keeps meaning "could not be attempted at all"; a timed-out
    /// AC WAS attempted and simply verified nothing.
    #[test]
    fn ac_command_killed_by_deadline_reports_timeout_not_skip() {
        let dir = tempdir().unwrap();
        let res = run_ac_command_with_timeout(
            SLEEPS_SECONDS,
            None,
            dir.path(),
            Duration::from_secs(1),
            None,
        );
        assert_eq!(res.status, "timeout", "stderr: {}", res.stderr_excerpt);
        assert_eq!(res.exit, None, "a killed command has no exit code");
        assert_eq!(res.stderr_excerpt, "timeout after 1000ms");
    }

    /// End-to-end: an exit-0 command whose `Expect:` is an INVALID regex skips
    /// (fail-open) with a reason — never a panic, never a false pass/fail.
    #[test]
    fn expect_regex_invalid_pattern_skips() {
        let dir = tempdir().unwrap();
        let res = run_ac_command("echo whatever", Some("[unterminated"), dir.path());
        assert_eq!(res.status, "skip", "stderr: {}", res.stderr_excerpt);
        assert!(
            res.stderr_excerpt.contains("not a valid regex"),
            "skip reason states the invalid pattern: {}",
            res.stderr_excerpt
        );
    }
}
