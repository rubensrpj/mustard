//! Layer 2 — Syntactic extraction, language-agnostic.
//!
//! One generic tree-sitter engine drives every language. A language is defined
//! entirely by DATA: a row in `languages.toml` (name, extensions, grammar) and a
//! set of `.scm` query files under `queries/<dir>/`. This module never names a
//! language, an extension, or a grammar node — it only understands a small,
//! generic capture vocabulary that every query speaks. The whole list, with
//! what each capture means, lives in `queries/README.md`, the one place it is
//! written.
//!
//! Three things come off the tree itself, with no capture and no grammar node
//! name: the documentation comment above a declaration (the `extra` nodes the
//! grammar attaches right above it), its signature (its own text up to the
//! body), the call sites of the file (a named leaf that reads as an
//! identifier and is followed by an opening parenthesis), and the names it
//! cites without calling (the same leaf, starting with a capital letter and
//! not followed by a parenthesis). A grammar that marks
//! none of it simply yields nothing, as with every other generic rule here.
//!
//! The per-language seam the old design called for is preserved: there is one
//! [`Analyzer`] instance per language, but all are the same generic type, each
//! parameterized by a compiled query. Precise AST facts go in; the same generic
//! `Extracted`/`Decl` come out, so the miner (Layer 4) never learns any syntax.
//!
//! `build.rs` embeds the registry and the query files into `OUT_DIR`; we include
//! the generated table here. Nothing language-specific lives in this file.

use crate::model::{CallSite, Decl};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Node, Parser, Query, QueryCursor};

#[derive(Default)]
pub(crate) struct Extracted {
    pub imports: Vec<String>,
    /// The imports the language puts in sight of more files than the one that
    /// writes them (`@import.global`).
    pub global_imports: Vec<String>,
    pub namespaces: Vec<String>,
    pub declarations: Vec<Decl>,
    pub calls: Vec<CallSite>,
    pub cites: Vec<CallSite>,
}

/// One language as produced by `build.rs` from `languages.toml` + its `.scm`
/// files. The grammar is already resolved to a tree-sitter [`Language`].
/// (Extensions live in the separate `LANG_EXTENSIONS` table used for detection.)
pub struct RawLang {
    pub name: &'static str,
    pub query: &'static str,
    pub language: Language,
}

// Brings `raw_langs()` and `LANG_EXTENSIONS` into scope — generated from the
// external language registry; see build.rs. This is the only place the grammar
// symbols are referenced, and it lives in OUT_DIR, not in src/.
include!(concat!(env!("OUT_DIR"), "/langs_generated.rs"));

/// Detect a file's language purely from data (the registry's extension table).
/// No `match` on extensions, no hardcoded mapping — adding a language to
/// `languages.toml` extends detection automatically.
pub fn detect_language(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    LANG_EXTENSIONS
        .iter()
        .find(|(_, exts)| exts.iter().any(|e| *e == ext))
        .map(|(name, _)| (*name).to_string())
}

/// Root-alias segments a language uses to alias the package root in qualified
/// import paths — pure registry data (`root_aliases` in languages.toml). A
/// language that declares none gets an empty slice, which disables the graph's
/// root-alias resolution branch for its modules.
pub fn root_aliases(lang: &str) -> &'static [&'static str] {
    LANG_ROOT_ALIASES
        .iter()
        .find(|(name, _)| *name == lang)
        .map(|(_, aliases)| *aliases)
        .unwrap_or(&[])
}

/// How a language's declared namespace is seen by its other files — pure
/// registry data (`namespace_scope` in languages.toml): `"folder"`, `"nested"`,
/// or empty when the language declares none.
pub fn namespace_scope(lang: &str) -> &'static str {
    LANG_NAMESPACE_SCOPE.iter().find(|(name, _)| *name == lang).map_or("", |(_, scope)| *scope)
}

/// The file extensions of a language, as the registry writes them.
pub fn extensions(lang: &str) -> &'static [&'static str] {
    LANG_EXTENSIONS.iter().find(|(name, _)| *name == lang).map_or(&[], |(_, exts)| *exts)
}

