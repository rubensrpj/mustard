//! `project_seed` — the install/update engine for Mustard in a project.
//!
//! ## What it owns
//!
//! One capability, shared by every installer face: lay the Mustard footprint
//! down in a project — `.claude/settings.json`, the injectable instruction
//! files under `.claude/mustard/`, `.claude/.gitignore`, and the single
//! project-root `mustard.json` — **idempotently and merge-first** (an existing
//! user file is preserved; only what is missing is created — for
//! `.claude/.gitignore`, which is a rule list rather than a document, "what is
//! missing" is read line by line). Consumers:
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
//! ## The two modes
//!
//! [`InstallMode`] decides which footprint the install lays down.
//! [`InstallMode::Shared`] is everything above, unchanged. Under
//! [`InstallMode::Private`] the same files still land on disk — the harness
//! reads them from fixed locations — but none of them is visible to the host
//! repository's git: the settings go to the untracked local layer
//! (`.claude/settings.local.json`), and [`footprint_rules`] is written into the
//! clone-local exclude file through [`crate::platform::git_exclude`]. Whatever
//! that repository ALREADY tracks under [`footprint_pathspecs`] is reported as
//! residue, never unlinked.
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

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::domain::command_detect::detect_commands;
use crate::domain::config::{Injectable, ProjectConfig, Runtime};
use crate::io::claude_paths::ClaudePaths;
use crate::io::fs;
use crate::platform::error::{Error, Result};
use crate::platform::git_exclude;
use crate::platform::seeds::{CLAUDE_GITIGNORE, ORCHESTRATOR_MD, SETTINGS_SEED};

// ---------------------------------------------------------------------------
// Install mode + the footprint it hides
// ---------------------------------------------------------------------------

/// Which footprint an install lays down in the project.
///
/// The mode is a CALLER argument and lives in no versioned file: a knob in
/// `mustard.json` would itself be the trace the private mode exists to
/// remove — the setting would announce the tool it hides.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum InstallMode {
    /// Today's behaviour, unchanged: the footprint is an ordinary part of the
    /// project and travels in its git like any other file.
    #[default]
    Shared,
    /// The footprint exists on disk — the harness needs it there — but is
    /// invisible to this clone's git: nothing to stage, nothing to diff,
    /// nothing to push.
    Private,
}

impl InstallMode {
    /// `true` for [`Self::Private`]. Spelled once here so the two call sites
    /// that branch on the mode read the same way.
    #[must_use]
    pub fn is_private(self) -> bool {
        matches!(self, Self::Private)
    }
}

/// `.claude/settings.json` — the shared-mode settings seed.
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
/// The shared per-directory instruction file a full scan writes Guards into.
///
/// `pub` because it is one half of the pair every reader of a subproject's
/// Guards has to resolve between — see [`CLAUDE_LOCAL_MD`].
pub const CLAUDE_MD: &str = "CLAUDE.md";
/// Its untracked local-layer twin, which a private scan writes instead.
///
/// `pub` for the same reason as [`CLAUDE_MD`]: the writer (`scan --full`) and
/// every reader of the Guards must spell this ONE literal, or the private mode
/// would write a file nobody reads.
pub const CLAUDE_LOCAL_MD: &str = "CLAUDE.local.md";
/// The pull-request template the CLI seeds when it finds a GitHub remote.
const GITHUB_PR_TEMPLATE: &str = ".github/pull_request_template.md";
/// What the HARNESS — as opposed to the install — puts inside a `.claude/`
/// directory, wherever that directory sits.
///
/// The seeds below are covered by [`at_any_depth`], which lifts each of them
/// automatically. This list is the rest: the durable output the harness writes
/// while it WORKS, which the seeded `.claude/.gitignore` deliberately leaves
/// versioned because in a shared install it belongs to the repository — the
/// scan census, the pattern molds, the spec directories, the grain model.
///
/// It is an enumeration and not the directory itself, and that is the whole
/// lesson of the round that produced it. A single `**/.claude/` cover was
/// simpler and reached everything, including a `.claude/commands/their-command.md`
/// the CLIENT authored — measured in a real repository, invisible to their own
/// `git add -A`. **An exclude rule may name only what Mustard writes.** The
/// trade that accepts is deliberate: a future producer nobody adds here stays
/// VISIBLE in `git status`, which is a noisy failure the operator can see,
/// instead of a silent one that eats their work.
///
/// A trailing slash means "the directory and everything under it"; everything
/// else is one file. `no_rule_reaches_a_depth_that_is_not_ours` refuses any
/// entry a private install cannot be shown to produce.
///
/// The FILES are listed here; the DIRECTORIES are derived from
/// [`ClaudePaths::documented_dirs`] by [`harness_claude_output`] — the catalog
/// whose own documentation says to derive from it instead of hand-maintaining a
/// duplicate. Hand-maintaining it is exactly what shipped the leak this replaces:
/// the typed list omitted `plans/` (filled because Mustard's own settings seed
/// sets `plansDirectory`), `graph/` and the runtime scratch, and 18 real files
/// carrying the operator's own prompt titles stayed visible to the client's git.
const HARNESS_CLAUDE_FILES: &[&str] = &[
    ".artifacts.json",
    "grain.dictionary.json",
    "grain.equivalences.json",
    "grain.model.json",
    "scan-declined.json",
    "scan-map.md",
    // NOT `skills/`. The shelf is a directory a client may also author in; what
    // the scan writes there is the `{role}-pattern` mold, and every one of the
    // 30 in this repository carries that suffix. The rule is the mold, not the
    // shelf — the same distinction that took `.claude/commands/` back.
    "skills/*-pattern/",
];

/// The `.claude/` directories a CLIENT may also author in, so no rule of ours
/// may cover them wholesale.
///
/// This is the ONLY hand-maintained half, and it is the safe half to get wrong:
/// forgetting an entry here hides something of the client's (loud, and the
/// ownership ratchet refuses it), while forgetting a directory in the derived
/// half leaks something of ours. `skills` is here and its `*-pattern/` molds are
/// covered by name in [`HARNESS_CLAUDE_FILES`].
const CLIENT_AUTHORED_CLAUDE_DIRS: &[&str] =
    &["commands", "skills", "refs", "agents", ".obsidian"];

