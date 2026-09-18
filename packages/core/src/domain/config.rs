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
use crate::platform::i18n::SupportedLocale;
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
#[derive(Default)]
pub struct GitConfig {
    /// Branch promotion map: `"*" → dev`, `dev → production`.
    ///
    /// **Optional, and no longer a constraint.** It used to be the ONLY
    /// definition of "which branches may a unit be cut from", which made the
    /// answer as old as the install: a branch created after `mustard init` was
    /// refused for existing. A unit is now cut from any branch git really has
    /// ([`crate::platform::git_branches::branch_catalog`]) and this map only
    /// PRE-SELECTS — see [`declared_bases`](GitConfig::declared_bases). A
    /// project that never declares one pre-selects nothing, and protects
    /// nothing, until it does.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub flow: BTreeMap<String, String>,
    /// Branches that refuse a direct commit or merge, BEYOND the bases
    /// `git.flow` already declares.
    ///
    /// The escape hatch for a team that also protects `develop` or a
    /// `release/*` line. Empty for almost every project, which is why it is
    /// skipped on serialize: an install writes no key, and the file stays as
    /// small as it was.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub protected: Vec<String>,
    /// Hosting provider — an OVERRIDE, empty by default.
    ///
    /// Empty means "ask the repository": the provider is detected from the
    /// `origin` remote's host
    /// ([`crate::platform::git_provider::resolve_provider`]). It used to be a
    /// question the install asked and froze, which meant every project carried
    /// an answer given on the day it was first opened.
    ///
    /// It survives as an override — and it WINS over detection — because a
    /// self-hosted instance is unrecognisable by hostname, and that is the one
    /// case where only the operator knows. Skipped on serialize so a fresh
    /// install writes no key at all: writing `"github"` by default would make
    /// every install a permanent override and detection would never run.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub provider: String,
    /// Whether the unit's branch on the SERVER is deleted when the unit leaves
    /// the stage. Off by default, and an install writes no key.
    ///
    /// Many teams may not delete a branch on the server: the merge is done by
    /// another area and the branch is theirs. So the server branch is never
    /// touched unless the project turns this on in `mustard.json`.
    #[serde(rename = "deleteRemoteBranch", default, skip_serializing_if = "std::ops::Not::not")]
    pub delete_remote_branch: bool,
}


impl GitConfig {
    /// The bases this project REALLY declares, derived from
    /// [`flow`](GitConfig::flow): every non-`*` key ∪ every value.
    ///
    /// **The only source of a base name in this project.** There used to be a
    /// second accessor that floored to `{main, master}` whenever the flow was
    /// empty, and an install writes no flow — so every fresh project was told
    /// it declared two branches it might not even carry, and the doors that
    /// decide by "is this a base?" decided by two literals. An empty flow
    /// yields an empty set here, and an empty set means the project has
    /// declared nothing: nothing is pre-selected and nothing is protected until
    /// it does.
    #[must_use]
    pub fn declared_bases(&self) -> BTreeSet<String> {
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
        bases
    }

    /// The base a picker opens ON: `flow["*"]` when present, else any single
    /// declared base (lexically-least, deterministic).
    ///
    /// `None` when the project declares no flow at all. It used to floor to
    /// `main`, and that literal is precisely what sent a unit whose project
    /// never named `main` off to a branch nobody measured. A caller with no
    /// answer here must ask the repository or ask the operator — never carry a
    /// name this project never wrote.
    ///
    /// A DEFAULT for the cursor, never a restriction on what the operator may
    /// pick instead.
    #[must_use]
    pub fn primary_base(&self) -> Option<String> {
        if let Some(star) = self.flow.get("*").map(|s| s.trim()).filter(|s| !s.is_empty()) {
            return Some(star.to_string());
        }
        self.declared_bases().into_iter().next()
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

/// The `language` block of `mustard.json`: the two languages a project writes
/// in, each declared on its own key.
///
/// Both are optional, and an install writes only the one the operator chose.
/// A language nobody chose is never written for them: a written default reads
/// the same as a choice, and the checks that judge the conversation by its
/// language would then judge a project by a language it never picked.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LanguageConfig {
    /// The language of everything a person reads: the conversation, specs,
    /// pages, comments in the code and commit messages. BCP-47 with the
    /// dialect (`pt-BR`, `en-US`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The language of the names in the code: variables, functions, files and
    /// commands. Always `en`: code is written in English, so the installer
    /// never asks for it and never writes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl LanguageConfig {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_none() && self.code.is_none()
    }
}

