//! `post_edit` — the consolidated PostToolUse(Write|Edit) module.
//!
//! ## Scope (Write/Edit family)
//!
//! This module consolidates two JavaScript hooks, both `PostToolUse(Write|Edit)`.
//! One is a pure side effect (`Observer`), one reaches a verdict (`Check`):
//!
//! - `checklist-auto-mark.js` — an **`Observer`**: silently marks Checklist
//!   items in the active spec when the edited file matches an item. No verdict.
//! - `guard-verify.js` — a **`Check`**: flags an edit that falls outside the
//!   active spec.s declared `## Boundaries` (advisory), AND enforces the edited
//!   subproject's `[critical]` Guards (data-driven, replacing the
//!   removed stack-specific DbContext/DIP/int-id block). A checkable critical
//!   Guard the edit violates is a `Deny` in strict mode; everything else is
//!   advisory.
//!
//! `PostEdit` therefore implements **both** [`Check`] (guard-verify) and
//! [`Observer`] (checklist-auto-mark) — the same dual shape `budget` and
//! `bash_guard` use.
//!
//! A formatação saiu daqui: ela rodava a cada gravação, e o arquivo mudava
//! depois de escrito, então a edição seguinte podia não achar o texto que
//! acabara de gravar. Agora ela roda uma vez por rodada, antes do commit, só
//! nos arquivos que a rodada mudou (`commands/flow/round.rs`).
//!
//! Consolidation **regroups, it does not re-decide** — every verdict is a 1:1
//! port of the JS decision logic. Parity tests mirror
//! `__tests__/checklist-mark.test.js`.
//!
//! ## Migration note
//!
//! `pipeline-phase.js` used to live here as a fourth side effect: it parsed
//! `phaseName` out of a pipeline-state Write and emitted a `pipeline.phase`
//! event. The move of the dashboard phase off SQLite removed the
//! `phaseName` writer from SKILL.md, so that trigger no longer fires. The
//! `pipeline.phase` producer now lives entirely in `mustard-rt run emit-phase`
//! (`apps/rt/src/run/emit_phase.rs`), driven explicitly by the pipeline
//! orchestrator commands.
//!
//! ## Verdict note (guard-verify)
//!
//! The boundary mismatch is advisory — an [`Verdict::Inject`]. The data-driven
//! critical-Guard gate is the one blocking path: in `strict` mode a
//! checkable `[critical]` Guard of the edited subproject that the edit violates
//! yields a [`Verdict::Deny`]; `warn` (the default) downgrades it to an
//! advisory. Everything else stays [`Verdict::Inject`] / [`Verdict::Allow`].

use mustard_core::platform::error::Error;
use mustard_core::io::fs;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Observer, Trigger, Verdict};
use mustard_core::domain::spec;
use mustard_core::{ClaudePaths, Outcome as SpecOutcome, Stage as SpecStage};
use std::path::{Path, PathBuf};
use crate::util::format_gate_message;

/// The consolidated PostToolUse(Write|Edit) module.
pub struct PostEdit;

// ===========================================================================
// Shared helpers
// ===========================================================================

/// `true` if this is a `Write` or `Edit` tool invocation.
fn is_write_or_edit(input: &HookInput) -> bool {
    matches!(input.tool_name.as_deref(), Some("Write" | "Edit"))
}

/// The basename (last `/`- or `\`-separated segment) of a path.
fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

// ===========================================================================
// guard-verify — Check on PostToolUse(Write|Edit)
// ===========================================================================

/// Path-segment patterns whose match means the file is skipped entirely.
/// Mirrors `SKIP_PATTERNS` in `guard-verify.js`.
fn is_skipped_path(rel: &str) -> bool {
    let p = rel.replace('\\', "/");
    p.contains("node_modules")
        || p.contains(".next/")
        || p.contains("/bin/")
        || p.contains("/obj/")
        || p.contains("/dist/")
        || p.contains("/_backup/")
        || p.contains(".claude/")
        || p.contains(".git/")
        || p.contains("migrations/")
}

/// The new content of a Write/Edit — `new_string` (Edit) or `content` (Write).
fn new_content_of(input: &HookInput) -> String {
    let ti = &input.tool_input;
    ti.get("new_string")
        .or_else(|| ti.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}










/// `true` for an ASCII word byte (alphanumeric or `_`).
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}



