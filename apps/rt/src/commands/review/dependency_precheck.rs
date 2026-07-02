//! `mustard-rt run dependency-precheck` — pre-dispatch factual gate.
//!
//! Reads a wave spec (or single spec), extracts every symbol the spec assumes
//! exists (capitalized JSX tags + named/default imports), then greps the
//! containing subproject for an `export` of each symbol. Symbols whose path
//! the spec itself declares under `## Files` / `## Arquivos` are excluded —
//! they will be created by the upcoming dispatch, not imported.
//!
//! Output is byte-stable JSON on a single line, exit 0 (fail-open). The
//! orchestrator parses the JSON to decide whether to surface a tactical-fix
//! suggestion before paying tokens to discover the gap mid-dispatch.
//!
//! Environment override `MUSTARD_DEPENDENCY_PRECHECK_MODE`:
//!  - `off`  → force `ok: true` regardless of detection
//!  - `warn` → emit the report as-is (orchestrator treats advisory)
//!  - `block` (default) → emit the report as-is (orchestrator may block)
//!
//! This module deliberately uses plain string scanning (no `regex` crate) —
//! consistent with the rest of the `run` face and the workspace's "no new
//! deps" guard.

use crate::commands::spec::spec_sections::is_heading;
use mustard_core::io::fs;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Wave number representation
// ---------------------------------------------------------------------------

/// A major.minor wave identifier that preserves fractional waves.
///
/// `wave-1-core`   → `WaveNumber { major: 1, minor: 0 }`
/// `wave-1_5-core` → `WaveNumber { major: 1, minor: 5 }`
/// `wave-1.5-core` → `WaveNumber { major: 1, minor: 5 }`
///
/// `Ord` is derived so that `(1,0) < (1,5) < (2,0)` holds naturally.
/// Using integer major+minor avoids float `Ord` instability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct WaveNumber {
    pub major: u32,
    pub minor: u32,
}

impl WaveNumber {
    /// Construct from major and minor components.
    pub(crate) const fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }
}

impl fmt::Display for WaveNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.minor == 0 {
            write!(f, "W{}", self.major)
        } else {
            write!(f, "W{}.{}", self.major, self.minor)
        }
    }
}

/// HTML and SVG primitive tag names that must never be flagged as missing.
/// Lowercase JSX tags (`<div>`) are skipped by the extractor already; this
/// list covers the few capitalized tags some libraries accept (none in
/// vanilla React, kept defensive) plus the SVG/HTML names referenced in the
/// spec brief.
const HTML_SVG_WHITELIST: &[&str] = &[
    // HTML
    "div", "span", "section", "header", "footer", "main", "nav", "article",
    "aside", "p", "a", "ul", "ol", "li", "table", "thead", "tbody", "tr",
    "td", "th", "tfoot", "caption", "colgroup", "col", "form", "input",
    "button", "select", "option", "textarea", "label", "fieldset", "legend",
    "img", "pre", "code", "kbd", "mark", "details", "summary", "dialog",
    "figure", "figcaption", "blockquote", "br", "hr", "i", "b", "strong",
    "em", "small", "sub", "sup", "time", "var", "template", "slot", "style",
    "script", "link", "meta", "title", "head", "body", "html",
    // SVG
    "svg", "path", "g", "rect", "circle", "line", "polyline", "polygon",
    "text", "tspan", "defs", "linearGradient", "radialGradient", "stop",
    // React Fragment shorthand handled separately (`<>`)
    "Fragment",
];

/// Section keys whose content is review prose, not dispatch payload. Any
/// JSX-like / import-like noise inside these blocks must be ignored so a
/// quoted symbol in `## Decisions` does not become a false positive.
const REVIEW_SECTION_KEYS: &[&str] = &[
    "concerns",
    "decisions",
];

/// English/Portuguese heading names for sections that are not in
/// [`spec_sections::variants`] but still need to be stripped before parsing.
/// Matched case-insensitively with the same `\b` boundary as `is_heading`.
const EXTRA_STRIP_HEADINGS: &[&str] = &[
    "Notes",
    "Notas",
    "Cobertura",
    "Coverage",
    "Critique Coverage",
    "Lessons",
    "Lições",
    "Lessons Learned",
];

/// One detected dependency reference.
#[derive(Debug, Clone)]
struct Dep {
    symbol: String,
    kind: DepKind,
    line: usize,
    import_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DepKind {
    Jsx,
    Import,
}

impl DepKind {
    fn as_str(self) -> &'static str {
        match self {
            DepKind::Jsx => "jsx",
            DepKind::Import => "import",
        }
    }
}

/// Walk up from `start_dir` to find the project root — a dir containing the
/// Mustard config directory.
///
/// This is a *probe* — each ancestor `dir` is an arbitrary candidate, not a
/// confirmed workspace root, so the canonical `ClaudePaths::for_project`
/// constructor is not appropriate here (it would only catch the I1 guard, not
/// detect the anchor). The literal probe join is therefore retained
/// deliberately: it tests whether a sibling config dir exists relative to an
/// ancestor we have not yet validated.
///
/// Callers that need the canonical workspace root (anchor = `mustard.json` +
/// config dir) should use `mustard_core::io::workspace::workspace_root` instead;
/// this probe predates that resolver and stays in place for back-compat with
/// repos that carry the config dir but no `mustard.json`.
fn find_project_root(start_dir: &Path) -> Option<PathBuf> {
    let mut dir = start_dir.to_path_buf();
    for _ in 0..10 {
        // Probe over an arbitrary candidate dir whose anchor has not yet
        // been validated (see fn-level doc).
        if dir.join(".claude").exists() { // ClaudePaths-exempt: ancestor probe

            return Some(dir);
        }
        match dir.parent() {
            Some(parent) if parent != dir => dir = parent.to_path_buf(),
            _ => break,
        }
    }
    None
}

/// Whether `name` is a known HTML/SVG primitive (case-sensitive — JSX is).
fn is_html_svg_primitive(name: &str) -> bool {
    HTML_SVG_WHITELIST.contains(&name)
}

/// Test whether a line is a `## ` heading whose name matches `target`
/// (case-insensitive, `\b` boundary). Used for [`EXTRA_STRIP_HEADINGS`]
/// entries that aren't in the canonical `is_heading` variants table.
fn is_heading_literal(line: &str, target: &str) -> bool {
    let Some(rest) = line.strip_prefix("##") else {
        return false;
    };
    let after_ws = rest.trim_start_matches([' ', '\t']);
    if after_ws.len() == rest.len() {
        return false;
    }
    let lower = after_ws.to_lowercase();
    let target_lower = target.to_lowercase();
    let Some(tail) = lower.strip_prefix(&target_lower) else {
        return false;
    };
    match tail.chars().next() {
        None => true,
        Some(c) => !(c.is_ascii_alphanumeric() || c == '_'),
    }
}

