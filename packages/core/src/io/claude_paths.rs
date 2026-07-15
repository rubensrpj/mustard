//! `claude_paths` — the single source of truth for every path under a
//! project's `.claude/` directory.
//!
//! ## Why
//!
//! Before this module, ~33 call-sites inside `apps/rt` open-coded their own
//! `root.join(".claude").join("...")` expressions, with three recurring
//! problems:
//!
//! - **Drift.** Some sites baked in `.claude/spec/{name}/wave-plan.md`, others
//!   `.claude/spec/{name}/wave-N-{role}/spec.md` with subtle slug variants.
//! - **Double-nesting.** A handful of call-sites accidentally re-applied
//!   `.join(".claude")` on top of a path that was already inside `.claude/`,
//!   producing the forbidden `.claude/.claude/` sequence. Guard I1 below
//!   exists to make this a typed error rather than silent corruption.
//! - **No catalog.** `claude_dir_prune` and `doctor` both maintained their own
//!   private lists of "known" directories / cache files — every new entry had
//!   to be added in three places.
//!
//! This module replaces all three failures with a single typed handle. Every
//! consumer in [`apps/rt`] calls [`ClaudePaths::for_project`] once, then asks
//! for the path it needs via a typed accessor.
//!
//! ## Canonical tree
//!
//! ```text
//! <root>/
//! ├── CLAUDE.md
//! ├── settings.json
//! ├── mustard.json
//! ├── grain.model.json
//! ├── pipeline-config.md
//! ├── .cache/
//! │   ├── detect.json
//! │   ├── scan-dispatch.json
//! │   └── knowledge-seen.json
//! ├── .harness/
//! ├── .metrics/
//! ├── .agent-state/
//! ├── .obsidian/
//! ├── commands/
//! ├── skills/
//! ├── refs/
//! ├── agents/
//! ├── agent-memory/
//! ├── graph/
//! ├── capabilities/
//! └── spec/
//!     └── {name}/
//!         ├── spec.md
//!         ├── meta.json
//!         ├── wave-plan.md
//!         ├── qa-report.json
//!         ├── qa-report.html
//!         ├── adr/
//!         ├── .events/
//!         ├── .blobs/
//!         └── wave-N-{role}/
//!             ├── spec.md
//!             ├── meta.json
//!             ├── diff.md
//!             ├── prompt.md
//!             ├── warnings.txt
//!             └── qa-report.json
//! ```
//!
//! ## Inviolable safety contract
//!
//! - **No `.claude/.claude/`.** [`ClaudePaths::for_project`] applies a
//!   defensive guard (I1): if the path it is handed terminates in `.claude`
//!   or contains the sequence `.claude/.claude/` anywhere, it returns
//!   [`ClaudePathsError::ForbiddenDotClaudeDotClaude`]. The canonical
//!   resolver lives in [`crate::io::workspace::workspace_root`]; this guard is
//!   defence-in-depth for the case where a future call-site bypasses it.
//! - **Validated names.** [`ClaudePaths::for_spec`] rejects empty spec names,
//!   `/` separators, and `..` traversal so a malformed user input cannot
//!   escape the spec sub-tree.
//! - **Validated wave slugs.** [`SpecPaths::for_wave`] enforces the
//!   `wave-<digits>(-<lowercase-role>)?` shape so wave directories stay
//!   uniform across the registry, the dashboard, and the QA gate.
//! - **Idempotent.** Every accessor recomputes from the stored root each
//!   call; identical inputs yield identical [`PathBuf`] outputs.

use std::path::{Path, PathBuf};

/// Errors returned by [`ClaudePaths`] constructors.
#[derive(Debug, thiserror::Error)]
pub enum ClaudePathsError {
    /// The path passed to [`ClaudePaths::for_project`] would produce a
    /// `.claude/.claude/` nesting. Either it terminates in `.claude` or
    /// contains the literal `.claude/.claude/` segment.
    #[error("path contains forbidden .claude/.claude/ sequence or terminates in .claude: {0:?}")]
    ForbiddenDotClaudeDotClaude(PathBuf),

