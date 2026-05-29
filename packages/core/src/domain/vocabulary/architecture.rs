//! `architecture` — deterministic, language- and stack-agnostic detection of
//! the **architectural style** of a codebase (Layered / MVC, Hexagonal /
//! Ports-Adapters, Clean / Onion, DDD) plus a coarse SOLID-adherence note.
//!
//! Where [`super::frameworks`] answers *what stack does this file belong to?*,
//! this module answers *how is the code organised?* — by classifying folder /
//! path segments into architectural **roles** (domain / application /
//! infrastructure / presentation / ports / adapters) and combining that with
//! the **direction of the dependency graph between roles** (does `domain`
//! import `infrastructure`, or only the reverse?).
//!
//! No LLM, no regex alternation: the role classification is one Aho-Corasick
//! pass per segment over the shared [`super::aho::KeyedAutomaton`] engine — the
//! same engine [`super::VocabularyMatcher`] and [`super::frameworks`] use. The
//! decision rule on top is pure arithmetic over the role-presence set + the
//! layer-edge directions.
//!
//! ## Agnostic by construction
//!
//! Nothing here hardcodes a stack, a language, or assumes any one architecture.
//! The role vocabulary is folder/segment names common across communities
//! (`domain`, `usecases`, `repositories`, `controllers`, `ports`, `adapters`,
//! …); a project that uses none of them yields [`ArchitectureStyle::Unknown`]
//! and the registry keeps the legacy `"unknown"` tag. A project may override
//! the base vocabulary wholesale via `.claude/vocab/architecture.toml`.
//!
//! ## Built-in base + on-disk override
//!
//! The base vocabulary is embedded via [`include_str!`] from
//! `architecture_builtin.toml`, so detection works offline. The override policy
//! mirrors [`super::frameworks::FrameworkVocabulary::load`] exactly.
//!
//! [`KeyedAutomaton`]: super::aho::KeyedAutomaton

use super::aho::KeyedAutomaton;
use super::VocabError;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The built-in architecture-role vocabulary, embedded at compile time. The
/// *guaranteed base* for [`detect_architecture_signals`]; an on-disk vocab
/// overrides it (see [`ArchitectureVocabulary::load`]).
const BUILTIN_ARCHITECTURE_TOML: &str = include_str!("architecture_builtin.toml");

/// The default on-disk vocabulary name resolved under `.claude/vocab/`.
/// [`ArchitectureVocabulary::load`] looks for `.claude/vocab/architecture.toml`.
pub const DEFAULT_ARCHITECTURE_NAME: &str = "architecture";

// ---------------------------------------------------------------------------
// Role schema
// ---------------------------------------------------------------------------

/// The architectural role a folder / path segment maps to. An *open* taxonomy
/// axis: there is no severity ordering between roles. New roles are added to the
/// TOML and this enum together.
///
/// `#[non_exhaustive]` so a later wave can add a variant without breaking a
/// downstream `match`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum LayerRole {
    /// Pure business model: `domain`, `entities`, `model`, `core`.
    Domain,
    /// Use-cases / orchestration: `application`, `usecases`, `services`,
    /// `handlers`, `commands`, `queries`.
    Application,
    /// Outer technical detail: `infrastructure`, `persistence`,
    /// `repositories`, `db`, `gateways`.
    Infrastructure,
    /// Delivery surface: `presentation`, `controllers`, `api`, `web`, `ui`,
    /// `views`, `routes`.
    Presentation,
    /// Hexagonal boundary interfaces: `ports`, `interfaces`, `boundaries`.
    Ports,
    /// Hexagonal boundary implementations: `adapters`.
    Adapters,
}

impl LayerRole {
    /// Canonical lowercase name used in the TOML `role = "..."` field.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Domain => "domain",
            Self::Application => "application",
            Self::Infrastructure => "infrastructure",
            Self::Presentation => "presentation",
            Self::Ports => "ports",
            Self::Adapters => "adapters",
        }
    }
}

/// One role group in an architecture vocabulary — a role plus its literal
/// segment signals. The TOML representation is one `[[role]]` table-array entry
/// per instance.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RoleSignal {
    /// Which role these segments belong to. Closed enum — an unknown `role`
    /// value surfaces as [`VocabError::InvalidToml`].
    pub role: LayerRole,
    /// The literal segment names to match (case-insensitive). Empty strings are
    /// dropped by the matcher constructor; duplicates collapse.
    #[serde(default)]
    pub signals: Vec<String>,
}