/// Whether `line` opens a section that should be stripped from the body.
fn opens_review_section(line: &str) -> bool {
    for key in REVIEW_SECTION_KEYS {
        if is_heading(line, key) {
            return true;
        }
    }
    for name in EXTRA_STRIP_HEADINGS {
        if is_heading_literal(line, name) {
            return true;
        }
    }
    false
}

/// Whether `line` opens any `## ` heading (used to close a stripped block).
fn is_any_h2(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("##") else {
        return false;
    };
    rest.starts_with([' ', '\t'])
}

/// Remove review/prose sections (Concerns, Decisions, Notes, Coverage,
/// Lessons, Cobertura, …) so symbols quoted in critique copy do not appear
/// in the dependency scan. Returns the surviving text with line breaks and
/// line indices preserved (stripped lines become empty, not deleted) so
/// later diagnostics (`location: "line N"`) remain accurate.
fn strip_review_sections(text: &str) -> String {
    let mut out = Vec::new();
    let mut stripping = false;
    for line in text.split('\n') {
        if opens_review_section(line) {
            stripping = true;
            out.push("");
            continue;
        }
        if stripping {
            if is_any_h2(line) {
                stripping = false;
                // fall through to push this heading line
            } else {
                out.push("");
                continue;
            }
        }
        out.push(line);
    }
    out.join("\n")
}

/// Parse the `## Files` / `## Arquivos` section, returning each declared path.
fn parse_files_section(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.split('\n').collect();
    let Some(start) = lines.iter().position(|l| is_heading(l, "files")) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in lines.iter().skip(start + 1) {
        let trimmed = line.trim();
        if is_any_h2(trimmed) {
            break;
        }
        if let Some(path) = parse_files_bullet(trimmed) {
            out.push(path);
        }
    }
    out
}

/// Parse one bullet line from a `## Files` section. Mirrors the JS pattern
/// `^-\s+\`?([^\s\`]+)\`?` — the path may be backtick-quoted; tokens after
/// the first whitespace (e.g. `(novo)`) are ignored.
fn parse_files_bullet(trimmed: &str) -> Option<String> {
    let rest = trimmed.strip_prefix('-')?;
    if !rest.starts_with([' ', '\t']) {
        return None;
    }
    let rest = rest.trim_start_matches([' ', '\t']);
    let rest = rest.strip_prefix('`').unwrap_or(rest);
    let token: String = rest
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != '`')
        .collect();
    if token.is_empty() || token.starts_with('#') {
        None
    } else {
        Some(token)
    }
}

/// Derive a probable PascalCase symbol name from a file path.
///
/// Two strategies:
///   1. Basename without extension when it is already PascalCase
///      (`EditorialBand.tsx` → `EditorialBand`).
///   2. Parent directory name when basename is generic
///      (`Foo/index.tsx` → `Foo`).
fn symbol_from_path(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/");
    let segments: Vec<&str> = normalized
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    let last = *segments.last()?;
    let stem: &str = match last.rfind('.') {
        Some(dot) if dot > 0 => &last[..dot],
        _ => last,
    };

    // `index` is the canonical re-export entry — use the parent dir name.
    let generic_stems = ["index", "mod", "main", "lib"];
    let candidate = if generic_stems.iter().any(|g| stem.eq_ignore_ascii_case(g)) {
        segments.iter().rev().nth(1).copied().unwrap_or(stem)
    } else {
        stem
    };
    if is_pascal_case(candidate) {
        Some(candidate.to_string())
    } else {
        None
    }
}

/// Whether `s` starts with an ASCII uppercase letter followed by alphanumerics
/// (the JSX-component naming convention).
fn is_pascal_case(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_uppercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Build the self-created exclusion set from a spec's `## Files` section.
/// Symbols, basename-derived names AND raw paths land in the same set — the
/// caller checks all three when filtering.
fn parse_self_created(text: &str) -> HashSet<String> {
    let mut set = HashSet::new();
    for path in parse_files_section(text) {
        if let Some(sym) = symbol_from_path(&path) {
            set.insert(sym);
        }
        set.insert(path);
    }
    set
}

/// Detect the subproject root for a list of declared file paths.
///
/// Walks each path looking for an `apps/<name>` or `packages/<name>` segment;
/// the first one shared by every path wins. Falls back to `None` when paths
/// disagree or none match the convention — the caller then defaults to the
/// repo root.
///
/// Also consumed by `pipeline::dispatch_plan` to derive a wave's `--subproject`
/// from its `## Files` section (single subproject discovery — no
/// reimplementation).
pub fn detect_subproject(files: &[String], repo_root: &Path) -> Option<PathBuf> {
    let mut chosen: Option<(String, String)> = None;
    for raw in files {
        let normalized = raw.replace('\\', "/");
        let segments: Vec<&str> = normalized
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        let mut found: Option<(String, String)> = None;
        let bases = ["apps", "packages"];
        for (i, seg) in segments.iter().enumerate() {
            if bases.contains(seg) {
                if let Some(name) = segments.get(i + 1) {
                    found = Some(((*seg).to_string(), (*name).to_string()));
                    break;
                }
            }
        }
        match (&chosen, &found) {
            (None, Some(f)) => chosen = Some(f.clone()),
            (Some(c), Some(f)) if c != f => return None,
            _ => {}
        }
    }
    chosen.map(|(base, name)| repo_root.join(base).join(name))
}

/// Extract JSX symbols (capitalized opening tags) from text.
///
/// Recognizes `<Foo`, `<Foo.Bar`, `<Foo />`, `<Foo prop=...>`. Closing tags
/// `</Foo>` are intentionally skipped (the opening tag covers them) to keep
/// duplicates low.
fn extract_jsx(text: &str) -> Vec<Dep> {
    let mut out = Vec::new();
    let mut in_code_fence = false;
    for (idx, line) in text.split('\n').enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_code_fence = !in_code_fence;
            continue;
        }
        // Inside ``` fences we still scan — fixtures and specs commonly put
        // JSX samples in fenced blocks. The fence toggle is kept so we could
        // skip later if the heuristic proves too noisy.
        let _ = in_code_fence;

        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] != b'<' {
                i += 1;
                continue;
            }
            let start = i + 1;
            if start >= bytes.len() {
                break;
            }
            let first = bytes[start];
            if !(first.is_ascii_uppercase()) {
                i += 1;
                continue;
            }
            // Walk an identifier: [A-Za-z0-9]+(\.[A-Za-z0-9]+)?
            let mut end = start;
            while end < bytes.len()
                && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'.')
            {
                end += 1;
            }
            // Must be followed by a tag-closer character so we don't
            // accidentally match `<EmailAddress@example` markdown.
            let next_ok = end >= bytes.len()
                || matches!(bytes[end], b' ' | b'\t' | b'\n' | b'/' | b'>');
            if next_ok && end > start {
                let raw = &line[start..end];
                let root = raw.split('.').next().unwrap_or(raw).to_string();
                if !root.is_empty()
                    && is_pascal_case(&root)
                    && !is_html_svg_primitive(&root)
                {
                    out.push(Dep {
                        symbol: root,
                        kind: DepKind::Jsx,
                        line: idx + 1,
                        import_path: None,
                    });
                }
            }
            i = end.max(i + 1);
        }
    }
    out
}

