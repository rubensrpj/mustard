//! `mustard-rt run migrate-spec-headers` — the terminal spec-header migration.
//!
//! ## Scope (spec-lifecycle-unification Wave 7)
//!
//! Rewrites every legacy spec header (`### Status:` + `### Phase:`) under
//! `<root>` (default `.claude/spec`) into the canonical three-line form:
//!
//! ```text
//! ### Stage: Execute
//! ### Outcome: Active
//! ### Flags: followup_open
//! ```
//!
//! It is **dry-run by default** (the review artifact is the audit log);
//! `--apply` must be passed explicitly to mutate spec files, and `--apply` is
//! mutually exclusive with `--dry-run`. Mapping follows the wave-plan table:
//! a terminal `Status` wins over `Phase`; a qualifier `Status`
//! (blocked/wave-failed) lets `Phase` decide the stage and becomes a flag.
//!
//! ## Safety (inviolable)
//!
//! - **Atomic per file.** A write goes through a sibling tempfile + rename, so
//!   a crash between files never leaves one half-written.
//! - **Idempotent.** A spec that already carries `### Stage:` is skipped.
//! - **Fail-open per file.** A read/parse/write error on one file increments
//!   `errors` in the audit log and continues — one bad file never aborts the
//!   batch.
//! - **Byte-stable.** Only the `### Status:`/`### Phase:` lines change; CRLF
//!   terminators, indentation, accented UTF-8 and every other byte are
//!   preserved. The rewrite never indexes a string with `&s[a..b]` (which
//!   panics off a char boundary) — it operates on whole lines.

use mustard_core::{Flags, Outcome, Stage};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::util::now_iso8601;

/// Options for `mustard-rt run migrate-spec-headers`.
pub struct MigrateOpts {
    /// Write the changes (atomic per file). When `false`, the run is a dry-run
    /// and no spec file is touched — only the audit log is written.
    pub apply: bool,
    /// Root directory to scan recursively for `*.md` files.
    pub root: PathBuf,
    /// Audit-log destination. `None` resolves to
    /// `.claude/.harness/migration-{date}.log.json`.
    pub log: Option<PathBuf>,
    /// Optional case-insensitive substring/glob-stem filter on the relative
    /// path; only matching files are processed. `None` processes all.
    pub filter: Option<String>,
}

// ---------------------------------------------------------------------------
// Header values resolved for one file
// ---------------------------------------------------------------------------

/// The canonical `(stage, outcome, flags)` resolved for one legacy header,
/// plus an optional note recorded in the audit log when `Status` and `Phase`
/// disagreed on the stage.
struct Resolved {
    stage: Stage,
    outcome: Outcome,
    flags: Flags,
    /// `Some(note)` when the terminal/qualifier rules overrode the `Phase`.
    inferred_stage_override: Option<String>,
}

/// The TitleCase header spelling of a [`Stage`] (round-trips through
/// [`Stage::parse`], which is case-insensitive).
fn stage_label(stage: Stage) -> &'static str {
    match stage {
        Stage::Analyze => "Analyze",
        Stage::Plan => "Plan",
        Stage::Execute => "Execute",
        Stage::QaReview => "QaReview",
        Stage::Close => "Close",
        // `Stage` is `#[non_exhaustive]`; a future variant degrades to Plan.
        _ => "Plan",
    }
}

/// The TitleCase header spelling of an [`Outcome`].
fn outcome_label(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Active => "Active",
        Outcome::Completed => "Completed",
        Outcome::Cancelled => "Cancelled",
        Outcome::Abandoned => "Abandoned",
        // `Outcome` is `#[non_exhaustive]`; a future variant degrades to Active.
        _ => "Active",
    }
}

/// The comma-separated flag tokens of a [`Flags`] (canonical snake_case
/// spellings; empty string when no flag is set).
fn flags_label(flags: &Flags) -> String {
    let mut out: Vec<&str> = Vec::new();
    if flags.blocked {
        out.push("blocked");
    }
    if flags.wave_failed {
        out.push("wave_failed");
    }
    if flags.followup_open {
        out.push("followup_open");
    }
    out.join(", ")
}

/// Whether a legacy `Status` token is terminal (Completed / Cancelled /
/// Abandoned) — a terminal status wins over any `Phase`.
fn terminal_outcome(status: &str) -> Option<Outcome> {
    match parse_outcome_tolerant(status) {
        Some(o) if o != Outcome::Active => Some(o),
        _ => None,
    }
}

