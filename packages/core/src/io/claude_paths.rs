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
//!   producing the forbidden `.claude/.claude/` sequence. The guard below
//!   exists to make this a typed error rather than silent corruption.
//! - **No catalog.** Cada consumidor mantinha a sua própria lista privada de
//!   pastas "conhecidas" — cada entrada nova tinha de ser acrescentada em
//!   três lugares.
//!
//! This module replaces all three failures with a single typed handle. Every
//! consumer in [`apps/rt`] calls [`ClaudePaths::for_project`] once, then asks
//! for the path it needs via a typed accessor.
//!
//! ## Canonical tree
//!
//! ```text
//! <root>/
//! ├── settings.json
//! ├── mustard.json
//! ├── grain.db
//! ├── .cache/
//! ├── .harness/
//! ├── .metrics/
//! ├── .agent-state/
//! ├── .obsidian/
//! ├── mustard/
//! ├── commands/
//! ├── skills/
//! ├── refs/
//! ├── agents/
//! ├── agent-memory/
//! ├── graph/
//! ├── capabilities/
//! └── spec/
//!     └── {name}/
//!         ├── spec.ndjson
//!         ├── spec.md
//!         ├── spec.html
//!         └── meta.json
//! ```
//!
//! ## Inviolable safety contract
//!
//! - **No `.claude/.claude/`.** [`ClaudePaths::for_project`] applies a
//!   defensive guard: if the path it is handed terminates in `.claude`
//!   or contains the sequence `.claude/.claude/` anywhere, it returns
//!   [`ClaudePathsError::ForbiddenDotClaudeDotClaude`]. The canonical
//!   resolver lives in [`crate::io::workspace::workspace_root`]; this guard is
//!   defence-in-depth for the case where a future call-site bypasses it.
//! - **Validated names.** [`ClaudePaths::for_spec`] rejects empty spec names,
//!   `/` separators, and `..` traversal so a malformed user input cannot
//!   escape the spec sub-tree.
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
    /// The spec directory itself (`<root>/.claude/spec/<name>/`).
    spec_dir: PathBuf,
}

/// Top-level directory names under `<root>/.claude/`. A lista mora num lugar
/// só para o semeador do projeto derivar dela as regras de exclusão da
/// instalação privada, em vez de manter uma cópia à mão.
///
/// Toda pasta alcançável por um método `&self` de `ClaudePaths` PRECISA
/// aparecer aqui, ou o que a Mustard escreve nela fica visível no
/// `git status` do cliente. `.pipeline-states` entra porque projetos
/// antigos ainda a têm.
const DOCUMENTED_DIRS: &[&str] = &[
    ".cache",
    ".harness",
    ".metrics",
    ".agent-state",
    ".obsidian",
    ".pipeline-states",
    // Injectable instruction files (`mustard.json#inject` targets) — seeded by
    // `mustard init` from `templates/mustard/`, user-editable, spliced into the
    // session by the rt hooks. Must never be pruned as an orphan.
    "mustard",
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
    // The git worktrees Claude Code creates for its own sessions. The
    // separate copies of each wave and of the final reviewer are not here:
    // the round and the close create them in the copies folder outside the
    // project, under the user's cache (`io::wave_prompt::copies_dir`).
    "worktrees",
    // Sanctioned scratch evidence — the throwaway a diagnosis RUNS to decide
    // between two hypotheses. Carved out of branch protection by the write
    // gate (`shared::paths` in the rt), alongside `plans`.
    "scratch",
    // Rendered agent dispatch stubs (the prompt renderer with `--emit ref`), read
    // back by the PreToolUse hook that expands them.
    ".dispatch",
    // Estado de sessão e de agente. Cada sessão tem a sua pasta de eventos,
    // lida pelo servidor MCP e pelo observador do painel. Soltos na raiz, fora
    // de qualquer sessão, ficam os arquivos `size-steps-agent-<id>` e
    // `size-deliver-agent-<id>` da medida do agente de onda: o tamanho da
    // conversa em cada fim de tarefa e a ordem de entregar. Ficam por agente
    // porque o `/clear` de quem conduz troca a sessão no meio da onda, e nem
    // as tarefas medidas nem a ordem se perdem.
    ".session",
    // Lista de pendências fora de qualquer unidade (`run pending`), resolvida
    // no checkout principal — sobrevive à troca de branch e ao fim da unidade.
    "pending",
];