/// Every `.claude/`-relative name the harness writes: the files above plus every
/// documented directory that is not one a client authors in.
///
/// Derived, so a directory added to the catalog is covered here without anyone
/// remembering to. Sorted, because the rules are written verbatim into an
/// exclude file and the report must stay byte-stable.
fn harness_claude_output() -> Vec<String> {
    let mut out: Vec<String> = HARNESS_CLAUDE_FILES.iter().map(|s| (*s).to_string()).collect();
    out.extend(
        crate::io::claude_paths::ClaudePaths::documented_dirs()
            .into_iter()
            .filter(|dir| !CLIENT_AUTHORED_CLAUDE_DIRS.contains(dir))
            .map(|dir| format!("{dir}/")),
    );
    out.sort();
    out
}
/// The timestamped copy `mustard init` leaves beside `.claude/` when the
/// operator chooses "backup and overwrite".
///
/// Not a seed and not enumerable — the name carries a clock reading — but it is
/// still something Mustard put in the project, and it sits OUTSIDE `.claude/`,
/// where [`CLAUDE_DIR_ANY_DEPTH`] does not reach. A pattern whose only slash is
/// the trailing one is unanchored, so this matches wherever such a directory
/// appears.
const CLAUDE_BACKUP_DIRS: &str = ".claude.backup.*/";

/// One entry of the FOOTPRINT — what a Mustard install can put in a project.
///
/// The three fields exist because the footprint's consumers genuinely disagree
/// about what an entry IS, and handing one flat list of strings to all of them
/// is what shipped an install that hid the CLIENT's own files: a gitignore
/// pattern carrying no slash matches at every depth, while a `git ls-files`
/// pathspec is a path and a directory cover is neither.
///
/// `CLAUDE.md` is the sharp case. A private install never writes one — the
/// Guards go to `CLAUDE.local.md` BESIDE it — so it carries no [`rule`] at all;
/// a rule would hide an instruction file the operator authored FOR the client
/// from that client's own `git add -A`. It still carries a [`pathspec`],
/// because a host repository that already versions its own is precisely the
/// case the whole mode exists for, and it is [`written`] `false`, because the
/// residue advice (`git rm --cached`) would untrack THEIR work.
///
/// [`rule`]: Self::rule
/// [`pathspec`]: Self::pathspec
/// [`written`]: Self::written
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FootprintEntry {
    /// The clone-local exclude rule that hides it, in gitignore syntax. `None`
    /// for a path Mustard never writes.
    pub rule: Option<String>,
    /// The `git ls-files` pathspec that finds it in the host repository's index.
    /// `None` for an entry that is a SHAPE rather than a path (`**/.claude/`,
    /// `.claude.backup.*/`) — measured against real git, `ls-files` answers
    /// nothing for either, so passing them would only be noise.
    pub pathspec: Option<String>,
    /// Whether MUSTARD writes this path. Only a written path is residue the
    /// operator may clear with `git rm --cached`.
    pub written: bool,
}

/// An entry Mustard writes whose rule and pathspec are the same string — the
/// ordinary seed.
fn seeded(path: &str) -> FootprintEntry {
    FootprintEntry {
        rule: Some(path.to_string()),
        pathspec: Some(path.to_string()),
        written: true,
    }
}

/// An entry Mustard writes whose ignore RULE is spelled differently from its
/// pathspec (today: the root-anchored `mustard.json`).
fn anchored(rule: &str, pathspec: &str) -> FootprintEntry {
    FootprintEntry {
        rule: Some(rule.to_string()),
        pathspec: Some(pathspec.to_string()),
        written: true,
    }
}

/// An entry Mustard only WATCHES — the host's own file, asked about but never
/// hidden and never advised away.
fn watched(pathspec: &str) -> FootprintEntry {
    FootprintEntry {
        rule: None,
        pathspec: Some(pathspec.to_string()),
        written: false,
    }
}

/// A COVER: a rule by shape, with no path for `ls-files` to answer.
fn cover(rule: &str) -> FootprintEntry {
    FootprintEntry {
        rule: Some(rule.to_string()),
        pathspec: None,
        written: true,
    }
}

/// The same `.claude/` rule, lifted to reach a `.claude/` at ANY depth.
///
/// A gitignore pattern that carries an interior slash is anchored to the
/// directory its rule file governs — the repository root, for the clone-local
/// exclude file — so `.claude/settings.json` covers the ROOT `.claude/` and no
/// other. A full scan creates one `.claude/` per SUBPROJECT, and a leading `**/`
/// is what reaches them (the shape this repository's own root `.gitignore`
/// already uses for `**/.claude/settings.local.json`).
///
/// Derived rather than listed: every seed keeps ONE declaration and gains its
/// depth-reaching twin here, so a seed added later cannot be hidden at the root
/// and forgotten in a subproject.
///
/// The root-anchored original is kept alongside the twin rather than replaced by
/// it, even though the twin subsumes it as a pattern. The original is the string
/// two other things read: [`PRIVATE_MARKS`], which is how the mode is detected
/// off the exclude file, and [`footprint_pathspecs`], which is the PATH the
/// residue report names. A pattern is not a path, and collapsing the two is the
/// mistake [`FootprintEntry`] exists to prevent.
fn at_any_depth(rule: &str) -> Option<FootprintEntry> {
    rule.strip_prefix(".claude/")
        .map(|rest| cover(&format!("**/.claude/{rest}")))
}