/// Build one [`Analyzer`] per language declared in the registry. A language
/// whose grammar/queries fail to compile is skipped with a warning rather than
/// aborting the whole run.
pub fn registry() -> HashMap<String, Analyzer> {
    let mut m = HashMap::new();
    for raw in raw_langs() {
        if let Some(a) = Analyzer::new(&raw) {
            m.insert(a.name.clone(), a);
        }
    }
    m
}

/// What a capture name means to the engine. Computed once per compiled query so
/// the hot path is an index lookup, not a string compare.
enum CapKind {
    Import,
    /// An import that is in sight of every file of the language under the
    /// same project, not only of the file that writes it.
    ImportGlobal,
    Namespace,
    Name,
    Supertype,
    /// An attribute or decorator adorning a declaration: never code of it.
    Decoration,
    /// The body of a declaration that the grammar keeps beside it rather than
    /// inside it: the declaration ends where its body ends.
    Body,
    Def(String),
    Ignore,
}

fn classify(cap: &str) -> CapKind {
    match cap {
        "import" => CapKind::Import,
        "import.global" => CapKind::ImportGlobal,
        "namespace" => CapKind::Namespace,
        "name" => CapKind::Name,
        "supertype" => CapKind::Supertype,
        "decoration" => CapKind::Decoration,
        "body" => CapKind::Body,
        other => match other.strip_prefix("definition.") {
            Some(kind) => CapKind::Def(kind.to_string()),
            None => CapKind::Ignore,
        },
    }
}

pub(crate) struct Analyzer {
    name: String,
    language: Language,
    query: Query,
    /// `cap_kinds[i]` is the role of capture index `i` in `query`.
    cap_kinds: Vec<CapKind>,
}

impl Analyzer {
    fn new(raw: &RawLang) -> Option<Analyzer> {
        let language = raw.language.clone();
        // Compile patterns individually and keep the ones that hold against this
        // grammar version. A single drifted node name then costs one pattern, not
        // the whole language — and never a panic.
        let good = compile_good_patterns(&language, raw.query, raw.name);
        if good.is_empty() {
            eprintln!("grain: no usable query patterns for '{}' — skipping", raw.name);
            return None;
        }
        let combined = good.join("\n");
        let query = match Query::new(&language, &combined) {
            Ok(q) => q,
            Err(e) => {
                eprintln!("grain: query for '{}' failed to compile: {e}", raw.name);
                return None;
            }
        };
        let cap_kinds = query.capture_names().iter().map(|n| classify(n)).collect();
        Some(Analyzer { name: raw.name.to_string(), language, query, cap_kinds })
    }