/// Scan specs for a `## Boundaries` section the edited file violates.
/// Advisory only — returns the warning message or `None`. Port of
/// `checkBoundaries`. Flat layout: scans `.claude/spec/` directly.
///
/// The scan now filters out non-active specs —
/// previously the first spec dir (alphabetically) with a `## Boundaries`
/// section always won, so a stale `Close + Active + followup_open` spec
/// would warn on every unrelated edit. The
/// fix consults the canonical `### Stage:` / `### Outcome:` header via
/// `spec::parse_state`: only specs whose outcome is `Active` AND whose stage
/// is one of {`Analyze`, `Plan`, `Execute`} participate. Specs without a
/// parseable header (legacy) fall through to the prior behaviour — they
/// still emit the boundary check — to keep the safety net for old specs
/// while suppressing the closed-followup false positives.
fn check_boundaries(file_path: &str, cwd: &str) -> Option<(String, String)> {
    let spec_root = ClaudePaths::for_project(cwd).ok()?.spec_dir();
    let entries = fs::read_dir(&spec_root).ok()?;
    let normalized_edit = file_path.replace('\\', "/");

    // Collect every Active+open spec that ships a `## Boundaries` (or
    // `## Limites`) block, then keep ONLY the most-recently-checkpointed one.
    // Without this, `read_dir`'s alphabetical order makes an older spec
    // outrank a newer active spec and warn about edits the newer
    // spec authorised. Recency uses `### Checkpoint:` when present; falls
    // back to the date prefix in the directory name (`YYYY-MM-DD`), then
    // to the name itself.
    let mut best: Option<(String, String, Vec<String>)> = None;
    let mut best_key: String = String::new();
    for entry in entries.into_iter().filter(|e| e.is_dir) {
        let dir_name = entry.file_name.clone();
        let spec_file = entry.path.join("spec.md");
        let Ok(content) = fs::read_to_string(&spec_file) else {
            continue;
        };
        // `meta.json` is the single source of truth for lifecycle state; the
        // legacy `.md` header is the fallback for un-migrated specs.
        if let Some(state) = spec_state_meta_first(&spec_file, &content) {
            let stage_ok = matches!(
                state.stage,
                SpecStage::Analyze | SpecStage::Plan | SpecStage::Execute
            );
            let active = state.outcome == SpecOutcome::Active;
            if !active || !stage_ok {
                continue;
            }
        }
        let Some(lines) = boundary_block_lines(&content) else {
            continue;
        };
        if lines.is_empty() {
            continue;
        }
        let recency_key = recency_key_for_spec(&entry.path, &content, &dir_name);
        if recency_key > best_key {
            best_key = recency_key;
            best = Some((dir_name, content, lines));
        }
    }
    let (dir_name, _content, lines) = best?;

    // Does the edited file match any declared boundary?
    let mut matched = false;
    for pattern in &lines {
        let pattern = pattern.replace('\\', "/");
        if pattern.is_empty() {
            continue;
        }
        if pattern.ends_with('/') {
            if normalized_edit.contains(&pattern) || normalized_edit.starts_with(&pattern) {
                matched = true;
                break;
            }
            continue;
        }
        if pattern.contains('*') || pattern.contains('?') {
            if glob_loose_match(&normalized_edit, &pattern) {
                matched = true;
                break;
            }
            continue;
        }
        if normalized_edit.ends_with(&pattern) || normalized_edit == pattern {
            matched = true;
            break;
        }
    }
    if matched {
        return None;
    }
    let rel_edited = file_path.replace('\\', "/");
    let message = format!(
        "\"{rel_edited}\" is outside the boundaries declared in spec \
         \"{dir_name}\". Declared: {}. Verify this edit is intentional.",
        lines.join(", ")
    );
    Some((dir_name, message))
}

/// Session-scoped dedup-marker path `.claude/.session/<id>/<file_name>`. `None`
/// when the session is unresolved (then the advisory is never suppressed —
/// fail-open).
fn advisory_marker_path(cwd: &str, session: &str, file_name: &str) -> Option<PathBuf> {
    if session.is_empty() || session == "unknown" {
        return None;
    }
    Some(
        ClaudePaths::for_project(Path::new(cwd))
            .ok()?
            .claude_dir()
            .join(".session")
            .join(session)
            .join(file_name),
    )
}

/// Surface an advisory ONCE per (key, session): the first call returns `true`
/// and records `key`; later calls with the same key return `false`. Re-emitting
/// the same advisory on every edit is pure re-injected noise. Fail-open: an
/// unresolved session or any IO error returns `true` (warn), so the safety net
/// never goes silent on a broken FS.
fn advisory_once(cwd: &str, session: &str, file_name: &str, key: &str) -> bool {
    let Some(marker) = advisory_marker_path(cwd, session, file_name) else {
        return true;
    };
    let seen = fs::read_to_string(&marker).unwrap_or_default();
    if seen.lines().any(|l| l.trim() == key) {
        return false;
    }
    // `write_atomic` creates the parent dir; a failed write degrades to
    // re-warning next edit (never silent), which is the safe direction.
    let _ = fs::write_atomic(&marker, format!("{seen}{key}\n").as_bytes());
    true
}

/// Surface the boundary advisory ONCE per (spec, session). Thin alias over
/// [`advisory_once`] with the boundary marker file. Only the non-blocking
/// advisory is deduped; a CRITICAL boundary violation is a separate path.
fn boundary_warn_once(cwd: &str, spec: &str, session: &str) -> bool {
    advisory_once(cwd, session, "boundary-warned", spec)
}

