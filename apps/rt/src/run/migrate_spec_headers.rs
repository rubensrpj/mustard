// SPEC LANG: pt-allowed — test fixtures use pt-BR spec content for round-trip assertions.
//! `mustard-rt run migrate-spec-headers` — the terminal spec-header migration.
//!
//! ## Retirement note (Wave 3 of mustard-unification, 2026-05-24)
//!
//! Wave 3 of `2026-05-24-mustard-unification` moved every machine-parseable
//! lifecycle field (`Stage`/`Outcome`/`Flags`/`Phase`/`Scope`/`Lang`/
//! `Checkpoint`/`Parent`/`Total waves`) from in-`.md` headers into a sidecar
//! `meta.json`, and the second-pass `mustard-rt run migrate-to-meta
//! --strip-headers` removed them from every existing `.md`. This module is no
//! longer used by the canonical pipeline — its **subcommand stays wired** as a
//! retroactive tool for one-off audits / rescues of git-history specs that
//! still carry the old shape (e.g. a co-worker's branch that has not been
//! re-migrated yet). Once that scenario disappears the module can be deleted
//! in a follow-up; until then, every public function and test stays unchanged.
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

use mustard_core::fs;
use mustard_core::spec::{
    self, flags_label, header_field, outcome_label, stage_label,
};
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
// Header extraction (tolerant, CRLF-safe) — delegated to `mustard_core::spec`
//
// The header-region scoping + the `### Key:` / `- **Key**:` tolerant extraction
// + the byte-stable rewrite all live in the canonical core module now; this
// subcommand only adds the dry-run/audit envelope on top.
// ---------------------------------------------------------------------------

/// Split a combined single-line header value into its pipe-separated segments,
/// returning the trailing non-`Phase` `Key: value` extras (e.g. `Scope: light`)
/// so the migration can preserve them as their own header lines — information
/// the canonical three-line header does not itself carry.
fn combined_extras(value: &str) -> Vec<(String, String)> {
    value
        .split('|')
        .skip(1)
        .filter_map(|seg| {
            let seg = seg.trim();
            seg.split_once(':')
                .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        })
        .filter(|(k, _)| !k.eq_ignore_ascii_case("phase"))
        .collect()
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
/// Detection, header-region scoping, the tolerant legacy parse and the
/// byte-stable rewrite are all delegated to [`mustard_core::spec`] — this
/// function only adds the audit-specific bits (the `inferred_stage_override`
/// note and combined-line `Scope:`-style extras preservation).
fn plan_for(content: &str) -> Plan {
    // Already migrated: the canonical `### Stage:` header exists (region-scoped
    // by core, so a body `### Stage:` does not count).
    if header_field(content, "Stage").is_some() {
        return Plan::AlreadyMigrated;
    }
    let raw_status = header_field(content, "Status");
    let mut phase = header_field(content, "Phase");
    if raw_status.is_none() && phase.is_none() {
        return Plan::NoStatusHeader;
    }

    // Combined single-line form: `### Status: completed | Phase: CLOSE | ...`.
    // Pull a `Phase` segment out when there is no separate `### Phase:` line,
    // and remember every other segment (e.g. `Scope: light`) so we can re-emit
    // it as its own canonical header line.
    let extras: Vec<(String, String)> = raw_status
        .as_deref()
        .map(combined_extras)
        .unwrap_or_default();
    if phase.is_none() {
        if let Some(raw) = raw_status.as_deref() {
            if let Some(seg) = raw
                .split('|')
                .skip(1)
                .map(str::trim)
                .find(|s| s.to_ascii_lowercase().starts_with("phase:"))
            {
                if let Some((_, v)) = seg.split_once(':') {
                    phase = Some(v.trim().to_string());
                }
            }
        }
    }

    // The leading status token (before any `|`), for the audit `before`.
    let status = raw_status
        .as_deref()
        .map(|raw| raw.split('|').next().unwrap_or(raw).trim().to_string());

    // The audit override note still needs the explicit resolution.
    let Some(resolved) = resolve(status.as_deref(), phase.as_deref()) else {
        return Plan::Malformed;
    };

    // The canonical state (single source of truth for the *value* written).
    let Some(state) = spec::parse_state(content) else {
        return Plan::Malformed;
    };

    // Core performs the byte-stable, CRLF/multibyte-safe header rewrite.
    let mut new_content = spec::rewrite_header(content, &state);

    // Preserve combined-line extras (e.g. `Scope: light`) as their own
    // canonical header lines, inserted right after the `### Flags:` line.
    if !extras.is_empty() {
        new_content = inject_extras(&new_content, &extras);
    }

    Plan::Migrate {
        resolved_status: raw_status,
        resolved_phase: phase,
        resolved,
        new_content,
    }
}

/// Insert `extras` as `### Key: value` header lines immediately after the
/// `### Flags:` line core emitted, reusing that line's terminator. Byte-stable:
/// the only mutation is the inserted lines. Fail-open: if no `### Flags:` line
/// is found the content is returned unchanged.
fn inject_extras(content: &str, extras: &[(String, String)]) -> String {
    let mut out = String::with_capacity(content.len() + 64);
    let mut injected = false;
    for seg in content.split_inclusive('\n') {
        out.push_str(seg);
        let body = seg.trim_end_matches(['\n', '\r']);
        if !injected && body.trim_start().starts_with("### Flags:") {
            let terminator = seg.get(body.len()..).unwrap_or("");
            let term = if terminator.is_empty() { "\n" } else { terminator };
            for (k, v) in extras {
                out.push_str("### ");
                out.push_str(k);
                out.push_str(": ");
                out.push_str(v);
                out.push_str(term);
            }
            injected = true;
        }
    }
    out
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
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = &entry.path;
        if entry.is_dir {
            collect_md_into(path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path.clone());
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

        let Ok(content) = fs::read_to_string(path) else {
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
                    match fs::write_atomic(path, new_content.as_bytes()) {
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
    let log_json = serde_json::to_string_pretty(&log).unwrap_or_else(|_| "{}".to_string());
    let log_written = fs::write_atomic(&log_path, log_json.as_bytes()).is_ok();

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
        assert_eq!(spec::header_region_lines(md),4);
        assert!(header_field(md, "Stage").is_none(), "body Stage not a header");
        assert!(header_field(md, "Status").is_some());
    }

    #[test]
    fn header_region_stops_at_code_fence() {
        let md = "# T\n### Status: approved\n\n```text\n### Stage: Plan\n```\n";
        assert_eq!(spec::header_region_lines(md),3);
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
    fn combined_extras_keeps_non_phase_segments() {
        // The `Phase` segment is consumed by the parser; the rest survive as
        // their own header lines.
        let extras = combined_extras("completed | Phase: CLOSE | Scope: light");
        assert_eq!(extras.len(), 1);
        assert_eq!(extras[0], ("Scope".to_string(), "light".to_string()));
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