/// The languages a project declared, as [`ProjectConfig::language`] reads them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Language {
    /// The declared text language; `None` when absent, blank or not one of the
    /// locales Mustard ships messages for.
    pub text: Option<SupportedLocale>,
    /// The declared code language, trimmed; `None` when absent or blank.
    pub code: Option<String>,
}

impl Language {
    /// The language Mustard writes its own messages in: the declared text
    /// language, or `pt-BR` when none was declared.
    ///
    /// Only for Mustard's OWN words. A check that judges what the user or the
    /// assistant wrote reads [`Language::text`] instead, because a default is
    /// the absence of a choice: an English project that never declared a
    /// language must not be judged as Portuguese.
    #[must_use]
    pub fn text_or_default(&self) -> SupportedLocale {
        self.text.unwrap_or_default()
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
/// (`buildCommand`, `maxActiveSpecs`, …). The nested structs keep
/// snake/lowercase naming (`amend.drift_threshold`, `git.provider`,
/// `language.text`, `subprojects.exclude`) to match the on-disk shape. Legacy
/// snake_case command keys are still accepted on read via `alias`.
///
/// The language keys that came before `language` (`specLang`, `lang`) and the
/// `tone` key are no longer part of the schema. A file that still carries them
/// keeps loading: they land in [`ProjectConfig::extra`] like any unknown key,
/// preserved on write and read by nobody.
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

    /// The text and code languages — see [`LanguageConfig`]. Read only through
    /// [`ProjectConfig::language`].
    #[serde(skip_serializing_if = "LanguageConfig::is_empty")]
    pub language: LanguageConfig,

    /// As siglas do dia a dia do projeto (`["PI", "PCP"]`), que a conferência
    /// de escrita aceita sem o nome por extenso. Lida só por
    /// [`ProjectConfig::acronyms`].
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub acronyms: Vec<String>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub source_extensions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_ext: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub architecture: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_active_specs: Option<u64>,
    /// Quantas ondas saem na mesma rodada, que é quantas compilam ao mesmo
    /// tempo. Ausente ⇒ o padrão do binário; a máquina com mais memória põe
    /// um número maior aqui.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_compiling_waves: Option<u64>,
    /// A chave que liga e desliga o Mustard no projeto. Desligado (`false`),
    /// nenhum gancho do Mustard age aqui; ausente, ele está ligado. Lida só
    /// por [`ProjectConfig::enabled`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Liga e desliga o gancho do rtk nas configurações locais do projeto.
    /// Ausente, ligado. Desligar não desinstala o rtk. Lida só por
    /// [`ProjectConfig::rtk`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rtk: Option<bool>,
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

    /// The file is there and did not load: it exists on disk and could not be
    /// read or parsed, so every field above is a fallback, not the project's
    /// own answer. An absent file *is* an answer — "this project declares
    /// nothing" — and leaves this `false`. A rule that would silently switch
    /// itself off on the empty fallback, like the write gate's base lock, asks
    /// here and refuses instead. Never on disk: it describes the load, not the
    /// document.
    #[serde(skip)]
    pub unreadable: bool,

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

    /// Load the config from `<root>/mustard.json`, falling back to
    /// [`ProjectConfig::default`] on any IO or parse error.
    ///
    /// The fallback keeps the two cases apart in
    /// [`ProjectConfig::unreadable`]: a project with no file declares nothing,
    /// and a file that exists and does not load declares nothing *that anyone
    /// can read*. The defaults are the same; what a caller may conclude from
    /// them is not.
    #[must_use]
    pub fn load(root: &Path) -> Self {
        let path = Self::json_path(root);
        let broken = || Self { unreadable: true, ..Self::default() };
        let Ok(text) = fs::read_to_string(&path) else {
            return if path.is_file() { broken() } else { Self::default() };
        };
        serde_json::from_str(&text).unwrap_or_else(|_| broken())
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

    /// Quantas ondas o projeto deixa compilar ao mesmo tempo; `None` cai no
    /// padrão do binário. O `0` é obedecido ao pé da letra (nada sai).
    #[must_use]
    pub fn max_compiling_waves(&self) -> Option<usize> {
        self.max_compiling_waves.and_then(|n| usize::try_from(n).ok())
    }

    /// O Mustard está ligado neste projeto: só um `enabled: false` escrito no
    /// arquivo o desliga. Um arquivo que não se lê não desliga nada.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.enabled != Some(false)
    }

    /// O gancho do rtk deve estar nas configurações locais: só um
    /// `rtk: false` escrito no arquivo o tira.
    #[must_use]
    pub fn rtk(&self) -> bool {
        self.rtk != Some(false)
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

    /// The languages this project declared — the one reader of the
    /// `language` block, and so the one place any part of Mustard learns a
    /// project's language.
    ///
    /// Nothing is inferred: an absent, blank or unsupported `language.text`
    /// is `None`, and the keys that came before it (`specLang`, `lang`) are not
    /// consulted. Mustard's own messages fall back through
    /// [`Language::text_or_default`]; a check that judges text reads
    /// [`Language::text`] and has no verdict without it.
    #[must_use]
    pub fn language(&self) -> Language {
        Language {
            text: self
                .language
                .text
                .as_deref()
                .and_then(|raw| raw.parse::<SupportedLocale>().ok()),
            code: non_blank(self.language.code.as_deref()),
        }
    }

    /// As siglas do projeto, sem espaço em volta e em maiúsculas, porque a
    /// conferência de escrita só reconhece sigla em maiúsculas: um `"pi"`
    /// escrito no arquivo vale como `PI`. Entradas em branco e repetidas saem.
    #[must_use]
    pub fn acronyms(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for raw in &self.acronyms {
            let acronym = raw.trim().to_ascii_uppercase();
            if !acronym.is_empty() && !out.contains(&acronym) {
                out.push(acronym);
            }
        }
        out
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
    if anchored_end
        && let Some(last) = segments.iter().rev().find(|s| !s.is_empty()) {
            return haystack.ends_with(last);
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
        // Empty by DEFAULT now: the provider is a detected fact with an
        // optional override, and a default of "github" would write that
        // override on every install.
        assert_eq!(cfg.git.provider, "");
        assert!(cfg.build_command().is_none());
        assert_eq!(cfg.vcs(), Some("git".to_string()));
        assert!(!cfg.unreadable, "no file is an answer: this project declares nothing");
    }

    /// A file that is there and does not load gives the same defaults as no
    /// file at all, and says so: the defaults are a fallback, not the
    /// project's own answer. The flag never reaches the disk.
    #[test]
    fn a_file_that_does_not_load_is_told_apart_from_no_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("mustard.json");
        for broken in ["{ not json", "[]", ""] {
            std::fs::write(&path, broken).unwrap();
            let cfg = ProjectConfig::load(dir.path());
            assert!(cfg.unreadable, "{broken:?} is there and does not load");
            assert!(cfg.git.declared_bases().is_empty(), "{broken:?} declares no base either");
        }
        std::fs::write(&path, r#"{"git":{"flow":{"*":"dev"}}}"#).unwrap();
        assert!(!ProjectConfig::load(dir.path()).unreadable, "a readable file is an answer");

        let out = dir.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        ProjectConfig { unreadable: true, ..ProjectConfig::default() }.write(&out).unwrap();
        let raw = std::fs::read_to_string(out.join("mustard.json")).unwrap();
        assert!(!raw.contains("unreadable"), "it describes the load, not the document: {raw}");
    }

    #[test]
    fn write_then_load_round_trips_and_uses_camelcase() {
        let dir = tempdir().unwrap();
        let cfg = ProjectConfig {
            build_command: Some("cargo build".into()),
            language: LanguageConfig { text: Some("pt-BR".into()), code: Some("en".into()) },
            ..Default::default()
        };
        cfg.write(dir.path()).unwrap();

        let raw = std::fs::read_to_string(dir.path().join("mustard.json")).unwrap();
        assert!(raw.contains("\"buildCommand\""), "top-level key is camelCase");
        assert!(!raw.contains("build_command"), "no snake_case on write");
        let value: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(value["language"], serde_json::json!({"text": "pt-BR", "code": "en"}));

        let back = ProjectConfig::load(dir.path());
        assert_eq!(back.build_command(), Some("cargo build".to_string()));
        assert_eq!(back.language().text, Some(SupportedLocale::PtBr));
        assert_eq!(back.language().code.as_deref(), Some("en"));
    }

    /// Um projeto que não declarou idioma não ganha a chave: nada é gravado
    /// por padrão.
    #[test]
    fn an_undeclared_language_writes_no_key() {
        let dir = tempdir().unwrap();
        ProjectConfig::default().write(dir.path()).unwrap();
        let raw = std::fs::read_to_string(dir.path().join("mustard.json")).unwrap();
        assert!(!raw.contains("\"language\""), "no language was chosen: {raw}");
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

    /// O idioma vem só do bloco `language`: o texto e o código, cada um na
    /// sua chave. Sem declaração não há idioma, e as mensagens do Mustard
    /// caem no pt-BR.
    #[test]
    fn language_reads_the_text_and_code_keys() {
        let cfg = ProjectConfig::default();
        assert_eq!(cfg.language(), Language::default());
        assert_eq!(cfg.language().text_or_default(), SupportedLocale::PtBr);

        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("mustard.json"),
            r#"{"language":{"text":"en-US","code":" en "}}"#,
        )
        .unwrap();
        let language = ProjectConfig::load(dir.path()).language();
        assert_eq!(language.text, Some(SupportedLocale::EnUs));
        assert_eq!(language.text_or_default(), SupportedLocale::EnUs);
        assert_eq!(language.code.as_deref(), Some("en"), "the code language is trimmed");

        // A short form or an unknown locale is not a declared text language.
        for text in ["pt", "fr-FR", "  "] {
            std::fs::write(
                dir.path().join("mustard.json"),
                format!(r#"{{"language":{{"text":"{text}"}}}}"#),
            )
            .unwrap();
            assert_eq!(ProjectConfig::load(dir.path()).language().text, None, "{text:?}");
        }
    }

    /// As siglas do projeto moram na chave `acronyms`: a leitura tira os
    /// espaços, passa para maiúsculas e deixa de fora o branco e a repetida.
    /// Sem a chave a lista sai vazia, e a gravação não a escreve.
    #[test]
    fn the_project_acronyms_are_read_from_their_key() {
        let dir = tempdir().unwrap();
        assert!(ProjectConfig::load(dir.path()).acronyms().is_empty());
        ProjectConfig::default().write(dir.path()).unwrap();
        let raw = std::fs::read_to_string(dir.path().join("mustard.json")).unwrap();
        assert!(!raw.contains("acronyms"), "no list, no key: {raw}");

        std::fs::write(dir.path().join("mustard.json"), r#"{"acronyms":[" PI ","pcp","","PI"]}"#).unwrap();
        let cfg = ProjectConfig::load(dir.path());
        assert_eq!(cfg.acronyms(), ["PI", "PCP"]);
        assert!(!cfg.extra.contains_key("acronyms"), "the key is part of the schema");
        cfg.write(dir.path()).unwrap();
        let raw: Value = serde_json::from_str(&std::fs::read_to_string(dir.path().join("mustard.json")).unwrap()).unwrap();
        assert_eq!(raw["acronyms"], serde_json::json!([" PI ", "pcp", "", "PI"]), "the file keeps what was written");
    }

    /// As chaves antigas de idioma e a do tom não são mais lidas: um projeto
    /// que só as tem não declarou idioma nenhum. Elas continuam no arquivo,
    /// como qualquer chave desconhecida.
    #[test]
    fn the_old_language_keys_and_the_tone_are_kept_but_never_read() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("mustard.json"),
            r#"{"specLang":"en-US","lang":"en-US","tone":"technical"}"#,
        )
        .unwrap();
        let cfg = ProjectConfig::load(dir.path());
        assert_eq!(cfg.language(), Language::default(), "nothing is read from the old keys");
        for key in ["specLang", "lang", "tone"] {
            assert!(cfg.extra.contains_key(key), "{key} survives as an unknown key");
        }
        cfg.write(dir.path()).unwrap();
        let raw = std::fs::read_to_string(dir.path().join("mustard.json")).unwrap();
        assert!(raw.contains("\"specLang\"") && !raw.contains("\"language\""), "{raw}");
    }

    #[test]
    fn inject_round_trips_through_write_and_load() {
        let dir = tempdir().unwrap();
        let cfg = ProjectConfig {
            inject: vec![
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
            ],
            ..Default::default()
        };
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
        let cfg = ProjectConfig {
            inject: vec![
                Injectable { on: "  ".into(), file: "x.md".into(), once: false },
                Injectable { on: "sessionStart".into(), file: String::new(), once: true },
                Injectable { on: " SessionStart ".into(), file: " a.md ".into(), once: true },
            ],
            ..Default::default()
        };
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

    /// A `worktree` block is no longer part of the schema — the harness puts
    /// nothing into a cut worktree beyond what git and its submodules bring, so
    /// there is nothing left to declare. A config that still carries the block
    /// must keep LOADING: it lands in the unknown-key catch-all like any custom
    /// key (preserved on write, read by nobody), and the modelled fields around
    /// it are unaffected.
    #[test]
    fn a_stale_worktree_declaration_is_inert_and_breaks_nothing() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("mustard.json"),
            r#"{"buildCommand":"make","worktree":{"carry":[".env"],"link":["node_modules"]}}"#,
        )
        .unwrap();
        let cfg = ProjectConfig::load(dir.path());
        assert_eq!(cfg.build_command.as_deref(), Some("make"), "the rest of the config still loads");
        assert!(
            cfg.extra.contains_key("worktree"),
            "the withdrawn block is an unknown key now — preserved, never interpreted",
        );
    }

    #[test]
    fn role_patterns_lowercased_and_filtered() {
        let cfg = ProjectConfig {
            role_patterns: vec![
                RolePattern { pattern: "Controllers".into(), role: "api".into() },
                RolePattern { pattern: " ".into(), role: "x".into() },
            ],
            ..Default::default()
        };
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

    /// As bases saem das chaves e dos valores do fluxo, e a chave `*` não é
    /// base nenhuma.
    #[test]
    fn as_bases_declaradas_saem_do_fluxo() {
        // Fluxo de dois degraus → {dev, main}.
        let mut cfg = ProjectConfig::default();
        cfg.git.flow.insert("*".into(), "dev".into());
        cfg.git.flow.insert("dev".into(), "main".into());
        let bases = cfg.git.declared_bases();
        assert!(bases.contains("dev") && bases.contains("main"));
        assert_eq!(bases.len(), 2, "a chave `*` não é base: {bases:?}");

        // Um degrau só → {main}.
        let mut single = ProjectConfig::default();
        single.git.flow.insert("*".into(), "main".into());
        assert_eq!(single.git.declared_bases(), BTreeSet::from(["main".to_string()]));

        // Fluxo develop/master — nenhum nome deste projeto aparece no código.
        let mut dm = ProjectConfig::default();
        dm.git.flow.insert("*".into(), "develop".into());
        dm.git.flow.insert("develop".into(), "master".into());
        assert_eq!(
            dm.git.declared_bases(),
            BTreeSet::from(["develop".to_string(), "master".to_string()]),
        );
    }

    /// Um projeto que não declara fluxo não declara base nenhuma: a lista sai
    /// vazia, e não com dois nomes que este repositório pode nem ter.
    #[test]
    fn sem_fluxo_o_projeto_nao_declara_base_nenhuma() {
        assert!(ProjectConfig::default().git.declared_bases().is_empty());
        assert_eq!(ProjectConfig::default().git.primary_base(), None);
    }

    /// A base do cursor é a do `*`; sem ela, a menor das declaradas.
    #[test]
    fn a_base_do_cursor_vem_do_fluxo_e_nunca_de_um_nome_fixo() {
        let mut cfg = ProjectConfig::default();
        cfg.git.flow.insert("*".into(), "develop".into());
        cfg.git.flow.insert("develop".into(), "master".into());
        assert_eq!(cfg.git.primary_base().as_deref(), Some("develop"));

        let mut no_star = ProjectConfig::default();
        no_star.git.flow.insert("develop".into(), "master".into());
        assert_eq!(no_star.git.primary_base().as_deref(), Some("develop"));
    }

    /// As duas chaves do projeto valem ligadas quando faltam; só o `false`
    /// escrito as desliga, e voltam ao arquivo como foram escritas.
    #[test]
    fn as_chaves_do_mustard_e_do_rtk_so_desligam_com_false_escrito() {
        let dir = tempdir().unwrap();
        let absent = ProjectConfig::load(dir.path());
        assert!(absent.enabled() && absent.rtk());

        std::fs::write(dir.path().join("mustard.json"), r#"{"enabled":false,"rtk":false}"#).unwrap();
        let off = ProjectConfig::load(dir.path());
        assert!(!off.enabled() && !off.rtk());
        off.write(dir.path()).unwrap();
        let raw = std::fs::read_to_string(dir.path().join("mustard.json")).unwrap();
        assert!(raw.contains("\"enabled\": false") && raw.contains("\"rtk\": false"), "{raw}");

        std::fs::write(dir.path().join("mustard.json"), "{ not json").unwrap();
        assert!(ProjectConfig::load(dir.path()).enabled(), "an unreadable file turns nothing off");
    }
}
