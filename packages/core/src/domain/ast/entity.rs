//! `extract_entities` — pull named entity declarations out of a source blob.
//!
//! An *entity* is a named type-level declaration: a `struct`, `enum`,
//! `trait`/`interface`, `class`, `record`, `type` alias, or `def`-style
//! class. This is the structural sibling of
//! [`super::extract_function_signatures`]: where that extractor finds
//! *functions*, this one finds *types*.
//!
//! Two paths, exactly like the rest of `ast::*`:
//!
//! 1. **AST path** (precise). When the loader resolves `lang_id` (built-in OR
//!    externally discovered) AND the `entity_definitions` query resolves
//!    (built-in base or on-disk override — see [`super::QuerySet`]), run the
//!    query against the parsed tree and emit one [`ExtractedEntity`] per
//!    `@name` capture. `refs` is populated from the `import_edges` query when
//!    it resolves, otherwise left empty. Mode is [`DetectionMode::Ast`].
//!
//! 2. **Textual floor** (heuristic, agnostic). When the grammar is missing OR
//!    the `entity_definitions` query does not resolve, scan the source line by
//!    line with a [`VocabularyMatcher`] over a fixed set of *universal*
//!    declaration keywords (`struct`, `class`, `interface`, `enum`, `record`,
//!    `trait`, `type`, `def`, `pub struct`, `export class`, ...). The keyword
//!    that fires becomes `kind`; the next identifier on the line becomes
//!    `name`. The scan **never branches on `lang_id`** — the same keyword set
//!    runs for every input. Mode is [`DetectionMode::Textual`].
//!
//! The floor is the only heuristic piece here and, like the signature
//! fallback, it is agnostic by construction: false positives are tolerated
//! because downstream consumers reconcile against declared entities before
//! acting.

use super::{DetectionMode, GrammarLoader, QuerySet, TreeSitterParser};
use crate::domain::vocabulary::{Layer, VocabLayer, VocabularyMatcher};
use tree_sitter::{Query, QueryCursor, StreamingIterator};

/// One named entity declaration extracted from a source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedEntity {
    /// Final identifier of the declared type (e.g. `User`, `Order`).
    pub name: String,
    /// Syntactic kind that produced the entity. In [`DetectionMode::Ast`] this
    /// is the tree-sitter node kind (e.g. `struct_item`, `class_declaration`);
    /// in [`DetectionMode::Textual`] it is the keyword that fired (e.g.
    /// `pub struct`, `export class`, `def`).
    pub kind: String,
    /// Imported modules / paths referenced by the source. Populated from the
    /// `import_edges` query in [`DetectionMode::Ast`]; empty in the textual
    /// floor (the floor does not parse imports). The same `refs` list is
    /// attached to every entity emitted from one source — imports are a
    /// file-level fact, not a per-entity one.
    pub refs: Vec<String>,
    /// 1-indexed line number where the declaration starts.
    pub line: usize,
    /// Whether the entity came from the AST path or the textual floor.
    pub mode: DetectionMode,
}

/// Universal entity-declaration keywords for the textual floor.
///
/// **Agnostic by construction** — no `lang_id` appears anywhere. The list is
/// ordered longest-first so multi-word forms win over single-word ones
/// (`pub struct` before `struct`, `export class` before `class`). The keyword
/// that matches at the start of a logical line becomes the entity `kind`; the
/// next identifier on that line becomes `name`.
const FLOOR_KEYWORDS: &[&str] = &[
    // Multi-word public/export forms (TS/JS, Java/C#).
    "export default class",
    "export default interface",
    "export abstract class",
    "export class",
    "export interface",
    "export enum",
    "export type",
    "public abstract class",
    "public static class",
    "public sealed class",
    "public class",
    "public interface",
    "public enum",
    "public struct",
    "public record",
    // Multi-word Rust forms.
    "pub struct",
    "pub enum",
    "pub trait",
    "pub union",
    "pub type",
    // Single-word universal forms.
    "interface",
    "struct",
    "trait",
    "record",
    "class",
    "enum",
    "union",
    "type",
    "def",
];

/// Extract named entity declarations from `source`.
///
/// Prefers the AST path (grammar resolved by `loader` AND an
/// `entity_definitions` query resolves). Falls back to the agnostic textual
/// floor otherwise. Never panics; returns an empty vector when nothing can
/// be extracted.
#[must_use]
pub fn extract_entities(
    loader: &GrammarLoader,
    source: &str,
    lang_id: &str,
) -> Vec<ExtractedEntity> {
    // AST path: language + entity_definitions query present.
    if let Some(language) = loader.language(lang_id) {
        let set = QuerySet::load_for(lang_id, loader.project_root(), Some(&language));
        if let Some(query) = set.entity_definitions() {
            if let Ok(mut parser) = TreeSitterParser::for_language(loader, lang_id) {
                if let Ok(tree) = parser.parse(source) {
                    let ts_tree = tree.as_tree_sitter();
                    // Imports are a file-level fact attached to every entity.
                    let refs = set
                        .import_edges()
                        .map(|q| extract_imports(q, ts_tree, source))
                        .unwrap_or_default();
                    return extract_entities_via_query(query, ts_tree, source, &refs);
                }
            }
        }
    }
    extract_entities_via_floor(source)
}

