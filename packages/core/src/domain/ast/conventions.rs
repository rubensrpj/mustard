//! `conventions` — agnostic, structural source-file predicates shared across
//! the scan engine.
//!
//! These are pure lexical / path predicates with no notion of any one
//! programming language, framework, or architecture. Each rule is a general
//! shape: a path-segment convention, a filename-stem convention, or a
//! comment-prefix character class. They live here, next to the other
//! `ast::*` agnostic primitives, so every later phase resolves them at a
//! single stable public path rather than reimplementing them per call site.

/// Path segments that, by widely-shared convention across communities, mark a
/// directory as holding tests, specs, fixtures, or mocks. Compared
/// case-insensitively against a whole `/`-delimited segment — never as a
/// substring — so `attestation/` (which merely contains the letters of
/// `test`) is not mistaken for a test directory.
const TEST_DIR_SEGMENTS: &[&str] = &[
    "test",
    "tests",
    "__tests__",
    "testdata",
    "fixtures",
    "__mocks__",
];

/// Segments that mark test terrain ONLY near the top of a project — because
/// the same words are ordinary domain nouns everywhere else.
///
/// `spec/` beside the source tree is a real and widespread convention. `spec/`
/// INSIDE the source tree is a folder about specifications, and the difference
/// is where it sits, not what it is called. Treating the word as decisive at
/// any depth cost this workspace 46 production modules and 1419 declarations —
/// every module of its own `commands/spec/` family and its `domain/spec/`
/// core — silently excluded from retrieval, so a request about specs could not
/// reach the code that implements them. Nothing failed; the files were simply
/// never candidates.
///
/// The unambiguous names above need no such qualification: no project calls a
/// domain folder `__tests__` or `testdata`, so those stay decisive at any
/// depth. This split is DATA — which words are ambiguous — not a rule about
/// any language or framework.
const AMBIGUOUS_TEST_DIR_SEGMENTS: &[&str] = &["spec", "specs"];

/// How deep an [`AMBIGUOUS_TEST_DIR_SEGMENTS`] segment may sit and still count
/// as test terrain (0-based index of the segment in the repo-relative path).
///
/// The convention places such a directory BESIDE the source tree, at the root
/// of the project it tests: `spec/…` in a single-project repository, or
/// `<subproject>/spec/…` under one level of monorepo nesting — index 0 through
/// 2. Deeper than that it is inside a source tree, where it names a domain
/// concept rather than a test suite.
const AMBIGUOUS_MAX_DEPTH: usize = 2;

/// Filename-stem suffixes that mark a file as a test/spec by convention, in any
/// of the common separator styles (`.`, `_`, `-`) plus the bare `tests` plural.
/// Matched case-insensitively against the stem (the filename with its final
/// extension removed).
const TEST_STEM_SUFFIXES: &[&str] = &[
    ".test", ".spec", "_test", "_spec", "-test", "-spec", "tests",
];

/// Bare suffixes (`test` / `spec`) accepted ONLY when they begin a camelCase
/// word inside the stem — i.e. the char before the suffix is a lowercase
/// letter or a digit and the suffix itself is capitalised in the original
/// (un-lowered) stem. This admits the `FooSpec` / `OrderTest` convention while
/// rejecting `latest` / `attest`, where the trailing `test` is not a word.
const TEST_CAMEL_SUFFIXES: &[&str] = &["test", "spec"];

/// Filename-stem prefixes that mark a file as a test by convention. Matched
/// case-insensitively against the stem.
///
/// `test_` only. The spec conventions communities actually use are SUFFIXES —
/// `foo_spec.rb`, `foo.spec.ts` — and both are already covered by
/// [`TEST_STEM_SUFFIXES`]. There is no established `spec_foo` test convention,
/// while `spec_views`, `spec_draft`, `spec_staleness` are ordinary names for
/// modules ABOUT specifications. Carrying the prefix hid four such production
/// modules, one of them the largest single file in its subproject.
const TEST_STEM_PREFIXES: &[&str] = &["test_"];

/// Single-line comment prefixes shared across the common comment styles. A
/// trimmed line starting with any of these is treated as a comment line. This
/// is intentionally the same set the scan engine's structural extractor uses,
/// so the two agree byte-for-byte.
const COMMENT_PREFIXES: &[&str] = &["//", "#", "--", "/*", "*", "<!--", ";", "%"];

