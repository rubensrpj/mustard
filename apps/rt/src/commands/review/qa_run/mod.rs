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
//! `.claude/spec/{spec}/qa-report.html` and prints its path on stderr; JSON is
//! still emitted on stdout — HTML is an artifact, never a replacement.

use crate::shared::context::project_dir;
use mustard_core::io::fs;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

mod render;
mod runner;

/// A parsed AC item: `- [ ] AC-N: description — Command: `cmd``.
///
/// `pub(crate)` (not `pub`) because `analyze_validation` reuses the exact
/// qa-run parser to flag AC sections that would later degrade to
/// `overall: skip` — single parser source, no drift.
pub(crate) struct AcItem {
    pub(crate) id: String,
    /// The AC description — the EARS `when X, then Y` statement, with any inline
    /// `Command:` tail stripped. Exposed so `analyze_validation`'s tautology
    /// linter and the close-time capability synthesis read it without a second
    /// parser (single parser source, no drift).
    pub(crate) statement: String,
    pub(crate) command: String,
}

/// One AC execution outcome.
///
/// `pub(crate)` so `close-pipeline` can carry the criteria of its in-process
/// QA run into the composite report (fields stay private — consumers go
/// through [`criteria_json`]).
pub(crate) struct AcResult {
    id: String,
    status: String,
    exit: Option<i64>,
    duration_ms: u128,
    stderr_excerpt: String,
}

