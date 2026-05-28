//! `extract_function_signatures` — pull public function declarations out of
//! a source blob.
//!
//! Two paths:
//!
//! 1. **AST path** (precise). When the loader has the language and a
//!    `function_signature.scm` query is installed under
//!    `.claude/grammars/{lang_id}/queries/`, run it against the parsed tree
//!    and emit one [`FunctionSig`] per capture group. The query is the
//!    source of truth — it decides what counts as "public".
//!
//! 2. **Fallback path** (heuristic, agnostic). When the grammar is missing
//!    OR the project ships no `function_signature.scm`, run a single regex
//!    over the source that matches the universal "public function" lexical
//!    shape across languages. The regex is documented as imprecise and
//!    explicitly does **not** branch on a language id — the same expression
//!    runs for every input. False positives are acceptable in this layer:
//!    a downstream consumer (the regression gate) reconciles against the
//!    declared touched-functions list before flagging anything.
//!
//! The fallback regex is the only piece of "heuristic" code under `ast::*`,
//! and even that is agnostic by construction.

use super::{FunctionSig, GrammarLoader, QuerySet, TreeSitterParser};
use tree_sitter::{Query, QueryCursor, StreamingIterator};

/// Agnostic fallback regex for public function declarations.
///
/// **Documented as imprecise.** This is a lexical shotgun that captures the
/// common "public-ish function declaration" shape across mainstream
/// languages. It explicitly does not enumerate language ids; it matches
/// whatever the surface syntax has in common. False positives are tolerated
/// because the consumer (the regression gate) reconciles against the
/// declared touched-functions list before scoring anything.
///
/// Recognised lexical prefixes:
///
/// - `pub fn NAME`, `pub async fn NAME`, `pub(crate) fn NAME` (Rust)
/// - `export function NAME`, `export async function NAME` (TS/JS)
/// - `export default function NAME` (TS/JS, named default exports)
/// - `def NAME` (Python — every `def` is module-public)
/// - `func NAME` (Go — capitalised names are public)
/// - `public T NAME` (.NET — `T` is any token, so this is loose)
/// - `public function NAME` (PHP)
/// - `function NAME` at column 0 (Lua / shell)
///
/// The captured name is the identifier immediately following the prefix.
/// Surrounding generics, attributes, and qualifiers are not captured.
///
/// Built lazily via [`OnceLock`] so the compile happens once per process.
const FALLBACK_FUNCTION_PATTERN: &str =
    r"(?m)^[ \t]*(?:pub(?:\s*\([^)]*\))?\s+(?:async\s+)?fn|\
       export\s+(?:default\s+)?(?:async\s+)?function|\
       export\s+(?:async\s+)?function|\
       public\s+(?:static\s+)?(?:async\s+)?(?:[A-Za-z_][\w<>,\s\[\]]*\s+)?function|\
       public\s+(?:static\s+)?(?:async\s+)?[A-Za-z_][\w<>,\s\[\]]*\s+|\
       def|\
       func|\
       function)\s+([A-Za-z_][A-Za-z0-9_]*)";

/// Extract public function signatures from `source`.
///
/// When the AST path is available (grammar resolved by `loader` AND the
/// project ships a `function_signature.scm` query), it is preferred and
/// returns precise signatures including params and return types when the
/// query captures them. Otherwise the fallback path returns name-only
/// signatures (empty `params`/`return_type`).
///
/// Never panics. Returns an empty vector when no signatures can be
/// extracted.
#[must_use]
pub fn extract_function_signatures(
    loader: &GrammarLoader,
    source: &str,
    lang_id: &str,
) -> Vec<FunctionSig> {
    // AST path: language + query present.
    if let Some(language) = loader.language(lang_id) {
        let set = QuerySet::load_for(lang_id, loader.project_root(), Some(&language));
        if let Some(query) = set.function_signature() {
            if let Ok(mut parser) = TreeSitterParser::for_language(loader, lang_id) {
                if let Ok(tree) = parser.parse(source) {
                    return extract_via_query(query, tree.as_tree_sitter(), source);
                }
            }
        }
    }
    extract_via_fallback_regex(source)
}