/// Top-level document deserialised from an architecture vocabulary TOML. The
/// `[[role]]` table array is the only key.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct ArchitectureVocabularyDoc {
    /// Every `[[role]]` table entry, in document order (= priority order).
    #[serde(default, rename = "role")]
    pub roles: Vec<RoleSignal>,
}

impl ArchitectureVocabularyDoc {
    /// Parse an architecture vocabulary TOML document. Pure on `&str`.
    ///
    /// # Errors
    /// Returns [`VocabError::InvalidToml`] when the input cannot be
    /// deserialised (bad `role`, malformed table array, …).
    pub fn parse_str(raw: &str) -> Result<Self, VocabError> {
        toml::from_str::<Self>(raw).map_err(|e| VocabError::InvalidToml(e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// Matcher (segment → role)
// ---------------------------------------------------------------------------

/// A built architecture-role matcher: the shared Aho-Corasick engine keyed on
/// [`LayerRole`]. Construct via [`ArchitectureVocabulary::builtin`],
/// [`ArchitectureVocabulary::from_doc`], or [`ArchitectureVocabulary::load`].
pub struct ArchitectureVocabulary {
    inner: KeyedAutomaton<LayerRole>,
    /// The signal terms preserved alongside the automaton so a *whole-segment*
    /// classifier can require an exact match rather than a substring hit. The
    /// Aho engine is a substring matcher; for path segments we want
    /// `repository` to classify the `repositories` segment without `category`
    /// accidentally matching `cat`. We therefore use the automaton only to find
    /// candidate hits, then keep the longest hit that the segment *contains as a
    /// bounded token*.
    terms: Vec<(LayerRole, String)>,
}

impl std::fmt::Debug for ArchitectureVocabulary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArchitectureVocabulary")
            .field("signal_count", &self.inner.term_count())
            .finish()
    }
}

impl ArchitectureVocabulary {
    /// Build a matcher from a parsed document. Signals are fed to the shared
    /// engine in document order so the first listed role wins a cross-role
    /// segment collision.
    ///
    /// # Errors
    /// Returns [`VocabError::NoTerms`] when no non-empty signal survives across
    /// every role.
    pub fn from_doc(doc: ArchitectureVocabularyDoc) -> Result<Self, VocabError> {
        // The signals are lowercased up-front so the segment classifier can do a
        // case-insensitive compare without re-lowering the (immutable) vocab on
        // every call.
        let mut terms: Vec<(LayerRole, String)> = Vec::new();
        let groups: Vec<(LayerRole, Vec<String>)> = doc
            .roles
            .into_iter()
            .map(|r| {
                let lowered: Vec<String> = r
                    .signals
                    .iter()
                    .map(|s| s.trim().to_ascii_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect();
                for s in &lowered {
                    terms.push((r.role, s.clone()));
                }
                (r.role, lowered)
            })
            .collect();
        let inner = KeyedAutomaton::from_groups(groups)?;
        Ok(Self { inner, terms })
    }

    /// Build the matcher from the embedded built-in vocabulary. Infallible in
    /// practice (validated by a unit test) but still surfaces a typed error so
    /// the `unwrap_used = deny` contract holds at every call site.
    ///
    /// # Errors
    /// Returns [`VocabError::InvalidToml`] if the embedded TOML ever fails to
    /// parse, or [`VocabError::NoTerms`] if it is emptied.
    pub fn builtin() -> Result<Self, VocabError> {
        let doc = ArchitectureVocabularyDoc::parse_str(BUILTIN_ARCHITECTURE_TOML)?;
        Self::from_doc(doc)
    }

    /// Load a *named* architecture vocabulary, preferring an on-disk override
    /// over the built-in base. Resolution mirrors
    /// [`super::frameworks::FrameworkVocabulary::load`] exactly.
    ///
    /// # Errors
    /// [`VocabError::FileNotFound`] (named vocab absent, non-default name),
    /// [`VocabError::InvalidToml`] (override present but unparseable),
    /// [`VocabError::Io`] (read failure), or [`VocabError::NoTerms`].
    pub fn load(name: &str, project_root: &Path) -> Result<Self, VocabError> {
        let path = project_root
            .join(".claude")
            .join("vocab")
            .join(format!("{name}.toml"));

        match std::fs::read_to_string(&path) {
            Ok(raw) => {
                let doc = ArchitectureVocabularyDoc::parse_str(&raw)?;
                Self::from_doc(doc)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if name == DEFAULT_ARCHITECTURE_NAME {
                    Self::builtin()
                } else {
                    Err(VocabError::FileNotFound(path.display().to_string()))
                }
            }
            Err(e) => Err(VocabError::Io(e.to_string())),
        }
    }

    /// Classify a single path **segment** (already lowercased by the caller, or
    /// not — this lowercases defensively) into its architectural role, when one
    /// of the vocabulary signals matches it as a bounded token.
    ///
    /// "Bounded token" means: the segment equals the signal, or the signal is a
    /// whole word inside the segment delimited by `-`, `_`, `.` or a
    /// case-like boundary. This keeps `repositories` → [`LayerRole::Domain`]…
    /// no — `repositories` → [`LayerRole::Infrastructure`] while `category`
    /// never matches `cat`. The Aho automaton finds candidate hits in O(n+m);
    /// the boundary check then accepts the longest sound candidate.
    #[must_use]
    pub fn classify_segment(&self, segment: &str) -> Option<LayerRole> {
        let seg = segment.trim().to_ascii_lowercase();
        if seg.is_empty() {
            return None;
        }
        // Fast path — exact segment equality against any signal. This is the
        // common case (`domain`, `adapters`, `usecases`) and it is unambiguous.
        if let Some((role, _)) = self
            .terms
            .iter()
            .find(|(_, term)| *term == seg)
        {
            return Some(*role);
        }
        // Token path — the segment is a compound (`user_repositories`,
        // `order-controllers`, `domain.services`). Split on the universal
        // separators and classify each part; the FIRST part that classifies
        // wins (folder roots like `domain` precede their refinements).
        let parts: Vec<&str> = seg.split(['-', '_', '.', ' ']).filter(|s| !s.is_empty()).collect();
        if parts.len() > 1 {
            for part in &parts {
                if let Some((role, _)) = self.terms.iter().find(|(_, term)| term == part) {
                    return Some(*role);
                }
            }
        }
        // Aho fallback — a signal appears as a substring AND is bounded. Used
        // for segments the separator split missed (e.g. camelCase
        // `userRepository`). We keep only hits flanked by a non-alphanumeric
        // char or a case boundary so `category` does not match `cat`.
        let mut best: Option<(LayerRole, usize)> = None;
        for hit in self.inner.scan(&seg) {
            if !is_bounded(&seg, hit.start, hit.end) {
                continue;
            }
            let len = hit.end - hit.start;
            match best {
                Some((_, blen)) if blen >= len => {}
                _ => best = Some((hit.key, len)),
            }
        }
        best.map(|(role, _)| role)
    }

    /// Total number of distinct signal segments across every role.
    #[must_use]
    pub fn signal_count(&self) -> usize {
        self.inner.term_count()
    }
}

/// `true` when the byte span `[start, end)` inside `seg` is a bounded token —
/// flanked on each side by the string boundary, a non-alphanumeric char, or a
/// lower→upper case boundary (camelCase). Prevents `cat` from matching inside
/// `category`.
fn is_bounded(seg: &str, start: usize, end: usize) -> bool {
    let bytes = seg.as_bytes();
    let left_ok = start == 0 || {
        let prev = bytes[start - 1] as char;
        !prev.is_ascii_alphanumeric()
    };
    let right_ok = end >= bytes.len() || {
        let next = bytes[end] as char;
        // Boundary if the next char is a separator/non-alnum, or an uppercase
        // letter starting a new camelCase word.
        !next.is_ascii_alphanumeric() || next.is_ascii_uppercase()
    };
    left_ok && right_ok
}

// ---------------------------------------------------------------------------
// Detection result + decision rule
// ---------------------------------------------------------------------------

/// The architectural style the detector inferred.
///
/// `#[non_exhaustive]` so a later wave can add a style (e.g. `EventDriven`,
/// `Microkernel`) without breaking downstream matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ArchitectureStyle {
    /// No architectural-role signal surfaced — the legacy `"unknown"` tag.
    Unknown,
    /// A flat / single-bucket layout (one dominant role) — `mvc` when the
    /// presentation+domain pairing reads as Model-View-Controller, else a
    /// generic `layered`.
    Layered,
    /// Hexagonal / ports-and-adapters: the `ports` AND `adapters` roles are
    /// both present.
    Hexagonal,
    /// Clean / Onion: domain + application present, and the dependency graph
    /// points inward (domain does NOT import infrastructure/presentation).
    Clean,
    /// Domain-Driven Design layered: the four canonical layers (domain,
    /// application, infrastructure, presentation) are all present but the
    /// inward-dependency rule could not be confirmed.
    Ddd,
}

impl ArchitectureStyle {
    /// Stable lowercase tag written to `entity-registry.json` and consumed by
    /// the dashboard / skill selection. [`Self::Unknown`] maps to `"unknown"`,
    /// preserving the legacy registry value byte-for-byte.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Layered => "layered",
            Self::Hexagonal => "hexagonal",
            Self::Clean => "clean",
            Self::Ddd => "ddd",
        }
    }
}

