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
//! every `dir` the registry itself declares, the name of every framework
//! route rule under `routes/`, and the `kind` of every build-system row of
//! `manifests.toml`. Adding a language to `languages.toml`, a framework to
//! `routes/` or a build system to `manifests.toml`, therefore widens this
//! check automatically — the one place each is declared stays the one place,
//! and this test cannot fall behind it.
//!
//! ## What is deliberately NOT checked, and why
//!
//! * **Build systems shorter than four characters** (`go`, `pub`): they are
//!   plain words of the engine's own language, and matching them reports prose.
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

#[path = "support/manifest_dir.rs"]
mod manifest_dir;
#[path = "support/model.rs"]
mod model;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Shortest registry id this ratchet will match on. See the module doc: below
/// this, a term is not distinctive enough to separate a language name from
/// ordinary prose, and the check would produce noise instead of signal.
const MIN_TERM_LEN: usize = 3;

/// Shortest build-system id the ratchet matches on; see
/// `declared_build_system_terms`.
const MIN_BUILD_SYSTEM_LEN: usize = 4;

fn crate_dir() -> PathBuf {
    manifest_dir::manifest_dir()
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

/// Every framework the route rules declare — the name of each
/// `routes/<framework>.toml`, lowercased. Like the languages, a framework is
/// data: the engine that joins the routes never spells one.
fn declared_framework_terms() -> BTreeSet<String> {
    let entries = std::fs::read_dir(crate_dir().join("routes")).expect("read routes/");
    let terms: BTreeSet<String> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "toml"))
        .filter_map(|path| path.file_stem().and_then(|s| s.to_str()).map(str::to_ascii_lowercase))
        .filter(|term| term.len() >= MIN_TERM_LEN)
        .collect();
    assert!(!terms.is_empty(), "routes/ must declare at least one framework");
    terms
}

/// Every build system the manifest rows declare — the `kind` of each
/// `[[manifest]]`, lowercased. A build system is data too, so the engine never
/// spells one. Ids under four letters (`go`, `pub`) are also plain words of the
/// engine's own language, and would only report prose.
fn declared_build_system_terms() -> BTreeSet<String> {
    let raw = std::fs::read_to_string(crate_dir().join("manifests.toml")).expect("read manifests.toml");
    let registry: toml::Value = toml::from_str(&raw).expect("manifests.toml is valid TOML");
    let rows = registry
        .get("manifest")
        .and_then(|v| v.as_array())
        .expect("manifests.toml declares [[manifest]] rows");
    let terms: BTreeSet<String> = rows
        .iter()
        .filter_map(|row| row.get("kind").and_then(|v| v.as_str()))
        .map(str::to_ascii_lowercase)
        .filter(|kind| kind.len() >= MIN_BUILD_SYSTEM_LEN)
        .collect();
    assert!(!terms.is_empty(), "manifests.toml must declare at least one build system");
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
    let mut terms = declared_language_terms();
    terms.extend(declared_framework_terms());
    terms.extend(declared_build_system_terms());
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
        "the engine must never spell a language the registry declares, nor a framework the route rules \
         declare — that knowledge belongs in languages.toml, manifests.toml, queries/<dir>/*.scm and \
         routes/, and this holds for comments too, because prose is what the next author copies:\n  {}",
        violations.join("\n  ")
    );
}

#[test]
fn responsibility_selection_has_no_language_or_framework_cases() {
    let mut terms=declared_language_terms();
    terms.extend(declared_framework_terms());
    let root=crate_dir().join("../..");
    for path in ["packages/core/src/domain/knowledge/selection.rs",
        "packages/core/src/io/knowledge/investigation.rs",
        "packages/core/src/io/code_search/quality.rs",
        "packages/core/src/io/code_search/scope.rs",
        "packages/core/src/io/code_search/task.rs",
        "packages/core/src/io/code_search/task_view.rs",
        "apps/rt/src/shared/knowledge_selection.rs"] {
        let text=std::fs::read_to_string(root.join(path)).unwrap();
        for (line,text) in text.lines().enumerate().take_while(|(_,line)|!line.trim_start().starts_with("#[cfg(test)]")) {
            for term in &terms {
                assert!(!mentions_whole_word(text,term),"{path}:{} names language/framework {term}: {text}",line+1);
            }
        }
    }
}

