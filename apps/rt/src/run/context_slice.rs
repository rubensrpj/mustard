//! `mustard-rt run context-slice` — a port of `scripts/context-slice.js`.
//!
//! Cuts the relevant term blocks from a `CONTEXT.md` glossary. Dumping the
//! whole glossary into every agent prompt is prompt bloat; this returns only
//! the term blocks whose term or definition matches an entity, file name, or
//! significant token of the active spec.
//!
//! Fail-graceful: a missing spec or context file yields an empty slice, never
//! an error. The relevance heuristic (entity/file extraction + frequency-
//! derived significant tokens) and the line cap mirror the JS version exactly.

use mustard_core::fs;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Default line cap for the emitted slice — mirrors `DEFAULT_MAX_LINES`.
const DEFAULT_MAX_LINES: usize = 250;

/// Truncation marker appended when the slice exceeds the cap.
const TRUNCATE_TAIL: &str = "\n...[truncated — glossary slice exceeded cap]";

/// A token shorter than this is almost always a common word — no signal.
const MIN_TOKEN_LEN: usize = 4;

/// A token appearing in more than this fraction of body tokens carries no
/// signal (it shows up everywhere).
const MAX_TOKEN_FREQUENCY: f64 = 0.04;

/// One parsed term block: its heading/definition term and its full text.
#[derive(Debug, Clone)]
struct TermBlock {
    term: String,
    text: String,
}

/// Result of slicing — mirrors the JS `sliceContext` return object.
#[derive(Debug)]
pub struct SliceResult {
    pub slice: String,
    /// Line count of the emitted slice. Part of the JS return shape; kept for
    /// parity even though the CLI path prints only `slice`.
    #[allow(dead_code)]
    pub line_count: usize,
    pub truncated: bool,
    pub block_count: usize,
}

/// Read a file, returning `None` on any error (fail-graceful).
fn read_file_safe(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok()
}