/// Whether a relative path points at a test/spec/fixture/mock file by
/// convention — agnostic to any programming language or framework.
///
/// The relative path is normalised to forward slashes and compared
/// case-insensitively. It is a test path when EITHER:
///
/// - any whole path segment equals one of [`TEST_DIR_SEGMENTS`] (segment
///   match, not substring — so `attestation/x.rs` is NOT a test path); OR
/// - the filename stem (the final component with its last extension removed)
///   ends with one of [`TEST_STEM_SUFFIXES`] or starts with one of
///   [`TEST_STEM_PREFIXES`].
#[must_use]
pub fn is_test_path(rel: &str) -> bool {
    let slashed = rel.replace('\\', "/");
    let normalised = slashed.to_ascii_lowercase();

    // Segment convention: an unambiguous test directory counts at any depth; an
    // ambiguous one (a word that is also an ordinary domain noun) counts only
    // where the convention actually places it — see
    // [`AMBIGUOUS_TEST_DIR_SEGMENTS`].
    for (i, segment) in normalised.split('/').enumerate() {
        if segment.is_empty() {
            continue;
        }
        if TEST_DIR_SEGMENTS.iter().any(|d| *d == segment) {
            return true;
        }
        if i <= AMBIGUOUS_MAX_DEPTH && AMBIGUOUS_TEST_DIR_SEGMENTS.iter().any(|d| *d == segment) {
            return true;
        }
    }

    // Stem convention: inspect only the filename (last segment), with its final
    // extension stripped, so `foo.test.ts` → stem `foo.test`, `foo_test.go` →
    // stem `foo_test`, `test_foo.py` → stem `test_foo`. The original-case stem
    // is kept alongside the lowered one so a camelCase word boundary
    // (`FooSpec`) can be detected.
    let Some(file_lc) = normalised.rsplit('/').next() else {
        return false;
    };
    let file_orig = slashed.rsplit('/').next().unwrap_or(file_lc);
    let stem = stem_of(file_lc);
    let stem_orig = stem_of(file_orig);
    if stem.is_empty() {
        return false;
    }
    if TEST_STEM_SUFFIXES.iter().any(|s| stem.ends_with(s)) {
        return true;
    }
    if TEST_STEM_PREFIXES.iter().any(|p| stem.starts_with(p)) {
        return true;
    }
    if ends_with_camel_word(stem, stem_orig) {
        return true;
    }
    false
}

/// Strip the final extension from a filename to get its stem. A leading dot is
/// not treated as an extension separator.
fn stem_of(file: &str) -> &str {
    match file.rfind('.') {
        Some(idx) if idx > 0 => &file[..idx],
        _ => file,
    }
}

/// `true` when `stem` (lowered) ends with one of [`TEST_CAMEL_SUFFIXES`] AND,
/// in the original-case `stem_orig`, that suffix begins a capitalised word
/// preceded by a lowercase letter or digit — the `FooSpec` / `OrderTest`
/// convention. Rejects `latest` / `attest`, where the trailing letters are not
/// a separate word.
fn ends_with_camel_word(stem: &str, stem_orig: &str) -> bool {
    if stem.len() != stem_orig.len() {
        // Lengths differ only under non-ASCII case folding; fall back to no
        // camel match rather than risk a byte-index mismatch.
        return false;
    }
    for suffix in TEST_CAMEL_SUFFIXES {
        if !stem.ends_with(suffix) {
            continue;
        }
        let start = stem.len() - suffix.len();
        if start == 0 {
            // The whole stem is exactly `test`/`spec`; that is covered by the
            // segment rule when it is a directory, and a bare file stem of
            // `test`/`spec` is not a camelCase compound.
            continue;
        }
        let bytes = stem_orig.as_bytes();
        let prev = bytes[start - 1] as char;
        let first = bytes[start] as char;
        if (prev.is_ascii_lowercase() || prev.is_ascii_digit()) && first.is_ascii_uppercase() {
            return true;
        }
    }
    false
}

