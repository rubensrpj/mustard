//! `project_seed` — the install/update engine for Mustard in a project.
//!
//! ## What it owns
//!
//! One capability, shared by every installer face: lay the Mustard footprint
//! down in a project — the harness settings, Mustard's own texts (the session
//! map under `.claude/mustard/` and the three agents under
//! `.claude/agents/mustard/`), `.claude/.gitignore`, and the single
//! project-root `mustard.json` — idempotently, and **merge-first for what the
//! OPERATOR owns**: an existing settings file, `.claude/.gitignore` or
//! `mustard.json` survives and only what is missing is created (for
//! `.claude/.gitignore`, which is a rule list rather than a document, "what is
//! missing" is read line by line). Mustard's own texts are NOT in that
//! category: they are always rewritten, in the language of `language.text`,
//! and the reason is written where they are seeded ([`files`]).
//!
//! Consumers:
//!
//! - `mustard init` (the CLI): calls the granular seeders with its own
//!   overwrite/merge decision, keeping its exclusive concerns (location guard,
//!   interactive git-flow prompts, `.github/`) in the CLI.
//! - `mustard-rt run upsert` (the plugin's bootstrap door): calls
//!   [`upsert_project`], the always-merge composition, prints the
//!   [`UpsertReport`], which says what the [`cleanup`] took out.
//!
//! The engine never records anything in git: no stage, no commit. What it
//! writes stays where it landed, and a commit is always the person's.
//!
//! The seed *content* comes from [`crate::platform::seeds`] (compiled-in
//! constants) — no `templates/` directory lookup is involved.
//!
//! ## The five parts
//!
//! - this file — the composition, the report and the names every part shares;
//! - [`footprint`] — what an install can put in a project, and the install
//!   mode that decides whether the host repository's git sees it;
//! - [`settings`] — the harness settings, their point migrations and the two
//!   switches kept there (the rtk hook and Claude Code's own signature);
//! - [`files`] — Mustard's own texts, `.claude/.gitignore`, `mustard.json`
//!   and the migrations of its `inject` list;
//! - [`cleanup`] — what an older Mustard wrote into files that are not its
//!   own, taken out by the upsert with no question.
//!
//! ## Contracts honoured
//!
//! - Writes go through [`fs::write_atomic`] only; nothing here panics
//!   (`unwrap`/`expect` are `deny` outside tests) and the migrations are
//!   fail-open (an IO error degrades to "nothing migrated").
//! - `mustard.json` is touched exclusively through [`ProjectConfig`], its
//!   single owner.
//! - Nothing is read from or written to `~/.claude/`.
//! - No `println!`: this is a library engine. Callers render the outcomes
//!   (the CLI prints didactic lines, the runtime prints the JSON report).

use std::path::Path;

use serde::Serialize;

use crate::domain::config::ProjectConfig;
use crate::io::fs;
use crate::platform::error::{Error, Result};
use crate::platform::git_exclude;

pub mod cleanup;
pub mod files;
pub mod footprint;
pub mod settings;

pub use cleanup::{CleanupDone, CleanupPlan, PendingList};
pub use files::{
    default_inject_entries, harness_text_paths, harness_texts, migrate_inject_declarations,
    project_page_template_path, same_declared_path, seed_gitignore, seed_harness_texts,
    session_map_declared_path,
};
pub use footprint::{
    carries_private_marks, detect_install_mode, footprint, footprint_pathspecs, footprint_rules,
    is_written_footprint, FootprintEntry, InstallMode, PRIVATE_MARKS,
};
pub use settings::{
    output_style_for, retire_planted_plugin_enablement, seed_settings, Switches, PAGE_DATABASE_TOOL,
    RTK_HOOK_COMMAND,
};