/// Run `query` over `tree` and emit one [`FunctionSig`] per `@name` capture.
///
/// Query conventions (per `function_signature.scm` template Mustard ships
/// in the next wave's grammars bundle):
///
/// - `@name` — function identifier node (required)
/// - `@params` — parameter list node (optional)
/// - `@return` — return-type node (optional)
fn extract_via_query(query: &Query, tree: &tree_sitter::Tree, source: &str) -> Vec<FunctionSig> {
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), source.as_bytes());

    let mut out: Vec<FunctionSig> = Vec::new();
    let capture_names = query.capture_names();

    while let Some(m) = matches.next() {
        let mut name: Option<(String, std::ops::Range<usize>)> = None;
        let mut params = String::new();
        let mut return_type = String::new();

        for cap in m.captures {
            let cap_name = capture_names
                .get(cap.index as usize)
                .copied()
                .unwrap_or("");
            let node = cap.node;
            let start = node.start_byte();
            let end = node.end_byte();
            let text = source.get(start..end).unwrap_or("");
            match cap_name {
                "name" => name = Some((text.to_string(), start..end)),
                "params" => params = text.to_string(),
                "return" => return_type = text.to_string(),
                _ => {}
            }
        }

        if let Some((name, span)) = name {
            out.push(FunctionSig {
                name,
                params,
                return_type,
                span,
            });
        }
    }

    out
}

/// Fallback agnostic regex sweep. Documented as imprecise — see
/// [`FALLBACK_FUNCTION_PATTERN`].
fn extract_via_fallback_regex(source: &str) -> Vec<FunctionSig> {
    let re = fallback_regex();
    let mut out: Vec<FunctionSig> = Vec::new();
    for cap in re.captures_iter(source) {
        // Capture group 1 holds the identifier under every alternation.
        let Some(name_match) = cap.get(1) else {
            continue;
        };
        out.push(FunctionSig {
            name: name_match.as_str().to_string(),
            params: String::new(),
            return_type: String::new(),
            span: name_match.start()..name_match.end(),
        });
    }
    out
}

/// One-shot compile of [`FALLBACK_FUNCTION_PATTERN`] guarded by a `OnceLock`.
/// Returns a reference to the cached compiled regex.
fn fallback_regex() -> &'static MiniRegex {
    use std::sync::OnceLock;
    static CELL: OnceLock<MiniRegex> = OnceLock::new();
    CELL.get_or_init(|| MiniRegex::compile(FALLBACK_FUNCTION_PATTERN))
}

// ---------------------------------------------------------------------------
// Tiny regex engine wrapper
// ---------------------------------------------------------------------------
//
// `mustard-core` does not currently depend on the `regex` crate. The
// fallback sweep is small and self-contained, so rather than adding a new
// dependency for a single call site we implement a minimal pattern matcher
// that handles the universal "function-like prefix + identifier" shape we
// actually need. It is *not* a general regex engine; it understands a fixed
// list of literal-prefix alternatives plus the identifier capture group.
//
// The list of prefixes is derived from [`FALLBACK_FUNCTION_PATTERN`] but is
// expressed as plain string literals — no language id appears anywhere.

/// Minimal pattern matcher for the agnostic fallback. Holds a list of
/// `(prefix, prefix_consumes_type_token)` tuples; matching strips the
/// optional intermediate tokens (`async`, type tokens for `public T name`)
/// and captures the trailing identifier.
struct MiniRegex {
    // Each entry: literal prefix (case-sensitive) that must be matched at
    // the *start* of a logical line (after leading whitespace) before we
    // try to capture an identifier.
    prefixes: Vec<&'static str>,
}

impl MiniRegex {
    fn compile(_pattern: &str) -> Self {
        // The pattern is fixed; we don't actually parse the regex source.
        // Hard-code the agnostic prefix list extracted from
        // FALLBACK_FUNCTION_PATTERN above. Order matters: longest first so
        // `pub async fn` is tried before `pub fn`.
        Self {
            prefixes: vec![
                "pub(crate) async fn",
                "pub(crate) fn",
                "pub async fn",
                "pub fn",
                "export default async function",
                "export default function",
                "export async function",
                "export function",
                "public static async function",
                "public static function",
                "public async function",
                "public function",
                "public static",
                "public",
                "function",
                "async function",
                "def",
                "func",
            ],
        }
    }

    fn captures_iter<'a, 'b>(&'a self, source: &'b str) -> CapturesIter<'a, 'b> {
        CapturesIter {
            engine: self,
            source,
            pos: 0,
        }
    }
}