/// Extract named-import specifiers from text.
///
/// Pattern: `import {...} from "..."` — case-sensitive `import` keyword.
/// Handles `import type {...}`, `as` aliases (the original is captured, the
/// alias is dropped), and multiline import bodies up to the first `}`.
fn extract_imports(text: &str) -> Vec<Dep> {
    let mut out = Vec::new();
    let lines: Vec<&str> = text.split('\n').collect();
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("import") else {
            continue;
        };
        if !rest.starts_with([' ', '\t']) {
            continue;
        }
        let rest = rest.trim_start();
        // Strip `type ` modifier.
        let rest = rest.strip_prefix("type ").map_or(rest, |r| r.trim_start());

        // Named imports: `{...}`.
        if let Some(after_brace) = rest.strip_prefix('{') {
            // Gather body across lines until the closing `}`.
            let mut body = String::new();
            if let Some(end) = after_brace.find('}') {
                body.push_str(&after_brace[..end]);
                // From `}` onwards on this line — find `from "..."`.
                let after = &after_brace[end + 1..];
                let import_path = parse_import_path(after);
                push_named(&mut out, &body, idx + 1, import_path.as_deref());
            } else {
                // Multiline — accumulate following lines until `}`.
                body.push_str(after_brace);
                body.push('\n');
                let mut found_end = false;
                let mut import_path: Option<String> = None;
                for follow in lines.iter().skip(idx + 1) {
                    if let Some(end) = follow.find('}') {
                        body.push_str(&follow[..end]);
                        let after = &follow[end + 1..];
                        import_path = parse_import_path(after);
                        found_end = true;
                        break;
                    }
                    body.push_str(follow);
                    body.push('\n');
                }
                if found_end {
                    push_named(&mut out, &body, idx + 1, import_path.as_deref());
                }
            }
            continue;
        }

        // Default import: `import Foo from "..."` (capitalized only).
        // Take the first identifier-ish token.
        let mut chars = rest.char_indices();
        let mut end = 0;
        for (i, c) in chars.by_ref() {
            if c.is_ascii_alphanumeric() || c == '_' || c == '$' {
                end = i + c.len_utf8();
            } else {
                break;
            }
        }
        if end == 0 {
            continue;
        }
        let ident = &rest[..end];
        if !is_pascal_case(ident) {
            continue;
        }
        // Expect ` from "..."`.
        let after = rest[end..].trim_start();
        if !after.starts_with("from") {
            continue;
        }
        let import_path = parse_import_path(after);
        out.push(Dep {
            symbol: ident.to_string(),
            kind: DepKind::Import,
            line: idx + 1,
            import_path,
        });
    }
    out
}

/// Push every capitalized identifier from a named-imports body
/// (`A, B as C, type D`) into `out`. Aliases drop the original, keep nothing
/// (we want the *exported* symbol, which is the part before `as`).
fn push_named(out: &mut Vec<Dep>, body: &str, line: usize, import_path: Option<&str>) {
    for raw in body.split(',') {
        let mut piece = raw.trim();
        if let Some(after) = piece.strip_prefix("type ") {
            piece = after.trim();
        }
        // `A as B` → keep `A`.
        if let Some((left, _right)) = piece.split_once(" as ") {
            piece = left.trim();
        }
        if piece.is_empty() {
            continue;
        }
        if !is_pascal_case(piece) {
            continue;
        }
        out.push(Dep {
            symbol: piece.to_string(),
            kind: DepKind::Import,
            line,
            import_path: import_path.map(str::to_string),
        });
    }
}

/// Parse `from "..."` / `from '...'` from the tail of an import statement.
fn parse_import_path(after: &str) -> Option<String> {
    let trimmed = after.trim_start();
    let rest = trimmed.strip_prefix("from")?;
    let rest = rest.trim_start();
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let inner = &rest[1..];
    let end = inner.find(quote)?;
    Some(inner[..end].to_string())
}