/// O nome do índice das specs, dentro de `.claude/spec/`.
pub const SPEC_INDEX_FILE: &str = "index.ndjson";

/// O nome do banco de lições, dentro de `.claude/spec/`.
pub const LESSONS_FILE: &str = "lessons.ndjson";

impl ClaudePaths {
    /// Build a handle pointing at `<root>/.claude/`.
    ///
    /// # Errors
    ///
    /// Returns [`ClaudePathsError::ForbiddenDotClaudeDotClaude`] when `root`
    /// terminates in `.claude` or contains the sequence `.claude/.claude/`
    /// anywhere. This is the defensive guard — the canonical resolver
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

    /// Build a handle without running the `.claude/.claude/` guard.
    ///
    /// **Fail-open callers only.** This bypass exists so a fallback branch in
    /// telemetry/event paths can keep using the same typed accessor surface
    /// as the happy path after `ClaudePaths::for_project(..).ok()` rejected
    /// the root. Production code that is not a fail-open fallback **must**
    /// use [`Self::for_project`] so guard violations are surfaced rather than
    /// silently materialised into `.claude/.claude/` paths.
    ///
    /// Even on the fallback branch, accessor calls over a
    /// `compose_unchecked(project)` handle beat open-coded
    /// `project.join(".claude").join("…")` strings.
    #[must_use]
    pub fn compose_unchecked(project: impl AsRef<Path>) -> Self {
        Self {
            root: project.as_ref().to_path_buf(),
        }
    }

    /// `<root>/.claude/` — the parent of every other accessor below.
    #[must_use]
    pub fn claude_dir(&self) -> PathBuf {
        self.root.join(".claude")
    }

    // -- top-level directories -------------------------------------------

    /// `<root>/.claude/spec/` — the parent of every per-spec directory.
    #[must_use]
    pub fn spec_dir(&self) -> PathBuf {
        self.claude_dir().join("spec")
    }

    /// `<root>/.claude/spec/index.ndjson` — o índice das specs: uma linha por
    /// spec, refeita pelo binário a cada evento gravado (`io::spec_index`).
    #[must_use]
    pub fn spec_index_path(&self) -> PathBuf {
        self.spec_dir().join(SPEC_INDEX_FILE)
    }

    /// `<root>/.claude/spec/lessons.ndjson` — o banco de lições, fora das
    /// pastas das specs e escrito só pelo binário (`io::lessons`).
    #[must_use]
    pub fn lessons_path(&self) -> PathBuf {
        self.spec_dir().join(LESSONS_FILE)
    }

    /// `<root>/.claude/pending/` — a lista de pendências que mora fora de
    /// qualquer unidade (`mustard-rt run pending`).
    #[must_use]
    pub fn pending_dir(&self) -> PathBuf {
        self.claude_dir().join("pending")
    }

    /// `<root>/.claude/pending/ledger.json` — o arquivo da lista; o chamador
    /// resolve `<root>` no checkout principal, para que um worktree leia o mesmo.
    #[must_use]
    pub fn pending_ledger_path(&self) -> PathBuf {
        self.pending_dir().join("ledger.json")
    }

    // -- root-level files ------------------------------------------------

    /// `<root>/.claude/settings.json` — hook wiring + permissions.
    #[must_use]
    pub fn settings_json_path(&self) -> PathBuf {
        self.claude_dir().join("settings.json")
    }

    /// `<root>/.claude/settings.local.json` — the untracked LOCAL LAYER beside
    /// [`Self::settings_json_path`].
    ///
    /// Claude Code reads it exactly like its shared twin, and the seeded
    /// `.claude/.gitignore` already covers it, so it is where machine-local or
    /// repository-invisible settings belong: the statusline heal observer
    /// already writes here, and a private install seeds the whole harness
    /// configuration here instead of into the shared file the host repository
    /// may version. Composed here rather than at the call sites because this
    /// module is the single owner of `.claude/` path composition.
    #[must_use]
    pub fn settings_local_json_path(&self) -> PathBuf {
        self.claude_dir().join("settings.local.json")
    }

