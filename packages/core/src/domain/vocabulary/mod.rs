//! `vocabulary` — four-layer term matcher backing the regression gate.
//!
//! Spec A / Wave 1 introduces the canonical vocabulary used by the regression
//! gate (W4) to detect intent drift inside agent plans and diffs. The
//! vocabulary is partitioned into four severity layers — [`Layer::Semantic`],
//! [`Layer::Pattern`], [`Layer::Keyword`], [`Layer::Noise`] — and lives in
//! `.claude/vocab/regression.toml`, editable at runtime without recompiling.
//!
//! ## Why this module exists
//!
//! Before the gate, ad-hoc `str::contains` calls were sprinkled across hooks
//! (`bash_guard`, `path_guard`, knowledge extractors), each carrying its own
//! private list of "interesting" tokens. Matching 100 needles with
//! `str::contains` is O(n × m) and the lists drifted between hooks. This
//! module replaces that with a single multi-pattern automaton built on top of
//! `aho-corasick`: one linear pass over the text, one source of truth for the
//! term list, one severity scale shared by every consumer.
//!
//! ## Design (SOLID + fail-open)
//!
//! - **Single responsibility.** This module knows the vocabulary file
//!   schema and the matcher contract. It does not read the gate verdict,
//!   trigger `AskUserQuestion`, or persist promotion decisions — those live
//!   in the gate runner (`apps/rt/src/run/gate_regression_check.rs`).
//! - **Open/Closed.** [`Layer`] is closed (exactly the four layers the gate
//!   defines); adding a new layer is a deliberate breaking change. The
//!   matcher is built once via [`VocabularyMatcher::from_layers`] and is
//!   immutable after construction — extension happens by rebuilding, never
//!   by mutating in place.
//! - **Dependency inversion / testability.** The parser ([`VocabularyDoc`])
//!   is pure on `&str`; the matcher constructor consumes `Vec<VocabLayer>`
//!   without touching the filesystem.
//! - **Fail-open.** A missing TOML file returns [`VocabError::FileNotFound`]
//!   that callers collapse into an empty vocabulary; an
//!   unparseable line surfaces as [`VocabError::InvalidToml`] without
//!   panicking; the matcher never panics on hostile haystacks
//!   (`aho-corasick` is byte-safe and UTF-8 unaware by design).
//!
//! ## On-disk schema (`.claude/vocab/regression.toml`)
//!
//! ```toml
//! [[layer]]
//! kind = "semantic"
//! terms = ["fail-open", "intent drift", "stub fail-open"]
//!
//! [[layer]]
//! kind = "pattern"
//! terms = ["None", "Vec::new()", "Default::default()"]
//!
//! [[layer]]
//! kind = "keyword"
//! terms = ["refactor", "deferred", "stub"]
//!
//! [[layer]]
//! kind = "noise"
//! terms = ["test", "fixture", "mock"]
//! ```
//!
//! Term order does not matter; duplicates inside the same layer are
//! deduplicated; the same term appearing in two layers keeps the *first*
//! occurrence and surfaces a `LayerCollision` warning to the caller.

pub mod aho;
pub mod stacks;

use crate::platform::error::Error as CoreError;
use serde::Deserialize;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// One of the four severity layers the regression gate operates on.
///
/// Order matters: variants are listed from most severe (semantic-level
/// concerns that almost always indicate intent drift) to least severe
/// (background noise that exists only to dampen false positives from the
/// other three layers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layer {
    /// High-level domain concerns: "fail-open", "regression", "stub deferral".
    /// A hit here is almost always escalated to a red verdict.
    Semantic,
    /// Concrete code patterns: `None`, `Vec::new()`, `unimplemented!()`,
    /// `todo!()`. A hit on a function the wave declared as preserved is a
    /// strong stub-fail-open signal.
    Pattern,
    /// Lexical hints: "refactor", "deferred", "placeholder". Weaker signal
    /// — used for plan-text scanning (Momento 1) where intent surfaces as
    /// English/Portuguese prose rather than code.
    Keyword,
    /// Background terms ("test", "fixture", "mock") that exist to balance
    /// the score: a paragraph that is mostly testing language is unlikely
    /// to be drifting intent.
    Noise,
}