    /// A spec name was empty.
    #[error("spec name is empty")]
    EmptySpecName,

    /// A spec name contained `/` or `\\` — only flat slugs are allowed.
    #[error("spec name contains path separator: {0:?}")]
    SpecNameHasSeparator(String),

    /// A spec name contained `..` — traversal is forbidden.
    #[error("spec name contains traversal segment '..': {0:?}")]
    SpecNameTraversal(String),

    /// A wave slug did not match `wave-<digits>(-<lowercase-role>)?`.
    #[error("wave slug does not match wave-<n>[-role]: {0:?}")]
    InvalidWaveSlug(String),
}

/// The canonical handle on a project's `.claude/` tree.
///
/// Build with [`ClaudePaths::for_project`]. Every accessor is pure: given the
/// same `root`, it always returns the same [`PathBuf`].
#[derive(Debug, Clone)]
pub struct ClaudePaths {
    /// The project root — the directory that *contains* `.claude/` and
    /// `mustard.json`. Never ends in `.claude`.
    root: PathBuf,
}

/// A handle on `<root>/.claude/spec/<name>/`. Build via
/// [`ClaudePaths::for_spec`].
#[derive(Debug, Clone)]
pub struct SpecPaths {
    /// The owning [`ClaudePaths`] root, kept for nested constructors.
    root: PathBuf,
    /// The spec directory itself (`<root>/.claude/spec/<name>/`).
    spec_dir: PathBuf,
    /// The spec slug (`<name>`).
    spec_name: String,
}

/// A handle on `<root>/.claude/spec/<name>/<wave-slug>/`. Build via
/// [`SpecPaths::for_wave`].
#[derive(Debug, Clone)]
pub struct WavePaths {
    /// The owning [`SpecPaths`], for chained accessors.
    spec: SpecPaths,
    /// The wave directory itself.
    wave_dir: PathBuf,
    /// The wave slug (`wave-1-rt` / `wave-2` / …).
    wave_slug: String,
}

/// Top-level directory names under `<root>/.claude/`. The list is kept in one
/// place so [`crate::io::claude_paths`] consumers (notably `claude_dir_prune`) can
/// derive their catalog from it instead of hand-maintaining a duplicate.
///
/// `.pipeline-states` is included because [`ClaudePaths::pipeline_states_dir`]
/// exposes it as a first-class accessor — every dir reachable through a
/// `&self` method on `ClaudePaths` MUST appear here, or the
/// `doctor --check claude-paths` audit will flag it as unexpected.
const DOCUMENTED_DIRS: &[&str] = &[
    ".cache",
    ".harness",
    ".metrics",
    ".agent-state",
    ".obsidian",
    ".pipeline-states",
    "commands",
    "skills",
    "refs",
    "agents",
    "agent-memory",
    "spec",
    "graph",
    "capabilities",
    // Plan-mode plan files — `settings.json#plansDirectory` points here.
    "plans",
];

/// File names under `<root>/.claude/.cache/` that Mustard owns. Single source
/// for the `doctor` cache-orphan check.
const CACHE_FILES: &[&str] = &[
    "detect.json",
    "scan-dispatch.json",
    "knowledge-seen.json",
];

impl ClaudePaths {
    /// Build a handle pointing at `<root>/.claude/`.
    ///
    /// # Errors
    ///
    /// Returns [`ClaudePathsError::ForbiddenDotClaudeDotClaude`] when `root`
    /// terminates in `.claude` or contains the sequence `.claude/.claude/`
    /// anywhere. This is the I1 defensive guard — the canonical resolver
    /// [`crate::io::workspace::workspace_root`] should already have caught the
    /// problem upstream.
    pub fn for_project(root: impl AsRef<Path>) -> Result<Self, ClaudePathsError> {
        let root = root.as_ref().to_path_buf();
        if violates_dot_claude_guard(&root) {
            // In a debug build this is a programming error somewhere
            // upstream — fire a `debug_assert!` so accidental violations
            // show up loudly during development. Suppressed under
            // `#[cfg(test)]` so the negative tests below can exercise the
            // typed-error path without panicking.
            #[cfg(all(debug_assertions, not(test)))]
            {
                let rendered = root.display().to_string();
                debug_assert!(
                    false,
                    "ClaudePaths::for_project received a .claude-nested path: {rendered}"
                );
            }
            return Err(ClaudePathsError::ForbiddenDotClaudeDotClaude(root));
        }
        Ok(Self { root })
    }