/// The line cap: `MUSTARD_GLOSSARY_MAX_LINES` when a positive integer, else
/// [`DEFAULT_MAX_LINES`].
fn resolve_max_lines() -> usize {
    std::env::var("MUSTARD_GLOSSARY_MAX_LINES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_MAX_LINES)
}

/// `true` if a line is a `## Heading` / `### Heading` (depth 2-3).
fn heading_term(line: &str) -> Option<String> {
    let trimmed = line.trim_end();
    let hashes = trimmed.chars().take_while(|&c| c == '#').count();
    if (2..=3).contains(&hashes) {
        let rest = trimmed[hashes..].trim_start();
        if rest.starts_with(char::is_whitespace) || !rest.is_empty() {
            let term = trimmed[hashes..].trim();
            if !term.is_empty() {
                // Must have had whitespace after the hashes.
                if trimmed
                    .as_bytes()
                    .get(hashes)
                    .is_some_and(u8::is_ascii_whitespace)
                {
                    return Some(term.to_string());
                }
            }
        }
    }
    None
}

/// `true` if a line is a definition line `[-*]? **Term** ...`; returns the term.
fn def_term(line: &str) -> Option<String> {
    let mut s = line.trim_start();
    // Optional `-` / `*` bullet followed by whitespace.
    if let Some(first) = s.chars().next() {
        if first == '-' || first == '*' {
            let rest = &s[1..];
            if rest.starts_with(char::is_whitespace) {
                s = rest.trim_start();
            }
        }
    }
    let after_open = s.strip_prefix("**")?;
    let end = after_open.find("**")?;
    let term = after_open[..end].trim();
    if term.is_empty() {
        None
    } else {
        Some(term.to_string())
    }
}

/// Extract a `## Heading` section body (until the next depth-2/3 heading).
/// `names` is the accepted heading texts (case-insensitive).
fn extract_section(text: &str, names: &[&str]) -> String {
    let lines: Vec<&str> = text.lines().collect();
    for name in names {
        let target = name.to_lowercase();
        for (i, line) in lines.iter().enumerate() {
            if let Some(term) = heading_term(line) {
                if term.to_lowercase() == target {
                    // Collect until the next depth-2/3 heading.
                    let mut out = vec![*line];
                    for next in &lines[i + 1..] {
                        if heading_term(next).is_some() {
                            break;
                        }
                        out.push(next);
                    }
                    return out.join("\n");
                }
            }
        }
    }
    String::new()
}

/// The set of relevance terms (lowercase) derived from a spec.
fn extract_relevance_terms(spec_text: &str) -> BTreeSet<String> {
    let mut terms: BTreeSet<String> = BTreeSet::new();
    if spec_text.is_empty() {
        return terms;
    }
    let mut add = |w: &str| {
        let t = w.to_lowercase();
        let t = t.trim();
        if t.chars().count() >= 2 {
            terms.insert(t.to_string());
        }
    };

    // 1. Identifier-like tokens in `## Entidades` / `## Entities`.
    let entity_section = extract_section(spec_text, &["Entidades", "Entities"]);
    for tok in identifier_tokens(&entity_section) {
        add(&tok);
    }

    // 2. File basenames + stems in `## Arquivos` / `## Files`.
    let file_section = extract_section(spec_text, &["Arquivos", "Files"]);
    for tok in path_tokens(&file_section) {
        let base = tok.replace('\\', "/");
        let base = base.rsplit('/').next().unwrap_or(&base);
        add(base);
        if let Some(stem) = base.rsplit_once('.') {
            add(stem.0);
        }
    }

    // 3. Frequency-derived significant body tokens.
    let body_tokens = body_tokens(spec_text);
    if !body_tokens.is_empty() {
        let mut freq: BTreeMap<&str, usize> = BTreeMap::new();
        for tok in &body_tokens {
            *freq.entry(tok.as_str()).or_insert(0) += 1;
        }
        let total = body_tokens.len() as f64;
        for (tok, count) in &freq {
            if tok.chars().count() < MIN_TOKEN_LEN {
                continue;
            }
            if (*count as f64) / total > MAX_TOKEN_FREQUENCY {
                continue;
            }
            terms.insert((*tok).to_string());
        }
    }
    terms
}

/// `\b[A-Za-z][A-Za-z0-9_]{1,}\b` matches — identifier-like tokens.
fn identifier_tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_alphabetic() {
            let start = i;
            // Not preceded by a word char (\b).
            if start > 0 && is_word(chars[start - 1]) {
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < chars.len() && is_word(chars[j]) {
                j += 1;
            }
            if j - start >= 2 {
                out.push(chars[start..j].iter().collect());
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

/// `[A-Za-z0-9_\-./\\]+\.[A-Za-z0-9]+` matches — path-ish tokens with an
/// extension.
fn path_tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let is_path = |c: char| {
        c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '\\')
    };
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if is_path(chars[i]) {
            let start = i;
            while i < chars.len() && is_path(chars[i]) {
                i += 1;
            }
            let tok: String = chars[start..i].iter().collect();
            // Must contain `.<alnum+>` extension.
            if let Some((_, ext)) = tok.rsplit_once('.') {
                if !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric()) {
                    out.push(tok);
                }
            }
        } else {
            i += 1;
        }
    }
    out
}

/// `[a-z][a-z0-9_]{2,}` matches over lowercased text — body tokens.
fn body_tokens(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let chars: Vec<char> = lower.chars().collect();
    let is_tok = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_';
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_lowercase() {
            let start = i;
            i += 1;
            while i < chars.len() && is_tok(chars[i]) {
                i += 1;
            }
            if i - start >= 3 {
                out.push(chars[start..i].iter().collect());
            }
        } else {
            i += 1;
        }
    }
    out
}

/// Parse a `CONTEXT.md` into term blocks. Content before the first recognised
/// block start is dropped (it is preamble, not a term).
fn parse_term_blocks(context_text: &str) -> Vec<TermBlock> {
    let mut blocks: Vec<(String, Vec<String>)> = Vec::new();
    let mut current: Option<(String, Vec<String>)> = None;

    for line in context_text.lines() {
        let term = heading_term(line).or_else(|| def_term(line));
        if let Some(t) = term {
            if let Some(c) = current.take() {
                blocks.push(c);
            }
            current = Some((t, vec![line.to_string()]));
        } else if let Some((_, lines)) = current.as_mut() {
            lines.push(line.to_string());
        }
    }
    if let Some(c) = current {
        blocks.push(c);
    }

    blocks
        .into_iter()
        .map(|(term, lines)| TermBlock {
            term,
            text: lines.join("\n").trim_end().to_string(),
        })
        .collect()
}