impl Layer {
    /// Canonical lowercase name used in the TOML `kind = "..."` field.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Semantic => "semantic",
            Self::Pattern => "pattern",
            Self::Keyword => "keyword",
            Self::Noise => "noise",
        }
    }

    /// Inverse of [`Layer::as_str`]. Case-insensitive; unknown tokens return
    /// `None` rather than defaulting to a layer.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "semantic" => Some(Self::Semantic),
            "pattern" => Some(Self::Pattern),
            "keyword" => Some(Self::Keyword),
            "noise" => Some(Self::Noise),
            _ => None,
        }
    }

    /// All four layers in declaration order (most → least severe).
    #[must_use]
    pub fn all() -> [Self; 4] {
        [Self::Semantic, Self::Pattern, Self::Keyword, Self::Noise]
    }
}

/// One layer of the vocabulary — a layer kind plus its term list.
///
/// The TOML representation is one `[[layer]]` table array entry per
/// instance; [`VocabularyDoc`] is the document wrapper that collects them.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct VocabLayer {
    /// Which layer these terms belong to. Closed enum — typos in the TOML
    /// surface as `VocabError::InvalidToml`.
    pub kind: Layer,
    /// The actual terms. Duplicates inside one layer are tolerated and
    /// deduplicated by the matcher constructor. Empty term strings are
    /// rejected to keep the automaton sane.
    #[serde(default)]
    pub terms: Vec<String>,
}

impl VocabLayer {
    /// Build a layer from a TOML string. The string must contain a single
    /// `[[layer]]` table (i.e. the body of one array entry, not a full
    /// document). Most callers should use [`VocabularyDoc::parse_str`]
    /// instead, which handles the full document with multiple layers.
    ///
    /// # Errors
    /// Returns [`VocabError::InvalidToml`] when the string cannot be
    /// deserialised into [`VocabLayer`].
    pub fn parse_str(raw: &str) -> Result<Self, VocabError> {
        toml::from_str::<Self>(raw).map_err(|e| VocabError::InvalidToml(e.to_string()))
    }

}

/// Top-level document deserialised from `.claude/vocab/regression.toml`.
///
/// The `[[layer]]` table array is the only top-level key; future schema
/// extensions can be added without breaking older clients because the
/// derive uses serde's default attribute on the inner `terms` field.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct VocabularyDoc {
    /// Every `[[layer]]` table entry, in document order.
    #[serde(default, rename = "layer")]
    pub layers: Vec<VocabLayer>,
    /// Optional `[thresholds]` table (W7#2). Tunes the gate's numeric knobs
    /// — currently just `line_change_threshold` — without recompiling the
    /// binary. Absent in the seed catalogue; the gate falls back to its
    /// hard-coded defaults when fields are missing.
    #[serde(default)]
    pub thresholds: GateThresholds,
}

/// Optional `[thresholds]` block consumed by `gate_regression_check`.
///
/// Every field is `Option<_>` so partial overrides work: a `regression.toml`
/// can tune only `line_change_threshold` without supplying placeholders for
/// future knobs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct GateThresholds {
    /// Moment-3 line-change threshold. Snapshot deltas whose `line_changes`
    /// strictly exceed this value fire a signal (W7#3 keeps the `body_emptied`
    /// short-circuit on top). Defaults to the gate's hard-coded `5`.
    #[serde(default, rename = "line_change")]
    pub line_change_threshold: Option<usize>,
}

impl VocabularyDoc {
    /// Parse the canonical TOML document. Pure on `&str`.
    ///
    /// # Errors
    /// Returns [`VocabError::InvalidToml`] when the input cannot be
    /// deserialised.
    pub fn parse_str(raw: &str) -> Result<Self, VocabError> {
        toml::from_str::<Self>(raw).map_err(|e| VocabError::InvalidToml(e.to_string()))
    }

