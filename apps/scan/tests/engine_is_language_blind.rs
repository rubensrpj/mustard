//! Ratchet: the engine under `src/` never spells a language.
//!
//! `languages.toml` opens with the claim that "grain's Rust source contains no
//! language name, extension, or grammar node name", and four test files repeat
//! it in their own headers. Until this ratchet existed, ALL of that was prose
//! asserting itself: nothing read `src/` and checked. It drifted exactly as you
//! would expect — seven doc comments naming five different languages, none of
//! which any test could see, and each one teaching the next reader that a
//! per-language special case is normal here.
//!
//! The forbidden vocabulary is DERIVED, never curated: it is every `name` and
//! every `dir` the registry itself declares. Adding a language to
//! `languages.toml` therefore widens this check automatically — the one place a
//! language is declared stays the one place, and this test cannot fall behind
//! it.
//!
//! ## What is deliberately NOT checked, and why
//!
//! * **Terms shorter than three characters.** A two-letter registry id is not
//!   distinctive enough to match on: it collides with ordinary English in prose
//!   and with identifier fragments in code, and a check that cries wolf gets
//!   deleted. Length is a property of the term, not a hand-picked exception —
//!   no name is ever listed here to be forgiven.
//! * **`#[cfg(test)]` modules.** Test data legitimately carries realistic file
//!   names, because a fixture that avoided them would stop resembling the input
//!   it stands for. The ENGINE must be blind; its fixtures must not be. The cut
//!   is the attribute itself, which the compiler already treats as the boundary
//!   between the two.
//!
//! Comments count. The seven violations this ratchet was born from were ALL in
//! doc comments and none in executable code — the engine behaved agnostically
//! and only its prose leaked. Prose is what the next author reads.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Shortest registry id this ratchet will match on. See the module doc: below
/// this, a term is not distinctive enough to separate a language name from
/// ordinary prose, and the check would produce noise instead of signal.
const MIN_TERM_LEN: usize = 3;

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every language id the registry declares — its `name` and its queries `dir`,
/// deduplicated and lowercased. This is the ONLY source of the vocabulary; the
/// test never spells a language itself.
fn declared_language_terms() -> BTreeSet<String> {
    let raw = std::fs::read_to_string(crate_dir().join("languages.toml")).expect("read languages.toml");
    let registry: toml::Value = toml::from_str(&raw).expect("languages.toml is valid TOML");
    let entries = registry
        .get("language")
        .and_then(|v| v.as_array())
        .expect("languages.toml declares [[language]] entries");

    let mut terms = BTreeSet::new();
    for entry in entries {
        for key in ["name", "dir"] {
            if let Some(v) = entry.get(key).and_then(|v| v.as_str())
                && v.len() >= MIN_TERM_LEN {
                    terms.insert(v.to_ascii_lowercase());
                }
        }
    }
    assert!(!terms.is_empty(), "the registry must declare at least one usable language id");
    terms
}

/// Every `.rs` file under `src/`, recursively.
fn engine_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            engine_sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Whether `line` contains `term` as a whole word, case-insensitively. Word
/// boundaries keep an id from matching inside a longer identifier — the reason
/// a method named `rsplit` is not a report about a language.
fn mentions_whole_word(line: &str, term: &str) -> bool {
    let haystack = line.to_ascii_lowercase();
    let bytes = haystack.as_bytes();
    let mut from = 0;
    while let Some(rel) = haystack[from..].find(term) {
        let start = from + rel;
        let end = start + term.len();
        let before_ok = start == 0 || !is_word_byte(bytes[start - 1]);
        let after_ok = end == bytes.len() || !is_word_byte(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[test]
fn no_engine_source_names_a_language_the_registry_declares() {
    let terms = declared_language_terms();
    let mut files = Vec::new();
    engine_sources(&crate_dir().join("src"), &mut files);
    files.sort();
    assert!(!files.is_empty(), "src/ must contain sources to check");

    let mut violations: Vec<String> = Vec::new();
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else { continue };
        let rel = file.strip_prefix(crate_dir()).unwrap_or(file).display().to_string().replace('\\', "/");
        for (i, line) in text.lines().enumerate() {
            // Everything from the first `#[cfg(test)]` attribute on is fixture
            // terrain — see the module doc for why the cut is here.
            if line.trim_start().starts_with("#[cfg(test)]") {
                break;
            }
            for term in &terms {
                if mentions_whole_word(line, term) {
                    violations.push(format!("{rel}:{} names `{term}` — {}", i + 1, line.trim()));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "the engine must never spell a language the registry declares — that knowledge belongs in \
         languages.toml, manifests.toml and queries/<dir>/*.scm, and this holds for comments too, \
         because prose is what the next author copies:\n  {}",
        violations.join("\n  ")
    );
}

#[test]
fn the_vocabulary_comes_from_the_registry_and_nowhere_else() {
    // A ratchet whose term list silently emptied would pass forever while
    // checking nothing. Assert it is actually loaded and plural, and that the
    // length floor is what excludes ids rather than any named exception.
    let terms = declared_language_terms();
    assert!(terms.len() >= 2, "expected several language ids, got {terms:?}");
    assert!(
        terms.iter().all(|t| t.len() >= MIN_TERM_LEN),
        "every checked term clears the length floor: {terms:?}"
    );
    assert!(
        terms.iter().all(|t| t.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')),
        "registry ids are lowercase slugs: {terms:?}"
    );
}
