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
use crate::util::platform;
use mustard_core::io::fs;
use mustard_core::ClaudePaths;
use mustard_core::time::now_iso8601;
use mustard_core::platform::metrics::{emit_metric, MetricLine};
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde_json::{json, Value};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
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

/// Parse the `## Acceptance Criteria` body into `AcItem`s.
///
/// Two AC shapes are supported, both off the same header parser:
///
/// 1. **Historical one-line** — `- [ ] AC-N: desc — Command: `cmd``. The
///    `Command:` marker sits on the AC line itself; the item is complete in a
///    single line.
/// 2. **Drafter multi-line** — the canonical shape the spec drafter emits:
///    ```text
///    - **AC-1** — desc.
///      Command: `cmd`
///    ```
///    no checkbox, an em-dash (`—`) id→desc separator, and `Command:` on the
///    next indented line.
///
/// So this is an indexed loop with **lookahead**: a line that parses as an AC
/// header but carries no same-line `Command:` marker triggers a scan of the
/// following lines for the first `Command:`. The scan stops at the next AC
/// header (`- **AC-` / `- [ ] AC-` …), a blank-line gap, or a `## ` heading —
/// so a header with no command anywhere yields no item (and never bleeds into
/// the next AC's command). Fail-open: a malformed block produces no item.
fn parse_ac_items(section: &str) -> Vec<AcItem> {
    let lines: Vec<&str> = section.split('\n').collect();
    let mut items = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let Some((id, after_sep)) = parse_ac_header(lines[i]) else {
            i += 1;
            continue;
        };
        // Prefer a same-line `Command:` marker (historical one-line form).
        if let Some(command) = extract_command(after_sep) {
            items.push(AcItem { id, command });
            i += 1;
            continue;
        }
        // Lookahead: scan following lines for the first `Command:` marker,
        // stopping at the next AC header, a blank-line gap, or a `## ` heading.
        let mut j = i + 1;
        let mut command = None;
        while j < lines.len() {
            let line = lines[j];
            if parse_ac_header(line).is_some() || line.trim().is_empty() || line.starts_with("## ")
            {
                break;
            }
            if let Some(cmd) = extract_command(line) {
                command = Some(cmd);
                break;
            }
            j += 1;
        }
        if let Some(command) = command {
            items.push(AcItem { id, command });
        }
        // Resume after the header line; the next header (if any) is re-parsed
        // on its own iteration regardless of where the lookahead landed.
        i += 1;
    }
    items
}

/// Parse one AC line in the historical one-line form
/// (`- [ ] AC-N: desc — Command: `cmd``) into a complete [`AcItem`].
///
/// Thin wrapper over [`parse_ac_header`] + [`extract_command`]; the multi-line
/// drafter form is handled by [`parse_ac_items`]'s lookahead, not here. Kept as
/// the unit-test surface for the single-line shapes (production parses through
/// [`parse_ac_items`], hence `#[cfg(test)]`).
#[cfg(test)]
fn parse_ac_line(line: &str) -> Option<AcItem> {
    let (id, after_sep) = parse_ac_header(line)?;
    let command = extract_command(after_sep)?;
    Some(AcItem { id, command })
}

/// Parse the AC **header** part of a line: the bullet, an OPTIONAL `[ ]`/`[x]`
/// checkbox, the (optionally bold-wrapped) `AC-<id>`, and the id→description
/// separator. Returns the uppercased id plus the text **after** the separator
/// (which may or may not hold a `Command:` marker — that is the caller's job).
///
/// Plain string scanning, no regex crate. Returns `None` for any non-AC line.
fn parse_ac_header(line: &str) -> Option<(String, &str)> {
    let t = line.trim_start();
    let rest = t.strip_prefix('-')?.trim_start();
    // The `[ ]` / `[x]` / `[X]` checkbox is OPTIONAL: the historical checklist
    // form has it (`- [ ] AC-1 …`), the drafter's `## Critérios de Aceitação`
    // section does NOT (`- **AC-1** …`). Consume it only when present.
    let rest = match rest.strip_prefix('[') {
        Some(after_open) => {
            let mark = after_open.chars().next()?;
            if !matches!(mark, ' ' | 'x' | 'X') {
                return None;
            }
            after_open[mark.len_utf8()..].strip_prefix(']')?.trim_start()
        }
        None => rest,
    };
    // Tolerate a bold-wrapped ID prefix: `**AC-G1.**` (canonical form used in
    // wave-plans + qa/review specs, and the drafter's `- **AC-1**`). Strip the
    // leading `**` here; the matching trailing `**` is consumed below after the
    // ID/separator.
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
    // Accept `.`, `:`, the em-dash `—` (U+2014), or a plain `-`/`--` as the
    // ID/description separator. The period form is canonical for the
    // deep-refactor pipeline (`**AC-G1.** desc`); the colon form is the
    // historical shape (`AC-G1: desc`); the dash forms are what the spec
    // drafter emits (`- **AC-1** — desc`). The separator may sit BEFORE the
    // closing bold `**` (canonical: `**AC-G1.**`) or after it (`**AC-1** —`).
    let after_sep = if bold {
        // Bold shapes:
        //   `**AC-G1.**` / `**AC-G1:**` — separator inside the bold span.
        //   `**AC-1**` then `—`/`-`/`.`/`:` — separator after the closing bold.
        let stripped = after_id.trim_start();
        if let Some(rest) = strip_separator(stripped) {
            // separator was inside the bold; expect `**` next, then description.
            rest.trim_start().strip_prefix("**")?
        } else if let Some(rest) = stripped.strip_prefix("**") {
            // bold closed first, then separator.
            strip_separator(rest.trim_start())?
        } else {
            return None;
        }
    } else {
        strip_separator(after_id.trim_start())?
    };
    Some((id.to_uppercase(), after_sep))
}

