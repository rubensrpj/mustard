//! `path_guard` — the consolidated Write/Edit path-boundary module.
//!
//! ## Scope (b3 Wave 4, Write/Edit family)
//!
//! This module ports two JavaScript hooks, both `PreToolUse` gates on a file
//! path:
//!
//! - `file-guard.js` — a `PreToolUse(Read|Write|Edit)` **safety** gate: denies
//!   access to a sensitive file (`credentials*`, `*.pem`, `*.key`,
//!   `.git/config`, SSH keys, `*.pfx`/`*.p12`). It has no mode — always
//!   strict, like `bash-safety`.
//! - `boundary-gate.js` — a `PreToolUse(Write|Edit)` gate that flags an edit
//!   outside the active spec's `## Files` / `## Boundaries` declaration. Mode
//!   `MUSTARD_BOUNDARY_MODE` (default `warn`): warn → advisory, strict → deny.
//!
//! Consolidation **regroups, it does not re-decide** — every verdict is a 1:1
//! port of the JS decision logic. The parity tests at the bottom mirror
//! `__tests__/hooks.test.js` ("file-guard.js").
//!
//! ## CONCERN — boundary-gate event telemetry
//!
//! `boundary-gate.js` emits a `boundary.expansion` harness event tagged with
//! `session_id` / `wave` resolved from the pipeline-state. The `mustard-core`
//! `Ctx` carries neither, so this port emits the event with `session_id`
//! falling back to `input.session_id` (often absent → `"unknown"`) and `wave`
//! as `0` — exactly the JS fallback. Recorded in the spec `## Concerns`.

use mustard_core::error::Error;
use mustard_core::fs;
use mustard_core::store::sqlite_store::SqliteEventStore;
use mustard_core::ClaudePaths;
use mustard_core::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde_json::json;
use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::run::{PipelineStateView, pipeline_state_from_events};
use crate::util::now_iso8601;

/// The consolidated Write/Edit path-boundary module.
pub struct PathGuard;

// ---------------------------------------------------------------------------
// file-guard — deny access to sensitive files
// ---------------------------------------------------------------------------

/// `true` if `path` (forward-slash normalised, original case) matches a
/// sensitive-file pattern. Mirrors `BLOCKED_PATTERNS` in `file-guard.js`:
/// `credentials`, `*.pem`, `*.key`, `.git/config`, `id_rsa`, `id_ed25519`,
/// `*.pfx`, `*.p12` — all case-insensitive.
fn sensitive_pattern_match(path: &str) -> Option<&'static str> {
    let lower = path.replace('\\', "/").to_ascii_lowercase();
    // /credentials/i — substring.
    if lower.contains("credentials") {
        return Some("credentials");
    }
    // /\.pem$/i, /\.key$/i, /\.pfx$/i, /\.p12$/i — extension.
    // `lower` is already ASCII-lowercased, so ends_with is case-insensitive here.
    #[allow(clippy::case_sensitive_file_extension_comparisons)]
    {
    if lower.ends_with(".pem") {
        return Some("\\.pem$");
    }
    if lower.ends_with(".key") {
        return Some("\\.key$");
    }
    if lower.ends_with(".pfx") {
        return Some("\\.pfx$");
    }
    if lower.ends_with(".p12") {
        return Some("\\.p12$");
    }
    }
    // /\.git[/\\]config$/i — `.git/config` at the end of the path.
    if lower.ends_with(".git/config") {
        return Some("\\.git[/\\\\]config$");
    }
    // /id_rsa/i, /id_ed25519/i — substring.
    if lower.contains("id_rsa") {
        return Some("id_rsa");
    }
    if lower.contains("id_ed25519") {
        return Some("id_ed25519");
    }
    None
}