    /// Build a handle without running the I1 guard.
    ///
    /// **Fail-open callers only.** This bypass exists so a fallback branch in
    /// telemetry/event paths can keep using the same typed accessor surface
    /// as the happy path after `ClaudePaths::for_project(..).ok()` rejected
    /// the root. Production code that is not a fail-open fallback **must**
    /// use [`Self::for_project`] so I1 violations are surfaced rather than
    /// silently materialised into `.claude/.claude/` paths.
    ///
    /// AC-TF1 of `2026-05-26-w2-residuals-50-unlisted-apps-rt` rewards
    /// preserving the helper surface even on the fallback branch — replacing
    /// open-coded `project.join(".claude").join("…")` strings with accessor
    /// calls over a `compose_unchecked(project)` handle.
    #[must_use]
    pub fn compose_unchecked(project: impl AsRef<Path>) -> Self {
        Self {
            root: project.as_ref().to_path_buf(),
        }
    }

    /// The project root that was passed to [`Self::for_project`].
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `<root>/.claude/` — the parent of every other accessor below.
    #[must_use]
    pub fn claude_dir(&self) -> PathBuf {
        self.root.join(".claude")
    }

    // -- top-level directories -------------------------------------------

    /// `<root>/.claude/.cache/` — Mustard-owned scratch JSON.
    #[must_use]
    pub fn cache_dir(&self) -> PathBuf {
        self.claude_dir().join(".cache")
    }

    /// `<root>/.claude/.harness/` — harness event-bus working state.
    #[must_use]
    pub fn harness_dir(&self) -> PathBuf {
        self.claude_dir().join(".harness")
    }

    /// `<root>/.claude/.metrics/` — telemetry rollups.
    #[must_use]
    pub fn metrics_dir(&self) -> PathBuf {
        self.claude_dir().join(".metrics")
    }

    /// `<root>/.claude/.agent-state/` — per-agent durable state.
    #[must_use]
    pub fn agent_state_dir(&self) -> PathBuf {
        self.claude_dir().join(".agent-state")
    }

    /// `<root>/.claude/commands/` — namespaced slash commands.
    #[must_use]
    pub fn commands_dir(&self) -> PathBuf {
        self.claude_dir().join("commands")
    }

    /// `<root>/.claude/skills/` — foundation skills.
    #[must_use]
    pub fn skills_dir(&self) -> PathBuf {
        self.claude_dir().join("skills")
    }

    /// `<root>/.claude/agent-memory/` — persistent agent memory files.
    #[must_use]
    pub fn agent_memory_dir(&self) -> PathBuf {
        self.claude_dir().join("agent-memory")
    }

    /// `<root>/.claude/spec/` — the parent of every per-spec directory.
    #[must_use]
    pub fn spec_dir(&self) -> PathBuf {
        self.claude_dir().join("spec")
    }

    /// `<root>/.claude/graph/` — graph artifacts (entity registry follow-up).
    #[must_use]
    pub fn graph_dir(&self) -> PathBuf {
        self.claude_dir().join("graph")
    }

    /// `<root>/.claude/capabilities/` — durable capability docs
    /// (`cap.{slug}.md`). Parent of every `.claude/capabilities/{slug}.md`
    /// authored by `mustard-rt run capability create`.
    #[must_use]
    pub fn capabilities_dir(&self) -> PathBuf {
        self.claude_dir().join("capabilities")
    }