struct CapturesIter<'a, 'b> {
    engine: &'a MiniRegex,
    source: &'b str,
    pos: usize,
}

struct Match {
    name_start: usize,
    name_end: usize,
}

impl Match {
    fn start(&self) -> usize {
        self.name_start
    }
    fn end(&self) -> usize {
        self.name_end
    }
}

// Adapter so the existing call site can pretend it's a `regex::Captures`.
struct Captures<'b> {
    match_: Match,
    source: &'b str,
}

impl<'b> Captures<'b> {
    fn get(&self, idx: usize) -> Option<MatchView<'b>> {
        if idx == 1 {
            Some(MatchView {
                start: self.match_.start(),
                end: self.match_.end(),
                source: self.source,
            })
        } else {
            None
        }
    }
}

struct MatchView<'b> {
    start: usize,
    end: usize,
    source: &'b str,
}

impl<'b> MatchView<'b> {
    fn as_str(&self) -> &'b str {
        &self.source[self.start..self.end]
    }
    fn start(&self) -> usize {
        self.start
    }
    fn end(&self) -> usize {
        self.end
    }
}

impl<'b> Iterator for CapturesIter<'_, 'b> {
    type Item = Captures<'b>;
    fn next(&mut self) -> Option<Self::Item> {
        while self.pos < self.source.len() {
            // Advance to start-of-line. Find next '\n' or use current line.
            let line_start = self.pos;
            let rest = &self.source[line_start..];
            let line_end_rel = rest.find('\n').unwrap_or(rest.len());
            let line = &rest[..line_end_rel];
            let next_pos = line_start + line_end_rel + 1; // skip '\n'

            // Strip leading whitespace.
            let stripped = line.trim_start();
            let leading_ws = line.len() - stripped.len();

            // Try each prefix.
            for prefix in &self.engine.prefixes {
                if let Some(after_prefix) = stripped.strip_prefix(prefix) {
                    // Must be followed by at least one whitespace char.
                    if after_prefix
                        .chars()
                        .next()
                        .is_some_and(char::is_whitespace)
                    {
                        // Skip across optional intermediate tokens:
                        // for `public T name`, the prefix matched is
                        // `public` and we need to skip the type token.
                        // For `pub fn name` the prefix already consumed
                        // `fn`, so no extra skip needed.
                        let after_prefix_ws = after_prefix.trim_start();
                        let (name_in_line, name_offset_in_after) =
                            extract_identifier_after(prefix, after_prefix_ws);
                        if let Some(name) = name_in_line {
                            // Compute absolute byte offsets back into the
                            // full source.
                            let prefix_end_in_line = leading_ws + prefix.len();
                            let after_prefix_len = after_prefix.len() - after_prefix_ws.len();
                            let name_start = line_start
                                + prefix_end_in_line
                                + after_prefix_len
                                + name_offset_in_after;
                            let name_end = name_start + name.len();
                            self.pos = next_pos;
                            return Some(Captures {
                                match_: Match { name_start, name_end },
                                source: self.source,
                            });
                        }
                    }
                }
            }
            self.pos = next_pos;
        }
        None
    }
}

/// Extract the identifier from `after`, handling the few prefixes that need
/// to skip across an intermediate token (`public T name` / `function NAME`).
///
/// Returns `(Some(name), name_offset_in_after)` when an identifier is found.
/// `name_offset_in_after` is the byte position of the identifier inside the
/// `after` slice.
fn extract_identifier_after<'a>(prefix: &str, after: &'a str) -> (Option<&'a str>, usize) {
    // `public T name` and `public static T name` — skip until last token.
    // These prefixes are followed by a type token then an identifier.
    let needs_type_skip = matches!(prefix, "public" | "public static");

    if needs_type_skip {
        // Take tokens until we find the last one before `(` or end-of-line.
        // The last word-token is the identifier.
        let mut last_ident: Option<(&str, usize)> = None;
        for token_match in iter_word_tokens(after) {
            let (token, start, end) = token_match;
            if is_identifier(token) {
                last_ident = Some((token, start));
            }
            // Stop at the first '(' — it begins the parameter list.
            if after[end..].starts_with('(') {
                break;
            }
        }
        return match last_ident {
            Some((name, off)) => (Some(name), off),
            None => (None, 0),
        };
    }

    // All other prefixes: the identifier is the first identifier-looking
    // token in `after`.
    let first = iter_word_tokens(after).find(|(t, _, _)| is_identifier(t));
    match first {
        Some((name, start, _)) => (Some(name), start),
        None => (None, 0),
    }
}