    /// Read the document from a path on disk. Distinguishes "file does not
    /// exist" (fail-open: caller treats as empty vocabulary) from a real
    /// I/O error (propagated).
    ///
    /// # Errors
    /// Returns [`VocabError::FileNotFound`] when the path does not exist,
    /// [`VocabError::Io`] for other read failures, and
    /// [`VocabError::InvalidToml`] for unparseable content.
    pub fn load_from_file(path: &Path) -> Result<Self, VocabError> {
        match std::fs::read_to_string(path) {
            Ok(raw) => Self::parse_str(&raw),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(VocabError::FileNotFound(path.display().to_string()))
            }
            Err(e) => Err(VocabError::Io(e.to_string())),
        }
    }

    /// Find the layer matching `kind` inside this document. Returns `None`
    /// when absent.
    #[must_use]
    pub fn layer(&self, kind: Layer) -> Option<&VocabLayer> {
        self.layers.iter().find(|l| l.kind == kind)
    }

    /// Return the term list for `kind` as borrowed `&str` slices in document
    /// order. Empty `Vec` when the layer is absent — callers that need to
    /// distinguish "absent" from "empty" should use [`Self::layer`] instead.
    ///
    /// Added to deduplicate the inline `[semantic]` / `[pattern]` walks that
    /// `subagent_inject` and `agent_prompt_render` used to ship (W5#2). The
    /// returned slices borrow from `self`; callers that need owned strings
    /// can `.iter().map(|s| s.to_string()).collect()`.
    #[must_use]
    pub fn layer_terms(&self, kind: Layer) -> Vec<&str> {
        self.layer(kind)
            .map(|l| l.terms.iter().map(String::as_str).collect())
            .unwrap_or_default()
    }
}

/// One match emitted by [`VocabularyMatcher::scan`].
///
/// The matcher reports the layer that owns the term, the byte offsets in
/// the haystack, and the matched term itself. Callers rank by layer
/// severity (semantic > pattern > keyword > noise) to produce the gate
/// verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanHit {
    /// Which layer the matched term belongs to.
    pub layer: Layer,
    /// The matched term as it appears in the vocabulary (NOT a substring of
    /// the haystack — important when the haystack contains case variants).
    pub term: String,
    /// Byte offset where the match starts (inclusive).
    pub start: usize,
    /// Byte offset where the match ends (exclusive).
    pub end: usize,
}

/// Typed error surface for the vocabulary module.
///
/// `#[non_exhaustive]` so later waves can add variants without breaking
/// downstream matches.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum VocabError {
    /// The TOML file does not exist on disk. Carries the path that was
    /// looked up. Kept distinct from [`VocabError::Io`] so callers can
    /// treat absence as "empty vocabulary" while still surfacing real I/O
    /// failures.
    #[error("vocabulary file not found: {0}")]
    FileNotFound(String),

    /// An underlying I/O operation failed (permissions, broken symlink,
    /// disk error).
    #[error("vocabulary io error: {0}")]
    Io(String),

    /// The TOML content failed to deserialise — typo in the `kind` field,
    /// invalid table-array shape, or malformed UTF-8 escape.
    #[error("invalid vocabulary toml: {0}")]
    InvalidToml(String),

    /// The matcher constructor was handed an empty term list across every
    /// layer. Surfacing this as a typed error (rather than silently
    /// building an empty automaton) catches misconfigured vocab files
    /// during W1's smoke tests.
    #[error("vocabulary has no terms across any layer")]
    NoTerms,
}

impl From<VocabError> for CoreError {
    fn from(e: VocabError) -> Self {
        match e {
            VocabError::FileNotFound(p) => CoreError::NotFound(p),
            VocabError::Io(m) => CoreError::Config(format!("vocab io: {m}")),
            VocabError::InvalidToml(m) => CoreError::Parse(m),
            VocabError::NoTerms => CoreError::Config("vocab has no terms".into()),
        }
    }
}

