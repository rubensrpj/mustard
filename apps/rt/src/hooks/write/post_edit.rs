//! `post_edit` — the consolidated PostToolUse(Write|Edit) module.
//!
//! ## Scope (b3 Wave 4, Write/Edit family)
//!
//! This module consolidates three JavaScript hooks, all `PostToolUse(Write|Edit)`.
//! Two are pure side effects (`Observer`), one reaches a verdict (`Check`):
//!
//! - `auto-format.js` — an **`Observer`**: runs Prettier / `dotnet format` on
//!   the just-written file. Fire-and-forget — no verdict.
//! - `checklist-auto-mark.js` — an **`Observer`**: silently marks Checklist
//!   items in the active spec when the edited file matches an item. No verdict.
//! - `guard-verify.js` — a **`Check`**: flags an edit that falls outside the
//!   active spec.s declared `## Boundaries` (advisory). The legacy
//!   critical-rule block (stack-specific DbContext/DIP/int-id rules) was
//!   removed — subproject Guards + review own that judgement.
//!
//! `PostEdit` therefore implements **both** [`Check`] (guard-verify) and
//! [`Observer`] (auto-format + checklist-auto-mark) — the same dual shape
//! `budget` and `bash_guard` use.
//!
//! Consolidation **regroups, it does not re-decide** — every verdict is a 1:1
//! port of the JS decision logic. Parity tests mirror
//! `__tests__/checklist-mark.test.js`.
//!
//! ## Migration note (dashboard-phase-from-sqlite)
//!
//! `pipeline-phase.js` used to live here as a fourth side effect: it parsed
//! `phaseName` out of a pipeline-state Write and emitted a `pipeline.phase`
//! event. Wave 2 of `2026-05-19-dashboard-phase-from-sqlite` removed the
//! `phaseName` writer from SKILL.md, so that trigger no longer fires. The
//! `pipeline.phase` producer now lives entirely in `mustard-rt run emit-phase`
//! (`apps/rt/src/run/emit_phase.rs`), driven explicitly by the pipeline
//! orchestrator commands.
//!
//! ## Verdict note (guard-verify)
//!
//! The boundary mismatch — advisory in the JS — is an [`Verdict::Inject`];
//! the module never blocks.

use mustard_core::platform::error::Error;
use mustard_core::io::fs;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Observer, Trigger, Verdict};
use mustard_core::domain::spec;
use mustard_core::{ClaudePaths, Outcome as SpecOutcome, Stage as SpecStage};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

/// The consolidated PostToolUse(Write|Edit) module.
pub struct PostEdit;

// ===========================================================================
// Shared helpers
// ===========================================================================

