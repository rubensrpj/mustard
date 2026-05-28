//! `mustard-rt run qa-run` — a port of `scripts/qa-run.js`.
//!
//! Executes the Acceptance Criteria defined in a spec file: locates the spec,
//! extracts the `## Acceptance Criteria` section, runs each AC command, and
//! emits a `qa.result` harness event plus a `qa` metric.
//!
//! Port note: the JS version shelled to `_lib/harness-event.js` and
//! `_lib/metrics-emit.js`. This port emits the event through the NDJSON router
//! ([`crate::shared::events::route::emit`]) and the metric through `mustard_core::platform::metrics`.
//!
//! Fail-open: a missing spec or no AC section degrades to an `overall: skip`
//! result and exit `0`; an AC failure exits `1` (the JS contract).
//!
//! `--format json` (default) prints the `{ event, payload }` JSON the pipeline
//! consumes. `--format html` additionally writes a standalone HTML report to
//! `.claude/.qa-reports/{spec}.html` and prints its path on stderr; JSON is
//! still emitted on stdout — HTML is an artifact, never a replacement.

use crate::report::{table, Report};
use crate::shared::context::{project_dir, session_id};
use mustard_core::io::fs;
use mustard_core::ClaudePaths;
use crate::util::now_iso8601;
use mustard_core::platform::metrics::{emit_metric, MetricLine};
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
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
fn find_spec_file(cwd: &Path, spec: &str) -> Option<PathBuf> {
    let paths = ClaudePaths::for_project(cwd).ok()?;
    // `specs/<spec>.md` is the legacy pre-flat-layout fallback; that directory
    // is not in the documented `ClaudePaths` catalog (post-flat-layout) so
    // build it from the claude_dir root.
    let legacy = paths.claude_dir().join("specs").join(format!("{spec}.md"));
    let sp = paths.for_spec(spec).ok()?;
    let candidates = [legacy, sp.spec_md_path(), sp.wave_plan_md_path()];
    candidates.into_iter().find(|c| c.exists())
}

