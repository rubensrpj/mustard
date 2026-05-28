//! Language-agnostic AST-light entity declaration extractor.
//!
//! Recognises *syntax*, not frameworks. Given a single source file and its
//! text, returns a list of [`ExtractedDecl`] entries — public/exported
//! declarations detected through generic per-line keyword scanning.
//!
//! The keyword set spans the languages Mustard sees in the wild:
//!
//! | Keyword(s) | Family |
//! |---|---|
//! | `pub` | Rust |
//! | `export`, `export default`, `export function`, `export class`, `export const`, `export type`, `export interface` | TS / JS |
//! | `public`, `public class`, `public static`, `public interface` | Java / C# |
//! | `def` | Python / Ruby |
//! | `class` | universal |
//! | `function`, `fn`, `func` | TS/JS / Rust / Go|Swift |
//! | `type`, `interface`, `struct`, `enum`, `trait` | typed langs |
//!
//! No framework awareness — `pub struct`, `export class`, and `def` are all
//! treated as "a public declaration exists at line N". Strings and comments
//! are stripped per line via simple prefix checks to avoid false matches.
//!
//! Fail-open: malformed UTF-8 in line content cannot occur here (input is
//! `&str`); empty source yields an empty vector. The extractor never panics.

use std::path::{Path, PathBuf};

/// One detected declaration line.
///
/// `kind` carries the syntactic token that triggered the match (e.g. `pub
/// struct`, `export function`, `class`, `def`). `name` is the next
/// identifier after the keyword, when one could be parsed. `file` is the
/// path passed by the caller (kept verbatim — relative or absolute).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedDecl {
    /// The syntactic kind token (e.g. `pub fn`, `export class`, `def`).
    pub kind: String,
    /// The declared symbol name, or empty string when not parseable.
    pub name: String,
    /// The source file path (verbatim from the caller).
    pub file: PathBuf,
    /// 1-indexed line number where the declaration starts.
    pub line: u32,
}

/// Universal single-line comment markers stripped before keyword scan.
const COMMENT_PREFIXES: &[&str] = &["//", "#", "--", ";", "%"];

/// Keyword tuples sorted longest-first so multi-word forms win over
/// single-word ones (`export class` before `class`, `pub fn` before `fn`).
const KEYWORDS: &[&str] = &[
    // multi-word public/export forms (TS/JS, Java/C#)
    "export default class",
    "export default function",
    "export default interface",
    "export default async function",
    "export async function",
    "export function",
    "export class",
    "export interface",
    "export const",
    "export type",
    "export enum",
    "export default",
    "public static class",
    "public static interface",
    "public class",
    "public interface",
    "public static",
    "public abstract class",
    // multi-word Rust forms
    "pub struct",
    "pub enum",
    "pub trait",
    "pub fn",
    "pub type",
    "pub const",
    // single-word universal
    "interface",
    "struct",
    "trait",
    "class",
    "enum",
    "type",
    "export",
    "public",
    "function",
    "func",
    "def",
    "fn",
    "pub",
];

/// Strip a single-line comment and surrounding whitespace from `line`.
///
/// Block comments (`/* … */`) are not stripped — they almost never carry
/// declaration keywords mid-comment, and the extra parsing cost is not
/// worth the rare false positive.
fn strip_comment(line: &str) -> &str {
    let trimmed = line.trim_start();
    for prefix in COMMENT_PREFIXES {
        if trimmed.starts_with(prefix) {
            return "";
        }
    }
    // Inline `//` comments — chop at first occurrence outside obvious string.
    if let Some(idx) = trimmed.find("//") {
        return trimmed[..idx].trim_end();
    }
    trimmed
}

