//! Scope selector for economy queries.
//!
//! [`EconomyScope`] is the single input every reader function in
//! [`economy::reader`](super::reader) takes. It expresses *which slice* of the
//! cost universe a caller is asking about — a single project's totals, a
//! single spec's slice of that project, a single wave within that spec, or a
//! union across several projects on disk.
//!
//! Keeping the scope as a first-class enum (instead of `Option<String>` triples
//! threaded through every signature) lets the writer/reader layer pattern-match
//! once and dispatch to the right SQL or to
//! [`multi_project::MultiProjectReader`](super::multi_project::MultiProjectReader),
//! and lets variants like [`EconomyScope::Windowed`] extend the API without
//! breaking callers — the enum is `#[non_exhaustive]`.
//!
//! [`EconomyScope::Windowed`] is the time dimension: it *composes* a
//! [`TimeWindow`] onto any other scope (never replacing it), so a caller can
//! ask for "this spec, last 7 days" by wrapping a [`EconomyScope::Spec`]. The
//! window restricts which events are folded (by timestamp); the wrapped scope
//! keeps deciding which project/spec/wave slice they belong to.
//!
//! The newtypes below ([`ProjectPath`], [`SpecId`], [`WaveId`], [`AgentId`])
//! exist for the same reason: stronger types at the API boundary prevent
//! accidental swaps (a spec id passed where a wave id was expected compiles to
//! a different error today than it did against three `String` parameters).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Filesystem path to a project root (the directory that owns
/// `.claude/.harness/mustard.db`).
///
/// Wrapped in a newtype so the API can distinguish a project path from any
/// other [`PathBuf`] at the type level. Hash/Eq are derived so it can be used
/// as a map key in [`multi_project`](super::multi_project) fan-out.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectPath(pub PathBuf);

impl ProjectPath {
    /// Build a [`ProjectPath`] from anything convertible to a [`PathBuf`].
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    /// Borrow the underlying path.
    #[must_use]
    pub fn as_path(&self) -> &std::path::Path {
        &self.0
    }
}

/// Spec identifier — the slug of the directory under `.claude/spec/active/`
/// (e.g. `"2026-05-20-economia-moat-unification"`).
///
/// Newtype so it cannot be confused with a [`WaveId`] or [`AgentId`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SpecId(pub String);

impl SpecId {
    /// Build a [`SpecId`] from anything convertible to a [`String`].
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Borrow the underlying id.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Wave identifier within a spec (e.g. `"wave-1-core-economy"`).
///
/// Newtype so reader functions can take `(SpecId, WaveId)` and a caller cannot
/// silently pass the wrong half.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WaveId(pub String);

impl WaveId {
    /// Build a [`WaveId`] from anything convertible to a [`String`].
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Borrow the underlying id.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Agent identifier — the role/skill name an event was attributed to
/// (e.g. `"core-impl"`, `"plan"`, `"explore"`).
///
/// Newtype so the per-agent breakdown in [`AgentCost`](super::model::AgentCost)
/// stays type-safe end-to-end.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentId(pub String);

impl AgentId {
    /// Build an [`AgentId`] from anything convertible to a [`String`].
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Borrow the underlying id.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An inclusive `[from, to]` time window, expressed as ISO-8601 bounds, used to
/// restrict an economy query to events whose timestamp falls inside it.
///
/// Both bounds are optional: `from = None` is unbounded-below, `to = None` is
/// unbounded-above, and both `None` means "no window" (every event is inside) —
/// which is what keeps the fail-open contract trivial. The bounds are ISO
/// strings (the shape the dashboard computes on its edge and sends over the
/// wire); [`bounds_ms`](TimeWindow::bounds_ms) resolves them to epoch-millis
/// once, tolerantly — a malformed bound degrades to "unbounded on that side"
/// rather than excluding everything.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TimeWindow {
    /// Inclusive lower bound as ISO-8601, or `None` for unbounded-below.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// Inclusive upper bound as ISO-8601, or `None` for unbounded-above.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
}

impl TimeWindow {
    /// Build a window from optional ISO-8601 `from` / `to` bounds.
    #[must_use]
    pub fn new(from: Option<String>, to: Option<String>) -> Self {
        Self { from, to }
    }