    /// `<root>/.claude/.pipeline-states/` — legacy pipeline-state JSON
    /// directory.
    ///
    /// **Note:** the per-wave / per-spec artefacts (`diff.md`, `prompt.md`,
    /// `warnings.txt`, `qa-report.{json,html}`) have
    /// moved into the per-spec / per-wave directories under [`Self::spec_dir`].
    /// This accessor remains for the *pipeline-state JSON files themselves*
    /// (`{spec}.json` markers).
    /// Active pipeline-state tracking writes here today; future work may move
    /// these to a per-spec destination, but that migration is out of scope
    /// for W2 of `2026-05-26-claude-paths-single-source`.
    #[must_use]
    pub fn pipeline_states_dir(&self) -> PathBuf {
        self.claude_dir().join(".pipeline-states")
    }

    /// `<root>/.claude/.pipeline-states/{spec}.json` — per-spec pipeline-state
    /// marker file.
    ///
    /// See [`Self::pipeline_states_dir`] for the legacy-vs-future tradeoff.
    #[must_use]
    pub fn pipeline_state_file(&self, spec: &str) -> PathBuf {
        self.pipeline_states_dir().join(format!("{spec}.json"))
    }

    // -- root-level files ------------------------------------------------

    /// `<root>/.claude/CLAUDE.md` — orchestrator rules.
    #[must_use]
    pub fn claude_md_path(&self) -> PathBuf {
        self.claude_dir().join("CLAUDE.md")
    }

    /// `<root>/.claude/settings.json` — hook wiring + permissions.
    #[must_use]
    pub fn settings_json_path(&self) -> PathBuf {
        self.claude_dir().join("settings.json")
    }

    /// `<root>/mustard.json` — Mustard project config (git flow, build/test
    /// commands, `specLang`, `tone`, runtime/version stamp).
    ///
    /// Lives at the **project root**, not under `.claude/`: it is the workspace
    /// anchor [`crate::io::workspace::workspace_root`] keys on, and it is
    /// user-facing, version-controlled config — the opposite of the ephemeral
    /// (often gitignored) state that fills `.claude/`. This is the single
    /// source of truth for the file's location; callers must not open-code
    /// `root.join("mustard.json")`.
    #[must_use]
    pub fn mustard_json_path(&self) -> PathBuf {
        self.root.join("mustard.json")
    }

    /// `<root>/.claude/pipeline-config.md` — long-form pipeline rules.
    #[must_use]
    pub fn pipeline_config_md_path(&self) -> PathBuf {
        self.claude_dir().join("pipeline-config.md")
    }

    // -- cache files -----------------------------------------------------

