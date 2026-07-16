//! Layer 2 — Syntactic extraction, language-agnostic.
//!
//! One generic tree-sitter engine drives every language. A language is defined
//! entirely by DATA: a row in `languages.toml` (name, extensions, grammar) and a
//! set of `.scm` query files under `queries/<dir>/`. This module never names a
//! language, an extension, or a grammar node — it only understands a small,
//! generic capture vocabulary that every query speaks:
//!
//!   `@import`            -> [`Extracted::imports`]   (text is cleaned to a path)
//!   `@namespace`         -> [`Extracted::namespaces`]
//!   `@definition.<kind>` -> a [`Decl`] whose `kind` is the suffix, verbatim
//!   `@name`              -> the name of the enclosing `@definition.*`
//!   `@supertype`         -> a base type/interface/trait, attached to the decl
//!                           that shares the same `@name`. Because the link is by
//!                           name, a base captured in a *detached* node — a Rust
//!                           `impl Trait for T` block — still lands on `T`.
//!
//! The per-language seam the old design called for is preserved: there is one
//! [`Analyzer`] instance per language, but all are the same generic type, each
//! parameterized by a compiled query. Precise AST facts go in; the same generic
//! `Extracted`/`Decl` come out, so the miner (Layer 4) never learns any syntax.
//!
//! `build.rs` embeds the registry and the query files into `OUT_DIR`; we include
//! the generated table here. Nothing language-specific lives in this file.

use crate::model::Decl;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Parser, Query, QueryCursor};

#[derive(Default)]
pub(crate) struct Extracted {
    pub imports: Vec<String>,
    pub namespaces: Vec<String>,
    pub declarations: Vec<Decl>,
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
    Namespace,
    Name,
    Supertype,
    Def(String),
    Ignore,
}

fn classify(cap: &str) -> CapKind {
    match cap {
        "import" => CapKind::Import,
        "namespace" => CapKind::Namespace,
        "name" => CapKind::Name,
        "supertype" => CapKind::Supertype,
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
        let mut decls: BTreeMap<usize, (String, String, usize)> = BTreeMap::new();
        let mut supers_by_name: HashMap<String, BTreeSet<String>> = HashMap::new();

        let mut matches = cursor.matches(&self.query, root, bytes);
        while let Some(m) = matches.next() {
            let mut def: Option<(usize, &str, usize)> = None;
            let mut name_text: Option<String> = None;
            let mut here_supers: Vec<String> = Vec::new();

            for cap in m.captures {
                let node = cap.node;
                match &self.cap_kinds[cap.index as usize] {
                    CapKind::Import => {
                        if let Ok(t) = node.utf8_text(bytes) {
                            let c = clean_import(t);
                            if !c.is_empty() {
                                out.imports.push(c);
                            }
                        }
                    }
                    CapKind::Namespace => {
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
                        }
                    }
                    CapKind::Supertype => {
                        if let Ok(t) = node.utf8_text(bytes) {
                            if let Some(n) = simple_type_name(t) {
                                here_supers.push(n);
                            }
                        }
                    }
                    CapKind::Def(kind) => {
                        def = Some((node.start_byte(), kind.as_str(), node.start_position().row + 1));
                    }
                    CapKind::Ignore => {}
                }
            }

            if let (Some((sb, kind, line)), Some(name)) = (def, &name_text) {
                decls.entry(sb).or_insert_with(|| (kind.to_string(), name.clone(), line));
            }
            if let Some(name) = &name_text {
                if !here_supers.is_empty() {
                    let key = simple_type_name(name).unwrap_or_else(|| name.clone());
                    let bucket = supers_by_name.entry(key).or_default();
                    for s in here_supers {
                        bucket.insert(s);
                    }
                }
            }
        }

        out.declarations = decls
            .into_values()
            .map(|(kind, name, line)| {
                let key = simple_type_name(&name).unwrap_or_else(|| name.clone());
                let supertypes = supers_by_name
                    .get(&key)
                    .map(|s| s.iter().cloned().collect())
                    .unwrap_or_default();
                Decl { kind, name, line, supertypes }
            })
            .collect();

        out.imports.sort();
        out.imports.dedup();
        out.namespaces.sort();
        out.namespaces.dedup();
        out
    }
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

/// Drop `;`-to-end-of-line comments. Our queries contain no string literals, so
/// a plain scan is safe.
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
