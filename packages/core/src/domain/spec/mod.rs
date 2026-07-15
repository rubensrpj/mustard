// SPEC LANG: pt-allowed — test fixtures cover pt-BR spec serialisation round-trips.
//! `spec` — the single canonical owner of **spec-document file I/O**:
//! reading, parsing, serializing and (atomically) rewriting the lifecycle
//! header (`### Stage:` / `### Outcome:` / `### Flags:`) of a Mustard spec
//! `.md` file.
//!
//! This module sits *on top of* [`crate::io::fs`]: its file-touching wrappers
//! ([`read_state`], [`write_state`]) route through the canonical filesystem
//! seam ([`fs::read_to_string`](crate::io::fs::read_to_string),
//! [`fs::write_atomic`](crate::io::fs::write_atomic)) rather than calling
//! `std::fs` directly. The pure `&str` parse / serialize cores are unchanged.
//!
//! ## Why this module exists
//!
//! Wave 7 of `spec-lifecycle-unification` migrated 166 spec headers from the
//! legacy `### Status:` + `### Phase:` shape to the new three-line canonical
//! form. Before this module, ~17 `mustard-rt` subcommands each carried their
//! *own* inline `### Status:` parser (and several their own header *writers*),
//! so the migration broke every one of them at once (`wave-tree` showed every
//! wave "queued", `pipeline-summary` showed "unknown"). The root cause was that
//! the spec-header format was parsed and written in a dozen-plus duplicated
//! sites. This module is the permanent single home: a new header format is a
//! change *here only* (Open/Closed).
//!
//! ## Design (SOLID)
//!
//! - **Single Responsibility.** This module knows the spec-document header
//!   format and nothing else. It does not open event stores, run pipelines, or
//!   render UIs.
//! - **Open/Closed.** The tolerant parser ([`parse_state`]) accepts every
//!   historical header shape (new three-line, legacy `### Status:`+`### Phase:`,
//!   combined-pipe `### Status: X | Phase: Y | Scope: Z`, and bullet
//!   `- **Status**:`). A future format is added here, not in consumers.
//! - **Dependency Inversion / testability.** The parse + serialize core
//!   ([`parse_state`], [`serialize_header`], [`rewrite_header`]) are *pure
//!   functions on `&str`* — unit-testable with zero filesystem. The
//!   file-touching wrappers ([`read_state`], [`write_state`]) are thin shells
//!   over those pure cores plus a fail-open read / atomic write.
//!
//! ## Inviolable safety contract
//!
//! - **Fail-open.** A missing / unreadable file yields `None`; an unparseable
//!   header yields `None`. Nothing here ever panics, even on hostile content —
//!   a hook running this on a malformed spec must never crash a session.
//! - **CRLF + multibyte safe.** The rewrite operates on whole lines via
//!   [`str::split_inclusive`] and never indexes a string with `&s[a..b]` across
//!   a byte that might not be a char boundary (which would panic). Accented
//!   UTF-8 bodies and `\r\n` terminators are preserved byte-for-byte.
//! - **Atomic writes.** [`write_state`] writes a sibling tempfile then renames
//!   over the target, so a crash mid-write never leaves a half-written spec.
//!
//! The legacy detection logic (separate / combined / bullet header shapes,
//! header-region scoping) was lifted verbatim from the since-retired
//! `migrate-spec-headers` run command, which then depended on this module —
//! proving the single-source-of-truth property.

use crate::domain::model::view::{Flags, Outcome, SpecState, Stage};
use std::path::Path;

// Byte-stable spec layout contract — Wave 1, `2026-05-25-mustard-deep-refactor`.
// Public API entry point: `validate(&SpecInput)`. Lives in its own submodule so
// the historical header parser/serializer above stays the single owner of
// header IO without bloating with the contract surface.
pub mod contract;