/// Map a legacy `Status` (token, no leading `### Status:`) onto the
/// qualifier-flags it implies, returning `(flags, is_pure_qualifier)`. A pure
/// qualifier (`blocked`/`paused`/`wave-failed`) lets `Phase` decide the stage.
fn qualifier_flags(status: &str) -> (Flags, bool) {
    let token = value_token(status);
    let flags = Flags::parse(&token);
    let lower = token.to_ascii_lowercase();
    let pure = matches!(
        lower.as_str(),
        "blocked" | "paused" | "wave-failed" | "wave_failed"
    );
    (flags, pure)
}

/// Normalise a legacy header *value* down to the single token that the core
/// `Stage::parse` / `Outcome::parse` enums understand.
///
/// Sub-plan files write decorated values like `QA (plano)`, `REVIEW (plano)` or
/// `completed | Phase: CLOSE | Scope: light`. We take the leading token before
/// any `(` or `|`, trim it, and hand that to the strict core parser. The
/// returned token keeps its original casing (the core parsers lowercase).
fn value_token(raw: &str) -> String {
    raw.split(['(', '|'])
        .next()
        .unwrap_or(raw)
        .trim()
        .to_string()
}

/// Tolerant [`Stage::parse`]: strips a trailing parenthetical / pipe segment
/// (`QA (plano)` → `QA`) and recognises the sub-plan `queued` sentinel as a
/// not-yet-started Plan item.
fn parse_stage_tolerant(raw: &str) -> Option<Stage> {
    let token = value_token(raw);
    if token.eq_ignore_ascii_case("queued") {
        return Some(Stage::Plan);
    }
    Stage::parse(&token)
}

/// Tolerant [`Outcome::parse`]: strips the parenthetical / pipe tail before
/// handing the leading token to the strict core parser. `queued` is a
/// not-yet-started item, so it carries the non-terminal `Active` outcome (the
/// caller's default), hence `None` here.
fn parse_outcome_tolerant(raw: &str) -> Option<Outcome> {
    Outcome::parse(&value_token(raw))
}

/// Resolve `(Stage, Outcome, Flags)` from the legacy `status` / `phase` tokens
/// per the wave-plan mapping table.
///
/// Returns `None` only when neither token yields any signal (a malformed spec
/// with both headers present but unparseable) — callers treat that as a skip.
fn resolve(status: Option<&str>, phase: Option<&str>) -> Option<Resolved> {
    let phase_stage = phase.and_then(parse_stage_tolerant);

    // 0. `queued` sub-plan sentinel: not-yet-started Plan item. The Phase (if
    //    any) names where it *will* run, but a queued item has not entered it,
    //    so the canonical stage stays Plan and the outcome is Active.
    if let Some(status) = status {
        if value_token(status).eq_ignore_ascii_case("queued") {
            return Some(Resolved {
                stage: Stage::Plan,
                outcome: Outcome::Active,
                flags: Flags::default(),
                inferred_stage_override: None,
            });
        }
    }

    // 1. Terminal status wins outright.
    if let Some(status) = status {
        if let Some(outcome) = terminal_outcome(status) {
            // Cancelled keeps the last known phase as the stage when present,
            // but a terminal outcome is only legal at Close (SpecState::new),
            // so the canonical stage is always Close. We still record the
            // override note when a non-Close phase was declared.
            let override_note = phase_stage.filter(|s| *s != Stage::Close).map(|_| {
                format!(
                    "phase -> close (terminal status {})",
                    status.trim().to_ascii_lowercase()
                )
            });
            // followup_open is the only flag that pairs with Close+Active; a
            // terminal outcome carries no qualifier flags.
            return Some(Resolved {
                stage: Stage::Close,
                outcome,
                flags: Flags::default(),
                inferred_stage_override: override_note,
            });
        }
    }

    // 2. closed-followup: Close + Active + followup_open.
    if let Some(status) = status {
        let lower = value_token(status).to_ascii_lowercase();
        if matches!(lower.as_str(), "closed-followup" | "closed_followup") {
            return Some(Resolved {
                stage: Stage::Close,
                outcome: Outcome::Active,
                flags: Flags {
                    followup_open: true,
                    ..Flags::default()
                },
                inferred_stage_override: None,
            });
        }
    }

    // 3. Qualifier status (blocked / wave-failed): Phase decides stage,
    //    status becomes a flag, Outcome stays Active.
    if let Some(status) = status {
        let (flags, pure) = qualifier_flags(status);
        if pure {
            // wave_failed is only legal at Execute; blocked defaults to Plan.
            let default_stage = if flags.wave_failed {
                Stage::Execute
            } else {
                Stage::Plan
            };
            let stage = phase_stage.unwrap_or(default_stage);
            // SpecState::new rejects wave_failed off Execute; clamp to Execute
            // when the declared phase disagrees so the result stays legal.
            let stage = if flags.wave_failed { Stage::Execute } else { stage };
            return Some(Resolved {
                stage,
                outcome: Outcome::Active,
                flags,
                inferred_stage_override: None,
            });
        }
    }

    // 4. Non-terminal status (draft/approved/planning/implementing/...) —
    //    Status maps to a stage; Phase refines it when present and they agree
    //    on a non-terminal stage. Status wins the Outcome (always Active here).
    let status_stage = status.and_then(parse_stage_tolerant);
    let stage = phase_stage.or(status_stage)?;
    Some(Resolved {
        stage,
        outcome: Outcome::Active,
        flags: Flags::default(),
        inferred_stage_override: None,
    })
}