/// Extract the `## Acceptance Criteria` section body (heading line stripped),
/// recognizing the EN and PT headings via [`crate::commands::spec::spec_sections`].
///
/// `pub(crate)`: shared with `analyze_validation` so section detection and AC
/// parsing cannot drift from what qa-run actually executes.
pub(crate) fn extract_ac_section(markdown: &str) -> Option<String> {
    // Reuse the shared, i18n-aware section extractor so this QA reader and the
    // rewave producer (which carries this section verbatim into `wave-plan.md`)
    // parse the heading identically and cannot drift.
    let block =
        crate::commands::spec::spec_sections::section_block(markdown, "acceptanceCriteria")?;
    // Body only — drop the heading line itself.
    Some(block.split_once('\n').map_or("", |(_, body)| body).to_string())
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
///
/// `pub(crate)`: `analyze_validation` calls this same parser at ANALYZE time
/// to warn about sections qa-run would later skip (zero parseable items).
pub(crate) fn parse_ac_items(section: &str) -> Vec<AcItem> {
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
            items.push(AcItem { id, statement: statement_of(after_sep), command });
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
            items.push(AcItem { id, statement: statement_of(after_sep), command });
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
    Some(AcItem { id, statement: statement_of(after_sep), command })
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

/// Extract the AC **statement** (the description) from the text after the id
/// separator: everything before an inline `Command:` marker, trimmed of a
/// trailing separator run (`—` / `-` / space). The multi-line drafter form
/// carries no inline command, so its whole `after_sep` — the EARS
/// `when X, then Y` — is the statement. Pure, total, never panics.
fn statement_of(after_sep: &str) -> String {
    let lower = after_sep.to_lowercase();
    let head = match lower.rfind("command:") {
        Some(idx) => &after_sep[..idx],
        None => after_sep,
    };
    head.trim().trim_end_matches(['—', '-', ' ']).trim().to_string()
}

/// The criteria array, as the JSON payload shape.
pub(crate) fn criteria_json(criteria: &[AcResult]) -> Vec<Value> {
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

/// Result of a QA run — `overall` plus the criteria.
///
/// `pub(crate)` so `close-pipeline` reads the per-criterion detail (which AC
/// failed) that the count-only [`QaSpecOutcome`] does not carry.
pub(crate) struct QaResult {
    pub(crate) overall: String,
    pub(crate) criteria: Vec<AcResult>,
}

/// Public outcome type returned by [`run_for_spec_with_options`].
///
/// Callers that do not want process::exit (e.g. `complete_spec`)
/// use this instead of the stdout-emitting [`run`] entry point.
pub struct QaSpecOutcome {
    pub spec: String,
    pub overall: String,
    pub passed: u32,
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
    /// It also makes [`run_ac_command`] skip outright (with an explicit
    /// reason) any `cargo build|test` that targets a [`SELF_CRATES`] member
    /// DIRECTLY via `-p`/`--package` — that form cannot be rewritten, only
    /// run externally. See [`targets_running_crate`].
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
/// outcome with `overall = "skip"`. `opts` lets the caller flip
/// [`QaRunOptions::self_invoked`] to enable the cargo-self-build rewrite.
pub fn run_for_spec_with_options(spec: &str, opts: QaRunOptions) -> QaSpecOutcome {
    let cwd = std::env::current_dir()
        .ok()
        .or_else(|| Some(std::path::PathBuf::from(crate::shared::context::project_dir())))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let result = run_qa_with_options(&cwd, spec, opts);
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
        total,
    }
}

/// Cwd-aware QA run returning the full per-criterion [`QaResult`] (not the
/// count-only outcome). The `close-pipeline` composite uses this so its report
/// can name the failed ACs. Sets/resets the thread-local [`QaRunOptions`]
/// around the run exactly like [`run_for_spec_with_options`].
pub(crate) fn run_qa_with_options(cwd: &Path, spec: &str, opts: QaRunOptions) -> QaResult {
    runner::QA_OPTIONS.with(|cell| cell.set(opts));
    let result = run_qa(cwd, spec);
    runner::QA_OPTIONS.with(|cell| cell.set(QaRunOptions::default()));
    result
}

/// `true` when `spec` carries at least one **executable** acceptance criterion
/// — the exact union [`run_qa`] would run: the spec's own `## Acceptance
/// Criteria` items PLUS any linked-capability ACs. This is the inverse of the
/// "`qa-run` would `skip`" predicate (an empty union is precisely the
/// `overall: skip` case), reusing the same `find_spec_file` +
/// [`extract_ac_section`] + [`parse_ac_items`] + [`gather_capability_acs`] path
/// so the two can never drift.
///
/// Consumed by the final-wave auto-settle in `emit-pipeline` to decide whether
/// a finished spec still owes a QA pass. Fail-open: a missing / unreadable spec
/// file with no linked-capability ACs reads as `false` (no criteria to verify).
pub(crate) fn spec_has_executable_acs(cwd: &Path, spec: &str) -> bool {
    let has_own_acs = runner::find_spec_file(cwd, spec)
        .and_then(|file| fs::read_to_string(&file).ok())
        .and_then(|markdown| extract_ac_section(&markdown))
        .is_some_and(|section| !parse_ac_items(&section).is_empty());
    has_own_acs || !runner::gather_capability_acs(cwd, spec).is_empty()
}

/// Run QA for `spec` under `cwd`. Always emits the event + metric.
fn run_qa(cwd: &Path, spec: &str) -> QaResult {
    let Some(spec_file) = runner::find_spec_file(cwd, spec) else {
        eprintln!("[qa-run] Spec file not found for \"{spec}\"");
        runner::emit_qa_metric(cwd, spec, "skip", &[]);
        return QaResult { overall: "skip".to_string(), criteria: Vec::new() };
    };
    let markdown = match fs::read_to_string(&spec_file) {
        Ok(m) => m,
        Err(err) => {
            eprintln!("[qa-run] Cannot read spec file: {err}");
            runner::emit_qa_metric(cwd, spec, "skip", &[]);
            return QaResult { overall: "skip".to_string(), criteria: Vec::new() };
        }
    };

    // The spec's OWN ACs — parsed exactly as before. An absent / unparseable
    // `## Acceptance Criteria` section yields none (it is no longer a hard skip
    // on its own, because the spec may still carry executable capability ACs).
    let mut items: Vec<(String, String)> = extract_ac_section(&markdown)
        .map(|section| {
            parse_ac_items(&section)
                .into_iter()
                .map(|it| (it.id, it.command))
                .collect()
        })
        .unwrap_or_default();
    let own_ac_count = items.len();

    // Append the executable ACs of every linked capability (F5). A spec with no
    // `## Capabilities` section adds nothing here, so its run is unchanged.
    let capability_acs = runner::gather_capability_acs(cwd, spec);
    items.extend(capability_acs);

    if items.is_empty() {
        // Nothing to run from either source ⇒ skip (preserves the historical
        // contract for specs that carry no ACs and link no capabilities).
        if own_ac_count == 0 {
            eprintln!("[qa-run] WARN: No \"Acceptance Criteria\" section and no linked capability ACs");
        } else {
            eprintln!("[qa-run] WARN: Acceptance Criteria section found but no parseable AC items");
        }
        runner::emit_qa_event(cwd, spec, "skip", &[]);
        runner::emit_qa_metric(cwd, spec, "skip", &[]);
        return QaResult { overall: "skip".to_string(), criteria: Vec::new() };
    }

    let mut criteria = Vec::new();
    let (mut fail_count, mut skip_count) = (0usize, 0usize);
    for (id, command) in &items {
        let mut res = runner::run_ac_command(command, cwd);
        res.id.clone_from(id);
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
    runner::emit_qa_event(cwd, spec, overall, &cjson);
    runner::emit_qa_metric(cwd, spec, overall, &criteria);
    render::write_sidecar(cwd, spec, &payload);
    // D4: materialise the human-readable report beside the phase dir.
    render::write_qa_report_md(cwd, spec, overall, &criteria);

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
        match render::write_html_report(&cwd, spec, &result.overall, &result.criteria) {
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

    /// The exposed `AcItem.statement` captures the EARS description with any
    /// inline `Command:` tail stripped — for both the multi-line drafter form
    /// and the historical one-line form.
    #[test]
    fn ac_item_captures_statement() {
        let items = parse_ac_items("- **AC-1** — when x happens, then y holds.\n  Command: `true`\n");
        assert_eq!(items[0].statement, "when x happens, then y holds.");
        assert_eq!(items[0].command, "true");
        let oneline = parse_ac_line("- [ ] AC-2: builds clean — Command: `cargo build`").unwrap();
        assert_eq!(oneline.statement, "builds clean", "inline Command tail stripped");
        assert_eq!(oneline.command, "cargo build");
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

    // --- F5: linked-capability scenario ACs run in QA --------------------

    /// Seed `<cwd>/.claude/spec/{spec}/spec.md` with `body`.
    fn seed_spec_md(cwd: &Path, spec: &str, body: &str) {
        let dir = cwd.join(".claude").join("spec").join(spec);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("spec.md"), body).unwrap();
    }

    /// Seed `<cwd>/.claude/capabilities/{slug}.md` by rendering a `Capability`
    /// through the canonical renderer (so the doc round-trips the parser qa-run
    /// uses).
    fn seed_capability(cwd: &Path, slug: &str, cap: &mustard_core::domain::capability::Capability) {
        let dir = cwd.join(".claude").join("capabilities");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{slug}.md")),
            crate::commands::capability::render(cap),
        )
        .unwrap();
    }

    /// A spec linking a capability whose scenario carries a command runs that
    /// scenario as an AC in QA; a documentary scenario (no command) is NOT run.
    /// The capability AC uses the SAME `run_ac_command` path as the spec's own
    /// AC, and the compiled id (`cap.{slug}-{scenario}`) appears in the result.
    #[test]
    fn linked_capability_command_scenario_runs_doc_scenario_skipped() {
        use mustard_core::domain::capability::{Capability, Requirement, Scenario};
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        let spec = "billing-feature";

        // The capability: one command-bearing scenario (`cd .` → exit 0 pass,
        // a builtin in BOTH cmd.exe and sh so the test is cross-platform) and
        // one pure-doc scenario (no command → must NOT be run).
        let cap = Capability {
            id: "cap.billing".into(),
            title: "Billing".into(),
            status: "active".into(),
            requirements: vec![Requirement {
                statement: "The system SHALL bill.".into(),
                scenarios: vec![
                    Scenario {
                        name: "charges".into(),
                        when: "an order ships".into(),
                        then: "the card is charged".into(),
                        command: Some("cd .".into()),
                    },
                    Scenario {
                        name: "documentary".into(),
                        when: "described only".into(),
                        then: "no command".into(),
                        command: None,
                    },
                ],
            }],
            ..Capability::default()
        };
        seed_capability(cwd, "billing", &cap);
        // The spec has its OWN AC plus a `## Capabilities` link to the cap.
        seed_spec_md(
            cwd,
            spec,
            "# Billing\n\n## Acceptance Criteria\n- **AC-1** — own.\n  Command: `cd .`\n\n## Capabilities\n- [[cap.billing]]\n",
        );

        let result = run_qa(cwd, spec);
        let ids: Vec<&str> = result.criteria.iter().map(|c| c.id.as_str()).collect();
        // Spec's own AC ran (unchanged behaviour).
        assert!(ids.contains(&"AC-1"), "spec's own AC ran: {ids:?}");
        // The command-bearing capability scenario ran, with its compiled id.
        assert!(
            ids.contains(&"cap.billing-charges"),
            "command-bearing capability scenario ran as an AC: {ids:?}"
        );
        // The documentary scenario (no command) was NOT compiled / NOT run.
        assert!(
            !ids.iter().any(|id| id.starts_with("cap.billing-documentary")),
            "documentary scenario (no command) must not run: {ids:?}"
        );
        // Both runnable ACs are `true` → overall pass.
        assert_eq!(result.overall, "pass");
        assert_eq!(result.criteria.len(), 2, "exactly own AC + one capability AC");
    }

    /// A spec with NO `## Capabilities` section runs exactly its own ACs — the
    /// capability gather adds nothing (the unchanged-behaviour guarantee).
    #[test]
    fn spec_without_capabilities_section_runs_only_own_acs() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        let spec = "plain-feature";
        seed_spec_md(
            cwd,
            spec,
            "# Plain\n\n## Acceptance Criteria\n- **AC-1** — own.\n  Command: `cd .`\n",
        );
        let result = run_qa(cwd, spec);
        assert_eq!(result.criteria.len(), 1, "only the spec's own AC ran");
        assert_eq!(result.criteria[0].id, "AC-1");
        assert_eq!(result.overall, "pass");
    }

    /// A linked-but-MISSING (or unreadable) capability doc is skipped and never
    /// aborts QA: the spec's own ACs still run and the run completes.
    #[test]
    fn missing_linked_capability_doc_is_skipped_not_fatal() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        let spec = "ghost-cap-feature";
        // Link a capability whose doc was never authored.
        seed_spec_md(
            cwd,
            spec,
            "# Ghost\n\n## Acceptance Criteria\n- **AC-1** — own.\n  Command: `cd .`\n\n## Capabilities\n- [[cap.ghost]]\n",
        );
        let result = run_qa(cwd, spec);
        // Only the spec's own AC ran; the missing cap added nothing, no panic.
        assert_eq!(result.criteria.len(), 1);
        assert_eq!(result.criteria[0].id, "AC-1");
        assert_eq!(result.overall, "pass");
    }

    /// A spec with NO own `## Acceptance Criteria` section but a linked
    /// capability that DOES carry a command-bearing scenario still runs that
    /// scenario — the executable capability AC is the whole point of the link.
    #[test]
    fn capability_ac_runs_even_without_own_ac_section() {
        use mustard_core::domain::capability::{Capability, Requirement, Scenario};
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        let spec = "caps-only-feature";
        let cap = Capability {
            id: "cap.only".into(),
            status: "active".into(),
            requirements: vec![Requirement {
                statement: "R".into(),
                scenarios: vec![Scenario {
                    name: "runs".into(),
                    when: "x".into(),
                    then: "y".into(),
                    command: Some("cd .".into()),
                }],
            }],
            ..Capability::default()
        };
        seed_capability(cwd, "only", &cap);
        seed_spec_md(cwd, spec, "# Caps Only\n\nNarrative.\n\n## Capabilities\n- [[cap.only]]\n");

        let result = run_qa(cwd, spec);
        assert_eq!(result.criteria.len(), 1, "the capability AC ran");
        assert_eq!(result.criteria[0].id, "cap.only-runs");
        assert_eq!(result.overall, "pass");
    }
}