/// The Mustard FOOTPRINT, declared in exactly one place.
///
/// Derived rather than re-typed: the seed entries are the same constants
/// [`upsert_project`] records, the injectable instruction files come straight
/// from [`INJECTABLE_SEEDS`] (so a new injectable is covered the day it is
/// added), and each of those seeds gains its depth-reaching twin through
/// [`at_any_depth`] instead of being spelled a second time. Three entries are
/// not seeds and are here on purpose:
///
/// - `settings.local.json` — so switching modes never leaves the other twin
///   visible;
/// - `CLAUDE.md` — watched, never hidden (see [`FootprintEntry`]);
/// - `.github/pull_request_template.md` — watched for the same reason from the
///   other direction. A private install explicitly REFUSES to seed it, so no
///   rule of ours may hide it; a repository that already tracks one (from an
///   earlier shared install, or because it is simply the client's) still has to
///   be named as residue, which is what the pathspec is for.
///
/// `CLAUDE.local.md` is the one rule deliberately left unanchored: a private
/// `scan --full` writes one per SUBPROJECT and the set cannot be enumerated, so
/// the rule has to reach every depth. It is safe there and nowhere else — the
/// name belongs to the untracked local layer by convention, so no rule of ours
/// is hiding a file a client would ever commit.
///
/// The list closes with the rules that are SHAPES rather than paths:
/// [`HARNESS_CLAUDE_OUTPUT`] lifted to every depth, and [`CLAUDE_BACKUP_DIRS`].
/// The seeds each name one file, and a rule that names one file can only hide
/// the files somebody thought of — a private install used to show a
/// subproject's whole `.claude/` and every spec directory the harness wrote
/// while working. These close that by naming what the harness produces, one
/// name at a time, because the shortcut of covering `.claude/` wholesale hid
/// the client's own files too.
#[must_use]
pub fn footprint() -> Vec<FootprintEntry> {
    let mut out = vec![seeded(SETTINGS_JSON), seeded(SETTINGS_LOCAL_JSON)];
    out.extend(
        INJECTABLE_SEEDS
            .iter()
            .map(|(name, _)| seeded(&format!(".claude/mustard/{name}"))),
    );
    out.push(seeded(CLAUDE_GITIGNORE_PATH));
    out.push(anchored(MUSTARD_JSON_RULE, MUSTARD_JSON));
    out.push(seeded(CLAUDE_LOCAL_MD));
    out.push(watched(CLAUDE_MD));
    out.push(watched(GITHUB_PR_TEMPLATE));
    let lifted: Vec<FootprintEntry> = out
        .iter()
        .filter_map(|entry| entry.rule.as_deref().and_then(at_any_depth))
        .collect();
    out.extend(lifted);
    out.extend(
        harness_claude_output()
            .iter()
            .map(|name| cover(&format!("**/.claude/{name}"))),
    );
    out.push(cover(CLAUDE_BACKUP_DIRS));
    out
}

/// The clone-local exclude RULES a private install writes — every footprint
/// entry Mustard itself puts in the project, and only those.
#[must_use]
pub fn footprint_rules() -> Vec<String> {
    footprint().into_iter().filter_map(|e| e.rule).collect()
}

/// The `git ls-files` PATHSPECS the residue question is asked with — every
/// footprint entry that names a real path, including the host's own `CLAUDE.md`,
/// which no rule hides but every private install must report.
#[must_use]
pub fn footprint_pathspecs() -> Vec<String> {
    footprint().into_iter().filter_map(|e| e.pathspec).collect()
}

/// Whether `path` — as `git ls-files` reported it — is one MUSTARD writes.
///
/// The predicate behind the residue ADVICE: `git rm --cached` clears a path the
/// install put there, and would untrack the client's own work for anything else.
#[must_use]
pub fn is_written_footprint(path: &str) -> bool {
    footprint()
        .iter()
        .any(|e| e.written && e.pathspec.as_deref() == Some(path))
}

/// The footprint entries that exist for the PRIVATE mode ALONE — the two
/// local-layer destinations no shared install ever writes.
///
/// They are what makes the mode self-evident: an exclude file carrying both was
/// written by a private install and by nothing else, so the mode needs no knob
/// in any versioned file. Deliberately NOT "every rule [`footprint_rules`]
/// returns": that list GROWS with each new injectable seed, so an all-rules test
/// would read an already-private project as shared the first time it ran a newer
/// Mustard — and the very next `scan --full` would then write straight into the
/// client's `CLAUDE.md`.
pub const PRIVATE_MARKS: [&str; 2] = [SETTINGS_LOCAL_JSON, CLAUDE_LOCAL_MD];

/// Whether an exclude file's body carries every [`PRIVATE_MARKS`] rule.
///
/// Compared on the TRIMMED line and skipping comments, exactly as
/// [`crate::platform::git_exclude::ensure_excluded`] compares before appending —
/// a CRLF file answers the same as an LF one, and a commented-out rule is not in
/// force.
#[must_use]
pub fn carries_private_marks(body: &str) -> bool {
    let mut seen = [false; PRIVATE_MARKS.len()];
    for line in body.lines().map(str::trim) {
        if line.starts_with('#') {
            continue;
        }
        for (slot, mark) in seen.iter_mut().zip(PRIVATE_MARKS) {
            *slot |= line == mark;
        }
    }
    seen.iter().all(|s| *s)
}

/// Which footprint the project at `root` was installed with — autodetected off
/// the clone-local exclude file, never configured.
///
/// The mode lives in no versioned file and in no environment variable: a knob in
/// `mustard.json` would itself be the trace the private mode exists to remove
/// (the setting would announce the tool it hides), and an env var is state the
/// operator has to remember. It is chosen ONCE through a `--private` flag and
/// thereafter read back off the exclude file that run wrote.
///
/// The single home for the question, so the two install faces cannot drift: the
/// CLI's `mustard init` and the runtime's `run upsert` both re-run over projects
/// that are already private, and either one silently deciding "shared" would
/// re-seed the versioned twin of a file it had already hidden.
///
/// Fail-open: no git, no repository, or an unreadable exclude file answers
/// [`InstallMode::Shared`] — today's behaviour, unchanged.
#[must_use]
pub fn detect_install_mode(root: &Path) -> InstallMode {
    let Some(path) = git_exclude::exclude_file(root) else {
        return InstallMode::Shared;
    };
    match fs::read_to_string(&path) {
        Ok(body) if carries_private_marks(&body) => InstallMode::Private,
        _ => InstallMode::Shared,
    }
}

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
}