/// `true` when a relevance term appears as a whole-word-ish substring of the
/// block's term name or full text.
fn block_matches(block: &TermBlock, terms: &BTreeSet<String>) -> bool {
    let hay_term = block.term.to_lowercase();
    let hay_text = block.text.to_lowercase();
    for t in terms {
        if t.is_empty() {
            continue;
        }
        if bounded_contains(&hay_term, t) || bounded_contains(&hay_text, t) {
            return true;
        }
    }
    false
}

/// `true` if `needle` occurs in `haystack` bounded by non-alphanumerics or the
/// string edges — the JS `(^|[^a-z0-9])needle([^a-z0-9]|$)` test.
fn bounded_contains(haystack: &str, needle: &str) -> bool {
    let is_word = |c: char| c.is_ascii_alphanumeric();
    let mut from = 0;
    while let Some(pos) = haystack[from..].find(needle) {
        let abs = from + pos;
        let before_ok = abs == 0
            || !haystack[..abs]
                .chars()
                .next_back()
                .is_some_and(is_word);
        let after_idx = abs + needle.len();
        let after_ok = after_idx >= haystack.len()
            || !haystack[after_idx..].chars().next().is_some_and(is_word);
        if before_ok && after_ok {
            return true;
        }
        from = abs + 1;
        if from >= haystack.len() {
            break;
        }
    }
    false
}

/// Resolve `--context` inputs into `CONTEXT.md` paths, expanding a
/// `CONTEXT-MAP.md` into the `*context.md` references it links. Missing files
/// are skipped; the result is deduped.
fn resolve_context_files(context_paths: &[String]) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut seen: BTreeSet<std::path::PathBuf> = BTreeSet::new();
    let push = |p: std::path::PathBuf, out: &mut Vec<_>, seen: &mut BTreeSet<_>| {
        let norm = p.canonicalize().unwrap_or(p);
        if seen.insert(norm.clone()) && norm.exists() {
            out.push(norm);
        }
    };
    for raw in context_paths {
        if raw.is_empty() {
            continue;
        }
        let base = Path::new(raw)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();
        if base == "context-map.md" {
            let Some(map_text) = read_file_safe(Path::new(raw)) else {
                continue;
            };
            let map_dir = Path::new(raw).parent().unwrap_or(Path::new("."));
            for tok in map_context_refs(&map_text) {
                let ref_path = tok.replace('\\', "/");
                let resolved = if Path::new(&ref_path).is_absolute() {
                    std::path::PathBuf::from(&ref_path)
                } else {
                    map_dir.join(&ref_path)
                };
                push(resolved, &mut out, &mut seen);
            }
        } else {
            push(std::path::PathBuf::from(raw), &mut out, &mut seen);
        }
    }
    out
}

/// `[A-Za-z0-9_\-./\\]*context\.md` matches (case-insensitive) inside a map.
fn map_context_refs(map_text: &str) -> Vec<String> {
    let lower = map_text.to_lowercase();
    let is_path = |c: char| {
        c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '\\')
    };
    let chars: Vec<char> = map_text.chars().collect();
    let lower_chars: Vec<char> = lower.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if is_path(lower_chars[i]) {
            let start = i;
            while i < chars.len() && is_path(lower_chars[i]) {
                i += 1;
            }
            let lower_tok: String = lower_chars[start..i].iter().collect();
            if lower_tok.ends_with("context.md") {
                out.push(chars[start..i].iter().collect());
            }
        } else {
            i += 1;
        }
    }
    out
}

