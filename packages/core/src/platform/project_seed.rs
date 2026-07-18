//! `project_seed` — the install/update engine for Mustard in a project.
//!
//! ## What it owns
//!
//! One capability, shared by every installer face: lay the Mustard footprint
//! down in a project — `.claude/settings.json`, the injectable instruction
//! files under `.claude/mustard/`, `.claude/.gitignore`, and the single
//! project-root `mustard.json` — **idempotently and merge-first** (an existing
//! user file is preserved; only what is missing is created). Consumers:
//!
//! - `mustard init` (the CLI): calls the granular seeders with its own
//!   overwrite/merge decision, keeping its exclusive concerns (location guard,
//!   interactive git-flow prompts, RTK/ripgrep, `.github/`) in the CLI.
//! - `mustard-rt run upsert` (the plugin's bootstrap door): calls
//!   [`upsert_project`], the always-merge composition, and prints the
//!   [`UpsertReport`].
//!
//! The seed *content* comes from [`crate::platform::seeds`] (compiled-in
//! constants) — no `templates/` directory lookup is involved.
//!
//! ## Contracts honoured
//!
//! - Writes go through [`fs::write_atomic`] only; nothing here panics
//!   (`unwrap`/`expect` are `deny` outside tests) and the migration is
//!   fail-open (an IO error degrades to "nothing migrated").
//! - `mustard.json` is touched exclusively through [`ProjectConfig`], its
//!   single owner.
//! - No `println!`: this is a library engine. Callers render the outcomes
//!   (the CLI prints didactic lines, the runtime prints the JSON report).

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::domain::command_detect::detect_commands;
use crate::domain::config::{Injectable, ProjectConfig, Runtime};
use crate::io::fs;
use crate::platform::error::Result;
use crate::platform::seeds::{CLAUDE_GITIGNORE, ORCHESTRATOR_MD, RESPONSE_STYLE_MD, SETTINGS_SEED};

// ---------------------------------------------------------------------------
// Report types
// ---------------------------------------------------------------------------

/// What one seeding step did to its target file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedOutcome {
    /// The file did not exist and was written.
    Created,
    /// The file existed and its content changed (backfill / overwrite).
    Updated,
    /// The file existed and was left byte-identical.
    Preserved,
}

/// The serializable result of one [`upsert_project`] run.
///
/// Field order is fixed by the struct (serde emits declaration order) and the
/// entry lists are pushed in a fixed sequence, so the serialized JSON is
/// deterministic — no timestamps, no absolute paths (every entry is
/// project-root-relative with forward slashes).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpsertReport {
    /// Whether `mustard.json` existed before this run (an update vs a first
    /// install).
    pub installed_before: bool,
    /// The version stamped into `mustard.json#version` this run, when one was
    /// supplied. `None` ⇒ the caller withheld a stamp and the existing value
    /// (if any) was preserved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Files created by this run.
    pub created: Vec<String>,
    /// Files that existed and were changed (key backfill, re-stamp).
    pub updated: Vec<String>,
    /// Files that existed and were left untouched.
    pub preserved: Vec<String>,
    /// Legacy-footprint migrations performed (see
    /// [`migrate_orchestrator_footprint`]).
    pub migrated: Vec<String>,
}

impl UpsertReport {
    /// Fold one file's [`SeedOutcome`] into the matching list.
    fn record(&mut self, name: &str, outcome: SeedOutcome) {
        let list = match outcome {
            SeedOutcome::Created => &mut self.created,
            SeedOutcome::Updated => &mut self.updated,
            SeedOutcome::Preserved => &mut self.preserved,
        };
        list.push(name.to_string());
    }
}

// ---------------------------------------------------------------------------
// The composed upsert
// ---------------------------------------------------------------------------