    /// `<root>/.claude/.cache/detect.json` — subproject-detection cache.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn detect_cache_path(&self) -> PathBuf {
        self.cache_dir().join("detect.json")
    }

    /// `<root>/.claude/.cache/knowledge-seen.json` — knowledge ingestion
    /// dedupe marker.
    #[must_use]
    pub fn knowledge_seen_path(&self) -> PathBuf {
        self.cache_dir().join("knowledge-seen.json")
    }

    // -- catalogs --------------------------------------------------------

    /// List of every top-level directory under `<root>/.claude/` that
    /// Mustard documents. Consumed by `claude_dir_prune::DOCUMENTED_DIRS`.
    #[must_use]
    pub fn documented_dirs() -> Vec<&'static str> {
        DOCUMENTED_DIRS.to_vec()
    }

    /// List of every file under `<root>/.claude/.cache/` that Mustard owns.
    /// Consumed by `doctor` for the cache-orphan check.
    #[must_use]
    pub fn cache_files() -> Vec<&'static str> {
        CACHE_FILES.to_vec()
    }

    /// Walk `<root>/.claude/` and return every direct child that is **not**
    /// in [`Self::documented_dirs`] (top-level) plus every cache file under
    /// `.cache/` that is not in [`Self::cache_files`].
    ///
    /// Fail-open: a missing `.claude/` returns an empty vector rather than
    /// erroring — callers (the `doctor` face) treat absence as "nothing to
    /// audit".
    #[must_use]
    pub fn audit_orphans(&self) -> Vec<PathBuf> {
        let mut orphans = Vec::new();
        let documented: std::collections::HashSet<&str> =
            DOCUMENTED_DIRS.iter().copied().collect();
        let claude = self.claude_dir();
        if let Ok(read) = std::fs::read_dir(&claude) {
            for entry in read.flatten() {
                let name = entry.file_name();
                let Some(name_str) = name.to_str() else { continue };
                // Top-level files belong in the root-file accessors; skip
                // them here (they are not "directories Mustard documents").
                let Ok(ty) = entry.file_type() else { continue };
                if !ty.is_dir() {
                    continue;
                }
                if !documented.contains(name_str) {
                    orphans.push(entry.path());
                }
            }
        }
        let cache_files: std::collections::HashSet<&str> = CACHE_FILES.iter().copied().collect();
        if let Ok(read) = std::fs::read_dir(self.cache_dir()) {
            for entry in read.flatten() {
                let name = entry.file_name();
                let Some(name_str) = name.to_str() else { continue };
                let Ok(ty) = entry.file_type() else { continue };
                if !ty.is_file() {
                    continue;
                }
                if !cache_files.contains(name_str) {
                    orphans.push(entry.path());
                }
            }
        }
        orphans
    }

    // -- nested constructors --------------------------------------------

    /// Build a [`SpecPaths`] for `<root>/.claude/spec/<name>/`.
    ///
    /// # Errors
    ///
    /// Returns [`ClaudePathsError::EmptySpecName`] when `name` is empty,
    /// [`ClaudePathsError::SpecNameHasSeparator`] when `name` contains a
    /// path separator, and [`ClaudePathsError::SpecNameTraversal`] when
    /// `name` contains `..`.
    pub fn for_spec(&self, name: &str) -> Result<SpecPaths, ClaudePathsError> {
        if name.is_empty() {
            return Err(ClaudePathsError::EmptySpecName);
        }
        if name.contains('/') || name.contains('\\') {
            return Err(ClaudePathsError::SpecNameHasSeparator(name.to_string()));
        }
        // `..` as a full segment OR embedded — both are unsafe.
        if name == ".." || name.split(['/', '\\']).any(|s| s == "..") || name.contains("..") {
            return Err(ClaudePathsError::SpecNameTraversal(name.to_string()));
        }
        let spec_dir = self.spec_dir().join(name);
        Ok(SpecPaths {
            root: self.root.clone(),
            spec_dir,
            spec_name: name.to_string(),
        })
    }
}

impl SpecPaths {
    /// The owning project root.
    #[must_use]
    pub fn project_root(&self) -> &Path {
        &self.root
    }

    /// The spec directory itself.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.spec_dir
    }

    /// The spec slug.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.spec_name
    }

    /// `<spec>/spec.md` — the spec narrative.
    #[must_use]
    pub fn spec_md_path(&self) -> PathBuf {
        self.spec_dir.join("spec.md")
    }

    /// `<spec>/meta.json` — sidecar lifecycle metadata.
    #[must_use]
    pub fn meta_json_path(&self) -> PathBuf {
        self.spec_dir.join("meta.json")
    }

    /// `<spec>/wave-plan.md` — top-level wave plan (when this spec is a
    /// multi-wave epic).
    #[must_use]
    pub fn wave_plan_md_path(&self) -> PathBuf {
        self.spec_dir.join("wave-plan.md")
    }

    /// `<spec>/.events/` — per-spec NDJSON event log (per the
    /// `2026-05-23-per-spec-event-log-claude-devtools` spec).
    #[must_use]
    pub fn events_dir(&self) -> PathBuf {
        self.spec_dir.join(".events")
    }

    /// `<spec>/qa-report.json` — aggregate (per-spec) QA report.
    #[must_use]
    pub fn qa_report_json_path(&self) -> PathBuf {
        self.spec_dir.join("qa-report.json")
    }

    /// `<spec>/qa-report.html` — aggregate (per-spec) QA report rendered.
    #[must_use]
    pub fn qa_report_html_path(&self) -> PathBuf {
        self.spec_dir.join("qa-report.html")
    }

    /// Build a [`WavePaths`] for `<spec>/<wave-slug>/`.
    ///
    /// # Errors
    ///
    /// Returns [`ClaudePathsError::InvalidWaveSlug`] when `wave_slug` does
    /// not match `wave-<digits>(-<lowercase-role>)?`.
    pub fn for_wave(&self, wave_slug: &str) -> Result<WavePaths, ClaudePathsError> {
        if !is_valid_wave_slug(wave_slug) {
            return Err(ClaudePathsError::InvalidWaveSlug(wave_slug.to_string()));
        }
        let wave_dir = self.spec_dir.join(wave_slug);
        Ok(WavePaths {
            spec: self.clone(),
            wave_dir,
            wave_slug: wave_slug.to_string(),
        })
    }
}