    /// `<root>/mustard.json` — Mustard project config (git flow, build/test
    /// commands, `language`, runtime/version stamp).
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

    // -- catalogs --------------------------------------------------------

    /// List of every top-level directory under `<root>/.claude/` that
    /// Mustard documents. Consumed by `claude_dir_prune::DOCUMENTED_DIRS`.
    #[must_use]
    pub fn documented_dirs() -> Vec<&'static str> {
        DOCUMENTED_DIRS.to_vec()
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
        Ok(SpecPaths { spec_dir })
    }
}

impl SpecPaths {
    /// The spec directory itself.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.spec_dir
    }

    /// `<spec>/spec.ndjson` — the spec's event file, one event per line,
    /// written only by the binary (`io::spec_events`). The `.md` and the
    /// `.html` beside it are projections of it.
    #[must_use]
    pub fn spec_ndjson_path(&self) -> PathBuf {
        self.spec_dir.join("spec.ndjson")
    }

    /// `<spec>/spec.md` — the spec as text, projected from `spec.ndjson`.
    #[must_use]
    pub fn spec_md_path(&self) -> PathBuf {
        self.spec_dir.join("spec.md")
    }

    /// `<spec>/spec.html` — the spec page, projected from `spec.ndjson`.
    #[must_use]
    pub fn spec_html_path(&self) -> PathBuf {
        self.spec_dir.join("spec.html")
    }

    /// `<spec>/meta.json` — sidecar lifecycle metadata.
    #[must_use]
    pub fn meta_json_path(&self) -> PathBuf {
        self.spec_dir.join("meta.json")
    }
}

// -- helpers ------------------------------------------------------------

/// The `.claude/.claude/` guard: a project root must never terminate in `.claude` and must never
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn compose_unchecked_skips_the_nested_claude_guard() {
        let dir = tempdir().unwrap();
        let bad = dir.path().join(".claude");
        // `for_project` rejects this path…
        assert!(ClaudePaths::for_project(&bad).is_err());
        // …but `compose_unchecked` produces a usable handle for fail-open
        // fallback paths. The handle materialises canonical sub-paths from
        // the (already-nested) root — the consumer's job is to recognise
        // they are in the fallback branch.
        let cp = ClaudePaths::compose_unchecked(&bad);
        // Spec accessor still produces a deterministic shape.
        assert_eq!(cp.spec_dir(), bad.join(".claude").join("spec"));
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
    fn documented_dirs_includes_all_top_level_dirs() {
        let dirs = ClaudePaths::documented_dirs();
        let expected = [
            ".cache",
            ".harness",
            ".metrics",
            ".agent-state",
            ".obsidian",
            ".pipeline-states",
            "mustard",
            "commands",
            "skills",
            "refs",
            "agents",
            "agent-memory",
            "spec",
            "graph",
            "capabilities",
            "plans",
            // Live harness state the audit used to call "unexpected" because
            // the catalog did not know its own directories.
            "worktrees",
            "scratch",
            ".dispatch",
            ".session",
            "pending",
        ];
        for name in expected {
            assert!(dirs.contains(&name), "missing {name} from documented_dirs");
        }
        // Exact, not merely a superset.
        // Every name here becomes a `**/.claude/<name>/` exclude rule in a
        // private install (`project_seed::harness_claude_output`), so a stray
        // entry hides a client directory from their own `git add -A`.
        assert_eq!(dirs.len(), expected.len());
    }

    #[test]
    fn spec_paths_use_canonical_layout() {
        let dir = tempdir().unwrap();
        let cp = ClaudePaths::for_project(dir.path()).unwrap();
        let sp = cp.for_spec("2026-05-26-claude-paths").unwrap();
        // The three files of a spec sit side by side in its folder.
        assert_eq!(sp.spec_ndjson_path(), sp.dir().join("spec.ndjson"));
        assert_eq!(sp.spec_md_path(), sp.dir().join("spec.md"));
        assert_eq!(sp.spec_html_path(), sp.dir().join("spec.html"));
        assert_eq!(sp.dir(), dir.path().join(".claude").join("spec").join("2026-05-26-claude-paths"));
        assert!(sp.spec_md_path().ends_with("spec.md"));
        assert!(sp.meta_json_path().ends_with("meta.json"));
    }
}
