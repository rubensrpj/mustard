//! `stacks` — registry of stack definitions + the public detection contract.
//!
//! Where [`super::frameworks`] answers *"which framework signals does this
//! file contain?"*, this module owns both the data the stack-inference engine
//! consumes — a TOML registry of stack definitions ([`StackDef`]) and the
//! serde contract a detection produces ([`StackDetection`]) — and the engine
//! itself ([`infer_stacks`] / [`StackRegistry::infer`]).
//!
//! ## Inference model
//!
//! The engine is *blind to stack names*: it only iterates the registry and
//! matches each definition's three signal **classes** against the evidence the
//! caller hands it —
//!
//! 1. `manifest_deps` — boundary-aware match against the parsed dependency
//!    names (exact, or the term followed by `@`/`/`), so `next` does not
//!    fire on `i18next` or `next-tick`;
//! 2. `path_markers`  — component-bounded presence in the project file paths;
//! 3. `code_signatures` — literal source substrings, matched in one pass over
//!    the supplied contents by the shared [`super::aho::KeyedAutomaton`]
//!    engine (the same primitive behind the framework detector — no second
//!    Aho-Corasick wiring).
//!
//! Confidence is a deterministic function of how many signal *classes*
//! converged (see [`CONFIDENCE_SCORING_VERSION`]), and every detection carries
//! the concrete signals that fired, so the output is explainable.
//!
//! ## Built-in base + on-disk override
//!
//! The base registry is embedded via [`include_str!`] from `stacks.toml`, so
//! inference works offline. A project may override it by dropping
//! `.claude/vocab/stacks.toml`; when that file exists it **replaces** the
//! built-in base wholesale — see [`StackRegistry::load`]. This mirrors the
//! built-in-plus-override policy of [`super::frameworks::FrameworkVocabulary`].
//!
//! No stack name is hardcoded in logic: stacks are pure DATA, declared in the
//! TOML (`[[stack]]` table array) and extended without recompiling.

use super::aho::KeyedAutomaton;
use super::VocabError;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// The built-in stack registry, embedded at compile time. The *guaranteed
/// base* for stack inference; an on-disk registry overrides it (see
/// [`StackRegistry::load`]).
const BUILTIN_STACKS_TOML: &str = include_str!("stacks.toml");

/// The default on-disk registry name resolved under `.claude/vocab/`.
/// [`StackRegistry::load`] looks for `.claude/vocab/stacks.toml`.
pub const DEFAULT_STACKS_NAME: &str = "stacks";

// ---------------------------------------------------------------------------
// Public detection contract
// ---------------------------------------------------------------------------

/// One detected stack on a scanned project — the public serde contract other
/// crates (rt, dashboard) and the scan tool render against. Pure data: no IO,
/// no matching logic.
///
/// Produced by the [`infer_stacks`] engine and carried on
/// [`crate::domain::scan::Project::detected_stacks`]; both sides default the
/// field so older payloads (without it) keep deserialising.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StackDetection {
    /// Canonical stack name as declared in the registry (e.g. `laravel`).
    pub name: String,
    /// Detection confidence in `0.0..=1.0` — the engine's aggregate over the
    /// signal classes that matched (manifest deps, path markers, code
    /// signatures).
    #[serde(default)]
    pub confidence: f32,
    /// The concrete signals that fired (e.g. `dep:laravel/framework`,
    /// `path:artisan`). Human-auditable evidence, not a stable enum.
    #[serde(default)]
    pub signals: Vec<String>,
}

// ---------------------------------------------------------------------------
// Registry schema
// ---------------------------------------------------------------------------

/// One stack definition in the registry — the declarative signal sets the
/// inference engine matches against. The TOML representation is one
/// `[[stack]]` table-array entry per instance.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct StackDef {
    /// Canonical stack identifier (lowercase, e.g. `laravel`, `django`).
    pub name: String,
    /// Optional host-language hint (lowercase, e.g. `php`, `python`).
    #[serde(default)]
    pub language: Option<String>,
    /// Dependency names as they appear in build manifests.
    #[serde(default)]
    pub manifest_deps: Vec<String>,
    /// Path fragments characteristic of the stack's conventional layout.
    #[serde(default)]
    pub path_markers: Vec<String>,
    /// Literal source substrings (Aho-Corasick signals, not grammars).
    #[serde(default)]
    pub code_signatures: Vec<String>,
}

