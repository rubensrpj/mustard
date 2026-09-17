//! The footprint: what a Mustard install can put in a project, declared once,
//! and the install mode that decides whether the host repository's git sees it.
//!
//! Three projections of one declaration live here — the exclude RULES a
//! private install writes, the `git ls-files` PATHSPECS the residue question is
//! asked with, and the predicate that says which tracked path is really ours —
//! plus the detector that reads the mode back off the clone-local exclude file.

use std::path::Path;

use crate::io::claude_paths::ClaudePaths;
use crate::io::fs;
use crate::platform::git_exclude;

use super::files::INJECTABLE_SEEDS;
use super::{
    CLAUDE_GITIGNORE_PATH, CLAUDE_LOCAL_MD, CLAUDE_MD, GITHUB_PR_TEMPLATE, MUSTARD_JSON,
    MUSTARD_JSON_RULE, SETTINGS_JSON, SETTINGS_LOCAL_JSON,
};

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
        ClaudePaths::documented_dirs()
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use tempfile::tempdir;

    use super::*;
    use crate::platform::git;
    use crate::platform::project_seed::upsert_project;
    use std::fs as std_fs;

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
            assert!(rules.contains(&lifted), "{lifted} missing: {rules:?}");
        }
        for name in harness_claude_output() {
            let expected = format!("**/.claude/{name}");
            assert!(rules.contains(&expected), "{expected} missing: {rules:?}");
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
                rules.contains(&expected),
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
    /// an over-broad rule makes the CLEAN-status criterion MORE likely to
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
            assert!(git::run(root, &args).ok, "git {args:?} failed");
        }

        upsert_project(root, Some("9.9.9"), InstallMode::Shared).unwrap();
        upsert_project(root, Some("9.9.9"), InstallMode::Private).unwrap();

        // The scan census, the grain model, the capability docs and the spec
        // directories — the root `.claude/`.
        for (name, body) in [
            ("scan-map.md", "Type: cargo\n"),
            ("grain.model.json", "{}\n"),
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
        // to what `upsert_project` seeds at install time. The rules that cover
        // these directories are derived from the documented catalog, so they
        // exist whether or not anything here produces them; a fixture that only
        // seeded left every one of them looking stale the moment the operator's
        // global gitignore stopped answering for them.
        //
        // The names are the real ones this repository carries, so the rule is
        // proven against the shape it will actually meet.
        for (name, body) in [
            (".agent-state/main-context.counter.json", "{}\n"),
            (".cache/spec-material.json", "{}\n"),
            (".dispatch/wave-1-impl.prompt.md", "# prompt\n"),
            (".harness/.last-stop", "0\n"),
            (".metrics/qa.jsonl", "{}\n"),
            (".pipeline-states/demo.json", "{}\n"),
            (".session/sess-demo/.events/2026-08-19.ndjson", "{}\n"),
            ("agent-memory/mustard-review.md", "# memory\n"),
            ("graph/entities.json", "{}\n"),
            // `mustard-rt run pending --add` — the agreed-work ledger that lives
            // outside every unit, so a worktree and the main checkout share it.
            ("pending/ledger.json", "{\"items\":[]}\n"),
            ("plans/2026-08-19-demo.md", "# plan\n"),
            ("scratch/probe.json", "{}\n"),
            ("worktrees/fix/demo/CLAUDE.md", "# unit\n"),
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
        assert!(git::run(root, &["init"]).ok, "git init failed");
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