// `## Funções tocadas` canonical-format parser — Wave 0,
// `2026-05-27-mustard-v4-foundation` (Spec A). Owns parsing, validation, and
// the fallback resolver consumed by W2 (snapshot) and W4 (gate). Lives here so
// the spec-document module is the single home of every parser that reads a
// `spec.md`; the body parser stays free of regression-check concerns. The
// module identifier is `touched_functions` (EN) to honour
// [`feedback_rust_identifiers_en_only`]; the section heading the parser
// matches against stays in PT-BR (`## Funções tocadas`) — that is content,
// not identifier.
pub mod touched_functions;

// ---------------------------------------------------------------------------
// Header-region scoping (tolerant, CRLF-safe)
// ---------------------------------------------------------------------------

/// The number of leading lines that make up the **header region** — the
/// contiguous metadata block at the top of a spec, *before* the body begins.
///
/// A spec header is a run of `### Key:` / `- **Key**:` lines (with blank lines
/// and a leading `# Title` allowed). The region ends at the first line that is
/// unmistakably body: a level-2 `## ` section heading, or the opening of a
/// fenced code block (```` ``` ````/`~~~`). Any `### Stage:`/`### Status:` that
/// appears *after* this point is prose or an example — never a real header — so
/// scoping header detection to `line_index < header_region_lines(content)`
/// stops the parser from being fooled by specs that document the new format.
#[must_use]
pub fn header_region_lines(content: &str) -> usize {
    let mut count = 0usize;
    for line in content.lines() {
        let t = line.trim_start();
        // A level-2 ATX heading or a fenced code block opener ends the header.
        if t.starts_with("## ") || t.starts_with("```") || t.starts_with("~~~") {
            break;
        }
        count += 1;
    }
    count
}

/// Strip a header prefix off a trimmed line, returning the value after the
/// `:`. Recognizes BOTH shapes, case-insensitively on the key:
///
/// - `### <Key>:` — the `###`-heading form.
/// - `- **<Key>**:` — the bullet-list form (`**`-bold key), as used by the
///   older `# Mustard 2.0 — Phase N` specs.
///
/// The returned value keeps its original casing and is trimmed. Char-boundary
/// safe: the only slices taken are at offsets derived from `strip_prefix`
/// results, which are always valid boundaries.
#[must_use]
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
                return rest.get(value_start..).map(|v| v.trim().to_string());
            }
        }
    }
    // `- **<Key>**:` bullet form.
    if let Some(rest) = t.strip_prefix("- **").or_else(|| t.strip_prefix("-\t**")) {
        let lower = rest.to_ascii_lowercase();
        if let Some(after_key) = lower.strip_prefix(&want) {
            let after_key = after_key.trim_start();
            if let Some(after_bold) = after_key.strip_prefix("**") {
                let after_bold = after_bold.trim_start();
                if let Some(after_colon) = after_bold.strip_prefix(':') {
                    let value_start = rest.len() - after_colon.len();
                    return rest.get(value_start..).map(|v| v.trim().to_string());
                }
            }
        }
    }
    None
}

/// The value of a header `<Key>` line (either shape; case-insensitive on the
/// key), trimmed — searched only inside the **header region** so a key
/// mentioned in prose or a code fence is not mistaken for the header. Returns
/// `None` when the key is absent.
#[must_use]
pub fn header_field(spec_md: &str, key: &str) -> Option<String> {
    let region = header_region_lines(spec_md);
    spec_md
        .lines()
        .take(region)
        .find_map(|line| strip_header_key(line, key))
}

/// `true` when a trimmed line is a header for `key` in either shape
/// (case-insensitive). Used to identify the lines to replace on rewrite.
#[must_use]
fn is_header_line(line: &str, key: &str) -> bool {
    strip_header_key(line, key).is_some()
}

// ---------------------------------------------------------------------------
// Token normalisation (lifted from migrate_spec_headers)
// ---------------------------------------------------------------------------

/// Normalise a header *value* down to the single leading token that the strict
/// core enums understand. Sub-plan files write decorated values like
/// `QA (plano)`, `REVIEW (plano)` or `completed | Phase: CLOSE | Scope: light`;
/// we take the leading token before any `(` or `|`. Casing is preserved (the
/// core parsers lowercase internally).
#[must_use]
fn value_token(raw: &str) -> String {
    raw.split(['(', '|'])
        .next()
        .unwrap_or(raw)
        .trim()
        .to_string()
}