/// Top-level document deserialised from a stack registry TOML. The
/// `[[stack]]` table array is the only key.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct StackRegistryDoc {
    /// Every `[[stack]]` table entry, in document order.
    #[serde(default, rename = "stack")]
    pub stacks: Vec<StackDef>,
}

impl StackRegistryDoc {
    /// Parse a stack registry TOML document. Pure on `&str`.
    ///
    /// # Errors
    /// Returns [`VocabError::InvalidToml`] when the input cannot be
    /// deserialised (missing `name`, malformed table array, …).
    pub fn parse_str(raw: &str) -> Result<Self, VocabError> {
        toml::from_str::<Self>(raw).map_err(|e| VocabError::InvalidToml(e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// A loaded stack registry — the validated definition list the wave-2
/// inference engine will match against. Construct via
/// [`StackRegistry::builtin`], [`StackRegistry::from_doc`], or
/// [`StackRegistry::load`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackRegistry {
    stacks: Vec<StackDef>,
}

impl StackRegistry {
    /// Build a registry from a parsed document, keeping document order
    /// (= priority order for the inference engine).
    ///
    /// # Errors
    /// Returns [`VocabError::NoTerms`] when the document declares no stack —
    /// fail loud on construction, matching the vocabulary-wide rule.
    pub fn from_doc(doc: StackRegistryDoc) -> Result<Self, VocabError> {
        if doc.stacks.is_empty() {
            return Err(VocabError::NoTerms);
        }
        Ok(Self { stacks: doc.stacks })
    }

    /// Build the registry from the embedded built-in TOML. Infallible in
    /// practice (the embedded TOML is validated by a unit test), but the
    /// constructor still surfaces a typed error rather than panicking so the
    /// `unwrap_used = deny` contract holds at every call site.
    ///
    /// # Errors
    /// Returns [`VocabError::InvalidToml`] if the embedded TOML ever fails to
    /// parse, or [`VocabError::NoTerms`] if it is emptied.
    pub fn builtin() -> Result<Self, VocabError> {
        let doc = StackRegistryDoc::parse_str(BUILTIN_STACKS_TOML)?;
        Self::from_doc(doc)
    }

    /// Load a *named* stack registry, preferring an on-disk override over the
    /// built-in base.
    ///
    /// Resolution (mirrors [`super::frameworks::FrameworkVocabulary::load`]):
    /// 1. If `{project_root}/.claude/vocab/{name}.toml` exists and parses, it
    ///    **replaces** the built-in base wholesale and is used as-is.
    /// 2. Otherwise (file absent) the embedded built-in base is used — but
    ///    only when `name` is [`DEFAULT_STACKS_NAME`]; a named registry that
    ///    does not exist on disk is a [`VocabError::FileNotFound`], because
    ///    silently substituting the stack base for an unrelated named registry
    ///    would hide a misconfiguration.
    ///
    /// A file that exists but fails to parse surfaces the parse error (it is
    /// *not* fail-open to the built-in): a malformed override is a real
    /// configuration bug the caller should see.
    ///
    /// # Errors
    /// [`VocabError::FileNotFound`] (named registry absent, non-default name),
    /// [`VocabError::InvalidToml`] (override present but unparseable),
    /// [`VocabError::Io`] (read failure), or [`VocabError::NoTerms`].
    pub fn load(name: &str, project_root: &Path) -> Result<Self, VocabError> {
        let path = project_root
            .join(".claude")
            .join("vocab")
            .join(format!("{name}.toml"));

        match std::fs::read_to_string(&path) {
            Ok(raw) => {
                let doc = StackRegistryDoc::parse_str(&raw)?;
                Self::from_doc(doc)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if name == DEFAULT_STACKS_NAME {
                    Self::builtin()
                } else {
                    Err(VocabError::FileNotFound(path.display().to_string()))
                }
            }
            Err(e) => Err(VocabError::Io(e.to_string())),
        }
    }

    /// The stack definitions in document order (= priority order).
    #[must_use]
    pub fn stacks(&self) -> &[StackDef] {
        &self.stacks
    }

    /// The declared host-language hint ([`StackDef::language`]) for the stack
    /// named `name`, or `None` when no such stack is registered or it declares
    /// no language. Case-sensitive on the registry's lowercase ids — detections
    /// carry the registry name verbatim, so the lookup matches.
    #[must_use]
    pub fn language_of(&self, name: &str) -> Option<&str> {
        self.stacks
            .iter()
            .find(|s| s.name == name)
            .and_then(|s| s.language.as_deref())
    }

    /// Run the inference engine against caller-supplied evidence.
    ///
    /// For every `[[stack]]` in this registry, three signal **classes** are
    /// matched independently:
    ///
    /// - **manifest deps** — a declared `manifest_deps` term fires when any
    ///   parsed dependency name in `deps` matches it at a name boundary: the
    ///   dependency IS the term, or starts with the term followed by `@`
    ///   (version suffix) or `/` (path suffix). A bare substring is not enough
    ///   — `next` must not fire on `i18next` or `next-tick`;
    /// - **path markers** — a declared `path_markers` fragment fires when it
    ///   is present in any of `paths` at a path-component boundary (so
    ///   `artisan` does not fire on `artisanal.txt`);
    /// - **code signatures** — declared `code_signatures` literals are matched
    ///   over every entry of `contents` in one pass of the shared
    ///   [`super::aho::KeyedAutomaton`] engine, keyed by registry index. When
    ///   two stacks declare the same signature, the engine's first-key-wins
    ///   dedup applies: the stack listed first in the registry owns the
    ///   signature (deterministic, document order = priority order).
    ///
    /// The engine never reads stack names: `name` is copied verbatim from the
    /// registry into the detection. Confidence is a pure function of how many
    /// classes converged — see [`CONFIDENCE_SCORING_VERSION`]. A stack with
    /// zero matched classes is **not** reported (no invented detections).
    ///
    /// Output ordering is deterministic: confidence descending, registry
    /// (document) order as the stable tiebreak. Each [`StackDetection`]
    /// carries the concrete signals that fired (`dep:`/`path:`/`code:`
    /// prefixed), in declaration order within each class.
    #[must_use]
    pub fn infer(&self, deps: &[String], paths: &[String], contents: &[String]) -> Vec<StackDetection> {
        // One automaton across every stack's code signatures, keyed by the
        // stack's registry index. `NoTerms` (no stack declares signatures)
        // simply means the code class never fires — not an error.
        let automaton = KeyedAutomaton::from_groups(
            self.stacks
                .iter()
                .enumerate()
                .map(|(idx, def)| (idx, def.code_signatures.clone())),
        )
        .ok();

        // Scan every content once; collect which signature terms fired, per
        // registry index.
        let mut fired_signatures: HashMap<usize, HashSet<String>> = HashMap::new();
        if let Some(ac) = &automaton {
            for content in contents {
                for hit in ac.scan(content) {
                    fired_signatures.entry(hit.key).or_default().insert(hit.term);
                }
            }
        }

        // Normalise path separators once so markers match both layouts.
        let norm_paths: Vec<String> = paths.iter().map(|p| p.replace('\\', "/")).collect();

        let mut detections: Vec<StackDetection> = Vec::new();
        for (idx, def) in self.stacks.iter().enumerate() {
            let mut signals: Vec<String> = Vec::new();
            let mut matched_classes = 0usize;

            // Class 1: manifest deps (boundary-aware over parsed dep names).
            let dep_signals: Vec<String> = def
                .manifest_deps
                .iter()
                .filter(|term| {
                    !term.is_empty() && deps.iter().any(|d| dep_matches_term(d, term))
                })
                .map(|term| format!("dep:{term}"))
                .collect();
            if !dep_signals.is_empty() {
                matched_classes += 1;
                signals.extend(dep_signals);
            }

            // Class 2: path markers (component-bounded presence).
            let path_signals: Vec<String> = def
                .path_markers
                .iter()
                .filter(|marker| marker_present(&norm_paths, marker))
                .map(|marker| format!("path:{marker}"))
                .collect();
            if !path_signals.is_empty() {
                matched_classes += 1;
                signals.extend(path_signals);
            }

            // Class 3: code signatures (Aho-Corasick hits collected above).
            // Iterate the declaration list so signal order stays the
            // registry's, not the haystack's.
            if let Some(fired) = fired_signatures.get(&idx) {
                let code_signals: Vec<String> = def
                    .code_signatures
                    .iter()
                    .filter(|sig| fired.contains(sig.as_str()))
                    .map(|sig| format!("code:{sig}"))
                    .collect();
                if !code_signals.is_empty() {
                    matched_classes += 1;
                    signals.extend(code_signals);
                }
            }

            if matched_classes == 0 {
                continue;
            }
            detections.push(StackDetection {
                name: def.name.clone(),
                confidence: confidence_for(matched_classes),
                signals,
            });
        }

        // Stable sort: equal confidences keep registry (push) order.
        detections.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
        detections
    }
}

// ---------------------------------------------------------------------------
// Confidence scoring (versioned)
// ---------------------------------------------------------------------------

/// Version of the convergence-scoring table below. Bump when the thresholds
/// change so downstream consumers can tell scores from different rules apart.
pub const CONFIDENCE_SCORING_VERSION: u32 = 1;

/// Confidence when exactly one signal class matched (low).
pub const CONFIDENCE_ONE_CLASS: f32 = 0.35;
/// Confidence when two signal classes converged (medium).
pub const CONFIDENCE_TWO_CLASSES: f32 = 0.65;
/// Confidence when all three signal classes converged (high).
pub const CONFIDENCE_THREE_CLASSES: f32 = 0.95;

/// Map a count of matched signal classes to a confidence score. Deterministic:
/// the count is the only input. Callers guarantee `matched_classes >= 1`
/// (zero-class stacks are never reported); counts above three saturate at the
/// high tier.
fn confidence_for(matched_classes: usize) -> f32 {
    match matched_classes {
        0 | 1 => CONFIDENCE_ONE_CLASS,
        2 => CONFIDENCE_TWO_CLASSES,
        _ => CONFIDENCE_THREE_CLASSES,
    }
}

/// `true` when the parsed dependency name `dep` matches the declared registry
/// `term` at a name boundary: `dep` IS the term, or starts with the term
/// immediately followed by `@` (a version suffix, e.g. `next@14.2.0`) or `/`
/// (a path suffix, e.g. a Go module's `gorm.io/gorm/v2` against `gorm.io/gorm`).
/// A bare substring must NOT match — `i18next` / `next-tick` are unrelated
/// packages and would otherwise false-positive the `next` term.
fn dep_matches_term(dep: &str, term: &str) -> bool {
    dep.strip_prefix(term)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('@') || rest.starts_with('/'))
}

/// `true` when `marker` is present in any of `paths` at a path-component
/// boundary. `paths` must already be `/`-normalised; the marker is normalised
/// here (separators + trailing slash). Empty markers never match — a blank
/// registry entry must not light up every project.
fn marker_present(paths: &[String], marker: &str) -> bool {
    let m = marker.replace('\\', "/");
    let m = m.trim_matches('/');
    if m.is_empty() {
        return false;
    }
    let suffix = format!("/{m}");
    let prefix = format!("{m}/");
    let infix = format!("/{m}/");
    paths
        .iter()
        .any(|p| p == m || p.ends_with(&suffix) || p.starts_with(&prefix) || p.contains(&infix))
}

// ---------------------------------------------------------------------------
// Engine entry points
// ---------------------------------------------------------------------------

/// Infer the stacks a project uses from parsed dependency names, project file
/// paths, and source contents, using the **built-in** registry. Convenience
/// entry point for callers that do not need a project-local override;
/// equivalent to building [`StackRegistry::builtin`] and calling
/// [`StackRegistry::infer`].
///
/// Degrades to an empty detection list — never panics, never invents a
/// detection — if the built-in registry ever fails to build. Callers that need
/// to distinguish "nothing detected" from "registry build error" should
/// construct the [`StackRegistry`] explicitly.
#[must_use]
pub fn infer_stacks(deps: &[String], paths: &[String], contents: &[String]) -> Vec<StackDetection> {
    match StackRegistry::builtin() {
        Ok(reg) => reg.infer(deps, paths, contents),
        Err(_) => Vec::new(),
    }
}

/// Infer stacks honouring a project-local registry override. Resolves the
/// registry via [`StackRegistry::load`]`(`[`DEFAULT_STACKS_NAME`]`, root)` so
/// a `.claude/vocab/stacks.toml` under `root` **replaces** the built-in base,
/// while a project with no override falls back to it.
///
/// The override-aware sibling of [`infer_stacks`], mirroring
/// [`super::frameworks::detect_framework_signals_with`]. A registry that is
/// absent (non-default resolution) or fails to parse degrades to an empty
/// detection list — never panics, never invents a detection.
#[must_use]
pub fn infer_stacks_with(
    root: &Path,
    deps: &[String],
    paths: &[String],
    contents: &[String],
) -> Vec<StackDetection> {
    match StackRegistry::load(DEFAULT_STACKS_NAME, root) {
        Ok(reg) => reg.infer(deps, paths, contents),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
// Exact float equality is intentional here: confidences are copied verbatim
// from the scoring constants (no arithmetic), so bit-exact comparison is the
// strongest — and correct — assertion.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Built-in registry
    // -----------------------------------------------------------------------

    #[test]
    fn stacks_registry_parses() {
        let reg = StackRegistry::builtin().expect("built-in stack registry parses");
        // The base seeds at least four stacks of distinct ecosystems
        // (composer/php, pip/python, npm/javascript, nuget-dotnet/csharp).
        assert!(reg.stacks().len() >= 4);
        let langs: Vec<_> = reg
            .stacks()
            .iter()
            .filter_map(|s| s.language.as_deref())
            .collect();
        assert!(langs.contains(&"php"));
        assert!(langs.contains(&"python"));
        assert!(langs.contains(&"javascript"));
        assert!(langs.contains(&"csharp"));
        // Every seeded stack carries all three signal classes.
        for s in reg.stacks() {
            assert!(!s.name.is_empty());
            assert!(!s.manifest_deps.is_empty(), "{} has manifest_deps", s.name);
            assert!(!s.path_markers.is_empty(), "{} has path_markers", s.name);
            assert!(!s.code_signatures.is_empty(), "{} has code_signatures", s.name);
        }
    }

    #[test]
    fn doc_parse_defaults_optional_fields() {
        let doc = StackRegistryDoc::parse_str("[[stack]]\nname = \"bare\"\n").unwrap();
        assert_eq!(doc.stacks.len(), 1);
        assert_eq!(doc.stacks[0].name, "bare");
        assert_eq!(doc.stacks[0].language, None);
        assert!(doc.stacks[0].manifest_deps.is_empty());
        assert!(doc.stacks[0].path_markers.is_empty());
        assert!(doc.stacks[0].code_signatures.is_empty());
    }

    #[test]
    fn doc_parse_rejects_missing_name() {
        let err = StackRegistryDoc::parse_str("[[stack]]\nlanguage = \"php\"\n").unwrap_err();
        assert!(matches!(err, VocabError::InvalidToml(_)));
    }

    #[test]
    fn empty_doc_is_no_terms() {
        let doc = StackRegistryDoc::parse_str("").unwrap();
        let err = StackRegistry::from_doc(doc).unwrap_err();
        assert!(matches!(err, VocabError::NoTerms));
    }

    // -----------------------------------------------------------------------
    // Named load + on-disk override
    // -----------------------------------------------------------------------

    #[test]
    fn load_default_falls_back_to_builtin_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let reg = StackRegistry::load(DEFAULT_STACKS_NAME, tmp.path())
            .expect("default name falls back to built-in");
        assert!(reg.stacks().len() >= 2);
    }

    #[test]
    fn load_non_default_named_registry_errors_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let err = StackRegistry::load("custom-stacks", tmp.path()).unwrap_err();
        assert!(matches!(err, VocabError::FileNotFound(_)));
    }

    #[test]
    fn on_disk_override_replaces_builtin() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".claude").join("vocab");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("stacks.toml"),
            r#"
[[stack]]
name = "bespoke"
language = "cobol"
manifest_deps = ["bespoke-runtime"]
"#,
        )
        .unwrap();

        let reg = StackRegistry::load(DEFAULT_STACKS_NAME, tmp.path()).unwrap();
        // The override IS respected and the built-in base is fully replaced.
        assert_eq!(reg.stacks().len(), 1);
        assert_eq!(reg.stacks()[0].name, "bespoke");
    }

    #[test]
    fn on_disk_override_parse_error_is_surfaced() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".claude").join("vocab");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("stacks.toml"), "this is = = not toml").unwrap();
        let err = StackRegistry::load(DEFAULT_STACKS_NAME, tmp.path()).unwrap_err();
        assert!(matches!(err, VocabError::InvalidToml(_)));
    }

    // -----------------------------------------------------------------------
    // StackDetection contract
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // infer_stacks — the matching engine
    // -----------------------------------------------------------------------

    /// A bespoke multi-signal fixture registry. Names are test literals only —
    /// the engine itself never reads them.
    fn fixture_registry() -> StackRegistry {
        let doc = StackRegistryDoc::parse_str(
            r#"
[[stack]]
name = "alpha"
manifest_deps = ["alpha/runtime"]
path_markers = ["alpha.config", "src/alpha"]
code_signatures = ["AlphaKernel::boot("]

[[stack]]
name = "beta"
manifest_deps = ["beta-core"]
path_markers = ["beta.lock"]
code_signatures = ["use BetaFacade"]
"#,
        )
        .unwrap();
        StackRegistry::from_doc(doc).unwrap()
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn infer_stacks_one_signal_class_is_low_confidence() {
        let reg = fixture_registry();
        // Only the dependency class fires (term + `@` version boundary).
        let out = reg.infer(&strings(&["alpha/runtime@^2"]), &[], &[]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "alpha");
        assert_eq!(out[0].confidence, CONFIDENCE_ONE_CLASS);
        assert_eq!(out[0].signals, vec!["dep:alpha/runtime"]);
    }

    #[test]
    fn infer_stacks_two_signal_classes_is_medium_confidence() {
        let reg = fixture_registry();
        let out = reg.infer(
            &strings(&["alpha/runtime"]),
            &strings(&["project/alpha.config"]),
            &[],
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].confidence, CONFIDENCE_TWO_CLASSES);
        assert_eq!(out[0].signals, vec!["dep:alpha/runtime", "path:alpha.config"]);
    }

    #[test]
    fn infer_stacks_three_signal_classes_is_high_confidence() {
        let reg = fixture_registry();
        let out = reg.infer(
            &strings(&["alpha/runtime"]),
            &strings(&["project/alpha.config"]),
            &strings(&["fn main() { AlphaKernel::boot(); }"]),
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "alpha");
        assert_eq!(out[0].confidence, CONFIDENCE_THREE_CLASSES);
        // Explainable output: every class that fired left its evidence.
        assert_eq!(
            out[0].signals,
            vec![
                "dep:alpha/runtime",
                "path:alpha.config",
                "code:AlphaKernel::boot(",
            ]
        );
    }

    #[test]
    fn infer_stacks_orders_by_confidence_desc_then_registry_order() {
        let reg = fixture_registry();
        // beta converges on 2 classes, alpha on 1 → beta first despite being
        // second in the registry.
        let out = reg.infer(
            &strings(&["alpha/runtime", "beta-core"]),
            &strings(&["beta.lock"]),
            &[],
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].name, "beta");
        assert_eq!(out[1].name, "alpha");

        // Tie on confidence → registry (document) order is the stable tiebreak.
        let tied = reg.infer(&strings(&["alpha/runtime", "beta-core"]), &[], &[]);
        assert_eq!(tied.len(), 2);
        assert_eq!(tied[0].name, "alpha");
        assert_eq!(tied[1].name, "beta");
    }

    #[test]
    fn infer_stacks_reports_nothing_without_evidence() {
        let reg = fixture_registry();
        let out = reg.infer(
            &strings(&["left-pad"]),
            &strings(&["src/main.rs"]),
            &strings(&["fn add(a: i32, b: i32) -> i32 { a + b }"]),
        );
        assert!(out.is_empty());
    }

    #[test]
    fn infer_stacks_dep_term_does_not_fire_on_unrelated_superstring_deps() {
        // Regression: the `next` term (seeded `nextjs` data) must NOT fire on
        // common React deps that merely CONTAIN it — they are unrelated
        // packages, and the old substring match manufactured a false
        // `nextjs(0.35)` detection on non-Next repos.
        let out = infer_stacks(
            &strings(&["i18next", "react-i18next", "next-tick"]),
            &[],
            &[],
        );
        assert!(out.is_empty(), "unrelated superstrings fired: {out:?}");
    }

    #[test]
    fn infer_stacks_dep_term_fires_on_exact_and_boundary_forms() {
        // The exact package.json key (the real pipeline shape: manifest keys
        // arrive verbatim, no version suffix)...
        let exact = infer_stacks(&strings(&["next"]), &[], &[]);
        assert_eq!(exact.len(), 1);
        assert_eq!(exact[0].name, "nextjs");
        assert_eq!(exact[0].signals, vec!["dep:next"]);
        // ...and a versioned `name@version` form (callers may pass requirement
        // strings; the `@` boundary keeps them matching) both fire.
        let versioned = infer_stacks(&strings(&["next@14.2.0"]), &[], &[]);
        assert_eq!(versioned.len(), 1);
        assert_eq!(versioned[0].name, "nextjs");
        // A `/` path boundary also matches (ecosystem-path deps, e.g. a Go
        // module major-version suffix against its declared base path)...
        assert!(dep_matches_term("gorm.io/gorm/v2", "gorm.io/gorm"));
        // ...but a scoped package whose NAME merely ends with the term does
        // not (no inverse suffix boundary — kept minimal on purpose).
        assert!(!dep_matches_term("@scope/next", "next"));
    }

    #[test]
    fn infer_stacks_path_marker_respects_component_boundaries() {
        let reg = fixture_registry();
        // `src/alpha` inside `artisanal`-style noise must not fire: the
        // marker only matches at a path-component boundary.
        let near_miss = reg.infer(&[], &strings(&["src/alphabetical/x.rs"]), &[]);
        assert!(near_miss.is_empty());
        let exact = reg.infer(&[], &strings(&["src/alpha/kernel.rs"]), &[]);
        assert_eq!(exact.len(), 1);
        assert_eq!(exact[0].signals, vec!["path:src/alpha"]);
    }

    #[test]
    fn infer_stacks_copies_registry_name_verbatim() {
        // Blind engine: a never-seen-before stack name flows through untouched.
        let doc = StackRegistryDoc::parse_str(
            "[[stack]]\nname = \"Bespoke-Stack_99\"\nmanifest_deps = [\"bespoke\"]\n",
        )
        .unwrap();
        let reg = StackRegistry::from_doc(doc).unwrap();
        let out = reg.infer(&strings(&["bespoke"]), &[], &[]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "Bespoke-Stack_99");
    }

    #[test]
    fn infer_stacks_builtin_entry_point_detects_from_seed_data() {
        // Test literals exercise the seeded DATA; the engine stays blind.
        let out = infer_stacks(&strings(&["laravel/framework"]), &[], &[]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "laravel");
        assert_eq!(out[0].confidence, CONFIDENCE_ONE_CLASS);
    }

    #[test]
    fn infer_stacks_with_degrades_to_empty_on_malformed_registry() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".claude").join("vocab");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("stacks.toml"), "this is = = not toml").unwrap();
        // A registry that cannot be built yields no detections — and no panic.
        let out = infer_stacks_with(
            tmp.path(),
            &strings(&["laravel/framework"]),
            &strings(&["artisan"]),
            &strings(&["Schema::create("]),
        );
        assert!(out.is_empty());
    }

    #[test]
    fn infer_stacks_with_honours_on_disk_override() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".claude").join("vocab");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("stacks.toml"),
            "[[stack]]\nname = \"bespoke\"\nmanifest_deps = [\"bespoke-runtime\"]\n",
        )
        .unwrap();
        // The override IS respected: its stack detects...
        let custom = infer_stacks_with(tmp.path(), &strings(&["bespoke-runtime"]), &[], &[]);
        assert_eq!(custom.len(), 1);
        assert_eq!(custom[0].name, "bespoke");
        // ...and the built-in base is fully replaced: seed deps no longer match.
        let builtin = infer_stacks_with(tmp.path(), &strings(&["laravel/framework"]), &[], &[]);
        assert!(builtin.is_empty());
    }

    #[test]
    fn infer_stacks_shared_signature_credits_first_registry_entry() {
        // Two stacks declare the same code signature: the shared engine's
        // first-key-wins dedup awards it to the first registry entry —
        // deterministic, document order = priority order.
        let doc = StackRegistryDoc::parse_str(
            r#"
[[stack]]
name = "first"
code_signatures = ["@SharedSig"]

[[stack]]
name = "second"
code_signatures = ["@SharedSig"]
"#,
        )
        .unwrap();
        let reg = StackRegistry::from_doc(doc).unwrap();
        let out = reg.infer(&[], &[], &strings(&["x @SharedSig y"]));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "first");
    }

    #[test]
    fn stack_detection_round_trips_through_serde() {
        let det = StackDetection {
            name: "laravel".into(),
            confidence: 0.9,
            signals: vec!["dep:laravel/framework".into(), "path:artisan".into()],
        };
        let json = serde_json::to_string(&det).expect("detection serialises");
        let back: StackDetection = serde_json::from_str(&json).expect("detection deserialises");
        assert_eq!(back, det);
    }
}