impl WavePaths {
    /// The owning [`SpecPaths`].
    #[must_use]
    pub fn spec(&self) -> &SpecPaths {
        &self.spec
    }

    /// The wave directory itself.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.wave_dir
    }

    /// The wave slug.
    #[must_use]
    pub fn slug(&self) -> &str {
        &self.wave_slug
    }

    /// `<wave>/spec.md` — per-wave spec narrative.
    #[must_use]
    pub fn spec_md_path(&self) -> PathBuf {
        self.wave_dir.join("spec.md")
    }

    /// `<wave>/meta.json` — per-wave lifecycle metadata.
    #[must_use]
    pub fn meta_json_path(&self) -> PathBuf {
        self.wave_dir.join("meta.json")
    }

    /// `<wave>/diff.md` — captured diff for this wave's edits.
    #[must_use]
    pub fn diff_md_path(&self) -> PathBuf {
        self.wave_dir.join("diff.md")
    }

    /// `<wave>/qa-report.json` — per-wave QA report (distinct from the
    /// per-spec aggregate at [`SpecPaths::qa_report_json_path`]).
    #[must_use]
    pub fn qa_report_json_path(&self) -> PathBuf {
        self.wave_dir.join("qa-report.json")
    }
}

// -- helpers ------------------------------------------------------------

/// I1 guard: a project root must never terminate in `.claude` and must never
/// contain the sub-sequence `.claude/.claude/`.
fn violates_dot_claude_guard(path: &Path) -> bool {
    let last_is_dot_claude =
        path.file_name().and_then(|s| s.to_str()) == Some(".claude");
    if last_is_dot_claude {
        return true;
    }
    // Normalise separators for the substring test so the guard works on both
    // POSIX and Windows path strings.
    let as_string = path.to_string_lossy().replace('\\', "/");
    as_string.contains(".claude/.claude/") || as_string.ends_with(".claude/.claude")
}

