//! qa-run acceptance-criteria execution engine: locate the spec file, run each
//! AC command (with per-AC timeouts and self-invocation guards), and emit the
//! `qa.result` event and metric. Split out of `qa_run` (F3 PERF-D).

use crate::shared::context::session_id;
use crate::util::platform;
use mustard_core::io::fs;
use mustard_core::ClaudePaths;
use mustard_core::time::now_iso8601;
use mustard_core::platform::metrics::{emit_metric, MetricLine};
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Instant;
use super::{AcResult, QaRunOptions};

/// Default per-AC timeout (2 min) for non-cargo commands, matching
/// `AC_TIMEOUT_MS` in `qa-run.js`.
const AC_TIMEOUT_SECS: u64 = 120;

/// Per-AC timeout ceiling (10 min) for commands invoking `cargo `: a
/// `cargo build`/`cargo test` AC that runs right after an edit must recompile,
/// and a cold compile routinely exceeds the 120 s default (real case:
/// `cargo test -p mustard-rt` hit 120 s mid-recompile and degraded to a
/// silent `skip`). Mirrors `TIMEOUT_RUST_SECS` in `verify-pipeline`.
const AC_TIMEOUT_CARGO_SECS: u64 = 600;

/// Crates whose binaries can be the very process running qa-run. A
/// self-invoked qa-run must never let an AC rebuild them — see
/// [`rewrite_self_invoked_cargo`] (the `--workspace` form, rewritten) and
/// [`targets_running_crate`] (the direct `-p`/`--package` form, skipped).
const SELF_CRATES: [&str; 2] = ["mustard-rt", "mustard-dashboard"];

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

/// Rewrite a `cargo build/test --workspace` command to skip the crate(s) in
/// execution when qa-run is invoked from inside `complete-spec`.
///
/// **The catch-22 this solves:** `complete-spec` calls
/// [`run_for_spec_with_options`] which forks shell commands for each AC. An
/// AC like `cargo build --workspace` then tries to relink the very
/// `mustard-rt.exe` that is currently the foreground process —
/// `Acesso negado. (os error 5)` on Windows. Same story when `dashboard.exe`
/// is held by a user testing the UI.
///
/// Gated by [`QaRunOptions::self_invoked`] (stored in the [`QA_OPTIONS`]
/// thread-local). When `false`, the rewrite is a no-op — external
/// `mustard-rt run qa-run` invocations from CI / standalone shells see the
/// original command untouched.
///
/// When `true`, every `cargo (build|test) ... --workspace ...` token sequence
/// gets `--exclude mustard-rt --exclude mustard-dashboard` appended.
/// Idempotent: won't double-add if the AC already excluded them.
///
/// This rewrite only covers the `--workspace` form. The DIRECT form
/// (`-p mustard-rt` / `--package mustard-rt`) has no salvaging rewrite — the
/// command's entire point is rebuilding the running binary — so
/// [`run_ac_command`] skips it outright via [`targets_running_crate`].
fn rewrite_self_invoked_cargo(command: &str) -> String {
    let opts = QA_OPTIONS.with(std::cell::Cell::get);
    if !opts.self_invoked {
        return command.to_string();
    }
    // Cheap detection: token sequence `cargo (build|test) ... --workspace`.
    let lower = command.to_ascii_lowercase();
    if !(lower.contains("cargo build") || lower.contains("cargo test")) {
        return command.to_string();
    }
    if !lower.contains("--workspace") {
        return command.to_string();
    }
    let mut out = command.to_string();
    for crate_name in SELF_CRATES {
        let needle_explicit = format!("--exclude {crate_name}");
        let needle_eq = format!("--exclude={crate_name}");
        if out.contains(&needle_explicit) || out.contains(&needle_eq) {
            continue;
        }
        // Append at the end — `cargo` accepts flags positionally after
        // `--workspace`. Adding to the tail keeps any post-`--` script args
        // (passed to the test binary) untouched.
        out.push_str(" --exclude ");
        out.push_str(crate_name);
    }
    out
}