/// `.claude/settings.json` — the shared-mode settings seed, and the team's file.
const SETTINGS_JSON: &str = ".claude/settings.json";
/// `.claude/settings.local.json` — its untracked local-layer twin.
const SETTINGS_LOCAL_JSON: &str = ".claude/settings.local.json";
/// `.claude/.gitignore` — the ephemeral-state cover.
const CLAUDE_GITIGNORE_PATH: &str = ".claude/.gitignore";
/// `mustard.json` — the single project-root config.
const MUSTARD_JSON: &str = "mustard.json";
/// The same file as an ignore RULE. The leading slash anchors it to the
/// repository root, which is the only `mustard.json` an install writes: without
/// it the bare name would match at every depth and hide a `mustard.json` the
/// CLIENT keeps in one of their own subprojects.
const MUSTARD_JSON_RULE: &str = "/mustard.json";
/// The shared per-directory instruction file an older scan wrote Guards into.
///
/// `pub` because it is one half of the pair every reader of a subproject's
/// Guards has to resolve between — see [`CLAUDE_LOCAL_MD`].
pub const CLAUDE_MD: &str = "CLAUDE.md";
/// Its untracked local-layer twin, which an older private scan wrote instead.
///
/// `pub` for the same reason as [`CLAUDE_MD`]: every reader of the Guards must
/// spell this ONE literal.
pub const CLAUDE_LOCAL_MD: &str = "CLAUDE.local.md";
/// The pull-request template the CLI seeds when it finds a GitHub remote.
const GITHUB_PR_TEMPLATE: &str = ".github/pull_request_template.md";

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
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
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
    /// Migrations of `mustard.json#inject` performed (see
    /// [`migrate_inject_declarations`]).
    pub migrated: Vec<String>,
    /// Whether this run installed in [`InstallMode::Private`].
    ///
    /// This and the three fields below skip serialization when empty, so a
    /// shared install's JSON is byte-identical to what it was before the mode
    /// existed.
    #[serde(default, skip_serializing_if = "is_false")]
    pub private: bool,
    /// The clone-local exclude rules this run appended. Empty on a converged
    /// second run — the append is idempotent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded: Vec<String>,
    /// [`footprint_pathspecs`] entries the host repository ALREADY tracks. An
    /// ignore rule cannot hide a tracked path, so these are residue: the
    /// install names them and unlinks nothing — `git rm --cached` rewrites the
    /// host's index, which is the operator's decision.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub already_tracked: Vec<String>,
    /// Why no rule could be written at all (no git, no repository, an
    /// unreadable or unwritable exclude file). Reported, never an error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_unavailable: Option<String>,
    /// What an older Mustard left in files that are not its own: the files this
    /// run took it out of, with what left each one, the rules of the Guards
    /// that went to the pending list, and the files without a mark, which are
    /// only listed and never touched. Absent when there is nothing to list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanup: Option<CleanupPlan>,
    /// What taking that list out did: the files edited and deleted, the
    /// number of the pending item that holds the rules, and what failed.
    /// Absent when the list changed nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleaned: Option<CleanupDone>,
}

/// `skip_serializing_if` predicate for the additive booleans above — a `false`
/// flag is simply absent, keeping the shared-install shape unchanged.
///
/// The `&bool` is serde's signature, not a choice: `skip_serializing_if` calls
/// the predicate with a reference to the field.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !*b
}