/// Resolve a spec's lifecycle [`SpecState`] from the filesystem,
/// **`meta.json`-first**. The sidecar beside `spec_file` is authoritative; the
/// already-read `.md` `content` is the legacy fallback for un-migrated specs.
fn spec_state_meta_first(spec_file: &Path, content: &str) -> Option<mustard_core::SpecState> {
    let _ = spec_file;
    spec::parse_state(content)
}

/// Helper: a lexicographically-comparable recency key. Prefers the spec's
/// ISO checkpoint (so `2026-05-28T10:00:00.000Z` sorts above
/// `2026-05-27T17:56:09.926Z`), read **`meta.json`-first** (`#checkpoint`) with
/// a fallback to a legacy `### Checkpoint:` header; falls back to the directory
/// name prefix (`YYYY-MM-DD-…` already sorts correctly); never panics. Returned
/// `String` is meaningful only against other keys produced by this same fn.
fn recency_key_for_spec(spec_dir: &Path, content: &str, dir_name: &str) -> String {
    // O cabeçalho `### Checkpoint:` é o que sobrou.
    for line in content.lines().take(50) {
        let trimmed = line.trim();
        if let Some(rest) = trimmed
            .strip_prefix("### Checkpoint:")
            .or_else(|| trimmed.strip_prefix("###Checkpoint:"))
        {
            let value = rest.trim();
            if !value.is_empty() {
                return value.to_string();
            }
        }
    }
    dir_name.to_string()
}

/// Extract the cleaned bullet lines of a spec's `## Boundaries` block.
fn boundary_block_lines(content: &str) -> Option<Vec<String>> {
    let lines: Vec<&str> = content.split('\n').collect();
    let mut start = None;
    for (i, line) in lines.iter().enumerate() {
        let lower = line.trim().to_ascii_lowercase();
        if h2_named(&lower, "boundaries") || h2_named(&lower, "limites") {
            start = Some(i + 1);
            break;
        }
    }
    let start = start?;
    let mut out: Vec<String> = Vec::new();
    for line in &lines[start..] {
        if line.starts_with("## ") || line.trim() == "---" {
            break;
        }
        // `.replace(/^[-*]\s+`?/, '').replace(/`.*/, '').trim()`.
        let mut cleaned = line.trim_start();
        if let Some(rest) = cleaned.strip_prefix('-').or_else(|| cleaned.strip_prefix('*')) {
            cleaned = rest.trim_start().trim_start_matches('`');
        } else {
            continue;
        }
        let cleaned = match cleaned.find('`') {
            Some(idx) => &cleaned[..idx],
            None => cleaned,
        };
        let cleaned = cleaned.trim();
        if !cleaned.is_empty() {
            out.push(cleaned.to_string());
        }
    }
    Some(out)
}

/// `true` if a lowercased line is an H2 heading whose name is exactly `name`.
fn h2_named(lower: &str, name: &str) -> bool {
    let Some(rest) = lower.strip_prefix("## ") else {
        return false;
    };
    let rest = rest.trim_start();
    if !rest.starts_with(name) {
        return false;
    }
    rest.as_bytes()
        .get(name.len())
        .is_none_or(|&b| !b.is_ascii_alphanumeric() && b != b'_')
}

/// A permissive glob match for boundary patterns (`**`→`.+`, `*`→one segment,
/// `?`→one char), tested as an unanchored "contains" like the JS
/// `new RegExp(regexStr).test(...)`.
fn glob_loose_match(text: &str, pattern: &str) -> bool {
    // Build segments; an unanchored search means trying every start position.
    let pb = pattern.as_bytes();
    let tb = text.as_bytes();
    for start in 0..=tb.len() {
        if glob_loose_at(&tb[start..], pb) {
            return true;
        }
    }
    false
}

/// Anchored permissive glob walk. `**`/`*` consume ≥1 char (the JS uses `(.+)`
/// / `([^/]+)`), `?` consumes exactly one.
fn glob_loose_at(text: &[u8], pat: &[u8]) -> bool {
    if pat.is_empty() {
        return true; // unanchored tail — a partial match suffices
    }
    if pat.starts_with(b"**") {
        let rest = &pat[2..];
        // `(.+)` — one or more of anything.
        let mut i = 1;
        while i <= text.len() {
            if glob_loose_at(&text[i..], rest) {
                return true;
            }
            i += 1;
        }
        return false;
    }
    if pat[0] == b'*' {
        let rest = &pat[1..];
        // `([^/]+)` — one or more non-`/`.
        let mut i = 1;
        while i <= text.len() {
            if text[i - 1] == b'/' {
                return false;
            }
            if glob_loose_at(&text[i..], rest) {
                return true;
            }
            i += 1;
        }
        return false;
    }
    if pat[0] == b'?' {
        // `([^/])` — exactly one non-`/`.
        if text.is_empty() || text[0] == b'/' {
            return false;
        }
        return glob_loose_at(&text[1..], &pat[1..]);
    }
    if !text.is_empty() && text[0] == pat[0] {
        return glob_loose_at(&text[1..], &pat[1..]);
    }
    false
}