    /// Resolve the ISO bounds to inclusive epoch-millis `(from_ms, to_ms)`,
    /// parsed once. A bound that is absent OR fails to parse becomes `None`
    /// (unbounded on that side): the fail-open rule — a malformed window never
    /// excludes more than it can justify.
    #[must_use]
    pub fn bounds_ms(&self) -> (Option<i64>, Option<i64>) {
        let parse =
            |iso: &Option<String>| iso.as_deref().and_then(crate::platform::time::parse_iso_millis);
        (parse(&self.from), parse(&self.to))
    }
}

/// Which slice of the cost universe a query is asking about.
///
/// Every function in [`economy::reader`](super::reader) takes an
/// [`EconomyScope`] and dispatches by variant:
///
/// - [`EconomyScope::Project`] — totals for a single project DB.
/// - [`EconomyScope::Spec`] — slice of one project DB filtered to one spec.
/// - [`EconomyScope::Wave`] — slice of one project DB filtered to one wave
///   inside a spec.
/// - [`EconomyScope::AllProjects`] — fan-out across N project DBs and merge
///   the results via
///   [`MultiProjectReader`](super::multi_project::MultiProjectReader).
///
/// `#[non_exhaustive]` so variants like [`EconomyScope::Windowed`] can be added
/// without breaking downstream `match` arms — consumers must keep a wildcard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum EconomyScope {
    /// All costs recorded against a single project root.
    Project(ProjectPath),
    /// Costs filtered to a single spec inside a single project.
    Spec {
        /// The project root that owns the spec.
        project: ProjectPath,
        /// The spec slug.
        spec: SpecId,
    },
    /// Costs filtered to a single wave inside a single spec.
    Wave {
        /// The project root that owns the spec.
        project: ProjectPath,
        /// The spec slug.
        spec: SpecId,
        /// The wave slug.
        wave: WaveId,
    },
    /// Union of [`EconomyScope::Project`] across multiple roots, evaluated by
    /// [`MultiProjectReader`](super::multi_project::MultiProjectReader).
    AllProjects(Vec<ProjectPath>),
    /// A [`TimeWindow`] composed onto an inner selector. The window restricts
    /// which events are folded (by timestamp); the boxed `inner` scope keeps
    /// deciding which project/spec/wave slice they belong to. Composition, not
    /// replacement — the inner can be any other variant, including
    /// [`AllProjects`](EconomyScope::AllProjects).
    Windowed {
        /// Inclusive `[from, to]` bound applied to every event's timestamp.
        window: TimeWindow,
        /// The selector the window composes with.
        inner: Box<EconomyScope>,
    },
}

impl EconomyScope {

    /// The wave slug this scope is filtered to, if any.
    #[must_use]
    pub fn wave_filter(&self) -> Option<&WaveId> {
        match self {
            Self::Wave { wave, .. } => Some(wave),
            Self::Windowed { inner, .. } => inner.wave_filter(),
            _ => None,
        }
    }

    /// Compose a [`TimeWindow`] onto this scope, wrapping it in
    /// [`Self::Windowed`]. The selector is preserved (never replaced); the
    /// window only narrows which events are folded.
    #[must_use]
    pub fn with_window(self, window: TimeWindow) -> Self {
        Self::Windowed {
            window,
            inner: Box::new(self),
        }
    }

    /// Like [`Self::with_window`], but a `None` window returns the scope
    /// unchanged (no wrapper). Used to thread an optional window through the
    /// [`AllProjects`](Self::AllProjects) fan-out without allocating a wrapper
    /// when there is no window.
    #[must_use]
    pub fn with_maybe_window(self, window: Option<TimeWindow>) -> Self {
        match window {
            Some(w) => self.with_window(w),
            None => self,
        }
    }

    /// Split this scope into its optional time window and the underlying
    /// selector. A [`Self::Windowed`] yields `(Some(window), inner)`; every
    /// other variant yields `(None, self)` unchanged.
    ///
    /// Readers call this once at entry so the rest of their logic matches on the
    /// base selector (`Project` / `Spec` / `Wave` / `AllProjects`) exactly as
    /// before, then apply the window — if any — while folding events.
    #[must_use]
    pub fn into_parts(self) -> (Option<TimeWindow>, EconomyScope) {
        match self {
            Self::Windowed { window, inner } => (Some(window), *inner),
            other => (None, other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newtypes_serialize_transparently() {
        let id = SpecId::new("abc");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"abc\"");
        let back: SpecId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn window_composes_without_replacing_scope() {
        let base = EconomyScope::Spec {
            project: ProjectPath::new("/p"),
            spec: SpecId::new("s"),
        };
        let win = TimeWindow::new(
            Some("2026-05-01T00:00:00.000Z".to_string()),
            Some("2026-05-31T00:00:00.000Z".to_string()),
        );
        // Wrapping preserves the inner selector — composition, not replacement.
        let (got_win, inner) = base.clone().with_window(win.clone()).into_parts();
        assert_eq!(got_win, Some(win));
        assert_eq!(inner, base);
        // A windowed scope still reports the inner wave filter.
        assert!(inner.wave_filter().is_none());
    }

    #[test]
    fn into_parts_on_unwindowed_scope_yields_no_window() {
        let base = EconomyScope::Project(ProjectPath::new("/p"));
        let (win, inner) = base.clone().into_parts();
        assert!(win.is_none());
        assert_eq!(inner, base);
    }

    #[test]
    fn with_maybe_window_none_leaves_scope_unwrapped() {
        let base = EconomyScope::Project(ProjectPath::new("/p"));
        assert_eq!(base.clone().with_maybe_window(None), base);
    }

    #[test]
    fn bounds_ms_parses_iso_and_degrades_on_malformed() {
        let win = TimeWindow::new(
            Some("2026-05-20T00:00:00.000Z".to_string()),
            Some("nonsense".to_string()),
        );
        let (from, to) = win.bounds_ms();
        assert!(from.is_some());
        assert!(
            to.is_none(),
            "an unparseable bound degrades to unbounded (fail-open)"
        );
        // An empty window is fully unbounded.
        assert_eq!(TimeWindow::default().bounds_ms(), (None, None));
    }
}