// ---------------------------------------------------------------------------
// Header extraction (tolerant, CRLF-safe)
// ---------------------------------------------------------------------------

/// Strip a legacy header prefix off a trimmed line, returning the remainder
/// after the key (i.e. starting at the `:`). Recognizes BOTH legacy shapes,
/// case-insensitively on the key:
///
/// - `### <Key>:` — the `###`-heading form.
/// - `- **<Key>**:` — the bullet-list form (`**`-bold key), as used by the
///   older `# Mustard 2.0 — Phase N` specs.
///
/// Returns `(value_after_colon_trimmed)` with the original casing preserved.
fn strip_header_key(line: &str, key: &str) -> Option<String> {
    let want = key.to_ascii_lowercase();
    let t = line.trim_start();
    // `### <Key>:` form.
    if let Some(rest) = t.strip_prefix("###") {
        let rest = rest.trim_start();
        let lower = rest.to_ascii_lowercase();
        if let Some(after_key) = lower.strip_prefix(&want) {
            let after_key = after_key.trim_start();
            if let Some(after_colon) = after_key.strip_prefix(':') {
                let value_start = rest.len() - after_colon.len();
                return Some(rest[value_start..].trim().to_string());
            }
        }
    }
    // `- **<Key>**:` bullet form.
    if let Some(rest) = t.strip_prefix("- **").or_else(|| t.strip_prefix("-\t**")) {
        let lower = rest.to_ascii_lowercase();
        if let Some(after_key) = lower.strip_prefix(&want) {
            let after_key = after_key.trim_start();
            // Expect the closing `**` then a `:`.
            if let Some(after_bold) = after_key.strip_prefix("**") {
                let after_bold = after_bold.trim_start();
                if let Some(after_colon) = after_bold.strip_prefix(':') {
                    let value_start = rest.len() - after_colon.len();
                    return Some(rest[value_start..].trim().to_string());
                }
            }
        }
    }
    None
}

/// The number of leading lines that make up the **header region** — the
/// contiguous metadata block at the top of a spec, *before* the body begins.
///
/// A spec header is a run of `### Key:` / `- **Key**:` lines (with blank lines
/// and a leading `# Title` allowed). The region ends at the first line that is
/// unmistakably body: a level-2 `## ` section heading, or the opening of a
/// fenced code block (```` ``` ````/`~~~`). Any `### Stage:`/`### Status:` that
/// appears *after* this point is prose or an example — never a real header — so
/// scoping header detection to `line_index < header_region_lines(content)`
/// stops the migration from being fooled by specs that document the new format.
///
/// We deliberately do NOT terminate on the first prose paragraph: legacy specs
/// interleave a `## Justificativa` body section, but their header bullets always
/// precede the first `## `/code-fence, so the level-2/fence boundary is the
/// robust, language-agnostic cutoff.
fn header_region_lines(content: &str) -> usize {
    let mut count = 0usize;
    for line in content.lines() {
        let t = line.trim_start();
        // A level-2 (or deeper-but-not-3) ATX heading ends the header block.
        // `## ` / `#### ` start a body section; `### ` and `# ` do not.
        if t.starts_with("## ") {
            break;
        }
        // A fenced code block opener ends the header block.
        if t.starts_with("```") || t.starts_with("~~~") {
            break;
        }
        count += 1;
    }
    count
}

