//! `ProjectConfig` — the single source of truth for `<root>/mustard.json`.
//!
//! ## Why this module exists
//!
//! Before it, the project config was read and written through a scatter of
//! ad-hoc parsers: `apps/rt/src/util/mustard_config.rs` (accessors, camelCase,
//! root), `apps/cli/.../git_flow.rs::MustardConfig` (the *writer*, snake_case,
//! partial), `spec_draft::read_mustard_tone`, `close_gate::read_mustard_commands`,
//! `i18n::project_locale` (reading `.claude/` hard-coded), plus a dozen inline
//! `serde_json::Value` peeks. Three failures followed: a **divergent schema**
//! (writer snake_case vs readers camelCase), a **split location** (`.claude/`
//! vs root), and **no single owner** of the file.
//!
//! This module replaces all of that with one typed handle. There is exactly
//! one schema (camelCase, defined by `serde`), one location (the project root,
//! via [`ClaudePaths::mustard_json_path`]), and one I/O path ([`load`] /
//! [`write`]). Consumers call [`ProjectConfig::load`] once and ask a typed
//! accessor — no `Value` juggling, no path strings, no compatibility wrappers.
//!
//! [`load`]: ProjectConfig::load
//! [`write`]: ProjectConfig::write
//! [`ClaudePaths::mustard_json_path`]: crate::ClaudePaths::mustard_json_path
//!
//! ## Fail-open
//!
//! Every field has a `Default`; `#[serde(default)]` fills missing keys. A
//! missing, unreadable, or malformed file yields [`ProjectConfig::default`] —
//! the gates then stand on their agnostic fallbacks rather than being blocked
//! by a config typo. Accessors normalise (trim, dotted-extension, lowercase)
//! exactly as the legacy `mustard_config` helpers did, so gate behaviour is
//! preserved.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::io::fs;
use crate::platform::error::Result;
use crate::platform::i18n::{I18n, SupportedLocale, Tone};
use crate::ClaudePaths;

/// Neutral placeholder returned when `buildCommand` is absent. Human-readable,
/// not runnable, so a drafted spec never hardcodes a stack-specific build the
/// project may not use.
pub const BUILD_COMMAND_FALLBACK: &str = "<build command>";

/// The `git` block of `mustard.json`.
///
/// It carries only what the project DECIDES and no probe can answer: which
/// branches it promotes through, and who hosts it. A fact the repository
/// already states — whether it has submodules, say — is read from the
/// repository when it is needed (`.gitmodules` on disk), never cached here: a
/// declaration written at `init` time goes stale the moment someone adds a
/// submodule, and a stale answer is worse than no answer. (`submodules: bool`
/// lived here, written by `init` and read by nobody; an unknown key in an
/// existing `mustard.json` is ignored on load, so older files keep working.)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct GitConfig {
    /// Branch promotion map: `"*" → dev`, `dev → production`.
    pub flow: BTreeMap<String, String>,
    /// Hosting provider — `github`, `gitlab`, or `bitbucket`.
    pub provider: String,
}

impl Default for GitConfig {
    fn default() -> Self {
        Self { flow: BTreeMap::new(), provider: "github".to_string() }
    }
}

impl GitConfig {
    /// The set of **integration base branches** this project promotes through,
    /// derived from [`flow`](GitConfig::flow): every non-`*` key ∪ every value.
    ///
    /// Examples: `{"*":"dev","dev":"main"}` → `{dev, main}`; `{"*":"main"}` →
    /// `{main}`; `{"*":"develop","develop":"master"}` → `{develop, master}`.
    /// An empty / absent flow falls back to `{main, master}` — the ONLY place a
    /// branch name is hardcoded, and only as a last resort. The rest is fully
    /// agnostic: the base set is whatever `git.flow` declares for the project.
    #[must_use]
    pub fn integration_bases(&self) -> BTreeSet<String> {
        let mut bases: BTreeSet<String> = BTreeSet::new();
        for (key, value) in &self.flow {
            let key = key.trim();
            if key != "*" && !key.is_empty() {
                bases.insert(key.to_string());
            }
            let value = value.trim();
            if !value.is_empty() {
                bases.insert(value.to_string());
            }
        }
        if bases.is_empty() {
            bases.insert("main".to_string());
            bases.insert("master".to_string());
        }
        bases
    }