/// The `file_path` of a Write/Edit invocation.
fn file_path_of(input: &HookInput) -> Option<String> {
    let ti = &input.tool_input;
    ti.get("file_path")
        .or_else(|| ti.get("path"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

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
/// wave-18-rt-followups (W4#7): the scan now filters out non-active specs —
/// previously the first spec dir (alphabetically) with a `## Boundaries`
/// section always won, so a stale `Close + Active + followup_open` spec
/// (e.g. `dashboard-i18n-migration`) would warn on every unrelated edit. The
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

    // W5#4: collect every Active+open spec that ships a `## Boundaries` (or
    // `## Limites`) block, then keep ONLY the most-recently-checkpointed one.
    // Without this, `read_dir`'s alphabetical order makes an older spec
    // (`2026-05-26-deep-refactor-followups`) outrank a newer active spec
    // (`2026-05-27-mustard-v4-foundation`) and warn about edits the newer
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

/// Session-scoped marker path for the boundary advisory dedup:
/// `.claude/.session/<id>/boundary-warned`. `None` when the session is
/// unresolved (then the advisory is never suppressed — fail-open).
fn boundary_marker_path(cwd: &str, session: &str) -> Option<PathBuf> {
    if session.is_empty() || session == "unknown" {
        return None;
    }
    Some(
        ClaudePaths::for_project(Path::new(cwd))
            .ok()?
            .claude_dir()
            .join(".session")
            .join(session)
            .join("boundary-warned"),
    )
}

/// Surface the boundary advisory ONCE per (spec, session): the first
/// out-of-scope edit for a spec alerts; later edits in the same session stay
/// silent — re-emitting the same advisory on every edit is pure re-injected
/// noise (it cost ~one token bill per edit before this gate). State lives in a
/// session-scoped marker (newline-delimited spec names). Fail-open: an
/// unresolved session or any IO error returns `true` (warn), so the safety net
/// never goes silent on a broken FS. Only the non-blocking advisory is
/// deduped; a CRITICAL boundary violation is a separate `Deny` path, untouched.
fn boundary_warn_once(cwd: &str, spec: &str, session: &str) -> bool {
    let Some(marker) = boundary_marker_path(cwd, session) else {
        return true;
    };
    let seen = fs::read_to_string(&marker).unwrap_or_default();
    if seen.lines().any(|l| l.trim() == spec) {
        return false;
    }
    // `write_atomic` creates the parent dir; a failed write degrades to
    // re-warning next edit (never silent), which is the safe direction.
    let _ = fs::write_atomic(&marker, format!("{seen}{spec}\n").as_bytes());
    true
}

/// Resolve a spec's lifecycle [`SpecState`] from the filesystem,
/// **`meta.json`-first**. The sidecar beside `spec_file` is authoritative; the
/// already-read `.md` `content` is the legacy fallback for un-migrated specs.
fn spec_state_meta_first(spec_file: &Path, content: &str) -> Option<mustard_core::SpecState> {
    if let Some(m) = mustard_core::domain::meta::read_meta_beside(spec_file) {
        if let Some(stage) = m.stage.as_deref().and_then(mustard_core::Stage::parse) {
            let outcome = m
                .outcome
                .as_deref()
                .and_then(mustard_core::Outcome::parse)
                .unwrap_or(mustard_core::Outcome::Active);
            // Qualifier flags now live in `meta.json#flags`; fall back to
            // all-false when the persisted triple is illegal (stale sidecar).
            let flags: mustard_core::Flags = m.flags.into();
            return mustard_core::SpecState::new(stage, outcome, flags)
                .or_else(|_| {
                    mustard_core::SpecState::new(stage, outcome, mustard_core::Flags::default())
                })
                .ok();
        }
    }
    spec::parse_state(content)
}

/// W5#4 helper: a lexicographically-comparable recency key. Prefers the spec's
/// ISO checkpoint (so `2026-05-28T10:00:00.000Z` sorts above
/// `2026-05-27T17:56:09.926Z`), read **`meta.json`-first** (`#checkpoint`) with
/// a fallback to a legacy `### Checkpoint:` header; falls back to the directory
/// name prefix (`YYYY-MM-DD-…` already sorts correctly); never panics. Returned
/// `String` is meaningful only against other keys produced by this same fn.
fn recency_key_for_spec(spec_dir: &Path, content: &str, dir_name: &str) -> String {
    // meta.json wins.
    if let Some(m) = mustard_core::domain::meta::read_meta_beside(&spec_dir.join("spec.md")) {
        if let Some(cp) = m.checkpoint.filter(|s| !s.trim().is_empty()) {
            return cp.trim().to_string();
        }
    }
    // Legacy fallback: the `### Checkpoint:` header.
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

/// The `guard-verify` verdict for a `PostToolUse(Write|Edit)` invocation.
fn guard_verify(input: &HookInput, cwd: &str) -> Verdict {
    if !is_write_or_edit(input) {
        return Verdict::Allow;
    }
    let Some(file_path) = file_path_of(input) else {
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
    // Advisory: a boundary mismatch — surfaced ONCE per (spec, session) so it
    // does not re-inject the same warning on every subsequent out-of-scope edit.
    if let Some((spec, warning)) = check_boundaries(&file_path, cwd) {
        if boundary_warn_once(cwd, &spec, &crate::shared::context::session_id()) {
            return Verdict::Inject {
                context: format!("[BOUNDARY WARNING] {warning}"),
            };
        }
    }
    Verdict::Allow
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
// auto-format — Observer on PostToolUse(Write|Edit)
// ===========================================================================

/// Extensions Prettier handles. Mirrors `PRETTIER_EXTS`.
const PRETTIER_EXTS: &[&str] = &[
    ".ts", ".tsx", ".js", ".jsx", ".json", ".css", ".md", ".html", ".scss",
];

/// The lowercase extension of a path (including the dot), or `""`.
fn extension(path: &str) -> String {
    let base = basename(path);
    match base.rfind('.') {
        Some(idx) if idx > 0 => base[idx..].to_ascii_lowercase(),
        _ => String::new(),
    }
}

/// `auto-format`: run the appropriate formatter on the just-written file.
///
/// Pure side effect — fail-open throughout, no verdict. Mirrors
/// `auto-format.js`: Prettier for the JS/TS/CSS/MD family (only when a
/// Prettier config or `node_modules/.bin/prettier` is present), `dotnet
/// format` for `.cs`.
fn run_auto_format(input: &HookInput, cwd: &str) {
    let Some(file_path) = file_path_of(input) else {
        return;
    };
    if file_path.is_empty() {
        return;
    }
    // The file must exist on disk (the JS `fs.existsSync` guard).
    if !Path::new(&file_path).exists() {
        return;
    }
    let ext = extension(&file_path);
    if PRETTIER_EXTS.contains(&ext.as_str()) {
        run_prettier(&file_path, cwd);
    } else if ext == ".cs" {
        run_dotnet_format(&file_path);
    }
}

/// Run Prettier on `file_path` when a Prettier setup is detected under `cwd`
/// (or its parent — monorepo). Best-effort.
fn run_prettier(file_path: &str, cwd: &str) {
    let has_prettier = ["node_modules/.bin/prettier", ".prettierrc", ".prettierrc.js", ".prettierrc.json", "prettier.config.js"]
        .iter()
        .any(|rel| Path::new(cwd).join(rel).exists());
    let parent_has = Path::new(cwd)
        .parent()
        .is_some_and(|p| p.join("node_modules/.bin/prettier").exists());
    if !has_prettier && !parent_has {
        return;
    }
    // `npx prettier --write "<file>"`.
    let _ = Command::new("npx")
        .args(["prettier", "--write", file_path])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Run `dotnet format` on `file_path`, scoping to the nearest `.sln`/`.csproj`.
/// Best-effort.
fn run_dotnet_format(file_path: &str) {
    // Walk up to 5 directories for a `.sln` or `.csproj`.
    let mut search_dir = Path::new(file_path).parent().map(Path::to_path_buf);
    let mut project_file: Option<std::path::PathBuf> = None;
    for _ in 0..5 {
        let Some(dir) = search_dir.clone() else {
            break;
        };
        if let Ok(entries) = fs::read_dir(&dir) {
            let mut sln = None;
            let mut csproj = None;
            for entry in entries {
                if std::path::Path::new(&entry.file_name)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("sln")) {
                    sln = Some(entry.path.clone());
                } else if entry.file_name.ends_with(".csproj") {
                    csproj = Some(entry.path.clone());
                }
            }
            if let Some(p) = sln.or(csproj) {
                project_file = Some(p);
                break;
            }
        }
        let parent = search_dir.as_ref().and_then(|d| d.parent()).map(Path::to_path_buf);
        if parent == search_dir {
            break;
        }
        search_dir = parent;
    }
    let Some(project) = project_file else {
        return;
    };
    let Some(project_dir) = project.parent() else {
        return;
    };
    let _ = Command::new("dotnet")
        .args(["format"])
        .arg(&project)
        .args(["--include", file_path, "--no-restore"])
        .current_dir(project_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

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
    let Some(file_path) = file_path_of(input) else {
        return;
    };
    if file_path.is_empty() {
        return;
    }
    let Some((spec_path, spec_name)) = find_active_spec(cwd) else {
        return;
    };
    // Don't auto-mark when the edited file IS the spec itself (avoid loops).
    if same_path(&file_path, &spec_path) {
        return;
    }
    // Meta-first: flip matching `meta.json#checklist` items (the events-first
    // home of per-wave progress, seeded by `wave-scaffold`) and emit one
    // `checklist.item.marked` per flip. Fail-open side effect; the legacy
    // markdown `## Checklist` pass below still runs for un-migrated specs.
    mark_meta_checklists(cwd, &spec_path, &spec_name, &file_path);
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

/// Flip every matching un-done `meta.json#checklist` item across the active
/// spec's dir + its `wave-N-*` subdirs, emitting one `checklist.item.marked`
/// event per flip (via the shared
/// [`crate::commands::checklist::mark_checklist_item::emit_item_marked`]).
///
/// Pure side effect — fail-open throughout: unreadable / checklist-less
/// sidecars are skipped, a failed atomic write skips that dir's emits, and
/// already-done items never flip twice (idempotent — no duplicate events).
fn mark_meta_checklists(cwd: &str, spec_path: &str, spec_name: &str, edited: &str) {
    use crate::commands::checklist::mark_checklist_item::{emit_item_marked, wave_number_of};
    use mustard_core::domain::model::event::ActorKind;

    let Some(spec_dir) = Path::new(spec_path).parent() else {
        return;
    };
    let edited_base = basename(edited).to_ascii_lowercase();
    let norm_edited = edited.replace('\\', "/").to_ascii_lowercase();

    let mut dirs = vec![spec_dir.to_path_buf()];
    if let Ok(entries) = fs::read_dir(spec_dir) {
        let mut waves: Vec<std::path::PathBuf> = entries
            .into_iter()
            .filter(|e| e.path.is_dir() && e.file_name.starts_with("wave-"))
            .map(|e| e.path)
            .collect();
        waves.sort();
        dirs.extend(waves);
    }

    for dir in dirs {
        let meta_path = dir.join("meta.json");
        let Some(mut meta) = mustard_core::read_meta(&meta_path) else {
            continue;
        };
        if meta.checklist.is_empty() {
            continue;
        }
        let wave = dir
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(wave_number_of)
            .unwrap_or(0);
        let mut flipped: Vec<usize> = Vec::new();
        for (i, item) in meta.checklist.iter_mut().enumerate() {
            if item.done || !meta_item_matches_edit(item, &norm_edited, &edited_base) {
                continue;
            }
            item.done = true;
            flipped.push(i);
        }
        if flipped.is_empty() {
            continue;
        }
        if mustard_core::domain::meta::write_meta(&meta_path, &meta).is_err() {
            continue;
        }
        for i in flipped {
            emit_item_marked(
                cwd,
                ActorKind::Hook,
                "checklist-auto-mark",
                spec_name,
                wave,
                &meta.checklist[i],
            );
        }
    }
}

/// `true` when a typed checklist item matches the just-edited file — the same
/// two strategies the markdown pass uses: the path anchor (exact / suffix /
/// `/`-segment contains / basename), then the edited basename inside the label.
fn meta_item_matches_edit(
    item: &mustard_core::domain::spec::contract::ChecklistItem,
    norm_edited: &str,
    edited_base: &str,
) -> bool {
    // Strategy 1: the item's path anchor.
    if let Some(p) = item.path.as_deref() {
        let target = p.trim().replace('\\', "/").to_ascii_lowercase();
        if !target.is_empty()
            && (norm_edited.ends_with(&target)
                || norm_edited.contains(&format!("/{target}"))
                || norm_edited == target
                || basename(&target) == edited_base)
        {
            return true;
        }
    }
    // Strategy 2: the edited basename anywhere in the label.
    !edited_base.is_empty() && item.label.to_ascii_lowercase().contains(edited_base)
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

/// Find the active spec for `cwd`. Strategy: the newest pipeline-state's
/// `spec`/`specName`, else the newest `.claude/spec/{name}/spec.md` (flat layout).
/// Port of `findActiveSpec`. Returns `(spec_path, spec_name)`.
fn find_active_spec(cwd: &str) -> Option<(String, String)> {
    let paths = ClaudePaths::for_project(Path::new(cwd)).ok()?;
    let claude = paths.claude_dir();
    if !claude.exists() {
        return None;
    }
    // Strategy 1: newest pipeline-state.
    let states = paths.pipeline_states_dir();
    if let Ok(entries) = fs::read_dir(&states) {
        let mut best: Option<(SystemTime, std::path::PathBuf)> = None;
        for entry in entries {
            if !std::path::Path::new(&entry.file_name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json")) || entry.file_name.ends_with(".metrics.json") {
                continue;
            }
            let Ok(mtime) = fs::modified(&entry.path) else {
                continue;
            };
            if best.as_ref().is_none_or(|(t, _)| mtime > *t) {
                best = Some((mtime, entry.path));
            }
        }
        if let Some((_, path)) = best {
            if let Ok(text) = fs::read_to_string(&path) {
                if let Ok(obj) = serde_json::from_str::<Value>(&text) {
                    let name = obj
                        .get("spec")
                        .or_else(|| obj.get("specName"))
                        .and_then(|v| v.as_str());
                    if let Some(name) = name {
                        let candidate = paths
                            .for_spec(name)
                            .map(|sp| sp.spec_md_path())
                            .ok()?;
                        if candidate.exists() {
                            return Some((
                                candidate.to_string_lossy().into_owned(),
                                name.to_string(),
                            ));
                        }
                    }
                }
            }
        }
    }
    // Strategy 2: newest spec dir (flat layout — scan spec/ directly).
    let active = paths.spec_dir();
    let entries = fs::read_dir(&active).ok()?;
    let mut best: Option<(SystemTime, String, String)> = None;
    for entry in entries.into_iter().filter(|e| e.is_dir) {
        let dir_name = entry.file_name.clone();
        let candidate = entry.path.join("spec.md");
        if !fs::exists(&candidate) {
            continue;
        }
        let Ok(mtime) = fs::modified(&candidate) else {
            continue;
        };
        if best.as_ref().is_none_or(|(t, _, _)| mtime > *t) {
            best = Some((mtime, candidate.to_string_lossy().into_owned(), dir_name));
        }
    }
    best.map(|(_, path, name)| (path, name))
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
// `pipeline.phase` events. SKILL.md no longer writes `phaseName` to the
// pipeline-state JSON, so the old PostToolUse(Write|Edit) emitter never had a
// real trigger after the Wave-2 SQLite migration. Kept as a comment so the
// migration intent is searchable.
// ===========================================================================

// ===========================================================================
// Contract impls
// ===========================================================================

impl Check for PostEdit {
    /// `guard-verify`: surface the boundary advisory for a
    /// `PostToolUse(Write|Edit)`. A mismatch with the active spec.s declared
    /// `## Boundaries` is an `Inject` advisory; everything else `Allow`s.
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
    /// Run the two fire-and-forget side effects of a `PostToolUse(Write|Edit)`:
    /// `auto-format`, `checklist-auto-mark`. The legacy `pipeline-phase`
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
        run_auto_format(input, &cwd);
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
        Ctx {
            project_dir: dir.to_string(),
            trigger: Some(Trigger::PostToolUse),
            workspace_root: None,
        }
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
        let pre_ctx = Ctx {
            project_dir: "/proj".to_string(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
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

    /// Write a spec + pipeline-state under `dir`, returning the spec.md path.
    fn setup_spec(dir: &Path, spec_name: &str, body: &str) -> std::path::PathBuf {
        let paths = ClaudePaths::for_project(dir).unwrap();
        let sp = paths.for_spec(spec_name).unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        let spec_file = sp.spec_md_path();
        std::fs::write(&spec_file, body).unwrap();
        let states = paths.pipeline_states_dir();
        std::fs::create_dir_all(&states).unwrap();
        std::fs::write(
            paths.pipeline_state_file(spec_name),
            json!({ "spec": spec_name, "phase": "EXECUTE" }).to_string(),
        )
        .unwrap();
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

    /// Meta-first auto-mark (checklist-progresso-por-onda W2): a Write of a
    /// checklist target file flips the matching item in the WAVE's
    /// `meta.json#checklist` (idempotently) and emits `checklist.item.marked`.
    #[test]
    fn checklist_marks_wave_meta_item_and_emits_event() {
        let dir = tempdir().unwrap();
        setup_spec(dir.path(), "demo", "# Spec\n\n## Notes\n");
        let paths = ClaudePaths::for_project(dir.path()).unwrap();
        let sp = paths.for_spec("demo").unwrap();
        let wave_dir = sp.dir().join("wave-1-rt");
        std::fs::create_dir_all(&wave_dir).unwrap();
        std::fs::write(wave_dir.join("spec.md"), "# wave-1-rt\n").unwrap();
        std::fs::write(
            wave_dir.join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active","parent":"demo","checklist":[{"label":"src/Services/UserService.cs","path":"src/Services/UserService.cs","done":false},{"label":"docs/notes.md","path":"docs/notes.md","done":false}]}"#,
        )
        .unwrap();

        let edited = dir
            .path()
            .join("src")
            .join("Services")
            .join("UserService.cs");
        let input = edit_input(&edited.to_string_lossy(), "whatever");
        PostEdit.observe(&input, &ctx(dir.path().to_str().unwrap()));

        let meta = mustard_core::read_meta(&wave_dir.join("meta.json")).unwrap();
        assert!(meta.checklist[0].done, "matching item flipped");
        assert!(!meta.checklist[1].done, "unrelated item untouched");

        // The NDJSON event landed under the spec's events sink.
        let events_dir = sp.events_dir();
        assert!(events_dir.exists(), "events dir must exist after the emit");
        let mut found = false;
        for f in std::fs::read_dir(&events_dir).unwrap() {
            let body = std::fs::read_to_string(f.unwrap().path()).unwrap_or_default();
            found = found
                || body
                    .lines()
                    .any(|l| l.contains("\"event\":\"checklist.item.marked\""));
        }
        assert!(found, "checklist.item.marked NDJSON line must be present");

        // Idempotent: a second observe flips nothing and emits no second event.
        let count_lines = |d: &std::path::Path| -> usize {
            std::fs::read_dir(d)
                .map(|it| {
                    it.flatten()
                        .map(|f| {
                            std::fs::read_to_string(f.path())
                                .unwrap_or_default()
                                .lines()
                                .filter(|l| l.contains("\"event\":\"checklist.item.marked\""))
                                .count()
                        })
                        .sum()
                })
                .unwrap_or(0)
        };
        let before = count_lines(&events_dir);
        PostEdit.observe(&input, &ctx(dir.path().to_str().unwrap()));
        assert_eq!(count_lines(&events_dir), before, "no duplicate event on re-edit");
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

    // --- Wave-3a: fail-open when pipeline-state JSON absent -----------------

    #[test]
    fn checklist_observe_fail_open_no_pipeline_state() {
        // No `.pipeline-states` dir, no SQLite DB → find_active_spec falls
        // through to strategy 2 (active spec dir) → no spec dir either →
        // returns None → observe is a silent no-op. Must not panic.
        let dir = tempdir().unwrap();
        let cwd_str = dir.path().to_str().unwrap();
        let input = edit_input(
            &dir.path().join("src").join("Foo.ts").to_string_lossy(),
            "const x = 1;",
        );
        // Must not panic.
        PostEdit.observe(&input, &ctx(cwd_str));
    }

    // pipeline-phase tests removed — the emitter was deleted (see § A.II of
    // the dashboard-phase-from-sqlite migration). `mustard-rt run emit-phase`
    // is the sole producer of `pipeline.phase` events; its tests live in
    // `apps/rt/src/run/emit_phase.rs`.

    // --- auto-format -------------------------------------------------------

    #[test]
    fn auto_format_skips_missing_file() {
        // The file does not exist — run_auto_format must be a silent no-op.
        let dir = tempdir().unwrap();
        let input = edit_input(
            &dir.path().join("nonexistent.ts").to_string_lossy(),
            "const x=1;",
        );
        // Must not panic.
        run_auto_format(&input, dir.path().to_str().unwrap());
    }

    #[test]
    fn extension_extraction() {
        assert_eq!(extension("/a/b/file.TS"), ".ts");
        assert_eq!(extension("/a/b/SKILL.md"), ".md");
        assert_eq!(extension("/a/b/noext"), "");
        assert_eq!(extension("/a/.hidden"), "");
    }

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

    // --- W5#4: boundary resolver picks the most recent active spec ---------

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
        // older one. With the W5#4 fix the resolver picks the newer spec and
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
}
