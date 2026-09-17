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
//! `Read`/`Edit`/`Write` globs (first line) + the write
//! gate's secret rule, which keeps the old case-insensitive full-path
//! substring semantics the globs cannot express.
//!
//! The path helper [`relative_to_cwd`] lives in `shared::paths`, with the
//! write gate's path classifier. The `file_path` extraction this module
//! used to host now lives on
//! [`HookInput::file_path`](mustard_core::domain::model::contract::HookInput::file_path).
//!
//! ## Migration off SQLite
//!
//! `boundary_gate` previously used the SQLite event store to feed
//! `pipeline_state_from_events`. The migration off SQLite replaces that with two
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
use crate::util::glob::glob_match;
use crate::shared::paths::relative_to_cwd;
use std::path::Path;


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

/// Resolve the spec a Write/Edit should be checked against, fail-open `None`:
/// the one current-spec ladder every door shares (the environment override,
/// then the checkout's branch, then this session's binding). A leftover
/// `.pipeline-states/` file names nothing.
pub(crate) fn resolve_boundary_spec(cwd: &str, session_id: Option<&str>) -> Option<String> {
    crate::shared::spec_state::active_spec(cwd, session_id)
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
fn resolve_boundary_files(cwd: &str, spec_name: &str) -> Vec<std::path::PathBuf> {
    let Ok(base) = ClaudePaths::for_project(Path::new(cwd))
        .and_then(|p| p.for_spec(spec_name))
        .map(|sp| sp.dir().to_path_buf())
    else {
        return Vec::new();
    };
    if !base.exists() {
        return Vec::new();
    }
    let root = base.join("spec.md");
    if root.exists() { vec![root] } else { Vec::new() }
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
            if let Some(candidate) = bare_path_pattern(token)
                && candidate.len() <= 200 {
                    push_unique(&mut patterns, &candidate);
                }
        }
    }
    patterns
}

/// A bare (un-backticked) token that reads as ONE concrete file path, trimmed
/// of surrounding punctuation (list dashes, table pipes, commas, stray
/// backticks) and forward-slash normalised — `None` for prose.
///
/// O julgamento é do reconhecedor estrito [`looks_like_file_path`], logo
/// acima: uma definição só de "lê como caminho". Só o aparo de bordas que ele
/// aplica por dentro é espelhado aqui, para que o padrão guardado seja igual
/// ao pedaço de texto que ele julgou.
/// As extensões que fazem um pedaço de texto ler como arquivo. É a metade que
/// mantém prosa de fora: "3.5", "e.g." e "https://example.com" não passam,
/// enquanto `src/list.rs` e `Cargo.toml` passam.
const KNOWN_FILE_EXTS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "vue", "svelte", "py", "go", "cs",
    "java", "kt", "swift", "dart", "rb", "php", "c", "h", "cpp", "hpp", "scala",
    "ex", "exs", "html", "css", "scss", "sass", "less", "json", "jsonc", "toml",
    "yaml", "yml", "xml", "ini", "env", "lock", "sql", "prisma", "graphql", "proto",
    "md", "mdx", "txt", "sh", "bash", "ps1", "bat",
];

/// Os caracteres que podem aparecer num pedaço de texto que se lê como caminho.
fn is_path_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(c, '.' | '/' | '-' | '_' | '(' | ')' | '[' | ']' | '{' | '}' | '*')
}

/// A extensão é conhecida.
fn is_known_file_ext(ext: &str) -> bool {
    KNOWN_FILE_EXTS.contains(&ext.to_ascii_lowercase().as_str())
}

/// O texto tem a forma de UM arquivo, não de um conjunto: glob, `{molde}` e
/// elisão de documentação (`.../spec.md`) ficam de fora, porque o consumidor
/// compara letra por letra com os caminhos declarados.
fn is_file_reference(token: &str) -> bool {
    if token.is_empty() || !token.chars().all(is_path_token_char) {
        return false;
    }
    if token.contains(['*', '{', '}']) {
        return false;
    }
    if token.contains("//")
        || token.split('/').any(|seg| seg.len() >= 3 && seg.chars().all(|c| c == '.'))
    {
        return false;
    }
    let name = token.rsplit('/').next().unwrap_or(token);
    let Some((stem, ext)) = name.rsplit_once('.') else {
        return false;
    };
    if stem.is_empty()
        || ext.is_empty()
        || !ext.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return false;
    }
    is_known_file_ext(ext) || token.contains('/')
}

/// O reconhecedor estrito: o texto lê como caminho de arquivo. Veio da
/// validação de spec quando ela saiu — aqui está o único chamador que sobrou.
fn looks_like_file_path(token: &str) -> bool {
    let token = token.trim_matches(|c: char| {
        !c.is_ascii_alphanumeric() && c != '.' && c != '/' && c != '\\' && c != '-' && c != '_'
    });
    let token = token.replace('\\', "/");
    if !is_file_reference(&token) {
        return false;
    }
    let ext = token.rsplit('.').next().unwrap_or("");
    is_known_file_ext(ext)
}