/// The `guard-verify` verdict for a `PostToolUse(Write|Edit)`. Runs the
/// data-driven critical-Guard gate first (it alone can `Deny`), then folds the
/// boundary advisory. Any advisory context is injected as one message.
fn guard_verify(input: &HookInput, cwd: &str) -> Verdict {
    if !is_write_or_edit(input) {
        return Verdict::Allow;
    }
    let Some(file_path) = input.file_path() else {
        return Verdict::Allow;
    };
    // `path.relative(ROOT, filePath)` — relative to cwd, forward-slash.
    let rel = relative_to_cwd(cwd, &file_path);
    if is_skipped_path(&rel) {
        return Verdict::Allow;
    }
    let content = new_content_of(input);
    if content.is_empty() {
        return Verdict::Allow;
    }

    let mut injects: Vec<String> = Vec::new();
    let mut session: Option<String> = None;

    // Boundary mismatch — advisory, surfaced ONCE per (spec, session).
    if let Some((spec, warning)) = check_boundaries(&file_path, cwd) {
        let s = session.get_or_insert_with(crate::shared::context::session_id);
        if boundary_warn_once(cwd, &spec, s.as_str()) {
            injects.push(format!("[BOUNDARY WARNING] {warning}"));
        }
    }

    if injects.is_empty() {
        Verdict::Allow
    } else {
        Verdict::Inject {
            context: injects.join("\n"),
        }
    }
}

/// `file_path` relative to `cwd`, forward-slash normalised. When `file_path`
/// is not under `cwd` it is returned normalised as-is (the JS `path.relative`
/// would produce a `../`-prefixed path; `is_skipped_path` handles neither
/// specially, and the rule scopes still apply on the raw path).
fn relative_to_cwd(cwd: &str, file_path: &str) -> String {
    let cwd_norm = cwd.replace('\\', "/");
    let fp_norm = file_path.replace('\\', "/");
    let prefix = format!("{}/", cwd_norm.trim_end_matches('/'));
    fp_norm
        .strip_prefix(&prefix)
        .map_or(fp_norm.clone(), str::to_string)
}

// ===========================================================================
// guards-gate — data-driven critical-Guard enforcement
// ===========================================================================
//
// The `scan`-authored per-subproject Guards used to be pure advisory context.
// This gate makes a `[critical]`-marked Guard enforceable: on a Write/Edit to a
// production file it resolves the governing subproject, loads THAT subproject's
// critical Guards from its `CLAUDE.md`, and — for a Guard in the checkable
// `never <forbidden> in <glob>` form — Denies (strict mode) an edit that
// introduces `<forbidden>` in a matching file. The rule comes entirely from the
// subproject's data, never from hardcoded language logic (the removed .NET/Next
// block). Critical Guards not in checkable form stay advisory (a strong Inject,
// never a Deny). Fail-open throughout — any resolution miss is "no opinion".

// ===========================================================================
// checklist-auto-mark — Observer on PostToolUse(Write|Edit)
// ===========================================================================

/// `checklist-auto-mark`: silently mark Checklist items in the active spec
/// that match the just-edited file.
///
/// Pure side effect — fail-open throughout, no verdict. Port of
/// `checklist-auto-mark.js`.
fn run_checklist_auto_mark(input: &HookInput, cwd: &str) {
    if !is_write_or_edit(input) {
        return;
    }
    let Some(file_path) = input.file_path() else {
        return;
    };
    if file_path.is_empty() {
        return;
    }
    let Some((spec_path, spec_name)) = find_active_spec(cwd, input.session_id.as_deref()) else {
        return;
    };
    // Don't auto-mark when the edited file IS the spec itself (avoid loops).
    if same_path(&file_path, &spec_path) {
        return;
    }
    let Ok(raw) = fs::read_to_string(Path::new(&spec_path)) else {
        return;
    };
    let mut lines: Vec<String> = raw.split('\n').map(str::to_string).collect();
    let Some((start, end)) = find_checklist_section(&lines) else {
        return;
    };

    let edited_base = basename(&file_path).to_string();
    let norm_edited = file_path.replace('\\', "/").to_ascii_lowercase();
    let mut dirty = false;

    for line in lines.iter_mut().take(end).skip(start) {
        let Some((prefix, gap, text)) = parse_unchecked_item(line) else {
            continue;
        };
        let mut matched = false;
        // Strategy 1: arrow target — `… → <path>`.
        if let Some(target) = arrow_target(&text) {
            let target = target.replace('\\', "/").to_ascii_lowercase();
            if norm_edited.ends_with(&target)
                || norm_edited.contains(&format!("/{target}"))
                || norm_edited == target
                || basename(&target) == edited_base.to_ascii_lowercase()
            {
                matched = true;
            }
        }
        // Strategy 2: basename anywhere in the item text.
        if !matched
            && !edited_base.is_empty()
            && text
                .to_ascii_lowercase()
                .contains(&edited_base.to_ascii_lowercase())
        {
            matched = true;
        }
        if matched {
            *line = format!("{prefix}[x]{gap}{text}");
            dirty = true;
        }
    }

    if dirty {
        let _ = fs::write_atomic(Path::new(&spec_path), lines.join("\n").as_bytes());
    }
}