    pub fn extract(&self, src: &str) -> Extracted {
        let mut out = Extracted::default();
        let mut parser = Parser::new();
        if parser.set_language(&self.language).is_err() {
            return out;
        }
        let tree = match parser.parse(src, None) {
            Some(t) => t,
            None => return out,
        };
        let bytes = src.as_bytes();
        let root = tree.root_node();
        let mut cursor = QueryCursor::new();

        // Declarations keyed by node start byte (so they emerge in document
        // order); supertypes keyed by the cleaned declaration name so a base
        // captured in a detached node attaches to the right decl.
        // The comment and the header are read only after every match, once the
        // decorations of the whole file are known: a decoration may be matched
        // after the declaration it adorns.
        let mut decls: BTreeMap<usize, Header> = BTreeMap::new();
        let mut decorations: Spans = BTreeSet::new();
        // What an import or a namespace capture covers: the names written
        // there are the path of the import, not a use of what they name.
        let mut import_spans: Spans = BTreeSet::new();
        let mut supers_by_name: HashMap<String, BTreeSet<String>> = HashMap::new();

        let mut matches = cursor.matches(&self.query, root, bytes);
        while let Some(m) = matches.next() {
            let mut def: Option<(Node, &str)> = None;
            let mut name_text: Option<String> = None;
            let mut name_byte = usize::MAX;
            let mut here_supers: Vec<String> = Vec::new();
            let mut body_end: Option<usize> = None;

            for cap in m.captures {
                let node = cap.node;
                match &self.cap_kinds[cap.index as usize] {
                    CapKind::Import | CapKind::ImportGlobal => {
                        import_spans.insert((node.start_byte(), node.end_byte()));
                        if let Ok(t) = node.utf8_text(bytes) {
                            let c = clean_import(t);
                            if !c.is_empty() {
                                if matches!(self.cap_kinds[cap.index as usize], CapKind::ImportGlobal) {
                                    out.global_imports.push(c);
                                } else {
                                    out.imports.push(c);
                                }
                            }
                        }
                    }
                    CapKind::Namespace => {
                        import_spans.insert((node.start_byte(), node.end_byte()));
                        if let Ok(t) = node.utf8_text(bytes) {
                            let t = t.trim();
                            if !t.is_empty() {
                                out.namespaces.push(t.to_string());
                            }
                        }
                    }
                    CapKind::Name => {
                        if let Ok(t) = node.utf8_text(bytes) {
                            name_text = Some(t.to_string());
                            name_byte = node.start_byte();
                        }
                    }
                    CapKind::Decoration => {
                        decorations.insert((node.start_byte(), node.end_byte()));
                    }
                    CapKind::Body => {
                        body_end = Some(node.end_position().row + 1);
                    }
                    CapKind::Supertype => {
                        if let Ok(t) = node.utf8_text(bytes)
                            && let Some(n) = simple_type_name(t) {
                                here_supers.push(n);
                            }
                    }
                    CapKind::Def(kind) => {
                        def = Some((node, kind.as_str()));
                    }
                    CapKind::Ignore => {}
                }
            }

            if let (Some((node, kind)), Some(name)) = (def, &name_text) {
                let header = decls.entry(node.start_byte()).or_insert_with(|| Header {
                    kind: kind.to_string(),
                    name: name.clone(),
                    node,
                    name_byte,
                    body_end: None,
                });
                header.body_end = header.body_end.max(body_end);
            }
            if let Some(name) = &name_text
                && !here_supers.is_empty() {
                    let key = simple_type_name(name).unwrap_or_else(|| name.clone());
                    let bucket = supers_by_name.entry(key).or_default();
                    for s in here_supers {
                        bucket.insert(s);
                    }
                }
        }

        // Where each declaration's own name is written: that name followed by
        // `(` is the header, not a call.
        let names_at: BTreeSet<usize> = decls.values().map(|h| h.name_byte).collect();
        out.declarations = decls
            .into_values()
            .map(|h| {
                let key = simple_type_name(&h.name).unwrap_or_else(|| h.name.clone());
                let supertypes = supers_by_name
                    .get(&key)
                    .map(|s| s.iter().cloned().collect())
                    .unwrap_or_default();
                Decl {
                    kind: h.kind,
                    name: h.name,
                    line: h.node.start_position().row + 1,
                    end_line: (h.node.end_position().row + 1).max(h.body_end.unwrap_or(0)),
                    supertypes,
                    doc: doc_above(h.node, bytes, &decorations),
                    signature: signature_of(h.node, bytes, &decorations),
                    calls: Vec::new(),
                    used_by: Vec::new(),
                }
            })
            .collect();

        // The call sites and the citations of the file, minus the
        // declaration headers themselves (`foo` in `fn foo(` is where it is
        // defined, not a use of it) and minus what is written inside a
        // decoration, an import or a namespace name.
        let quiet: Spans = decorations.union(&import_spans).copied().collect();
        (out.calls, out.cites) = use_sites(root, bytes, &quiet, &names_at);

        out.imports.sort();
        out.imports.dedup();
        out.global_imports.sort();
        out.global_imports.dedup();
        out.namespaces.sort();
        out.namespaces.dedup();
        out
    }
}