/// The value of a legacy `<Key>` header line (either shape; case-insensitive on
/// the key), trimmed — searching only the **header region** so a `### Status:`
/// mentioned in prose or a code fence is not mistaken for the header. Mirrors
/// the tolerant parser in `hooks::spec_hygiene`. Returns `None` when absent.
fn header_field(spec_md: &str, key: &str) -> Option<String> {
    let region = header_region_lines(spec_md);
    spec_md
        .lines()
        .take(region)
        .find_map(|line| strip_header_key(line, key))
}

/// `true` when a header line (trimmed) is a legacy header for `key` in either
/// shape (case-insensitive). Used to identify the two legacy lines to replace.
fn is_header_line(line: &str, key: &str) -> bool {
    strip_header_key(line, key).is_some()
}

/// Split a combined single-line header value into its pipe-separated segments.
///
/// Older specs cram the whole header onto the `### Status:` line:
///
/// ```text
/// ### Status: completed | Phase: CLOSE | Scope: light
/// ```
///
/// `header_field("Status")` returns `completed | Phase: CLOSE | Scope: light`.
/// This splits on `|` into the leading status value plus the trailing
/// `Key: value` segments, returning `(status_value, extra_segments)` where each
/// extra is `(key, value)` with original casing preserved and trimmed.
fn split_combined_status(value: &str) -> (String, Vec<(String, String)>) {
    let mut parts = value.split('|');
    let status = parts.next().unwrap_or("").trim().to_string();
    let extras = parts
        .filter_map(|seg| {
            let seg = seg.trim();
            seg.split_once(':')
                .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        })
        .collect();
    (status, extras)
}

/// The classification of a `*.md` file before migration.
enum Plan {
    /// Already migrated — has `### Stage:`.
    AlreadyMigrated,
    /// No `### Status:` and no `### Phase:` — not a spec / malformed.
    NoStatusHeader,
    /// Migratable: the resolved new state plus the rewritten content.
    Migrate {
        resolved_status: Option<String>,
        resolved_phase: Option<String>,
        resolved: Resolved,
        new_content: String,
    },
    /// Has a legacy header but it could not be resolved to any state.
    Malformed,
}