    /// The **primary** integration base: `flow["*"]` when present, else any
    /// single integration base (lexically-least, deterministic), else `main`.
    /// Agnostic — the only literal is the last-resort `main` for a project with
    /// no `git.flow`.
    #[must_use]
    pub fn primary_base(&self) -> String {
        if let Some(star) = self.flow.get("*").map(|s| s.trim()).filter(|s| !s.is_empty()) {
            return star.to_string();
        }
        self.integration_bases()
            .into_iter()
            .next()
            .unwrap_or_else(|| "main".to_string())
    }
}

/// `subprojects.exclude` / `.include` — repo-root-relative path overrides.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Subprojects {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
}

impl Subprojects {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.exclude.is_empty() && self.include.is_empty()
    }
}

/// The `amend` block. Note the field is `drift_threshold` (snake_case) on disk
/// — this sub-struct keeps Rust's natural snake naming so it matches existing
/// files; only the top-level `ProjectConfig` keys are camelCase.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Amend {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drift_threshold: Option<u64>,
}

impl Amend {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.drift_threshold.is_none()
    }
}

/// Gate enforcement modes (`off` | `warn` | `strict`) — the project-level
/// default for each gate, formerly carried as `MUSTARD_*_MODE` env vars in
/// `settings.json`. They live here so `mustard.json` is the single source of
/// project config; each gate resolves in cascade **env var → this field →
/// built-in default**, so an env var still overrides per-run (CI/debug) and an
/// absent field falls back to the gate's own default. Each is a free string
/// parsed by the gate (an unknown value falls through to the gate default).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GateModes {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec_size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_validate_lines: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checklist: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_budget: Option<String>,
}

impl GateModes {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.spec_size.is_none()
            && self.skill_size.is_none()
            && self.skill_validate_lines.is_none()
            && self.checklist.is_none()
            && self.boundary.is_none()
            && self.main_budget.is_none()
    }
}

/// Host runtime metadata stamped into `mustard.json` by `init`/`update`.
///
/// `kind` is the literal `"native"` (the CLI is a compiled binary, not a JS
/// runtime); `os`/`arch` come from `std::env::consts`. Owned here in the core
/// so the config is a self-contained domain type with no `apps/cli` dependency.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Runtime {
    pub kind: String,
    pub os: String,
    pub arch: String,
}

impl Runtime {
    /// Capture the current host's runtime metadata.
    #[must_use]
    pub fn detect() -> Self {
        Self {
            kind: "native".to_string(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
        }
    }
}

/// One `{ pattern, role }` role-classification override.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RolePattern {
    /// Substring (or simple `*` glob) tested against the file path.
    pub pattern: String,
    /// The role assigned on the first matching pattern.
    pub role: String,
}

/// One `inject` declaration: an instruction file the session hooks splice into
/// the agent's window as `additionalContext` on a given trigger.
///
/// The files live in the project (canonically `.claude/mustard/*.md`, seeded
/// by `init` and freely editable by the user); `mustard.json#inject` declares
/// which file rides which trigger. This replaces the planted
/// `.claude/CLAUDE.md` orchestrator: content is *injected*, never written into
/// the project's own instruction files.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Injectable {
    /// Trigger name (`userPromptSubmit`, `sessionStart`). Matched
    /// case-insensitively — [`ProjectConfig::injectables`] lowercases it.
    pub on: String,
    /// Project-root-relative path of the file whose content is injected.
    pub file: String,
    /// Deliver at most once per session (the hooks keep a per-session marker
    /// under `.claude/.session/<id>/`). Defaults to `false` (every trigger).
    #[serde(default)]
    pub once: bool,
}

/// The build/test/lint/type-check command set resolved from `mustard.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Commands {
    pub build: Option<String>,
    pub test: Option<String>,
    pub lint: Option<String>,
    pub type_check: Option<String>,
}