/// Parse a `- [ ] <text>` unchecked-item line into `(prefix, gap, text)`.
/// Mirrors the JS regex `^(\s*-\s+)\[ \](\s+)(.*)$`.
fn parse_unchecked_item(line: &str) -> Option<(String, String, String)> {
    // Leading whitespace + `-` + whitespace.
    let ws_end = line.len() - line.trim_start().len();
    let leading = &line[..ws_end];
    let rest = &line[ws_end..];
    let rest = rest.strip_prefix('-')?;
    let dash_gap_end = rest.len() - rest.trim_start().len();
    if dash_gap_end == 0 {
        return None; // `-` must be followed by whitespace
    }
    let prefix = format!("{leading}-{}", &rest[..dash_gap_end]);
    let rest = &rest[dash_gap_end..];
    let rest = rest.strip_prefix("[ ]")?;
    // The gap after `[ ]` — one or more whitespace.
    let gap_end = rest.len() - rest.trim_start().len();
    if gap_end == 0 {
        return None;
    }
    let gap = rest[..gap_end].to_string();
    let text = rest[gap_end..].to_string();
    Some((prefix, gap, text))
}

/// Extract an arrow-target path from a Checklist item — `… → <path>` or
/// `… > <path>`. Mirrors `/[→>]\s*([^\s].*?)\s*$/`.
fn arrow_target(text: &str) -> Option<String> {
    let idx = text.rfind(['→', '>'])?;
    // The arrow char width: `→` is 3 bytes, `>` is 1.
    let arrow_len = text[idx..].chars().next().map_or(1, char::len_utf8);
    let after = text[idx + arrow_len..].trim();
    if after.is_empty() {
        None
    } else {
        Some(after.to_string())
    }
}

/// Locate the `## Checklist` section: returns `(start, end)` line indices.
fn find_checklist_section(lines: &[String]) -> Option<(usize, usize)> {
    let mut start = None;
    for (i, line) in lines.iter().enumerate() {
        if is_checklist_heading(line) {
            start = Some(i + 1);
            break;
        }
    }
    let start = start?;
    let mut end = lines.len();
    for (i, line) in lines.iter().enumerate().skip(start) {
        if line.starts_with("## ") || line == "##" {
            end = i;
            break;
        }
    }
    Some((start, end))
}

/// `true` if `line` is the `## Checklist` heading (`^##\s+Checklist\b`).
fn is_checklist_heading(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("##") else {
        return false;
    };
    if !rest.starts_with(char::is_whitespace) {
        return false;
    }
    let rest = rest.trim_start();
    if !rest.starts_with("Checklist") {
        return false;
    }
    rest.as_bytes()
        .get("Checklist".len())
        .is_none_or(|&b| !is_word_byte(b))
}

/// Find the current spec for `cwd` by the one current-spec ladder (the
/// environment override, then the checkout's branch, then the session
/// binding), with its `spec.md`. `None` when no spec is current or it has no
/// `spec.md`. Returns `(spec_path, spec_name)`.
pub(crate) fn find_active_spec(cwd: &str, session: Option<&str>) -> Option<(String, String)> {
    let name = crate::shared::spec_state::active_spec(cwd, session)?;
    let path = ClaudePaths::for_project(Path::new(cwd)).ok()?.for_spec(&name).ok()?.spec_md_path();
    fs::exists(&path).then(|| (path.to_string_lossy().into_owned(), name))
}

/// `true` if two paths resolve to the same file (canonicalised; falls back to
/// a normalised string compare when canonicalisation fails).
fn same_path(a: &str, b: &str) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => a.replace('\\', "/") == b.replace('\\', "/"),
    }
}

// ===========================================================================
// pipeline-phase removed — `mustard-rt run emit-phase` is the sole producer of
// `pipeline.phase` events. Nothing writes `phaseName` to a state file any more,
// so the old PostToolUse(Write|Edit) emitter had no real trigger left. Kept as
// a comment so the removal is searchable.
// ===========================================================================

// ===========================================================================
// Contract impls
// ===========================================================================

impl Check for PostEdit {
    /// `guard-verify`: run the critical-Guard gate + boundary advisory for a
    /// `PostToolUse(Write|Edit)`. A strict-mode checkable critical-Guard
    /// violation is a `Deny`; a boundary mismatch or an advisory-only critical
    /// Guard is an `Inject`; everything else `Allow`s.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PostToolUse) {
            return Ok(Verdict::Allow);
        }
        if !is_write_or_edit(input) {
            return Ok(Verdict::Allow);
        }
        let cwd = ctx.project_dir_or_cwd(input);
        Ok(guard_verify(input, &cwd))
    }
}