/// A declaration as the query gave it, before the supertypes captured
/// elsewhere in the file are attached to it.
struct Header<'t> {
    kind: String,
    name: String,
    node: Node<'t>,
    /// Where the name capture starts, which tells the header from a call.
    name_byte: usize,
    /// The last line of the body kept beside the declaration, when there is one.
    body_end: Option<usize>,
}

/// The byte spans (start, end) of the decorations of a file.
type Spans = BTreeSet<(usize, usize)>;

fn is_decoration(node: &Node, decorations: &Spans) -> bool {
    decorations.contains(&(node.start_byte(), node.end_byte()))
}

/// How much of a documentation comment is kept. The map is read by machine,
/// never whole by a person, but a comment is prose: the first lines say what
/// the declaration is, and the rest is detail nobody searches by.
const DOC_MAX_CHARS: usize = 400;

/// How much of a signature is kept — a header longer than this is a parameter
/// list, and the parameter list is already in it up to here.
const SIGNATURE_MAX_CHARS: usize = 200;

/// The documentation comment written right above `node`: the `extra` nodes the
/// grammar attaches immediately above it (that is what a comment is in every
/// grammar), cleaned of their markers and joined into one line. A blank line
/// between the comment and the declaration ends the block — what is detached
/// from the declaration is not its documentation. Empty when there is none.
///
/// Two things stand between a comment and its declaration without breaking the
/// block. A decoration is passed over, and so is an unnamed token (a keyword)
/// until the first comment is joined. And when the siblings end with nothing
/// joined, the declaration is wrapped in another node, so the search goes on
/// above the wrapper.
fn doc_above(node: Node, bytes: &[u8], decorations: &Spans) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut anchor = node;
    let mut top = node.start_position().row;
    'climb: loop {
        let mut cur = anchor;
        while let Some(prev) = cur.prev_sibling() {
            if prev.end_position().row + 1 < top {
                break 'climb;
            }
            if prev.is_extra() {
                let Ok(text) = prev.utf8_text(bytes) else { break 'climb };
                parts.push(clean_comment(text));
            } else if !(is_decoration(&prev, decorations) || (!prev.is_named() && parts.is_empty())) {
                break 'climb;
            }
            top = prev.start_position().row;
            cur = prev;
        }
        if !parts.is_empty() {
            break;
        }
        let Some(parent) = anchor.parent() else { break };
        top = top.min(parent.start_position().row);
        anchor = parent;
    }
    parts.reverse();
    one_line(&parts.join(" "), DOC_MAX_CHARS)
}

