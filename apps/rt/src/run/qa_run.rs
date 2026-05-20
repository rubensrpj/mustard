//! `mustard-rt run qa-run` — a port of `scripts/qa-run.js`.
//!
//! Executes the Acceptance Criteria defined in a spec file: locates the spec,
//! extracts the `## Acceptance Criteria` section, runs each AC command, and
//! emits a `qa.result` harness event plus a `qa` metric.
//!
//! Port note: the JS version shelled to `_lib/harness-event.js` and
//! `_lib/metrics-emit.js`. This port emits the event through `mustard_core`'s
//! [`SqliteEventStore`] and the metric through `mustard_core::metrics`.
//!
//! Fail-open: a missing spec or no AC section degrades to an `overall: skip`
//! result and exit `0`; an AC failure exits `1` (the JS contract).
//!
//! `--format json` (default) prints the `{ event, payload }` JSON the pipeline
//! consumes. `--format html` additionally writes a standalone HTML report to
//! `.claude/.qa-reports/{spec}.html` and prints its path on stderr; JSON is
//! still emitted on stdout — HTML is an artifact, never a replacement.

use crate::report::{table, Report};
use crate::run::env::{project_dir, session_id};
use crate::util::now_iso8601;
use mustard_core::store::event_store::EventSink;
use mustard_core::store::sqlite_store::SqliteEventStore;
use mustard_core::metrics::{emit_metric, MetricLine};
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

/// Per-AC timeout (2 min), matching `AC_TIMEOUT_MS` in `qa-run.js`.
const AC_TIMEOUT_SECS: u64 = 120;

/// A parsed AC item: `- [ ] AC-N: description — Command: `cmd``.
struct AcItem {
    id: String,
    command: String,
}

/// One AC execution outcome.
struct AcResult {
    id: String,
    status: String,
    exit: Option<i64>,
    duration_ms: u128,
    stderr_excerpt: String,
}

/// Locate the spec file: `.claude/specs/{spec}.md`, then
/// `.claude/spec/active/{spec}/spec.md`, then `completed/`.
fn find_spec_file(cwd: &Path, spec: &str) -> Option<PathBuf> {
    let candidates = [
        cwd.join(".claude").join("specs").join(format!("{spec}.md")),
        cwd.join(".claude").join("spec").join("active").join(spec).join("spec.md"),
        cwd.join(".claude").join("spec").join("completed").join(spec).join("spec.md"),
    ];
    candidates.into_iter().find(|c| c.exists())
}

/// Extract the `## Acceptance Criteria` section body (heading line stripped),
/// recognizing the EN and PT headings via [`crate::run::spec_sections`].
fn extract_ac_section(markdown: &str) -> Option<String> {
    let lines: Vec<&str> = markdown.split('\n').collect();
    let start = lines
        .iter()
        .position(|l| crate::run::spec_sections::is_heading(l, "acceptanceCriteria"))?;
    let mut end = lines.len();
    for (i, l) in lines.iter().enumerate().skip(start + 1) {
        if l.starts_with("## ") {
            end = i;
            break;
        }
    }
    // Body only — drop the heading line itself.
    Some(lines[start + 1..end].join("\n"))
}

/// Parse `- [ ] AC-N: description — Command: `cmd`` lines.
///
/// JS regex (case-insensitive):
/// `^\s*-\s*\[[ xX]\]\s*(AC-\d+)\s*:\s*(.+?)\s*(?:—|-{1,2})\s*Command\s*:\s*`?([^`\n]+)`?\s*$`
fn parse_ac_items(section: &str) -> Vec<AcItem> {
    let mut items = Vec::new();
    for line in section.split('\n') {
        if let Some(item) = parse_ac_line(line) {
            items.push(item);
        }
    }
    items
}