/// The full deterministic architecture report for a subproject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchitectureReport {
    /// The inferred style.
    pub style: ArchitectureStyle,
    /// The roles whose folder signals were observed, in canonical order.
    pub roles_present: Vec<LayerRole>,
    /// Coarse SOLID-adherence note, inferable only when the dependency
    /// direction between roles is known: `Some(true)` when no inward layer was
    /// observed importing an outward layer (dependency-inversion respected),
    /// `Some(false)` when at least one such violation was seen, `None` when no
    /// layer edge was supplied (cannot infer).
    pub solid_adherence: Option<bool>,
}

impl ArchitectureReport {
    /// The unknown / no-signal report.
    #[must_use]
    pub fn unknown() -> Self {
        Self {
            style: ArchitectureStyle::Unknown,
            roles_present: Vec::new(),
            solid_adherence: None,
        }
    }
}

/// A directed dependency observed between two architectural roles: `from`
/// (the file's own role) imports / references something classified as `to`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerEdge {
    /// The role of the file that declared the import.
    pub from: LayerRole,
    /// The role the imported symbol / path was classified into.
    pub to: LayerRole,
}

/// "Inward rank" of a role for the dependency-inversion check. Lower = more
/// central (domain is the centre). An edge from a lower rank to a higher rank
/// (centre depending on the outside) is a dependency-inversion violation.
fn inward_rank(role: LayerRole) -> u8 {
    match role {
        LayerRole::Domain => 0,
        LayerRole::Ports => 1,
        LayerRole::Application => 2,
        LayerRole::Adapters => 3,
        LayerRole::Infrastructure | LayerRole::Presentation => 4,
    }
}