/// Extract the `## Acceptance Criteria` section body (heading line stripped),
/// recognizing the EN and PT headings via [`crate::commands::spec::spec_sections`].
fn extract_ac_section(markdown: &str) -> Option<String> {
    let lines: Vec<&str> = markdown.split('\n').collect();
    let start = lines
        .iter()
        .position(|l| crate::commands::spec::spec_sections::is_heading(l, "acceptanceCriteria"))?;
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
    // Tolerate a bold-wrapped ID prefix: `**AC-G1.**` (canonical form used in
    // wave-plans + qa/review specs). Strip the leading `**` here; the matching
    // trailing `**` is consumed below after the ID/separator.
    let (rest, bold) = match rest.strip_prefix("**") {
        Some(r) => (r.trim_start(), true),
        None => (rest, false),
    };
    // `AC-<id>` where id matches `[A-Za-z0-9]+(-[A-Za-z0-9]+)*`.
    let lower = rest.to_lowercase();
    if !lower.starts_with("ac-") {
        return None;
    }
    let after_ac = &rest[3..];
    // Accept multi-segment IDs like `AC-W4-1`, `AC-TF-3`, `AC-G1`, `AC-1`.
    // Pattern: `[A-Za-z0-9]+(-[A-Za-z0-9]+)*` — each `-` must be followed by
    // at least one alphanumeric character to be part of the ID (not a separator
    // to the description text). Wave-plans use `AC-G1`/`AC-G2` (global ACs
    // spanning every wave) and wave-scoped IDs like `AC-W4-1`..`AC-W4-10`.
    let first_end = after_ac
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(after_ac.len());
    if first_end == 0 {
        return None;
    }
    // Extend the ID through any additional `-<alphanum>` segments.
    let mut id_end = first_end;
    loop {
        let tail = &after_ac[id_end..];
        // Must start with `-` followed by at least one alphanumeric.
        if !tail.starts_with('-') {
            break;
        }
        let seg_start = 1; // skip the `-`
        let seg_len = tail[seg_start..]
            .find(|c: char| !c.is_ascii_alphanumeric())
            .unwrap_or(tail[seg_start..].len());
        if seg_len == 0 {
            break;
        }
        id_end += 1 + seg_len; // consume `-` + segment
    }
    let id = format!("AC-{}", &after_ac[..id_end]);
    let after_id = &after_ac[id_end..];
    // Accept `.` or `:` as the ID/description separator. The period form is
    // canonical for the deep-refactor pipeline (`**AC-G1.** desc`); the colon
    // form is the historical shape (`AC-G1: desc`). The separator may sit
    // BEFORE the closing bold `**` (canonical: `**AC-G1.**`) or after it
    // (defensive: `**AC-G1** : desc`).
    let after_sep = if bold {
        // Two valid bold shapes:
        //   `**AC-G1.**` — separator inside the bold span; **after** "AC-G1"
        //     comes "." then "**"; description follows.
        //   `**AC-G1:**` — same with colon.
        //   `**AC-G1**.` — separator outside the bold span (rare/defensive).
        let stripped = after_id.trim_start();
        if let Some(rest) = stripped.strip_prefix('.').or_else(|| stripped.strip_prefix(':')) {
            // separator was inside the bold; expect `**` next, then description.
            rest.trim_start().strip_prefix("**")?
        } else if let Some(rest) = stripped.strip_prefix("**") {
            // bold closed first, then separator.
            let r = rest.trim_start();
            r.strip_prefix('.').or_else(|| r.strip_prefix(':'))?
        } else {
            return None;
        }
    } else {
        let stripped = after_id.trim_start();
        stripped.strip_prefix('.').or_else(|| stripped.strip_prefix(':'))?
    };
    let after_colon = after_sep;
    // Find the trailing ` Command: ` marker. Match `command:` (with the colon
    // attached) so embedded words like "commands/mustard/*" in the description
    // don't false-positive on a bare "command" substring. Use the LAST
    // occurrence — defensive against descriptions that legitimately contain
    // the literal string `command:` before the actual marker.
    let lower_seg = after_colon.to_lowercase();
    let cmd_idx = lower_seg.rfind("command:")?;
    let cmd_tail = after_colon[cmd_idx + "command:".len()..].trim();
    // W8#3: tolerate `Command: `<cmd>` (annotation)` — when the command is
    // backtick-quoted, take only the text between the first pair of backticks
    // and ignore any trailing parenthetical (e.g. "(entregue em W1)"). The
    // historical bare form (`Command: cargo test`) keeps the old behaviour.
    let command = if let Some(rest) = cmd_tail.strip_prefix('`') {
        let close = rest.find('`').unwrap_or(rest.len());
        rest[..close].trim().to_string()
    } else {
        cmd_tail.trim().to_string()
    };
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
    let Ok(mut child) = child else {
        return AcResult {
            id: String::new(),
            status: "skip".to_string(),
            exit: None,
            duration_ms: t0.elapsed().as_millis(),
            stderr_excerpt: "command not found".to_string(),
        };
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
                    exit: Some(status.code().map_or(1, i64::from)),
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
    // `qa.result` is non-pipeline → per-spec NDJSON via the W5 router.
    let _ = crate::shared::events::route::emit(cwd.to_string_lossy().as_ref(), &ev);
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

/// Write the JSON sidecar at `<root>/.claude/spec/{spec}/qa-report.json` (the
/// per-spec aggregate, per the W2 path catalog).
fn write_sidecar(cwd: &Path, spec: &str, payload: &Value) {
    let Some(sp) = ClaudePaths::for_project(cwd)
        .ok()
        .and_then(|p| p.for_spec(spec).ok())
    else {
        return;
    };
    let target = sp.qa_report_json_path();
    if let Some(parent) = target.parent() {
        if fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    if let Ok(text) = serde_json::to_string_pretty(payload) {
        let _ = fs::write_atomic(&target, text.as_bytes());
    }
}

/// Write the standalone HTML report at `<root>/.claude/spec/{spec}/qa-report.html`.
fn write_html_report(cwd: &Path, spec: &str, overall: &str, criteria: &[AcResult]) -> Option<PathBuf> {
    let sp = ClaudePaths::for_project(cwd).ok()?.for_spec(spec).ok()?;
    let path = sp.qa_report_html_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok()?;
    }
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
                c.exit.map_or_else(|| "n/a".to_string(), |e| e.to_string()),
                format!("{:.1}s", c.duration_ms as f64 / 1000.0),
                c.stderr_excerpt.chars().take(80).collect(),
            ]
        })
        .collect();
    report.section(
        "Acceptance Criteria",
        &table(&["ID", "Status", "Exit", "Duration", "stderr"], &rows),
    );
    fs::write_atomic(&path, report.render().as_bytes()).ok()?;
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
        .or_else(|| Some(std::path::PathBuf::from(crate::shared::context::project_dir())))
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
    let markdown = match fs::read_to_string(&spec_file) {
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
        res.id.clone_from(&item.id);
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

    /// Wave-plans use `AC-G1`, `AC-G2` (the `G` modifier marks global ACs that
    /// span every wave). The id parser must accept any alphanumeric suffix
    /// after `AC-`, not just digits — otherwise `qa-run` finds the section but
    /// returns zero parseable items (the bug found while closing
    /// `2026-05-20-mustard-wave-network-standard`).
    #[test]
    fn parses_ac_id_with_alphanumeric_suffix() {
        let a = parse_ac_line("- [ ] AC-G1: flag exposed — Command: `mustard-rt --version`").unwrap();
        assert_eq!(a.id, "AC-G1");
        assert_eq!(a.command, "mustard-rt --version");
        let b = parse_ac_line("- [x] AC-G7: skill reads modelo — Command: `grep -q Modelo SKILL.md`").unwrap();
        assert_eq!(b.id, "AC-G7");
        assert_eq!(b.command, "grep -q Modelo SKILL.md");
    }

    /// Multi-segment IDs (`AC-W4-1`, `AC-TF-3`, `AC-W4-10`) must parse
    /// correctly. These appear in wave-scoped specs where the wave number is
    /// embedded in the ID (e.g. wave-4 ACs use `AC-W4-N`). This was the bug
    /// fixed in `2026-05-23-tf-qa-run-parser-multidash-ac`: the scanner stopped
    /// at the first `-` inside the ID suffix, producing `AC-W4` instead of
    /// `AC-W4-1` and returning zero parseable items for the whole section.
    #[test]
    fn parses_ac_id_multi_segment() {
        // Two-segment: wave-scoped single digit.
        let a = parse_ac_line("- [ ] AC-W4-1: layout ok — Command: `cargo build`").unwrap();
        assert_eq!(a.id, "AC-W4-1");
        assert_eq!(a.command, "cargo build");
        // Two-segment: wave-scoped double digit.
        let b = parse_ac_line("- [x] AC-W4-10: all tokens — Command: `cargo test`").unwrap();
        assert_eq!(b.id, "AC-W4-10");
        assert_eq!(b.command, "cargo test");
        // Two-segment: TF prefix.
        let c = parse_ac_line("- [ ] AC-TF-3: parser fix — Command: `true`").unwrap();
        assert_eq!(c.id, "AC-TF-3");
        assert_eq!(c.command, "true");
        // Single-segment regression: AC-1 and AC-G1 must still work.
        let d = parse_ac_line("- [ ] AC-1: base — Command: `echo ok`").unwrap();
        assert_eq!(d.id, "AC-1");
        let e = parse_ac_line("- [ ] AC-G1: global — Command: `echo ok`").unwrap();
        assert_eq!(e.id, "AC-G1");
    }

    /// Bold-wrapped ID with period separator — canonical form used by every
    /// AC line in the `2026-05-25-mustard-deep-refactor` spec + every wave
    /// spec (`- [ ] **AC-G1.** desc. Command: \`rtk x\``). Regression guard
    /// for the parser fix made while closing that pipeline (qa-run was
    /// returning zero items for an otherwise well-formed section).
    #[test]
    fn parses_ac_bold_period_form() {
        let a = parse_ac_line("- [ ] **AC-G1.** descr. Command: `rtk x`").unwrap();
        assert_eq!(a.id, "AC-G1");
        assert_eq!(a.command, "rtk x");
    }

    /// Bold-wrapped ID with colon separator — defensive coverage for authors
    /// who write `**AC-G2:**` (mixing the old colon convention with the new
    /// bold wrapper). Same code path as the period form, different separator.
    #[test]
    fn parses_ac_bold_colon_form() {
        let a = parse_ac_line("- [ ] **AC-G2:** descr. Command: `rtk y`").unwrap();
        assert_eq!(a.id, "AC-G2");
        assert_eq!(a.command, "rtk y");
    }

    /// Plain (non-bold) ID with period separator — the third shape that the
    /// new code must accept: `AC-G3.` without bold wrapping. The historical
    /// parser only accepted `:`, so this exercises the additive period branch.
    #[test]
    fn parses_ac_plain_period_form() {
        let a = parse_ac_line("- [ ] AC-G3. descr. Command: `rtk z`").unwrap();
        assert_eq!(a.id, "AC-G3");
        assert_eq!(a.command, "rtk z");
    }

    /// PT heading "Critérios de Aceitação globais" (suffix word after the
    /// canonical name) must still resolve — `is_heading` matches with a
    /// word-boundary tolerance after the variant. Regression guard for
    /// language-agnostic parsing.
    #[test]
    fn extracts_ac_section_pt_heading_with_suffix() {
        let md = "# Spec\n\n## Critérios de Aceitação globais\n- [ ] AC-G1: x — Command: `true`\n\n## Files\n- a.rs\n";
        let section = extract_ac_section(md).unwrap();
        assert!(section.contains("AC-G1"));
        assert!(!section.contains("Files"));
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