/// Install or update Mustard in the project rooted at `root` — idempotent,
/// always merge-mode (an existing user file is preserved; only what is
/// missing is created or backfilled).
///
/// Steps, in order:
///
/// 1. legacy-footprint migration ([`migrate_orchestrator_footprint`]) — runs
///    first so the old layout is gone when the new one lands;
/// 2. `.claude/settings.json` — seed when absent, backfill missing top-level
///    keys when present, always passing through
///    [`retire_planted_plugin_enablement`];
/// 3. `.claude/mustard/{orchestrator,response-style}.md` — created when
///    absent, a user-customised copy survives;
/// 4. `.claude/.gitignore` — created when absent;
/// 5. `mustard.json` (via [`ProjectConfig`], the single owner) — created with
///    defaults (empty `git.flow`, detected commands, default `inject`,
///    `runtime`, `version`) when absent; when present only `version` is
///    re-stamped (and only when `version` is `Some`), an empty `inject` is
///    backfilled, and an absent `runtime` is filled — everything else is
///    preserved verbatim.
///
/// `version` is supplied by the caller because the core does not own a
/// product version: the CLI passes its crate version (the canonical
/// `mustard.json` line), a caller with no authoritative version passes
/// `None` and the stamp is withheld.
///
/// # Errors
///
/// An IO or serialization failure from any seeding step. The migration step
/// is fail-open and never errors.
pub fn upsert_project(root: &Path, version: Option<&str>) -> Result<UpsertReport> {
    let installed_before = ProjectConfig::exists(root);
    let claude_dir = root.join(".claude");
    fs::create_dir_all(&claude_dir)?;

    let mut report = UpsertReport {
        installed_before,
        version: version.map(str::to_string),
        ..UpsertReport::default()
    };

    // 1. Migration away from the planted-orchestrator layout (fail-open).
    report.migrated = migrate_orchestrator_footprint(root, &claude_dir).migrated;

    // 2..4. The `.claude/` seeds, merge-mode.
    report.record(".claude/settings.json", seed_settings(&claude_dir, false)?);
    for (name, outcome) in seed_injectable_files(&claude_dir, false)? {
        report.record(&format!(".claude/mustard/{name}"), outcome);
    }
    report.record(".claude/.gitignore", seed_gitignore(&claude_dir, false)?);

    // 5. The single project-root mustard.json.
    let outcome = upsert_mustard_json(root, version)?;
    report.record("mustard.json", outcome);

    Ok(report)
}

// ---------------------------------------------------------------------------
// settings.json
// ---------------------------------------------------------------------------

/// Marketplace name older `init` builds planted in the PROJECT
/// `settings.json#extraKnownMarketplaces` (retired — see
/// [`retire_planted_plugin_enablement`]).
const PLUGIN_MARKETPLACE: &str = "mustard";

/// `settings.json#enabledPlugins` key older `init` builds planted (retired).
const PLUGIN_ID: &str = "mustard@mustard";

/// The placeholder URL those older builds wrote. Kept ONLY as the recognition
/// literal for the migration: an `extraKnownMarketplaces.mustard` entry whose
/// url equals this literal is provably ours and safe to remove; any other url
/// is user-authored and survives. Plugin enablement is the USER's choice at
/// user scope (`~/.claude/settings.json`) — the project seed never writes it.
const MARKETPLACE_REPO_URL: &str = "REPLACE_WITH_MUSTARD_PLUGIN_MARKETPLACE_GIT_URL";

/// Seed `.claude/settings.json` from the compiled-in [`SETTINGS_SEED`].
///
/// - Absent (or `overwrite == true`): the seed is the base.
/// - Present under merge: the user's file is the base and any top-level seed
///   key it lacks is backfilled — user edits are never clobbered.
///
/// Both paths pass through [`retire_planted_plugin_enablement`]. The file is
/// only rewritten when the serialized result differs from what is on disk, so
/// a settled project reports [`SeedOutcome::Preserved`].
///
/// # Errors
///
/// An IO error writing the file, or a serialization failure.
pub fn seed_settings(claude_dir: &Path, overwrite: bool) -> Result<SeedOutcome> {
    let dest = claude_dir.join("settings.json");
    let existing_raw = fs::read_to_string(&dest).ok();
    let existed = existing_raw.is_some();

    let seed = parse_json_object(SETTINGS_SEED);
    let mut settings = if overwrite || !existed {
        seed
    } else {
        // Merge: the user's file is the base (fail-open: a malformed file
        // degrades to the seed, matching the historical init semantics).
        let mut existing = existing_raw
            .as_deref()
            .map(parse_json_object)
            .unwrap_or_default();
        for (key, value) in seed {
            existing.entry(key).or_insert(value);
        }
        existing
    };

    retire_planted_plugin_enablement(&mut settings);

    let mut serialized = serde_json::to_string_pretty(&Value::Object(settings))?;
    serialized.push('\n');
    if existing_raw.as_deref() == Some(serialized.as_str()) {
        return Ok(SeedOutcome::Preserved);
    }
    fs::write_atomic(&dest, serialized.as_bytes())?;
    Ok(if existed { SeedOutcome::Updated } else { SeedOutcome::Created })
}