/// Decide the architectural style from the set of roles present and the
/// observed layer-dependency edges. **Pure** and deterministic — no IO, no
/// language id, no stack assumption.
///
/// Decision rule (first match wins):
///
/// 1. **Hexagonal** — both [`LayerRole::Ports`] and [`LayerRole::Adapters`]
///    present (the defining marker of ports-and-adapters).
/// 2. **Clean** — [`LayerRole::Domain`] AND [`LayerRole::Application`] present,
///    and the dependency direction is inward (no edge from a central layer to
///    an outer one). The hallmark of Clean/Onion is the inward dependency rule.
/// 3. **DDD** — all four canonical layers present (domain + application +
///    infrastructure + presentation) without a confirmable inward rule.
/// 4. **Layered** — at least two distinct roles present, or one of the classic
///    layered/MVC buckets (presentation + domain).
/// 5. **Unknown** — no role signal at all.
///
/// `solid_adherence` is set from the layer edges alone (independent of style):
/// any edge whose `from` is more central than its `to` flips it to
/// `Some(false)`; with edges present but no violation it is `Some(true)`; with
/// no edges it stays `None`.
#[must_use]
pub fn detect_architecture(
    roles_present: &BTreeSet<LayerRole>,
    layer_edges: &[LayerEdge],
) -> ArchitectureReport {
    if roles_present.is_empty() {
        return ArchitectureReport::unknown();
    }

    // SOLID / dependency-inversion adherence from the edges (style-independent).
    let mut solid_adherence: Option<bool> = None;
    for edge in layer_edges {
        if edge.from == edge.to {
            continue;
        }
        let violated = inward_rank(edge.from) < inward_rank(edge.to);
        solid_adherence = Some(solid_adherence.unwrap_or(true) && !violated);
    }
    let inward_ok = solid_adherence.unwrap_or(false);

    let has = |r: LayerRole| roles_present.contains(&r);
    let ordered: Vec<LayerRole> = [
        LayerRole::Domain,
        LayerRole::Application,
        LayerRole::Infrastructure,
        LayerRole::Presentation,
        LayerRole::Ports,
        LayerRole::Adapters,
    ]
    .into_iter()
    .filter(|r| has(*r))
    .collect();

    let style = if has(LayerRole::Ports) && has(LayerRole::Adapters) {
        ArchitectureStyle::Hexagonal
    } else if has(LayerRole::Domain) && has(LayerRole::Application) && inward_ok {
        ArchitectureStyle::Clean
    } else if has(LayerRole::Domain)
        && has(LayerRole::Application)
        && has(LayerRole::Infrastructure)
        && has(LayerRole::Presentation)
    {
        ArchitectureStyle::Ddd
    } else if ordered.len() >= 2 || (has(LayerRole::Presentation) && has(LayerRole::Domain)) {
        ArchitectureStyle::Layered
    } else {
        // Exactly one role present and it is not part of a layered pairing:
        // a single bucket is still a (degenerate) layered organisation.
        ArchitectureStyle::Layered
    };

    ArchitectureReport {
        style,
        roles_present: ordered,
        solid_adherence,
    }
}