/// The verdict returned by [`check_layer_promotion`].
///
/// The Wave 1 contract is that *every* cross-layer promotion needs
/// confirmation — the gate (W4) wires this verdict to an
/// `AskUserQuestion` prompt. `Allowed` exists as a variant only to leave
/// room for future automation: today no caller ever returns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromotionVerdict {
    /// Promotion is benign and can be applied without confirmation. Today
    /// the only caller path that hits this is the self-promotion no-op
    /// (`from == to`); the variant is preserved so a future "auto-allow
    /// semantic → semantic family" rule has somewhere to land without an
    /// API break.
    Allowed,
    /// Promotion is allowed in principle but must be confirmed by the
    /// user. This is the default verdict for every cross-layer
    /// transition.
    NeedsConfirmation,
    /// Promotion violates an invariant and must be rejected outright. No
    /// rule produces this verdict in Wave 1; it exists so the W4 gate can
    /// later forbid e.g. `Noise → Semantic` jumps that are almost always
    /// vocab-file vandalism.
    Forbidden,
}

/// Check whether promoting `term` from layer `from` to layer `to`
/// requires user confirmation.
///
/// Wave 1 contract (AC-A-14): every cross-layer promotion returns
/// [`PromotionVerdict::NeedsConfirmation`]; same-layer self-promotion is
/// a no-op and returns [`PromotionVerdict::Allowed`]. The orchestrator
/// (which has access to the `AskUserQuestion` tool) consumes the verdict
/// — this module never prompts the user directly.
///
/// The `term` argument is accepted but currently unused; it is kept in
/// the signature so future rules can scope decisions per-term (e.g.
/// "`fail-open` is locked to `Semantic` and cannot be promoted").
#[must_use]
pub fn check_layer_promotion(_term: &str, from: Layer, to: Layer) -> PromotionVerdict {
    if from == to {
        return PromotionVerdict::Allowed;
    }
    PromotionVerdict::NeedsConfirmation
}

/// The matcher itself — opaque handle over the `aho-corasick` automaton
/// plus the layer/term mapping. Construct via
/// [`VocabularyMatcher::from_layers`] or [`VocabularyMatcher::from_file`].
pub struct VocabularyMatcher {
    inner: aho::AhoMatcher,
    // Source path, only set when the matcher was built via `from_file`;
    // `None` when the matcher was built in memory (`from_layers`).
    source_path: Option<PathBuf>,
}

impl std::fmt::Debug for VocabularyMatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VocabularyMatcher")
            .field("source_path", &self.source_path)
            .field("term_count", &self.inner.term_count())
            .finish()
    }
}

impl VocabularyMatcher {
    /// Build a matcher from an in-memory layer list. The list may contain
    /// any number of layers (zero, one, or all four); duplicates inside a
    /// single layer are deduplicated; the same term appearing in two
    /// layers keeps the *first* occurrence and is dropped from the
    /// second.
    ///
    /// # Errors
    /// Returns [`VocabError::NoTerms`] when every layer is empty (or the
    /// outer list itself is empty). An empty automaton would happily
    /// match nothing — surfacing the misconfiguration up-front matches
    /// the project-wide "fail loud on construction, fail open on
    /// matching" rule.
    pub fn from_layers(layers: Vec<VocabLayer>) -> Result<Self, VocabError> {
        let inner = aho::AhoMatcher::from_layers(layers)?;
        Ok(Self {
            inner,
            source_path: None,
        })
    }

    /// Build a matcher from a vocabulary TOML file on disk.
    ///
    /// Equivalent to [`VocabularyDoc::load_from_file`] + [`Self::from_layers`],
    /// but remembers the source path for diagnostics.
    ///
    /// # Errors
    /// See [`VocabularyDoc::load_from_file`] and [`Self::from_layers`].
    pub fn from_file(path: &Path) -> Result<Self, VocabError> {
        let doc = VocabularyDoc::load_from_file(path)?;
        let inner = aho::AhoMatcher::from_layers(doc.layers)?;
        Ok(Self {
            inner,
            source_path: Some(path.to_path_buf()),
        })
    }

