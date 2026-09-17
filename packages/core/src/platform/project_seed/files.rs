//! The static seeds: the harness's own instruction files under
//! `.claude/mustard/`, the `.claude/.gitignore` rule list, and the project-root
//! `mustard.json` — with the migrations that bring an older `inject` list onto
//! the current router layout.

use std::path::{Path, PathBuf};

use crate::domain::command_detect::detect_commands;
use crate::domain::config::{Injectable, ProjectConfig, Runtime};
use crate::io::fs;
use crate::platform::error::Result;
use crate::platform::seeds::{CLAUDE_GITIGNORE, DISPATCH_MD, MATERIAL_MD, ORCHESTRATOR_MD};

use super::SeedOutcome;

/// The injectable instruction files seeded under `.claude/mustard/`:
/// `(basename, compiled-in body)`.
///
/// There is no third element any more. The pair used to carry a catalog of
/// fingerprints of every SUPERSEDED body, so the merge could guess whether the
/// copy on disk was a stale seed (refresh) or the operator's own text
/// (preserve). The guess errs on the wrong side: a copy the catalog does not
/// recognise is PRESERVED, so a corrected rule can fail to reach an installed
/// project in silence. These files are the harness's rules, so
/// [`seed_injectable_files`] simply writes them.
pub(crate) const INJECTABLE_SEEDS: &[(&str, &str)] = &[
    ("orchestrator.md", ORCHESTRATOR_MD),
    ("dispatch.md", DISPATCH_MD),
    ("material.md", MATERIAL_MD),
];

/// Seed the injectable instruction files into `.claude/mustard/`.
///
/// **Always rewritten.** Unlike every other seeder here there is no
/// overwrite/merge decision to take: every file [`INJECTABLE_SEEDS`] carries is
/// the harness's own rules, not project configuration, so each install and each
/// update lays the compiled-in body down again. A copy that diverged — an
/// operator's edit, or a seed from an older release — is replaced, and the
/// replacement is reported as [`SeedOutcome::Updated`], never as
/// [`SeedOutcome::Preserved`]. The loss is real and deliberate: it is the price
/// of a corrected rule reaching every installed project, and it shows up in the
/// caller's report instead of passing unsaid.
///
/// **A copy that cannot be READ is rewritten too**, not preserved. It used to be
/// preserved, on the general rule that we never stomp what we could not inspect
/// — but that rule belongs to the seeds whose owner is the project. Here it
/// carved a hole in the always-rewrite contract this doc states: a corrupted or
/// unreadable `dispatch.md` kept the correction out of the project, silently,
/// which is the exact failure the fingerprint catalog was removed to end.
///
/// Returns `(basename, outcome)` per file, in declaration order
/// (deterministic).
///
/// # Errors
///
/// An IO error creating the directory or writing a file.
pub fn seed_injectable_files(claude_dir: &Path) -> Result<Vec<(String, SeedOutcome)>> {
    let dest_dir = claude_dir.join("mustard");
    fs::create_dir_all(&dest_dir)?;
    let mut out = Vec::with_capacity(INJECTABLE_SEEDS.len());
    for (name, body) in INJECTABLE_SEEDS {
        let dest = dest_dir.join(name);
        out.push(((*name).to_string(), seed_static_file(&dest, body, true)?));
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
/// even under overwrite (no gratuitous rewrite).
///
/// **An unreadable-but-present file follows `overwrite`, not a blanket
/// "preserve".** Under MERGE (`overwrite == false`) it is preserved: the caller
/// wanted to keep what is there, and a file we could not inspect is exactly
/// what must not be stomped. Under OVERWRITE it is REWRITTEN, because that flag
/// is the always-rewrite contract [`seed_injectable_files`] states three lines
/// above its own call — these are the harness's rules, not project
/// configuration, and preserving one is precisely how a corrected rule stops at
/// the project boundary in silence. That silence is the defect the fingerprint
/// catalog was removed to end; a file whose bytes cannot even be read is the
/// LEAST trustworthy copy to leave a window reading.
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
        Err(_) if overwrite => {
            fs::write_atomic(dest, body.as_bytes())?;
            Ok(SeedOutcome::Updated)
        }
        Err(_) => Ok(SeedOutcome::Preserved),
    }
}