/// The full `mustard.json` document — the project config, at the project root.
///
/// `#[serde(rename_all = "camelCase")]` applies to the **top-level** keys only
/// (`buildCommand`, `specLang`, `maxActiveSpecs`, …). The nested structs keep
/// snake/lowercase naming (`amend.drift_threshold`, `git.provider`,
/// `subprojects.exclude`) to match the on-disk shape. Legacy snake_case command
/// keys are still accepted on read via `alias`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectConfig {
    /// Git promotion flow + provider + submodule flag.
    pub git: GitConfig,

    #[serde(skip_serializing_if = "Option::is_none", alias = "build_command")]
    pub build_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "test_command")]
    pub test_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "lint_command")]
    pub lint_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "type_check_command")]
    pub type_check_command: Option<String>,

    /// Version-control binary. Absent ⇒ `git` default; `""` ⇒ explicit opt-out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vcs: Option<String>,

    /// Spec language (BCP-47). Canonical key on write.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec_lang: Option<String>,
    /// Legacy alias of `spec_lang`, still read for back-compat (precedence below
    /// `spec_lang` is via `lang.or(spec_lang)` in [`ProjectConfig::i18n`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    /// Banner / drafter tone (`didactic` | `technical` | `concise`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tone: Option<String>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub source_extensions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_ext: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub architecture: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_active_specs: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub role_patterns: Vec<RolePattern>,
    /// Declared context injections (`[{on, file, once}]`) — see [`Injectable`].
    /// Consumed through the normalising [`ProjectConfig::injectables`] accessor.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inject: Vec<Injectable>,
    /// Optional architectural layer order for the deterministic wave fallback
    /// used when the import DAG has no depth (all-net-new features, no edges to
    /// order by). Roles are scheduled in this order — each wave depends on the
    /// previous; roles not listed fall to the tail (lexically). Empty/absent → a
    /// documented default. Project-overridable so a non-standard architecture
    /// sets its own dependency direction (keeps the wave engine agnostic).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wave_layer_order: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Subprojects::is_empty")]
    pub subprojects: Subprojects,
    #[serde(skip_serializing_if = "Amend::is_empty")]
    pub amend: Amend,
    #[serde(skip_serializing_if = "GateModes::is_empty")]
    pub gates: GateModes,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<Runtime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// Any keys not modelled above — preserved verbatim across a load→write
    /// round-trip so a future field (or a user's custom key) is never dropped.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl ProjectConfig {
    /// The canonical on-disk path: `<root>/mustard.json`, via [`ClaudePaths`].
    /// The `unwrap_or_else` is defence-in-depth — `root` should never terminate
    /// in `.claude` (the workspace resolver guarantees it).
    fn json_path(root: &Path) -> PathBuf {
        ClaudePaths::for_project(root)
            .map(|p| p.mustard_json_path())
            .unwrap_or_else(|_| root.join("mustard.json"))
    }

    /// Whether `<root>/mustard.json` exists on disk. Lets `init`/`update`
    /// distinguish "fresh project" from "re-run over an existing config"
    /// without a second path join at the call site.
    #[must_use]
    pub fn exists(root: &Path) -> bool {
        Self::json_path(root).is_file()
    }

    /// Load the config from `<root>/mustard.json`, fail-open to
    /// [`ProjectConfig::default`] on any IO or parse error.
    #[must_use]
    pub fn load(root: &Path) -> Self {
        let path = Self::json_path(root);
        let Ok(text) = fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    /// Serialize and atomically write to `<root>/mustard.json`.
    ///
    /// # Errors
    /// [`crate::platform::error::Error::Parse`] on serialization failure (never
    /// happens for this type in practice) or [`crate::platform::error::Error::Io`]
    /// on a write failure.
    pub fn write(&self, root: &Path) -> Result<()> {
        let path = Self::json_path(root);
        let mut json = serde_json::to_string_pretty(self)?;
        json.push('\n');
        fs::write_atomic(&path, json.as_bytes())
    }

    /// `buildCommand`, trimmed; `None` when absent or blank.
    #[must_use]
    pub fn build_command(&self) -> Option<String> {
        non_blank(self.build_command.as_deref())
    }

    /// `buildCommand` or [`BUILD_COMMAND_FALLBACK`].
    #[must_use]
    pub fn build_command_or_fallback(&self) -> String {
        self.build_command().unwrap_or_else(|| BUILD_COMMAND_FALLBACK.to_string())
    }

    /// The four close-gate commands, each trimmed / `None` when blank.
    #[must_use]
    pub fn commands(&self) -> Commands {
        Commands {
            build: non_blank(self.build_command.as_deref()),
            test: non_blank(self.test_command.as_deref()),
            lint: non_blank(self.lint_command.as_deref()),
            type_check: non_blank(self.type_check_command.as_deref()),
        }
    }

    /// VCS binary policy: `Some("git")` by default, `Some(bin)` when pinned,
    /// `None` when the user set `vcs` to an empty string (explicit opt-out).
    #[must_use]
    pub fn vcs(&self) -> Option<String> {
        match self.vcs.as_deref() {
            None => Some("git".to_string()),
            Some(raw) => {
                let t = raw.trim();
                if t.is_empty() {
                    None
                } else {
                    Some(t.to_string())
                }
            }
        }
    }

    /// Architecture-style override, trimmed + lowercased; `None` when blank.
    #[must_use]
    pub fn architecture(&self) -> Option<String> {
        let raw = self.architecture.as_deref()?.trim();
        if raw.is_empty() {
            None
        } else {
            Some(raw.to_ascii_lowercase())
        }
    }

    /// Hard cap on concurrent active specs; `None` falls back to the built-in
    /// default. `0` is honoured literally (freeze new starts).
    #[must_use]
    pub fn max_active_specs(&self) -> Option<usize> {
        self.max_active_specs.and_then(|n| usize::try_from(n).ok())
    }

    /// Ordered role-classification overrides; `pattern` lowercased, entries with
    /// a blank `pattern` or `role` skipped (fail-open).
    #[must_use]
    pub fn role_patterns(&self) -> Vec<RolePattern> {
        self.role_patterns
            .iter()
            .filter_map(|rp| {
                let pattern = rp.pattern.trim();
                let role = rp.role.trim();
                if pattern.is_empty() || role.is_empty() {
                    return None;
                }
                Some(RolePattern { pattern: pattern.to_lowercase(), role: role.to_string() })
            })
            .collect()
    }

    /// `amend.drift_threshold` as a `u32`; `None` when absent or out of range.
    #[must_use]
    pub fn drift_threshold(&self) -> Option<u32> {
        self.amend.drift_threshold.and_then(|n| u32::try_from(n).ok())
    }

    /// Declared injectables, normalised fail-open: entries with a blank `on`
    /// or `file` are dropped (a config typo silences that entry, never the
    /// hook), `on` is trimmed + ASCII-lowercased so hooks compare against
    /// lowercase trigger names, and `file` is trimmed. Order preserved.
    #[must_use]
    pub fn injectables(&self) -> Vec<Injectable> {
        self.inject
            .iter()
            .filter_map(|entry| {
                let on = entry.on.trim();
                let file = entry.file.trim();
                if on.is_empty() || file.is_empty() {
                    return None;
                }
                Some(Injectable {
                    on: on.to_ascii_lowercase(),
                    file: file.to_string(),
                    once: entry.once,
                })
            })
            .collect()
    }

    /// Resolve the banner/drafter [`I18n`] (locale + tone) for this project.
    ///
    /// Locale precedence: `lang` then `spec_lang`; unparseable / absent ⇒
    /// [`SupportedLocale::default`] (`pt-BR`). Tone: `tone` or
    /// [`Tone::default`] (`didactic`). Reuses the `platform::i18n` primitives.
    #[must_use]
    pub fn i18n(&self) -> I18n {
        let lang = self
            .lang
            .as_deref()
            .or(self.spec_lang.as_deref())
            .and_then(|s| s.parse::<SupportedLocale>().ok())
            .unwrap_or_default();
        let tone = self.tone.as_deref().and_then(Tone::parse).unwrap_or_default();
        I18n::new(lang, tone)
    }
}

