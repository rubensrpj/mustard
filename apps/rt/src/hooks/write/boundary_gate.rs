//! `boundary_gate` — flag a Write/Edit outside the active spec's declared
//! boundary (`## Files` / `## Boundaries`).
//!
//! ## Scope
//!
//! ONE behavior: a `PreToolUse(Write|Edit)` gate that checks the edited path
//! against the active spec's declared file list. Mode `MUSTARD_BOUNDARY_MODE`
//! (default `warn`): warn → advisory, strict → deny.
//!
//! The sensitive-file law that used to live here as `file-guard`
//! (`credentials*`, `*.pem`, `*.key`, `.git/config`, SSH keys, `*.pfx`,
//! `*.p12`) is now two-layer: `settings.json permissions.deny`
//! `Read`/`Edit`/`Write` globs (first line, survives `/unhook`) + the
//! [`super::secret_files`] residue, which keeps the old case-insensitive
//! full-path substring semantics the globs cannot express.
//!
//! This module also hosts the shared path helper [`relative_to_cwd`] that
//! `work_branch_gate` consumes to scope branching to in-repo mutations. The
//! `file_path` extraction it used to host now lives on
//! [`HookInput::file_path`](mustard_core::domain::model::contract::HookInput::file_path).
//!
//! ## W3C migration
//!
//! `boundary_gate` previously used the SQLite event store to feed
//! `pipeline_state_from_events`. The W3C migration replaces that with two
//! filesystem reads:
//!
//! 1. `read_harness_events_from_ndjson_dir` — per-spec NDJSON event log.
//! 2. Spec header (`### Stage:` / `### Outcome:`) — fallback to determine
//!    whether the spec is already completed/closed when no `pipeline.status`
//!    event exists in the NDJSON log.

use mustard_core::platform::error::Error;
use mustard_core::io::fs;
use mustard_core::ClaudePaths;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::domain::model::event::HarnessEvent;
use mustard_core::view::projection::read_harness_events_from_ndjson_dir;
use crate::util::glob::glob_match;
use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::commands::{PipelineStateView, pipeline_state_from_events};

/// The spec-boundary gate.
pub struct BoundaryGate;

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