/// Drop the punctuation a comment is written with, line by line, and leave the
/// prose. Generic: the marker characters are the ones every comment syntax
/// draws its lines with, not a language's.
fn clean_comment(raw: &str) -> String {
    raw.lines()
        .map(|line| {
            let line = line.trim();
            let line = line.strip_suffix("*/").unwrap_or(line);
            line.trim_start_matches(['/', '*', '#', '-', ';', '!', '<', '=']).trim()
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The declaration's own header: its text up to where the body opens — the
/// first `{` or `;` with no bracket open, or the first line break with nothing
/// left open. The body itself never comes: it was measured as the worst thing
/// to keep in the map. It starts after the decorations and comments that open
/// the node: an attribute is not the header of what it adorns.
fn signature_of(node: Node, bytes: &[u8], decorations: &Spans) -> String {
    let mut start = node.start_byte();
    let mut walker = node.walk();
    for child in node.children(&mut walker) {
        if !(is_decoration(&child, decorations) || child.is_extra()) {
            start = child.start_byte();
            break;
        }
        start = child.end_byte();
    }
    let Ok(text) = std::str::from_utf8(&bytes[start..node.end_byte()]) else { return String::new() };
    let mut depth: i32 = 0;
    let mut end = text.len();
    for (i, ch) in text.char_indices() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            '{' | ';' | '\n' if depth <= 0 => {
                end = i;
                break;
            }
            _ => {}
        }
    }
    one_line(&text[..end], SIGNATURE_MAX_CHARS)
}

/// One line of text, whitespace collapsed, cut at `max` characters — on a word
/// boundary when there is one, so what is kept still reads.
fn one_line(text: &str, max: usize) -> String {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let Some((cut, _)) = joined.char_indices().nth(max) else { return joined };
    let cut = joined[..cut].rfind(' ').unwrap_or(cut);
    joined[..cut].trim_end().to_string()
}

/// Every call the file makes and every name it cites without calling.
///
/// A call is a named leaf that reads as an identifier and is followed by an
/// opening parenthesis — the one shape a call has in every language we parse.
/// A citation is the same leaf starting with a capital letter and not followed
/// by a parenthesis: a type, a constant or an enum member named in a parameter,
/// a comparison or a path. Only the capitalised names are kept: every good
/// link a citation gives comes from them, and keeping every lowercase name
/// would weigh on the map five times as much for nothing.
///
/// Read off the tree, so what is inside a comment is never a use, and a
/// keyword never is either (it is not a named node). A citation right against
/// a quote is quoted text, not a name. The name is NOT resolved here: `graph`
/// does that with the whole project in hand, which is why what is stored
/// survives a pass that reads only the files that changed. What is written
/// right before the name, in `q::name` or `q.name`, is kept as its qualifier.
///
/// Nothing inside `quiet` is a use: not a decoration (an attribute calls
/// nothing), not an import (`Modules` in `using App.Modules;` is the path of
/// the import) and not a namespace name. A declaration's own name, at
/// `names_at`, is its header.
fn use_sites(
    root: Node,
    bytes: &[u8],
    quiet: &Spans,
    names_at: &BTreeSet<usize>,
) -> (Vec<CallSite>, Vec<CallSite>) {
    let mut calls: BTreeSet<(usize, String, String)> = BTreeSet::new();
    let mut cites: BTreeSet<(usize, String, String)> = BTreeSet::new();
    let mut cursor = root.walk();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if is_decoration(&node, quiet) || node.is_extra() {
            continue;
        }
        if node.child_count() > 0 {
            for child in node.children(&mut cursor) {
                stack.push(child);
            }
            continue;
        }
        if !node.is_named() || names_at.contains(&node.start_byte()) {
            continue;
        }
        let Ok(text) = node.utf8_text(bytes) else { continue };
        if !is_identifier(text) {
            continue;
        }
        let site = (node.start_position().row + 1, text.to_string(), qualifier_before(node, bytes));
        if followed_by_open_paren(node, bytes) {
            calls.insert(site);
        } else if text.chars().next().is_some_and(char::is_uppercase) && !against_a_quote(node, bytes) {
            cites.insert(site);
        }
    }
    let sites = |found: BTreeSet<(usize, String, String)>| {
        found.into_iter().map(|(line, name, qualifier)| CallSite { name, line, qualifier }).collect()
    };
    (sites(calls), sites(cites))
}

/// The name written right before the node and joined to it by `::` or `.`:
/// `preco` in `crate::preco::total`, `model` in `model.User`. Empty when the
/// node stands alone, or when what comes before the separator is not a name
/// (`f().total`).
fn qualifier_before(node: Node, bytes: &[u8]) -> String {
    let before = bytes[..node.start_byte()].trim_ascii_end();
    let Some(before) = before.strip_suffix(b"::").or_else(|| before.strip_suffix(b".")) else {
        return String::new();
    };
    if before.ends_with(b".") {
        return String::new();
    }
    let before = before.trim_ascii_end();
    let start = before
        .iter()
        .rposition(|b| !(b.is_ascii_alphanumeric() || *b == b'_' || *b >= 0x80))
        .map_or(0, |i| i + 1);
    match std::str::from_utf8(&before[start..]) {
        Ok(q) if is_identifier(q) => q.to_string(),
        _ => String::new(),
    }
}