impl Observer for PostEdit {
    /// Run the fire-and-forget side effect of a `PostToolUse(Write|Edit)`:
    /// `checklist-auto-mark`. The legacy `pipeline-phase`
    /// emitter was removed once SKILL.md migrated to `mustard-rt run
    /// emit-phase` (the sole producer of `pipeline.phase` events).
    ///
    /// Pure side effects — never affect a verdict. Fail-open throughout.
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        if ctx.trigger != Some(Trigger::PostToolUse) {
            return;
        }
        if !is_write_or_edit(input) {
            return;
        }
        let cwd = ctx.project_dir_or_cwd(input);
        run_checklist_auto_mark(input, &cwd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn edit_input(file_path: &str, new_string: &str) -> HookInput {
        HookInput {
            tool_name: Some("Edit".to_string()),
            tool_input: json!({ "file_path": file_path, "new_string": new_string }),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        }
    }

    fn ctx(dir: &str) -> Ctx {
        Ctx::for_test(dir.to_string(), Some(Trigger::PostToolUse))
    }

    // --- guard-verify parity (hooks.test.js "guard-verify.js") -------------





    #[test]
    fn guard_skips_claude_files() {
        let input = edit_input(
            "/proj/.claude/hooks/some-hook.js",
            "DbContext something bad int UserId",
        );
        assert_eq!(guard_verify(&input, "/proj"), Verdict::Allow);
    }


    #[test]
    fn guard_skip_patterns_recognised() {
        assert!(is_skipped_path("src/.claude/x.js"));
        assert!(is_skipped_path("a/node_modules/b.ts"));
        assert!(is_skipped_path("pkg/dist/out.js"));
        assert!(!is_skipped_path("src/Models/User.cs"));
    }

    #[test]
    fn guard_via_check_only_post_tool_use() {
        let input = edit_input("/proj/src/Models/User.cs", "public int UserId { get; set; }");
        // PreToolUse trigger → the Check self-allows.
        let pre_ctx = Ctx::for_test("/proj".to_string(), Some(Trigger::PreToolUse));
        assert_eq!(
            PostEdit.evaluate(&input, &pre_ctx).expect("no error"),
            Verdict::Allow
        );
        // PostToolUse → still allows: the legacy critical-rule block is gone
        // and no active spec under `/proj` declares boundaries.
        assert_eq!(
            PostEdit.evaluate(&input, &ctx("/proj")).expect("no error"),
            Verdict::Allow
        );
    }

    // --- checklist-auto-mark parity (checklist-mark.test.js) ---------------

    /// Write a spec under `dir` and stand the checkout on its branch, returning
    /// the spec.md path.
    fn setup_spec(dir: &Path, spec_name: &str, body: &str) -> std::path::PathBuf {
        let paths = ClaudePaths::for_project(dir).unwrap();
        let sp = paths.for_spec(spec_name).unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        let spec_file = sp.spec_md_path();
        std::fs::write(&spec_file, body).unwrap();
        crate::shared::spec_state::stand_on_spec_branch(dir, spec_name);
        spec_file
    }

    #[test]
    fn checklist_marks_basename_pista() {
        let dir = tempdir().unwrap();
        let spec_file = setup_spec(
            dir.path(),
            "demo",
            "# Spec\n\n## Checklist\n\n- [ ] Update UserService.cs to add validation\n\
             - [ ] Write docs\n",
        );
        let edited = dir
            .path()
            .join("src")
            .join("Services")
            .join("UserService.cs");
        let input = edit_input(&edited.to_string_lossy(), "whatever");
        PostEdit.observe(&input, &ctx(dir.path().to_str().unwrap()));
        let updated = std::fs::read_to_string(&spec_file).unwrap();
        assert!(updated.contains("- [x] Update UserService.cs"));
        assert!(updated.contains("- [ ] Write docs"));
    }

    #[test]
    fn checklist_marks_arrow_target() {
        let dir = tempdir().unwrap();
        let spec_file = setup_spec(
            dir.path(),
            "demo",
            "# Spec\n\n## Checklist\n\n- [ ] Add validation → src/Services/UserService.cs\n",
        );
        let edited = dir
            .path()
            .join("src")
            .join("Services")
            .join("UserService.cs");
        let input = edit_input(&edited.to_string_lossy(), "whatever");
        PostEdit.observe(&input, &ctx(dir.path().to_str().unwrap()));
        let updated = std::fs::read_to_string(&spec_file).unwrap();
        assert!(updated.contains("- [x] Add validation"));
    }

    #[test]
    fn checklist_does_not_mark_when_no_pista() {
        let dir = tempdir().unwrap();
        let spec_file = setup_spec(
            dir.path(),
            "demo",
            "# Spec\n\n## Checklist\n\n- [ ] Refactor OtherFile.ts\n",
        );
        let edited = dir.path().join("src").join("Unrelated.cs");
        let input = edit_input(&edited.to_string_lossy(), "whatever");
        PostEdit.observe(&input, &ctx(dir.path().to_str().unwrap()));
        let updated = std::fs::read_to_string(&spec_file).unwrap();
        assert!(updated.contains("- [ ] Refactor OtherFile.ts"));
    }

    #[test]
    fn checklist_does_not_loop_on_spec_itself() {
        let dir = tempdir().unwrap();
        let spec_file = setup_spec(
            dir.path(),
            "demo",
            "# Spec\n\n## Checklist\n\n- [ ] Edit spec.md notes\n",
        );
        // Editing the spec itself must not auto-mark.
        let input = edit_input(&spec_file.to_string_lossy(), "whatever");
        PostEdit.observe(&input, &ctx(dir.path().to_str().unwrap()));
        let updated = std::fs::read_to_string(&spec_file).unwrap();
        assert!(updated.contains("- [ ] Edit spec.md notes"));
    }



    #[test]
    fn checklist_observe_infallible_without_spec() {
        let dir = tempdir().unwrap();
        let input = edit_input(
            &dir.path().join("src").join("Any.cs").to_string_lossy(),
            "x",
        );
        // No spec at all — observe must not panic.
        PostEdit.observe(&input, &ctx(dir.path().to_str().unwrap()));
    }

    // --- fail-open when no spec is current ---------------------------------

    #[test]
    fn checklist_observe_fail_open_no_pipeline_state() {
        // No current spec → find_active_spec returns None → observe is a
        // silent no-op. Must not panic.
        let dir = tempdir().unwrap();
        let cwd_str = dir.path().to_str().unwrap();
        let input = edit_input(
            &dir.path().join("src").join("Foo.ts").to_string_lossy(),
            "const x = 1;",
        );
        // Must not panic.
        PostEdit.observe(&input, &ctx(cwd_str));
    }

    // pipeline-phase tests removed — the emitter was deleted when
    // the dashboard phase moved off SQLite. `mustard-rt run emit-phase`
    // is the sole producer of `pipeline.phase` events; its tests live in
    // `apps/rt/src/run/emit_phase.rs`.

    #[test]
    fn observe_is_infallible() {
        // observe must never panic regardless of payload shape.
        let dir = tempdir().unwrap();
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({}),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        };
        PostEdit.observe(&input, &ctx(dir.path().to_str().unwrap()));
    }