    /// Scan `haystack` for every term in the vocabulary. Returns hits in
    /// the order the matcher emits them (typically left-to-right, no
    /// overlap unless the automaton is configured otherwise).
    ///
    /// O(n + m) where n is `haystack.len()` and m is the total length of
    /// all vocabulary terms. The Wave 1 bench
    /// (`vocabulary::bench::scan_10k_chars_100_terms`) asserts <5ms
    /// for the canonical 10 000 × 100 fixture.
    #[must_use]
    pub fn scan(&self, haystack: &str) -> Vec<ScanHit> {
        self.inner.scan(haystack)
    }

    /// Total number of terms across every layer. Useful for diagnostics
    /// and for the bench harness that needs to assert "100 terms loaded".
    #[must_use]
    pub fn term_count(&self) -> usize {
        self.inner.term_count()
    }

    /// Number of terms inside a single layer.
    #[must_use]
    pub fn term_count_for(&self, layer: Layer) -> usize {
        self.inner.term_count_for(layer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Layer enum
    // -----------------------------------------------------------------------

    #[test]
    fn layer_round_trips_through_as_str_and_parse() {
        for layer in Layer::all() {
            assert_eq!(Layer::parse(layer.as_str()), Some(layer));
        }
    }

    #[test]
    fn layer_parse_is_case_insensitive() {
        assert_eq!(Layer::parse("Semantic"), Some(Layer::Semantic));
        assert_eq!(Layer::parse("PATTERN"), Some(Layer::Pattern));
        assert_eq!(Layer::parse("  noise  "), Some(Layer::Noise));
    }

    #[test]
    fn layer_parse_rejects_unknown() {
        assert_eq!(Layer::parse("severe"), None);
        assert_eq!(Layer::parse(""), None);
    }

    // -----------------------------------------------------------------------
    // VocabularyDoc / VocabLayer parsing
    // -----------------------------------------------------------------------

    #[test]
    fn vocabulary_doc_parses_full_document() {
        let toml = r#"
[[layer]]
kind = "semantic"
terms = ["fail-open", "intent drift"]

[[layer]]
kind = "pattern"
terms = ["None", "Vec::new()"]

[[layer]]
kind = "keyword"
terms = ["refactor"]

[[layer]]
kind = "noise"
terms = ["test"]
"#;
        let doc = VocabularyDoc::parse_str(toml).unwrap();
        assert_eq!(doc.layers.len(), 4);
        assert_eq!(doc.layer(Layer::Semantic).unwrap().terms.len(), 2);
        assert_eq!(doc.layer(Layer::Pattern).unwrap().terms[0], "None");
    }

    #[test]
    fn vocabulary_doc_parse_rejects_unknown_kind() {
        let toml = r#"
[[layer]]
kind = "severe"
terms = ["a"]
"#;
        let err = VocabularyDoc::parse_str(toml).unwrap_err();
        assert!(matches!(err, VocabError::InvalidToml(_)));
    }

    #[test]
    fn vocabulary_doc_parses_empty_document() {
        let doc = VocabularyDoc::parse_str("").unwrap();
        assert!(doc.layers.is_empty());
    }

    #[test]
    fn load_from_file_returns_file_not_found_for_missing_path() {
        let path = std::env::temp_dir().join("definitely-not-a-real-vocab.toml");
        let err = VocabularyDoc::load_from_file(&path).unwrap_err();
        assert!(matches!(err, VocabError::FileNotFound(_)));
    }

    // -----------------------------------------------------------------------
    // Matcher
    // -----------------------------------------------------------------------

    fn sample_layers() -> Vec<VocabLayer> {
        vec![
            VocabLayer {
                kind: Layer::Semantic,
                terms: vec!["fail-open".into(), "intent drift".into()],
            },
            VocabLayer {
                kind: Layer::Pattern,
                terms: vec!["None".into(), "Vec::new()".into()],
            },
            VocabLayer {
                kind: Layer::Keyword,
                terms: vec!["refactor".into(), "stub".into()],
            },
            VocabLayer {
                kind: Layer::Noise,
                terms: vec!["test".into()],
            },
        ]
    }

    #[test]
    fn matcher_from_empty_returns_no_terms_error() {
        let err = VocabularyMatcher::from_layers(vec![]).unwrap_err();
        assert!(matches!(err, VocabError::NoTerms));

        let err = VocabularyMatcher::from_layers(vec![VocabLayer {
            kind: Layer::Semantic,
            terms: vec![],
        }])
        .unwrap_err();
        assert!(matches!(err, VocabError::NoTerms));
    }

    #[test]
    fn matcher_scan_finds_terms_with_correct_layer() {
        let m = VocabularyMatcher::from_layers(sample_layers()).unwrap();
        let hits = m.scan("we should refactor this stub before fail-open kicks in");
        let layers: Vec<_> = hits.iter().map(|h| h.layer).collect();
        assert!(layers.contains(&Layer::Keyword)); // refactor + stub
        assert!(layers.contains(&Layer::Semantic)); // fail-open
    }

    #[test]
    fn matcher_scan_offsets_are_byte_correct() {
        let m = VocabularyMatcher::from_layers(sample_layers()).unwrap();
        let h = "refactor here";
        let hits = m.scan(h);
        let hit = hits.iter().find(|h| h.term == "refactor").unwrap();
        assert_eq!(&h[hit.start..hit.end], "refactor");
    }

    #[test]
    fn matcher_term_count_reflects_layers() {
        let m = VocabularyMatcher::from_layers(sample_layers()).unwrap();
        assert_eq!(m.term_count(), 7);
        assert_eq!(m.term_count_for(Layer::Semantic), 2);
        assert_eq!(m.term_count_for(Layer::Noise), 1);
    }

    #[test]
    fn matcher_dedupes_terms_inside_one_layer() {
        let m = VocabularyMatcher::from_layers(vec![VocabLayer {
            kind: Layer::Semantic,
            terms: vec!["fail-open".into(), "fail-open".into()],
        }])
        .unwrap();
        assert_eq!(m.term_count(), 1);
    }

    #[test]
    fn matcher_drops_cross_layer_duplicate_keeping_first() {
        let m = VocabularyMatcher::from_layers(vec![
            VocabLayer {
                kind: Layer::Semantic,
                terms: vec!["stub".into()],
            },
            VocabLayer {
                kind: Layer::Keyword,
                terms: vec!["stub".into(), "refactor".into()],
            },
        ])
        .unwrap();
        // `stub` keeps the Semantic layer; only `refactor` survives in Keyword.
        let hits = m.scan("we should not stub");
        let stub_hit = hits.iter().find(|h| h.term == "stub").unwrap();
        assert_eq!(stub_hit.layer, Layer::Semantic);
        assert_eq!(m.term_count_for(Layer::Keyword), 1);
    }

    // -----------------------------------------------------------------------
    // Layer promotion guard (T1.7)
    // -----------------------------------------------------------------------

    #[test]
    fn self_promotion_is_no_op_allowed() {
        for layer in Layer::all() {
            assert_eq!(
                check_layer_promotion("fail-open", layer, layer),
                PromotionVerdict::Allowed
            );
        }
    }

    #[test]
    fn cross_layer_promotion_needs_confirmation() {
        // Spec contract AC-A-14: every cross-layer change asks the user.
        assert_eq!(
            check_layer_promotion("stub", Layer::Noise, Layer::Keyword),
            PromotionVerdict::NeedsConfirmation
        );
        assert_eq!(
            check_layer_promotion("fail-open", Layer::Pattern, Layer::Semantic),
            PromotionVerdict::NeedsConfirmation
        );
        assert_eq!(
            check_layer_promotion("test", Layer::Semantic, Layer::Noise),
            PromotionVerdict::NeedsConfirmation
        );
    }

    // -----------------------------------------------------------------------
    // VocabError → CoreError conversion
    // -----------------------------------------------------------------------

    #[test]
    fn file_not_found_maps_to_core_not_found() {
        let core: CoreError = VocabError::FileNotFound("/tmp/x".into()).into();
        assert!(matches!(core, CoreError::NotFound(p) if p == "/tmp/x"));
    }

    #[test]
    fn invalid_toml_maps_to_core_parse() {
        let core: CoreError = VocabError::InvalidToml("bad".into()).into();
        assert!(matches!(core, CoreError::Parse(_)));
    }
}

// ---------------------------------------------------------------------------
// Bench (T1.6 — AC-A-11)
//
// Lives next to the unit tests rather than under `benches/` so the cargo-test
// path matches the AC literal:
//
//     cargo test -p mustard-core --release vocabulary::bench::scan_10k_chars_100_terms
//
// A `#[test]` in release is enough — aho-corasick is the only hot path and
// the assertion is a wall-clock budget, not a statistical comparison. If the
// budget needs tightening in a later wave, port to `criterion` then.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod bench {
    use super::*;
    use std::time::{Duration, Instant};

    /// Build a synthetic vocabulary of 100 terms — 25 per layer — and confirm
    /// that one `.scan()` over a 10 000-char haystack finishes inside 5ms.
    ///
    /// AC-A-11 contract: regression-gate scans must stay sub-frame so the
    /// gate never feels like it is "hanging" between agent edits.
    #[test]
    fn scan_10k_chars_100_terms() {
        // 25 terms per layer × 4 layers = 100 terms total.
        let layers: Vec<VocabLayer> = Layer::all()
            .iter()
            .map(|&kind| {
                let terms = (0..25)
                    .map(|idx| format!("term_{}_{idx}", kind.as_str()))
                    .collect::<Vec<_>>();
                VocabLayer { kind, terms }
            })
            .collect();

        let matcher = VocabularyMatcher::from_layers(layers)
            .expect("matcher constructs from 100 synthetic terms");
        assert_eq!(matcher.term_count(), 100);

        // Build a 10 000-char haystack that sprinkles a handful of real
        // terms across mostly-neutral filler. Hits exercise the automaton's
        // emit path; filler exercises the scanning path.
        let unit = "lorem ipsum dolor sit amet, term_semantic_3 consectetur \
                    adipiscing elit, sed do eiusmod term_pattern_7 tempor \
                    incididunt ut labore et dolore magna aliqua. term_keyword_12 \
                    ut enim ad minim veniam, quis term_noise_19 nostrud \
                    exercitation ullamco laboris nisi ut aliquip ex ea commodo. ";
        let mut haystack = String::with_capacity(10_000);
        while haystack.len() < 10_000 {
            haystack.push_str(unit);
        }
        haystack.truncate(10_000);
        assert_eq!(haystack.len(), 10_000);

        // Single-shot timing — we are asserting a budget, not measuring
        // variance. If a future change makes scan() rebuild the automaton
        // per call, this jumps to ~50ms and the assert fires loud.
        let start = Instant::now();
        let hits = matcher.scan(&haystack);
        let elapsed = start.elapsed();

        // Sanity: the haystack contains real terms, so we expect >0 hits.
        // This catches "matcher built but empty automaton" regressions
        // that would otherwise pass the timing budget trivially.
        assert!(
            !hits.is_empty(),
            "scan returned zero hits — matcher likely degenerate"
        );

        // Eprintln so the elapsed shows up under `cargo test -- --nocapture`
        // without spamming the default test runner output.
        eprintln!(
            "AC-A-11 scan_10k_chars_100_terms: {elapsed:?} ({} hits)",
            hits.len()
        );

        assert!(
            elapsed < Duration::from_millis(5),
            "AC-A-11 violated: {elapsed:?}"
        );
    }
}