impl UpsertReport {
    /// `mustard.json` changed after this report was made, by a later step of
    /// the same install, like the answers `run upsert` records: it moves from
    /// `preserved` to `updated`. A file this run created stays created.
    pub fn record_mustard_json_change(&mut self) {
        if self.created.iter().chain(&self.updated).any(|name| name == MUSTARD_JSON) {
            return;
        }
        self.preserved.retain(|name| name != MUSTARD_JSON);
        self.updated.push(MUSTARD_JSON.to_string());
    }

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

/// Install or update Mustard in the project rooted at `root` — idempotent, and
/// merge-only for everything the OPERATOR owns: the settings file,
/// `.claude/.gitignore` and the project-root `mustard.json` survive when they
/// exist, and only what is missing is created or backfilled.
///
/// Mustard's own texts — every path [`harness_texts`] names — are the
/// exception, and take no merge decision from anybody: they are the harness's
/// own text rather than project configuration, so every run writes the
/// compiled-in body again, in the language of `language.text`. A copy that
/// diverged is REPLACED, and the replacement is reported as
/// [`SeedOutcome::Updated`], never [`SeedOutcome::Preserved`] — see
/// [`seed_harness_texts`].
///
/// Steps, in order:
///
/// 1. the `inject` migrations ([`migrate_inject_declarations`]) — they only
///    touch `mustard.json`;
/// 2. the settings file — seed when absent, backfill missing top-level keys
///    when present, with the point migrations of [`seed_settings`], rtk's hook
///    following `mustard.json#rtk`, Claude Code's signature kept off and the
///    response style of `language.text` chosen — and, in either mode, the
///    folder of the project's separate copies allowed in the local settings;
/// 3. Mustard's own texts — the compiled-in body of each is written every
///    time;
/// 4. `.claude/.gitignore` — created when absent, and when present the pattern
///    lines it lacks are appended (see [`seed_gitignore`]);
/// 5. `mustard.json` (via [`ProjectConfig`], the single owner) — created with
///    defaults when absent; when present only `version` is re-stamped (and
///    only when `version` is `Some`), an empty `inject` is backfilled, and an
///    absent `runtime` is filled — everything else is preserved verbatim;
/// 6. the cleanup ([`cleanup::plan`], then [`cleanup::apply`]) — what an older
///    Mustard left between its marks is taken out in this same call, with no
///    question: the rules of the Guards go first to the project's pending list
///    (see [`upsert_project_with`]), and a file without a mark is only listed.
///
/// Nothing is staged or committed, whatever git tracks: the stamp in a
/// versioned `mustard.json` stays a change for the person to commit.
///
/// `version` is supplied by the caller because the core does not own a
/// product version.
///
/// Under [`InstallMode::Private`] a step 0 runs first — [`footprint_rules`] is
/// written into the clone-local exclude file, and whatever the host repository
/// already tracks under [`footprint_pathspecs`] is recorded as residue — so no
/// seed is ever momentarily visible to that repository's git. When that write
/// cannot happen inside a repository that exists, step 0 REFUSES and no step
/// after it runs. Step 2 then targets the local settings layer.
///
/// # Errors
///
/// An IO or serialization failure from any seeding step. The migrations are
/// fail-open and never error. The private step errors — [`Error::NotHidden`] —
/// only when a REAL repository refused the exclude write, and it does so before
/// anything at all has been written.
pub fn upsert_project(
    root: &Path,
    version: Option<&str>,
    mode: InstallMode,
) -> Result<UpsertReport> {
    upsert_project_with(root, version, mode, &cleanup::NoPendingList)
}

/// [`upsert_project`] with the project's pending list, `pending`, where the
/// cleanup writes the rules it takes out of the instruction files, in one
/// item, before any file changes. The core does not own that list: the
/// runtime's `run upsert` hands its door in. Without it ([`upsert_project`]),
/// the rules cannot be written down, so no file of the cleanup changes while
/// a rule would leave; every other step runs the same.
///
/// # Errors
///
/// The same as [`upsert_project`].
pub fn upsert_project_with(
    root: &Path,
    version: Option<&str>,
    mode: InstallMode,
    pending: &dyn PendingList,
) -> Result<UpsertReport> {
    let installed_before = ProjectConfig::exists(root);
    let mut report = UpsertReport {
        installed_before,
        version: version.map(str::to_string),
        ..UpsertReport::default()
    };

    // 0. Private mode: hide the footprint BEFORE any of it is written — before
    //    even `.claude/` is created. The two halves read two different
    //    projections of the ONE declaration — the rules are what an install may
    //    hide, the pathspecs are what it may ask about, and they are not the
    //    same set (see `FootprintEntry`).
    //
    //    This is the one step that can REFUSE. If git resolved an exclude file
    //    in a real repository and the write still did not land, the seeds below
    //    would land VISIBLY in a client's git while the report said "private".
    //    Nothing is written on that path.
    if mode.is_private() {
        let outcome = git_exclude::ensure_excluded(root, &footprint_rules());
        if let Some(failure) = outcome.unavailable {
            if failure.is_blocking() {
                return Err(Error::NotHidden(failure.reason().to_string()));
            }
            report.exclude_unavailable = Some(failure.reason().to_string());
        }
        report.private = true;
        report.excluded = outcome.appended;
        report.already_tracked = git_exclude::tracked_paths(root, &footprint_pathspecs());
    }

    let claude_dir = root.join(".claude");
    fs::create_dir_all(&claude_dir)?;

    // 1. The `inject` migrations (fail-open).
    report.migrated = migrate_inject_declarations(root, &claude_dir);

    // 2..4. The `.claude/` seeds, merge-mode — except Mustard's own texts,
    //       which take no mode: they are always rewritten.
    let config = ProjectConfig::load(root);
    let text = config.language().text_or_default();
    for (name, outcome) in seed_settings(&claude_dir, false, mode, config.rtk(), text)? {
        report.record(name, outcome);
    }
    for (rel, outcome) in seed_harness_texts(&claude_dir, text)? {
        report.record(&format!(".claude/{rel}"), outcome);
    }
    report.record(CLAUDE_GITIGNORE_PATH, seed_gitignore(&claude_dir, false)?);

    // 5. The single project-root mustard.json.
    let outcome = files::upsert_mustard_json(root, version)?;
    report.record(MUSTARD_JSON, outcome);

    // 6. What an older Mustard left in files that are not its own is its own
    //    leftover, and leaves now. A shared install writes the seed into
    //    `.claude/settings.json` itself, so there that file is the install's
    //    and not the team's.
    let mut plan = cleanup::plan(root);
    if !mode.is_private() {
        plan.files.retain(|change| change.path != SETTINGS_JSON);
    }
    if plan.has_changes() {
        report.cleaned = Some(cleanup::apply(root, &plan, pending)?);
    }
    report.cleanup = (!plan.is_empty()).then_some(plan);

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::i18n::Locale;
    use crate::platform::seeds::session_map;
    use serde_json::{json, Value};
    use std::fs as std_fs;
    use tempfile::tempdir;

    // --- upsert_project: fresh install --------------------------------------

    #[test]
    fn fresh_upsert_creates_everything() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        let report = upsert_project(root, Some("9.9.9"), InstallMode::Shared).unwrap();

        assert!(!report.installed_before, "no mustard.json existed before");
        assert_eq!(report.version.as_deref(), Some("9.9.9"));
        assert_eq!(
            report.created,
            vec![
                ".claude/settings.json",
                ".claude/settings.local.json",
                ".claude/mustard/session-map.md",
                ".claude/mustard/pages/spec.html",
                ".claude/mustard/pages/project.html",
                ".claude/agents/mustard/wave.md",
                ".claude/agents/mustard/review.md",
                ".claude/agents/mustard/skill.md",
                ".claude/.gitignore",
                "mustard.json",
            ],
            "every seed is created on a fresh project"
        );
        assert!(report.updated.is_empty());
        assert!(report.preserved.is_empty());
        assert!(report.migrated.is_empty());
        assert_eq!(report.cleanup, None, "a fresh project has nothing to clean");

        // The seeds landed with the compiled-in content.
        let settings: Value = serde_json::from_str(
            &std_fs::read_to_string(root.join(".claude/settings.json")).unwrap(),
        )
        .unwrap();
        assert!(settings.get("statusLine").is_some(), "real seed content laid down");
        assert_eq!(
            std_fs::read_to_string(root.join(".claude/mustard/session-map.md")).unwrap(),
            session_map(Locale::PtBr),
            "a project that declares no language gets the pt-BR text",
        );
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
        let report = upsert_project(dir.path(), None, InstallMode::Shared).unwrap();
        assert_eq!(report.version, None);
        let config = ProjectConfig::load(dir.path());
        assert_eq!(config.version, None, "no stamp when the caller withheld a version");
    }