/// Remove the plugin-enablement pair older `init` builds planted in the
/// PROJECT settings — and ONLY that pair:
///
/// - `extraKnownMarketplaces.mustard` goes only when its url is the
///   [`MARKETPLACE_REPO_URL`] placeholder (provably ours; a user-authored
///   mustard marketplace with a real url survives).
/// - `enabledPlugins."mustard@mustard"` goes only when the marketplace entry
///   was ours-or-absent (an alias the user wired to a real marketplace stays).
///
/// Emptied containers are dropped so a clean project carries no residue.
/// Every other marketplace/plugin key is untouched.
pub fn retire_planted_plugin_enablement(settings: &mut Map<String, Value>) {
    let planted_marketplace = settings
        .get("extraKnownMarketplaces")
        .and_then(|m| m.get(PLUGIN_MARKETPLACE))
        .and_then(|e| e.pointer("/source/url"))
        .and_then(Value::as_str)
        == Some(MARKETPLACE_REPO_URL);
    if planted_marketplace {
        if let Some(obj) = settings
            .get_mut("extraKnownMarketplaces")
            .and_then(Value::as_object_mut)
        {
            obj.remove(PLUGIN_MARKETPLACE);
        }
    }
    let marketplace_present = settings
        .get("extraKnownMarketplaces")
        .and_then(|m| m.get(PLUGIN_MARKETPLACE))
        .is_some();
    if !marketplace_present {
        if let Some(obj) = settings.get_mut("enabledPlugins").and_then(Value::as_object_mut) {
            obj.remove(PLUGIN_ID);
        }
    }
    for container in ["extraKnownMarketplaces", "enabledPlugins"] {
        let emptied = settings
            .get(container)
            .and_then(Value::as_object)
            .is_some_and(Map::is_empty);
        if emptied {
            settings.remove(container);
        }
    }
}