/// Convenience: classify every segment of every `paths` entry using the
/// **built-in** vocabulary and return the role-presence set. Fail-open — a
/// vocabulary build error yields an empty set (detection degrades to
/// `Unknown`, never panics).
///
/// `paths` are forward-slash-normalised relative paths; each `/`-delimited
/// segment is classified independently. Callers that need a project-local
/// override should build [`ArchitectureVocabulary`] explicitly.
#[must_use]
pub fn detect_architecture_signals(paths: &[String]) -> BTreeSet<LayerRole> {
    match ArchitectureVocabulary::builtin() {
        Ok(v) => roles_in_paths(&v, paths),
        Err(_) => BTreeSet::new(),
    }
}

/// Collect the set of roles any segment of any path classifies into.
#[must_use]
pub fn roles_in_paths(vocab: &ArchitectureVocabulary, paths: &[String]) -> BTreeSet<LayerRole> {
    let mut roles: BTreeSet<LayerRole> = BTreeSet::new();
    for path in paths {
        for segment in path.split('/') {
            if let Some(role) = vocab.classify_segment(segment) {
                roles.insert(role);
            }
        }
    }
    roles
}

/// Convenience: count, per role, how many of `paths` contain a segment of that
/// role. Useful for the dominant-layer heuristic and for diagnostics.
#[must_use]
pub fn role_path_counts(vocab: &ArchitectureVocabulary, paths: &[String]) -> BTreeMap<LayerRole, usize> {
    let mut counts: BTreeMap<LayerRole, usize> = BTreeMap::new();
    for path in paths {
        let mut seen: BTreeSet<LayerRole> = BTreeSet::new();
        for segment in path.split('/') {
            if let Some(role) = vocab.classify_segment(segment) {
                seen.insert(role);
            }
        }
        for role in seen {
            *counts.entry(role).or_insert(0) += 1;
        }
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vocab() -> ArchitectureVocabulary {
        ArchitectureVocabulary::builtin().expect("built-in architecture vocab builds")
    }

    #[test]
    fn builtin_toml_parses_and_builds() {
        let v = vocab();
        assert!(v.signal_count() > 0);
    }

    #[test]
    fn classify_exact_segments() {
        let v = vocab();
        assert_eq!(v.classify_segment("domain"), Some(LayerRole::Domain));
        assert_eq!(v.classify_segment("application"), Some(LayerRole::Application));
        assert_eq!(v.classify_segment("infrastructure"), Some(LayerRole::Infrastructure));
        assert_eq!(v.classify_segment("controllers"), Some(LayerRole::Presentation));
        assert_eq!(v.classify_segment("ports"), Some(LayerRole::Ports));
        assert_eq!(v.classify_segment("adapters"), Some(LayerRole::Adapters));
    }

    #[test]
    fn classify_is_case_insensitive() {
        let v = vocab();
        assert_eq!(v.classify_segment("Domain"), Some(LayerRole::Domain));
        assert_eq!(v.classify_segment("UseCases"), Some(LayerRole::Application));
    }

    #[test]
    fn classify_compound_segment_first_part_wins() {
        let v = vocab();
        assert_eq!(v.classify_segment("user_repositories"), Some(LayerRole::Infrastructure));
        assert_eq!(v.classify_segment("order-controllers"), Some(LayerRole::Presentation));
    }

    #[test]
    fn classify_rejects_false_substring() {
        let v = vocab();
        // `category` must NOT classify as a role just because some signal is a
        // substring; no architectural role contains a sound `cat`/`gory` token.
        assert_eq!(v.classify_segment("category"), None);
        assert_eq!(v.classify_segment("randomname"), None);
    }

    #[test]
    fn roles_in_clean_layout() {
        let v = vocab();
        let paths = vec![
            "src/domain/user.rs".to_string(),
            "src/application/create_user.rs".to_string(),
            "src/infrastructure/pg_user_repo.rs".to_string(),
        ];
        let roles = roles_in_paths(&v, &paths);
        assert!(roles.contains(&LayerRole::Domain));
        assert!(roles.contains(&LayerRole::Application));
        assert!(roles.contains(&LayerRole::Infrastructure));
    }

    #[test]
    fn hexagonal_when_ports_and_adapters() {
        let mut roles = BTreeSet::new();
        roles.insert(LayerRole::Domain);
        roles.insert(LayerRole::Ports);
        roles.insert(LayerRole::Adapters);
        let report = detect_architecture(&roles, &[]);
        assert_eq!(report.style, ArchitectureStyle::Hexagonal);
    }

    #[test]
    fn clean_when_domain_application_and_inward_edges() {
        let mut roles = BTreeSet::new();
        roles.insert(LayerRole::Domain);
        roles.insert(LayerRole::Application);
        roles.insert(LayerRole::Infrastructure);
        // Infrastructure depends on domain (inward) — the inversion is respected.
        let edges = vec![LayerEdge {
            from: LayerRole::Infrastructure,
            to: LayerRole::Domain,
        }];
        let report = detect_architecture(&roles, &edges);
        assert_eq!(report.style, ArchitectureStyle::Clean);
        assert_eq!(report.solid_adherence, Some(true));
    }

    #[test]
    fn ddd_when_four_layers_no_confirmable_inward_rule() {
        let mut roles = BTreeSet::new();
        roles.insert(LayerRole::Domain);
        roles.insert(LayerRole::Application);
        roles.insert(LayerRole::Infrastructure);
        roles.insert(LayerRole::Presentation);
        // No edges supplied → inward rule cannot be confirmed → DDD, not Clean.
        let report = detect_architecture(&roles, &[]);
        assert_eq!(report.style, ArchitectureStyle::Ddd);
        assert_eq!(report.solid_adherence, None);
    }

    #[test]
    fn solid_violation_when_domain_imports_infrastructure() {
        let mut roles = BTreeSet::new();
        roles.insert(LayerRole::Domain);
        roles.insert(LayerRole::Application);
        // Domain importing infrastructure is a dependency-inversion violation.
        let edges = vec![LayerEdge {
            from: LayerRole::Domain,
            to: LayerRole::Infrastructure,
        }];
        let report = detect_architecture(&roles, &edges);
        assert_eq!(report.solid_adherence, Some(false));
        // With the inward rule violated, it is not Clean — falls to Layered.
        assert_eq!(report.style, ArchitectureStyle::Layered);
    }

    #[test]
    fn unknown_when_no_role_signal() {
        let report = detect_architecture(&BTreeSet::new(), &[]);
        assert_eq!(report.style, ArchitectureStyle::Unknown);
        assert_eq!(report.style.as_str(), "unknown");
    }

    #[test]
    fn layered_when_two_unrelated_roles() {
        let mut roles = BTreeSet::new();
        roles.insert(LayerRole::Presentation);
        roles.insert(LayerRole::Infrastructure);
        let report = detect_architecture(&roles, &[]);
        assert_eq!(report.style, ArchitectureStyle::Layered);
    }

    #[test]
    fn on_disk_override_replaces_builtin() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".claude").join("vocab");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("architecture.toml"),
            r#"
[[role]]
role = "domain"
signals = ["kernel"]
"#,
        )
        .unwrap();
        let v = ArchitectureVocabulary::load(DEFAULT_ARCHITECTURE_NAME, tmp.path()).unwrap();
        assert_eq!(v.classify_segment("kernel"), Some(LayerRole::Domain));
        // Base wholesale-replaced: `domain` no longer classifies.
        assert_eq!(v.classify_segment("domain"), None);
    }

    #[test]
    fn load_default_falls_back_to_builtin_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let v = ArchitectureVocabulary::load(DEFAULT_ARCHITECTURE_NAME, tmp.path()).unwrap();
        assert_eq!(v.classify_segment("domain"), Some(LayerRole::Domain));
    }
}