/// Strip the ID→description separator from the front of `s`, returning the
/// remainder. Accepts `.`, `:`, the em-dash `—` (U+2014), `--`, or a single
/// `-`. Returns `None` if `s` does not begin with a recognised separator.
fn strip_separator(s: &str) -> Option<&str> {
    if let Some(rest) = s.strip_prefix('.').or_else(|| s.strip_prefix(':')) {
        return Some(rest);
    }
    if let Some(rest) = s.strip_prefix('—') {
        return Some(rest);
    }
    // `--` before a single `-` so `--` is consumed whole.
    if let Some(rest) = s.strip_prefix("--").or_else(|| s.strip_prefix('-')) {
        return Some(rest);
    }
    None
}

/// Extract the command from a fragment that may contain a `Command:` marker.
///
/// Matches `command:` (colon attached) so embedded words like
/// `commands/mustard/*` in a description don't false-positive on a bare
/// "command" substring. Uses the LAST occurrence — defensive against
/// descriptions that legitimately contain the literal `command:` before the
/// real marker. When the command is backtick-quoted, takes only the text
/// between the first pair of backticks and ignores any trailing parenthetical
/// (e.g. "(entregue em W1)"); the bare form (`Command: cargo test`) keeps the
/// historical behaviour. Returns `None` when no marker is present or the
/// command is empty.
fn extract_command(fragment: &str) -> Option<String> {
    let lower_seg = fragment.to_lowercase();
    let cmd_idx = lower_seg.rfind("command:")?;
    let cmd_tail = fragment[cmd_idx + "command:".len()..].trim();
    let command = if let Some(rest) = cmd_tail.strip_prefix('`') {
        let close = rest.find('`').unwrap_or(rest.len());
        rest[..close].trim().to_string()
    } else {
        cmd_tail.trim().to_string()
    };
    if command.is_empty() {
        return None;
    }
    Some(command)
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

/// Write the consolidated Markdown report at `.claude/spec/{spec}/qa/report.md`
/// (D4). The QA phase materialises its verdict by code so the result is durable
/// and visible in the dashboard, instead of depending on an agent remembering to
/// fill in a template. Atomic via [`fs::write_atomic`]. Fail-open: a missing
/// project root or write error is a silent no-op (the `qa.result` event is the
/// load-bearing record; this file is the human-readable mirror).
fn write_qa_report_md(cwd: &Path, spec: &str, overall: &str, criteria: &[AcResult]) {
    let Some(sp) = ClaudePaths::for_project(cwd)
        .ok()
        .and_then(|p| p.for_spec(spec).ok())
    else {
        return;
    };
    let qa_dir = sp.dir().join("qa");
    if fs::create_dir_all(&qa_dir).is_err() {
        return;
    }

    let mut body = String::new();
    body.push_str("# QA Report\n\n");
    let _ = writeln!(body, "- Spec: `{spec}`");
    let _ = writeln!(body, "- Overall: **{}**", overall.to_uppercase());
    let _ = writeln!(body, "- Criteria: {}\n", criteria.len());
    body.push_str("## Acceptance Criteria\n\n");
    body.push_str("| ID | Status | Exit | Duration | Detail |\n");
    body.push_str("|----|--------|------|----------|--------|\n");
    for c in criteria {
        let exit = c.exit.map_or_else(|| "n/a".to_string(), |e| e.to_string());
        let duration = format!("{:.1}s", c.duration_ms as f64 / 1000.0);
        // Keep the detail cell on one line and pipe-safe so the table stays valid.
        let detail: String = c
            .stderr_excerpt
            .replace('|', "\\|")
            .replace('\n', " ")
            .chars()
            .take(80)
            .collect();
        let _ = writeln!(
            body,
            "| {} | {} | {} | {} | {} |",
            c.id,
            c.status.to_uppercase(),
            exit,
            duration,
            detail,
        );
    }
    body.push('\n');

    let _ = fs::write_atomic(qa_dir.join("report.md"), body.as_bytes());
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
    // D4: materialise the human-readable report beside the phase dir.
    write_qa_report_md(cwd, spec, overall, &criteria);

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

    /// The canonical drafter format: NO checkbox, em-dash (`—`) separator, and
    /// `Command:` on the next indented line. Multiple ACs, one command
    /// containing `&&`. This is the exact shape that produced `overall: skip`
    /// (zero parseable items) before this fix — the regression that motivated
    /// the tactical-fix. All ids + commands must come through intact.
    #[test]
    fn parses_drafter_multiline_format() {
        let section = "\
- **AC-1** — Workspace compila, testa e linta verde.
  Command: `cargo test && cargo clippy --all-targets`
- **AC-2** — Após complete-spec, o meta da raiz fica Close/Completed.
  Command: `cargo test -p mustard-rt status_sync_integration`
";
        let items = parse_ac_items(section);
        assert_eq!(items.len(), 2, "both ACs must parse");
        assert_eq!(items[0].id, "AC-1");
        assert_eq!(items[0].command, "cargo test && cargo clippy --all-targets");
        assert_eq!(items[1].id, "AC-2");
        assert_eq!(items[1].command, "cargo test -p mustard-rt status_sync_integration");
    }

    /// Regression lock: the historical one-line forms must keep parsing
    /// identically through `parse_ac_items` (not just `parse_ac_line`). Covers
    /// the checkbox + `:`/`—` separator + same-line `Command:` shapes.
    #[test]
    fn parses_historical_oneline_format_via_items() {
        let section = "\
- [ ] AC-1: builds clean — Command: `cargo build`
- [x] AC-2: tests pass - Command: cargo test
- [ ] **AC-G1.** descr. Command: `rtk x`
";
        let items = parse_ac_items(section);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].id, "AC-1");
        assert_eq!(items[0].command, "cargo build");
        assert_eq!(items[1].id, "AC-2");
        assert_eq!(items[1].command, "cargo test");
        assert_eq!(items[2].id, "AC-G1");
        assert_eq!(items[2].command, "rtk x");
    }

    /// Drafter header with NO `Command:` anywhere (neither same-line nor on a
    /// following line) must yield NO item — not a panic, and crucially not a
    /// false item that bleeds the NEXT AC's command into this one. The
    /// lookahead stops at the next AC header.
    #[test]
    fn drafter_header_without_command_yields_no_item() {
        let section = "\
- **AC-1** — Description with no command at all.
- **AC-2** — This one has a command.
  Command: `cargo test`
";
        let items = parse_ac_items(section);
        // AC-1 has no command → dropped; AC-2 keeps its own command.
        assert_eq!(items.len(), 1, "only AC-2 has a command");
        assert_eq!(items[0].id, "AC-2");
        assert_eq!(items[0].command, "cargo test");
    }

    /// A trailing AC header with no command and no following AC (end of
    /// section) yields no item — the lookahead runs off the end safely.
    #[test]
    fn trailing_header_without_command_is_dropped() {
        let section = "- **AC-1** — Dangling header, no command.\n";
        assert!(parse_ac_items(section).is_empty());
    }

    /// Em-dash separator on a plain (non-bold, non-checkbox) header parses via
    /// `parse_ac_line` too — the dash-family separators are additive to `.`/`:`.
    #[test]
    fn parses_emdash_separator_single_line() {
        let a = parse_ac_line("- AC-1 — desc — Command: `cargo build`").unwrap();
        assert_eq!(a.id, "AC-1");
        assert_eq!(a.command, "cargo build");
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

    /// D4: `write_qa_report_md` materialises `.claude/spec/{spec}/qa/report.md`
    /// with the overall verdict + a per-AC table.
    #[test]
    fn qa_report_md_is_materialized() {
        let dir = tempdir().unwrap();
        let criteria = vec![
            AcResult {
                id: "AC-1".into(),
                status: "pass".into(),
                exit: Some(0),
                duration_ms: 120,
                stderr_excerpt: String::new(),
            },
            AcResult {
                id: "AC-2".into(),
                status: "fail".into(),
                exit: Some(1),
                duration_ms: 50,
                stderr_excerpt: "boom | pipe".into(),
            },
        ];
        write_qa_report_md(dir.path(), "demo", "fail", &criteria);

        let report_path = ClaudePaths::for_project(dir.path())
            .unwrap()
            .for_spec("demo")
            .unwrap()
            .dir()
            .join("qa")
            .join("report.md");
        let md = std::fs::read_to_string(&report_path).unwrap();
        assert!(md.starts_with("# QA Report"));
        assert!(md.contains("Overall: **FAIL**"));
        assert!(md.contains("AC-1"));
        assert!(md.contains("AC-2"));
        // Pipe in the detail cell is escaped so the table stays valid.
        assert!(md.contains("boom \\| pipe"));
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