/// Parse a JSON object fail-open: anything that is not a JSON object yields
/// an empty map (mirrors the CLI's historical `read_json_object` semantics).
fn parse_json_object(raw: &str) -> Map<String, Value> {
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Injectable instruction files + .gitignore
// ---------------------------------------------------------------------------

/// The injectable instruction files seeded under `.claude/mustard/`:
/// `(basename, compiled-in body)`.
const INJECTABLE_SEEDS: &[(&str, &str)] = &[
    ("orchestrator.md", ORCHESTRATOR_MD),
    ("response-style.md", RESPONSE_STYLE_MD),
];

/// Seed the injectable instruction files into `.claude/mustard/`.
///
/// Merge (`overwrite == false`): an existing file is preserved — a
/// user-customised injectable survives. Overwrite: the seed content is
/// (re)laid down. Returns `(basename, outcome)` per file, in declaration
/// order (deterministic).
///
/// # Errors
///
/// An IO error creating the directory or writing a file.
pub fn seed_injectable_files(
    claude_dir: &Path,
    overwrite: bool,
) -> Result<Vec<(String, SeedOutcome)>> {
    let dest_dir = claude_dir.join("mustard");
    fs::create_dir_all(&dest_dir)?;
    let mut out = Vec::with_capacity(INJECTABLE_SEEDS.len());
    for (name, body) in INJECTABLE_SEEDS {
        let dest = dest_dir.join(name);
        out.push(((*name).to_string(), seed_static_file(&dest, body, overwrite)?));
    }
    Ok(out)
}

/// Seed `.claude/.gitignore` (the ephemeral harness state cover). Merge
/// preserves an existing file; overwrite re-lays the seed.
///
/// # Errors
///
/// An IO error writing the file.
pub fn seed_gitignore(claude_dir: &Path, overwrite: bool) -> Result<SeedOutcome> {
    seed_static_file(&claude_dir.join(".gitignore"), CLAUDE_GITIGNORE, overwrite)
}

/// Write one static seed to `dest` honouring merge/overwrite, reporting what
/// happened. An existing byte-identical file is [`SeedOutcome::Preserved`]
/// even under overwrite (no gratuitous rewrite), and a file that exists but
/// cannot be read (a genuine IO error, not absence) is preserved too — never
/// stomp what we could not inspect.
fn seed_static_file(dest: &Path, body: &str, overwrite: bool) -> Result<SeedOutcome> {
    match fs::read_to_string(dest) {
        Ok(existing) => {
            if !overwrite || existing == body {
                return Ok(SeedOutcome::Preserved);
            }
            fs::write_atomic(dest, body.as_bytes())?;
            Ok(SeedOutcome::Updated)
        }
        Err(crate::platform::error::Error::NotFound(_)) => {
            fs::write_atomic(dest, body.as_bytes())?;
            Ok(SeedOutcome::Created)
        }
        Err(_) => Ok(SeedOutcome::Preserved),
    }
}

// ---------------------------------------------------------------------------
// mustard.json
// ---------------------------------------------------------------------------

/// The default `mustard.json#inject` declarations: the orchestrator rides
/// every session's first prompt, the response style rides the session start —
/// both once per session. Written in the same casing the docs use; the config
/// accessor lowercases `on` at read time.
#[must_use]
pub fn default_inject_entries() -> Vec<Injectable> {
    vec![
        Injectable {
            on: "userPromptSubmit".to_string(),
            file: ".claude/mustard/orchestrator.md".to_string(),
            once: true,
        },
        Injectable {
            on: "sessionStart".to_string(),
            file: ".claude/mustard/response-style.md".to_string(),
            once: true,
        },
    ]
}

/// Create or minimally update the project-root `mustard.json` through
/// [`ProjectConfig`] (the single owner).
///
/// Absent → created with an empty `git.flow` (the project decides later),
/// agnostically detected commands, the default `inject` declarations,
/// `runtime`, and `version` (when supplied). Present → `version` is re-stamped
/// (only when `Some` and different), an empty `inject` is backfilled with the
/// defaults, an absent `runtime` is filled — everything else is preserved
/// verbatim, and the file is not rewritten when nothing changed.
fn upsert_mustard_json(root: &Path, version: Option<&str>) -> Result<SeedOutcome> {
    let existed = ProjectConfig::exists(root);
    let mut config = ProjectConfig::load(root);

    if !existed {
        let commands = detect_commands(root);
        config.build_command = commands.build;
        config.test_command = commands.test;
        config.lint_command = commands.lint;
        config.type_check_command = commands.type_check;
        config.inject = default_inject_entries();
        config.runtime = Some(Runtime::detect());
        config.version = version.map(str::to_string);
        config.write(root)?;
        return Ok(SeedOutcome::Created);
    }

    let mut changed = false;
    if let Some(version) = version {
        if config.version.as_deref() != Some(version) {
            config.version = Some(version.to_string());
            changed = true;
        }
    }
    if config.inject.is_empty() {
        config.inject = default_inject_entries();
        changed = true;
    }
    if config.runtime.is_none() {
        config.runtime = Some(Runtime::detect());
        changed = true;
    }
    if !changed {
        return Ok(SeedOutcome::Preserved);
    }
    config.write(root)?;
    Ok(SeedOutcome::Updated)
}

// ---------------------------------------------------------------------------
// Legacy-footprint migration
// ---------------------------------------------------------------------------

/// Marker that identifies a Mustard-planted orchestrator file (vs a file the
/// user authored at the same path). Matches the heading the legacy
/// `templates/CLAUDE.md` always opened with.
const ORCHESTRATOR_MARKER: &str = "# Orchestrator Rules";

/// The exact `@import` line older `/scan` passes injected at the top of the
/// project-root `CLAUDE.md` (mirrors `scan_claude::MAP_IMPORT_LINE`).
const SCAN_MAP_IMPORT_LINE: &str = "@.claude/scan-map.md";

/// Prefix of the breadcrumb line older `/scan` passes wrote into the
/// project-root `CLAUDE.md` (the root form is Orchestrator-only).
const ORCHESTRATOR_BREADCRUMB_PREFIX: &str = "> Orchestrator:";

/// What [`migrate_orchestrator_footprint`] found and did — silent facts the
/// caller renders (the CLI prints didactic lines, the runtime folds
/// `migrated` into its report).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MigrationOutcome {
    /// Project-root-relative names of what was migrated: `.claude/CLAUDE.md`
    /// (deleted planted orchestrator) and/or `CLAUDE.md` (Mustard lines
    /// stripped).
    pub migrated: Vec<String>,
    /// A `.claude/CLAUDE.md` exists but carries no Mustard marker — it is the
    /// user's own file and was left untouched.
    pub foreign_claude_md: bool,
}