/// The default `mustard.json#inject` declarations: every injectable
/// [`INJECTABLE_SEEDS`] carries, on `userPromptSubmit`, one sibling hook each.
/// Written in the same casing the docs use; the config accessor lowercases `on`
/// at read time.
///
/// **Why one event with a hook per injectable.** A hook RESPONSE is capped at
/// 10,000 characters; past that the harness saves the overflow to a file and
/// hands the window a preview plus a path, so the text stops being in force
/// even though nothing was truncated mid-sentence. The cap is per response, not
/// per event: sibling hooks on one event are separate invocations and Claude
/// Code keeps the `additionalContext` of every one of them (measured 2026-08-25
/// — two siblings emitting 6,000 characters each both arrived intact). So each
/// injectable gets its own hook and its own ceiling, and there is no composite
/// budget between them.
///
/// `userPromptSubmit` is chosen because it is SELF-HEALING: the `once` markers
/// live under `.claude/.session/<session_id>/`, so any path that loses the
/// window (a new session, a `fork`, a resume) also loses the marker and the
/// next prompt re-delivers. `sessionStart` cannot do that — it only fires on
/// openings, and `fork` matched no matcher at all until this unit.
///
/// Mustard's own dispatcher fold joins the Injects of ONE invocation into a
/// single response, and that response has ONE 10,000-character ceiling — so
/// joining never buys an injectable room of its own. A hook per injectable is
/// what does, without touching the dispatcher. Rationale in full:
/// `plugin/refs/mustard/router-rationale.md`.
///
/// The didactic response style used to ride `sessionStart` here; it is now the
/// `mustard-didactic` plugin output-style (survives `/clear` natively), so it
/// no longer needs a per-project injectable. [`retire_response_style_inject`]
/// retires the stale entry from projects installed before the move.
#[must_use]
pub fn default_inject_entries() -> Vec<Injectable> {
    injectable_declared_paths()
        .into_iter()
        .map(|file| Injectable {
            on: "userPromptSubmit".to_string(),
            file,
            once: true,
        })
        .collect()
}

/// The basename of every injectable instruction file the seed carries, in
/// declaration order.
///
/// Public because the whole SET is a question more than one consumer has to
/// ask, and every consumer that answers it by retyping the names is a place a
/// new injectable is forgotten: the doctor's delivery check asks it, and so
/// does the prose ratchet that forbids any document from naming a proper
/// subset of them. Derived from [`INJECTABLE_SEEDS`], which stays the one
/// declaration.
#[must_use]
pub fn injectable_names() -> Vec<&'static str> {
    INJECTABLE_SEEDS.iter().map(|(name, _)| *name).collect()
}

/// Every injectable the seed carries, as `(basename, compiled-in body)`.
///
/// The BODY half is public for the same reason the names are: the ratchet that
/// compares this repository's own delivered copies under `.claude/mustard/`
/// against what the binary ships has to ask the seed which files those are AND
/// what each should contain. Asking for the names and then reaching for a
/// constant per name is the enumeration that goes stale — it stayed green
/// through a fourth injectable in a measured sabotage.
#[must_use]
pub fn injectable_seeds() -> Vec<(&'static str, &'static str)> {
    INJECTABLE_SEEDS.to_vec()
}

/// The path each injectable is DECLARED and delivered under, in the same
/// order — the one place the `.claude/mustard/` prefix is spelled for a
/// declaration.
#[must_use]
pub fn injectable_declared_paths() -> Vec<String> {
    injectable_names()
        .into_iter()
        .map(|name| format!(".claude/mustard/{name}"))
        .collect()
}

/// Declared path of the router's first part (§ Intent Routing) — spelled on
/// its own because [`backfill_dispatch_inject`] keys the pre-split migration on
/// THAT entry specifically, not on the set.
const ORCHESTRATOR_INJECT_FILE: &str = ".claude/mustard/orchestrator.md";