/// Parse one AC line with plain string scanning (no regex crate available).
fn parse_ac_line(line: &str) -> Option<AcItem> {
    let t = line.trim_start();
    let rest = t.strip_prefix('-')?.trim_start();
    // `[ ]`, `[x]`, `[X]`.
    let rest = rest.strip_prefix('[')?;
    let mark = rest.chars().next()?;
    if !matches!(mark, ' ' | 'x' | 'X') {
        return None;
    }
    let rest = rest[mark.len_utf8()..].strip_prefix(']')?.trim_start();
    // `AC-<digits>`.
    let lower = rest.to_lowercase();
    if !lower.starts_with("ac-") {
        return None;
    }
    let after_ac = &rest[3..];
    let digits_end = after_ac.find(|c: char| !c.is_ascii_digit()).unwrap_or(after_ac.len());
    if digits_end == 0 {
        return None;
    }
    let id = format!("AC-{}", &after_ac[..digits_end]);
    let after_id = after_ac[digits_end..].trim_start();
    let after_colon = after_id.strip_prefix(':')?;
    // Find a `Command:` segment after an em-dash / hyphen separator.
    let lower_seg = after_colon.to_lowercase();
    let cmd_idx = lower_seg.find("command")?;
    // The char just before `command` (after trimming `:` and ws) should be a
    // separator — the JS pattern requires `—` / `-` / `--` before `Command`.
    let cmd_tail = &after_colon[cmd_idx + "command".len()..];
    let cmd_tail = cmd_tail.trim_start().strip_prefix(':')?.trim();
    let command = cmd_tail.trim_matches('`').trim().to_string();
    if command.is_empty() {
        return None;
    }
    Some(AcItem { id: id.to_uppercase(), command })
}

/// Build the platform shell invocation for an AC `command` string.
///
/// On non-Windows: `sh -c <command>` — `std`'s normal arg passing is correct
/// for `sh`, which parses argv entries directly.
///
/// On Windows: `cmd.exe` does **not** parse its command line via the
/// `CommandLineToArgvW` rules that `std`'s `Command::arg` quoting assumes, so
/// passing a complex `command` (quotes, `()`, `|`, `&&`) through `arg` corrupts
/// it. Instead, append the command verbatim with `CommandExt::raw_arg` (a SAFE
/// API — no `unsafe` needed) and invoke `cmd /S /C "<command>"`: with `/S` and a
/// command line whose first and last chars are quotes, `cmd` strips exactly
/// that outer quote pair and runs the remainder literally.
#[cfg(windows)]
fn build_shell_command(command: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let mut c = Command::new("cmd");
    // Single verbatim argument: `/S /C "<command>"`. One `raw_arg` call so the
    // whole `cmd` command line is exactly this — no `std` quoting, no extra
    // separators between the `/S /C` flags and the quoted payload.
    c.raw_arg(format!("/S /C \"{command}\""));
    c
}