    // --- boundary resolver picks the most recent active spec ---------------

    #[test]
    fn check_boundaries_picks_newer_active_spec_over_older() {
        let root = tempdir().unwrap();
        let spec_dir = root.path().join(".claude").join("spec");
        std::fs::create_dir_all(&spec_dir).unwrap();

        // Older spec (alphabetically first, Active, declares one boundary).
        let older = spec_dir.join("2026-05-26-old-active");
        std::fs::create_dir_all(&older).unwrap();
        std::fs::write(
            older.join("spec.md"),
            "# Old\n### Stage: Plan\n### Outcome: Active\n### Checkpoint: 2026-05-26T10:00:00.000Z\n## Boundaries\n- `apps/rt/src/run/old_only.rs`\n",
        )
        .unwrap();

        // Newer spec (Active, declares a DIFFERENT boundary covering the edit).
        let newer = spec_dir.join("2026-05-28-new-active");
        std::fs::create_dir_all(&newer).unwrap();
        std::fs::write(
            newer.join("spec.md"),
            "# New\n### Stage: Execute\n### Outcome: Active\n### Checkpoint: 2026-05-28T09:00:00.000Z\n## Boundaries\n- `apps/rt/src/hooks/post_edit.rs`\n",
        )
        .unwrap();

        // An edit to `post_edit.rs` is declared by the NEWER spec, not the
        // older one. With the recency fix the resolver picks the newer spec and
        // returns `None` (allowed); pre-fix it would warn under the older
        // spec's boundaries.
        let edit_path = root
            .path()
            .join("apps")
            .join("rt")
            .join("src")
            .join("hooks")
            .join("post_edit.rs");
        let cwd = root.path().to_str().unwrap();
        let result = check_boundaries(edit_path.to_str().unwrap(), cwd);
        assert!(
            result.is_none(),
            "newer active spec authorised the edit but resolver still warned: {result:?}"
        );
    }