/// Wave slugs are `wave-<digits>(-<lowercase-role>)?`. Examples:
/// `wave-1`, `wave-2-rt`, `wave-12-mixed`.
fn is_valid_wave_slug(slug: &str) -> bool {
    // Hand-rolled to avoid pulling in `regex`.
    let Some(rest) = slug.strip_prefix("wave-") else {
        return false;
    };
    let (digits, tail) = rest
        .find('-')
        .map_or((rest, ""), |idx| (&rest[..idx], &rest[idx + 1..]));
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    if tail.is_empty() {
        // `wave-N` — fine, with no role.
        return slug == format!("wave-{digits}");
    }
    // `tail` must be ASCII-lowercase + digits, no further hyphens.
    tail.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn for_project_sets_root() {
        let dir = tempdir().unwrap();
        let cp = ClaudePaths::for_project(dir.path()).unwrap();
        assert_eq!(cp.root(), dir.path());
    }

    #[test]
    fn compose_unchecked_skips_i1_guard() {
        let dir = tempdir().unwrap();
        let bad = dir.path().join(".claude");
        // `for_project` rejects this path…
        assert!(ClaudePaths::for_project(&bad).is_err());
        // …but `compose_unchecked` produces a usable handle for fail-open
        // fallback paths. The handle materialises canonical sub-paths from
        // the (already-nested) root — the consumer's job is to recognise
        // they are in the fallback branch.
        let cp = ClaudePaths::compose_unchecked(&bad);
        assert_eq!(cp.root(), bad.as_path());
        // Cache accessor still produces a deterministic shape.
        assert_eq!(cp.cache_dir(), bad.join(".claude").join(".cache"));
    }

    #[test]
    fn for_project_rejects_terminal_dot_claude() {
        let dir = tempdir().unwrap();
        let bad = dir.path().join(".claude");
        let err = ClaudePaths::for_project(&bad).unwrap_err();
        assert!(matches!(
            err,
            ClaudePathsError::ForbiddenDotClaudeDotClaude(_)
        ));
    }

    #[test]
    fn for_project_rejects_dot_claude_dot_claude_sequence() {
        let dir = tempdir().unwrap();
        let bad = dir.path().join(".claude").join(".claude");
        let err = ClaudePaths::for_project(&bad).unwrap_err();
        assert!(matches!(
            err,
            ClaudePathsError::ForbiddenDotClaudeDotClaude(_)
        ));
    }

    #[test]
    fn cache_dir_under_root_dot_cache() {
        let dir = tempdir().unwrap();
        let cp = ClaudePaths::for_project(dir.path()).unwrap();
        assert_eq!(cp.cache_dir(), dir.path().join(".claude").join(".cache"));
        assert_eq!(
            cp.detect_cache_path(),
            dir.path().join(".claude").join(".cache").join("detect.json")
        );
    }

    #[test]
    fn for_spec_rejects_empty_name() {
        let dir = tempdir().unwrap();
        let cp = ClaudePaths::for_project(dir.path()).unwrap();
        let err = cp.for_spec("").unwrap_err();
        assert!(matches!(err, ClaudePathsError::EmptySpecName));
    }

    #[test]
    fn for_spec_rejects_path_traversal() {
        let dir = tempdir().unwrap();
        let cp = ClaudePaths::for_project(dir.path()).unwrap();
        // forward slash separator
        let err = cp.for_spec("foo/bar").unwrap_err();
        assert!(matches!(err, ClaudePathsError::SpecNameHasSeparator(_)));
        // backslash separator (Windows)
        let err = cp.for_spec("foo\\bar").unwrap_err();
        assert!(matches!(err, ClaudePathsError::SpecNameHasSeparator(_)));
        // `..` segment
        let err = cp.for_spec("..").unwrap_err();
        assert!(matches!(err, ClaudePathsError::SpecNameTraversal(_)));
        // embedded `..`
        let err = cp.for_spec("foo..bar").unwrap_err();
        assert!(matches!(err, ClaudePathsError::SpecNameTraversal(_)));
    }

    #[test]
    fn for_wave_rejects_malformed_slug() {
        let dir = tempdir().unwrap();
        let cp = ClaudePaths::for_project(dir.path()).unwrap();
        let sp = cp.for_spec("my-spec").unwrap();
        // missing `wave-` prefix
        assert!(matches!(
            sp.for_wave("1-rt"),
            Err(ClaudePathsError::InvalidWaveSlug(_))
        ));
        // missing digits
        assert!(matches!(
            sp.for_wave("wave-"),
            Err(ClaudePathsError::InvalidWaveSlug(_))
        ));
        // non-digit number
        assert!(matches!(
            sp.for_wave("wave-X-rt"),
            Err(ClaudePathsError::InvalidWaveSlug(_))
        ));
        // uppercase role
        assert!(matches!(
            sp.for_wave("wave-1-RT"),
            Err(ClaudePathsError::InvalidWaveSlug(_))
        ));
        // valid cases — must succeed
        assert!(sp.for_wave("wave-1").is_ok());
        assert!(sp.for_wave("wave-2-rt").is_ok());
        assert!(sp.for_wave("wave-12-mixed").is_ok());
    }

    #[test]
    fn documented_dirs_includes_all_top_level_dirs() {
        let dirs = ClaudePaths::documented_dirs();
        for expected in [
            ".cache",
            ".harness",
            ".metrics",
            ".agent-state",
            ".obsidian",
            ".pipeline-states",
            "commands",
            "skills",
            "refs",
            "agents",
            "agent-memory",
            "spec",
            "graph",
            "capabilities",
            "plans",
        ] {
            assert!(dirs.contains(&expected), "missing {expected} from documented_dirs");
        }
    }

    #[test]
    fn cache_files_lists_three_caches() {
        let files = ClaudePaths::cache_files();
        assert_eq!(files.len(), 3);
        for expected in [
            "detect.json",
            "scan-dispatch.json",
            "knowledge-seen.json",
        ] {
            assert!(files.contains(&expected), "missing {expected} from cache_files");
        }
    }

    #[test]
    fn paths_are_idempotent() {
        let dir = tempdir().unwrap();
        let cp = ClaudePaths::for_project(dir.path()).unwrap();
        assert_eq!(cp.cache_dir(), cp.cache_dir());
        assert_eq!(cp.claude_md_path(), cp.claude_md_path());
        let sp = cp.for_spec("my-spec").unwrap();
        assert_eq!(sp.spec_md_path(), sp.spec_md_path());
        let wp = sp.for_wave("wave-1-rt").unwrap();
        assert_eq!(wp.diff_md_path(), wp.diff_md_path());
    }

    #[test]
    fn audit_orphans_returns_empty_on_clean_tree() {
        let dir = tempdir().unwrap();
        let cp = ClaudePaths::for_project(dir.path()).unwrap();
        // Build only documented children — no orphans expected.
        let claude = cp.claude_dir();
        std::fs::create_dir_all(&claude).unwrap();
        for d in ClaudePaths::documented_dirs() {
            std::fs::create_dir_all(claude.join(d)).unwrap();
        }
        // Drop one expected cache file so the cache pass exercises its scan
        // and still finds no orphan.
        std::fs::write(cp.detect_cache_path(), b"{}").unwrap();
        let orphans = cp.audit_orphans();
        assert!(orphans.is_empty(), "expected no orphans, got {orphans:?}");
    }

    #[test]
    fn audit_orphans_flags_unknown_dir_and_cache_file() {
        let dir = tempdir().unwrap();
        let cp = ClaudePaths::for_project(dir.path()).unwrap();
        let claude = cp.claude_dir();
        std::fs::create_dir_all(&claude).unwrap();
        // Plant one undocumented top-level directory and one undocumented
        // cache file.
        std::fs::create_dir_all(claude.join("legacy-bucket")).unwrap();
        std::fs::create_dir_all(cp.cache_dir()).unwrap();
        std::fs::write(cp.cache_dir().join("stale.json"), b"{}").unwrap();
        let orphans = cp.audit_orphans();
        assert_eq!(orphans.len(), 2, "got {orphans:?}");
    }

    #[test]
    fn spec_and_wave_paths_use_canonical_layout() {
        let dir = tempdir().unwrap();
        let cp = ClaudePaths::for_project(dir.path()).unwrap();
        let sp = cp.for_spec("2026-05-26-claude-paths").unwrap();
        assert!(sp.spec_md_path().ends_with("spec.md"));
        assert!(sp.meta_json_path().ends_with("meta.json"));
        assert!(sp.wave_plan_md_path().ends_with("wave-plan.md"));
        assert!(sp.events_dir().ends_with(".events"));
        let wp = sp.for_wave("wave-1-rt").unwrap();
        assert!(wp.diff_md_path().ends_with("diff.md"));
        assert!(wp.qa_report_json_path().ends_with("qa-report.json"));
    }
}