/// Idempotent migration away from the planted-orchestrator layout.
///
/// (a) `.claude/CLAUDE.md`: deleted when it carries the
/// [`ORCHESTRATOR_MARKER`] (it is ours); a file *without* the marker is the
/// user's own and survives untouched (reported via
/// [`MigrationOutcome::foreign_claude_md`]). (b) the project-root `CLAUDE.md`:
/// the exact [`SCAN_MAP_IMPORT_LINE`] and any line starting with
/// [`ORCHESTRATOR_BREADCRUMB_PREFIX`] are removed — every other byte
/// (including line endings) is preserved verbatim, and the file is only
/// rewritten when something actually changed. Fail-open throughout: an
/// unreadable file or a failed write degrades to "not migrated", never an
/// error.
pub fn migrate_orchestrator_footprint(root: &Path, claude_dir: &Path) -> MigrationOutcome {
    let mut outcome = MigrationOutcome::default();

    // (a) the planted orchestrator under .claude/.
    let legacy = claude_dir.join("CLAUDE.md");
    if legacy.is_file() {
        match fs::read_to_string(&legacy) {
            Ok(text) if text.contains(ORCHESTRATOR_MARKER) => {
                if fs::remove_file(&legacy).is_ok() {
                    outcome.migrated.push(".claude/CLAUDE.md".to_string());
                }
            }
            Ok(_) => outcome.foreign_claude_md = true,
            Err(_) => {}
        }
    }

    // (b) the root CLAUDE.md — give the file back to the user.
    let root_md = root.join("CLAUDE.md");
    let Ok(text) = fs::read_to_string(&root_md) else {
        return outcome; // absent or unreadable → nothing to migrate.
    };
    let cleaned = strip_mustard_root_lines(&text);
    if cleaned == text {
        return outcome; // nothing of ours in it — do not rewrite.
    }
    if fs::write_atomic(&root_md, cleaned.as_bytes()).is_ok() {
        outcome.migrated.push("CLAUDE.md".to_string());
    }
    outcome
}