/// `true` when `command` is a `cargo build`/`cargo test` invocation that
/// targets one of [`SELF_CRATES`] DIRECTLY via `-p`/`--package` — both the
/// split (`-p mustard-rt`) and glued (`-p=mustard-rt`) spellings, matched on
/// token boundaries so `-p mustard-rt-extras` does NOT match.
///
/// Companion to [`rewrite_self_invoked_cargo`]: the `--workspace` form can be
/// salvaged by appending `--exclude`, but the direct form cannot — executed
/// from inside the very binary it rebuilds, the link step hits
/// `Acesso negado. (os error 5)` on Windows. [`run_ac_command`] uses this to
/// skip such ACs immediately (when [`QaRunOptions::self_invoked`] is set)
/// instead of burning the whole timeout on a doomed compile.
fn targets_running_crate(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    if !(lower.contains("cargo build") || lower.contains("cargo test")) {
        return false;
    }
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    tokens.iter().enumerate().any(|(i, tok)| {
        SELF_CRATES.iter().any(|crate_name| {
            let split_form = (*tok == "-p" || *tok == "--package")
                && tokens.get(i + 1).is_some_and(|next| next == crate_name);
            let glued_form = tok
                .strip_prefix("-p=")
                .or_else(|| tok.strip_prefix("--package="))
                .is_some_and(|value| value == *crate_name);
            split_form || glued_form
        })
    })
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

/// Run one AC command. Mirrors the JS classification: `pass` (exit 0), `fail`
/// (non-zero exit), `skip` (timeout or spawn failure).
///
/// `expect` is the AC's optional `Expect:` evidence regex. When present and the
/// command exits 0, the regex must match the command's combined stdout+stderr
/// or the "green" command is downgraded to `fail` (it printed no expected
/// evidence); an uncompilable pattern degrades to `skip` (fail-open). When
/// absent, the exit-code-only verdict is byte-for-byte the historical one.
pub(super) fn run_ac_command(command: &str, expect: Option<&str>, cwd: &Path) -> AcResult {
    let t0 = Instant::now();
    // Self-invocation guard for the DIRECT `-p`/`--package` form: no rewrite
    // can save this command (unlike `--workspace`, which gets `--exclude`d in
    // `rewrite_self_invoked_cargo`) — skip immediately instead of burning the
    // timeout on a compile that dies relinking the running exe (os error 5).
    let opts = QA_OPTIONS.with(std::cell::Cell::get);
    if opts.self_invoked && targets_running_crate(command) {
        return AcResult {
            id: String::new(),
            status: "skip".to_string(),
            exit: None,
            duration_ms: t0.elapsed().as_millis(),
            stderr_excerpt:
                "self-invocation: cannot rebuild the running binary; run this AC externally"
                    .to_string(),
        };
    }
    // POSIX-style AC commands assume a shell; use the platform shell. Windows
    // AC are documented to be cross-shell-safe (`node -e`, `bash -c`).
    // Self-invoked rewrite first — see `rewrite_self_invoked_cargo` for why.
    let rewritten = rewrite_self_invoked_cargo(command);
    let mut cmd = platform::build_shell_command(&rewritten);
    cmd.current_dir(cwd);

    // No native wait-with-timeout in std; spawn + poll.
    let child = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();
    let Ok(mut child) = child else {
        return AcResult {
            id: String::new(),
            status: "skip".to_string(),
            exit: None,
            duration_ms: t0.elapsed().as_millis(),
            stderr_excerpt: "command not found".to_string(),
        };
    };

    let timeout_secs = ac_timeout_secs(command);
    let deadline = Instant::now() + std::time::Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = child.wait_with_output().ok();
                let (stderr, stdout) = out
                    .map(|o| {
                        (
                            String::from_utf8_lossy(&o.stderr).trim().to_string(),
                            String::from_utf8_lossy(&o.stdout).trim().to_string(),
                        )
                    })
                    .unwrap_or_default();
                let duration_ms = t0.elapsed().as_millis();
                // Full combined output (stderr first, then stdout): the haystack
                // the optional `Expect:` regex matches against AND the source of
                // the bounded excerpt shown on failure.
                let combined_full = [stderr, stdout]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                if status.success() {
                    // Optional `Expect:` evidence gate. Absent ⇒ the legacy
                    // exit-0 pass (byte-for-byte). Present ⇒ the regex must
                    // match the command's own output, else the "green" command
                    // proved nothing (fail); an uncompilable pattern degrades to
                    // skip, never a panic (fail-open).
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
                return AcResult {
                    id: String::new(),
                    status: "fail".to_string(),
                    exit: Some(status.code().map_or(1, i64::from)),
                    duration_ms,
                    stderr_excerpt: excerpt(&combined_full),
                };
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return AcResult {
                        id: String::new(),
                        status: "skip".to_string(),
                        exit: None,
                        duration_ms: t0.elapsed().as_millis(),
                        stderr_excerpt: format!("timeout after {}ms", timeout_secs * 1000),
                    };
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(_) => {
                return AcResult {
                    id: String::new(),
                    status: "skip".to_string(),
                    exit: None,
                    duration_ms: t0.elapsed().as_millis(),
                    stderr_excerpt: "wait failed".to_string(),
                };
            }
        }
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
    let (mut pass, mut fail, mut skip) = (0, 0, 0);
    for c in criteria {
        match c.status.as_str() {
            "pass" => pass += 1,
            "fail" => fail += 1,
            "skip" => skip += 1,
            _ => {}
        }
    }
    let line = MetricLine::new(now_iso8601(), "qa").note(overall).extras(json!({
        "spec": spec,
        "overall": overall,
        "passCount": pass,
        "failCount": fail,
        "skipCount": skip,
        "category": "verification",
    }));
    let _ = emit_metric(cwd, &line);
}

thread_local! {
    /// Active [`QaRunOptions`] for the current thread's qa-run.
    ///
    /// Set by [`run_for_spec_with_options`] and read by
    /// [`rewrite_self_invoked_cargo`]. A `thread_local!` Cell — not an env
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

    /// With `self_invoked=true`, a direct `-p` cargo test on a self crate is
    /// an IMMEDIATE skip with the explicit reason — it never spawns. (A
    /// pass-through in this empty tempdir would have spawned cargo and come
    /// back as `fail`, not `skip`: there is no Cargo.toml here.)
    #[test]
    fn qa_self_invoked_direct_p_self_crate_skips_immediately() {
        let dir = tempdir().unwrap();
        QA_OPTIONS.with(|cell| cell.set(QaRunOptions { self_invoked: true }));
        let res = run_ac_command("cargo test -p mustard-rt qa_run", None, dir.path());
        QA_OPTIONS.with(|cell| cell.set(QaRunOptions::default()));
        assert_eq!(res.status, "skip");
        assert_eq!(res.exit, None);
        assert_eq!(
            res.stderr_excerpt,
            "self-invocation: cannot rebuild the running binary; run this AC externally"
        );
    }

    /// Token-boundary detection across the accepted spellings, plus the
    /// negatives that must NOT match: other crates, prefix-sharing crate
    /// names, non-build/test cargo subcommands, non-cargo commands.
    #[test]
    fn qa_self_invoked_detection_token_boundaries() {
        // Split and glued spellings, both self crates.
        assert!(targets_running_crate("cargo test -p mustard-rt"));
        assert!(targets_running_crate("cargo test -p=mustard-rt -- --nocapture"));
        assert!(targets_running_crate("cargo build --package mustard-dashboard"));
        assert!(targets_running_crate("cargo build --package=mustard-dashboard --release"));
        // Token boundary: prefix-sharing names must not match.
        assert!(!targets_running_crate("cargo test -p mustard-rt-extras"));
        assert!(!targets_running_crate("cargo test -p=mustard-rt-extras"));
        // Other crates / no -p at all.
        assert!(!targets_running_crate("cargo test -p mustard-core"));
        assert!(!targets_running_crate("cargo test --workspace"));
        // Only build/test relink the binary's crate via -p here.
        assert!(!targets_running_crate("cargo fmt -p mustard-rt"));
        assert!(!targets_running_crate("echo -p mustard-rt"));
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

    /// The `--workspace` rewrite path is unchanged by the direct-form guard:
    /// self-invoked workspace tests still get the `--exclude` pair appended,
    /// non-workspace forms pass through the rewrite verbatim, and with
    /// `self_invoked=false` everything is verbatim.
    #[test]
    fn qa_self_invoked_workspace_rewrite_unchanged() {
        QA_OPTIONS.with(|cell| cell.set(QaRunOptions { self_invoked: true }));
        let workspace = rewrite_self_invoked_cargo("cargo test --workspace");
        let direct_other = rewrite_self_invoked_cargo("cargo test -p mustard-core");
        QA_OPTIONS.with(|cell| cell.set(QaRunOptions::default()));
        assert_eq!(
            workspace,
            "cargo test --workspace --exclude mustard-rt --exclude mustard-dashboard"
        );
        // The rewrite never touches non-workspace forms (the direct SELF form
        // is handled upstream by the immediate skip, not by rewriting).
        assert_eq!(direct_other, "cargo test -p mustard-core");
        // External invocation: verbatim.
        assert_eq!(
            rewrite_self_invoked_cargo("cargo test --workspace"),
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