/// Create or minimally update the project-root `mustard.json` through
/// [`ProjectConfig`] (the single owner).
///
/// Absent → created with an empty `git.flow` (the project decides later),
/// agnostically detected commands, the default `inject` declarations,
/// `runtime`, and `version` (when supplied). Present → `version` is re-stamped
/// (only when `Some` and different), an empty `inject` is backfilled with the
/// defaults, an absent `runtime` is filled — everything else is preserved
/// verbatim, and the file is not rewritten when nothing changed.
pub(super) fn upsert_mustard_json(root: &Path, version: Option<&str>) -> Result<SeedOutcome> {
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
    if let Some(version) = version
        && config.version.as_deref() != Some(version) {
            config.version = Some(version.to_string());
            changed = true;
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
// The `inject` migrations
// ---------------------------------------------------------------------------

/// Bring an already-installed project's `mustard.json#inject` onto the current
/// router layout, and say what changed.
///
/// Touches only `mustard.json`, which is Mustard's own file: the response-style
/// entry that became a plugin output-style is retired (with its orphaned file),
/// and a router split across events, or missing a part split off later, is
/// completed. Idempotent and fail-open — an unreadable or unwritable config
/// degrades to "nothing migrated", never an error.
pub fn migrate_inject_declarations(root: &Path, claude_dir: &Path) -> Vec<String> {
    let mut migrated = Vec::new();
    if retire_response_style_inject(root, claude_dir) {
        migrated.push("mustard.json (response-style → output-style)".to_string());
    }
    if backfill_dispatch_inject(root) {
        // Names the RESULT, not the state it left. The old wording said
        // "dispatch injectable on sessionStart" — the event the migration
        // moves entries OFF — and a reader relayed it to the operator as
        // where the file had landed, i.e. the exact broken state the
        // migration exists to undo (measured in the field, 2026-08-26).
        migrated.push("mustard.json (router injectables consolidated)".to_string());
    }
    migrated
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

/// `true` when two declared injectable paths name the SAME file.
///
/// A declaration is written by hand as often as it is seeded, and the same file
/// has several honest spellings: a `./` prefix, backslashes on Windows, a
/// trailing slash, mixed case on a case-insensitive filesystem. Comparing the
/// raw strings makes each of those a different file, and the only consequence a
/// user ever sees is a section that silently reaches nobody.
///
/// Normalisation is deliberately conservative — separators, one leading `./`,
/// trailing separators, and ASCII case. It never resolves symlinks or touches
/// the filesystem: this runs during install, on paths that may not exist yet.
pub fn same_declared_path(a: &str, b: &str) -> bool {
    fn norm(s: &str) -> String {
        let s = s.trim().replace('\\', "/");
        let s = s.strip_prefix("./").unwrap_or(&s).to_string();
        s.trim_end_matches('/').to_ascii_lowercase()
    }
    norm(a) == norm(b)
}

/// Bring an already-installed project's `inject` list onto the current router
/// layout: every part on `userPromptSubmit`, one sibling hook each.
///
/// Two historical shapes reach this, and both are repaired:
///
/// - **Partly delivered** — the project declares the orchestrator and not every
///   part that has been split off it since (dispatch, then material). Those
///   parts are seeded to disk unconditionally, so without this they exist in the
///   project and are declared by nobody: the rules reach no window and nothing
///   says so. Derived from [`default_inject_entries`] rather than named one at a
///   time, so the NEXT split is repaired the day it is declared.
/// - **Split across events** — the project declares them, with one on
///   `sessionStart`. That event misses every path that opens no session:
///   `fork` matched no matcher at all, and `startup` never cleared the
///   per-session markers. The entry is MOVED, never duplicated.
///
/// Deliberately conditional on the orchestrator entry being present, so an
/// operator who removed the router from their `inject` list is not handed it
/// back — this migration repairs a router the project already declares, it does
/// not re-impose one that was dropped. A project whose `inject` is empty is not
/// touched here either: [`upsert_mustard_json`] already backfills that case
/// with the full defaults.
///
/// Returns `true` when the list changed. Idempotent — a project already on the
/// current layout (or a fresh one) returns `false`. Fail-open: an unwritable
/// config degrades to `false`, never an error.
fn backfill_dispatch_inject(root: &Path) -> bool {
    if !ProjectConfig::exists(root) {
        return false;
    }
    let mut config = ProjectConfig::load(root);
    // **Equivalent spellings of one path are the same declaration.** The exact
    // string match this used to do read `./.claude/mustard/orchestrator.md` and
    // `.claude\mustard\orchestrator.md` as OTHER files, so a project that wrote
    // either got the second half seeded to disk and never declared — the state
    // this function's own doc calls strictly worse than the over-budget file it
    // replaced: the section reaches nobody, and nothing says so.
    let declares = |file: &str| config.inject.iter().any(|e| same_declared_path(&e.file, file));
    if !declares(ORCHESTRATOR_INJECT_FILE) {
        return false;
    }

    let mut changed = false;
    // Every part that was never declared: add it, on the current event. The
    // orchestrator is excluded by the guard above (it is already declared, or we
    // returned), so this only ever backfills a part split off it later.
    let missing: Vec<Injectable> = default_inject_entries()
        .into_iter()
        .filter(|e| !declares(&e.file))
        .collect();
    if !missing.is_empty() {
        config.inject.extend(missing);
        changed = true;
    }
    // Either half left on another event: move it — but ONLY once the installed
    // plugin registers a hook per injectable.
    //
    // Moving every declared part onto one event is safe because each rides its
    // own sibling hook and is measured alone. A plugin that predates those
    // siblings has ONE hook for the event, which folds the whole set into a
    // single response: measured at 11,646 characters on the router as it stood
    // then, over the 10,000 cap. The split the migration undoes was at least
    // under it, so migrating against a stale plugin would leave the project
    // worse than it found it.
    //
    // The project's own `mustard.json` is not the authority on this — the
    // installed plugin is. When it cannot be read, the move is SKIPPED: a half
    // still delivered on its old event beats one delivered nowhere.
    if plugin_registers_sibling_hooks(root) {
        for seeded in default_inject_entries() {
            for entry in &mut config.inject {
                if same_declared_path(&entry.file, &seeded.file) && entry.on != seeded.on {
                    entry.on.clone_from(&seeded.on);
                    changed = true;
                }
            }
        }
    }

    changed && config.write(root).is_ok()
}

/// Does the INSTALLED plugin register one hook per injectable?
///
/// Read from the plugin's own `hooks/hooks.json`, because that file is what
/// actually runs — a project's `mustard.json` says what should be delivered,
/// never how. A registration carrying `--inject` is the signal: each injectable
/// gets its own invocation, so each is measured alone against the ceiling.
///
/// `false` when the manifest cannot be found or read. That is deliberate and it
/// is the conservative direction: the caller only MOVES entries when this is
/// true, so an unreadable manifest leaves the layout exactly as it was.
fn plugin_registers_sibling_hooks(root: &Path) -> bool {
    plugin_hooks_manifest(root).is_some_and(|m| manifest_registers_sibling_hooks(&m))
}

/// Does THIS manifest register a hook per injectable?
///
/// Split from the locator so the decision is testable without a plugin install:
/// the locator reads the machine's own registry, which no temporary directory
/// can stand in for.
///
/// TWO claims is the bar, not one. The migration moves BOTH router halves onto
/// one event, and a manifest claiming a single injectable still folds the other
/// into a shared response — 11,646 characters against a 10,000 cap. An
/// unreadable or absent manifest answers `false`, which is the conservative
/// direction: the caller only moves entries on a yes.
fn manifest_registers_sibling_hooks(manifest: &Path) -> bool {
    let Ok(text) = fs::read_to_string(manifest) else {
        return false;
    };
    text.matches("--inject").count() >= 2
}

/// Where the installed plugin's hook manifest lives.
///
/// The REGISTRY first — Claude Code records each plugin's `installPath`, and
/// that is the directory it runs hooks out of. Two guesses were tried before
/// this and both fail in a real install: `CLAUDE_PLUGIN_ROOT` reaches hooks but
/// not the shell that runs `upsert`, and `<project>/plugin/hooks/` exists only
/// inside the Mustard source repository. Relying on either answered a confident
/// "no siblings registered" in every user project, which silently disabled the
/// migration this manifest guards (found in review).
///
/// The two guesses remain as FALLBACKS, in that order: the environment variable
/// for a caller that does run under a hook, and the in-repo copy for a source
/// checkout — which is also what the tests seed.
fn plugin_hooks_manifest(root: &Path) -> Option<PathBuf> {
    if let Some(p) = crate::platform::harness::installed_plugin_hooks_manifest() {
        return Some(p);
    }
    if let Some(dir) = std::env::var_os("CLAUDE_PLUGIN_ROOT") {
        let p = Path::new(&dir).join("hooks").join("hooks.json");
        if p.is_file() {
            return Some(p);
        }
    }
    let in_repo = root.join("plugin").join("hooks").join("hooks.json");
    in_repo.is_file().then_some(in_repo)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::git;
    use crate::platform::project_seed::{upsert_project, InstallMode};
    use std::fs as std_fs;
    use tempfile::tempdir;

    /// The migration recognises an equivalent spelling of the router's
    /// declared path.
    ///
    /// Matching the raw string made `./.claude/mustard/orchestrator.md` and the
    /// backslash form read as OTHER files, so a project that declared either got
    /// the second half seeded to disk and never declared. The section then
    /// reaches nobody and nothing says so — strictly worse than the over-budget
    /// single file the split replaced.
    #[test]
    fn backfill_dispatch_inject_matches_equivalent_path_spellings() {
        for spelling in [
            ".claude/mustard/orchestrator.md",
            "./.claude/mustard/orchestrator.md",
            ".claude\\mustard\\orchestrator.md",
            ".claude/mustard/Orchestrator.md",
        ] {
            assert!(
                super::same_declared_path(spelling, super::ORCHESTRATOR_INJECT_FILE),
                "`{spelling}` was read as a different file from the declared router, \
                 so the migration would seed the second half and never declare it",
            );
        }
        assert!(
            !super::same_declared_path(
                ".claude/mustard/dispatch.md",
                super::ORCHESTRATOR_INJECT_FILE
            ),
            "two genuinely different files must not be folded together",
        );
    }

    /// Um caminho declarado com espaços nas pontas é o mesmo arquivo, e um
    /// nome que só começa igual a outro é outro arquivo.
    #[test]
    fn a_declared_path_ignores_padding_and_never_matches_a_prefix() {
        assert!(super::same_declared_path("  .claude/mustard/orchestrator.md  ", super::ORCHESTRATOR_INJECT_FILE));
        assert!(!super::same_declared_path(".claude/mustard/dispatch.md", ".claude/mustard/dispatch.md.bak"));
    }


    // --- .gitignore line-merge ------------------------------------------------

    /// Seeding over an EXISTING ignore file appends the patterns it
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

    /// Uma instalação anterior à lista de pendências recebe `pending/` pelo merge
    /// por linha, sem perder nada do que já estava lá. Sem a linha, o primeiro
    /// `git add -A` levaria o ledger para o branch em uso — e ele existe
    /// justamente para não mudar com o branch.
    #[test]
    fn the_line_merge_backfills_the_pending_ledger() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        std_fs::create_dir_all(&claude).unwrap();
        std_fs::write(claude.join(".gitignore"), "# mine\nmy-notes/\nworktrees/\n").unwrap();

        assert_eq!(seed_gitignore(&claude, false).unwrap(), SeedOutcome::Updated);
        let merged = std_fs::read_to_string(claude.join(".gitignore")).unwrap();
        assert!(merged.lines().any(|l| l.trim() == "pending/"), "pending/ backfilled: {merged}");
        assert!(merged.starts_with("# mine\nmy-notes/\nworktrees/\n"), "merge-only: {merged}");
    }

    /// The two GATE MARKERS are held back, and the unit's own RECORD is not.
    ///
    /// Both halves are asserted, and the second is the one that makes the test
    /// worth having: a blanket `spec/` cover would pass the first half alone,
    /// and that blanket cover is exactly the state this rule replaces — it hid
    /// 45 of 86 units from git on this repository while the seeded ignore next
    /// to it said a spec's content stays versioned.
    ///
    /// Driven through REAL git rather than by reading the file back. What is
    /// being bought is git's own decision, and `check-ignore` is the question
    /// the operator would ask; a substring search over the file would pass on a
    /// pattern git does not actually apply at that depth.
    #[test]
    fn the_seeded_gitignore_holds_back_the_gate_markers() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        assert!(
            git::run(root, &["init", "-q"]).ok,
            "the test measures git's decision, so it needs a real repository",
        );

        let claude = root.join(".claude");
        std_fs::create_dir_all(&claude).unwrap();
        seed_gitignore(&claude, false).unwrap();

        let ignored = |rel: &str| git::run(root, &["check-ignore", "-q", rel]).ok;

        // Held back: presence of either file IS the gate's answer, so a commit
        // would let `git clone` hand a fresh checkout an approval nobody gave.
        let marker = ".claude/spec/a-unit/.approved-by-user";
        assert!(ignored(marker), "a gate marker must never be versionable: {marker}");

        // Kept: the unit's record is what a reviewer reads, and it belongs to
        // the repository.
        for kept in ["spec.md", "wave-plan.md", "pr-body.md", "meta.json", "review/findings.md"] {
            let path = format!(".claude/spec/a-unit/{kept}");
            assert!(!ignored(&path), "the unit's own record must stay versioned: {path}");
        }

        // Unchanged by this addition: the machine sidecars stay ignored.
        for sidecar in [".events/2026-01-01.ndjson", ".dispatch/wave-1.prompt.md"] {
            let path = format!(".claude/spec/a-unit/{sidecar}");
            assert!(ignored(&path), "regenerable machine state stays ignored: {path}");
        }
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

        // The stale inject entry is gone; the router's own parts remain.
        let config = ProjectConfig::load(root);
        assert!(
            !config.inject.iter().any(|e| e.file.ends_with("response-style.md")),
            "response-style entry retired: {:?}",
            config.inject,
        );
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

    /// A project installed BEFORE the two-event split gains the second half.
    ///
    /// Without this the split is a regression for every existing install:
    /// `dispatch.md` lands on disk (the seed is unconditional) while
    /// `mustard.json` declares only the `userPromptSubmit` half, so the § Dispatch
    /// rules are delivered to nobody — strictly worse than the over-budget
    /// single file they replaced.
    #[test]
    fn migration_backfills_the_dispatch_half_on_a_pre_split_project() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std_fs::write(
            root.join("mustard.json"),
            r#"{"version":"1.0.0","inject":[{"on":"userPromptSubmit","file":".claude/mustard/orchestrator.md","once":true}]}"#,
        )
        .unwrap();

        let report = upsert_project(root, None, InstallMode::Shared).unwrap();

        let config = ProjectConfig::load(root);
        let dispatch = config
            .inject
            .iter()
            .find(|e| e.file == ".claude/mustard/dispatch.md")
            .expect("the second half was not declared");
        assert_eq!(
            dispatch.on, "userPromptSubmit",
            "the backfilled half must ride the self-healing event, not the one \
             whose missed paths caused this defect",
        );
        // The line must name the RESULT. It used to say "on sessionStart" —
        // the event entries are moved OFF — and a reader relayed that to the
        // operator as where the file had landed (measured in the field).
        assert!(
            report.migrated.iter().any(|m| m.contains("router injectables")),
            "migration reported: {:?}",
            report.migrated,
        );
        assert!(
            !report.migrated.iter().any(|m| m.contains("sessionStart")),
            "the report must not name the event it moved AWAY from: {:?}",
            report.migrated,
        );

        // Idempotent: a second upsert adds nothing and reports nothing.
        let again = upsert_project(root, None, InstallMode::Shared).unwrap();
        assert_eq!(
            ProjectConfig::load(root).inject.len(),
            config.inject.len(),
            "the backfill ran twice",
        );
        assert!(
            !again.migrated.iter().any(|m| m.contains("router injectables")),
            "an already-split project must report no migration: {:?}",
            again.migrated,
        );
    }

    /// The stale-plugin branch the migration test cannot isolate: a manifest
    /// WITHOUT sibling hooks must not authorise the move.
    ///
    /// With one hook per event, moving the whole set onto `userPromptSubmit`
    /// folds it into a single response — measured at 11,646 characters on the
    /// router as it stood then, over the 10,000 cap. The split being undone was
    /// at least under it, so migrating against a stale plugin leaves the project
    /// worse.
    #[test]
    fn a_manifest_without_sibling_hooks_does_not_authorise_the_move() {
        let dir = tempdir().unwrap();
        let hooks = dir.path().join("hooks");
        std_fs::create_dir_all(&hooks).unwrap();

        // The pre-sibling shape: one hook for the whole event.
        std_fs::write(
            hooks.join("hooks.json"),
            r#"{"hooks":{"UserPromptSubmit":[
                {"hooks":[{"command":"mustard-rt on UserPromptSubmit"}]}
            ]}}"#,
        )
        .unwrap();
        assert!(
            !manifest_registers_sibling_hooks(&hooks.join("hooks.json")),
            "one hook for the event is not sibling hooks",
        );

        // One `--inject` is not enough either: the halves need one each.
        std_fs::write(
            hooks.join("hooks.json"),
            r#"{"hooks":{"UserPromptSubmit":[
                {"hooks":[{"command":"mustard-rt on UserPromptSubmit --inject .claude/mustard/orchestrator.md"}]}
            ]}}"#,
        )
        .unwrap();
        assert!(!manifest_registers_sibling_hooks(&hooks.join("hooks.json")));

        // Two claims: the move is safe.
        std_fs::write(
            hooks.join("hooks.json"),
            r#"{"hooks":{"UserPromptSubmit":[
                {"hooks":[{"command":"mustard-rt on UserPromptSubmit --inject .claude/mustard/orchestrator.md"}]},
                {"hooks":[{"command":"mustard-rt on UserPromptSubmit --inject .claude/mustard/dispatch.md"}]}
            ]}}"#,
        )
        .unwrap();
        assert!(manifest_registers_sibling_hooks(&hooks.join("hooks.json")));

        // An unreadable manifest answers NO — the conservative direction, since
        // the caller only moves entries on a yes.
        assert!(!manifest_registers_sibling_hooks(&hooks.join("nao-existe.json")));
    }

    /// A project split across two events is MOVED onto one, not
    /// duplicated.
    ///
    /// This is the shape every project installed between the split and this
    /// unit carries: both halves declared, dispatch on `sessionStart`. That
    /// event misses every path that opens no session — `fork` matched no
    /// matcher at all — so the question that opens a unit was absent with
    /// nothing to say so.
    #[test]
    fn migration_consolidates_the_router_onto_userpromptsubmit() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std_fs::write(
            root.join("mustard.json"),
            r#"{"version":"1.0.0","inject":[
                {"on":"userPromptSubmit","file":".claude/mustard/orchestrator.md","once":true},
                {"on":"sessionStart","file":".claude/mustard/dispatch.md","once":true}
            ]}"#,
        )
        .unwrap();

        // The move is gated on the INSTALLED plugin registering a hook per
        // injectable: with a stale one, a single hook folds the whole set into
        // one response (measured at 11,646 chars on the router as it stood then,
        // over the cap) — worse than the split it would undo.
        //
        // That gate reads the plugin registry of the machine running the test,
        // which a tempdir cannot fake, so the stale-plugin BRANCH is covered by
        // `plugin_registers_sibling_hooks` unit tests rather than here. What
        // this test pins is the consolidation itself, with the manifest in
        // reach.
        let hooks = root.join("plugin/hooks");
        std_fs::create_dir_all(&hooks).unwrap();
        std_fs::write(
            hooks.join("hooks.json"),
            r#"{"hooks":{"UserPromptSubmit":[
                {"hooks":[{"command":"mustard-rt on UserPromptSubmit --inject .claude/mustard/orchestrator.md"}]},
                {"hooks":[{"command":"mustard-rt on UserPromptSubmit --inject .claude/mustard/dispatch.md"}]}
            ]}}"#,
        )
        .unwrap();
        upsert_project(root, None, InstallMode::Shared).unwrap();

        let config = ProjectConfig::load(root);
        // The count is the SEEDED layout, not a literal: the migration adds the
        // parts a pre-split project never declared, and a fixed 2 would read
        // that backfill as a duplication the day a third part shipped.
        assert_eq!(
            config.inject.len(),
            default_inject_entries().len(),
            "the entry was duplicated: {:?}",
            config.inject,
        );
        for seeded in default_inject_entries() {
            assert_eq!(
                config
                    .inject
                    .iter()
                    .filter(|e| same_declared_path(&e.file, &seeded.file))
                    .count(),
                1,
                "`{}` is declared more than once: {:?}",
                seeded.file,
                config.inject,
            );
        }
        for entry in &config.inject {
            assert_eq!(
                entry.on, "userPromptSubmit",
                "`{}` was left on `{}`; a half delivered only at session start is missed \
                 by every path that opens no session",
                entry.file, entry.on,
            );
        }

        // Idempotent: a project already consolidated is not touched again.
        let again = upsert_project(root, None, InstallMode::Shared).unwrap();
        // Asserts on the CURRENT wording. It used to look for "dispatch", a
        // word the report lost when it was rewritten to name the result — so
        // the check could never fire again, whatever the behaviour (found in
        // review). An assertion whose needle left the haystack is worse than
        // none: it reads as coverage.
        assert!(
            !again.migrated.iter().any(|m| m.contains("router injectables")),
            "an already-consolidated project must report no migration: {:?}",
            again.migrated,
        );
    }

    /// An operator who removed the router from `inject` is not handed it back.
    /// The backfill completes a declared router; it does not re-impose one.
    #[test]
    fn migration_does_not_reimpose_a_router_the_operator_dropped() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std_fs::write(
            root.join("mustard.json"),
            r#"{"version":"1.0.0","inject":[{"on":"sessionStart","file":"docs/my-rules.md","once":false}]}"#,
        )
        .unwrap();

        upsert_project(root, None, InstallMode::Shared).unwrap();

        let config = ProjectConfig::load(root);
        assert_eq!(config.inject.len(), 1, "curated inject list untouched: {:?}", config.inject);
        assert_eq!(config.inject[0].file, "docs/my-rules.md");
    }

    /// The injectables are ALWAYS rewritten: a diverged copy in the project
    /// comes back to the seed, and the run reports `Updated`, never `Preserved`.
    ///
    /// This replaces the fingerprint-catalog merge, which tried to tell a stale
    /// seed from the operator's own text and preserved whatever it failed to
    /// recognise — so a corrected rule could stop at the project boundary
    /// without a word. The write is what makes the rule reach an installed
    /// project; `Updated` in the report is what makes the overwrite visible.
    #[test]
    fn the_injectables_are_always_rewritten() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        std_fs::create_dir_all(claude.join("mustard")).unwrap();
        for (name, _) in INJECTABLE_SEEDS {
            std_fs::write(claude.join("mustard").join(name), "# the operator wrote this\n").unwrap();
        }

        let report = seed_injectable_files(&claude).unwrap();

        assert_eq!(report.len(), INJECTABLE_SEEDS.len());
        for ((reported, outcome), (name, body)) in report.iter().zip(INJECTABLE_SEEDS) {
            assert_eq!(reported, name, "the report must name the file it wrote");
            assert_eq!(
                *outcome,
                SeedOutcome::Updated,
                "{name} diverged and was replaced, so the overwrite has to be \
                 REPORTED — a silent `Preserved` is the failure this rule removes",
            );
            assert_eq!(
                std_fs::read_to_string(claude.join("mustard").join(name)).unwrap(),
                *body,
                "{name} still carries the operator's text, so the harness reads a \
                 rule Mustard did not ship",
            );
        }

        // Idempotent: a second run changes nothing and says so.
        let again = seed_injectable_files(&claude).unwrap();
        for (name, outcome) in &again {
            assert_eq!(*outcome, SeedOutcome::Preserved, "{name} rewritten with no change");
        }
    }

    /// An injectable that EXISTS but cannot be read is rewritten, not preserved.
    ///
    /// The general "never stomp what we could not inspect" rule belongs to the
    /// seeds whose owner is the project. Applied here it carved a hole straight
    /// through the always-rewrite contract stated three lines above the call: a
    /// corrupted `dispatch.md` kept every correction out of the project, in
    /// silence, and reported `Preserved` — the same shape as the fingerprint
    /// catalog that was removed for exactly this.
    ///
    /// Unreadable is spelled as invalid UTF-8 rather than a permission bit: it
    /// is a real corruption, it reaches the same `Err(_)` arm, and it behaves
    /// identically on Windows, where a chmod-based test would silently pass by
    /// never producing the error at all.
    #[test]
    fn an_unreadable_injectable_is_rewritten_not_silently_preserved() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        let mustard = claude.join("mustard");
        std_fs::create_dir_all(&mustard).unwrap();
        for (name, _) in INJECTABLE_SEEDS {
            // Lone continuation bytes: present, non-empty, and not UTF-8.
            std_fs::write(mustard.join(name), [0x80_u8, 0xFF, 0xFE]).unwrap();
        }

        let report = seed_injectable_files(&claude).unwrap();

        for ((name, outcome), (_, body)) in report.iter().zip(INJECTABLE_SEEDS) {
            assert_eq!(
                *outcome,
                SeedOutcome::Updated,
                "{name} was unreadable and reported `{outcome:?}` — a rule corrected \
                 here never reaches the project, and nothing says so",
            );
            assert_eq!(
                std_fs::read_to_string(mustard.join(name)).unwrap(),
                *body,
                "{name} still carries the unreadable bytes the window cannot use",
            );
        }
    }

    /// Every part of the router rides the SELF-HEALING event, and each fits the
    /// ceiling one hook response carries.
    ///
    /// `additionalContext` is capped at 10,000 characters per hook RESPONSE; the
    /// overflow is not cut mid-sentence — the harness writes it to a file and
    /// hands the window a preview plus a path, which is worse than a visible
    /// truncation: the router silently stops being in force.
    ///
    /// This replaces a ratchet that asserted the halves must sit on DIFFERENT
    /// events, on the theory that siblings share one ceiling. They do not.
    /// Measured 2026-08-25: two sibling hooks on one `UserPromptSubmit`, each
    /// emitting 6,000 characters, both arrived intact and separate. The cap is
    /// per response, so one sibling hook per injectable gives each its own.
    ///
    /// What the event choice buys instead is RE-DELIVERY. The `once` markers
    /// live under `.claude/.session/<session_id>/`, so any path that loses the
    /// window — a new session, a `fork`, a resume — also loses the marker, and
    /// the next prompt re-delivers on its own. `sessionStart` cannot do that:
    /// it fires only on openings, and `fork` matched no matcher at all.
    #[test]
    fn the_router_rides_the_self_healing_event_and_neither_half_overflows() {
        let entries = default_inject_entries();
        for file in injectable_declared_paths() {
            let entry = entries
                .iter()
                .find(|e| e.file == file)
                .unwrap_or_else(|| panic!("{file} is not declared — it would be seeded, never delivered"));
            assert_eq!(
                entry.on, "userPromptSubmit",
                "{file} rides `{}`; a half delivered only at session start is missed by \
                 every path that opens no session — `fork` above all",
                entry.on,
            );
        }

        // Each injectable is delivered by its OWN sibling hook, so each is
        // measured alone against the real ceiling. No composite budget applies.
        for (name, body) in INJECTABLE_SEEDS {
            let chars = body.chars().count();
            assert!(
                chars <= 10_000,
                "{name} is {chars} characters; a hook response carries 10,000 of \
                 additionalContext and the overflow becomes a file path instead of \
                 text in force. Split it and give each half a hook — never compress \
                 a rule out to fit",
            );
        }
    }
}