/// Slice one or more `CONTEXT.md` files against a spec.
fn slice_context(
    context_paths: &[String],
    spec_path: &str,
    max_lines: Option<usize>,
) -> SliceResult {
    let cap = max_lines
        .filter(|&n| n > 0)
        .unwrap_or_else(resolve_max_lines);
    let empty = || SliceResult {
        slice: String::new(),
        line_count: 0,
        truncated: false,
        block_count: 0,
    };

    let Some(spec_text) = read_file_safe(Path::new(spec_path)) else {
        return empty();
    };
    let resolved = resolve_context_files(context_paths);
    if resolved.is_empty() {
        return empty();
    }

    let terms = extract_relevance_terms(&spec_text);
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut matched: Vec<String> = Vec::new();

    for path in &resolved {
        let Some(text) = read_file_safe(path) else {
            continue;
        };
        for block in parse_term_blocks(&text) {
            let key = block.term.to_lowercase();
            if seen.contains(&key) {
                continue;
            }
            if block_matches(&block, &terms) {
                seen.insert(key);
                matched.push(block.text);
            }
        }
    }

    if matched.is_empty() {
        return empty();
    }

    let joined = matched.join("\n\n");
    let all_lines: Vec<&str> = joined.split('\n').collect();
    if all_lines.len() <= cap {
        return SliceResult {
            line_count: all_lines.len(),
            slice: joined,
            truncated: false,
            block_count: matched.len(),
        };
    }
    let kept = format!("{}{TRUNCATE_TAIL}", all_lines[..cap].join("\n"));
    SliceResult {
        slice: kept,
        line_count: cap,
        truncated: true,
        block_count: matched.len(),
    }
}

/// Run `mustard-rt run context-slice`, writing the slice to stdout.
///
/// Exit code is always `0` (fail-graceful).
///
/// W8.T8.8 — `--context-claude-md <path>` accepts a CLAUDE.md path as an extra
/// source. CLAUDE.md is not a term-block glossary, so it is sliced through a
/// simpler heuristic: every `## Heading` / `### Heading` block whose body
/// contains a spec-derived relevance term is kept. The output is appended
/// after the CONTEXT.md slice (separated by a blank line) so callers parsing
/// the legacy CONTEXT.md slice see byte-stable output when the new flag is
/// omitted.
pub fn run(
    context: &[String],
    spec: Option<&str>,
    max_lines: Option<usize>,
    context_claude_md: Option<&str>,
) {
    let Some(spec) = spec else {
        eprintln!("[context-slice] --spec <path> is required");
        return;
    };
    if context.is_empty() && context_claude_md.is_none() {
        eprintln!(
            "[context-slice] no --context / --context-claude-md given; emitting empty slice"
        );
        return;
    }

    let mut emitted_anything = false;

    if !context.is_empty() {
        let result = slice_context(context, spec, max_lines);
        if result.truncated {
            eprintln!(
                "[context-slice] WARN: relevant glossary slice is {} blocks and exceeds the \
                 {}-line cap (MUSTARD_GLOSSARY_MAX_LINES). Truncated. Narrow the spec's scope \
                 or raise the cap if every block is needed.",
                result.block_count,
                resolve_max_lines()
            );
        }
        if !result.slice.is_empty() {
            println!("{}", result.slice);
            emitted_anything = true;
        }
    }

    // T8.8: slice CLAUDE.md against the same spec-derived relevance terms.
    if let Some(claude_md_path) = context_claude_md {
        let slice = slice_claude_md(claude_md_path, spec, max_lines);
        if !slice.is_empty() {
            if emitted_anything {
                println!();
            }
            println!("{slice}");
        }
    }

    // Wave 4 (project-profiler): delegate to the unified `context-resolve`
    // walk. The spec's `## Entidades` / `## Entities` section drives the
    // entity seeds; the resolver walks the concept-node graph and emits a
    // one-line stderr summary of the closure. Stdout (the glossary slice)
    // stays byte-stable for the legacy parser. Fail-open everywhere.
    delegate_to_resolver(spec);
}