/// Remove the Mustard-owned lines from a root `CLAUDE.md` body: the exact
/// [`SCAN_MAP_IMPORT_LINE`] and any line starting with
/// [`ORCHESTRATOR_BREADCRUMB_PREFIX`]. Line-terminator-preserving — every
/// surviving line keeps its original bytes (CRLF included), so the rest of the
/// file round-trips byte-for-byte.
fn strip_mustard_root_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        let content = content.strip_suffix('\r').unwrap_or(content);
        if content == SCAN_MAP_IMPORT_LINE {
            continue;
        }
        if content.starts_with(ORCHESTRATOR_BREADCRUMB_PREFIX) {
            continue;
        }
        out.push_str(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs as std_fs;
    use tempfile::tempdir;

    // --- upsert_project: fresh install --------------------------------------

    #[test]
    fn fresh_upsert_creates_everything() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        let report = upsert_project(root, Some("9.9.9")).unwrap();

        assert!(!report.installed_before, "no mustard.json existed before");
        assert_eq!(report.version.as_deref(), Some("9.9.9"));
        assert_eq!(
            report.created,
            vec![
                ".claude/settings.json",
                ".claude/mustard/orchestrator.md",
                ".claude/mustard/response-style.md",
                ".claude/.gitignore",
                "mustard.json",
            ],
            "every seed is created on a fresh project"
        );
        assert!(report.updated.is_empty());
        assert!(report.preserved.is_empty());
        assert!(report.migrated.is_empty());

        // The seeds landed with the compiled-in content.
        let settings: Value = serde_json::from_str(
            &std_fs::read_to_string(root.join(".claude/settings.json")).unwrap(),
        )
        .unwrap();
        assert!(settings.get("statusLine").is_some(), "real seed content laid down");
        assert!(
            std_fs::read_to_string(root.join(".claude/mustard/orchestrator.md"))
                .unwrap()
                .starts_with("# Orchestrator Rules")
        );
        assert!(root.join(".claude/mustard/response-style.md").is_file());
        assert!(
            std_fs::read_to_string(root.join(".claude/.gitignore"))
                .unwrap()
                .contains(".events/")
        );

        // mustard.json: empty git.flow, default inject, runtime, version.
        let config = ProjectConfig::load(root);
        assert!(config.git.flow.is_empty(), "git.flow starts empty — the project decides");
        assert_eq!(config.inject, default_inject_entries());
        assert!(config.runtime.is_some(), "runtime stamped");
        assert_eq!(config.version.as_deref(), Some("9.9.9"));
    }

    #[test]
    fn fresh_upsert_without_version_stamps_none() {
        let dir = tempdir().unwrap();
        let report = upsert_project(dir.path(), None).unwrap();
        assert_eq!(report.version, None);
        let config = ProjectConfig::load(dir.path());
        assert_eq!(config.version, None, "no stamp when the caller withheld a version");
    }

    #[test]
    fn upsert_is_idempotent_second_run_preserves_all() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        upsert_project(root, Some("9.9.9")).unwrap();

        let second = upsert_project(root, Some("9.9.9")).unwrap();

        assert!(second.installed_before);
        assert!(second.created.is_empty(), "nothing to create: {:?}", second.created);
        assert!(second.updated.is_empty(), "nothing changed: {:?}", second.updated);
        assert_eq!(
            second.preserved,
            vec![
                ".claude/settings.json",
                ".claude/mustard/orchestrator.md",
                ".claude/mustard/response-style.md",
                ".claude/.gitignore",
                "mustard.json",
            ],
        );
    }

    // --- upsert_project: merge over user files -------------------------------

    #[test]
    fn merge_preserves_user_files_and_backfills_missing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // A user-customised injectable + a settings.json with a user key + a
        // curated mustard.json (own inject list, own version).
        std_fs::create_dir_all(root.join(".claude/mustard")).unwrap();
        std_fs::write(root.join(".claude/mustard/orchestrator.md"), "USER EDIT").unwrap();
        std_fs::write(
            root.join(".claude/settings.json"),
            "{\n  \"userKey\": true\n}\n",
        )
        .unwrap();
        std_fs::write(
            root.join("mustard.json"),
            r#"{"version":"1.0.0","buildCommand":"make","inject":[{"on":"sessionStart","file":"docs/my-rules.md","once":false}]}"#,
        )
        .unwrap();

        let report = upsert_project(root, Some("9.9.9")).unwrap();

        assert!(report.installed_before);
        // The customised injectable survives; only the missing one is created.
        assert_eq!(
            std_fs::read_to_string(root.join(".claude/mustard/orchestrator.md")).unwrap(),
            "USER EDIT",
        );
        assert!(report.created.contains(&".claude/mustard/response-style.md".to_string()));
        assert!(report.preserved.contains(&".claude/mustard/orchestrator.md".to_string()));
        // settings.json: user key kept, missing seed keys backfilled.
        let settings: Value = serde_json::from_str(
            &std_fs::read_to_string(root.join(".claude/settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(settings.get("userKey"), Some(&json!(true)));
        assert!(settings.get("permissions").is_some(), "seed keys backfilled");
        assert!(report.updated.contains(&".claude/settings.json".to_string()));
        // mustard.json: version re-stamped, curated inject + commands preserved.
        let config = ProjectConfig::load(root);
        assert_eq!(config.version.as_deref(), Some("9.9.9"));
        assert_eq!(config.build_command.as_deref(), Some("make"));
        assert_eq!(config.inject.len(), 1, "curated inject list preserved");
        assert_eq!(config.inject[0].file, "docs/my-rules.md");
        assert!(report.updated.contains(&"mustard.json".to_string()));
    }

    #[test]
    fn existing_config_without_version_argument_keeps_its_version() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std_fs::create_dir_all(root.join(".claude")).unwrap();
        std_fs::write(root.join("mustard.json"), r#"{"version":"1.0.0"}"#).unwrap();

        upsert_project(root, None).unwrap();

        let config = ProjectConfig::load(root);
        assert_eq!(
            config.version.as_deref(),
            Some("1.0.0"),
            "a None version must never clobber the existing stamp"
        );
    }

    #[test]
    fn empty_inject_is_backfilled_on_existing_config() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std_fs::create_dir_all(root.join(".claude")).unwrap();
        std_fs::write(root.join("mustard.json"), r#"{"buildCommand":"make"}"#).unwrap();

        let report = upsert_project(root, None).unwrap();

        let config = ProjectConfig::load(root);
        assert_eq!(config.inject, default_inject_entries(), "empty inject backfilled");
        assert_eq!(config.build_command.as_deref(), Some("make"), "rest preserved");
        assert!(report.updated.contains(&"mustard.json".to_string()));
    }

    // --- migration -----------------------------------------------------------

    #[test]
    fn migration_removes_planted_orchestrator_and_root_lines() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let claude = root.join(".claude");
        std_fs::create_dir_all(&claude).unwrap();
        std_fs::write(claude.join("CLAUDE.md"), "# Orchestrator Rules\n\nrouter.\n").unwrap();
        std_fs::write(
            root.join("CLAUDE.md"),
            "@.claude/scan-map.md\n\n# (root)\n\n> Orchestrator: [x](x)\n\n## Guards\n\n- keep\n",
        )
        .unwrap();

        let report = upsert_project(root, None).unwrap();

        assert!(!claude.join("CLAUDE.md").exists(), "planted orchestrator deleted");
        let root_md = std_fs::read_to_string(root.join("CLAUDE.md")).unwrap();
        assert!(!root_md.contains("@.claude/scan-map.md"));
        assert!(!root_md.contains("> Orchestrator:"));
        assert!(root_md.contains("# (root)"), "user content survives: {root_md}");
        assert!(root_md.contains("- keep"), "user guard survives: {root_md}");
        assert_eq!(report.migrated, vec![".claude/CLAUDE.md", "CLAUDE.md"]);
    }

    #[test]
    fn migration_preserves_foreign_claude_md_and_is_byte_preserving() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let claude = root.join(".claude");
        std_fs::create_dir_all(&claude).unwrap();
        // A user-authored .claude/CLAUDE.md (no marker) must never be deleted.
        std_fs::write(claude.join("CLAUDE.md"), "MY OWN NOTES\n").unwrap();
        // CRLF root file with nothing of ours — must not be rewritten at all.
        let crlf_body = "# Mine\r\n\r\ncontent stays\r\n";
        std_fs::write(root.join("CLAUDE.md"), crlf_body).unwrap();

        let outcome = migrate_orchestrator_footprint(root, &claude);

        assert!(outcome.migrated.is_empty());
        assert!(outcome.foreign_claude_md, "foreign file reported, not deleted");
        assert_eq!(std_fs::read_to_string(claude.join("CLAUDE.md")).unwrap(), "MY OWN NOTES\n");
        assert_eq!(
            std_fs::read_to_string(root.join("CLAUDE.md")).unwrap(),
            crlf_body,
            "untouched file round-trips byte-for-byte"
        );

        // A CRLF file WITH our lines loses only them, keeping CRLF elsewhere.
        let mixed = "@.claude/scan-map.md\r\n# Mine\r\n> Orchestrator: [x](x)\r\nrest\r\n";
        std_fs::write(root.join("CLAUDE.md"), mixed).unwrap();
        let outcome = migrate_orchestrator_footprint(root, &claude);
        assert_eq!(outcome.migrated, vec!["CLAUDE.md"]);
        assert_eq!(
            std_fs::read_to_string(root.join("CLAUDE.md")).unwrap(),
            "# Mine\r\nrest\r\n",
            "surviving lines keep their CRLF terminators"
        );
    }

    // --- retire_planted_plugin_enablement (moved from the CLI init) ----------

    #[test]
    fn retire_removes_only_the_planted_placeholder_pair() {
        // The exact pair an older init planted: placeholder marketplace URL +
        // the mustard@mustard alias. Both go; the user's own keys survive.
        let mut settings: Map<String, Value> = serde_json::from_str(&format!(
            r#"{{"extraKnownMarketplaces":{{
                    "acme":{{"source":{{"source":"git","url":"x"}}}},
                    "mustard":{{"source":{{"source":"git","url":"{MARKETPLACE_REPO_URL}"}}}}}},
                "enabledPlugins":{{"acme@acme":true,"mustard@mustard":true}}}}"#,
        ))
        .unwrap();

        retire_planted_plugin_enablement(&mut settings);

        assert!(settings["extraKnownMarketplaces"].get("mustard").is_none(), "placeholder gone");
        assert!(settings["enabledPlugins"].get("mustard@mustard").is_none(), "alias gone");
        // Theirs survive.
        assert!(settings["extraKnownMarketplaces"].get("acme").is_some());
        assert_eq!(settings["enabledPlugins"]["acme@acme"], json!(true));
    }

    #[test]
    fn retire_preserves_a_user_authored_mustard_marketplace() {
        // A REAL url under the `mustard` key is the user's wiring — the entry
        // and the alias that resolves against it both stay.
        let mut settings: Map<String, Value> = serde_json::from_str(
            r#"{"extraKnownMarketplaces":{"mustard":{"source":{"source":"git","url":"https://example.com/real.git"}}},
                "enabledPlugins":{"mustard@mustard":true}}"#,
        )
        .unwrap();

        retire_planted_plugin_enablement(&mut settings);

        assert!(settings["extraKnownMarketplaces"].get("mustard").is_some(), "real url stays");
        assert_eq!(settings["enabledPlugins"]["mustard@mustard"], json!(true), "alias stays");
    }

    #[test]
    fn retire_drops_emptied_containers() {
        // A settings.json whose ONLY marketplace/plugin keys were ours ends up
        // with no residue containers at all.
        let mut settings: Map<String, Value> = serde_json::from_str(&format!(
            r#"{{"extraKnownMarketplaces":{{"mustard":{{"source":{{"source":"git","url":"{MARKETPLACE_REPO_URL}"}}}}}},
                "enabledPlugins":{{"mustard@mustard":true}}}}"#,
        ))
        .unwrap();

        retire_planted_plugin_enablement(&mut settings);

        assert!(settings.get("extraKnownMarketplaces").is_none(), "emptied container dropped");
        assert!(settings.get("enabledPlugins").is_none(), "emptied container dropped");
    }

    // --- report determinism ---------------------------------------------------

    #[test]
    fn report_serializes_deterministically_without_volatile_fields() {
        let dir = tempdir().unwrap();
        let report = upsert_project(dir.path(), Some("9.9.9")).unwrap();
        let json = serde_json::to_string_pretty(&report).unwrap();
        assert!(json.contains("\"installedBefore\": false"));
        assert!(json.contains("\"version\": \"9.9.9\""));
        assert!(!json.contains("timestamp"), "no timestamps in the report");
        let root_str = dir.path().to_string_lossy().into_owned();
        assert!(
            !json.contains(&root_str.replace('\\', "\\\\")),
            "no absolute paths in the report: {json}"
        );
    }
}