/// Run the `entity_definitions` query over `tree` and emit one
/// [`ExtractedEntity`] per `@name` capture.
///
/// Query conventions (see `queries_builtin/{lang}/entity_definitions.scm`):
///
/// - `@name` — the type identifier node (required).
/// - `@kind` — the enclosing declaration node (optional). When present its
///   tree-sitter node kind populates [`ExtractedEntity::kind`]; otherwise the
///   `@name` node kind is used as a last resort.
fn extract_entities_via_query(
    query: &Query,
    tree: &tree_sitter::Tree,
    source: &str,
    refs: &[String],
) -> Vec<ExtractedEntity> {
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), source.as_bytes());
    let capture_names = query.capture_names();

    let mut out: Vec<ExtractedEntity> = Vec::new();

    while let Some(m) = matches.next() {
        let mut name: Option<(String, usize)> = None;
        let mut kind: Option<String> = None;

        for cap in m.captures {
            let cap_name = capture_names.get(cap.index as usize).copied().unwrap_or("");
            let node = cap.node;
            match cap_name {
                "name" => {
                    let text = source.get(node.start_byte()..node.end_byte()).unwrap_or("");
                    // Tree-sitter rows are 0-indexed; surface 1-indexed lines.
                    name = Some((text.to_string(), node.start_position().row + 1));
                }
                "kind" => kind = Some(node.kind().to_string()),
                _ => {}
            }
        }

        if let Some((name, line)) = name {
            // `@kind` anchors the declaration node; fall back to a generic
            // label when the query omitted it.
            let kind = kind.unwrap_or_else(|| "entity".to_string());
            out.push(ExtractedEntity {
                name,
                kind,
                refs: refs.to_vec(),
                line,
                mode: DetectionMode::Ast,
            });
        }
    }

    out
}