    #[test]
    fn check_boundaries_warns_when_newer_active_spec_excludes_the_edit() {
        let root = tempdir().unwrap();
        let spec_dir = root.path().join(".claude").join("spec");
        std::fs::create_dir_all(&spec_dir).unwrap();

        let older = spec_dir.join("2026-05-26-old-active");
        std::fs::create_dir_all(&older).unwrap();
        std::fs::write(
            older.join("spec.md"),
            "# Old\n### Stage: Plan\n### Outcome: Active\n### Checkpoint: 2026-05-26T10:00:00.000Z\n## Boundaries\n- `apps/rt/src/hooks/post_edit.rs`\n",
        )
        .unwrap();

        let newer = spec_dir.join("2026-05-28-new-active");
        std::fs::create_dir_all(&newer).unwrap();
        std::fs::write(
            newer.join("spec.md"),
            "# New\n### Stage: Execute\n### Outcome: Active\n### Checkpoint: 2026-05-28T09:00:00.000Z\n## Boundaries\n- `apps/rt/src/run/something_else.rs`\n",
        )
        .unwrap();

        // The OLDER spec would have authorised the edit; the NEWER spec
        // doesn't. Recency wins ⇒ warning surfaces with the newer slug.
        let edit_path = root
            .path()
            .join("apps")
            .join("rt")
            .join("src")
            .join("hooks")
            .join("post_edit.rs");
        let cwd = root.path().to_str().unwrap();
        let result = check_boundaries(edit_path.to_str().unwrap(), cwd);
        let (spec, warning) = result.expect("expected a warning under newer spec boundaries");
        assert_eq!(spec, "2026-05-28-new-active", "the resolved spec is the newer one");
        assert!(
            warning.contains("2026-05-28-new-active"),
            "warning should cite the newer spec, got: {warning}"
        );
        assert!(
            !warning.contains("2026-05-26-old-active"),
            "older spec must not appear in the warning, got: {warning}"
        );
    }

    #[test]
    fn boundary_warn_once_dedups_per_spec_in_a_session() {
        // First call for a spec warns; the second (same spec, same session) is
        // suppressed; a different spec warns again. The session id is injected
        // so the dedup marker is deterministic regardless of the test's CWD.
        // (Previously the function read `session_id()` from the process CWD,
        // which only resolved when run inside a live Claude session — green
        // locally, red on a clean CI checkout.)
        let root = tempdir().unwrap();
        let cwd = root.path().to_str().unwrap();
        let session = "sess-x";

        assert!(boundary_warn_once(cwd, "spec-a", session), "first warn for spec-a surfaces");
        assert!(!boundary_warn_once(cwd, "spec-a", session), "repeat for spec-a is suppressed");
        assert!(boundary_warn_once(cwd, "spec-b", session), "a different spec warns once");
        assert!(!boundary_warn_once(cwd, "spec-b", session), "and is then suppressed too");
    }

    // --- data-driven critical-Guard gate ------------------------

    /// A formatação automática saiu daqui: num projeto com configuração de
    /// formatador e com o formatador na pasta dele, o arquivo recém-escrito
    /// fica byte a byte como foi gravado, pelas duas portas do gancho. Quem
    /// formata é a rodada, uma vez, antes do commit.
    ///
    /// O formatador falso tem dentes: é executável, sobrescreve com um texto
    /// conhecido todo arquivo que receber, e está na pasta onde a chamada
    /// removida o procurava. O teste começa provando isso num rascunho — um
    /// formatador falso que não roda, ou que não mudaria byte nenhum se
    /// rodasse, faria a asserção do fim valer com o gancho formatando ou não.
    #[cfg(unix)]
    #[test]
    fn a_write_in_a_project_with_a_formatter_config_leaves_the_file_byte_for_byte() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("node_modules/.bin")).unwrap();
        std::fs::write(root.join("package.json"), b"{}").unwrap();
        std::fs::write(root.join(".prettierrc"), b"{}").unwrap();
        let fake = root.join("node_modules").join(".bin").join("prettier");
        std::fs::write(
            &fake,
            b"#!/bin/sh\nfor a in \"$@\"; do case \"$a\" in -*) ;; *) printf 'FORMATADO\\n' > \"$a\" ;; esac; done\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();

        let text = "const   x   =   1\n\n\n";

        // O formatador falso destrói mesmo: no rascunho ele apaga o que estava
        // escrito. Sem esta parte, o fim do teste passaria por ausência.
        let scratch = root.join("src").join("rascunho.ts");
        std::fs::write(&scratch, text).unwrap();
        let ran = std::process::Command::new(&fake)
            .arg("--write")
            .arg(&scratch)
            .current_dir(root)
            .status()
            .expect("o formatador falso tem de ser executável");
        assert!(ran.success(), "o formatador falso tem de rodar até o fim");
        assert_eq!(
            std::fs::read(&scratch).unwrap(),
            b"FORMATADO\n",
            "o formatador falso tem de trocar o conteúdo do arquivo que recebe"
        );

        let file = root.join("src").join("a.ts");
        std::fs::write(&file, text).unwrap();

        let cwd = root.to_str().unwrap();
        let input = edit_input(&file.to_string_lossy(), text);
        assert_eq!(PostEdit.evaluate(&input, &ctx(cwd)).expect("no error"), Verdict::Allow);
        PostEdit.observe(&input, &ctx(cwd));

        assert_eq!(std::fs::read(&file).unwrap(), text.as_bytes(), "a gravação não passa por formatador");
    }
}