/// Classify + (when migratable) compute the rewritten content for `content`.
///
/// The rewrite replaces the **first** legacy header line (`### Status:` or
/// `### Phase:`, whichever appears first) in place with the three canonical
/// lines, and drops any further legacy line. All other bytes — CRLF
/// terminators, indentation, accents — are copied verbatim.
fn plan_for(content: &str) -> Plan {
    // All header detection is scoped to the header region (lines before the
    // first `## ` section / code fence), so a `### Stage:`/`### Status:` written
    // in prose or an example never counts as the header.
    if header_field(content, "Stage").is_some() {
        return Plan::AlreadyMigrated;
    }
    let raw_status = header_field(content, "Status");
    let mut phase = header_field(content, "Phase");
    if raw_status.is_none() && phase.is_none() {
        return Plan::NoStatusHeader;
    }

    // Combined single-line form: `### Status: completed | Phase: CLOSE | Scope: light`.
    // Split the status value; pull a `Phase` segment out when there is no
    // separate `### Phase:` line, and preserve every other segment (e.g.
    // `Scope: light`) as its own canonical `### Key: value` line so no info is
    // lost. `status` becomes just the leading value.
    let mut extras: Vec<(String, String)> = Vec::new();
    let status = if let Some(raw) = raw_status.as_deref() {
        let (lead, segs) = split_combined_status(raw);
        for (k, v) in segs {
            if k.eq_ignore_ascii_case("phase") {
                if phase.is_none() {
                    phase = Some(v);
                }
            } else {
                extras.push((k, v));
            }
        }
        Some(lead)
    } else {
        None
    };

    let Some(resolved) = resolve(status.as_deref(), phase.as_deref()) else {
        return Plan::Malformed;
    };

    // Split into lines while keeping each terminator (`\n` / `\r\n`). We rebuild
    // byte-for-byte: every segment that is not a legacy header is re-emitted
    // unchanged, so CRLF and trailing-newline shape are preserved exactly. Only
    // legacy lines inside the header region are rewritten — a `### Status:` in
    // the body (prose/code) is copied verbatim like any other line.
    let region = header_region_lines(content);
    let mut out = String::with_capacity(content.len() + 64);
    let mut placed = false;
    for (idx, seg) in content.split_inclusive('\n').enumerate() {
        // The terminator (`\r\n` or `\n`, or none on a final unterminated line).
        let body = seg.trim_end_matches(['\n', '\r']);
        let terminator = &seg[body.len()..];
        let in_region = idx < region;
        if in_region && (is_header_line(body, "Status") || is_header_line(body, "Phase")) {
            if !placed {
                // Preserve the indentation of the first legacy line, and reuse
                // its terminator for each of the new lines.
                let indent_len = body.len() - body.trim_start().len();
                let indent = &body[..indent_len];
                let term = if terminator.is_empty() { "\n" } else { terminator };
                out.push_str(indent);
                out.push_str("### Stage: ");
                out.push_str(stage_label(resolved.stage));
                out.push_str(term);
                out.push_str(indent);
                out.push_str("### Outcome: ");
                out.push_str(outcome_label(resolved.outcome));
                out.push_str(term);
                out.push_str(indent);
                out.push_str("### Flags: ");
                out.push_str(&flags_label(&resolved.flags));
                out.push_str(term);
                // Preserve any extra segments carried on a combined single line
                // (e.g. `Scope: light`) as their own canonical header lines.
                for (k, v) in &extras {
                    out.push_str(indent);
                    out.push_str("### ");
                    out.push_str(k);
                    out.push_str(": ");
                    out.push_str(v);
                    out.push_str(term);
                }
                placed = true;
            }
            // Drop the legacy line (the first one was already replaced above;
            // a second legacy line is simply removed).
            continue;
        }
        out.push_str(seg);
    }

    Plan::Migrate {
        resolved_status: raw_status,
        resolved_phase: phase,
        resolved,
        new_content: out,
    }
}

// ---------------------------------------------------------------------------
// Atomic write
// ---------------------------------------------------------------------------

/// Write `content` to `path` atomically: a sibling tempfile is written and
/// flushed, then renamed over the target. A crash before the rename leaves the
/// original untouched. Returns the IO error on failure.
fn atomic_write(path: &Path, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "spec.md".to_string());
    let tmp = dir.join(format!(".{file_name}.migrate.tmp"));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.flush()?;
    }
    std::fs::rename(&tmp, path)
}

// ---------------------------------------------------------------------------
// Audit log
// ---------------------------------------------------------------------------

/// Per-file audit record.
#[derive(Serialize)]
struct FileRecord {
    path: String,
    action: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    before: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    after: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    inferred_stage_override: Option<String>,
}

/// The complete audit log written to disk in both modes.
#[derive(Serialize)]
struct AuditLog {
    ran_at: String,
    mode: &'static str,
    root: String,
    total_files: usize,
    migrated: usize,
    skipped_already_migrated: usize,
    skipped_malformed: usize,
    errors: usize,
    files: Vec<FileRecord>,
}

// ---------------------------------------------------------------------------
// File discovery
// ---------------------------------------------------------------------------

/// Recursively collect every `*.md` under `root`, sorted for stable output.
fn collect_md(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_md_into(root, &mut out);
    out.sort();
    out
}