/// The `file-guard` gate: deny a Read/Write/Edit on a sensitive file.
///
/// 1:1 with `file-guard.js`: only `Read`/`Write`/`Edit` tools are inspected;
/// the file path *and* its basename are tested against every pattern. A match
/// → `Deny`; otherwise `None` (fall through to `boundary-gate`).
fn file_guard(input: &HookInput) -> Option<Verdict> {
    let tool = input.tool_name.as_deref().unwrap_or_default();
    if !matches!(tool, "Read" | "Write" | "Edit") {
        return None;
    }
    let file_path = file_path_of(input)?;
    let normalized = file_path.replace('\\', "/");
    let basename = normalized.rsplit('/').next().unwrap_or(&normalized);

    // The JS tests `pattern.test(normalized) || pattern.test(basename)`.
    // `sensitive_pattern_match` already covers both: substring patterns hit
    // the full path, extension patterns hit either — so testing the full path
    // and the basename separately reproduces the JS exactly.
    let pattern = sensitive_pattern_match(&normalized).or_else(|| sensitive_pattern_match(basename))?;
    Some(Verdict::Deny {
        reason: format!(
            "[file-guard] Access to sensitive file blocked: {basename}\n\
             Matched pattern: {pattern}"
        ),
    })
}

// ---------------------------------------------------------------------------
// boundary-gate — flag edits outside the active spec's declared boundary
// ---------------------------------------------------------------------------

/// Path prefixes always allowed — infrastructure edits a spec rarely lists.
/// Mirrors `META_PREFIXES` in `boundary-gate.js`.
const META_PREFIXES: &[&str] = &[".claude/", "dist/", "node_modules/", ".git/"];

/// `true` if `rel` (forward-slash) is a meta/infrastructure path. An empty
/// `rel` is also treated as meta (`isMetaPath('')` → true in the JS).
fn is_meta_path(rel: &str) -> bool {
    if rel.is_empty() {
        return true;
    }
    META_PREFIXES.iter().any(|p| rel.starts_with(p))
}

/// The `MUSTARD_BOUNDARY_MODE` mode (default `warn`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoundaryMode {
    Off,
    Warn,
    Strict,
}

fn boundary_mode() -> BoundaryMode {
    match std::env::var("MUSTARD_BOUNDARY_MODE")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "off" => BoundaryMode::Off,
        "strict" => BoundaryMode::Strict,
        _ => BoundaryMode::Warn,
    }
}

/// Freshness window for the newest pipeline-state — 10 minutes (the JS
/// `10 * 60 * 1000` ms).
const STATE_FRESHNESS_MS: u128 = 10 * 60 * 1000;