/// Resolve `MUSTARD_BOUNDARY_MODE` in cascade: env var → `mustard.json`
/// (`gates.boundary`, supplied as `config_override`) → built-in `warn`. An env
/// var set to a non-empty value wins; an absent string OR an unrecognised value
/// falls back to `warn`.
fn boundary_mode(config_override: Option<&str>) -> BoundaryMode {
    let s = std::env::var("MUSTARD_BOUNDARY_MODE")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| config_override.map(str::to_string));
    match s.unwrap_or_default().to_ascii_lowercase().as_str() {
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

/// Resolve the spec a Write/Edit should be checked against, fail-open `None`.
///
/// Priority:
/// 1. [`crate::shared::context::spec_for_session`] — the
///    `.session/<id>/active-spec` marker bound to THIS session. This is the
///    canonical session→spec link the event router maintains; the rest of the
///    harness already resolves the active spec this way. Using it here means
///    the boundary check runs against the spec the session is actually
///    working on.
/// 2. `MUSTARD_ACTIVE_SPEC` — the explicit env override `/feature` & `/spec` set.
/// 3. Legacy: the newest *fresh* (`mtime < 10 min`) `.pipeline-states/*.json`
///    `specName`. Kept only for flows that carry no session binding; the
///    freshness window stops an ancient leftover from misattributing.
///
/// Why not the legacy scan first: picking the newest state file by mtime alone
/// attributes an edit to whichever spec last touched a `.pipeline-states` file —
/// which can be a *finished, leftover* spec (a stale `payables-…`), raising a
/// BOUNDARY WARNING for a spec the current session never touched.
fn resolve_boundary_spec(cwd: &str, session_id: Option<&str>) -> Option<String> {
    if let Some(sid) = session_id.filter(|s| !s.is_empty() && *s != "unknown") {
        if let Some(spec) = crate::shared::context::spec_for_session(cwd, sid) {
            return Some(spec);
        }
    }
    if let Ok(s) = std::env::var("MUSTARD_ACTIVE_SPEC") {
        if !s.is_empty() {
            return Some(s);
        }
    }
    read_newest_fresh_state(cwd)
        .and_then(|s| s.get("specName").and_then(|v| v.as_str()).map(str::to_string))
}

/// Resolve the spec file(s) whose `## Files` / `## Boundaries` a Write/Edit is
/// checked against — `.claude/spec/{specName}/spec.md` (flat layout), with a
/// wave-plan branch that resolves `wave-{N}-*/spec.md` instead.
///
/// ## Which wave, and why it is not one number
///
/// A dispatch round runs EVERY wave of the lowest incomplete dependency level at
/// once ([`crate::commands::pipeline::wave_advance`]'s own contract), so while a
/// round is in flight there are several waves writing. The projection's
/// `currentWave` is a scalar — `max(completedWaves) + 1` — so during round 1 it
/// says `1` to every sibling. Checking each of them against wave 1's file list
/// meant the wave-2 agent got a boundary warning on every single edit, INCLUDING
/// the files wave 2's own `## Files` declares. A gate that cries wolf on a
/// correctly declared file teaches the operator to ignore the gate.
///
/// So the wave is resolved in this order:
///
/// 1. **The wave the WRITER belongs to** — the stamp its dispatch carried into
///    its own transcript, read back by
///    [`crate::hooks::task::subagent_inject::wave_from_child_transcript`]. This
///    is the only signal that differs between siblings in flight; when it
///    resolves, the boundary is exactly one wave's and the gate is at its
///    narrowest.
/// 2. **Every wave of the round in flight** — the union of the `## Files` of the
///    waves at the lowest incomplete dependency level. Strictly narrower than
///    allowing everything, and it can never accuse a correctly declared file.
///    This is the honest answer when the writer cannot be identified: the gate
///    stays on, it just stops naming a wave it did not establish.
///
/// `spec_name` is the spec identifier. `view` is the typed projection — `None`
/// means wave info is unknown, so the flat `spec.md` is used. `input` is the
/// hook invocation, the only per-writer thing in scope.
///
/// Fail-open: an unresolvable spec dir yields an empty list and the caller
/// passes the edit through.
fn resolve_boundary_files(
    cwd: &str,
    spec_name: &str,
    view: Option<&PipelineStateView>,
    input: &HookInput,
) -> Vec<std::path::PathBuf> {
    let Ok(base) = ClaudePaths::for_project(Path::new(cwd))
        .and_then(|p| p.for_spec(spec_name))
        .map(|sp| sp.dir().to_path_buf())
    else {
        return Vec::new();
    };
    if !base.exists() {
        return Vec::new();
    }
    let is_wave_plan = view.and_then(|v| v.is_wave_plan).unwrap_or(false);
    if is_wave_plan {
        if let Some(wave) = crate::hooks::task::subagent_inject::wave_from_child_transcript(input) {
            if let Some(file) = wave_boundary_file(&base, wave) {
                return vec![file];
            }
        }
        let round: Vec<std::path::PathBuf> = waves_in_flight(cwd, spec_name, view)
            .into_iter()
            .filter_map(|wave| wave_boundary_file(&base, wave))
            .collect();
        if !round.is_empty() {
            return round;
        }
    }
    let root = base.join("spec.md");
    if root.exists() { vec![root] } else { Vec::new() }
}

/// The `wave-{wave}-*/spec.md` under `base`, or `None` when the wave has no
/// directory (or no `spec.md`) on disk. The role is unknown here, so the
/// shared resolver is asked with an empty one and takes its `wave-{N}-*`
/// fallback — the SAME scanner the dispatch used to find the wave's spec, so
/// the boundary read here cannot disagree with the boundary the wave's agent
/// was handed.
fn wave_boundary_file(base: &Path, wave: u32) -> Option<std::path::PathBuf> {
    crate::commands::pipeline::dispatch_plan::wave_spec_path(base, wave, "")
}

/// The waves of the round currently in flight: those at the LOWEST dependency
/// level that has not completed, which is exactly the set `wave-advance`
/// dispatches together.
///
/// Derived from the same two sources the dispatcher uses — the wave plan's DAG
/// ([`crate::commands::pipeline::dispatch_plan::build_plan`]) and the
/// projection's `completedWaves` — so this set cannot drift from the round the
/// orchestrator actually launched.
///
/// Fail-open: a spec whose plan yields nothing pending falls back to the
/// projection's scalar, so a late edit still lands on a wave rather than on
/// nothing; an empty plan yields an empty set and the caller falls through to
/// the parent `spec.md`.
fn waves_in_flight(cwd: &str, spec_name: &str, view: Option<&PipelineStateView>) -> Vec<u32> {
    use crate::commands::pipeline::dispatch_plan;

    let project = Path::new(cwd);
    let spec_dir = dispatch_plan::resolve_spec_dir(project, spec_name);
    let plan = dispatch_plan::build_plan(project, &spec_dir, spec_name, None);
    let completed: &[u32] = view.map_or(&[], |v| v.completed_waves.as_slice());
    let pending: Vec<&dispatch_plan::DispatchItem> = plan
        .iter()
        .filter(|it| !completed.contains(&it.wave))
        .collect();
    let Some(level) = pending.iter().map(|it| it.level).min() else {
        return view.map_or_else(Vec::new, |v| vec![v.current_wave]);
    };
    pending
        .iter()
        .filter(|it| it.level == level)
        .map(|it| it.wave)
        .collect()
}

/// Name the boundary the gate ACTUALLY checked, so the author can go and look at
/// it: `{spec}/{wave-dir}` for one wave, `{spec}/{a}+{b}` when the round's waves
/// were taken together, and the bare spec slug when the parent `spec.md` was the
/// boundary.
///
/// Naming matters here: the parent's own `## Files` may well list the edited
/// path, so citing the parent slug would send the author to a section that
/// already contains it.
fn boundary_label(spec_name: &str, files: &[std::path::PathBuf]) -> String {
    let waves: Vec<&str> = files
        .iter()
        .filter_map(|f| f.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str()))
        .filter(|n| n.starts_with("wave-"))
        .collect();
    if waves.is_empty() {
        spec_name.to_string()
    } else {
        format!("{spec_name}/{}", waves.join("+"))
    }
}

