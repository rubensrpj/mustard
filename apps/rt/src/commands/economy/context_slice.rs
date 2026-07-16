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

use mustard_core::io::fs;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// A token shorter than this is almost always a common word — no signal.
const MIN_TOKEN_LEN: usize = 4;

/// A token appearing in more than this fraction of body tokens carries no
/// signal (it shows up everywhere).
const MAX_TOKEN_FREQUENCY: f64 = 0.04;

/// One parsed term block: its heading/definition term and its full text.
#[derive(Debug, Clone)]
pub(crate) struct TermBlock {
    term: String,
    text: String,
}

impl TermBlock {
    /// The block's heading/definition term — read by sibling modules
    /// (`grill_capture`) that resolve a glossary the SAME way the slicer does
    /// and need to know which terms already have a block (update-not-duplicate).
    pub(crate) fn term(&self) -> &str {
        &self.term
    }
}

/// Result of slicing — mirrors the JS `sliceContext` return object.
#[derive(Debug)]
pub(crate) struct SliceResult {
    pub slice: String,
}

/// Read a file, returning `None` on any error (fail-graceful).
fn read_file_safe(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok()
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
pub(crate) fn parse_term_blocks(context_text: &str) -> Vec<TermBlock> {
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
pub(crate) fn block_matches(block: &TermBlock, terms: &BTreeSet<String>) -> bool {
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
pub(crate) fn resolve_context_files(context_paths: &[String]) -> Vec<std::path::PathBuf> {
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

/// Directly-requested context paths (the `--context` / `--context-claude-md`
/// args) that do not exist on disk. A caller naming a path that is absent is a
/// misconfiguration worth surfacing — distinct from the legitimate "no
/// `CONTEXT.md` glossary authored" case, where no path is named at all. Empty
/// strings are ignored (an unfilled placeholder, not a request).
fn missing_requested_paths(paths: &[&str]) -> Vec<String> {
    paths
        .iter()
        .filter(|p| !p.is_empty() && !Path::new(p).exists())
        .map(|p| (*p).to_string())
        .collect()
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
fn slice_context(context_paths: &[String], spec_path: &str) -> SliceResult {
    let empty = || SliceResult { slice: String::new() };

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

    // No line cap: relevance (`block_matches`) is the only filter — every
    // matched block is emitted in full.
    let joined = matched.join("\n\n");
    SliceResult {
        slice: joined,
    }
}

/// Relevance-slice a `CONTEXT.md` body against free relevance-source text (a
/// dispatch prompt, say) instead of a spec file — the SAME term-block matching
/// [`slice_context`] runs, keyed on arbitrary text. Returns the matched blocks
/// joined (empty when nothing matches). No size cap — relevance is the only
/// filter, every matched block in full. The shared home so the `subagent_inject`
/// hook gets the same relevance slice as the renderer, never a raw CONTEXT.md dump.
#[must_use]
pub fn slice_text(context_md: &str, relevance_source: &str) -> String {
    // Terms straight from the source prose (a dispatch prompt). The spec-keyed
    // `extract_relevance_terms` only reads `## Entities`/`## Files` sections,
    // which free prompt text lacks — so here significant content tokens (≥
    // `MIN_TOKEN_LEN` chars, lowercased) are the relevance signal.
    let terms: BTreeSet<String> = relevance_source
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| t.chars().count() >= MIN_TOKEN_LEN)
        .map(str::to_ascii_lowercase)
        .collect();
    if terms.is_empty() {
        return String::new();
    }
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut matched: Vec<String> = Vec::new();
    for block in parse_term_blocks(context_md) {
        let key = block.term.to_lowercase();
        if seen.contains(&key) {
            continue;
        }
        if block_matches(&block, &terms) {
            seen.insert(key);
            matched.push(block.text);
        }
    }
    matched.join("\n\n")
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

    // Surface any directly-requested context path that is absent — a caller
    // misconfiguration (e.g. a stale `guards.md` path), not the by-design "no
    // glossary authored → blank" case where no path is named at all.
    let mut requested: Vec<&str> = context.iter().map(String::as_str).collect();
    if let Some(p) = context_claude_md {
        requested.push(p);
    }
    for missing in missing_requested_paths(&requested) {
        eprintln!("[context-slice] requested context file not found: {missing}");
    }

    let mut emitted_anything = false;

    if !context.is_empty() {
        let result = slice_context(context, spec);
        if !result.slice.is_empty() {
            println!("{}", result.slice);
            emitted_anything = true;
        }
    }

    // T8.8: slice CLAUDE.md against the same spec-derived relevance terms.
    if let Some(claude_md_path) = context_claude_md {
        let slice = slice_claude_md(claude_md_path, spec);
        if !slice.is_empty() {
            if emitted_anything {
                println!();
            }
            println!("{slice}");
        }
    }

}

/// Slice a CLAUDE.md file against the spec-derived relevance terms.
///
/// Strategy: parse CLAUDE.md into heading-bounded sections (depth 2-3) and
/// keep every section whose heading or body contains any relevance term.
/// Fail-graceful: a missing CLAUDE.md or spec yields an empty string.
fn slice_claude_md(claude_md_path: &str, spec_path: &str) -> String {
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
    // No line cap — every relevance-matched section is kept in full.
    format!("## CLAUDE.md (slice)\n{}", kept.join("\n\n"))
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
    fn missing_requested_paths_reports_only_absent_named_paths() {
        let dir = tempdir().unwrap();
        let present = dir.path().join("CLAUDE.md");
        std::fs::write(&present, "# c").unwrap();
        let present_s = present.to_string_lossy().to_string();
        let absent_s = dir.path().join("guards.md").to_string_lossy().to_string();

        // A directly-named but absent path is reported; an existing one is not;
        // an empty placeholder is ignored.
        let missing =
            missing_requested_paths(&[present_s.as_str(), absent_s.as_str(), ""]);
        assert_eq!(missing, vec![absent_s.clone()]);

        // No path named at all → nothing reported (the "no glossary authored,
        // blank by design" case stays silent).
        assert!(missing_requested_paths(&[]).is_empty());
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
        );
        assert!(result.slice.contains("Widget"));
        assert!(!result.slice.contains("Gadget"));
    }

    #[test]
    fn slice_context_missing_spec_is_empty() {
        let result = slice_context(&["nope.md".to_string()], "missing-spec.md");
        assert!(result.slice.is_empty());
    }
}