/// Pull the next identifier token after a keyword tail.
///
/// Walks `rest` skipping whitespace and common punctuation (`(`, `<`, `:`)
/// then collects ASCII-alphanumeric + `_` characters as the identifier.
/// Returns `""` when no identifier can be parsed (e.g. anonymous default
/// export, `pub use`, generics-only lines).
fn parse_name(rest: &str) -> String {
    let s = rest.trim_start();
    // Allow common type-decoration prefixes that precede the name.
    let s = s
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

/// Returns `true` when `line` matches `keyword` as a whole-word prefix
/// (the next byte after the keyword is whitespace, EOL, or punctuation).
fn starts_with_keyword(line: &str, keyword: &str) -> bool {
    if !line.starts_with(keyword) {
        return false;
    }
    match line.as_bytes().get(keyword.len()) {
        None => true,
        Some(b) => !(b.is_ascii_alphanumeric() || *b == b'_'),
    }
}

/// Extract every recognisable declaration line from `source`.
///
/// The caller owns the file path; `extract_decls` does not touch the
/// filesystem and never reads from disk. Strings and block comments are
/// not parsed — a literal `"export class X"` inside a string would
/// produce a false positive, accepted as a documented trade-off (the
/// downstream consumers only need decl *counts*, not perfect AST fidelity).
#[must_use]
pub fn extract_decls(file: &Path, source: &str) -> Vec<ExtractedDecl> {
    let mut out: Vec<ExtractedDecl> = Vec::new();
    for (idx, raw_line) in source.lines().enumerate() {
        let line = strip_comment(raw_line);
        if line.is_empty() {
            continue;
        }
        for keyword in KEYWORDS {
            if starts_with_keyword(line, keyword) {
                let rest = &line[keyword.len()..];
                let name = parse_name(rest);
                out.push(ExtractedDecl {
                    kind: (*keyword).to_string(),
                    name,
                    file: file.to_path_buf(),
                    // `lines()` is 0-indexed; surface 1-indexed numbers.
                    #[allow(clippy::cast_possible_truncation)]
                    line: (idx as u32) + 1,
                });
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn names(decls: &[ExtractedDecl]) -> Vec<String> {
        decls.iter().map(|d| d.name.clone()).collect()
    }

    #[test]
    fn rust_pub_decls() {
        let src = "pub struct User { pub id: i32 }\n\
                   pub enum Status { Active, Pending }\n\
                   pub fn run() {}\n\
                   pub trait Service {}\n\
                   fn private_helper() {}\n";
        let decls = extract_decls(&PathBuf::from("u.rs"), src);
        let kinds: Vec<&str> = decls.iter().map(|d| d.kind.as_str()).collect();
        assert!(kinds.contains(&"pub struct"));
        assert!(kinds.contains(&"pub enum"));
        assert!(kinds.contains(&"pub fn"));
        assert!(kinds.contains(&"pub trait"));
        // Private `fn` still matched — `fn` is a keyword in the universal set.
        assert!(kinds.contains(&"fn"));
        assert!(names(&decls).contains(&"User".to_string()));
        assert!(names(&decls).contains(&"Status".to_string()));
    }

    #[test]
    fn typescript_exports() {
        let src = "export class Widget {}\n\
                   export function compute(x) {}\n\
                   export const TOKENS = [];\n\
                   export interface Schema {}\n\
                   export type Id = string;\n\
                   export default class Defaulted {}\n";
        let decls = extract_decls(&PathBuf::from("w.ts"), src);
        let kinds: Vec<&str> = decls.iter().map(|d| d.kind.as_str()).collect();
        assert!(kinds.contains(&"export class"));
        assert!(kinds.contains(&"export function"));
        assert!(kinds.contains(&"export const"));
        assert!(kinds.contains(&"export interface"));
        assert!(kinds.contains(&"export type"));
        assert!(kinds.contains(&"export default class"));
        assert!(names(&decls).contains(&"Widget".to_string()));
        assert!(names(&decls).contains(&"compute".to_string()));
    }

    #[test]
    fn java_csharp_public() {
        let src = "public class Account {}\n\
                   public interface Repo {}\n\
                   public static class Helpers {}\n";
        let decls = extract_decls(&PathBuf::from("A.java"), src);
        let kinds: Vec<&str> = decls.iter().map(|d| d.kind.as_str()).collect();
        assert!(kinds.contains(&"public class"));
        assert!(kinds.contains(&"public interface"));
        assert!(kinds.contains(&"public static class"));
        assert!(names(&decls).contains(&"Account".to_string()));
    }

    #[test]
    fn python_ruby_def_class() {
        let src = "class User:\n\
                   \x20   pass\n\
                   def run():\n\
                   \x20   return 1\n";
        let decls = extract_decls(&PathBuf::from("u.py"), src);
        let kinds: Vec<&str> = decls.iter().map(|d| d.kind.as_str()).collect();
        assert!(kinds.contains(&"class"));
        assert!(kinds.contains(&"def"));
        assert!(names(&decls).contains(&"User".to_string()));
        assert!(names(&decls).contains(&"run".to_string()));
    }

    #[test]
    fn go_swift_func() {
        let src = "func Run() {}\n\
                   type Account struct {}\n";
        let decls = extract_decls(&PathBuf::from("a.go"), src);
        let kinds: Vec<&str> = decls.iter().map(|d| d.kind.as_str()).collect();
        assert!(kinds.contains(&"func"));
        assert!(kinds.contains(&"type"));
    }

    #[test]
    fn comments_are_stripped() {
        let src = "// pub struct NotADecl\n\
                   # def not_python\n\
                   pub struct Real {}\n";
        let decls = extract_decls(&PathBuf::from("c.rs"), src);
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].name, "Real");
        assert_eq!(decls[0].line, 3);
    }

    #[test]
    fn empty_source_yields_empty() {
        let decls = extract_decls(&PathBuf::from("e.rs"), "");
        assert!(decls.is_empty());
    }

    #[test]
    fn multi_word_keyword_wins_over_single() {
        // `export class X` must match `export class`, not just `export` or `class`.
        let decls = extract_decls(
            &PathBuf::from("m.ts"),
            "export class Widget {}\npub struct User {}\n",
        );
        assert_eq!(decls.len(), 2);
        assert_eq!(decls[0].kind, "export class");
        assert_eq!(decls[1].kind, "pub struct");
    }

    #[test]
    fn line_numbers_are_one_indexed() {
        let src = "\n\npub struct First {}\n";
        let decls = extract_decls(&PathBuf::from("l.rs"), src);
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].line, 3);
    }
}