fn collect_md_into(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(std::result::Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect_md_into(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path);
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Run `mustard-rt run migrate-spec-headers`.
///
/// Walks `<root>`, classifies each `*.md`, and either records a dry-run diff or
/// applies the atomic rewrite. The audit log is written in **both** modes (it
/// is the review artifact). Prints a one-line JSON summary to stdout.
pub fn run(opts: MigrateOpts) {
    let mode = if opts.apply { "apply" } else { "dry-run" };
    let files = collect_md(&opts.root);
    let filter = opts.filter.as_deref().map(str::to_ascii_lowercase);

    let mut records: Vec<FileRecord> = Vec::new();
    let mut migrated = 0usize;
    let mut skipped_already = 0usize;
    let mut skipped_malformed = 0usize;
    let mut errors = 0usize;
    let mut total = 0usize;

    for path in &files {
        let rel = path.display().to_string();
        if let Some(ref f) = filter {
            if !rel.to_ascii_lowercase().contains(f) {
                continue;
            }
        }
        total += 1;

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => {
                // Fail-open: a per-file read error is logged, never aborts.
                errors += 1;
                records.push(FileRecord {
                    path: rel,
                    action: "error",
                    before: None,
                    after: None,
                    reason: Some("read-failed"),
                    inferred_stage_override: None,
                });
                continue;
            }
        };

        match plan_for(&content) {
            Plan::AlreadyMigrated => {
                skipped_already += 1;
                records.push(FileRecord {
                    path: rel,
                    action: "skipped",
                    before: None,
                    after: None,
                    reason: Some("already-migrated"),
                    inferred_stage_override: None,
                });
            }
            Plan::NoStatusHeader => {
                skipped_malformed += 1;
                records.push(FileRecord {
                    path: rel,
                    action: "skipped",
                    before: None,
                    after: None,
                    reason: Some("no-status-header"),
                    inferred_stage_override: None,
                });
            }
            Plan::Malformed => {
                skipped_malformed += 1;
                records.push(FileRecord {
                    path: rel,
                    action: "skipped",
                    before: None,
                    after: None,
                    reason: Some("unresolvable-header"),
                    inferred_stage_override: None,
                });
            }
            Plan::Migrate {
                resolved_status,
                resolved_phase,
                resolved,
                new_content,
            } => {
                let before = json!({
                    "status": resolved_status,
                    "phase": resolved_phase,
                });
                let after = json!({
                    "stage": stage_label(resolved.stage),
                    "outcome": outcome_label(resolved.outcome),
                    "flags": flags_label(&resolved.flags)
                        .split(',')
                        .map(|t| t.trim().to_string())
                        .filter(|t| !t.is_empty())
                        .collect::<Vec<_>>(),
                });
                if opts.apply {
                    match atomic_write(path, &new_content) {
                        Ok(()) => {}
                        Err(_) => {
                            errors += 1;
                            records.push(FileRecord {
                                path: rel,
                                action: "error",
                                before: Some(before),
                                after: Some(after),
                                reason: Some("write-failed"),
                                inferred_stage_override: resolved.inferred_stage_override,
                            });
                            continue;
                        }
                    }
                }
                migrated += 1;
                records.push(FileRecord {
                    path: rel,
                    action: "migrated",
                    before: Some(before),
                    after: Some(after),
                    reason: None,
                    inferred_stage_override: resolved.inferred_stage_override,
                });
            }
        }
    }

    let log = AuditLog {
        ran_at: now_iso8601(),
        mode,
        root: opts.root.display().to_string(),
        total_files: total,
        migrated,
        skipped_already_migrated: skipped_already,
        skipped_malformed,
        errors,
        files: records,
    };

    // Resolve the audit-log path (default `.claude/.harness/migration-{date}.log.json`).
    let log_path = opts.log.unwrap_or_else(default_log_path);
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let log_json = serde_json::to_string_pretty(&log).unwrap_or_else(|_| "{}".to_string());
    let log_written = std::fs::write(&log_path, &log_json).is_ok();

    // One-line stdout summary (byte-stable JSON).
    let summary = json!({
        "mode": mode,
        "root": opts.root.display().to_string(),
        "total_files": total,
        "migrated": migrated,
        "skipped_already_migrated": skipped_already,
        "skipped_malformed": skipped_malformed,
        "errors": errors,
        "log": log_path.display().to_string(),
        "log_written": log_written,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&summary).unwrap_or_else(|_| "{}".to_string())
    );
}

/// `.claude/.harness/migration-{YYYY-MM-DD}.log.json` relative to the cwd.
fn default_log_path() -> PathBuf {
    let date = now_iso8601().get(..10).unwrap_or("unknown").to_string();
    PathBuf::from(".claude")
        .join(".harness")
        .join(format!("migration-{date}.log.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_happy_path_approved_execute() {
        let r = resolve(Some("approved"), Some("EXECUTE")).expect("resolves");
        assert_eq!(r.stage, Stage::Execute);
        assert_eq!(r.outcome, Outcome::Active);
        assert_eq!(flags_label(&r.flags), "");
    }

    #[test]
    fn resolve_approved_no_phase_defaults_plan() {
        let r = resolve(Some("approved"), None).expect("resolves");
        assert_eq!(r.stage, Stage::Plan);
        assert_eq!(r.outcome, Outcome::Active);
    }

    #[test]
    fn resolve_completed_overrides_execute_phase() {
        let r = resolve(Some("completed"), Some("EXECUTE")).expect("resolves");
        assert_eq!(r.stage, Stage::Close);
        assert_eq!(r.outcome, Outcome::Completed);
        assert!(r.inferred_stage_override.is_some());
    }

    #[test]
    fn resolve_cancelled_with_plan_overrides() {
        let r = resolve(Some("cancelled"), Some("PLAN")).expect("resolves");
        assert_eq!(r.stage, Stage::Close);
        assert_eq!(r.outcome, Outcome::Cancelled);
        assert!(r.inferred_stage_override.is_some());
    }

    #[test]
    fn resolve_closed_followup_sets_flag() {
        let r = resolve(Some("closed-followup"), None).expect("resolves");
        assert_eq!(r.stage, Stage::Close);
        assert_eq!(r.outcome, Outcome::Active);
        assert!(r.flags.followup_open);
        assert_eq!(flags_label(&r.flags), "followup_open");
    }

    #[test]
    fn resolve_blocked_is_qualifier_phase_decides() {
        let r = resolve(Some("blocked"), Some("EXECUTE")).expect("resolves");
        assert_eq!(r.stage, Stage::Execute);
        assert!(r.flags.blocked);
        assert_eq!(r.outcome, Outcome::Active);
    }

    #[test]
    fn resolved_states_are_legal_spec_states() {
        // Every resolved triple must pass SpecState::new (W1 invariants).
        use mustard_core::SpecState;
        for (s, p) in [
            (Some("approved"), Some("EXECUTE")),
            (Some("completed"), Some("EXECUTE")),
            (Some("cancelled"), Some("PLAN")),
            (Some("closed-followup"), None),
            (Some("blocked"), Some("EXECUTE")),
            (Some("wave-failed"), Some("PLAN")),
            (Some("implementing"), None),
        ] {
            let r = resolve(s, p).expect("resolves");
            SpecState::new(r.stage, r.outcome, r.flags)
                .unwrap_or_else(|e| panic!("illegal state for {s:?}/{p:?}: {e}"));
        }
    }

    #[test]
    fn plan_skips_already_migrated() {
        let md = "# X\n### Stage: Execute\n### Outcome: Active\n";
        assert!(matches!(plan_for(md), Plan::AlreadyMigrated));
    }

    #[test]
    fn plan_skips_no_header() {
        let md = "# X\nno header here\n";
        assert!(matches!(plan_for(md), Plan::NoStatusHeader));
    }

    #[test]
    fn plan_rewrite_preserves_other_headers_and_order() {
        let md = "# Spec\n### Parent: [[epic]]\n### Status: approved\n### Phase: EXECUTE\n### Lang: pt\n\nbody\n";
        let Plan::Migrate { new_content, .. } = plan_for(md) else {
            panic!("expected migrate");
        };
        assert!(new_content.contains("### Parent: [[epic]]"));
        assert!(new_content.contains("### Stage: Execute"));
        assert!(new_content.contains("### Outcome: Active"));
        assert!(new_content.contains("### Flags:"));
        assert!(new_content.contains("### Lang: pt"));
        assert!(!new_content.contains("### Status:"));
        assert!(!new_content.contains("### Phase:"));
        // Parent precedes Stage which precedes Lang (relative order kept).
        let parent = new_content.find("### Parent:").unwrap();
        let stage = new_content.find("### Stage:").unwrap();
        let lang = new_content.find("### Lang:").unwrap();
        assert!(parent < stage && stage < lang);
    }

    #[test]
    fn plan_rewrite_is_crlf_byte_safe_with_accents() {
        // Build with explicit CRLF + accented Portuguese to prove no panic and
        // byte-stable terminators.
        let md = [
            "# Especificação — fase ó",
            "### Status: implementing",
            "### Phase: EXECUTE",
            "### Lang: pt",
            "",
            "Justificativa: configuração não pronta — ção ó é.",
            "",
        ]
        .join("\r\n");
        let Plan::Migrate { new_content, .. } = plan_for(&md) else {
            panic!("expected migrate");
        };
        // CRLF preserved on the rewritten header lines.
        assert!(new_content.contains("### Stage: Execute\r\n"));
        assert!(new_content.contains("### Outcome: Active\r\n"));
        // Accented body untouched.
        assert!(new_content.contains("Justificativa: configuração não pronta — ção ó é."));
        assert!(new_content.contains("Especificação — fase ó"));
    }

    #[test]
    fn header_region_stops_at_level2_heading() {
        let md = "# T\n### Status: approved\n### Phase: PLAN\n\n## Tarefas\n### Stage: Plan\n";
        // The header region covers the first four lines; the `### Stage:` after
        // `## Tarefas` is body, not a header.
        assert_eq!(header_region_lines(md), 4);
        assert!(header_field(md, "Stage").is_none(), "body Stage not a header");
        assert!(header_field(md, "Status").is_some());
    }

    #[test]
    fn header_region_stops_at_code_fence() {
        let md = "# T\n### Status: approved\n\n```text\n### Stage: Plan\n```\n";
        assert_eq!(header_region_lines(md), 3);
        assert!(header_field(md, "Stage").is_none());
    }

    #[test]
    fn plan_migrates_when_body_mentions_stage() {
        // Legacy HEADER + a `### Stage:` line inside a `## Tarefas` section.
        // BUG 1: must migrate the header, not skip as already-migrated.
        let md = "# T\n### Status: completed\n### Phase: CLOSE\n\n## Tarefas\n### Stage: Plan (exemplo)\nbody\n";
        let Plan::Migrate { new_content, .. } = plan_for(md) else {
            panic!("expected migrate, got skip");
        };
        assert!(new_content.contains("### Stage: Close"));
        assert!(new_content.contains("### Outcome: Completed"));
        // The body's documentary `### Stage: Plan (exemplo)` line is untouched.
        assert!(new_content.contains("### Stage: Plan (exemplo)"));
        assert!(!new_content.contains("### Status:"));
    }

    #[test]
    fn split_combined_status_extracts_segments() {
        let (status, extras) = split_combined_status("completed | Phase: CLOSE | Scope: light");
        assert_eq!(status, "completed");
        assert_eq!(extras.len(), 2);
        assert_eq!(extras[0], ("Phase".to_string(), "CLOSE".to_string()));
        assert_eq!(extras[1], ("Scope".to_string(), "light".to_string()));
    }

    #[test]
    fn plan_migrates_combined_pipe_line() {
        // BUG 2: status+phase(+scope) on one line.
        let md = "# T\n### Status: completed | Phase: CLOSE | Scope: light\n\nbody\n";
        let Plan::Migrate { new_content, .. } = plan_for(md) else {
            panic!("expected migrate");
        };
        assert!(new_content.contains("### Stage: Close"), "{new_content}");
        assert!(new_content.contains("### Outcome: Completed"), "{new_content}");
        assert!(new_content.contains("### Flags:"), "{new_content}");
        // The extra `Scope: light` segment is preserved as its own header line.
        assert!(new_content.contains("### Scope: light"), "{new_content}");
        assert!(!new_content.contains("| Phase:"), "{new_content}");
        // Idempotent: a second pass skips (now has a real `### Stage:`).
        assert!(matches!(plan_for(&new_content), Plan::AlreadyMigrated));
    }

    #[test]
    fn resolve_queued_with_parenthetical_phase() {
        // BUG 3: sub-plan `queued` + `QA (plano)`.
        let r = resolve(Some("queued"), Some("QA (plano)")).expect("resolves");
        // Documented choice: a queued item has NOT started, so it stays Plan.
        assert_eq!(r.stage, Stage::Plan);
        assert_eq!(r.outcome, Outcome::Active);
    }

    #[test]
    fn parse_stage_tolerant_strips_parenthetical() {
        assert_eq!(parse_stage_tolerant("QA (plano)"), Some(Stage::QaReview));
        assert_eq!(parse_stage_tolerant("REVIEW (plano)"), Some(Stage::QaReview));
        assert_eq!(parse_stage_tolerant("PLAN (plano)"), Some(Stage::Plan));
        assert_eq!(parse_stage_tolerant("queued"), Some(Stage::Plan));
    }
}