/// Iterator over `(token, start, end)` triples — word-like runs of
/// `[A-Za-z_0-9]` separated by anything else.
fn iter_word_tokens(s: &str) -> impl Iterator<Item = (&str, usize, usize)> {
    let bytes = s.as_bytes();
    let mut out: Vec<(&str, usize, usize)> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if is_word_byte(bytes[i]) {
            let start = i;
            while i < bytes.len() && is_word_byte(bytes[i]) {
                i += 1;
            }
            // Safe slice: the boundaries are at byte positions where
            // is_word_byte was true (so they're ASCII), guaranteeing UTF-8
            // boundary alignment.
            if let Some(tok) = s.get(start..i) {
                out.push((tok, start, i));
            }
        } else {
            i += 1;
        }
    }
    out.into_iter()
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn is_identifier(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap_or(' ');
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_extracts_rust_pub_fn() {
        let tmp = tempfile::tempdir().unwrap();
        let loader = GrammarLoader::empty(tmp.path());
        let source = "pub fn foo(a: i32) -> i32 { a }";
        let sigs = extract_function_signatures(&loader, source, "rust");
        assert!(sigs.iter().any(|s| s.name == "foo"));
    }

    #[test]
    fn fallback_extracts_rust_pub_async_fn() {
        let tmp = tempfile::tempdir().unwrap();
        let loader = GrammarLoader::empty(tmp.path());
        let source = "pub async fn handle() -> Result<()> { Ok(()) }";
        let sigs = extract_function_signatures(&loader, source, "rust");
        assert!(sigs.iter().any(|s| s.name == "handle"));
    }

    #[test]
    fn fallback_extracts_typescript_export_function() {
        let tmp = tempfile::tempdir().unwrap();
        let loader = GrammarLoader::empty(tmp.path());
        let source = "export function compute(x: number): number { return x; }";
        let sigs = extract_function_signatures(&loader, source, "typescript");
        assert!(sigs.iter().any(|s| s.name == "compute"));
    }

    #[test]
    fn fallback_extracts_typescript_export_async_function() {
        let tmp = tempfile::tempdir().unwrap();
        let loader = GrammarLoader::empty(tmp.path());
        let source = "export async function load() { return 1; }";
        let sigs = extract_function_signatures(&loader, source, "typescript");
        assert!(sigs.iter().any(|s| s.name == "load"));
    }

    #[test]
    fn fallback_extracts_python_def() {
        let tmp = tempfile::tempdir().unwrap();
        let loader = GrammarLoader::empty(tmp.path());
        let source = "def parse(text):\n    return text\n";
        let sigs = extract_function_signatures(&loader, source, "python");
        assert!(sigs.iter().any(|s| s.name == "parse"));
    }

    #[test]
    fn fallback_extracts_go_func() {
        let tmp = tempfile::tempdir().unwrap();
        let loader = GrammarLoader::empty(tmp.path());
        let source = "func Handle(req Request) error { return nil }";
        let sigs = extract_function_signatures(&loader, source, "go");
        assert!(sigs.iter().any(|s| s.name == "Handle"));
    }

    #[test]
    fn fallback_returns_empty_on_garbage() {
        let tmp = tempfile::tempdir().unwrap();
        let loader = GrammarLoader::empty(tmp.path());
        let sigs = extract_function_signatures(&loader, "lkj asdf 1234", "anything");
        assert!(sigs.is_empty());
    }

    #[test]
    fn fallback_span_locates_identifier() {
        let tmp = tempfile::tempdir().unwrap();
        let loader = GrammarLoader::empty(tmp.path());
        let source = "pub fn alpha() {}\npub fn beta() {}\n";
        let sigs = extract_function_signatures(&loader, source, "rust");
        let alpha = sigs.iter().find(|s| s.name == "alpha").expect("alpha");
        assert_eq!(&source[alpha.span.clone()], "alpha");
        let beta = sigs.iter().find(|s| s.name == "beta").expect("beta");
        assert_eq!(&source[beta.span.clone()], "beta");
    }
}