/// Search `subproject` recursively for an `export` of `symbol`. Looks for
/// `export (function|const|interface|type|class|enum|default function|default class) SYMBOL`
/// inside `.ts`/`.tsx`/`.js`/`.jsx`/`.rs`/`.vue`/`.svelte` files.
fn grep_symbol_in_subproject(symbol: &str, subproject: &Path) -> bool {
    let exts: &[&str] = &["ts", "tsx", "js", "jsx", "rs", "vue", "svelte", "mts", "cts"];
    let skip_dirs: &[&str] = &[
        "target",
        "node_modules",
        "dist",
        ".next",
        "out",
        "build",
        ".turbo",
        ".cache",
    ];
    let needles = [
        format!("export function {symbol}"),
        format!("export async function {symbol}"),
        format!("export const {symbol}"),
        format!("export let {symbol}"),
        format!("export var {symbol}"),
        format!("export class {symbol}"),
        format!("export interface {symbol}"),
        format!("export type {symbol}"),
        format!("export enum {symbol}"),
        format!("export abstract class {symbol}"),
        format!("export default function {symbol}"),
        format!("export default class {symbol}"),
        format!("export {{ {symbol}"),
        format!("export {{{symbol}"),
        format!(", {symbol} }}"),
        format!(", {symbol}}}"),
        format!(", {symbol},"),
        format!(", {symbol} ,"),
        format!("pub fn {symbol}"),
        format!("pub struct {symbol}"),
        format!("pub enum {symbol}"),
        format!("pub trait {symbol}"),
        format!("pub const {symbol}"),
        format!("pub type {symbol}"),
    ];
    let mut stack: Vec<PathBuf> = vec![subproject.to_path_buf()];
    let mut visited = 0u32;
    while let Some(dir) = stack.pop() {
        visited = visited.saturating_add(1);
        // Safety budget — keep walks bounded so a misconfigured cwd cannot
        // hang the gate. 8k directories is comfortably above any sane
        // subproject and well below the cost of dispatch we are trying to
        // avoid.
        if visited > 8000 {
            break;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries {
            if entry.is_dir {
                if skip_dirs.iter().any(|s| *s == entry.file_name) {
                    continue;
                }
                stack.push(entry.path);
                continue;
            }
            let path = entry.path;
            let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            if !exts.contains(&ext) {
                continue;
            }
            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };
            for needle in &needles {
                if content.contains(needle.as_str()) {
                    // Confirm symbol boundary (avoid `FooBar` matching `Foo`).
                    if has_word_boundary_hit(&content, needle, symbol) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Confirm at least one hit of `needle` in `content` ends with a non-word
/// character following `symbol`. Defensive against `export const FooBar`
/// satisfying a search for `Foo`.
fn has_word_boundary_hit(content: &str, needle: &str, symbol: &str) -> bool {
    let mut from = 0usize;
    while let Some(idx) = content[from..].find(needle) {
        let absolute = from + idx;
        let end = absolute + needle.len();
        // The needle already ends with the symbol or a trailing char (`,`,
        // ` `, `}`). When it ends with the symbol, the next byte must not be
        // a word char.
        if needle.ends_with(symbol) {
            let next = content.as_bytes().get(end).copied();
            let is_word = matches!(next, Some(b) if b.is_ascii_alphanumeric() || b == b'_');
            if !is_word {
                return true;
            }
        } else {
            return true;
        }
        from = end;
    }
    false
}

/// Suggest a tactical-fix file path for a missing symbol based on its
/// import path (e.g. `@/components/page` → `<subproject>/src/components/page/Foo/index.tsx`).
/// Returns `None` when no import path is known or the path is not a `@/` alias.
fn suggest_tactical_fix_path(
    subproject: &Path,
    symbol: &str,
    import_path: Option<&str>,
    repo_root: &Path,
) -> Option<String> {
    let path = import_path?;
    let rel = path.strip_prefix("@/")?;
    let abs = subproject.join("src").join(rel).join(symbol).join("index.tsx");
    let rel_to_root = abs
        .strip_prefix(repo_root)
        .map(Path::to_path_buf)
        .unwrap_or(abs);
    Some(rel_to_root.to_string_lossy().replace('\\', "/"))
}

/// Locate the `wave-plan.md` that governs the given spec, if any.
///
/// Wave specs live at `{spec_dir}/wave-N-{role}/spec.md`; the plan sits at
/// `{spec_dir}/wave-plan.md`. The spec file's *grandparent* therefore holds
/// the plan. Returns `None` for single-spec layouts (no wave plan present).
fn find_wave_plan(spec_path: &Path) -> Option<PathBuf> {
    let parent = spec_path.parent()?; // wave-N-role/
    let grand = parent.parent()?;     // {spec_dir}
    let candidate = grand.join("wave-plan.md");
    if candidate.is_file() {
        Some(candidate)
    } else {
        None
    }
}

/// Parse `wave-N[-role]` or `wave-N_M[-role]` from the spec's parent directory name.
fn extract_wave_number_from_spec_path(spec_path: &Path) -> Option<WaveNumber> {
    let parent = spec_path.parent()?;
    let name = parent.file_name()?.to_str()?;
    parse_wave_number_from_token(name)
}

/// Parse `wave-N[-role]`, `wave-N_M[-role]`, `wave-N.M[-role]`, or plain `N` → `WaveNumber`.
///
/// Fractional separator is `_` or `.` (both accepted for robustness).
/// Plain integers (e.g. from wave-plan table cells like `[[1]]`) parse as
/// `WaveNumber { major: N, minor: 0 }`.
fn parse_wave_number_from_token(token: &str) -> Option<WaveNumber> {
    let lower = token.trim().to_lowercase();
    if let Some(rest) = lower.strip_prefix("wave-") {
        return parse_major_minor(rest);
    }
    // Plain numeric token: bare `N` from wave-plan table cells.
    let digits: String = lower.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse::<u32>().ok().map(|n| WaveNumber::new(n, 0))
    }
}

/// Parse `N`, `N-role`, `N_M-role`, or `N.M-role` → `WaveNumber`.
fn parse_major_minor(rest: &str) -> Option<WaveNumber> {
    let major_digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if major_digits.is_empty() {
        return None;
    }
    let major: u32 = major_digits.parse().ok()?;
    let after_major = &rest[major_digits.len()..];
    // Separator is `_` or `.` for fractional; `-` (or end) means minor=0.
    let minor: u32 = if after_major.starts_with('_') || after_major.starts_with('.') {
        let minor_rest = &after_major[1..];
        let minor_digits: String = minor_rest
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if minor_digits.is_empty() {
            0
        } else {
            minor_digits.parse().unwrap_or(0)
        }
    } else {
        0
    };
    Some(WaveNumber::new(major, minor))
}

/// Split a markdown table row `| a | b | c |` into trimmed cells.
fn split_table_row(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let inner = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    inner.split('|').map(|s| s.trim().to_string()).collect()
}

/// Whether a row looks like the `|---|---|` divider line.
fn is_table_divider(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') {
        return false;
    }
    let cells = split_table_row(trimmed);
    !cells.is_empty()
        && cells.iter().all(|c| {
            !c.is_empty()
                && c.chars().all(|ch| ch == '-' || ch == ':' || ch.is_whitespace())
        })
}

/// Whether `header_cell` names the "depends on" column. Accepts EN/PT
/// variants commonly seen in wave plans.
fn is_deps_header(header_cell: &str) -> bool {
    let lower = header_cell.trim().to_lowercase();
    matches!(
        lower.as_str(),
        "depende de"
            | "depends on"
            | "depends"
            | "deps"
            | "dependencies"
            | "dependências"
            | "dependencias"
    )
}

/// Parse the `Depends on` cell of the row whose first cell equals
/// `current_wave`. Recognized tokens:
///   - `[[N]]`, `[[wave-N]]`, `[[wave-N-role]]`
///   - bare `N`, `wave-N`, `wave-N-role`
///
/// Tokens are separated by `,`, `;`, whitespace, or the wikilink wrappers.
/// Returns an empty vec when no waves table is found, the current row is
/// missing, or the deps cell is `—` / `-` / `none`.
fn parse_wave_plan_deps(plan_text: &str, current_wave: WaveNumber) -> Vec<WaveNumber> {
    let lines: Vec<&str> = plan_text.split('\n').collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        // Look for a header row that contains a "wave"-ish column AND a deps
        // column; skip free-form leading pipes that aren't tables.
        if line.trim().starts_with('|') && lines.get(i + 1).is_some_and(|n| is_table_divider(n)) {
            let header_cells = split_table_row(line);
            let deps_col = header_cells.iter().position(|c| is_deps_header(c));
            let wave_col = header_cells
                .iter()
                .position(|c| c.trim().eq_ignore_ascii_case("wave"));
            if let (Some(dcol), Some(wcol)) = (deps_col, wave_col) {
                // Scan data rows until a non-table line.
                let mut j = i + 2;
                while j < lines.len() {
                    let row = lines[j];
                    if !row.trim().starts_with('|') {
                        break;
                    }
                    let cells = split_table_row(row);
                    if let (Some(wcell), Some(dcell)) = (cells.get(wcol), cells.get(dcol)) {
                        if parse_wave_number_from_token(wcell) == Some(current_wave) {
                            return parse_deps_cell(dcell);
                        }
                    }
                    j += 1;
                }
                return Vec::new();
            }
        }
        i += 1;
    }
    Vec::new()
}

/// Tokenize the deps cell into wave numbers.
fn parse_deps_cell(cell: &str) -> Vec<WaveNumber> {
    let trimmed = cell.trim();
    let empty_markers = ["", "—", "-", "–", "none", "nenhuma", "n/a"];
    if empty_markers.iter().any(|m| trimmed.eq_ignore_ascii_case(m)) {
        return Vec::new();
    }
    let mut out: Vec<WaveNumber> = Vec::new();
    let normalized: String = trimmed
        .chars()
        .map(|c| match c {
            '[' | ']' | ',' | ';' | '|' => ' ',
            _ => c,
        })
        .collect();
    for token in normalized.split_whitespace() {
        if let Some(n) = parse_wave_number_from_token(token) {
            if !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out
}

/// Glob `{spec_dir}/wave-{N}-*` siblings, extract each parent spec's
/// `## Files` section, and accumulate a `symbol → "wave-N-role"` map.
fn parent_wave_promises(
    spec_dir: &Path,
    parent_wave_nums: &[WaveNumber],
) -> HashMap<String, String> {
    let mut out: HashMap<String, String> = HashMap::new();
    let Ok(entries) = fs::read_dir(spec_dir) else {
        return out;
    };
    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        let dir_name = entry.file_name.clone();
        let Some(n) = parse_wave_dir_number(&dir_name) else {
            continue;
        };
        if !parent_wave_nums.contains(&n) {
            continue;
        }
        let label = dir_name.clone();
        let spec_md = entry.path.join("spec.md");
        let Ok(text) = fs::read_to_string(&spec_md) else {
            continue;
        };
        let promised = parse_self_created(&text);
        for sym in promised {
            // `parse_self_created` returns symbols AND raw paths; only the
            // PascalCase entries are useful for cross-spec matching.
            if !is_pascal_case(&sym) {
                continue;
            }
            // First-promising wave wins (lower wave number) to mirror the
            // "earliest promise" intuition; equal labels collapse anyway.
            out.entry(sym).or_insert_with(|| label.clone());
        }
    }
    out
}

/// Parse `wave-N-role` or `wave-N_M-role` directory name → `WaveNumber`.
/// Returns `None` for any other directory name.
fn parse_wave_dir_number(dir_name: &str) -> Option<WaveNumber> {
    let lower = dir_name.to_lowercase();
    let rest = lower.strip_prefix("wave-")?;
    parse_major_minor(rest)
}

/// Dispatch `mustard-rt run dependency-precheck`.
pub fn run(spec_arg: Option<&str>, subproject_override: Option<&str>) {
    let Some(spec_arg) = spec_arg else {
        // Single-line JSON for parser stability (orchestrator reads stdout).
        println!(
            "{}",
            json!({
                "missing": [],
                "ok": true,
                "promise_violations": [],
                "spec": null,
                "subproject": null,
                "suggested_tactical_fix_files": [],
                "would_be_created_here": [],
                "error": "no-spec-arg",
            })
        );
        return;
    };
    println!("{}", check(spec_arg, subproject_override));
}

/// Compute the dependency-precheck report for a spec — the core of
/// `dependency-precheck`, returned as the byte-stable JSON Value the CLI face
/// prints. Extracted from [`run`] so `wave-advance` can embed the same verdict
/// per impl wave (one annotation that rides in the round it already returns)
/// instead of the orchestrator paying a separate CLI round-trip per wave.
/// Identical logic and shape — single source of the precheck truth, honouring
/// `MUSTARD_DEPENDENCY_PRECHECK_MODE` (`off` → forced `ok: true`).
pub(crate) fn check(spec_arg: &str, subproject_override: Option<&str>) -> Value {
    let mode = std::env::var("MUSTARD_DEPENDENCY_PRECHECK_MODE")
        .unwrap_or_else(|_| "block".to_string())
        .to_lowercase();

    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let spec_path_raw = if Path::new(spec_arg).is_absolute() {
        PathBuf::from(spec_arg)
    } else {
        cwd.join(spec_arg)
    };
    // Accept either a directory (containing spec.md) or the file itself.
    let spec_path = if spec_path_raw.is_dir() {
        spec_path_raw.join("spec.md")
    } else {
        spec_path_raw
    };
    let spec_dir = spec_path.parent().map_or_else(|| cwd.clone(), Path::to_path_buf);
    let repo_root = find_project_root(&spec_dir).unwrap_or_else(|| cwd.clone());

    let spec_slug = spec_path
        .strip_prefix(&repo_root)
        .map_or_else(|_| spec_arg.to_string(), |p| p.to_string_lossy().replace('\\', "/"));

    let Ok(spec_text) = fs::read_to_string(&spec_path) else {
        return json!({
            "missing": [],
            "ok": true,
            "promise_violations": [],
            "spec": spec_slug,
            "subproject": null,
            "suggested_tactical_fix_files": [],
            "would_be_created_here": [],
            "error": "spec-not-readable",
        });
    };

    let files = parse_files_section(&spec_text);
    if files.is_empty() {
        return json!({
            "missing": [],
            "ok": true,
            "promise_violations": [],
            "spec": spec_slug,
            "subproject": null,
            "suggested_tactical_fix_files": [],
            "would_be_created_here": [],
            "reason": "no-files-section",
        });
    }

    let self_created = parse_self_created(&spec_text);
    let would_be_created_here: BTreeSet<String> = self_created
        .iter()
        .filter(|s| is_pascal_case(s))
        .cloned()
        .collect();

    let stripped = strip_review_sections(&spec_text);

    // Aggregate deps; first-seen line wins for diagnostics, but every kind
    // and import_path observed is preserved so the orchestrator sees the
    // strongest hint.
    let mut deps: BTreeMap<String, Dep> = BTreeMap::new();
    for d in extract_jsx(&stripped).into_iter().chain(extract_imports(&stripped)) {
        deps.entry(d.symbol.clone()).and_modify(|existing| {
            if existing.import_path.is_none() && d.import_path.is_some() {
                existing.import_path.clone_from(&d.import_path);
            }
        }).or_insert(d);
    }

    let subproject_path = subproject_override
        .map(|s| {
            let p = PathBuf::from(s);
            if p.is_absolute() { p } else { repo_root.join(p) }
        })
        .or_else(|| detect_subproject(&files, &repo_root));

    let scan_root = subproject_path.clone().unwrap_or_else(|| repo_root.clone());

    let subproject_field = subproject_path
        .as_ref()
        .and_then(|p| p.strip_prefix(&repo_root).ok().map(Path::to_path_buf))
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .map_or(Value::Null, Value::String);

    // Stack-awareness (loosen on a non-JS/TS subproject): the JSX/import
    // extractor and the `export`/`pub` grep only reason about JS/TS source, so a
    // foreign-language spec (C#, Python, …) yields a false "missing" for every
    // symbol — the walk never reads `.cs`/`.py` and `List<Payable>` lexes as a
    // `<Payable>` JSX tag tagged `jsx`. Decline to judge what this gate cannot
    // parse: emit a clean `ok:true` with an explicit `skipped` marker, honouring
    // the same spirit as `MUSTARD_DEPENDENCY_PRECHECK_MODE=off` for every
    // unsupported stack. The signal is the spec's `## Files` extensions plus the
    // repo model's detected stacks — both fail-open, so an undetected/JS-less
    // target keeps the gate's historical behaviour.
    let model_path = repo_root.join(".claude").join("grain.model.json");
    let target_langs = mustard_core::resolve_target_languages(&files, &model_path, &repo_root);
    if !mustard_core::target_understood(&target_langs) {
        return json!({
            "languages": target_langs.into_iter().collect::<Vec<_>>(),
            "missing": [],
            "mode": mode,
            "ok": true,
            "promise_violations": [],
            "skipped": "stack-unsupported",
            "spec": spec_slug,
            "subproject": subproject_field,
            "suggested_tactical_fix_files": [],
            "would_be_created_here": would_be_created_here.into_iter().collect::<Vec<_>>(),
        });
    }

    let mut missing: Vec<Value> = Vec::new();
    let mut suggested: BTreeSet<String> = BTreeSet::new();
    let mut grep_cache: BTreeMap<String, bool> = BTreeMap::new();
    for (symbol, dep) in &deps {
        if self_created.contains(symbol) {
            continue;
        }
        let found = if let Some(cached) = grep_cache.get(symbol) {
            *cached
        } else {
            let f = grep_symbol_in_subproject(symbol, &scan_root);
            grep_cache.insert(symbol.clone(), f);
            f
        };
        if found {
            continue;
        }
        let mut entry = Map::new();
        entry.insert("symbol".to_string(), Value::String(symbol.clone()));
        entry.insert("kind".to_string(), Value::String(dep.kind.as_str().to_string()));
        entry.insert("location".to_string(), Value::String(format!("line {}", dep.line)));
        if let Some(p) = &dep.import_path {
            entry.insert("import_path".to_string(), Value::String(p.clone()));
        }
        missing.push(Value::Object(entry));

        if let Some(sub) = subproject_path.as_ref() {
            if let Some(suggestion) =
                suggest_tactical_fix_path(sub, symbol, dep.import_path.as_deref(), &repo_root)
            {
                suggested.insert(suggestion);
            }
        }
    }

    let effective_ok = if mode == "off" { true } else { missing.is_empty() };

    // Cross-spec promise check: classify every missing symbol against the
    // governing wave plan (if any). Layout: `{spec_dir}/wave-plan.md` plus
    // sibling `wave-N-role/spec.md` directories.
    let promise_violations: Vec<Value> = if let Some(plan_path) = find_wave_plan(&spec_path) {
        let plan_text = fs::read_to_string(&plan_path).unwrap_or_default();
        let current_wave = extract_wave_number_from_spec_path(&spec_path);
        let parent_waves = match current_wave {
            Some(n) => parse_wave_plan_deps(&plan_text, n),
            None => Vec::new(),
        };
        let plan_dir = plan_path
            .parent()
            .map_or_else(|| spec_dir.clone(), Path::to_path_buf);
        let promise_map = parent_wave_promises(&plan_dir, &parent_waves);
        let mut out: Vec<Value> = Vec::new();
        for entry in &missing {
            let symbol = entry.get("symbol").and_then(Value::as_str).unwrap_or("");
            if symbol.is_empty() {
                continue;
            }
            let mut obj = Map::new();
            obj.insert("symbol".to_string(), Value::String(symbol.to_string()));
            match promise_map.get(symbol) {
                Some(label) => {
                    obj.insert(
                        "parent_promised".to_string(),
                        Value::String(label.clone()),
                    );
                    obj.insert("actually_delivered".to_string(), Value::Bool(false));
                }
                None => {
                    obj.insert("no_parent_promised_this".to_string(), Value::Bool(true));
                }
            }
            out.push(Value::Object(obj));
        }
        out
    } else {
        Vec::new()
    };

    json!({
        "missing": missing,
        "mode": mode,
        "ok": effective_ok,
        "promise_violations": promise_violations,
        "spec": spec_slug,
        "subproject": subproject_field,
        "suggested_tactical_fix_files": suggested.into_iter().collect::<Vec<_>>(),
        "would_be_created_here": would_be_created_here.into_iter().collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitelist_html_primitives_skipped() {
        let deps = extract_jsx("<div>\n<table>\n<svg>\n<EditorialBand>\n");
        let symbols: Vec<_> = deps.iter().map(|d| d.symbol.as_str()).collect();
        assert!(!symbols.contains(&"div"));
        assert!(!symbols.contains(&"table"));
        assert!(!symbols.contains(&"svg"));
        assert!(symbols.contains(&"EditorialBand"));
    }

    #[test]
    fn jsx_capitalized_extracted() {
        let deps = extract_jsx("<EditorialBand>\n<Foo.Bar />\n<KpiValue value={1} />\n");
        let symbols: Vec<_> = deps.iter().map(|d| d.symbol.as_str()).collect();
        assert!(symbols.contains(&"EditorialBand"));
        assert!(symbols.contains(&"Foo"));
        assert!(symbols.contains(&"KpiValue"));
    }

    #[test]
    fn imports_parsed() {
        let deps = extract_imports("import { A, B as C, type D } from \"./x\";\n");
        let symbols: BTreeSet<_> = deps.iter().map(|d| d.symbol.as_str()).collect();
        assert!(symbols.contains("A"));
        assert!(symbols.contains("B"));
        assert!(symbols.contains("D"));
        // `C` is the local alias — not the exported symbol.
        assert!(!symbols.contains("C"));
    }

    #[test]
    fn default_import_capitalized_only() {
        let deps = extract_imports(
            "import Foo from \"./foo\";\nimport bar from \"./bar\";\n",
        );
        let symbols: BTreeSet<_> = deps.iter().map(|d| d.symbol.as_str()).collect();
        assert!(symbols.contains("Foo"));
        assert!(!symbols.contains("bar"));
    }

    #[test]
    fn self_created_excluded() {
        let spec = "# Spec\n\n## Files\n- apps/dashboard/src/components/LocalPrimitiveA.tsx\n- apps/dashboard/src/Foo/index.tsx\n";
        let set = parse_self_created(spec);
        assert!(set.contains("LocalPrimitiveA"));
        assert!(set.contains("Foo"));
    }

    #[test]
    fn review_sections_stripped() {
        let spec = "# Spec\n\n## Concerns\n<FalsePositive>\nimport { Bogus } from \"./x\";\n\n## Tasks\n<RealOne>\n";
        let stripped = strip_review_sections(spec);
        let deps = extract_jsx(&stripped);
        let symbols: Vec<_> = deps.iter().map(|d| d.symbol.as_str()).collect();
        assert!(!symbols.contains(&"FalsePositive"));
        assert!(symbols.contains(&"RealOne"));
        let imports = extract_imports(&stripped);
        let import_syms: Vec<_> = imports.iter().map(|d| d.symbol.as_str()).collect();
        assert!(!import_syms.contains(&"Bogus"));
    }

    #[test]
    fn cobertura_section_stripped() {
        let spec = "# Spec\n\n## Cobertura\n<FalsePositive>\n\n## Tasks\n<RealOne>\n";
        let stripped = strip_review_sections(spec);
        let deps = extract_jsx(&stripped);
        let symbols: Vec<_> = deps.iter().map(|d| d.symbol.as_str()).collect();
        assert!(!symbols.contains(&"FalsePositive"));
        assert!(symbols.contains(&"RealOne"));
    }

    #[test]
    fn subproject_detection_apps() {
        let files = vec![
            "apps/dashboard/src/pages/X.tsx".to_string(),
            "apps/dashboard/src/Y.tsx".to_string(),
        ];
        let root = Path::new("/repo");
        let sub = detect_subproject(&files, root).unwrap();
        assert_eq!(sub, PathBuf::from("/repo/apps/dashboard"));
    }

    #[test]
    fn subproject_detection_disagreement_returns_none() {
        let files = vec![
            "apps/dashboard/src/X.tsx".to_string(),
            "apps/cli/src/Y.rs".to_string(),
        ];
        let root = Path::new("/repo");
        assert!(detect_subproject(&files, root).is_none());
    }

    #[test]
    fn symbol_from_path_basename_and_index() {
        assert_eq!(
            symbol_from_path("apps/dashboard/src/EditorialBand.tsx").as_deref(),
            Some("EditorialBand")
        );
        assert_eq!(
            symbol_from_path("apps/dashboard/src/components/Foo/index.tsx").as_deref(),
            Some("Foo")
        );
        // Lowercase basename → no PascalCase symbol.
        assert_eq!(symbol_from_path("apps/dashboard/src/foo.tsx"), None);
    }

    #[test]
    fn parse_import_path_double_and_single_quotes() {
        assert_eq!(
            parse_import_path("from \"@/components/page\";").as_deref(),
            Some("@/components/page"),
        );
        assert_eq!(
            parse_import_path("from '@/components/page';").as_deref(),
            Some("@/components/page"),
        );
    }

    #[test]
    fn suggest_tactical_fix_resolves_alias() {
        let sub = PathBuf::from("/repo/apps/dashboard");
        let root = Path::new("/repo");
        let out = suggest_tactical_fix_path(
            &sub,
            "EditorialBand",
            Some("@/components/page"),
            root,
        )
        .unwrap();
        assert_eq!(
            out,
            "apps/dashboard/src/components/page/EditorialBand/index.tsx"
        );
    }

    #[test]
    fn word_boundary_avoids_false_positive() {
        // `export const FooBar` must not satisfy a search for `Foo`.
        let content = "export const FooBar = 1;\n";
        let needle = "export const Foo";
        assert!(!has_word_boundary_hit(content, needle, "Foo"));
        let content_real = "export const Foo = 1;\n";
        assert!(has_word_boundary_hit(content_real, needle, "Foo"));
    }

    #[test]
    fn parse_wave_plan_deps_extracts_wikilinks() {
        let plan = "\
# Wave Plan

| Wave | Role | Depende de | Summary |
|------|------|------------|---------|
| 1 | general | — | foundation |
| 2 | ui | [[1]] | primitives |
| 3 | ui | [[wave-2-ui]], [[1]] | pages |
";
        let w1 = WaveNumber::new(1, 0);
        let w2 = WaveNumber::new(2, 0);
        let w3 = WaveNumber::new(3, 0);
        assert_eq!(parse_wave_plan_deps(plan, w1), Vec::<WaveNumber>::new());
        assert_eq!(parse_wave_plan_deps(plan, w2), vec![w1]);
        assert_eq!(parse_wave_plan_deps(plan, w3), vec![w2, w1]);
    }

    #[test]
    fn parse_wave_plan_deps_handles_bare_numbers_and_depends_on() {
        let plan = "\
| Wave | Role | Depends on | Summary |
|------|------|------------|---------|
| 2 | ui | 1 | x |
| 3 | ui | wave-2, 1 | y |
";
        let w1 = WaveNumber::new(1, 0);
        let w2 = WaveNumber::new(2, 0);
        let w3 = WaveNumber::new(3, 0);
        assert_eq!(parse_wave_plan_deps(plan, w2), vec![w1]);
        assert_eq!(parse_wave_plan_deps(plan, w3), vec![w2, w1]);
    }

    #[test]
    fn parse_wave_number_from_token_variants() {
        // Brackets are stripped at the `parse_deps_cell` layer; this helper
        // sees only the bare token.
        assert_eq!(
            parse_wave_number_from_token("wave-3-ui"),
            Some(WaveNumber::new(3, 0))
        );
        assert_eq!(
            parse_wave_number_from_token("wave-7"),
            Some(WaveNumber::new(7, 0))
        );
        assert_eq!(
            parse_wave_number_from_token("5"),
            Some(WaveNumber::new(5, 0))
        );
        assert_eq!(parse_wave_number_from_token("—"), None);
        assert_eq!(parse_wave_number_from_token("foo"), None);
        // parse_deps_cell strips brackets first, then calls this helper.
        assert_eq!(parse_deps_cell("[[2]]"), vec![WaveNumber::new(2, 0)]);
        assert_eq!(
            parse_deps_cell("[[wave-2-ui]], [[1]]"),
            vec![WaveNumber::new(2, 0), WaveNumber::new(1, 0)]
        );
        assert_eq!(parse_deps_cell("—"), Vec::<WaveNumber>::new());
    }

    #[test]
    fn parent_wave_promises_collects_symbols() {
        let tmp = std::env::temp_dir().join(format!(
            "mustard-rt-precheck-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("wave-1-general")).unwrap();
        std::fs::create_dir_all(tmp.join("wave-2-ui")).unwrap();
        std::fs::write(
            tmp.join("wave-1-general/spec.md"),
            "# W1\n\n## Files\n- apps/rt/src/Foundation.rs\n",
        )
        .unwrap();
        std::fs::write(
            tmp.join("wave-2-ui/spec.md"),
            "# W2\n\n## Files\n- apps/dashboard/src/components/page/PromisedButMissing/index.tsx\n",
        )
        .unwrap();

        let w1 = WaveNumber::new(1, 0);
        let w2 = WaveNumber::new(2, 0);
        let map = parent_wave_promises(&tmp, &[w1, w2]);
        assert_eq!(map.get("Foundation").map(String::as_str), Some("wave-1-general"));
        assert_eq!(
            map.get("PromisedButMissing").map(String::as_str),
            Some("wave-2-ui"),
        );

        // Restrict to just wave 2 → wave-1 symbols absent.
        let map2 = parent_wave_promises(&tmp, &[w2]);
        assert!(!map2.contains_key("Foundation"));
        assert!(map2.contains_key("PromisedButMissing"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn no_wave_plan_returns_empty() {
        let tmp = std::env::temp_dir().join(format!(
            "mustard-rt-precheck-noplan-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let spec_path = tmp.join("solo.md");
        std::fs::write(&spec_path, "# Solo\n\n## Files\n- apps/x.rs\n").unwrap();
        assert!(find_wave_plan(&spec_path).is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // -----------------------------------------------------------------------
    // WaveNumber parser tests (FIX: fractional wave collision)
    // -----------------------------------------------------------------------

    #[test]
    fn parses_simple_wave_dir() {
        assert_eq!(
            parse_wave_dir_number("wave-1-core"),
            Some(WaveNumber::new(1, 0))
        );
    }

    #[test]
    fn parses_underscore_fraction() {
        assert_eq!(
            parse_wave_dir_number("wave-1_5-core"),
            Some(WaveNumber::new(1, 5))
        );
    }

    #[test]
    fn parses_dot_fraction() {
        assert_eq!(
            parse_wave_dir_number("wave-1.5-core"),
            Some(WaveNumber::new(1, 5))
        );
    }

    #[test]
    fn parses_two_digit_major() {
        assert_eq!(
            parse_wave_dir_number("wave-10-core"),
            Some(WaveNumber::new(10, 0))
        );
    }

    #[test]
    fn parses_two_digit_minor() {
        assert_eq!(
            parse_wave_dir_number("wave-10_25-core"),
            Some(WaveNumber::new(10, 25))
        );
    }

    #[test]
    fn no_collision_w1_vs_w1_5() {
        // The core fix: wave-1_5-core must NOT equal wave-1-core.
        assert_ne!(
            parse_wave_dir_number("wave-1-core"),
            parse_wave_dir_number("wave-1_5-core"),
        );
    }

    #[test]
    fn ord_natural() {
        let w1 = WaveNumber::new(1, 0);
        let w1_5 = WaveNumber::new(1, 5);
        let w2 = WaveNumber::new(2, 0);
        let mut nums = vec![w2, w1_5, w1];
        nums.sort();
        assert_eq!(nums, vec![w1, w1_5, w2]);
    }

    #[test]
    fn rejects_non_wave_prefix() {
        assert_eq!(parse_wave_dir_number("something-1-core"), None);
    }

    #[test]
    fn rejects_no_digits() {
        assert_eq!(parse_wave_dir_number("wave-abc-core"), None);
    }

    #[test]
    fn promise_violations_classifies_correctly() {
        // End-to-end on the on-disk fixture: wave-3-ui references one
        // symbol promised by wave-2 (but the file doesn't exist) and two
        // symbols nobody promised.
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let spec_path = Path::new(manifest_dir)
            .join("tests/fixtures/dependency_precheck/promise_violation/wave-3-ui/spec.md");
        assert!(spec_path.is_file(), "fixture missing: {}", spec_path.display());

        let plan = find_wave_plan(&spec_path).expect("wave-plan.md not found");
        let plan_text = std::fs::read_to_string(&plan).unwrap();
        let current = extract_wave_number_from_spec_path(&spec_path).unwrap();
        assert_eq!(current, WaveNumber::new(3, 0));
        let parents = parse_wave_plan_deps(&plan_text, current);
        assert_eq!(parents, vec![WaveNumber::new(2, 0)]);

        let plan_dir = plan.parent().unwrap();
        let promises = parent_wave_promises(plan_dir, &parents);
        assert_eq!(
            promises.get("PromisedButMissing").map(String::as_str),
            Some("wave-2-ui"),
        );
        assert!(!promises.contains_key("NeverPromisedA"));
        assert!(!promises.contains_key("NeverPromisedB"));
    }

    #[test]
    fn csharp_spec_skips_with_stack_unsupported() {
        let tmp = std::env::temp_dir().join(format!(
            "mustard-rt-precheck-cs-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join(".claude")).unwrap();
        // A C# spec that names its entity as a generic — the JSX extractor would
        // mis-read `List<Payable>` as a `<Payable>` tag and report it "missing"
        // (the grep never reads `.cs`). The gate must decline, cleanly.
        let spec = "# Payable\n\nUse List<Payable> and IRepository<Payable>.\n\n\
            ## Files\n- backend/App/DTOs/Payable.cs\n- backend/App/Services/Recurrence.cs\n";
        std::fs::write(tmp.join("spec.md"), spec).unwrap();

        let verdict = check(&tmp.join("spec.md").to_string_lossy(), None);
        assert_eq!(verdict["ok"], json!(true), "unsupported stack is clean: {verdict}");
        assert_eq!(verdict["skipped"], json!("stack-unsupported"), "{verdict}");
        assert_eq!(verdict["missing"], json!([]), "no false missing: {verdict}");
        assert_eq!(verdict["languages"], json!(["csharp"]), "{verdict}");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn ts_spec_still_runs_precheck() {
        let tmp = std::env::temp_dir().join(format!(
            "mustard-rt-precheck-ts-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join(".claude")).unwrap();
        // A TS spec importing a symbol nobody creates → the gate still runs and
        // reports it missing (a JS/TS target is understood, never skipped).
        let spec = "# Feature\n\nimport { MissingThing } from \"@/components/x\";\n\n\
            ## Files\n- apps/web/src/pages/New.tsx\n";
        std::fs::write(tmp.join("spec.md"), spec).unwrap();

        let verdict = check(&tmp.join("spec.md").to_string_lossy(), None);
        assert!(verdict.get("skipped").is_none(), "TS spec is not skipped: {verdict}");
        let missing = verdict["missing"].as_array().expect("missing array");
        assert!(
            missing.iter().any(|m| m["symbol"] == json!("MissingThing")),
            "TS import of an uncreated symbol is still flagged: {verdict}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