/// Tolerant [`Stage::parse`]: strips a trailing parenthetical / pipe segment
/// and recognises the sub-plan `queued` sentinel as a not-yet-started Plan item.
#[must_use]
fn parse_stage_tolerant(raw: &str) -> Option<Stage> {
    let token = value_token(raw);
    if token.eq_ignore_ascii_case("queued") {
        return Some(Stage::Plan);
    }
    Stage::parse(&token)
}

/// Tolerant [`Outcome::parse`]: strips the parenthetical / pipe tail. `queued`
/// is a not-yet-started item carrying the non-terminal `Active` outcome (the
/// caller's default), hence `None` here.
#[must_use]
fn parse_outcome_tolerant(raw: &str) -> Option<Outcome> {
    Outcome::parse(&value_token(raw))
}

/// Whether a legacy `Status` token is terminal (Completed / Cancelled /
/// Abandoned) — a terminal status wins over any `Phase`.
#[must_use]
fn terminal_outcome(status: &str) -> Option<Outcome> {
    match parse_outcome_tolerant(status) {
        Some(o) if o != Outcome::Active => Some(o),
        _ => None,
    }
}

/// Map a legacy `Status` token onto the qualifier-flags it implies, returning
/// `(flags, is_pure_qualifier)`. A pure qualifier (`blocked`/`paused`/
/// `wave-failed`) lets `Phase` decide the stage.
#[must_use]
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