    #[test]
    fn upsert_is_idempotent_second_run_preserves_all() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        upsert_project(root, Some("9.9.9"), InstallMode::Shared).unwrap();

        let second = upsert_project(root, Some("9.9.9"), InstallMode::Shared).unwrap();

        assert!(second.installed_before);
        assert!(second.created.is_empty(), "nothing to create: {:?}", second.created);
        assert!(second.updated.is_empty(), "nothing changed: {:?}", second.updated);
        assert_eq!(
            second.preserved,
            vec![
                ".claude/settings.json",
                ".claude/settings.local.json",
                ".claude/mustard/session-map.md",
                ".claude/mustard/pages/spec.html",
                ".claude/mustard/pages/project.html",
                ".claude/agents/mustard/wave.md",
                ".claude/agents/mustard/review.md",
                ".claude/agents/mustard/skill.md",
                ".claude/.gitignore",
                "mustard.json",
            ],
        );
    }

    /// Instalar e atualizar deixa na pasta dos agentes do Mustard só os três
    /// de hoje — onda, revisão e skill —, no idioma do projeto, nos dois
    /// idiomas. O projeto de uma versão antiga, que ainda tem o agente de onda
    /// de tarefa única, perde esse arquivo na atualização, que diz o que
    /// tirou; o agente do projeto com o mesmo nome, fora da pasta do Mustard,
    /// fica como está; e a atualização seguinte não tem mais nada a tirar. O
    /// produto não traz mais o molde dele em idioma nenhum.
    #[test]
    fn an_update_removes_the_retired_single_task_wave_agent() {
        let today = ["review.md", "skill.md", "wave.md"];
        let files_in = |dir: &Path| -> Vec<String> {
            let mut names: Vec<String> = std_fs::read_dir(dir)
                .unwrap()
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        };

        for text in [Locale::PtBr, Locale::EnUs] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            std_fs::write(root.join("mustard.json"), format!(r#"{{"language":{{"text":"{}"}}}}"#, text.as_str()))
                .unwrap();
            upsert_project(root, Some("9.9.9"), InstallMode::Shared).unwrap();
            let agents = root.join(".claude/agents/mustard");
            assert_eq!(files_in(&agents), today, "the {text} install seeds another set of agents");

            // A instalação antiga: o agente de tarefa única ainda na pasta do
            // Mustard, e um agente do próprio projeto com o mesmo nome.
            std_fs::write(agents.join("wave-solo.md"), "---\nname: mustard-wave-solo\n---\n\nO molde antigo.\n").unwrap();
            let own = "---\nname: wave-solo\n---\n\nO agente do projeto.\n";
            std_fs::write(root.join(".claude/agents/wave-solo.md"), own).unwrap();

            let report = upsert_project(root, Some("9.9.9"), InstallMode::Shared).unwrap();
            assert_eq!(files_in(&agents), today, "the {text} update left the retired agent behind");
            for (name, body) in crate::platform::seeds::agent_texts(text) {
                assert_eq!(std_fs::read_to_string(agents.join(format!("{name}.md"))).unwrap(), body, "{text} `{name}`");
            }
            assert_eq!(
                report.migrated,
                vec![".claude/agents/mustard/wave-solo.md (retired agent)".to_string()],
                "the {text} update does not say what it took out",
            );
            assert_eq!(
                std_fs::read_to_string(root.join(".claude/agents/wave-solo.md")).unwrap(),
                own,
                "the project's own agent changed",
            );

            let again = upsert_project(root, Some("9.9.9"), InstallMode::Shared).unwrap();
            assert!(again.migrated.is_empty(), "nothing is left to retire: {:?}", again.migrated);
        }

        for lang in ["pt-BR", "en-US"] {
            let shipped = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates/agents").join(lang);
            assert_eq!(files_in(&shipped), today, "the product still ships another {lang} agent template");
        }
    }

    // --- upsert_project: merge over user files -------------------------------

    #[test]
    fn merge_preserves_user_files_and_backfills_missing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // A diverged session map + a settings.json with a user key + a curated
        // mustard.json (own inject list, own version, English text).
        std_fs::create_dir_all(root.join(".claude/mustard")).unwrap();
        std_fs::write(root.join(".claude/mustard/session-map.md"), "USER EDIT").unwrap();
        std_fs::write(
            root.join(".claude/settings.json"),
            "{\n  \"userKey\": true\n}\n",
        )
        .unwrap();
        std_fs::write(
            root.join("mustard.json"),
            r#"{"version":"1.0.0","buildCommand":"make","language":{"text":"en-US"},"inject":[{"on":"sessionStart","file":"docs/my-rules.md","once":false}]}"#,
        )
        .unwrap();

        let report = upsert_project(root, Some("9.9.9"), InstallMode::Shared).unwrap();

        assert!(report.installed_before);
        // The session map is NOT a user file: it goes back to the seed, in the
        // declared language, and the report says `Updated` so the overwrite is
        // never silent.
        assert_eq!(
            std_fs::read_to_string(root.join(".claude/mustard/session-map.md")).unwrap(),
            session_map(Locale::EnUs),
        );
        assert!(report.created.contains(&".claude/.gitignore".to_string()));
        assert!(report.updated.contains(&".claude/mustard/session-map.md".to_string()));
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

        upsert_project(root, None, InstallMode::Shared).unwrap();

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

        let report = upsert_project(root, None, InstallMode::Shared).unwrap();

        let config = ProjectConfig::load(root);
        assert_eq!(config.inject, default_inject_entries(), "empty inject backfilled");
        assert_eq!(config.build_command.as_deref(), Some("make"), "rest preserved");
        assert!(report.updated.contains(&"mustard.json".to_string()));
    }

    // --- report determinism ---------------------------------------------------

    #[test]
    fn report_serializes_deterministically_without_volatile_fields() {
        let dir = tempdir().unwrap();
        let report = upsert_project(dir.path(), Some("9.9.9"), InstallMode::Shared).unwrap();
        let json = serde_json::to_string_pretty(&report).unwrap();
        assert!(json.contains("\"installedBefore\": false"));
        assert!(json.contains("\"version\": \"9.9.9\""));
        assert!(!json.contains("timestamp"), "no timestamps in the report");
        let root_str = dir.path().to_string_lossy().into_owned();
        assert!(
            !json.contains(&root_str.replace('\\', "\\\\")),
            "no absolute paths in the report: {json}"
        );
        // The private half is ABSENT, not false/empty: a shared install's JSON
        // is byte-identical to what it was before the mode existed.
        for key in ["private", "excluded", "alreadyTracked", "excludeUnavailable"] {
            assert!(!json.contains(key), "{key} leaked into a shared report: {json}");
        }
    }

    // --- the cleanup, in the same call -----------------------------------------

    /// Um instalador nunca grava no git: num repositório que versiona o
    /// `mustard.json`, a árvore limpa fica com o selo novo por commitar, e o
    /// histórico não ganha commit nenhum.
    #[test]
    fn an_install_never_commits_what_it_wrote() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let git = |args: &[&str]| crate::platform::git::run(root, args);
        for args in [
            &["init", "-q"][..],
            &["config", "user.email", "t@example.com"],
            &["config", "user.name", "t"],
        ] {
            assert!(git(args).ok, "git {args:?}");
        }
        std_fs::write(root.join("mustard.json"), "{\n  \"version\": \"0.0.0-old\"\n}\n").unwrap();
        assert!(git(&["add", "mustard.json"]).ok && git(&["commit", "-q", "-m", "config"]).ok);
        let head = git(&["rev-parse", "HEAD"]).out();

        let report = upsert_project(root, Some("9.9.9"), InstallMode::Private).unwrap();

        assert!(report.updated.contains(&"mustard.json".to_string()), "{report:?}");
        assert_eq!(git(&["rev-parse", "HEAD"]).out(), head, "the install made a commit");
        let changed = git(&["diff", "--name-only"]).out().unwrap_or_default();
        assert_eq!(changed, "mustard.json", "the stamp is left for the person to commit");
        let staged = git(&["diff", "--cached", "--name-only"]).out().unwrap_or_default();
        assert_eq!(staged, "", "nothing is staged either");
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("stamp"), "the report says nothing about recording: {json}");
    }

    /// O `upsert` num projeto com as marcas do Mustard nos `CLAUDE.md`, um
    /// `CLAUDE.md` sem marca e as linhas do molde no `settings.json` da
    /// equipe: na mesma chamada, sem pergunta, sai só o que está entre as
    /// marcas, o import e a linha de navegação (o resto fica byte a byte, com
    /// os fins de linha), o arquivo que era só do Mustard é apagado, as linhas
    /// do molde saem do `settings.json`, o arquivo sem marca continua igual e
    /// só aparece na lista, e o relatório diz o que saiu. As regras tiradas
    /// das Guards vão à lista de pendências num item só, antes de qualquer
    /// arquivo mudar, e nunca ao banco de lições: sem como gravar a
    /// pendência, nenhum arquivo muda.
    #[test]
    fn migration_preserves_foreign_claude_md_and_is_byte_preserving() {
        /// A lista de pendências do teste: guarda na memória cada item.
        #[derive(Default)]
        struct Memory(std::cell::RefCell<Vec<(String, String)>>);
        impl PendingList for Memory {
            fn add(&self, title: &str, detail: &str) -> std::result::Result<String, String> {
                let mut items = self.0.borrow_mut();
                items.push((title.to_string(), detail.to_string()));
                Ok(format!("P-{}", items.len()))
            }
        }

        let dir = tempdir().unwrap();
        let root = dir.path();
        let write = |root: &Path, rel: &str, body: &str| {
            let path = root.join(rel);
            std_fs::create_dir_all(path.parent().unwrap()).unwrap();
            std_fs::write(path, body).unwrap();
        };
        let read = |root: &Path, rel: &str| std_fs::read(root.join(rel)).unwrap();
        // The team's own file, with the marks of an older scan in the middle.
        let team = "@.claude/scan-map.md\r\n# Api\r\n\r\nOur own rules stay.\r\n\r\n## Guards\r\n\r\n<!-- mustard:guards -->\r\n<!-- facts: kind=cargo -->\r\n- Reuse the shared client.\r\n<!-- /mustard:guards -->\r\n\r\nTeam tail.\r\n";
        // A file that is only what the scan wrote.
        let only_ours = "@.claude/scan-map.md\n\n# Web\n\n> Parent: [../../CLAUDE.md](../../CLAUDE.md) | Orchestrator: [../../.claude/mustard/orchestrator.md](../../.claude/mustard/orchestrator.md)\n\n## Guards\n\n<!-- mustard:guards -->\n- Never block the render.\n<!-- /mustard:guards -->\n";
        // The scan's traces without any mark (neither the import nor a block).
        let unmarked = "# Cli\n\n> Parent: [../../CLAUDE.md](../../CLAUDE.md) | Orchestrator: [../../.claude/mustard/orchestrator.md](../../.claude/mustard/orchestrator.md)\n\n## Guards\n\n- the team wrote this one\n";
        // A root file with nothing of Mustard's.
        let root_md = "# Team\n\nNothing of Mustard here.\n";
        // The team's settings, with one line of an older install's seed.
        let team_settings = "{\n  \"respectGitignore\": true,\n  \"teamKey\": 1\n}\n";
        let lay_out = |root: &Path| {
            write(root, "apps/api/CLAUDE.md", team);
            write(root, "apps/web/CLAUDE.md", only_ours);
            write(root, "apps/cli/CLAUDE.md", unmarked);
            write(root, "CLAUDE.md", root_md);
            write(root, SETTINGS_JSON, team_settings);
        };
        lay_out(root);

        // --- one call takes it out and says what left ---------------------------
        let list = Memory::default();
        let report = upsert_project_with(root, None, InstallMode::Private, &list).unwrap();
        let plan = report.cleanup.clone().expect("what left is listed");
        let listed: Vec<(&str, &cleanup::Action)> = plan.files.iter().map(|f| (f.path.as_str(), &f.action)).collect();
        assert_eq!(
            listed,
            [
                ("apps/api/CLAUDE.md", &cleanup::Action::Edit),
                ("apps/web/CLAUDE.md", &cleanup::Action::Delete),
                (SETTINGS_JSON, &cleanup::Action::Edit),
            ],
        );
        assert_eq!(plan.unmarked, ["apps/cli/CLAUDE.md"], "the file without a mark is only listed");
        let rules: Vec<&str> = plan.rules.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(rules, ["Reuse the shared client.", "Never block the render."]);
        let done = report.cleaned.clone().expect("the report says what the cleanup did");
        assert!(done.failed.is_empty(), "{done:?}");
        assert_eq!(done.edited, ["apps/api/CLAUDE.md", SETTINGS_JSON]);
        assert_eq!(done.deleted, ["apps/web/CLAUDE.md"]);
        assert_eq!(done.pending.as_deref(), Some("P-1"), "{done:?}");
        let detail = list.0.borrow()[0].1.clone();
        assert!(
            detail.contains("Reuse the shared client. (saiu de apps/api/CLAUDE.md)")
                && detail.contains("Never block the render. (saiu de apps/web/CLAUDE.md)"),
            "{detail}"
        );
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["cleaned"]["deleted"], json!(["apps/web/CLAUDE.md"]), "{json}");
        assert_eq!(json["cleaned"]["pending"], json!("P-1"), "{json}");
        assert!(json.get("confirm").is_none() && !json.to_string().contains("token"), "no code to confirm: {json}");

        assert_eq!(
            String::from_utf8(read(root, "apps/api/CLAUDE.md")).unwrap(),
            "# Api\r\n\r\nOur own rules stay.\r\n\r\n## Guards\r\n\r\n\r\nTeam tail.\r\n",
            "only what sits between the marks and the import leave; every other byte stays",
        );
        assert!(!root.join("apps/web/CLAUDE.md").exists(), "the file that was only Mustard's goes");
        assert_eq!(read(root, "apps/cli/CLAUDE.md"), unmarked.as_bytes(), "the unmarked file is never touched");
        assert_eq!(read(root, "CLAUDE.md"), root_md.as_bytes(), "a file with nothing of Mustard's is never touched");
        let settings: Value = serde_json::from_slice(&read(root, SETTINGS_JSON)).unwrap();
        assert_eq!(settings, json!({ "teamKey": 1 }), "only the seed's line leaves the team's settings");

        assert!(!root.join(".claude/spec/lessons.ndjson").exists(), "nothing goes to the lesson bank");
        let again = upsert_project_with(root, None, InstallMode::Private, &list).unwrap();
        assert!(again.cleaned.is_none(), "nothing left to take out: {again:?}");
        assert_eq!(list.0.borrow().len(), 1, "a second install writes nothing more");

        // --- the pending item comes first ---------------------------------------
        let blocked = tempdir().unwrap();
        lay_out(blocked.path());
        // Without the pending list, no rule can be written down.
        let report = upsert_project(blocked.path(), None, InstallMode::Private).unwrap();
        let done = report.cleaned.expect("the report says what the cleanup did");
        assert!(!done.failed.is_empty(), "the failure is said: {done:?}");
        assert!(done.edited.is_empty() && done.deleted.is_empty(), "{done:?}");
        assert_eq!(read(blocked.path(), "apps/api/CLAUDE.md"), team.as_bytes());
        assert_eq!(read(blocked.path(), "apps/web/CLAUDE.md"), only_ours.as_bytes());
        assert_eq!(read(blocked.path(), SETTINGS_JSON), team_settings.as_bytes());
    }

    /// O `upsert` tira, na mesma chamada, o `spec.md` e o `spec.html` que um
    /// binário mais antigo deixou dentro de uma pasta de spec que já tem o seu
    /// `spec.ndjson`: nenhuma pasta de obra guarda a página nem o texto dela
    /// depois da instalação. O arquivo de eventos, que é o único que deveria
    /// morar ali, fica como estava.
    #[test]
    fn upsert_takes_out_a_spec_folders_stale_page_and_document() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std_fs::create_dir_all(root.join(".claude/spec/arestas-da-suzano")).unwrap();
        std_fs::write(root.join(".claude/spec/arestas-da-suzano/spec.ndjson"), "{}\n").unwrap();
        std_fs::write(root.join(".claude/spec/arestas-da-suzano/spec.md"), "# old render\n").unwrap();
        std_fs::write(root.join(".claude/spec/arestas-da-suzano/spec.html"), "<html></html>").unwrap();

        let report = upsert_project(root, None, InstallMode::Shared).unwrap();

        let done = report.cleaned.expect("the report says what the cleanup did");
        assert!(done.failed.is_empty(), "{done:?}");
        assert_eq!(
            done.deleted,
            [".claude/spec/arestas-da-suzano/spec.md", ".claude/spec/arestas-da-suzano/spec.html"],
        );
        assert!(!root.join(".claude/spec/arestas-da-suzano/spec.md").exists());
        assert!(!root.join(".claude/spec/arestas-da-suzano/spec.html").exists());
        assert!(
            root.join(".claude/spec/arestas-da-suzano/spec.ndjson").exists(),
            "the event file is the one that stays"
        );

        // A segunda rodada não acha mais nada: a limpeza converge.
        let again = upsert_project(root, None, InstallMode::Shared).unwrap();
        assert!(again.cleanup.is_none(), "nothing left to clean the second time: {:?}", again.cleanup);
    }
}