fn bare_path_pattern(token: &str) -> Option<String> {
    if !looks_like_file_path(token) {
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

/// Collect harness events for `spec_name` from the per-spec NDJSON event log.
///
/// Replaces the broad `store.replay()` (all events from SQLite) with a
/// targeted read of the per-spec `.events/` directory. Fail-open: an absent
/// or unreadable directory returns an empty vec.
fn read_spec_events(cwd: &str, spec_name: &str) -> Vec<HarnessEvent> {
    // O fluxo de eventos do gravador velho saiu: não há o que ler.
    let _ = (cwd, spec_name);
    Vec::new()
}

/// The boundary check: flag a Write/Edit outside the active spec's declared
/// `## Files` / `## Boundaries`.
///
/// 1:1 with `boundary-gate.js` — every early `process.exit(0)` maps to
/// `None` (pass through). A real mismatch → `Deny` in strict mode, `Warn` in
/// warn mode.
///
/// Since the migration off SQLite, spec pipeline-state fields (`isWavePlan`, `currentWave`,
/// `status`) are derived from the NDJSON event log via
/// `pipeline_state_from_events`. Whether the spec is settled comes from its
/// state in `spec.ndjson`, through the lock's one rule. Fail-open: projection
/// `None` → wave info unknown.
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
    // Resolve the spec THIS edit belongs to, by the one current-spec ladder
    // (see `resolve_boundary_spec`).
    let spec_name = resolve_boundary_spec(cwd, input.session_id.as_deref())?;
    let spec_name = spec_name.as_str();

    // Collect events from the NDJSON event log (no SQLite).
    let _events = read_spec_events(cwd, spec_name);

    // Derive the spec's pipeline state from the NDJSON event log.
    // Fail-open: absent events dir or no events → projection is None.
    // Skip when the spec is settled: its state, read by the lock's one rule,
    // is closed, has its pull request open, was delivered or was discarded.
    let settled = crate::shared::spec_state::lock_state(Path::new(cwd), spec_name)
        .and_then(|state| state.phase)
        .is_some_and(|phase| matches!(phase, "closed" | "pr_open" | "delivered" | "discarded"));
    if settled {
        return None;
    }
    // The boundary is a LIST because a dispatch round can have several waves
    // writing at once — see `resolve_boundary_files`. The declared patterns are
    // their union: a path any wave of the round declared is inside the boundary.
    let spec_files = resolve_boundary_files(cwd, spec_name);
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
        let ctx = Ctx::for_test(String::new(), Some(Trigger::PreToolUse));
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
        // No current spec → pass through.
        let dir = tempdir().unwrap();
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({ "file_path": "src/main.ts" }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(dir.path().to_string_lossy().into_owned()),
            ..HookInput::default()
        };
        let ctx = Ctx::for_test(dir.path().to_string_lossy().into_owned(), Some(Trigger::PreToolUse));
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
        // The checkout stands on spec "demo"'s branch. Phase derives from
        // NDJSON `pipeline.phase` events; none is seeded here → phase is
        // empty → not CLOSE → gate runs.
        crate::shared::spec_state::stand_on_spec_branch(cwd, "demo");
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

    // --- NDJSON event source ------------------------------------------------

    #[test]
    fn boundary_gate_allows_when_no_events_and_no_patterns() {
        // No events at all → projection None → gate falls through (no spec
        // file patterns found because the spec dir doesn't exist).
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        let cwd_str = cwd.to_string_lossy().into_owned();
        crate::shared::context::bind_session_spec(&cwd_str, "s-ghost", "ghost");
        let input = HookInput {
            tool_name: Some("Edit".to_string()),
            tool_input: serde_json::json!({ "file_path": "src/any.ts" }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(cwd_str.clone()),
            session_id: Some("s-ghost".to_string()),
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
    fn boundary_gate_skips_a_settled_spec() {
        // Whether the spec is settled comes from its state in `spec.ndjson`: a
        // spec in execution is checked, a closed one skips the gate.
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        let paths = ClaudePaths::for_project(cwd).unwrap();
        crate::shared::spec_state::stand_on_spec_branch(cwd, "myspec");
        let sp = paths.for_spec("myspec").unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        std::fs::write(sp.spec_md_path(), "# Spec\n\n## Files\n\n- `src/allowed.ts`\n").unwrap();
        let record = |phase: &str| {
            let state = serde_json::json!({ "phase": phase });
            mustard_core::io::spec_events::write(
                &sp.dir().join("spec.ndjson"),
                "state",
                state.as_object().cloned().unwrap(),
                &[],
            )
            .unwrap();
        };
        record("running");

        let cwd_str = cwd.to_string_lossy().into_owned();
        let input = HookInput {
            tool_name: Some("Edit".to_string()),
            tool_input: serde_json::json!({ "file_path": "src/forbidden.ts" }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(cwd_str.clone()),
            ..HookInput::default()
        };
        assert!(boundary_gate(&input, &cwd_str).is_some(), "a spec in execution is checked");
        record("closed");
        // Closed → skip (None), even though src/forbidden.ts is outside the boundary.
        assert!(boundary_gate(&input, &cwd_str).is_none(), "a closed spec must skip the boundary gate");
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
        let ctx = Ctx::for_test(String::new(), Some(Trigger::PostToolUse));
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