/// The node touches a quote on either side: it is the text of a string, not a
/// name. Generic: every language quotes its text with one of these.
fn against_a_quote(node: Node, bytes: &[u8]) -> bool {
    let quote = |b: Option<&u8>| b.is_some_and(|b| matches!(b, b'"' | b'\'' | b'`'));
    quote(node.start_byte().checked_sub(1).and_then(|i| bytes.get(i))) || quote(bytes.get(node.end_byte()))
}

/// The next character after the node, whitespace apart, opens a parameter list.
fn followed_by_open_paren(node: Node, bytes: &[u8]) -> bool {
    bytes
        .get(node.end_byte()..)
        .and_then(|rest| rest.iter().find(|b| !b.is_ascii_whitespace()))
        .is_some_and(|b| *b == b'(')
}

/// The text reads as a name: letters, digits and underscores, starting with a
/// letter or an underscore. Two characters at least, the same floor
/// [`simple_type_name`] uses.
fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && text.chars().all(|c| c.is_alphanumeric() || c == '_')
        && text.chars().count() >= 2
}

/// Split a `.scm` source into top-level patterns and keep the ones that compile
/// against this grammar. Resilience over strictness: a query referencing a node
/// a given grammar version lacks drops that one pattern, not the language.
fn compile_good_patterns(lang: &Language, src: &str, name: &str) -> Vec<String> {
    split_patterns(src)
        .into_iter()
        .filter(|p| match Query::new(lang, p) {
            Ok(_) => true,
            Err(e) => {
                eprintln!("grain: '{name}' query pattern skipped ({e}): {}", first_line(p));
                false
            }
        })
        .collect()
}

/// Break a query into its top-level S-expression patterns. A pattern runs from a
/// `(`/`[` at depth 0 up to the next one, so trailing `@captures` (e.g. the
/// `@definition.class` after a closing paren) bundle with the preceding pattern.
fn split_patterns(src: &str) -> Vec<String> {
    let s = strip_comments(src);
    let mut starts = Vec::new();
    let mut depth: i32 = 0;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' | '[' => {
                if depth == 0 {
                    starts.push(i);
                }
                depth += 1;
            }
            ')' | ']' => depth -= 1,
            _ => {}
        }
    }
    let mut out = Vec::new();
    for k in 0..starts.len() {
        let end = if k + 1 < starts.len() { starts[k + 1] } else { s.len() };
        let pat = s[starts[k]..end].trim().to_string();
        if !pat.is_empty() {
            out.push(pat);
        }
    }
    out
}

/// Drop `;`-to-end-of-line comments. The only string literals our queries hold
/// are the patterns of a `#match?`, which never carry a `;`, so a plain scan is
/// safe.
fn strip_comments(src: &str) -> String {
    src.lines()
        .map(|l| match l.find(';') {
            Some(i) => &l[..i],
            None => l,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or(s).trim()
}

/// Reduce a captured import/using statement to its path, dropping leading
/// keywords, quotes, and punctuation so it matches across languages.
fn clean_import(txt: &str) -> String {
    let mut s = txt.trim().to_string();
    for kw in ["global ", "using static ", "using ", "import ", "from "] {
        if let Some(rest) = s.strip_prefix(kw) {
            s = rest.trim().to_string();
        }
    }
    s.replace(['"', '\'', ';', '`'], "").trim().to_string()
}

/// `"A.B.EntityBase"` -> `"EntityBase"`; `"IServiceBase<A,B>"` ->
/// `"IServiceBase"`; `"std::fmt::Display"` -> `"Display"`. Returns `None` for
/// anything that reduces to fewer than two identifier characters (e.g. a bare
/// `<T>` generic-argument list captured by a wildcard).
fn simple_type_name(txt: &str) -> Option<String> {
    // Cut generic arguments / call parens first.
    let head = txt.split(['<', '(']).next().unwrap_or(txt).trim();
    // Take the last qualified segment.
    let tail = head.rsplit(['.', ' ', ':']).next().unwrap_or(head).trim();
    let name: String = tail.chars().filter(|c| c.is_alphanumeric() || *c == '_').collect();
    if name.len() >= 2 {
        Some(name)
    } else {
        None
    }
}