/// Whether a line is a single-line comment — agnostic to any programming
/// language. The line is leading-trimmed, then matched against the shared
/// [`COMMENT_PREFIXES`] set.
#[must_use]
pub fn is_comment(line: &str) -> bool {
    let trimmed = line.trim_start();
    COMMENT_PREFIXES.iter().any(|p| trimmed.starts_with(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dir_segment_matches() {
        assert!(is_test_path("tests/"));
        assert!(is_test_path("tests/foo.rs"));
        assert!(is_test_path("src/__tests__/x.ts"));
        assert!(is_test_path("pkg/spec/runner.rb"));
        assert!(is_test_path("a/specs/b.rs"));
        assert!(is_test_path("data/testdata/sample.json"));
        assert!(is_test_path("a/fixtures/b.json"));
        assert!(is_test_path("a/__mocks__/b.js"));
    }

    #[test]
    fn test_stem_suffix_and_prefix_match() {
        assert!(is_test_path("src/foo.test.ts"));
        assert!(is_test_path("foo_test.go"));
        assert!(is_test_path("test_foo.py"));
        assert!(is_test_path("bar.spec.js"));
        assert!(is_test_path("a/b-test.kt"));
        assert!(is_test_path("a/widget-spec.rb"));
        // A stem ending in `tests` (no extension separator) is a test stem.
        assert!(is_test_path("integrationtests.go"));
        // `spec_` is NOT a prefix convention. The spec conventions in use are
        // suffixes (`widget-spec.rb`, `bar.spec.js`), both asserted above,
        // while `spec_*` names modules ABOUT specifications.
        assert!(!is_test_path("src/spec_views.rs"), "a module about specs is not a test");
        assert!(!is_test_path("src/commands/spec_draft.rs"));
        // `test_` stays: it is the established convention.
        assert!(is_test_path("test_runner.rb"));
    }

    #[test]
    fn an_ambiguous_segment_counts_only_where_the_convention_puts_it() {
        // `spec/` beside the source tree is the convention; `spec/` inside it
        // is a domain folder. Same word, decided by where it sits — the
        // distinction that cost this workspace 46 production modules.
        assert!(is_test_path("spec/models/user_spec.rb"), "project-root spec/ is test terrain");
        assert!(is_test_path("apps/api/spec/thing.rb"), "one level of monorepo nesting still is");
        assert!(!is_test_path("apps/rt/src/commands/spec/cli.rs"), "inside the source tree it is a domain folder");
        assert!(!is_test_path("packages/core/src/domain/spec/contract.rs"));
        // Unambiguous names need no such qualification — nobody calls a domain
        // folder `__tests__`, so depth never rescues one.
        assert!(is_test_path("apps/rt/src/deep/nested/__tests__/x.ts"));
        assert!(is_test_path("apps/rt/src/deep/nested/fixtures/x.rs"));
    }

    #[test]
    fn foospec_camel_stem_is_test() {
        // `FooSpec` / `OrderTest` — the suffix begins a capitalised camelCase
        // word, so the stem reads as a spec/test by convention.
        assert!(is_test_path("FooSpec.ts"));
        assert!(is_test_path("a/b/OrderTest.java"));
        assert!(is_test_path("UserServiceSpec.scala"));
        // `latest` / `attest` end in the same letters but are not a word
        // boundary — they must NOT match.
        assert!(!is_test_path("src/latest.rs"));
        assert!(!is_test_path("src/attest.go"));
    }

    #[test]
    fn non_test_paths_rejected() {
        assert!(!is_test_path("src/models.rs"));
        assert!(!is_test_path("attestation/x.rs"));
        assert!(!is_test_path("src/domain/config.rs"));
        // `attestation` contains `test` as a substring but not as a segment.
        assert!(!is_test_path("a/attestation/b.rs"));
        // A plain dir that merely contains `spec` as substring is not a match.
        assert!(!is_test_path("src/specimens/x.rs"));
    }

    #[test]
    fn backslash_paths_are_normalised() {
        assert!(is_test_path(r"src\__tests__\x.ts"));
        assert!(is_test_path(r"src\foo.test.ts"));
        assert!(!is_test_path(r"src\models.rs"));
    }

    #[test]
    fn comment_prefixes_match_after_trim() {
        assert!(is_comment("// line"));
        assert!(is_comment("   # indented"));
        assert!(is_comment("-- sql"));
        assert!(is_comment("/* block start"));
        assert!(is_comment(" * doc continuation"));
        assert!(is_comment("<!-- html"));
        assert!(is_comment("; ini"));
        assert!(is_comment("% tex"));
    }

    #[test]
    fn non_comment_lines_rejected() {
        assert!(!is_comment("let x = 1;"));
        assert!(!is_comment("struct User {}"));
        assert!(!is_comment(""));
        assert!(!is_comment("   "));
    }
}