/// Trim a string-ish option, returning `None` when absent or blank.
fn non_blank(raw: Option<&str>) -> Option<String> {
    let t = raw?.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// Test whether `pattern` (lowercased) matches `haystack` (lowercased). `*` is a
/// wildcard for "any run of characters"; a pattern with no `*` is a plain
/// substring test. Moved here from `mustard_config` — it is pure domain logic.
#[must_use]
pub fn glob_matches(pattern: &str, haystack: &str) -> bool {
    if !pattern.contains('*') {
        return haystack.contains(pattern);
    }
    let segments: Vec<&str> = pattern.split('*').collect();
    let anchored_start = !pattern.starts_with('*');
    let anchored_end = !pattern.ends_with('*');
    let mut cursor = 0usize;
    for (i, seg) in segments.iter().enumerate() {
        if seg.is_empty() {
            continue;
        }
        let Some(found) = haystack[cursor..].find(seg) else {
            return false;
        };
        let abs = cursor + found;
        if anchored_start && i == 0 && abs != 0 {
            return false;
        }
        cursor = abs + seg.len();
    }
    if anchored_end {
        if let Some(last) = segments.iter().rev().find(|s| !s.is_empty()) {
            return haystack.ends_with(last);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn load_absent_is_default_fail_open() {
        let dir = tempdir().unwrap();
        let cfg = ProjectConfig::load(dir.path());
        assert_eq!(cfg.git.provider, "github");
        assert!(cfg.build_command().is_none());
        assert_eq!(cfg.vcs(), Some("git".to_string()));
    }

    #[test]
    fn write_then_load_round_trips_and_uses_camelcase() {
        let dir = tempdir().unwrap();
        let mut cfg = ProjectConfig::default();
        cfg.build_command = Some("cargo build".into());
        cfg.spec_lang = Some("pt-BR".into());
        cfg.tone = Some("technical".into());
        cfg.write(dir.path()).unwrap();

        let raw = std::fs::read_to_string(dir.path().join("mustard.json")).unwrap();
        assert!(raw.contains("\"buildCommand\""), "top-level key is camelCase");
        assert!(raw.contains("\"specLang\""));
        assert!(!raw.contains("build_command"), "no snake_case on write");

        let back = ProjectConfig::load(dir.path());
        assert_eq!(back.build_command(), Some("cargo build".to_string()));
    }

    #[test]
    fn reads_legacy_snake_case_command_aliases() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("mustard.json"),
            r#"{"build_command":"make","test_command":"make test"}"#,
        )
        .unwrap();
        let cfg = ProjectConfig::load(dir.path());
        assert_eq!(cfg.build_command(), Some("make".to_string()));
        assert_eq!(cfg.commands().test, Some("make test".to_string()));
    }

    #[test]
    fn unknown_keys_preserved_across_round_trip() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("mustard.json"),
            r#"{"buildCommand":"x","customKey":{"a":1}}"#,
        )
        .unwrap();
        let cfg = ProjectConfig::load(dir.path());
        assert!(cfg.extra.contains_key("customKey"));
        cfg.write(dir.path()).unwrap();
        let raw = std::fs::read_to_string(dir.path().join("mustard.json")).unwrap();
        assert!(raw.contains("customKey"), "unknown key survives write");
    }

    #[test]
    fn vcs_default_and_optout() {
        let mut cfg = ProjectConfig::default();
        assert_eq!(cfg.vcs(), Some("git".to_string()));
        cfg.vcs = Some("jj".into());
        assert_eq!(cfg.vcs(), Some("jj".to_string()));
        cfg.vcs = Some("  ".into());
        assert_eq!(cfg.vcs(), None);
    }

    #[test]
    fn max_active_specs_honours_zero() {
        let mut cfg = ProjectConfig::default();
        assert_eq!(cfg.max_active_specs(), None);
        cfg.max_active_specs = Some(0);
        assert_eq!(cfg.max_active_specs(), Some(0));
        cfg.max_active_specs = Some(5);
        assert_eq!(cfg.max_active_specs(), Some(5));
    }

    #[test]
    fn i18n_precedence_lang_over_spec_lang_and_tone() {
        let mut cfg = ProjectConfig::default();
        // default → pt-BR / didactic
        assert_eq!(cfg.i18n(), I18n::new(SupportedLocale::PtBr, Tone::Didactic));
        cfg.spec_lang = Some("en-US".into());
        assert_eq!(cfg.i18n().lang, SupportedLocale::EnUs);
        cfg.lang = Some("pt-BR".into()); // lang wins over spec_lang
        assert_eq!(cfg.i18n().lang, SupportedLocale::PtBr);
        cfg.tone = Some("concise".into());
        assert_eq!(cfg.i18n().tone, Tone::Concise);
    }

    #[test]
    fn inject_round_trips_through_write_and_load() {
        let dir = tempdir().unwrap();
        let mut cfg = ProjectConfig::default();
        cfg.inject = vec![
            Injectable {
                on: "userPromptSubmit".into(),
                file: ".claude/mustard/orchestrator.md".into(),
                once: true,
            },
            Injectable {
                on: "sessionStart".into(),
                file: ".claude/mustard/response-style.md".into(),
                once: false,
            },
        ];
        cfg.write(dir.path()).unwrap();

        let raw = std::fs::read_to_string(dir.path().join("mustard.json")).unwrap();
        assert!(raw.contains("\"inject\""), "inject key serialized: {raw}");
        assert!(raw.contains("userPromptSubmit"), "on value preserved verbatim on disk");

        let back = ProjectConfig::load(dir.path());
        assert_eq!(back.inject.len(), 2, "both entries survive the round-trip");
        assert_eq!(back.inject[0].file, ".claude/mustard/orchestrator.md");
        assert!(back.inject[0].once);
        assert!(!back.inject[1].once, "explicit once=false survives");

        // The accessor normalises `on` to lowercase without touching the file.
        let norm = back.injectables();
        assert_eq!(norm[0].on, "userpromptsubmit");
        assert_eq!(norm[1].on, "sessionstart");
    }

    #[test]
    fn injectables_filters_blank_entries_fail_open() {
        let mut cfg = ProjectConfig::default();
        cfg.inject = vec![
            Injectable { on: "  ".into(), file: "x.md".into(), once: false },
            Injectable { on: "sessionStart".into(), file: "".into(), once: true },
            Injectable { on: " SessionStart ".into(), file: " a.md ".into(), once: true },
        ];
        let got = cfg.injectables();
        assert_eq!(got.len(), 1, "blank on/file entries are dropped: {got:?}");
        assert_eq!(got[0].on, "sessionstart", "on is trimmed + lowercased");
        assert_eq!(got[0].file, "a.md", "file is trimmed");
        assert!(got[0].once);
    }

    #[test]
    fn load_without_inject_defaults_to_empty() {
        // An older mustard.json that predates the field loads fine and the
        // accessor yields no injectables (fail-open Default).
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"), r#"{"buildCommand":"make"}"#).unwrap();
        let cfg = ProjectConfig::load(dir.path());
        assert!(cfg.inject.is_empty());
        assert!(cfg.injectables().is_empty());
        // `once` missing on disk → false.
        std::fs::write(
            dir.path().join("mustard.json"),
            r#"{"inject":[{"on":"sessionStart","file":"a.md"}]}"#,
        )
        .unwrap();
        let cfg = ProjectConfig::load(dir.path());
        assert_eq!(cfg.injectables().len(), 1);
        assert!(!cfg.injectables()[0].once, "absent once defaults to false");
    }

    #[test]
    fn role_patterns_lowercased_and_filtered() {
        let mut cfg = ProjectConfig::default();
        cfg.role_patterns = vec![
            RolePattern { pattern: "Controllers".into(), role: "api".into() },
            RolePattern { pattern: " ".into(), role: "x".into() },
        ];
        let got = cfg.role_patterns();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].pattern, "controllers");
    }

    #[test]
    fn glob_matches_substring_and_wildcard() {
        assert!(glob_matches("controller", "src/usercontroller.rb"));
        assert!(!glob_matches("controller", "src/user.rb"));
        assert!(glob_matches("src/*.rb", "src/foo.rb"));
        assert!(!glob_matches("*.rb", "x.rs"));
    }

    #[test]
    fn integration_bases_derives_from_flow_keys_and_values() {
        // Standard two-tier flow → {dev, main}.
        let mut cfg = ProjectConfig::default();
        cfg.git.flow.insert("*".into(), "dev".into());
        cfg.git.flow.insert("dev".into(), "main".into());
        let bases = cfg.git.integration_bases();
        assert!(bases.contains("dev") && bases.contains("main"));
        assert_eq!(bases.len(), 2, "the `*` key is not itself a base: {bases:?}");

        // GitHub-flow single main → {main}.
        let mut single = ProjectConfig::default();
        single.git.flow.insert("*".into(), "main".into());
        assert_eq!(
            single.git.integration_bases(),
            BTreeSet::from(["main".to_string()]),
        );

        // develop/master flow (agnostic — no dev/main anywhere) → {develop, master}.
        let mut dm = ProjectConfig::default();
        dm.git.flow.insert("*".into(), "develop".into());
        dm.git.flow.insert("develop".into(), "master".into());
        assert_eq!(
            dm.git.integration_bases(),
            BTreeSet::from(["develop".to_string(), "master".to_string()]),
        );
    }

    #[test]
    fn integration_bases_empty_flow_falls_back_to_main_master() {
        let cfg = ProjectConfig::default();
        assert_eq!(
            cfg.git.integration_bases(),
            BTreeSet::from(["main".to_string(), "master".to_string()]),
        );
    }

    #[test]
    fn primary_base_prefers_star_then_first_then_main() {
        // flow["*"] wins.
        let mut cfg = ProjectConfig::default();
        cfg.git.flow.insert("*".into(), "develop".into());
        cfg.git.flow.insert("develop".into(), "master".into());
        assert_eq!(cfg.git.primary_base(), "develop");

        // No `*` → lexically-least integration base.
        let mut no_star = ProjectConfig::default();
        no_star.git.flow.insert("develop".into(), "master".into());
        assert_eq!(no_star.git.primary_base(), "develop");

        // Empty flow → last-resort `main`.
        assert_eq!(ProjectConfig::default().git.primary_base(), "main");
    }
}