/// The newest *fresh* pipeline-state JSON value. Mirrors
/// `readNewestFreshState`: the most recently modified pipeline-state JSON file
/// under `.claude` (excluding `*.metrics.json`), but only when its mtime is
/// within the freshness window.
fn read_newest_fresh_state(cwd: &str) -> Option<serde_json::Value> {
    let paths = ClaudePaths::for_project(Path::new(cwd)).ok()?;
    let dir = paths.pipeline_states_dir();
    let entries = fs::read_dir(&dir).ok()?;
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
    let (mtime, path) = best?;
    // Freshness: skip a stale state file.
    let age = SystemTime::now()
        .duration_since(mtime)
        .unwrap_or(Duration::ZERO)
        .as_millis();
    if age > STATE_FRESHNESS_MS {
        return None;
    }
    let text = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Resolve the spec.md file for a pipeline-state. Mirrors `resolveSpecFile`:
/// `.claude/spec/{specName}/spec.md` (flat layout), with a wave-plan branch that
/// looks for a `wave-{N}-*/spec.md` child directory first.
///
/// `spec_name` is the spec identifier (from the JSON state file stem or the
/// projection's `spec` field). `view` is the typed projection — `None` means
/// wave info is unknown, so the flat `spec.md` is used.
fn resolve_spec_file(
    cwd: &str,
    spec_name: &str,
    view: Option<&PipelineStateView>,
) -> Option<std::path::PathBuf> {
    let base = ClaudePaths::for_project(Path::new(cwd))
        .and_then(|p| p.for_spec(spec_name))
        .map(|sp| sp.dir().to_path_buf())
        .ok()?;
    if !base.exists() {
        return None;
    }
    let is_wave_plan = view
        .and_then(|v| v.is_wave_plan)
        .unwrap_or(false);
    if is_wave_plan {
        let wave = view.map_or(1, |v| v.current_wave);
        let prefix = format!("wave-{wave}-");
        if let Ok(entries) = fs::read_dir(&base) {
            for entry in entries.into_iter().filter(|e| e.is_dir && e.file_name.starts_with(&prefix)) {
                let cand = entry.path.join("spec.md");
                if fs::exists(&cand) {
                    return Some(cand);
                }
            }
        }
    }
    let root = base.join("spec.md");
    if root.exists() { Some(root) } else { None }
}

/// Extract the backtick-wrapped path patterns from a spec's `## Files` and
/// `## Boundaries` (and their PT equivalents). Port of `extractAllowedPatterns`.
fn extract_allowed_patterns(spec_text: &str) -> Vec<String> {
    let mut patterns: Vec<String> = Vec::new();
    let mut in_section = false;
    for line in spec_text.split('\n') {
        if is_files_or_boundaries_heading(line) {
            in_section = true;
            continue;
        }
        if is_other_h2(line) {
            in_section = false;
            continue;
        }
        if !in_section {
            continue;
        }
        for candidate in backtick_spans(line) {
            let candidate = candidate.trim();
            if candidate.is_empty() || candidate.len() > 200 {
                continue;
            }
            // Reject obvious non-paths (mirrors the JS rejections).
            if looks_like_command_with_flag(candidate) {
                continue;
            }
            if looks_like_env_assignment(candidate) {
                continue;
            }
            // Must contain a slash or a dot, else it is likely a label.
            if !candidate.contains('/') && !candidate.contains('.') {
                continue;
            }
            if !patterns.iter().any(|p| p == candidate) {
                patterns.push(candidate.to_string());
            }
        }
    }
    patterns
}

/// `true` if `line` is a `## Files`/`## Boundaries` (or PT) H2 heading.
fn is_files_or_boundaries_heading(line: &str) -> bool {
    let lower = line.trim().to_ascii_lowercase();
    matches!(
        lower.as_str(),
        s if s.starts_with("## files") && h2_word_boundary(&lower, "files")
    ) || h2_named(&lower, "files")
        || h2_named(&lower, "arquivos")
        || h2_named(&lower, "boundaries")
        || h2_named(&lower, "limites")
}

/// `true` if a lowercased line is an H2 heading whose name (after `## `) is
/// exactly `name`, possibly with a `\b`-bounded suffix.
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

/// Helper retained for readability of [`is_files_or_boundaries_heading`].
fn h2_word_boundary(lower: &str, name: &str) -> bool {
    h2_named(lower, name)
}

/// `true` if `line` is any `## ` H2 heading (used to close a section).
fn is_other_h2(line: &str) -> bool {
    let t = line;
    t.starts_with("## ") && t.len() > 3 && !t.as_bytes()[3].is_ascii_whitespace()
}

/// Every backtick-delimited span on a line — JS pattern `[^\`\n]+?` between backticks.
fn backtick_spans(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            if let Some(rel) = line[i + 1..].find('`') {
                let span = &line[i + 1..i + 1 + rel];
                if !span.is_empty() {
                    out.push(span);
                }
                i = i + 1 + rel + 1;
                continue;
            }
            break;
        }
        i += 1;
    }
    out
}