/// `skip_serializing_if` predicate for the additive booleans above — a `false`
/// flag is simply absent, keeping the shared-install shape unchanged.
///
/// The `&bool` is serde's signature, not a choice: `skip_serializing_if` calls
/// the predicate with a reference to the field. Taking it by value does not
/// compile, so the lint is answered where it is raised rather than by changing
/// a signature that is fixed elsewhere.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !*b
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
/// 3. `.claude/mustard/orchestrator.md` — created when
///    absent, a user-customised copy survives;
/// 4. `.claude/.gitignore` — created when absent, and when present the pattern
///    lines it lacks are appended, so a project installed before a rule existed
///    still receives it (the one seed that merges by LINE — see
///    [`seed_gitignore`]);
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
/// Under [`InstallMode::Private`] a step 0 runs first — [`footprint_rules`] is
/// written into the clone-local exclude file, and whatever the host repository
/// already tracks under [`footprint_pathspecs`] is recorded as residue — so no
/// seed is ever momentarily visible to that repository's git. When that write
/// cannot happen inside a repository that exists, step 0 REFUSES and no step
/// after it runs. Step 2 then targets the local settings layer.
/// [`InstallMode::Shared`] does neither and is byte-for-byte what it was before
/// the mode existed.
///
/// # Errors
///
/// An IO or serialization failure from any seeding step. The migration is
/// fail-open and never errors. The private step errors — [`Error::NotHidden`] —
/// only when a REAL repository refused the exclude write, and it does so before
/// anything at all has been written.
pub fn upsert_project(
    root: &Path,
    version: Option<&str>,
    mode: InstallMode,
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
    //    This is the one step that can REFUSE. Everything else in this engine
    //    fails open, because everything else costs a feature when it degrades;
    //    a private install that cannot hide costs the operator's belief. If git
    //    resolved an exclude file in a real repository and the write still did
    //    not land, the seeds below would land VISIBLY in a client's git while
    //    the report said "private". Nothing is written on that path.
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

    // 1. Migration away from the planted-orchestrator layout (fail-open).
    report.migrated = migrate_orchestrator_footprint(root, &claude_dir).migrated;

    // 2..4. The `.claude/` seeds, merge-mode.
    report.record(settings_footprint(mode), seed_settings(&claude_dir, false, mode)?);
    for (name, outcome) in seed_injectable_files(&claude_dir, false)? {
        report.record(&format!(".claude/mustard/{name}"), outcome);
    }
    report.record(CLAUDE_GITIGNORE_PATH, seed_gitignore(&claude_dir, false)?);

    // 5. The single project-root mustard.json.
    let outcome = upsert_mustard_json(root, version)?;
    report.record(MUSTARD_JSON, outcome);

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

/// Seed the harness settings from the compiled-in [`SETTINGS_SEED`].
///
/// The destination follows `mode`: `.claude/settings.json` when shared,
/// `.claude/settings.local.json` when private (see [`settings_dest`]). The
/// merge semantics are the same either way, applied to whichever file is the
/// target — the other one is never read and never written.
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
pub fn seed_settings(claude_dir: &Path, overwrite: bool, mode: InstallMode) -> Result<SeedOutcome> {
    let dest = settings_dest(claude_dir, mode);
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

/// The settings file `mode` seeds, composed through [`ClaudePaths`] — the
/// single owner of `.claude/` path composition, so no call site joins the name
/// by hand.
///
/// `claude_dir` is already `<root>/.claude`, so the project root is recovered
/// from it rather than re-resolved. [`ClaudePaths::compose_unchecked`] is the
/// right constructor for that: the I1 guard exists to catch a caller passing
/// `.claude` AS the root, which is precisely the mistake being undone here —
/// there is no untrusted input left for it to reject.
fn settings_dest(claude_dir: &Path, mode: InstallMode) -> PathBuf {
    let paths = ClaudePaths::compose_unchecked(claude_dir.parent().unwrap_or(claude_dir));
    if mode.is_private() {
        paths.settings_local_json_path()
    } else {
        paths.settings_json_path()
    }
}

/// The project-root-relative name [`upsert_project`] reports for the file
/// [`settings_dest`] wrote — the report half of the same decision, and one of
/// the [`footprint_rules`] entries.
fn settings_footprint(mode: InstallMode) -> &'static str {
    if mode.is_private() {
        SETTINGS_LOCAL_JSON
    } else {
        SETTINGS_JSON
    }
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
const INJECTABLE_SEEDS: &[(&str, &str)] = &[("orchestrator.md", ORCHESTRATOR_MD)];

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

/// Seed `.claude/.gitignore` (the ephemeral harness state cover).
///
/// The ONE seed that merges by LINE instead of by file. Every other seed is a
/// document whose owner is either the user or Mustard, so "preserve what
/// exists" is the whole answer. This one is a rule list, and preserving it
/// whole meant that a project installed before a rule existed never received
/// it: the entry landed in the template, `mustard init` reported
/// [`SeedOutcome::Preserved`], and the rule reached fresh projects only —
/// exactly the state review found for `scratch/`, where the write gate had
/// already been taught to allow a path nothing ignored.
///
/// Merge therefore APPENDS the pattern lines the file lacks, under a header
/// that says who wrote them. The user's own lines, order and comments are
/// untouched, and a file that already carries every pattern is
/// [`SeedOutcome::Preserved`] — so the operation converges after one run.
/// Overwrite re-lays the seed whole, as before.
///
/// # Errors
///
/// An IO error writing the file.
pub fn seed_gitignore(claude_dir: &Path, overwrite: bool) -> Result<SeedOutcome> {
    let dest = claude_dir.join(".gitignore");
    if overwrite {
        return seed_static_file(&dest, CLAUDE_GITIGNORE, true);
    }
    let Ok(existing) = fs::read_to_string(&dest) else {
        // Absent → the seed IS the file. Unreadable-but-present degrades to
        // preserved inside `seed_static_file` — never stomp what we could not
        // inspect.
        return seed_static_file(&dest, CLAUDE_GITIGNORE, false);
    };
    let missing = missing_ignore_patterns(&existing, CLAUDE_GITIGNORE);
    if missing.is_empty() {
        return Ok(SeedOutcome::Preserved);
    }
    let mut merged = existing;
    if !merged.ends_with('\n') {
        merged.push('\n');
    }
    merged.push_str(GITIGNORE_BACKFILL_HEADER);
    for pattern in missing {
        merged.push_str(pattern);
        merged.push('\n');
    }
    fs::write_atomic(&dest, merged.as_bytes())?;
    Ok(SeedOutcome::Updated)
}

/// The header the line-merge writes above the patterns it appends, so the
/// addition is attributable rather than mysterious.
const GITIGNORE_BACKFILL_HEADER: &str =
    "\n# Added by Mustard: patterns the seed gained after this file was written.\n";

/// The seed's pattern lines that `existing` does not already carry, in seed
/// order and without repeats.
///
/// Compared on the TRIMMED pattern only: comments and blank lines are layout,
/// and re-appending a comment whose pattern is already present would grow the
/// file on every install. Deliberately literal — two patterns that match the
/// same paths by different spellings read as different rules here, which is the
/// conservative direction (a redundant line costs a line; a missing one costs
/// the rule).
fn missing_ignore_patterns<'a>(existing: &str, seed: &'a str) -> Vec<&'a str> {
    let is_pattern = |line: &&str| !line.is_empty() && !line.starts_with('#');
    let present: Vec<&str> = existing.lines().map(str::trim).filter(is_pattern).collect();
    let mut missing: Vec<&str> = Vec::new();
    for pattern in seed.lines().map(str::trim).filter(is_pattern) {
        if !present.contains(&pattern) && !missing.contains(&pattern) {
            missing.push(pattern);
        }
    }
    missing
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
/// every session's first prompt, once per session. Written in the same casing
/// the docs use; the config accessor lowercases `on` at read time.
///
/// The didactic response style used to ride `sessionStart` here; it is now the
/// `mustard-didactic` plugin output-style (survives `/clear` natively), so it
/// no longer needs a per-project injectable. [`migrate_response_style_inject`]
/// retires the stale entry from projects installed before the move.
#[must_use]
pub fn default_inject_entries() -> Vec<Injectable> {
    vec![Injectable {
        on: "userPromptSubmit".to_string(),
        file: ".claude/mustard/orchestrator.md".to_string(),
        once: true,
    }]
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
/// error. It also retires the legacy response-style injectable (the
/// `sessionStart` → `.claude/mustard/response-style.md` entry and its file),
/// now shipped as the `mustard-didactic` plugin output-style.
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

    // response-style injectable → output-style. The didactic style moved to the
    // `mustard-didactic` plugin output-style (part of the system prompt, so it
    // survives `/clear`), so the legacy per-project `sessionStart` inject entry
    // and its file are retired. Runs before the root-`CLAUDE.md` step below,
    // which returns early when the root file is absent. Idempotent + fail-open.
    if retire_response_style_inject(root, claude_dir) {
        outcome
            .migrated
            .push("mustard.json (response-style → output-style)".to_string());
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

/// Retire the legacy `sessionStart` → `.claude/mustard/response-style.md`
/// injectable: drop the `mustard.json#inject` entry and delete the orphaned
/// instruction file. The didactic response style now ships as the
/// `mustard-didactic` plugin output-style (part of the system prompt, so it
/// survives `/clear`), which no per-project injectable can match.
///
/// Returns `true` when something was retired. Idempotent — a project already
/// migrated (or a fresh one) returns `false`. Fail-open: an unreadable or
/// unwritable config degrades to `false`, never an error.
fn retire_response_style_inject(root: &Path, claude_dir: &Path) -> bool {
    const LEGACY_FILE: &str = ".claude/mustard/response-style.md";
    let mut changed = false;

    // Drop the stale inject entry via the config owner (single source of truth).
    if ProjectConfig::exists(root) {
        let mut config = ProjectConfig::load(root);
        let before = config.inject.len();
        config.inject.retain(|e| e.file != LEGACY_FILE);
        if config.inject.len() != before && config.write(root).is_ok() {
            changed = true;
        }
    }

    // Delete the orphaned instruction file under this project's `.claude/`.
    let orphan = claude_dir.join("mustard").join("response-style.md");
    if orphan.is_file() && fs::remove_file(&orphan).is_ok() {
        changed = true;
    }

    changed
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

        let report = upsert_project(root, Some("9.9.9"), InstallMode::Shared).unwrap();

        assert!(!report.installed_before, "no mustard.json existed before");
        assert_eq!(report.version.as_deref(), Some("9.9.9"));
        assert_eq!(
            report.created,
            vec![
                ".claude/settings.json",
                ".claude/mustard/orchestrator.md",
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
                ".claude/mustard/orchestrator.md",
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

        let report = upsert_project(root, Some("9.9.9"), InstallMode::Shared).unwrap();

        assert!(report.installed_before);
        // The customised injectable survives; only the missing seed is created.
        assert_eq!(
            std_fs::read_to_string(root.join(".claude/mustard/orchestrator.md")).unwrap(),
            "USER EDIT",
        );
        assert!(report.created.contains(&".claude/.gitignore".to_string()));
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

    // --- .gitignore line-merge ------------------------------------------------

    /// AC-13 — seeding over an EXISTING ignore file appends the patterns it
    /// lacks instead of preserving the file whole.
    ///
    /// The defect this closes shipped in this very repository: the write gate
    /// was taught to allow `.claude/scratch/`, the `scratch/` line landed in the
    /// template, and every already-initialised project — Mustard included — kept
    /// an ignore file without it, because `Preserved` skipped the whole file. A
    /// gate that allows a path nothing ignores, under a `/git` law that stages
    /// everything, commits throwaway evidence into the unit.
    ///
    /// Both halves, so "append" cannot become "overwrite": the user's own lines
    /// survive, nothing already present is duplicated, and the second run is
    /// `Preserved` — the merge converges.
    #[test]
    fn seeding_over_an_existing_ignore_adds_the_missing_lines() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        std_fs::create_dir_all(&claude).unwrap();
        // An ignore file from an older install: two of the seed's patterns, plus
        // a line only this project's user knows about.
        std_fs::write(
            claude.join(".gitignore"),
            "# Mustard harness scratch\n.cache/\nworktrees/\n\n# mine\nmy-notes/\n",
        )
        .unwrap();

        let outcome = seed_gitignore(&claude, false).unwrap();

        assert_eq!(outcome, SeedOutcome::Updated, "a file missing patterns is not preserved");
        let merged = std_fs::read_to_string(claude.join(".gitignore")).unwrap();
        assert!(merged.contains("my-notes/"), "the user's own rule survives: {merged}");
        assert!(merged.contains("# mine"), "…and so do their comments: {merged}");
        // EVERY pattern of the seed is now in force — the point of the change.
        for pattern in CLAUDE_GITIGNORE.lines().map(str::trim) {
            if pattern.is_empty() || pattern.starts_with('#') {
                continue;
            }
            assert!(
                merged.lines().any(|l| l.trim() == pattern),
                "seed pattern {pattern:?} missing after the merge: {merged}",
            );
        }
        assert_eq!(
            merged.lines().filter(|l| l.trim() == ".cache/").count(),
            1,
            "a pattern already present is not appended twice: {merged}",
        );

        // Convergence: the same call over the merged file changes nothing.
        let second = seed_gitignore(&claude, false).unwrap();
        assert_eq!(second, SeedOutcome::Preserved, "the merge is idempotent");
        assert_eq!(
            std_fs::read_to_string(claude.join(".gitignore")).unwrap(),
            merged,
            "…byte for byte",
        );
    }

    #[test]
    fn a_fresh_ignore_is_the_seed_verbatim() {
        // The absent case must stay a plain copy: no backfill header, no
        // reordering — a fresh project's file IS the template.
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        std_fs::create_dir_all(&claude).unwrap();

        assert_eq!(seed_gitignore(&claude, false).unwrap(), SeedOutcome::Created);

        assert_eq!(
            std_fs::read_to_string(claude.join(".gitignore")).unwrap(),
            CLAUDE_GITIGNORE,
        );
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

        let report = upsert_project(root, None, InstallMode::Shared).unwrap();

        assert!(!claude.join("CLAUDE.md").exists(), "planted orchestrator deleted");
        let root_md = std_fs::read_to_string(root.join("CLAUDE.md")).unwrap();
        assert!(!root_md.contains("@.claude/scan-map.md"));
        assert!(!root_md.contains("> Orchestrator:"));
        assert!(root_md.contains("# (root)"), "user content survives: {root_md}");
        assert!(root_md.contains("- keep"), "user guard survives: {root_md}");
        assert_eq!(report.migrated, vec![".claude/CLAUDE.md", "CLAUDE.md"]);
    }

    #[test]
    fn migration_retires_legacy_response_style_injectable() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let claude = root.join(".claude");
        std_fs::create_dir_all(claude.join("mustard")).unwrap();
        // A project installed before the output-style move: the response style
        // rides sessionStart in #inject and its file sits under .claude/mustard/.
        std_fs::write(
            root.join("mustard.json"),
            r#"{"version":"1.0.0","inject":[{"on":"userPromptSubmit","file":".claude/mustard/orchestrator.md","once":true},{"on":"sessionStart","file":".claude/mustard/response-style.md","once":true}]}"#,
        )
        .unwrap();
        std_fs::write(claude.join("mustard/response-style.md"), "# Response Style\n").unwrap();

        let report = upsert_project(root, None, InstallMode::Shared).unwrap();

        // The stale inject entry is gone; only the orchestrator entry remains.
        let config = ProjectConfig::load(root);
        assert_eq!(config.inject.len(), 1, "response-style entry retired: {:?}", config.inject);
        assert_eq!(config.inject[0].file, ".claude/mustard/orchestrator.md");
        // The orphaned instruction file is deleted.
        assert!(
            !claude.join("mustard/response-style.md").exists(),
            "orphan response-style.md removed"
        );
        // And the migration is reported.
        assert!(
            report.migrated.iter().any(|m| m.contains("response-style")),
            "migration reported: {:?}",
            report.migrated
        );
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

    // --- install mode ---------------------------------------------------------

    /// The path `seed_settings` writes and the name `upsert_project` reports for
    /// it are two halves of one decision. They are computed by two functions, so
    /// pin them together — a drift here would report a file nobody wrote.
    #[test]
    fn the_settings_destination_and_its_reported_name_agree() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        for mode in [InstallMode::Shared, InstallMode::Private] {
            let dest = settings_dest(&claude, mode);
            let name = settings_footprint(mode);
            assert!(
                dest.ends_with(name.trim_start_matches(".claude/")),
                "{mode:?}: reported {name} but wrote {dest:?}",
            );
            assert!(
                footprint_rules().iter().any(|p| p == name),
                "{mode:?}: {name} is not in the footprint the private mode hides",
            );
        }
        assert_ne!(
            settings_dest(&claude, InstallMode::Shared),
            settings_dest(&claude, InstallMode::Private),
            "the two modes must not share a destination",
        );
    }

    /// The footprint is derived, so adding an injectable seed grows it without
    /// anyone editing a second list.
    #[test]
    fn the_footprint_is_derived_from_the_seeds_it_hides() {
        let rules = footprint_rules();
        for (name, _) in INJECTABLE_SEEDS {
            let expected = format!(".claude/mustard/{name}");
            assert!(rules.contains(&expected), "{expected} missing: {rules:?}");
        }
        for expected in [
            SETTINGS_JSON,
            SETTINGS_LOCAL_JSON,
            CLAUDE_GITIGNORE_PATH,
            MUSTARD_JSON_RULE,
            CLAUDE_LOCAL_MD,
            CLAUDE_BACKUP_DIRS,
        ] {
            assert!(rules.iter().any(|p| p == expected), "{expected} missing: {rules:?}");
        }
        // Every `.claude/` seed also reaches a `.claude/` at DEPTH — without the
        // lift, the per-file entries hide the root one and no subproject's.
        // Derived from the seeds, so this is a property and not a second list.
        for seed in [SETTINGS_JSON, SETTINGS_LOCAL_JSON, CLAUDE_GITIGNORE_PATH] {
            let lifted = format!("**/{seed}");
            assert!(rules.iter().any(|p| *p == lifted), "{lifted} missing: {rules:?}");
        }
        for name in harness_claude_output() {
            let expected = format!("**/.claude/{name}");
            assert!(rules.iter().any(|p| *p == expected), "{expected} missing: {rules:?}");
        }
        // Every documented directory that is not client-authored MUST be covered.
        // This is the direction the ratchet lacked: it validated the rules that
        // were emitted and never asked whether one was MISSING, which is how
        // `plans/` — filled by Mustard's own `plansDirectory` seed — leaked 18
        // files carrying the operator's prompt titles into a client's repo.
        for dir in crate::io::claude_paths::ClaudePaths::documented_dirs() {
            if CLIENT_AUTHORED_CLAUDE_DIRS.contains(&dir) {
                continue;
            }
            let expected = format!("**/.claude/{dir}/");
            assert!(
                rules.iter().any(|p| *p == expected),
                "{expected} missing — a documented harness directory with no rule leaks: {rules:?}",
            );
        }
        // No duplicates and no backslashes — the list is written verbatim into a
        // git exclude file, which speaks forward slashes on every platform.
        let mut seen = rules.clone();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), rules.len(), "duplicate rule: {rules:?}");
        for rule in &rules {
            assert!(!rule.contains('\\'), "not a git rule: {rule}");
        }
    }

    /// **An exclude rule may name ONLY what Mustard writes.** Asked of every
    /// emitted rule, against real git, in both directions: it must match
    /// something a private install really produces, and it must match nothing
    /// the CLIENT authored.
    ///
    /// The previous version of this ratchet asked about ANCHORING — does the
    /// rule carry an interior slash — and that is the wrong question twice over.
    /// It passed `.github/pull_request_template.md`, a rule for a file a private
    /// install explicitly refuses to seed, purely because the path has a slash
    /// in it. And it passed a bare `**/.claude/` cover, which review then
    /// measured hiding a client-authored `.claude/commands/their-command.md`
    /// from that client's own `git add -A`. Anchoring says where a rule reaches;
    /// only ownership says whether it should.
    ///
    /// Both halves are needed, and the negative one carries most of the weight:
    /// an over-broad rule makes the CLEAN-status criterion (AC-8) MORE likely to
    /// pass, not less, so nothing else in this unit can see it.
    ///
    /// The matching is done by git — `check-ignore` in a probe repository
    /// carrying ONE rule at a time and no `.gitignore` of any kind, so every
    /// answer is attributable to that rule alone. A matcher of our own would be
    /// asserting our reading of gitignore syntax, which is precisely the reading
    /// that was wrong.
    #[test]
    fn no_rule_reaches_a_depth_that_is_not_ours() {
        let work = tempdir().unwrap();
        let ours = install_and_work(&work.path().join("host"));
        let probe = tempdir().unwrap();
        let probe_exclude = probe_repo(probe.path());

        for rule in footprint_rules() {
            let mine = check_ignore(probe.path(), &probe_exclude, &rule, &ours);
            assert!(
                !mine.is_empty(),
                "no file a private install produced is matched by {rule:?} — a rule that \
                 hides nothing of ours can only ever hide something of theirs. Either the \
                 rule is stale, or the fixture above must be taught to write the path it \
                 exists for.\nwhat the install produced: {ours:?}",
            );
            let stolen = check_ignore(probe.path(), &probe_exclude, &rule, CLIENT_AUTHORED);
            assert!(
                stolen.is_empty(),
                "{rule:?} hides {stolen:?}, which Mustard never writes — under a \
                 `git add -A` law those files would silently never be committed",
            );
        }

        // The host's own instruction file is asked about and never hidden.
        assert!(
            !footprint_rules().iter().any(|r| r == CLAUDE_MD),
            "a private install never writes a CLAUDE.md, so it must never hide one",
        );
        assert!(
            footprint_pathspecs().iter().any(|p| p == CLAUDE_MD),
            "…but it must still REPORT one the host repository already tracks",
        );
        assert!(
            !is_written_footprint(CLAUDE_MD),
            "the residue advice (`git rm --cached`) must never target the client's own file",
        );
        assert!(is_written_footprint(MUSTARD_JSON), "a real seed IS removable residue");
    }

    /// The mode is read back off the exclude file, so the marks the detector
    /// looks for must be rules some install really writes.
    #[test]
    fn the_private_marks_are_rules_an_install_writes() {
        let rules = footprint_rules();
        for mark in PRIVATE_MARKS {
            assert!(
                rules.iter().any(|r| r == mark),
                "{mark} is written by no install, so nothing could ever detect it: {rules:?}",
            );
        }
        let body = PRIVATE_MARKS.map(|m| format!("  {m}  \r\n")).concat();
        assert!(carries_private_marks(&format!("# theirs\nbuild/\n{body}")));
        assert!(!carries_private_marks(&format!("{}\n", PRIVATE_MARKS[0])), "one rule is not the mode");
        assert!(
            !carries_private_marks(&PRIVATE_MARKS.map(|m| format!("#{m}\n")).concat()),
            "a commented-out rule is not in force",
        );
        assert!(!carries_private_marks(""), "an empty exclude file is a shared install");
    }

    /// Fail-open: a tree git knows nothing about is a shared install, not an
    /// error and not a panic.
    #[test]
    fn detect_install_mode_degrades_to_shared_without_a_repository() {
        let dir = tempdir().unwrap();
        assert_eq!(detect_install_mode(dir.path()), InstallMode::Shared);
    }

    // --- ownership ratchet scaffolding ---------------------------------------

    /// Files a CLIENT plausibly authors in a repository Mustard was installed
    /// into, and that Mustard writes NONE of. The negative half of the ownership
    /// ratchet — every one of these must survive every rule.
    ///
    /// Three are specific answers to defects this unit shipped and review then
    /// measured in a real repository: `.claude/commands/their-command.md` and
    /// `.claude/agents/their-agent.md` were swallowed by a bare `**/.claude/`
    /// cover, and `.github/pull_request_template.md` carried a rule of its own
    /// even though a private install explicitly refuses to seed it.
    const CLIENT_AUTHORED: &[&str] = &[
        "CLAUDE.md",
        "README.md",
        "mustard.json.example",
        "packages/api/CLAUDE.md",
        "packages/api/mustard.json",
        "packages/api/src/lib.rs",
        ".claude/commands/their-command.md",
        ".claude/agents/their-agent.md",
        ".claude/refs/their-notes.md",
        ".claude/skills/their-own-skill/SKILL.md",
        "packages/api/.claude/skills/their-other-skill/SKILL.md",
        ".github/pull_request_template.md",
        ".github/workflows/ci.yml",
    ];

    /// Everything a private install and the harness it installs really put in a
    /// project, measured by walking the tree afterwards rather than by listing.
    ///
    /// The install runs SHARED first and private second, which is not decoration:
    /// that is the project the `settings.json` / `settings.local.json` pair
    /// exists for — an operator switching an existing install to private, where
    /// leaving the shared twin visible would announce the tool the mode hides.
    ///
    /// What follows the two installs is what the harness writes while it WORKS,
    /// which no function in this crate can produce (the scan and the spec cycle
    /// live in `apps/rt`). It is therefore written here — and that is exactly the
    /// obligation the ratchet imposes: a rule may not be added without declaring,
    /// right here, the Mustard path it exists for.
    fn install_and_work(root: &Path) -> Vec<String> {
        std_fs::create_dir_all(root).unwrap();
        for args in [
            vec!["init"],
            vec!["config", "user.email", "t@example.com"],
            vec!["config", "user.name", "t"],
        ] {
            let ok = std::process::Command::new("git")
                .args(&args)
                .current_dir(root)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            assert!(ok, "git {args:?} failed");
        }

        upsert_project(root, Some("9.9.9"), InstallMode::Shared).unwrap();
        upsert_project(root, Some("9.9.9"), InstallMode::Private).unwrap();

        // The scan census, the grain model, the capability docs and the spec
        // directories — the root `.claude/`.
        for (name, body) in [
            ("scan-map.md", "Type: cargo\n"),
            ("grain.model.json", "{}\n"),
            ("grain.dictionary.json", "{}\n"),
            ("grain.equivalences.json", "{}\n"),
            ("scan-declined.json", "{}\n"),
            (".artifacts.json", "{}\n"),
            ("capabilities/cap.demo.md", "# cap\n"),
            ("spec/demo/spec.md", "# demo\n"),
            ("spec/demo/qa/report.md", "# QA\n"),
        ] {
            let dest = root.join(".claude").join(name);
            std_fs::create_dir_all(dest.parent().unwrap()).unwrap();
            std_fs::write(dest, body).unwrap();
        }
        // The RUNTIME output — what the harness writes while it runs, as opposed
        // to what `upsert_project` seeds at install time. The eight rules that
        // cover these directories are derived from the documented catalog, so
        // they exist whether or not anything here produces them; a fixture that
        // only seeded left every one of them looking stale the moment the
        // operator's global gitignore stopped answering for them.
        //
        // The names are the real ones this repository carries, so the rule is
        // proven against the shape it will actually meet.
        for (name, body) in [
            (".agent-state/main-context.counter.json", "{}\n"),
            (".cache/spec-material.json", "{}\n"),
            (".harness/.last-stop", "0\n"),
            (".metrics/qa.jsonl", "{}\n"),
            (".pipeline-states/demo.json", "{}\n"),
            ("agent-memory/mustard-review.md", "# memory\n"),
            ("graph/entities.json", "{}\n"),
            ("plans/2026-08-19-demo.md", "# plan\n"),
        ] {
            let dest = root.join(".claude").join(name);
            std_fs::create_dir_all(dest.parent().unwrap()).unwrap();
            std_fs::write(dest, body).unwrap();
        }
        // A subproject's own `.claude/`, which a full scan creates: its census,
        // its pattern molds, and the Guards on the untracked local layer beside
        // the client's own instruction file.
        let sub = root.join("packages/api");
        for (name, body) in [
            (".claude/scan-map.md", "Type: cargo\n"),
            (".claude/skills/core-demo-pattern/SKILL.md", "# mold\n"),
            ("CLAUDE.local.md", "## Guards\n"),
        ] {
            let dest = sub.join(name);
            std_fs::create_dir_all(dest.parent().unwrap()).unwrap();
            std_fs::write(dest, body).unwrap();
        }
        // The timestamped copy `mustard init`'s backup-and-overwrite branch
        // leaves beside `.claude/`. Placed rather than produced: that branch is
        // INTERACTIVE and never runs with stdin off a terminal.
        let backup = root.join(".claude.backup.20260817-101500");
        std_fs::create_dir_all(&backup).unwrap();
        std_fs::write(backup.join("settings.json"), "{}\n").unwrap();

        let mut found = Vec::new();
        collect_files(root, root, &mut found);
        found.sort();
        assert!(found.len() > 10, "the fixture measured almost nothing: {found:?}");
        found
    }

    /// Every file under `dir`, as a project-root-relative path with forward
    /// slashes. `.git/` is skipped — it is git's own, not Mustard's, and
    /// `check-ignore` refuses to answer about it.
    fn collect_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std_fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.file_name().and_then(|n| n.to_str()) == Some(".git") {
                continue;
            }
            if path.is_dir() {
                collect_files(root, &path, out);
            } else if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }

    /// An EMPTY repository whose only ignore source is its clone-local exclude
    /// file — returns that file's path. Nothing is committed and no `.gitignore`
    /// exists anywhere, so a `check-ignore` answer here is attributable to the
    /// one rule the caller writes into it.
    fn probe_repo(root: &Path) -> PathBuf {
        let ok = std::process::Command::new("git")
            .arg("init")
            .current_dir(root)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(ok, "git init failed");
        let path = git_exclude::exclude_file(root).expect("git resolves the exclude file");
        std_fs::create_dir_all(path.parent().unwrap()).unwrap();
        path
    }

    /// Which of `paths` git says the single `rule` matches.
    ///
    /// **The operator's own git config is shut out, and that is the whole
    /// point.** `check-ignore` answers from EVERY ignore source at once, so a
    /// developer whose `~/.config/git/ignore` carries one line about `.claude/`
    /// gets that line folded into every answer — and the positive half of the
    /// ratchet ("this rule hides something of OURS") then passes for every rule
    /// ever written, including a rule that matches nothing. It measured the
    /// machine instead of the code: green on the author's laptop, red on all
    /// three CI runners, for eight rules at once.
    ///
    /// So the global and system layers are pointed at `/dev/null` and
    /// `core.excludesFile` is emptied on the command line. What remains is the
    /// clone-local exclude file carrying the one rule the caller wrote — which
    /// is what the doc above always claimed this measured.
    fn check_ignore<S: AsRef<str>>(
        probe: &Path,
        exclude: &Path,
        rule: &str,
        paths: &[S],
    ) -> Vec<String> {
        std_fs::write(exclude, format!("{rule}\n")).unwrap();
        let out = std::process::Command::new("git")
            .args(["-c", "core.excludesFile=", "check-ignore", "--no-index", "--"])
            .args(paths.iter().map(AsRef::as_ref))
            .current_dir(probe)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("git check-ignore ran");
        // Exit 1 means "nothing matched" and is not a failure; 128 is.
        assert_ne!(out.status.code(), Some(128), "git check-ignore refused {rule:?}");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.trim().replace('\\', "/"))
            .filter(|l| !l.is_empty())
            .collect()
    }
}