/// Run the `import_edges` query over `tree` and collect the captured import
/// text, deduplicated and in first-seen order.
fn extract_imports(query: &Query, tree: &tree_sitter::Tree, source: &str) -> Vec<String> {
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), source.as_bytes());
    let capture_names = query.capture_names();

    let mut out: Vec<String> = Vec::new();
    while let Some(m) = matches.next() {
        for cap in m.captures {
            let cap_name = capture_names.get(cap.index as usize).copied().unwrap_or("");
            if cap_name != "import" {
                continue;
            }
            let node = cap.node;
            let text = source.get(node.start_byte()..node.end_byte()).unwrap_or("");
            let text = text.trim();
            if !text.is_empty() && !out.iter().any(|e| e == text) {
                out.push(text.to_string());
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Textual floor
// ---------------------------------------------------------------------------

/// Agnostic textual floor. Scans `source` line by line with a
/// [`VocabularyMatcher`] over [`FLOOR_KEYWORDS`] and emits an entity per line
/// whose leading keyword fires. Never branches on a language id.
fn extract_entities_via_floor(source: &str) -> Vec<ExtractedEntity> {
    let Some(matcher) = floor_matcher() else {
        return Vec::new();
    };

    let mut out: Vec<ExtractedEntity> = Vec::new();
    for (idx, raw_line) in source.lines().enumerate() {
        let line = strip_comment(raw_line);
        if line.is_empty() {
            continue;
        }
        // Scan the line; the leftmost-first automaton prefers the longest /
        // most-severe term registered first (we register longest-first), so
        // the keyword that fires is the best structural match.
        let Some(hit) = matcher.scan(line).into_iter().next() else {
            continue;
        };
        // The keyword must anchor the declaration: it has to sit at the start
        // of the (whitespace-stripped) line, not buried mid-expression. This
        // keeps `let x: Type` or `return new Class()` from registering.
        let stripped = line.trim_start();
        let leading_ws = line.len() - stripped.len();
        if hit.start != leading_ws {
            continue;
        }
        let keyword = &line[hit.start..hit.end];
        let rest = &line[hit.end..];
        let name = parse_name(rest);
        if name.is_empty() {
            continue;
        }
        out.push(ExtractedEntity {
            name,
            kind: keyword.to_string(),
            refs: Vec::new(),
            line: idx + 1,
            mode: DetectionMode::Textual,
        });
    }
    out
}

/// One-shot build of the floor [`VocabularyMatcher`] guarded by a `OnceLock`.
/// Returns `None` only if the matcher fails to build, which cannot happen for
/// the static non-empty [`FLOOR_KEYWORDS`] list (kept as `Option` so the
/// floor degrades to "no entities" rather than panicking).
fn floor_matcher() -> Option<&'static VocabularyMatcher> {
    use std::sync::OnceLock;
    static CELL: OnceLock<Option<VocabularyMatcher>> = OnceLock::new();
    CELL.get_or_init(|| {
        let layer = VocabLayer {
            kind: Layer::Keyword,
            terms: FLOOR_KEYWORDS.iter().map(|s| (*s).to_string()).collect(),
        };
        VocabularyMatcher::from_layers(vec![layer]).ok()
    })
    .as_ref()
}

/// Strip a single-line comment from `line`, returning `""` for comment-only
/// lines. Mirrors the agnostic comment handling used by the apps-layer
/// entity extractor so the floor does not register keywords inside comments.
fn strip_comment(line: &str) -> &str {
    const COMMENT_PREFIXES: &[&str] = &["//", "#", "--", ";", "%"];
    let trimmed = line.trim_start();
    for prefix in COMMENT_PREFIXES {
        if trimmed.starts_with(prefix) {
            return "";
        }
    }
    if let Some(idx) = trimmed.find("//") {
        return trimmed[..idx].trim_end();
    }
    trimmed
}

/// Pull the next identifier after a keyword tail. Skips whitespace and the
/// common decoration prefixes (`*`, `&`) then collects ASCII-alphanumeric +
/// `_`. Returns `""` when no identifier can be parsed.
fn parse_name(rest: &str) -> String {
    let s = rest
        .trim_start()
        .trim_start_matches('*')
        .trim_start_matches('&')
        .trim_start();
    let mut out = String::new();
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn builtin_loader() -> (tempfile::TempDir, GrammarLoader) {
        let tmp = tempfile::tempdir().expect("temp dir");
        let loader = GrammarLoader::with_builtins(tmp.path());
        (tmp, loader)
    }

    fn names(entities: &[ExtractedEntity]) -> Vec<String> {
        entities.iter().map(|e| e.name.clone()).collect()
    }

    // -----------------------------------------------------------------------
    // AST path — one fixture per built-in language.
    // -----------------------------------------------------------------------

    #[test]
    fn ast_rust_struct() {
        let (_tmp, loader) = builtin_loader();
        let ents = extract_entities(&loader, "pub struct User { id: i32 }", "rust");
        assert!(
            ents.iter().any(|e| e.name == "User" && e.mode == DetectionMode::Ast),
            "expected AST entity `User`, got {ents:?}"
        );
    }

    #[test]
    fn ast_csharp_class() {
        let (_tmp, loader) = builtin_loader();
        let ents = extract_entities(&loader, "public class Order {}", "c-sharp");
        assert!(
            ents.iter().any(|e| e.name == "Order" && e.mode == DetectionMode::Ast),
            "expected AST entity `Order`, got {ents:?}"
        );
    }

    #[test]
    fn ast_typescript_interface() {
        let (_tmp, loader) = builtin_loader();
        let ents = extract_entities(&loader, "export interface Foo {}", "typescript");
        assert!(
            ents.iter().any(|e| e.name == "Foo" && e.mode == DetectionMode::Ast),
            "expected AST entity `Foo`, got {ents:?}"
        );
    }

    #[test]
    fn ast_python_class() {
        let (_tmp, loader) = builtin_loader();
        let ents = extract_entities(&loader, "class Bar:\n    pass\n", "python");
        assert!(
            ents.iter().any(|e| e.name == "Bar" && e.mode == DetectionMode::Ast),
            "expected AST entity `Bar`, got {ents:?}"
        );
    }

    #[test]
    fn ast_go_struct() {
        let (_tmp, loader) = builtin_loader();
        let ents = extract_entities(&loader, "package x\ntype Baz struct {}\n", "go");
        assert!(
            ents.iter().any(|e| e.name == "Baz" && e.mode == DetectionMode::Ast),
            "expected AST entity `Baz`, got {ents:?}"
        );
    }

    #[test]
    fn ast_java_class() {
        let (_tmp, loader) = builtin_loader();
        let ents = extract_entities(&loader, "class Qux {}", "java");
        assert!(
            ents.iter().any(|e| e.name == "Qux" && e.mode == DetectionMode::Ast),
            "expected AST entity `Qux`, got {ents:?}"
        );
    }

    #[test]
    fn ast_tsx_class() {
        let (_tmp, loader) = builtin_loader();
        let ents = extract_entities(&loader, "export class Widget {}", "tsx");
        assert!(
            ents.iter().any(|e| e.name == "Widget" && e.mode == DetectionMode::Ast),
            "expected AST entity `Widget`, got {ents:?}"
        );
    }

    #[test]
    fn ast_kind_is_node_type_not_keyword() {
        let (_tmp, loader) = builtin_loader();
        let ents = extract_entities(&loader, "pub struct User {}", "rust");
        let user = ents.iter().find(|e| e.name == "User").expect("User");
        assert_eq!(user.kind, "struct_item", "AST kind is the node type");
    }

    // -----------------------------------------------------------------------
    // import_edges — Ast mode captures something non-empty.
    // -----------------------------------------------------------------------

    #[test]
    fn ast_rust_import_edges_non_empty() {
        let (_tmp, loader) = builtin_loader();
        let src = "use crate::foo::Bar;\npub struct Local {}\n";
        let ents = extract_entities(&loader, src, "rust");
        let local = ents
            .iter()
            .find(|e| e.name == "Local")
            .expect("Local entity present");
        assert!(
            !local.refs.is_empty(),
            "expected non-empty refs from import_edges, got {:?}",
            local.refs
        );
        assert!(
            local.refs.iter().any(|r| r.contains("foo")),
            "expected import to reference `foo`, got {:?}",
            local.refs
        );
    }

    // -----------------------------------------------------------------------
    // Textual floor — empty loader still finds the name via keyword scan.
    // -----------------------------------------------------------------------

    #[test]
    fn floor_rust_struct_without_grammar() {
        let tmp = tempfile::tempdir().unwrap();
        let loader = GrammarLoader::empty(tmp.path());
        let ents = extract_entities(&loader, "pub struct User { id: i32 }", "rust");
        assert!(
            ents.iter()
                .any(|e| e.name == "User" && e.mode == DetectionMode::Textual),
            "expected textual entity `User`, got {ents:?}"
        );
    }

    #[test]
    fn floor_python_class_without_grammar() {
        let tmp = tempfile::tempdir().unwrap();
        let loader = GrammarLoader::empty(tmp.path());
        let ents = extract_entities(&loader, "class Bar:\n    pass\n", "python");
        assert!(
            ents.iter()
                .any(|e| e.name == "Bar" && e.mode == DetectionMode::Textual),
            "expected textual entity `Bar`, got {ents:?}"
        );
    }

    #[test]
    fn floor_is_agnostic_runs_for_unknown_lang_id() {
        let tmp = tempfile::tempdir().unwrap();
        let loader = GrammarLoader::empty(tmp.path());
        // A made-up lang id must still flow through the floor.
        let ents = extract_entities(&loader, "interface Schema {}", "totally-made-up");
        assert!(names(&ents).contains(&"Schema".to_string()));
    }

    #[test]
    fn floor_skips_comment_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let loader = GrammarLoader::empty(tmp.path());
        let src = "// pub struct NotReal\npub struct Real {}\n";
        let ents = extract_entities(&loader, src, "rust");
        assert!(names(&ents).contains(&"Real".to_string()));
        assert!(!names(&ents).contains(&"NotReal".to_string()));
    }

    #[test]
    fn floor_empty_source_yields_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let loader = GrammarLoader::empty(tmp.path());
        assert!(extract_entities(&loader, "", "rust").is_empty());
    }

    #[test]
    fn floor_line_numbers_are_one_indexed() {
        let tmp = tempfile::tempdir().unwrap();
        let loader = GrammarLoader::empty(tmp.path());
        let ents = extract_entities(&loader, "\n\nclass First:\n    pass\n", "python");
        let first = ents.iter().find(|e| e.name == "First").expect("First");
        assert_eq!(first.line, 3);
    }

    #[test]
    fn parse_name_handles_decoration() {
        assert_eq!(parse_name(" User { }"), "User");
        assert_eq!(parse_name(" *Ptr"), "Ptr");
        assert_eq!(parse_name("   "), "");
    }

    #[test]
    fn ast_path_used_when_grammar_present_not_floor() {
        // Sanity: the same source through with_builtins is Ast, through empty
        // is Textual — proving the path selection works off grammar presence,
        // not lang_id.
        let tmp = tempfile::tempdir().unwrap();
        let with = GrammarLoader::with_builtins(tmp.path());
        let without = GrammarLoader::empty(tmp.path());
        let src = "pub struct User {}";
        let a = extract_entities(&with, src, "rust");
        let b = extract_entities(&without, src, "rust");
        assert!(a.iter().any(|e| e.name == "User" && e.mode == DetectionMode::Ast));
        assert!(b.iter().any(|e| e.name == "User" && e.mode == DetectionMode::Textual));
    }
}