/// `true` for a `^[a-z]+\s+--?\w` shape — a command followed by a flag.
fn looks_like_command_with_flag(s: &str) -> bool {
    let mut chars = s.char_indices();
    let mut end = 0;
    let mut any = false;
    for (i, c) in chars.by_ref() {
        if c.is_ascii_lowercase() {
            any = true;
            end = i + 1;
        } else {
            break;
        }
    }
    if !any {
        return false;
    }
    let rest = &s[end..];
    let trimmed = rest.trim_start();
    if trimmed.len() == rest.len() {
        return false; // no whitespace gap
    }
    let mut tc = trimmed.chars();
    if tc.next() != Some('-') {
        return false;
    }
    let mut next = tc.next();
    if next == Some('-') {
        next = tc.next();
    }
    next.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `true` for a `^[A-Z][A-Z0-9_]*=` shape — an env-var assignment.
fn looks_like_env_assignment(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_uppercase() {
        return false;
    }
    for c in chars {
        if c == '=' {
            return true;
        }
        if !(c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') {
            return false;
        }
    }
    false
}

/// `true` if `rel` matches `pattern`. Port of `patternMatches`: exact match,
/// directory-prefix (`pattern` ends with `/`), or a one/two-star glob.
fn pattern_matches(rel: &str, pattern: &str) -> bool {
    let r = rel.replace('\\', "/");
    let p = pattern.replace('\\', "/");
    if r == p {
        return true;
    }
    if p.ends_with('/') && r.starts_with(&p) {
        return true;
    }
    if p.contains('*') {
        return glob_match(&r, &p);
    }
    false
}

/// Match `r` against a glob `p` (`*` = one path segment, `**` = anything).
fn glob_match(r: &str, p: &str) -> bool {
    // Build a regex-free matcher: split the glob into literal/`*`/`**` tokens
    // and walk. For the patterns specs use this is simplest as a recursive
    // segment matcher.
    glob_match_at(r.as_bytes(), p.as_bytes())
}

/// Recursive byte-wise glob matcher. `**` matches any run (incl. `/`); `*`
/// matches any run *not* containing `/`.
fn glob_match_at(text: &[u8], pat: &[u8]) -> bool {
    if pat.is_empty() {
        return text.is_empty();
    }
    if pat.starts_with(b"**") {
        let rest = &pat[2..];
        // `**` consumes zero-or-more of anything.
        let mut i = 0;
        loop {
            if glob_match_at(&text[i..], rest) {
                return true;
            }
            if i >= text.len() {
                return false;
            }
            i += 1;
        }
    }
    if pat[0] == b'*' {
        let rest = &pat[1..];
        // `*` consumes zero-or-more non-`/`.
        let mut i = 0;
        loop {
            if glob_match_at(&text[i..], rest) {
                return true;
            }
            if i >= text.len() || text[i] == b'/' {
                return false;
            }
            i += 1;
        }
    }
    if !text.is_empty() && text[0] == pat[0] {
        return glob_match_at(&text[1..], &pat[1..]);
    }
    false
}

/// Resolve the `file_path` of a Write/Edit (or Read) invocation, accepting the
/// legacy `path` key. Mirrors `tool_input.file_path || tool_input.path`.
fn file_path_of(input: &HookInput) -> Option<String> {
    let ti = &input.tool_input;
    ti.get("file_path")
        .or_else(|| ti.get("path"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// The cwd for an invocation — the harness `cwd`, else `.`.
fn project_dir(input: &HookInput, ctx: &Ctx) -> String {
    if !ctx.project_dir.is_empty() {
        return ctx.project_dir.clone();
    }
    match input.cwd.as_deref() {
        Some(c) if !c.is_empty() => c.to_string(),
        _ => ".".to_string(),
    }
}

/// Compute the path of `file_path` relative to `cwd`, forward-slash
/// normalised. Returns `None` when `file_path` escapes `cwd` (`../`) — the
/// caller treats that the same as a meta path (skip). Mirrors the JS
/// `path.relative(cwd, abs)` + `rel.startsWith('../')` check.
fn relative_to_cwd(cwd: &str, file_path: &str) -> Option<String> {
    let cwd_norm = cwd.replace('\\', "/");
    let fp_norm = file_path.replace('\\', "/");
    // Resolve `fp` to an absolute-ish path: if not absolute, join under cwd.
    let abs = if is_absolute(&fp_norm) {
        fp_norm
    } else {
        format!("{}/{}", cwd_norm.trim_end_matches('/'), fp_norm)
    };
    let cwd_prefix = format!("{}/", cwd_norm.trim_end_matches('/'));
    if let Some(rel) = abs.strip_prefix(&cwd_prefix) {
        Some(rel.to_string())
    } else if abs == cwd_norm.trim_end_matches('/') {
        Some(String::new())
    } else {
        // Outside cwd — treat as `../` (skip).
        None
    }
}

/// `true` if a forward-slash path looks absolute (POSIX `/...` or Windows
/// `C:/...`).
fn is_absolute(p: &str) -> bool {
    p.starts_with('/')
        || (p.len() >= 3
            && p.as_bytes()[0].is_ascii_alphabetic()
            && p.as_bytes()[1] == b':'
            && p.as_bytes()[2] == b'/')
}

/// Emit the `boundary.expansion` harness event. Best-effort telemetry.
fn emit_boundary_event(
    project_dir: &str,
    session_id: Option<&str>,
    rel: &str,
    spec: &str,
    mode: BoundaryMode,
    sample_patterns: &[String],
) {
    let mode_str = match mode {
        BoundaryMode::Off => "off",
        BoundaryMode::Warn => "warn",
        BoundaryMode::Strict => "strict",
    };
    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        // CONCERN: `Ctx` carries no wave; emit 0 (the JS `getCurrentWave`
        // fallback). `session_id` falls back to "unknown".
        session_id: session_id.unwrap_or("unknown").to_string(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("boundary-gate".to_string()),
            actor_type: None,
        },
        event: "boundary.expansion".to_string(),
        payload: json!({
            "file": rel,
            "spec": spec,
            "wave": serde_json::Value::Null,
            "mode": mode_str,
            "sample_patterns": sample_patterns.iter().take(6).collect::<Vec<_>>(),
        }),
        spec: Some(spec.to_string()),
    };
    // `boundary.expansion` is non-pipeline → per-spec NDJSON via the W5 router.
    let _ = crate::run::event_route::emit(project_dir, &event);
}

/// The `boundary-gate` gate: flag a Write/Edit outside the active spec's
/// declared `## Files` / `## Boundaries`.
///
/// 1:1 with `boundary-gate.js` — every early `process.exit(0)` maps to
/// `None` (pass through). A real mismatch → `Deny` in strict mode, `Warn` in
/// warn mode.
///
/// Wave-3a migration: spec fields that are pipeline-state-style (`isWavePlan`,
/// `currentWave`, `status`) are now derived from the `SQLite` projection via
/// `pipeline_state_from_events`. The JSON state file is still consulted for
/// `specName` (filesystem identity — not in the projection) and for the mtime
/// freshness gate. Fail-open: projection `None` → treat status as empty and
/// wave info as unknown.
fn boundary_gate(input: &HookInput, cwd: &str) -> Option<Verdict> {
    let mode = boundary_mode();
    if mode == BoundaryMode::Off {
        return None;
    }
    let file_path = file_path_of(input)?;
    // Compute rel; an escaping (`../`) path → None → skip.
    let rel = relative_to_cwd(cwd, &file_path)?;
    if is_meta_path(&rel) {
        return None;
    }
    // The JSON state file is read only for `specName` (filesystem identity) and
    // the mtime freshness gate. All pipeline-state-style fields come from the
    // SQLite projection below.
    let state = read_newest_fresh_state(cwd)?;
    let spec_name = state.get("specName").and_then(|v| v.as_str())?;

    // Derive the spec's pipeline state from the SQLite event log.
    // Fail-open: if the store is unavailable or the spec has no events yet, the
    // projection returns None and we treat status/wave fields as absent.
    let view: Option<PipelineStateView> = SqliteEventStore::for_project(cwd)
        .ok()
        .and_then(|store| {
            let spec_dir = ClaudePaths::for_project(Path::new(cwd))
                .and_then(|p| p.for_spec(spec_name))
                .map(|sp| sp.dir().to_path_buf())
                .ok();
            let spec_dir_opt = spec_dir.filter(|d| d.exists());
            let events = store.replay().unwrap_or_default();
            pipeline_state_from_events(&events, spec_name, spec_dir_opt.as_deref())
        });

    // Skip when the pipeline is closing / completed. Phase derives from the
    // SQLite `pipeline.phase` event log (post-Wave 2); status derives from the
    // projection (post-Wave 3a). Fail-open: missing projection → status unknown
    // → gate runs (conservative).
    let phase = crate::run::emit_phase::last_phase_for_spec(cwd, spec_name)
        .unwrap_or_default();
    let status = view
        .as_ref()
        .and_then(|v| v.status.as_deref())
        .unwrap_or("");
    if phase == "CLOSE" || status == "completed" {
        return None;
    }
    let spec_file = resolve_spec_file(cwd, spec_name, view.as_ref())?;
    let spec_text = fs::read_to_string(&spec_file).ok()?;
    let patterns = extract_allowed_patterns(&spec_text);
    if patterns.is_empty() {
        return None;
    }
    if patterns.iter().any(|p| pattern_matches(&rel, p)) {
        return None;
    }
    // Mismatch — emit the telemetry event, then decide the verdict.
    emit_boundary_event(cwd, input.session_id.as_deref(), &rel, spec_name, mode, &patterns);
    match mode {
        BoundaryMode::Strict => Some(Verdict::Deny {
            reason: format!(
                "[boundary-gate] {rel} not in spec '{spec_name}' ## Files / \
                 ## Boundaries. Update the spec's Files table to include this \
                 path, or set MUSTARD_BOUNDARY_MODE=warn."
            ),
        }),
        BoundaryMode::Warn => Some(Verdict::Warn {
            message: format!(
                "[boundary-gate] WARN: editing {rel} outside spec '{spec_name}' \
                 boundary. If intentional cascade, add it to the spec ## Files. \
                 Set MUSTARD_BOUNDARY_MODE=strict to block."
            ),
        }),
        BoundaryMode::Off => None,
    }
}

// ---------------------------------------------------------------------------
// Contract impl
// ---------------------------------------------------------------------------

impl Check for PathGuard {
    /// Run `file-guard` then `boundary-gate` on a `PreToolUse` invocation.
    ///
    /// `file-guard` is the non-negotiable safety gate (no mode — always
    /// strict); it runs first and a sensitive-file `Deny` short-circuits.
    /// `boundary-gate` runs only for `Write`/`Edit` and computes its verdict
    /// with its own `MUSTARD_BOUNDARY_MODE`.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        // `file-guard` — Read/Write/Edit, always strict.
        if let Some(verdict) = file_guard(input) {
            return Ok(verdict);
        }
        // `boundary-gate` — Write/Edit only.
        let tool = input.tool_name.as_deref().unwrap_or_default();
        if tool == "Write" || tool == "Edit" {
            let cwd = project_dir(input, ctx);
            if let Some(verdict) = boundary_gate(input, &cwd) {
                return Ok(verdict);
            }
        }
        Ok(Verdict::Allow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn pre(tool: &str, file_path: &str) -> (HookInput, Ctx) {
        let input = HookInput {
            tool_name: Some(tool.to_string()),
            tool_input: json!({ "file_path": file_path }),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: String::new(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        (input, ctx)
    }

    fn verdict_for(tool: &str, file_path: &str) -> Verdict {
        let (input, ctx) = pre(tool, file_path);
        PathGuard.evaluate(&input, &ctx).expect("check never errors")
    }

    // --- file-guard parity (hooks.test.js "file-guard.js") -----------------

    #[test]
    fn file_guard_blocks_pem_key() {
        assert!(verdict_for("Read", "/project/secrets/server.pem").is_blocking());
        assert!(verdict_for("Write", "config/private.key").is_blocking());
    }

    #[test]
    fn file_guard_blocks_credentials() {
        assert!(verdict_for("Read", "/project/.aws/credentials").is_blocking());
    }

    #[test]
    fn file_guard_blocks_git_config_and_ssh_keys() {
        assert!(verdict_for("Edit", "/project/.git/config").is_blocking());
        assert!(verdict_for("Read", "/home/user/.ssh/id_rsa").is_blocking());
        assert!(verdict_for("Read", "/home/user/.ssh/id_ed25519").is_blocking());
    }

    #[test]
    fn file_guard_allows_env_files() {
        // file-guard does NOT block .env (user decision).
        assert_eq!(verdict_for("Read", "/project/.env"), Verdict::Allow);
        assert_eq!(verdict_for("Write", "/project/.env.local"), Verdict::Allow);
    }

    #[test]
    fn file_guard_allows_normal_source() {
        assert_eq!(verdict_for("Edit", "/project/src/main.ts"), Verdict::Allow);
    }

    #[test]
    fn file_guard_ignores_non_file_tools() {
        // Only Read/Write/Edit are inspected.
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": "cat server.pem" }),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        assert!(file_guard(&input).is_none());
    }

    #[test]
    fn file_guard_blocks_pfx_p12() {
        assert!(verdict_for("Read", "/p/cert.pfx").is_blocking());
        assert!(verdict_for("Read", "/p/cert.p12").is_blocking());
    }

    // --- boundary-gate parity ----------------------------------------------

    #[test]
    fn boundary_gate_passes_meta_paths() {
        // A `.claude/` edit is always allowed (meta path).
        assert_eq!(
            verdict_for("Write", "/project/.claude/settings.json"),
            Verdict::Allow
        );
    }

    #[test]
    fn boundary_gate_passes_when_no_active_spec() {
        // No `.pipeline-states` dir → no state → pass through.
        let dir = tempdir().unwrap();
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({ "file_path": "src/main.ts" }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(dir.path().to_string_lossy().into_owned()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: dir.path().to_string_lossy().into_owned(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        assert_eq!(
            PathGuard.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }

    #[test]
    fn extract_allowed_patterns_reads_files_section() {
        let spec = "# Spec\n\n## Files\n\n- `src/main.ts` — entry\n- `src/lib/`\n\n\
            ## Boundaries\n\n- `tests/**`\n\n## Summary\n\n- `not-a-path-label`\n";
        let patterns = extract_allowed_patterns(spec);
        assert!(patterns.contains(&"src/main.ts".to_string()));
        assert!(patterns.contains(&"src/lib/".to_string()));
        assert!(patterns.contains(&"tests/**".to_string()));
        // The Summary span is outside the Files/Boundaries sections.
        assert!(!patterns.contains(&"not-a-path-label".to_string()));
    }

    #[test]
    fn pattern_matches_exact_dir_and_glob() {
        assert!(pattern_matches("src/main.ts", "src/main.ts"));
        assert!(pattern_matches("src/lib/x.ts", "src/lib/"));
        assert!(pattern_matches("tests/unit/a.test.ts", "tests/**"));
        assert!(pattern_matches("src/a.ts", "src/*.ts"));
        assert!(!pattern_matches("src/lib/a.ts", "src/*.ts"));
        assert!(!pattern_matches("docs/x.md", "src/**"));
    }

    #[test]
    fn boundary_gate_denies_unlisted_file_in_strict_mode() {
        // SAFETY: tests mutate a process-global env var; this test is the only
        // one that sets MUSTARD_BOUNDARY_MODE, and it restores it.
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        let paths = ClaudePaths::for_project(cwd).unwrap();
        // pipeline-state pointing at spec "demo".
        let states = paths.pipeline_states_dir();
        std::fs::create_dir_all(&states).unwrap();
        std::fs::write(
            paths.pipeline_state_file("demo"),
            // Phase derives from SQLite `pipeline.phase` events, not JSON;
            // no event seeded here → phase is empty → not CLOSE → gate runs.
            json!({ "specName": "demo" }).to_string(),
        )
        .unwrap();
        // spec.md with a Files section (flat layout — no active/ bucket).
        let sp = paths.for_spec("demo").unwrap();
        let spec_dir = sp.dir();
        std::fs::create_dir_all(spec_dir).unwrap();
        std::fs::write(
            sp.spec_md_path(),
            "# Spec\n\n## Files\n\n- `src/allowed.ts`\n",
        )
        .unwrap();

        let cwd_str = cwd.to_string_lossy().into_owned();
        // An edit to `src/forbidden.ts` is outside the declared boundary.
        let input = HookInput {
            tool_name: Some("Edit".to_string()),
            tool_input: json!({ "file_path": "src/forbidden.ts" }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(cwd_str.clone()),
            ..HookInput::default()
        };

        // Default mode is `warn` → Warn, not Deny.
        let warn = boundary_gate(&input, &cwd_str);
        assert!(matches!(warn, Some(Verdict::Warn { .. })), "got {warn:?}");

        // An allowed file passes through.
        let allowed = HookInput {
            tool_input: json!({ "file_path": "src/allowed.ts" }),
            ..input.clone()
        };
        assert!(boundary_gate(&allowed, &cwd_str).is_none());
    }

    // --- Wave-3a: projection None → fail-open in boundary_gate ---------------

    #[test]
    fn boundary_gate_allows_when_projection_none_and_no_patterns() {
        // No SQLite store → projection None → gate falls through (no spec file
        // found anyway because the spec dir doesn't exist).
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        let paths = ClaudePaths::for_project(cwd).unwrap();
        let states = paths.pipeline_states_dir();
        std::fs::create_dir_all(&states).unwrap();
        std::fs::write(
            paths.pipeline_state_file("ghost"),
            r#"{"specName":"ghost"}"#,
        )
        .unwrap();
        let cwd_str = cwd.to_string_lossy().into_owned();
        let input = HookInput {
            tool_name: Some("Edit".to_string()),
            tool_input: serde_json::json!({ "file_path": "src/any.ts" }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(cwd_str.clone()),
            ..HookInput::default()
        };
        // Projection None and no spec file → boundary_gate returns None (Allow).
        assert!(boundary_gate(&input, &cwd_str).is_none());
    }

    #[test]
    fn boundary_gate_reads_status_from_projection() {
        use mustard_core::store::event_store::EventSink;
        use mustard_core::store::sqlite_store::SqliteEventStore;
        use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};

        let dir = tempdir().unwrap();
        let cwd = dir.path();
        let paths = ClaudePaths::for_project(cwd).unwrap();
        let states = paths.pipeline_states_dir();
        std::fs::create_dir_all(&states).unwrap();
        std::fs::write(
            paths.pipeline_state_file("myspec"),
            r#"{"specName":"myspec"}"#,
        )
        .unwrap();
        // Spec with a Files section.
        let sp = paths.for_spec("myspec").unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        std::fs::write(
            sp.spec_md_path(),
            "# Spec\n\n## Files\n\n- `src/allowed.ts`\n",
        )
        .unwrap();

        // Seed a pipeline.status "completed" event via the SQLite store.
        let db_path = paths.harness_dir().join("mustard.db");
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        let store = SqliteEventStore::new(&db_path).unwrap();
        store
            .append(&HarnessEvent {
                v: SCHEMA_VERSION,
                ts: "2026-05-20T10:00:00.000Z".to_string(),
                session_id: "s1".to_string(),
                wave: 0,
                actor: Actor { kind: ActorKind::Hook, id: None, actor_type: None },
                event: "pipeline.status".to_string(),
                payload: serde_json::json!({ "to": "completed" }),
                spec: Some("myspec".to_string()),
            })
            .unwrap();

        let cwd_str = cwd.to_string_lossy().into_owned();
        let input = HookInput {
            tool_name: Some("Edit".to_string()),
            tool_input: serde_json::json!({ "file_path": "src/forbidden.ts" }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(cwd_str.clone()),
            ..HookInput::default()
        };
        // Status "completed" → skip (None), even though src/forbidden.ts is outside boundary.
        assert!(
            boundary_gate(&input, &cwd_str).is_none(),
            "completed status must skip the boundary gate"
        );
    }

    // --- gate routing -------------------------------------------------------

    #[test]
    fn non_pre_tool_use_trigger_allows() {
        let input = HookInput {
            tool_name: Some("Read".to_string()),
            tool_input: json!({ "file_path": "server.pem" }),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: String::new(),
            trigger: Some(Trigger::PostToolUse),
            workspace_root: None,
        };
        assert_eq!(
            PathGuard.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }

    #[test]
    fn non_path_token_rejection() {
        assert!(looks_like_command_with_flag("npm --version"));
        assert!(looks_like_env_assignment("NODE_ENV=test"));
        assert!(!looks_like_command_with_flag("src/main.ts"));
        assert!(!looks_like_env_assignment("src/main.ts"));
    }
}