/// Split a combined single-line header value into its pipe-separated segments.
///
/// Older specs cram the whole header onto the `### Status:` line:
/// `### Status: completed | Phase: CLOSE | Scope: light`. Returns
/// `(status_value, extra_segments)` where each extra is `(key, value)`.
#[must_use]
fn split_combined(value: &str) -> (String, Vec<(String, String)>) {
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

// ---------------------------------------------------------------------------
// Resolution: (status, phase) tokens → SpecState  (lifted from migrate)
// ---------------------------------------------------------------------------

/// Resolve a [`SpecState`] from legacy `status` / `phase` tokens per the
/// wave-plan mapping table. Returns `None` only when neither token yields any
/// signal. The result is always a *legal* [`SpecState`] (passes
/// [`SpecState::new`]); illegal triples are clamped to the nearest legal one.
#[must_use]
fn resolve(status: Option<&str>, phase: Option<&str>) -> Option<SpecState> {
    let phase_stage = phase.and_then(parse_stage_tolerant);

    // 0. `queued` sub-plan sentinel: not-yet-started Plan item.
    if let Some(status) = status {
        if value_token(status).eq_ignore_ascii_case("queued") {
            return Some(state_or_fallback(Stage::Plan, Outcome::Active, Flags::default()));
        }
    }

    // 1. Terminal status wins outright — only legal at Close.
    if let Some(status) = status {
        if let Some(outcome) = terminal_outcome(status) {
            return Some(state_or_fallback(Stage::Close, outcome, Flags::default()));
        }
    }

    // 2. closed-followup: Close + Active + followup_open.
    if let Some(status) = status {
        let lower = value_token(status).to_ascii_lowercase();
        if matches!(lower.as_str(), "closed-followup" | "closed_followup") {
            return Some(state_or_fallback(
                Stage::Close,
                Outcome::Active,
                Flags {
                    followup_open: true,
                    ..Flags::default()
                },
            ));
        }
    }

    // 3. Qualifier status (blocked / wave-failed): Phase decides stage,
    //    status becomes a flag, Outcome stays Active.
    if let Some(status) = status {
        let (flags, pure) = qualifier_flags(status);
        if pure {
            let default_stage = if flags.wave_failed {
                Stage::Execute
            } else {
                Stage::Plan
            };
            let stage = phase_stage.unwrap_or(default_stage);
            // wave_failed is only legal at Execute — clamp.
            let stage = if flags.wave_failed { Stage::Execute } else { stage };
            return Some(state_or_fallback(stage, Outcome::Active, flags));
        }
    }

    // 4. Non-terminal status: Phase refines, Status decides Outcome (Active).
    let status_stage = status.and_then(parse_stage_tolerant);
    let stage = phase_stage.or(status_stage)?;
    Some(state_or_fallback(stage, Outcome::Active, Flags::default()))
}

/// Build a legal [`SpecState`], degrading to the earliest-meaningful state
/// (`Plan` + `Active`) rather than returning `None` on an illegal triple.
#[must_use]
fn state_or_fallback(stage: Stage, outcome: Outcome, flags: Flags) -> SpecState {
    SpecState::new(stage, outcome, flags).unwrap_or_else(|_| SpecState {
        stage: Stage::Plan,
        outcome: Outcome::Active,
        flags: Flags::default(),
    })
}

// ---------------------------------------------------------------------------
// Public: parse
// ---------------------------------------------------------------------------

/// Parse the lifecycle [`SpecState`] out of a spec-document's text.
///
/// Tolerant of every historical header shape — header-region scoped so a
/// `### Stage:` written in the body (prose / example) is **not** read as the
/// header. Returns `None` when no recognisable lifecycle header exists (a
/// non-spec file or one with only a `### Parent:` etc.).
///
/// Resolution precedence:
/// 1. The new `### Stage:` / `### Outcome:` / `### Flags:` triple (the canonical
///    form). When `### Stage:` is present it is authoritative.
/// 2. Otherwise the legacy `### Status:` (+ optional `### Phase:`), including the
///    combined-pipe and `- **Status**:` bullet shapes.
#[must_use]
pub fn parse_state(spec_md: &str) -> Option<SpecState> {
    // --- New canonical form: a `### Stage:` or `### Outcome:` line marks the
    //     new header. `### Stage:` is authoritative for the position; when only
    //     `### Outcome:` is present (a degenerate but valid header), the stage
    //     is inferred from the outcome (terminal ⇒ Close, else Plan).
    let stage_raw = header_field(spec_md, "Stage");
    let outcome_raw = header_field(spec_md, "Outcome");
    if stage_raw.is_some() || outcome_raw.is_some() {
        let outcome = outcome_raw
            .as_deref()
            .and_then(Outcome::parse)
            .unwrap_or(Outcome::Active);
        let stage = match stage_raw.as_deref().and_then(parse_stage_tolerant) {
            Some(s) => s,
            // No explicit stage: a terminal outcome can only sit at Close
            // (SpecState::new enforces this); a non-terminal one defaults to Plan.
            None if outcome != Outcome::Active => Stage::Close,
            None => Stage::Plan,
        };
        let flags = header_field(spec_md, "Flags")
            .map(|f| Flags::parse(&f))
            .unwrap_or_default();
        return Some(state_or_fallback(stage, outcome, flags));
    }

    // --- Legacy form: `### Status:` (+ `### Phase:`). ---
    let raw_status = header_field(spec_md, "Status");
    let mut phase = header_field(spec_md, "Phase");
    if raw_status.is_none() && phase.is_none() {
        return None;
    }

    // Combined single-line: `### Status: completed | Phase: CLOSE | ...`.
    let status = if let Some(raw) = raw_status.as_deref() {
        let (lead, segs) = split_combined(raw);
        for (k, v) in segs {
            if k.eq_ignore_ascii_case("phase") && phase.is_none() {
                phase = Some(v);
            }
        }
        Some(lead)
    } else {
        None
    };

    resolve(status.as_deref(), phase.as_deref())
}

/// Read and parse the lifecycle [`SpecState`] of a spec file at `path`.
/// Fail-open: a missing / unreadable file or an unparseable header → `None`.
#[must_use]
pub fn read_state(path: &Path) -> Option<SpecState> {
    let content = crate::io::fs::read_to_string(path).ok()?;
    parse_state(&content)
}

// ---------------------------------------------------------------------------
// Public: serialize + status word
// ---------------------------------------------------------------------------

/// The `TitleCase` header spelling of a [`Stage`] (round-trips through the
/// case-insensitive [`Stage::parse`]).
#[must_use]
pub fn stage_label(stage: Stage) -> &'static str {
    match stage {
        Stage::Analyze => "Analyze",
        Stage::Plan => "Plan",
        Stage::Execute => "Execute",
        Stage::QaReview => "QaReview",
        Stage::Close => "Close",
    }
}