/// Extract the allowed path patterns from a spec's `## Files` and
/// `## Boundaries` (and their PT equivalents). Port of `extractAllowedPatterns`.
///
/// Two harvests per section line, deduplicated into one set:
///
/// 1. **Backtick spans** — the original rule. Accepts globs (`src/**`) and
///    directory prefixes (`src/lib/`) besides concrete files.
/// 2. **Bare tokens** — a path declared WITHOUT backticks. A MIXED spec is the
///    dangerous shape: the backticked entries make the set non-empty, so every
///    bare-declared file used to warn. Bare tokens pass through the crate's
///    single strict path recogniser (see [`bare_path_pattern`]), so prose never
///    becomes a pattern — a bare glob or directory still needs backticks.
fn extract_allowed_patterns(spec_text: &str) -> Vec<String> {
    fn push_unique(patterns: &mut Vec<String>, candidate: &str) {
        if !patterns.iter().any(|p| p == candidate) {
            patterns.push(candidate.to_string());
        }
    }
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
            push_unique(&mut patterns, candidate);
        }
        // Bare tokens: whitespace-split so bullets (`- src/a.ts — why`) and
        // table rows (`| src/a.ts | why |`) both contribute their path.
        for token in line.split_whitespace() {
            if let Some(candidate) = bare_path_pattern(token) {
                if candidate.len() <= 200 {
                    push_unique(&mut patterns, &candidate);
                }
            }
        }
    }
    patterns
}

/// A bare (un-backticked) token that reads as ONE concrete file path, trimmed
/// of surrounding punctuation (list dashes, table pipes, commas, stray
/// backticks) and forward-slash normalised — `None` for prose.
///
/// The JUDGEMENT is delegated to the crate's single strict path recogniser,
/// [`crate::commands::review::analyze_validation::looks_like_file_path`] — one
/// definition of "reads as a path", tuned in one place. Only the edge trim
/// that recogniser applies internally is mirrored here, so the stored pattern
/// equals the token the recogniser judged.
fn bare_path_pattern(token: &str) -> Option<String> {
    if !crate::commands::review::analyze_validation::looks_like_file_path(token) {
        return None;
    }
    let trimmed = token.trim_matches(|c: char| {
        !c.is_ascii_alphanumeric() && c != '.' && c != '/' && c != '\\' && c != '-' && c != '_'
    });
    Some(trimmed.replace('\\', "/"))
}