/// Slice a CLAUDE.md file against the spec-derived relevance terms.
///
/// Strategy: parse CLAUDE.md into heading-bounded sections (depth 2-3) and
/// keep every section whose heading or body contains any relevance term.
/// Fail-graceful: a missing CLAUDE.md or spec yields an empty string.
fn slice_claude_md(claude_md_path: &str, spec_path: &str, max_lines: Option<usize>) -> String {
    let cap = max_lines
        .filter(|&n| n > 0)
        .unwrap_or_else(resolve_max_lines);
    let Some(spec_text) = read_file_safe(Path::new(spec_path)) else {
        return String::new();
    };
    let Some(claude_text) = read_file_safe(Path::new(claude_md_path)) else {
        return String::new();
    };
    let terms = extract_relevance_terms(&spec_text);
    if terms.is_empty() {
        return String::new();
    }

    // Parse into ## / ### sections.
    let lines: Vec<&str> = claude_text.lines().collect();
    let mut sections: Vec<Vec<&str>> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in &lines {
        if heading_term(line).is_some() && !current.is_empty() {
            sections.push(std::mem::take(&mut current));
        }
        current.push(line);
    }
    if !current.is_empty() {
        sections.push(current);
    }

    let mut kept: Vec<String> = Vec::new();
    for sec in sections {
        let body = sec.join("\n");
        let body_lower = body.to_ascii_lowercase();
        let matches = terms.iter().any(|t| bounded_contains(&body_lower, t));
        if matches {
            kept.push(body);
        }
    }
    if kept.is_empty() {
        return String::new();
    }
    let joined = kept.join("\n\n");
    let all_lines: Vec<&str> = joined.split('\n').collect();
    if all_lines.len() <= cap {
        format!("## CLAUDE.md (slice)\n{joined}")
    } else {
        let trimmed = all_lines[..cap].join("\n");
        format!("## CLAUDE.md (slice)\n{trimmed}{TRUNCATE_TAIL}")
    }
}

/// Pull entity names from the spec's `## Entidades`/`## Entities` section
/// and feed them to the unified resolver. The resolver is invoked purely
/// for its side-effect (a stderr summary + cache warm-up) — the stdout
/// glossary slice produced above is the byte-stable contract.
fn delegate_to_resolver(spec_path: &str) {
    let Some(spec_text) = read_file_safe(Path::new(spec_path)) else {
        return;
    };
    let section = extract_section(&spec_text, &["Entidades", "Entities"]);
    let entities: Vec<String> = identifier_tokens(&section);
    if entities.is_empty() {
        return;
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let scope = crate::run::scan::resolve::ResolveScope {
        entities,
        ..crate::run::scan::resolve::ResolveScope::default()
    };
    let out = crate::run::scan::resolve::resolve_closure(&cwd, &scope);
    if !out.closure.is_empty() {
        eprintln!(
            "[context-slice] context-resolve closure={} truncated={}",
            out.closure.len(),
            out.truncated,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parse_term_blocks_splits_on_headings_and_def_lines() {
        let text = "preamble\n## Alpha\nbody a\n### Beta\nbody b\n- **Gamma** def g";
        let blocks = parse_term_blocks(text);
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].term, "Alpha");
        assert_eq!(blocks[2].term, "Gamma");
    }

    #[test]
    fn extract_relevance_terms_pulls_entity_names() {
        let spec = "## Entities\n- UserAccount\n- OrderLine\n\n## Body\nshort.";
        let terms = extract_relevance_terms(spec);
        assert!(terms.contains("useraccount"));
        assert!(terms.contains("orderline"));
    }

    #[test]
    fn bounded_contains_respects_word_boundaries() {
        assert!(bounded_contains("the user account", "user"));
        assert!(!bounded_contains("superuser", "user"));
        assert!(bounded_contains("user", "user"));
    }

    #[test]
    fn slice_context_returns_matching_blocks_only() {
        let dir = tempdir().unwrap();
        let spec = dir.path().join("spec.md");
        std::fs::write(&spec, "## Entities\n- Widget\n").unwrap();
        let ctx = dir.path().join("CONTEXT.md");
        std::fs::write(&ctx, "## Widget\nA widget thing.\n## Gadget\nUnrelated.\n").unwrap();
        let result = slice_context(
            &[ctx.to_string_lossy().to_string()],
            &spec.to_string_lossy(),
            None,
        );
        assert_eq!(result.block_count, 1);
        assert!(result.slice.contains("Widget"));
        assert!(!result.slice.contains("Gadget"));
    }

    #[test]
    fn slice_context_missing_spec_is_empty() {
        let result = slice_context(&["nope.md".to_string()], "missing-spec.md", None);
        assert_eq!(result.block_count, 0);
        assert!(result.slice.is_empty());
    }
}