/// The `TitleCase` header spelling of an [`Outcome`].
#[must_use]
pub fn outcome_label(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Active => "Active",
        Outcome::Completed => "Completed",
        Outcome::Cancelled => "Cancelled",
        Outcome::Abandoned => "Abandoned",
        Outcome::Superseded => "Superseded",
        Outcome::Absorbed => "Absorbed",
    }
}

/// The comma-separated canonical flag tokens of a [`Flags`] (empty string when
/// no flag is set).
#[must_use]
pub fn flags_label(flags: &Flags) -> String {
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

/// Serialize a [`SpecState`] into the three canonical header lines (no trailing
/// newline on each — the caller decides terminators).
#[must_use]
pub fn serialize_header(state: &SpecState) -> [String; 3] {
    [
        format!("### Stage: {}", stage_label(state.stage)),
        format!("### Outcome: {}", outcome_label(state.outcome)),
        format!("### Flags: {}", flags_label(&state.flags)),
    ]
}

/// A single lowercase "status word" projected from a [`SpecState`] —
/// `"completed"` / `"cancelled"` / `"abandoned"` / `"closed-followup"` /
/// `"blocked"` / `"wave-failed"` / or the active stage word
/// (`"plan"` / `"implementing"` / `"qa"` / …).
///
/// This is the **compatibility accessor** for consumers that map a status
/// string to an icon (`wave-tree`, `pipeline-summary`). New consumers should
/// switch on [`SpecState::stage`] / [`SpecState::outcome`] directly — the word
/// is provided only where it minimises churn against the old string-keyed code.
///
/// The active-stage spellings (`implementing`, `qa`) match the legacy
/// `### Status:` vocabulary the old icon maps keyed off, so the rendered output
/// is byte-identical to pre-migration behaviour. `Superseded` / `Absorbed`
/// project to `"completed"` — the retired flat vocabulary had no dedicated
/// words for either, and both are "work that survives elsewhere".
#[must_use]
pub fn status_word(state: &SpecState) -> &'static str {
    match state.outcome {
        Outcome::Completed | Outcome::Superseded | Outcome::Absorbed => "completed",
        Outcome::Cancelled => "cancelled",
        Outcome::Abandoned => "abandoned",
        Outcome::Active => {
            if state.flags.blocked {
                "blocked"
            } else if state.flags.wave_failed {
                "wave-failed"
            } else if state.flags.followup_open {
                "closed-followup"
            } else {
                match state.stage {
                    Stage::Analyze | Stage::Plan => "planning",
                    Stage::Execute => "implementing",
                    Stage::QaReview => "qa",
                    Stage::Close => "closed-followup",
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Public: rewrite (pure) + write (atomic)
// ---------------------------------------------------------------------------

/// Rewrite `content` so its lifecycle header reads `state`, returning the new
/// content. **Pure** — no I/O. Byte-stable: every non-header byte (CRLF
/// terminators, indentation, accented UTF-8) is preserved exactly.
///
/// Behaviour:
/// - Replaces the **first** legacy/new header line in the header region
///   (`### Stage:` / `### Outcome:` / `### Flags:` / `### Status:` / `### Phase:`,
///   whichever appears first) in place with the three canonical lines, and drops
///   any further such header line.
/// - When the spec has **no** lifecycle header at all, the three lines are
///   inserted directly after the leading `# Title` (or at the very top when
///   there is no title), so a freshly scaffolded spec gains a valid header.
/// - A `### Stage:`/`### Status:` appearing in the *body* (outside the header
///   region) is copied verbatim like any other line.
#[must_use]
pub fn rewrite_header(content: &str, state: &SpecState) -> String {
    let region = header_region_lines(content);
    let lines = serialize_header(state);
    let mut out = String::with_capacity(content.len() + 64);
    let mut placed = false;

    // Determine whether any lifecycle header exists in the region.
    let has_header = content
        .lines()
        .take(region)
        .any(|l| {
            is_header_line(l, "Stage")
                || is_header_line(l, "Outcome")
                || is_header_line(l, "Flags")
                || is_header_line(l, "Status")
                || is_header_line(l, "Phase")
        });

    // Track the title line so we can insert after it when there is no header.
    // `inserted_after_title` only matters on the no-header insert path.
    let mut inserted_after_title = false;

    for (idx, seg) in content.split_inclusive('\n').enumerate() {
        let body = seg.trim_end_matches(['\n', '\r']);
        let terminator = seg.get(body.len()..).unwrap_or("");
        let term = if terminator.is_empty() { "\n" } else { terminator };
        let in_region = idx < region;

        // Replace-in-place path: an existing lifecycle header line.
        if has_header
            && in_region
            && (is_header_line(body, "Stage")
                || is_header_line(body, "Outcome")
                || is_header_line(body, "Flags")
                || is_header_line(body, "Status")
                || is_header_line(body, "Phase"))
        {
            if !placed {
                let indent_len = body.len() - body.trim_start().len();
                let indent = body.get(..indent_len).unwrap_or("");
                for line in &lines {
                    out.push_str(indent);
                    out.push_str(line);
                    out.push_str(term);
                }
                placed = true;
            }
            // Drop this legacy/new header line (replaced above / removed).
            continue;
        }

        out.push_str(seg);

        // Insert path: no header existed — emit the three lines right after the
        // first `# Title` line (level-1 ATX heading), else they go at the top.
        if !has_header && !inserted_after_title && body.trim_start().starts_with("# ") {
            for line in &lines {
                out.push_str(line);
                out.push_str(term);
            }
            inserted_after_title = true;
            placed = true;
        }
    }

    // No header and no title encountered: prepend the canonical lines.
    if !placed {
        let mut head = String::with_capacity(content.len() + 64);
        for line in &lines {
            head.push_str(line);
            head.push('\n');
        }
        head.push_str(&out);
        return head;
    }

    out
}

/// Atomically write `state`'s header into the spec file at `path`, preserving
/// every other byte. A sibling tempfile is written + flushed, then renamed over
/// the target — a crash before the rename leaves the original untouched.
///
/// Fail-open: a missing / unreadable file is a no-op returning `Ok(())`. Returns
/// the IO error only on a real write/rename failure.
///
/// # Errors
///
/// Returns the underlying [`std::io::Error`] when the tempfile cannot be
/// written or the rename fails. A *read* failure (missing file) is **not** an
/// error — it is a fail-open no-op.
pub fn write_state(path: &Path, state: &SpecState) -> std::io::Result<()> {
    let Ok(content) = crate::io::fs::read_to_string(path) else {
        // Fail-open: nothing to rewrite (missing / unreadable file).
        return Ok(());
    };
    let new_content = rewrite_header(&content, state);
    // Route through the canonical filesystem seam (tempfile + rename). Map the
    // crate error back to `io::Error` to keep this wrapper's public signature
    // stable for the consumers fixed in the next pass.
    crate::io::fs::write_atomic(path, new_content.as_bytes())
        .map_err(|e| std::io::Error::other(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse: new canonical form -----------------------------------------

    #[test]
    fn parse_new_three_line_header() {
        let md = "# Spec\n### Stage: Execute\n### Outcome: Active\n### Flags: \n\nbody\n";
        let st = parse_state(md).expect("parses");
        assert_eq!(st.stage, Stage::Execute);
        assert_eq!(st.outcome, Outcome::Active);
        assert!(!st.flags.blocked);
    }

    #[test]
    fn parse_new_header_with_flag() {
        let md = "# S\n### Stage: Close\n### Outcome: Active\n### Flags: followup_open\n";
        let st = parse_state(md).expect("parses");
        assert_eq!(st.stage, Stage::Close);
        assert!(st.flags.followup_open);
    }

    #[test]
    fn parse_new_terminal_outcome() {
        let md = "# S\n### Stage: Close\n### Outcome: Completed\n### Flags: \n";
        let st = parse_state(md).expect("parses");
        assert_eq!(st.outcome, Outcome::Completed);
        assert!(st.is_terminal());
    }

    // --- parse: legacy forms -----------------------------------------------

    #[test]
    fn parse_legacy_separate_status_phase() {
        let md = "# S\n### Status: approved\n### Phase: EXECUTE\n";
        let st = parse_state(md).expect("parses");
        assert_eq!(st.stage, Stage::Execute);
        assert_eq!(st.outcome, Outcome::Active);
    }

    #[test]
    fn parse_legacy_terminal_status_wins() {
        let md = "# S\n### Status: completed\n### Phase: EXECUTE\n";
        let st = parse_state(md).expect("parses");
        assert_eq!(st.stage, Stage::Close);
        assert_eq!(st.outcome, Outcome::Completed);
    }

    #[test]
    fn parse_legacy_combined_pipe_line() {
        let md = "# S\n### Status: completed | Phase: CLOSE | Scope: light\n\nbody\n";
        let st = parse_state(md).expect("parses");
        assert_eq!(st.stage, Stage::Close);
        assert_eq!(st.outcome, Outcome::Completed);
    }

    #[test]
    fn parse_legacy_bullet_form() {
        let md = "# Mustard 2.0 — Phase 3\n- **Status**: implementing\n";
        let st = parse_state(md).expect("parses");
        assert_eq!(st.stage, Stage::Execute);
    }

    #[test]
    fn parse_legacy_closed_followup() {
        let md = "# S\n### Status: closed-followup\n";
        let st = parse_state(md).expect("parses");
        assert_eq!(st.stage, Stage::Close);
        assert!(st.flags.followup_open);
    }

    // --- parse: header-region scoping (the Stage-in-body case) -------------

    #[test]
    fn parse_ignores_stage_mentioned_in_body() {
        // Legacy header + a `### Stage:` documented inside a `## ` section.
        // The body line must NOT be read as the header.
        let md = "# T\n### Status: completed\n### Phase: CLOSE\n\n## Tasks\n### Stage: Plan (exemplo)\nbody\n";
        let st = parse_state(md).expect("parses");
        // Resolved from the *header* (completed), not the body Stage: Plan.
        assert_eq!(st.stage, Stage::Close);
        assert_eq!(st.outcome, Outcome::Completed);
    }

    #[test]
    fn parse_ignores_stage_in_code_fence() {
        let md = "# T\n### Status: approved\n\n```text\n### Stage: Close\n```\n";
        let st = parse_state(md).expect("parses");
        // Header region stops at the fence; the body Stage: Close is ignored.
        assert_eq!(st.stage, Stage::Plan); // approved → Plan
    }

    #[test]
    fn parse_returns_none_without_lifecycle_header() {
        assert!(parse_state("# Just a title\n\nbody\n").is_none());
        assert!(parse_state("### Parent: epic-x\n").is_none());
    }

    // --- serialize + status word -------------------------------------------

    #[test]
    fn serialize_emits_three_canonical_lines() {
        let st = SpecState::new(Stage::Execute, Outcome::Active, Flags::default()).unwrap();
        let lines = serialize_header(&st);
        assert_eq!(lines[0], "### Stage: Execute");
        assert_eq!(lines[1], "### Outcome: Active");
        assert_eq!(lines[2], "### Flags: ");
    }

    #[test]
    fn status_word_projects_active_stages_and_terminals() {
        let plan = SpecState::new(Stage::Plan, Outcome::Active, Flags::default()).unwrap();
        assert_eq!(status_word(&plan), "planning");
        let exec = SpecState::new(Stage::Execute, Outcome::Active, Flags::default()).unwrap();
        assert_eq!(status_word(&exec), "implementing");
        let qa = SpecState::new(Stage::QaReview, Outcome::Active, Flags::default()).unwrap();
        assert_eq!(status_word(&qa), "qa");
        let done = SpecState::new(Stage::Close, Outcome::Completed, Flags::default()).unwrap();
        assert_eq!(status_word(&done), "completed");
        let followup = SpecState::new(
            Stage::Close,
            Outcome::Active,
            Flags {
                followup_open: true,
                ..Flags::default()
            },
        )
        .unwrap();
        assert_eq!(status_word(&followup), "closed-followup");
    }

    // --- rewrite: in-place replacement, byte-stable ------------------------

    #[test]
    fn rewrite_replaces_legacy_header_in_place_preserving_order() {
        let md = "# Spec\n### Parent: [[epic]]\n### Status: approved\n### Phase: EXECUTE\n### Lang: pt\n\nbody\n";
        let st = parse_state(md).unwrap();
        let out = rewrite_header(md, &st);
        assert!(out.contains("### Parent: [[epic]]"));
        assert!(out.contains("### Stage: Execute"));
        assert!(out.contains("### Outcome: Active"));
        assert!(out.contains("### Lang: pt"));
        assert!(!out.contains("### Status:"));
        assert!(!out.contains("### Phase:"));
        let parent = out.find("### Parent:").unwrap();
        let stage = out.find("### Stage:").unwrap();
        let lang = out.find("### Lang:").unwrap();
        assert!(parent < stage && stage < lang);
    }

    #[test]
    fn rewrite_is_crlf_and_accent_byte_safe() {
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
        let st = parse_state(&md).unwrap();
        let out = rewrite_header(&md, &st);
        assert!(out.contains("### Stage: Execute\r\n"));
        assert!(out.contains("### Outcome: Active\r\n"));
        assert!(out.contains("Justificativa: configuração não pronta — ção ó é."));
        assert!(out.contains("Especificação — fase ó"));
        // Idempotent: re-parsing yields the same state.
        assert_eq!(parse_state(&out), Some(st));
    }

    #[test]
    fn rewrite_changes_state_value() {
        let md = "# S\n### Stage: Plan\n### Outcome: Active\n### Flags: \n";
        let done = SpecState::new(Stage::Close, Outcome::Completed, Flags::default()).unwrap();
        let out = rewrite_header(md, &done);
        assert!(out.contains("### Stage: Close"));
        assert!(out.contains("### Outcome: Completed"));
        assert_eq!(parse_state(&out), Some(done));
    }

    #[test]
    fn rewrite_inserts_header_after_title_when_absent() {
        let md = "# Fresh Spec\n\nsome body\n";
        let st = SpecState::new(Stage::Plan, Outcome::Active, Flags::default()).unwrap();
        let out = rewrite_header(md, &st);
        assert!(out.contains("### Stage: Plan"));
        // Title still first.
        assert!(out.find("# Fresh Spec").unwrap() < out.find("### Stage:").unwrap());
        assert!(out.contains("some body"));
        assert_eq!(parse_state(&out), Some(st));
    }

    #[test]
    fn rewrite_does_not_touch_body_stage_mention() {
        let md = "# T\n### Status: completed\n### Phase: CLOSE\n\n## Tasks\n### Stage: Plan (exemplo)\n";
        let st = parse_state(md).unwrap();
        let out = rewrite_header(md, &st);
        assert!(out.contains("### Stage: Close")); // the header
        assert!(out.contains("### Stage: Plan (exemplo)")); // the body, untouched
    }

    // --- file wrappers (fail-open) -----------------------------------------

    #[test]
    fn read_state_missing_file_is_none() {
        let p = std::path::Path::new("/nonexistent/spec/path/spec.md");
        assert!(read_state(p).is_none());
    }

    #[test]
    fn write_state_missing_file_is_noop_ok() {
        let p = std::path::Path::new("/nonexistent/spec/path/spec.md");
        let st = SpecState::new(Stage::Plan, Outcome::Active, Flags::default()).unwrap();
        assert!(write_state(p, &st).is_ok());
    }

    #[test]
    fn write_state_round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spec.md");
        crate::io::fs::write_atomic(
            &path,
            b"# S\n### Status: approved\n### Phase: EXECUTE\n\nbody\n",
        )
        .unwrap();
        let done = SpecState::new(Stage::Close, Outcome::Completed, Flags::default()).unwrap();
        write_state(&path, &done).unwrap();
        assert_eq!(read_state(&path), Some(done));
        // Body preserved.
        assert!(crate::io::fs::read_to_string(&path).unwrap().contains("body"));
    }
}