/// `true` if `line` is a `## Files`/`## Boundaries` (or PT) H2 heading.
fn is_files_or_boundaries_heading(line: &str) -> bool {
    let lower = line.trim().to_ascii_lowercase();
    h2_named(&lower, "files")
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

/// Compute the path of `file_path` relative to `cwd`, forward-slash
/// normalised. Returns `None` when `file_path` escapes `cwd` (`../`) — the
/// caller treats that the same as a meta path (skip). Mirrors the JS
/// `path.relative(cwd, abs)` + `rel.startsWith('../')` check.
/// `pub(crate)`: `work_branch_gate` reuses it to scope branching to in-repo
/// mutations.
pub(crate) fn relative_to_cwd(cwd: &str, file_path: &str) -> Option<String> {
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

/// Collect harness events for `spec_name` from the per-spec NDJSON event log.
///
/// W3C: replaces the broad `store.replay()` (all events from SQLite) with a
/// targeted read of the per-spec `.events/` directory. Fail-open: an absent
/// or unreadable directory returns an empty vec.
fn read_spec_events(cwd: &str, spec_name: &str) -> Vec<HarnessEvent> {
    let Ok(cp) = ClaudePaths::for_project(cwd) else {
        return Vec::new();
    };
    let Ok(sp) = cp.for_spec(spec_name) else {
        return Vec::new();
    };
    read_harness_events_from_ndjson_dir(&sp.events_dir())
}

/// Determine whether a spec is completed/closed by reading its lifecycle
/// metadata. Used as a fallback when the NDJSON event log contains no
/// `pipeline.status` event for the spec.
///
/// **`meta.json` is the single source of truth** (`#outcome`); the legacy
/// `### Stage:` / `### Outcome:` `spec.md` header is the fallback for
/// un-migrated specs.
///
/// Returns `true` when the metadata declares a terminal state (Completed,
/// Cancelled, Superseded, …). Returns `false` on any read/parse error
/// (fail-open: the gate continues running rather than silently blocking all
/// edits).
fn spec_header_is_terminal(cwd: &str, spec_name: &str) -> bool {
    use mustard_core::domain::spec;
    let Ok(cp) = ClaudePaths::for_project(cwd) else {
        return false;
    };
    let Ok(sp) = cp.for_spec(spec_name) else {
        return false;
    };
    let spec_md = sp.spec_md_path();
    // meta.json wins: a non-`Active` outcome is terminal.
    if let Some(m) = mustard_core::domain::meta::read_meta_beside(&spec_md) {
        if let Some(outcome) = m.outcome.as_deref().and_then(mustard_core::Outcome::parse) {
            return outcome != mustard_core::Outcome::Active;
        }
    }
    // Legacy fallback: read the lifecycle header from the markdown.
    let Ok(text) = std::fs::read_to_string(&spec_md) else {
        return false;
    };
    match spec::parse_state(&text) {
        Some(state) => !state.is_active(),
        None => false,
    }
}

/// The boundary check: flag a Write/Edit outside the active spec's declared
/// `## Files` / `## Boundaries`.
///
/// 1:1 with `boundary-gate.js` — every early `process.exit(0)` maps to
/// `None` (pass through). A real mismatch → `Deny` in strict mode, `Warn` in
/// warn mode.
///
/// W3C migration: spec pipeline-state fields (`isWavePlan`, `currentWave`,
/// `status`) are derived from the NDJSON event log via
/// `pipeline_state_from_events`. The JSON state file is still consulted for
/// `specName` (filesystem identity) and the mtime freshness gate. When the
/// NDJSON log contains no `pipeline.status` event, the spec header
/// (`### Stage:` / `### Outcome:`) is used as a fallback to detect terminal
/// state. Fail-open: projection `None` → treat status as empty and wave info
/// as unknown.
fn boundary_gate(input: &HookInput, cwd: &str) -> Option<Verdict> {
    // Cascade override: load the project config once and read gates.boundary.
    let gates = crate::shared::context::project_config_cached(Path::new(cwd)).gates;
    let mode = boundary_mode(gates.boundary.as_deref());
    if mode == BoundaryMode::Off {
        return None;
    }
    let file_path = input.file_path()?;
    // Compute rel; an escaping (`../`) path → None → skip.
    let rel = relative_to_cwd(cwd, &file_path)?;
    if is_meta_path(&rel) {
        return None;
    }
    // Resolve the spec THIS edit belongs to. Canonical source: the session→spec
    // binding (`active-spec` marker) the event router maintains — the same link
    // the rest of the harness uses — falling back to the legacy newest-fresh
    // `.pipeline-states` scan only for a session with no binding (see
    // `resolve_boundary_spec`).
    let spec_name = resolve_boundary_spec(cwd, input.session_id.as_deref())?;
    let spec_name = spec_name.as_str();

    // Collect events from the NDJSON event log (W3C — no SQLite).
    let events = read_spec_events(cwd, spec_name);

    // Derive the spec's pipeline state from the NDJSON event log.
    // Fail-open: absent events dir or no events → projection is None.
    let spec_dir = ClaudePaths::for_project(Path::new(cwd))
        .and_then(|p| p.for_spec(spec_name))
        .map(|sp| sp.dir().to_path_buf())
        .ok();
    let spec_dir_opt = spec_dir.filter(|d| d.exists());
    let view: Option<PipelineStateView> =
        pipeline_state_from_events(&events, spec_name, spec_dir_opt.as_deref());

    // Skip when the pipeline is closing / completed.
    // - Phase: from NDJSON `pipeline.phase` events via `emit_phase`.
    // - Status: from the NDJSON projection; falls back to spec header when the
    //   projection has no status (e.g. no pipeline.status event yet in NDJSON).
    let phase = crate::commands::event::emit_phase::last_phase_for_spec(cwd, spec_name)
        .unwrap_or_default();
    let status = view
        .as_ref()
        .and_then(|v| v.status.as_deref())
        .unwrap_or("");
    // Header fallback: if projection yields no status, check spec.md directly.
    let is_terminal = status == "completed"
        || (status.is_empty() && spec_header_is_terminal(cwd, spec_name));
    if phase == "CLOSE" || is_terminal {
        return None;
    }
    // The boundary is a LIST because a dispatch round can have several waves
    // writing at once — see `resolve_boundary_files`. The declared patterns are
    // their union: a path any wave of the round declared is inside the boundary.
    let spec_files = resolve_boundary_files(cwd, spec_name, view.as_ref(), input);
    let mut patterns: Vec<String> = Vec::new();
    for spec_file in &spec_files {
        let Ok(spec_text) = fs::read_to_string(spec_file) else {
            continue;
        };
        for pattern in extract_allowed_patterns(&spec_text) {
            if !patterns.contains(&pattern) {
                patterns.push(pattern);
            }
        }
    }
    if patterns.is_empty() {
        return None;
    }
    if patterns.iter().any(|p| pattern_matches(&rel, p)) {
        return None;
    }
    let boundary_name = boundary_label(spec_name, &spec_files);
    // Mismatch — decide the verdict.
    match mode {
        BoundaryMode::Strict => Some(Verdict::Deny {
            reason: format!(
                "[boundary-gate] {rel} not in '{boundary_name}' ## Files / \
                 ## Boundaries. Update that spec's Files to include this \
                 path, or set MUSTARD_BOUNDARY_MODE=warn."
            ),
        }),
        BoundaryMode::Warn => Some(Verdict::Warn {
            message: format!(
                "[boundary-gate] WARN: editing {rel} outside '{boundary_name}' \
                 boundary. If intentional cascade, add it to that spec's \
                 ## Files. Set MUSTARD_BOUNDARY_MODE=strict to block."
            ),
        }),
        BoundaryMode::Off => None,
    }
}

// ---------------------------------------------------------------------------
// Contract impl
// ---------------------------------------------------------------------------

impl Check for BoundaryGate {
    /// Run the boundary check on a `PreToolUse(Write|Edit)` invocation; any
    /// other trigger/tool self-allows.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        let tool = input.tool_name.as_deref().unwrap_or_default();
        if tool == "Write" || tool == "Edit" {
            let cwd = ctx.project_dir_or_cwd(input);
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
            inject_only: None,
        };
        (input, ctx)
    }

    fn verdict_for(tool: &str, file_path: &str) -> Verdict {
        let (input, ctx) = pre(tool, file_path);
        BoundaryGate.evaluate(&input, &ctx).expect("check never errors")
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
    fn boundary_gate_ignores_non_write_tools() {
        // Read (or any non-Write/Edit tool) is not this gate's business.
        assert_eq!(verdict_for("Read", "src/main.ts"), Verdict::Allow);
        assert_eq!(verdict_for("Bash", "src/main.ts"), Verdict::Allow);
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
            inject_only: None,
        };
        assert_eq!(
            BoundaryGate.evaluate(&input, &ctx).expect("no error"),
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
            // Phase derives from NDJSON `pipeline.phase` events, not JSON;
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

    // --- W3C: NDJSON event source -------------------------------------------

    #[test]
    fn boundary_gate_allows_when_no_events_and_no_patterns() {
        // No events at all → projection None → gate falls through (no spec
        // file patterns found because the spec dir doesn't exist).
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
        // No spec dir → no patterns → boundary_gate returns None (Allow).
        assert!(boundary_gate(&input, &cwd_str).is_none());
    }

    #[test]
    fn boundary_gate_attributes_to_session_spec_not_newest_state() {
        // Regression (#1): the gate must check the edit against the spec THIS
        // session is bound to (the `active-spec` marker), NOT the newest
        // `.pipeline-states` file by mtime. Otherwise a finished, leftover spec
        // misattributes every edit and raises a BOUNDARY WARNING for a spec the
        // session never touched.
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        let paths = ClaudePaths::for_project(cwd).unwrap();
        let states = paths.pipeline_states_dir();
        std::fs::create_dir_all(&states).unwrap();
        // A stale leftover state file — newest by mtime, would win the legacy scan.
        std::fs::write(
            paths.pipeline_state_file("old-leftover"),
            r#"{"specName":"old-leftover"}"#,
        )
        .unwrap();
        let old = paths.for_spec("old-leftover").unwrap();
        std::fs::create_dir_all(old.dir()).unwrap();
        std::fs::write(old.spec_md_path(), "# Old\n## Files\n- `src/old.ts`\n").unwrap();
        // The current run's spec + its own boundary.
        let cur = paths.for_spec("current-run").unwrap();
        std::fs::create_dir_all(cur.dir()).unwrap();
        std::fs::write(cur.spec_md_path(), "# Cur\n## Files\n- `src/current.ts`\n").unwrap();

        let cwd_str = cwd.to_string_lossy().into_owned();
        // Bind THIS session to current-run.
        crate::shared::context::bind_session_spec(&cwd_str, "sess-1", "current-run");

        let input = HookInput {
            tool_name: Some("Edit".to_string()),
            tool_input: serde_json::json!({ "file_path": "src/forbidden.ts" }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(cwd_str.clone()),
            session_id: Some("sess-1".to_string()),
            ..HookInput::default()
        };
        match boundary_gate(&input, &cwd_str) {
            Some(Verdict::Warn { message }) => {
                assert!(
                    message.contains("current-run"),
                    "must cite the session spec: {message}"
                );
                assert!(
                    !message.contains("old-leftover"),
                    "must NOT cite the leftover spec: {message}"
                );
            }
            other => panic!("expected Warn citing current-run, got {other:?}"),
        }
    }

    #[test]
    fn boundary_gate_reads_terminal_status_from_spec_header() {
        // W3C: terminal state is detected via spec header fallback (no SQLite).
        // A spec whose header says `### Outcome: Completed` must skip the gate.
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
        // Spec with Completed header + Files section.
        let sp = paths.for_spec("myspec").unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        std::fs::write(
            sp.spec_md_path(),
            "# Spec\n### Stage: Close\n### Outcome: Completed\n\n\
             ## Files\n\n- `src/allowed.ts`\n",
        )
        .unwrap();
        // No NDJSON events seeded → projection yields None; header fallback fires.

        let cwd_str = cwd.to_string_lossy().into_owned();
        let input = HookInput {
            tool_name: Some("Edit".to_string()),
            tool_input: serde_json::json!({ "file_path": "src/forbidden.ts" }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(cwd_str.clone()),
            ..HookInput::default()
        };
        // Terminal header → skip (None), even though src/forbidden.ts is outside boundary.
        assert!(
            boundary_gate(&input, &cwd_str).is_none(),
            "completed spec header must skip the boundary gate"
        );
    }

    /// AC-12 — when the gate checks an edit against a WAVE's file list, the
    /// warning names that wave as the boundary it checked, not the parent slug
    /// alone. The parent's own `## Files` deliberately LISTS the edited path
    /// here: pointing the author at the parent would send them to a section
    /// that already contains the file, while the boundary that actually
    /// refused it was the current wave's.
    #[test]
    fn boundary_warning_names_the_boundary_it_checked() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        let paths = ClaudePaths::for_project(cwd).unwrap();
        let sp = paths.for_spec("demo").unwrap();
        let spec_dir = sp.dir().to_path_buf();
        std::fs::create_dir_all(&spec_dir).unwrap();
        // Parent lists BOTH files; the wave declares only one of them.
        std::fs::write(
            sp.spec_md_path(),
            "# Spec\n\n## Files\n\n- `src/edited.ts`\n- `src/wave.ts`\n",
        )
        .unwrap();
        // `wave-plan.md` flips the projection's is_wave_plan FS fallback.
        std::fs::write(spec_dir.join("wave-plan.md"), "# Wave Plan\n").unwrap();
        let wave_dir = spec_dir.join("wave-1-proof");
        std::fs::create_dir_all(&wave_dir).unwrap();
        std::fs::write(
            wave_dir.join("spec.md"),
            "# W1\n\n## Files\n\n- `src/wave.ts`\n",
        )
        .unwrap();

        let cwd_str = cwd.to_string_lossy().into_owned();
        // One non-terminal event so the projection exists (current_wave
        // defaults to 1 → the gate resolves wave-1-proof/spec.md). Written via
        // the writer directly, with wave_role=None, so an ambient
        // MUSTARD_ACTIVE_WAVE cannot re-route it into a wave subdir.
        crate::shared::events::writer_ndjson::write_event_with_ts(
            cwd,
            Some("demo"),
            None,
            "sess-wave",
            "pipeline.status",
            "pipeline",
            None,
            Some("sess-wave"),
            Some("boundary-test"),
            None,
            &json!({ "to": "active" }),
            Some("2026-07-30T00:00:00.000Z"),
        )
        .expect("seed event written");
        crate::shared::context::bind_session_spec(&cwd_str, "sess-wave", "demo");

        let input = HookInput {
            tool_name: Some("Edit".to_string()),
            tool_input: json!({ "file_path": "src/edited.ts" }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(cwd_str.clone()),
            session_id: Some("sess-wave".to_string()),
            ..HookInput::default()
        };
        match boundary_gate(&input, &cwd_str) {
            Some(Verdict::Warn { message }) => {
                assert!(
                    message.contains("wave-1-proof"),
                    "the warning must name the WAVE boundary it checked: {message}"
                );
            }
            other => panic!("expected Warn naming the wave boundary, got {other:?}"),
        }
        // The file the wave itself declares passes against the same boundary.
        let allowed = HookInput {
            tool_input: json!({ "file_path": "src/wave.ts" }),
            ..input
        };
        assert!(boundary_gate(&allowed, &cwd_str).is_none());
    }

    /// Seed a spec whose round dispatches waves 1 and 2 TOGETHER (same
    /// dependency level) with wave 3 behind them, each wave declaring one file
    /// of its own, and bind `sess` to it. Returns the project root as a string.
    fn parallel_round_spec(cwd: &Path, sid: &str) -> String {
        let paths = ClaudePaths::for_project(cwd).expect("paths");
        let sp = paths.for_spec("round").expect("spec paths");
        let spec_dir = sp.dir().to_path_buf();
        std::fs::create_dir_all(&spec_dir).expect("spec dir");
        std::fs::write(
            spec_dir.join("wave-plan.md"),
            "| Wave | Spec | Role | Depends on | Summary |\n\
             |------|------|------|------------|---------|\n\
             | 1 | [[wave-1-carry]] | carry | — | carries |\n\
             | 2 | [[wave-2-reap]] | reap | — | reaps |\n\
             | 3 | [[wave-3-isolate]] | isolate | [[wave-1-carry]] | isolates |\n",
        )
        .expect("wave plan");
        for (dir, file) in [
            ("wave-1-carry", "src/carry.rs"),
            ("wave-2-reap", "src/reap.rs"),
            ("wave-3-isolate", "src/isolate.rs"),
        ] {
            std::fs::create_dir_all(spec_dir.join(dir)).expect("wave dir");
            std::fs::write(
                spec_dir.join(dir).join("spec.md"),
                format!("# {dir}\n\n## Files\n\n- `{file}`\n"),
            )
            .expect("wave spec");
        }
        let cwd_str = cwd.to_string_lossy().into_owned();
        // One non-terminal event so the projection exists. Nothing has completed,
        // so its scalar `current_wave` is 1 — which is the whole problem.
        crate::shared::events::writer_ndjson::write_event_with_ts(
            cwd,
            Some("round"),
            None,
            sid,
            "pipeline.status",
            "pipeline",
            None,
            Some(sid),
            Some("boundary-test"),
            None,
            &json!({ "to": "active" }),
            Some("2026-08-12T00:00:00.000Z"),
        )
        .expect("seed event written");
        crate::shared::context::bind_session_spec(&cwd_str, sid, "round");
        cwd_str
    }

    /// A `PreToolUse(Edit)` on `file` from an unidentified writer.
    fn round_edit(cwd_str: &str, sid: &str, file: &str) -> HookInput {
        HookInput {
            tool_name: Some("Edit".to_string()),
            tool_input: json!({ "file_path": file }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(cwd_str.to_string()),
            session_id: Some(sid.to_string()),
            ..HookInput::default()
        }
    }

    /// A dispatch ROUND runs every wave of the lowest incomplete dependency
    /// level AT ONCE, so waves 1 and 2 are both writing while the projection's
    /// `currentWave` — a scalar, `max(completedWaves) + 1` — still reads 1.
    /// Resolving the boundary from that scalar checked BOTH agents against wave
    /// 1's file list, so the wave-2 agent was warned on every edit it made,
    /// including on the files its own `## Files` declares. A gate that cries
    /// wolf on a correctly declared file trains the operator to ignore the gate.
    ///
    /// Three-sided, because a gate that never warns would pass a one-sided test:
    /// a sibling's declared file passes, a file NO wave of the round declared
    /// still warns, and the warning names the boundary it actually checked.
    #[test]
    fn a_parallel_siblings_declared_file_is_not_accused() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        let sid = "sess-round";
        let cwd_str = parallel_round_spec(cwd, sid);

        // The sibling's own declared file — the measured false accusation.
        assert!(
            boundary_gate(&round_edit(&cwd_str, sid, "src/reap.rs"), &cwd_str).is_none(),
            "wave 2 declares src/reap.rs in its own ## Files"
        );
        assert!(
            boundary_gate(&round_edit(&cwd_str, sid, "src/carry.rs"), &cwd_str).is_none(),
            "wave 1 declares src/carry.rs in its own ## Files"
        );

        // Wave 3 is at the NEXT level — not in this round — so its file is
        // outside the boundary, and the warning names the round it checked.
        match boundary_gate(&round_edit(&cwd_str, sid, "src/isolate.rs"), &cwd_str) {
            Some(Verdict::Warn { message }) => {
                assert!(
                    message.contains("wave-1-carry+wave-2-reap"),
                    "the warning must name the round it checked: {message}"
                );
            }
            other => panic!("a file no wave of the round declared must warn, got {other:?}"),
        }
    }

    /// When the WRITER can be identified, the boundary is that one wave's — the
    /// gate at its narrowest. Same round as
    /// [`a_parallel_siblings_declared_file_is_not_accused`], but the hook input
    /// carries what a real `PreToolUse` inside a subagent carries (`agent_id` +
    /// `transcript_path`), which locates the child's own transcript and the wave
    /// stamp its dispatch left on the first line.
    ///
    /// The stamp is produced by the REAL dispatch expansion here, never written
    /// by hand: a fixture-shaped marker would keep passing after the stamp
    /// format moved.
    #[test]
    fn the_writing_agents_own_wave_narrows_the_boundary() {
        use crate::commands::agent::agent_prompt_render::PROMPT_REF_MARKER;
        use crate::hooks::task::subagent_inject::SubagentInject;

        let dir = tempdir().unwrap();
        let cwd = dir.path();
        let sid = "sess-narrow";
        let cwd_str = parallel_round_spec(cwd, sid);

        // The dispatch the wave-2 agent received, expanded + stamped by the hook.
        let rel = ".claude/spec/round/.dispatch/wave-2-reap.first.prompt.md";
        std::fs::create_dir_all(cwd.join(".claude/spec/round/.dispatch")).expect("dispatch dir");
        std::fs::write(cwd.join(rel), "<!-- PREFIX-STABLE -->\n## ROLE\nROLE: reap\n")
            .expect("rendered prompt");
        let verdict = SubagentInject
            .evaluate(
                &HookInput {
                    tool_name: Some("Task".to_string()),
                    tool_input: json!({
                        "prompt": format!("{PROMPT_REF_MARKER} {rel}\nStub — pass verbatim.\n"),
                        "subagent_type": "impl",
                    }),
                    hook_event_name: Some("PreToolUse".to_string()),
                    ..HookInput::default()
                },
                &Ctx {
                    project_dir: cwd_str.clone(),
                    trigger: Some(Trigger::PreToolUse),
                    workspace_root: None,
                    inject_only: None,
                },
            )
            .expect("hook must not error");
        let Verdict::Rewrite { tool_input } = verdict else {
            panic!("a ref stub must be rewritten, got {verdict:?}");
        };
        let stamped = tool_input
            .get("prompt")
            .and_then(|v| v.as_str())
            .expect("rewritten prompt")
            .to_string();

        // Claude Code persists that prompt as the first line of the CHILD's own
        // transcript, stored beside the parent's under the documented layout.
        let transcripts = cwd.join("transcripts");
        std::fs::create_dir_all(transcripts.join("sess/subagents")).expect("child dir");
        let parent = transcripts.join("sess.jsonl");
        std::fs::write(&parent, "{\"type\":\"summary\"}\n").expect("parent transcript");
        std::fs::write(
            transcripts.join("sess/subagents/agent-child2.jsonl"),
            format!(
                "{}\n",
                json!({ "type": "user", "isSidechain": true, "message": { "content": stamped } })
            ),
        )
        .expect("child transcript");

        let from_wave_2 = |file: &str| HookInput {
            agent_id: Some("child2".to_string()),
            raw: json!({ "transcript_path": parent.to_string_lossy() }),
            ..round_edit(&cwd_str, sid, file)
        };

        assert!(
            boundary_gate(&from_wave_2("src/reap.rs"), &cwd_str).is_none(),
            "the writing wave's own declared file always passes"
        );
        match boundary_gate(&from_wave_2("src/carry.rs"), &cwd_str) {
            Some(Verdict::Warn { message }) => {
                assert!(
                    message.contains("round/wave-2-reap"),
                    "an identified writer is checked against its OWN wave alone: {message}"
                );
                assert!(
                    !message.contains("wave-1-carry"),
                    "the sibling's boundary is not this writer's: {message}"
                );
            }
            other => panic!("a sibling's file is outside this writer's boundary, got {other:?}"),
        }
    }

    /// A MIXED spec — some Files entries backticked, some bare — is the
    /// dangerous shape: the backticked entries make the set non-empty, so a
    /// bare-declared file used to warn on every edit. Bare bullet and table
    /// declarations must contribute; prose around them must not.
    #[test]
    fn extract_allowed_patterns_accepts_bare_declared_paths() {
        let spec = "# Spec\n\n## Files\n\n\
            - `src/ticked.ts` — with backticks\n\
            - src/bare.ts — declared without backticks\n\
            | src/table.ts | a table row without backticks |\n\n\
            ## Summary\n\nsrc/outside.ts is not in the section\n";
        let patterns = extract_allowed_patterns(spec);
        assert!(patterns.contains(&"src/ticked.ts".to_string()));
        assert!(
            patterns.contains(&"src/bare.ts".to_string()),
            "a bare-declared path must contribute to the allowed set: {patterns:?}"
        );
        assert!(
            patterns.contains(&"src/table.ts".to_string()),
            "a bare table-row path must contribute to the allowed set: {patterns:?}"
        );
        // Outside the section nothing is harvested; prose words never become
        // patterns.
        assert!(!patterns.contains(&"src/outside.ts".to_string()));
        assert!(!patterns.iter().any(|p| p == "with" || p == "declared"));
    }

    // --- gate routing -------------------------------------------------------

    #[test]
    fn non_pre_tool_use_trigger_allows() {
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({ "file_path": "src/main.ts" }),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: String::new(),
            trigger: Some(Trigger::PostToolUse),
            workspace_root: None,
            inject_only: None,
        };
        assert_eq!(
            BoundaryGate.evaluate(&input, &ctx).expect("no error"),
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