#[test]
fn the_vocabulary_comes_from_the_registry_and_nowhere_else() {
    // A ratchet whose term list silently emptied would pass forever while
    // checking nothing. Assert it is actually loaded and plural, and that the
    // length floor is what excludes ids rather than any named exception.
    let mut terms = declared_language_terms();
    assert!(terms.len() >= 2, "expected several language ids, got {terms:?}");
    let frameworks = declared_framework_terms();
    assert!(frameworks.len() >= 2, "expected several frameworks, got {frameworks:?}");
    terms.extend(frameworks);
    let build_systems = declared_build_system_terms();
    assert!(build_systems.len() >= 2, "expected several build systems, got {build_systems:?}");
    terms.extend(build_systems);
    assert!(
        terms.iter().all(|t| t.len() >= MIN_TERM_LEN),
        "every checked term clears the length floor: {terms:?}"
    );
    assert!(
        terms.iter().all(|t| t.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')),
        "registry ids are lowercase slugs: {terms:?}"
    );
}

/// Cada arquivo de um projeto de teste com o que ele importa, como o scan o
/// lê: o caminho e as dependências que o mapa grava para ele.
fn dependencies(files: &[(&str, &str)]) -> Vec<(String, Vec<String>)> {
    let project = tempfile::Builder::new().prefix("scan-language-blind-").tempdir().unwrap();
    for (path, text) in files {
        let file = project.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    let out = tempfile::Builder::new().prefix("scan-language-blind-map-").tempdir().unwrap();
    let (map, _) = model::scan(project.path(), out.path(), &[]);
    let mut modules: Vec<(String, Vec<String>)> = map["modules"]
        .as_array()
        .expect("model.modules")
        .iter()
        .map(|m| {
            let deps = m["deps"].as_array().map(|d| d.iter().map(|d| d.as_str().unwrap().to_string()).collect());
            (m["path"].as_str().unwrap().to_string(), deps.unwrap_or_default())
        })
        .collect();
    modules.sort();
    modules
}

/// Uma língua que não escreve `::` entre as partes de um nome nunca vê o `::`
/// virar barra: o import `"a::b"` de um arquivo dela é o texto de um pacote,
/// e não o caminho `a/b` do projeto. Quem declara o `::` como separador (a
/// língua do controle) o lê como caminho, com o mesmo formato de projeto.
#[test]
fn a_language_that_does_not_write_double_colons_never_sees_them_become_slashes() {
    for (importer, target, import) in [
        ("main.ts", "a/b.ts", "import { b } from \"a::b\";\nconsole.log(b);\n"),
        ("main.js", "a/b.js", "import { b } from \"a::b\";\nconsole.log(b);\n"),
    ] {
        let modules = dependencies(&[(target, "export const b = 1;\n"), (importer, import)]);
        let importer_deps = &modules.iter().find(|(path, _)| path == importer).expect("the importer is mapped").1;
        assert!(importer_deps.is_empty(), "`a::b` in {importer} became the path of {target}: {modules:?}");
    }
    let control = dependencies(&[
        ("src/main.rs", "mod util;\n\nuse crate::util::helper;\n\nfn main() {\n    let _ = helper();\n}\n"),
        ("src/util.rs", "pub fn helper() -> usize {\n    1\n}\n"),
    ]);
    let main = &control.iter().find(|(path, _)| path == "src/main.rs").expect("the importer is mapped").1;
    assert_eq!(main, &["src/util.rs"], "a language that declares `::` still reads it as a path: {control:?}");
}
