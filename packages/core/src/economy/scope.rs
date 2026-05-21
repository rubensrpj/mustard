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
//! and lets future variants (e.g. `TimeWindow`) extend the API without breaking
//! callers — the enum is `#[non_exhaustive]`.
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
/// `#[non_exhaustive]` so a future `TimeWindow` variant can be added without
/// breaking downstream `match` arms — consumers must keep a wildcard.
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
}

impl EconomyScope {
    /// The project paths this scope covers.
    ///
    /// Returns a single-element slice for the single-project variants and the
    /// full list for [`EconomyScope::AllProjects`]. Useful for the read-only
    /// fan-out loop in [`MultiProjectReader`](super::multi_project::MultiProjectReader).
    #[must_use]
    pub fn project_paths(&self) -> Vec<&ProjectPath> {
        match self {
            Self::Project(p) | Self::Spec { project: p, .. } | Self::Wave { project: p, .. } => {
                vec![p]
            }
            Self::AllProjects(list) => list.iter().collect(),
        }
    }

    /// The spec slug this scope is filtered to, if any.
    #[must_use]
    pub fn spec_filter(&self) -> Option<&SpecId> {
        match self {
            Self::Spec { spec, .. } | Self::Wave { spec, .. } => Some(spec),
            Self::Project(_) | Self::AllProjects(_) => None,
        }
    }

    /// The wave slug this scope is filtered to, if any.
    #[must_use]
    pub fn wave_filter(&self) -> Option<&WaveId> {
        match self {
            Self::Wave { wave, .. } => Some(wave),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_paths_singleton_for_project_variant() {
        let scope = EconomyScope::Project(ProjectPath::new("/tmp/a"));
        let paths = scope.project_paths();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].as_path(), std::path::Path::new("/tmp/a"));
    }

    #[test]
    fn project_paths_fans_out_for_all_projects() {
        let scope = EconomyScope::AllProjects(vec![
            ProjectPath::new("/tmp/a"),
            ProjectPath::new("/tmp/b"),
        ]);
        assert_eq!(scope.project_paths().len(), 2);
    }

    #[test]
    fn spec_filter_is_set_for_spec_and_wave_variants() {
        let project = ProjectPath::new("/tmp/a");
        let spec = SpecId::new("feature-x");
        let scope = EconomyScope::Spec {
            project: project.clone(),
            spec: spec.clone(),
        };
        assert_eq!(scope.spec_filter(), Some(&spec));
        assert!(scope.wave_filter().is_none());

        let scope_wave = EconomyScope::Wave {
            project,
            spec: spec.clone(),
            wave: WaveId::new("w1"),
        };
        assert_eq!(scope_wave.spec_filter(), Some(&spec));
        assert_eq!(scope_wave.wave_filter().map(WaveId::as_str), Some("w1"));
    }

    #[test]
    fn newtypes_serialize_transparently() {
        let id = SpecId::new("abc");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"abc\"");
        let back: SpecId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }
}