/// See the `#[cfg(windows)]` variant for the rationale.
#[cfg(not(windows))]
fn build_shell_command(command: &str) -> Command {
    let mut c = Command::new("sh");
    c.arg("-c").arg(command);
    c
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
    for crate_name in ["mustard-rt", "mustard-dashboard"] {
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

/// Run one AC command. Mirrors the JS classification: `pass` (exit 0), `fail`
/// (non-zero exit), `skip` (timeout or spawn failure).
fn run_ac_command(command: &str, cwd: &Path) -> AcResult {
    let t0 = Instant::now();
    // POSIX-style AC commands assume a shell; use the platform shell. Windows
    // AC are documented to be cross-shell-safe (`node -e`, `bash -c`).
    // Self-invoked rewrite first — see `rewrite_self_invoked_cargo` for why.
    let rewritten = rewrite_self_invoked_cargo(command);
    let mut cmd = build_shell_command(&rewritten);
    cmd.current_dir(cwd);

    // No native wait-with-timeout in std; spawn + poll.
    let child = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(_) => {
            return AcResult {
                id: String::new(),
                status: "skip".to_string(),
                exit: None,
                duration_ms: t0.elapsed().as_millis(),
                stderr_excerpt: "command not found".to_string(),
            };
        }
    };

    let deadline = Instant::now() + std::time::Duration::from_secs(AC_TIMEOUT_SECS);
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
                if status.success() {
                    return AcResult {
                        id: String::new(),
                        status: "pass".to_string(),
                        exit: Some(0),
                        duration_ms,
                        stderr_excerpt: String::new(),
                    };
                }
                let combined: String = [stderr, stdout]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ")
                    .chars()
                    .take(100)
                    .collect();
                return AcResult {
                    id: String::new(),
                    status: "fail".to_string(),
                    exit: Some(status.code().map(i64::from).unwrap_or(1)),
                    duration_ms,
                    stderr_excerpt: combined,
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
                        stderr_excerpt: format!("timeout after {}ms", AC_TIMEOUT_SECS * 1000),
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
fn emit_qa_event(cwd: &Path, spec: &str, overall: &str, criteria: &[Value]) {
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
    let _ = SqliteEventStore::for_project(cwd).and_then(|store| store.append(&ev));
}

/// Emit the `qa` metric (fail-silent).
fn emit_qa_metric(cwd: &Path, spec: &str, overall: &str, criteria: &[AcResult]) {
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

/// The criteria array, as the JSON payload shape.
fn criteria_json(criteria: &[AcResult]) -> Vec<Value> {
    criteria
        .iter()
        .map(|c| {
            json!({
                "id": c.id,
                "status": c.status,
                "exit": c.exit,
                "duration_ms": c.duration_ms,
                "stderr_excerpt": c.stderr_excerpt,
            })
        })
        .collect()
}

/// Write the JSON sidecar at `.claude/.qa-reports/{spec}.json`.
fn write_sidecar(cwd: &Path, spec: &str, payload: &Value) {
    let dir = cwd.join(".claude").join(".qa-reports");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    if let Ok(text) = serde_json::to_string_pretty(payload) {
        let _ = std::fs::write(dir.join(format!("{spec}.json")), text);
    }
}

/// Write the standalone HTML report at `.claude/.qa-reports/{spec}.html`.
fn write_html_report(cwd: &Path, spec: &str, overall: &str, criteria: &[AcResult]) -> Option<PathBuf> {
    let dir = cwd.join(".claude").join(".qa-reports");
    std::fs::create_dir_all(&dir).ok()?;
    let mut report = Report::new(
        format!("QA Report — {spec}"),
        format!("overall: {overall} · {} criteria", criteria.len()),
    );
    let rows: Vec<Vec<String>> = criteria
        .iter()
        .map(|c| {
            vec![
                c.id.clone(),
                c.status.to_uppercase(),
                c.exit.map(|e| e.to_string()).unwrap_or_else(|| "n/a".to_string()),
                format!("{:.1}s", c.duration_ms as f64 / 1000.0),
                c.stderr_excerpt.chars().take(80).collect(),
            ]
        })
        .collect();
    report.section(
        "Acceptance Criteria",
        &table(&["ID", "Status", "Exit", "Duration", "stderr"], &rows),
    );
    let path = dir.join(format!("{spec}.html"));
    std::fs::write(&path, report.render()).ok()?;
    Some(path)
}

/// Result of a QA run — `overall` plus the criteria.
struct QaResult {
    overall: String,
    criteria: Vec<AcResult>,
}

/// Public outcome type returned by [`run_for_spec`].
///
/// Callers that do not want process::exit (e.g. `complete_spec`, `qa_run_all`)
/// use this instead of the stdout-emitting [`run`] entry point.
pub struct QaSpecOutcome {
    pub spec: String,
    pub overall: String,
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
    pub total: u32,
}

/// Options for [`run_for_spec_with_options`].
#[derive(Debug, Clone, Copy, Default)]
pub struct QaRunOptions {
    /// `true` when invoked from a process that **is itself** the binary
    /// some AC commands try to rebuild (`mustard-rt`/`mustard-dashboard`).
    ///
    /// Setting this flag makes [`rewrite_self_invoked_cargo`] auto-append
    /// `--exclude mustard-rt --exclude mustard-dashboard` to any
    /// `cargo build|test ... --workspace ...` command, so the AC does not
    /// fail with `failed to remove file mustard-rt.exe` (Windows os error 5)
    /// just because the very process running qa-run is holding the exe.
    ///
    /// `complete_spec::run_qa_fail_open` sets this. External callers
    /// (`mustard-rt run qa-run --spec X` from a CI shell) leave it `false`.
    pub self_invoked: bool,
}

/// Run QA for `spec` under the current working directory, emit `qa.result`,
/// and return a typed outcome — no stdout, no `process::exit`.
///
/// Designed for callers that need the result (e.g. `complete_spec`) without
/// taking over the process. Errors are fail-open: a missing spec returns an
/// outcome with `overall = "skip"`.
pub fn run_for_spec(spec: &str) -> QaSpecOutcome {
    run_for_spec_with_options(spec, QaRunOptions::default())
}

/// Like [`run_for_spec`] but lets the caller flip [`QaRunOptions::self_invoked`]
/// to enable the cargo-self-build rewrite.
pub fn run_for_spec_with_options(spec: &str, opts: QaRunOptions) -> QaSpecOutcome {
    QA_OPTIONS.with(|cell| cell.set(opts));
    let outcome = run_for_spec_inner(spec);
    QA_OPTIONS.with(|cell| cell.set(QaRunOptions::default()));
    outcome
}

thread_local! {
    /// Active [`QaRunOptions`] for the current thread's qa-run.
    ///
    /// Set by [`run_for_spec_with_options`] and read by
    /// [`rewrite_self_invoked_cargo`]. A `thread_local!` Cell — not an env
    /// var — because `unsafe_code` is forbidden in this crate and Rust 2024
    /// requires `unsafe` for env mutation, but a Cell-backed `thread_local`
    /// is plain safe Rust.
    static QA_OPTIONS: std::cell::Cell<QaRunOptions> = const {
        std::cell::Cell::new(QaRunOptions { self_invoked: false })
    };
}

fn run_for_spec_inner(spec: &str) -> QaSpecOutcome {
    let cwd = std::env::current_dir()
        .ok()
        .or_else(|| Some(std::path::PathBuf::from(crate::run::env::project_dir())))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let result = run_qa(&cwd, spec);
    let (mut passed, mut failed, mut skipped) = (0u32, 0u32, 0u32);
    for c in &result.criteria {
        match c.status.as_str() {
            "pass" => passed += 1,
            "fail" => failed += 1,
            _ => skipped += 1,
        }
    }
    let total = passed + failed + skipped;
    QaSpecOutcome {
        spec: spec.to_string(),
        overall: result.overall,
        passed,
        failed,
        skipped,
        total,
    }
}

/// Run QA for `spec` under `cwd`. Always emits the event + metric.
fn run_qa(cwd: &Path, spec: &str) -> QaResult {
    let Some(spec_file) = find_spec_file(cwd, spec) else {
        eprintln!("[qa-run] Spec file not found for \"{spec}\"");
        emit_qa_metric(cwd, spec, "skip", &[]);
        return QaResult { overall: "skip".to_string(), criteria: Vec::new() };
    };
    let markdown = match std::fs::read_to_string(&spec_file) {
        Ok(m) => m,
        Err(err) => {
            eprintln!("[qa-run] Cannot read spec file: {err}");
            emit_qa_metric(cwd, spec, "skip", &[]);
            return QaResult { overall: "skip".to_string(), criteria: Vec::new() };
        }
    };
    let Some(section) = extract_ac_section(&markdown) else {
        eprintln!("[qa-run] WARN: No \"Acceptance Criteria\" section found in spec");
        emit_qa_event(cwd, spec, "skip", &[]);
        emit_qa_metric(cwd, spec, "skip", &[]);
        return QaResult { overall: "skip".to_string(), criteria: Vec::new() };
    };
    let items = parse_ac_items(&section);
    if items.is_empty() {
        eprintln!("[qa-run] WARN: Acceptance Criteria section found but no parseable AC items");
        emit_qa_event(cwd, spec, "skip", &[]);
        emit_qa_metric(cwd, spec, "skip", &[]);
        return QaResult { overall: "skip".to_string(), criteria: Vec::new() };
    }

    let mut criteria = Vec::new();
    let (mut fail_count, mut skip_count) = (0usize, 0usize);
    for item in &items {
        let mut res = run_ac_command(&item.command, cwd);
        res.id = item.id.clone();
        if res.status == "fail" {
            fail_count += 1;
        } else if res.status == "skip" {
            skip_count += 1;
        }
        criteria.push(res);
    }
    let overall = if fail_count > 0 {
        "fail"
    } else if skip_count == items.len() {
        "skip"
    } else {
        "pass"
    };

    let cjson = criteria_json(&criteria);
    let payload = json!({ "spec": spec, "overall": overall, "criteria": cjson });
    emit_qa_event(cwd, spec, overall, &cjson);
    emit_qa_metric(cwd, spec, overall, &criteria);
    write_sidecar(cwd, spec, &payload);

    QaResult { overall: overall.to_string(), criteria }
}

/// Dispatch `mustard-rt run qa-run`.
pub fn run(spec: &str, format: &str) {
    let cwd = std::env::current_dir()
        .ok()
        .or_else(|| Some(PathBuf::from(project_dir())))
        .unwrap_or_else(|| PathBuf::from("."));

    let result = run_qa(&cwd, spec);
    let cjson = criteria_json(&result.criteria);

    if format == "html" {
        match write_html_report(&cwd, spec, &result.overall, &result.criteria) {
            Some(path) => eprintln!("[qa-run] HTML report: {}", path.display()),
            None => eprintln!("[qa-run] WARN: could not write HTML report"),
        }
    }

    // JSON is always emitted on stdout (the pipeline-consumed contract).
    let out = json!({
        "event": "qa.result",
        "payload": { "spec": spec, "overall": result.overall, "criteria": cjson },
    });
    println!("{}", serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".to_string()));

    if result.overall == "fail" {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parses_ac_lines_with_and_without_backticks() {
        let a = parse_ac_line("- [ ] AC-1: builds clean — Command: `cargo build`").unwrap();
        assert_eq!(a.id, "AC-1");
        assert_eq!(a.command, "cargo build");
        let b = parse_ac_line("- [x] AC-2: tests pass - Command: cargo test").unwrap();
        assert_eq!(b.id, "AC-2");
        assert_eq!(b.command, "cargo test");
        assert!(parse_ac_line("- just a bullet").is_none());
    }

    #[test]
    fn extracts_ac_section_body() {
        let md = "# Spec\n\n## Acceptance Criteria\n- [ ] AC-1: x — Command: `true`\n\n## Files\n- a.rs\n";
        let section = extract_ac_section(md).unwrap();
        assert!(section.contains("AC-1"));
        assert!(!section.contains("Files"));
    }

    #[test]
    fn skips_when_spec_missing() {
        let dir = tempdir().unwrap();
        let r = run_qa(dir.path(), "ghost");
        assert_eq!(r.overall, "skip");
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
        let res = run_ac_command(cmd, dir.path());
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
        let res = run_ac_command(cmd, dir.path());
        assert_eq!(res.status, "pass", "stderr: {}", res.stderr_excerpt);
        assert_eq!(res.exit, Some(0));
    }

    #[test]
    fn html_report_is_standalone() {
        let dir = tempdir().unwrap();
        let criteria = vec![AcResult {
            id: "AC-1".into(),
            status: "pass".into(),
            exit: Some(0),
            duration_ms: 12,
            stderr_excerpt: String::new(),
        }];
        let path = write_html_report(dir.path(), "demo", "pass", &criteria).unwrap();
        let html = std::fs::read_to_string(path).unwrap();
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("<style>"));
        assert!(!html.contains("href=") && !html.contains("src="));
        assert!(html.contains("AC-1"));
    }
}
