//! `mustard init` — thin bootstrap for a Claude Code project (Mustard 2.0).
//!
//! The heavy `.claude/` payload — commands, skills, agents, refs, hooks — ships
//! in the **`mustard` plugin**, distributed through a private git marketplace.
//! `init` does not copy that payload; it lays down the small set of files a
//! plugin cannot ship. The flow:
//!
//! 1. guard the location: init refuses a directory that sits inside a git
//!    repository without being its root (the workspace resolver anchors on git
//!    roots — see [`seeding::guard_init_location`]);
//! 2. hide the footprint from this clone's git, before anything is written
//!    ([`seeding::hide_footprint`]);
//! 3. handle an already-present `.claude/` (force-overwrite, merge, or
//!    backup-then-overwrite — interactively asked when no flag decides it,
//!    [`questions`]);
//! 4. seed the harness into `.claude/` — delegated to the core seeding engine
//!    (`mustard_core::platform::project_seed`, fed by the compiled-in
//!    `platform::seeds` constants; `mustard-rt run upsert` consumes the same
//!    engine):
//!    - `settings.local.json` — the reduced SEED (env / permissions /
//!      statusLine / plansDirectory …), rtk's hook following
//!      `mustard.json#rtk`, and Claude Code's own signature off; plugin
//!      enablement is NOT planted (user-scope choice);
//!    - `mustard/*.md` — the injectable instruction files, always rewritten:
//!      `orchestrator.md`, `dispatch.md` and `material.md`, one sibling hook
//!      each on `userPromptSubmit`;
//!    - `.gitignore` — covers the ephemeral harness state;
//! 5. write the single project-root `mustard.json` ([`project_config`]):
//!    git-flow + detected commands + the `runtime`/`version` stamp + the
//!    default `inject` declarations (seeded only when the user has none);
//! 6. list what an older Mustard left in files that are not its own — the
//!    marks in the `CLAUDE.md` files, the seed's lines in the team's
//!    `.claude/settings.json`, a planted `.claude/CLAUDE.md` — and say how to
//!    take it out. Nothing of it is removed here: that happens through
//!    `/mustard:upsert`, after the person says yes to the list.
//!
//! Nothing is staged or committed, and nothing is written outside the project:
//! `~/.claude/` is never touched. A re-run re-stamps `mustard.json#version`,
//! and in a repository that versions the file the new stamp is a change for the
//! person to commit.
//!
//! The install is always PRIVATE (`mustard_core::InstallMode::Private`): every
//! file above lands on disk — the harness needs it there — but none of it is
//! visible to the host repository's git, and no `.github/` scaffolding is
//! seeded. There is no flag and no prompt for it.
//!
//! What is NOT a step of `init`, though it still happens on a `mustard init`
//! run: the rtk gate and the ripgrep installer ([`tools`]). They act on the
//! MACHINE, so they live in `cli::dispatch`, and a library call never takes
//! them on its caller's behalf. `apps/cli/tests/library_is_pure.rs` measures it.
//!
//! ## The parts
//!
//! - this file — the flow and its two library entry points;
//! - [`questions`] — what to do with an existing `.claude/`;
//! - [`seeding`] — the guard, the footprint, the narration and the payload;
//! - [`project_config`] — the one write of `mustard.json`;
//! - [`tools`] — the rtk gate and the ripgrep installer.

use std::path::Path;

use anyhow::{Context, Result};
use mustard_core::io::fs as mfs;
use mustard_core::{InstallMode, ProjectConfig, Runtime};

mod project_config;
mod questions;
mod seeding;
mod tools;

pub(crate) use tools::{ensure_ripgrep, probe_rtk};

use questions::ExistingAction;

/// Flags accepted by `mustard init`.
#[derive(Debug, Default, Clone)]
pub struct InitOptions {
    /// Overwrite an existing `.claude/` without a backup.
    pub force: bool,
    /// Accept defaults without prompting.
    pub yes: bool,
    /// Print intended actions without touching disk.
    pub dry_run: bool,
}

/// What an `init` run actually DID, reported as a fact rather than folded into
/// `Result`.
///
/// The distinction is load-bearing and was found in review: `Ok(())` used to
/// cover both "the project was seeded" and "the operator answered Cancel", so a
/// caller that acted on success alone changed the machine after an explicit
/// refusal. A closed set of dispositions lets the caller judge; the callee keeps
/// its opinion to itself. Mold: `core-outcome-pattern`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitOutcome {
    /// The project was seeded: `.claude/` and `mustard.json` are on disk.
    Installed,
    /// An existing `.claude/` was found and the operator chose to stop.
    ///
    /// NOT "nothing was written": `hide_footprint` runs BEFORE the prompt, and a
    /// cancelled run was measured appending 31 rules to the project's
    /// `.git/info/exclude`. What this variant promises is narrower and is the
    /// part the caller needs — no install completed, so nothing about the
    /// MACHINE may change on this run's account.
    Cancelled,
    /// `--dry-run`: the plan was printed and no path was touched.
    DryRun,
}

/// [`init`] with the `templates/` directory supplied explicitly.
///
/// Splitting this out keeps template resolution (an environment concern) out
/// of the install logic, so tests can drive a fixture tree and a caller can
/// point at its own bundled payload — no process-global env var.
pub fn init_with_templates(
    project_path: &Path,
    templates_dir: &Path,
    options: &InitOptions,
) -> Result<InitOutcome> {
    // NO RTK PROBE HERE, and that is the point of this function's split.
    //
    // Probing `PATH` is an environment concern, which the doc above says this
    // half deliberately does not carry. It used to call `probe_rtk()`, and that
    // function ends in `std::process::exit(1)` — so this function, which
    // returns `Result<()>`, could instead KILL ITS CALLER'S PROCESS. Any
    // library consumer lost the chance to handle it; a `Result` that sometimes
    // terminates the program is not a contract.
    //
    // Measured, not theorised: an integration test in another crate died as
    // "test exited abnormally" the first time CI ran it, because a clean runner
    // has no `rtk` and the process simply vanished mid-test. The `cfg!(test)`
    // escape hatch in `probe_rtk` does not reach it: that flag is true only
    // while THIS crate compiles its own unit tests, never for an integration
    // test living in another crate.
    //
    // The gate itself is not softened — it moved to `cli::dispatch`, where the
    // terminal user still meets it before any disk write. See `probe_rtk`.

    let project_path = project_path
        .canonicalize()
        .with_context(|| format!("resolving project path {}", project_path.display()))?;
    let claude_path = project_path.join(".claude");
    // Unconditional, and both install faces spell it the same way. There is no
    // flag, no config key and no detection step that could answer otherwise:
    // installing the harness INTO someone else's repository is not an outcome
    // this command can be steered towards, by anyone, including by forgetting.
    let mode = InstallMode::Private;

    // Location guard — runs in dry-run too: the honest "intended action" for a
    // subdirectory of a git repository is a refusal, not a simulated install.
    seeding::guard_init_location(&project_path)?;

    println!("\nMustard\n");

    let runtime = Runtime::detect();
    println!("[mustard] runtime: {} {}/{}", runtime.kind, runtime.os, runtime.arch);

    if options.dry_run {
        if mode.is_private() {
            println!(
                "  (dry-run) would install PRIVATELY: the harness settings would go to settings.local.json,"
            );
            println!(
                "            the footprint would be added to this clone's git exclude file, and no .github/ would be seeded"
            );
        }
        println!("  (dry-run) would seed the harness into {}:", claude_path.display());
        println!("    settings.local.json — reduced seed, rtk's hook per mustard.json#rtk, Claude Code's signature off");
        println!("    mustard/*.md   — injectable instruction files (orchestrator, dispatch, material); hooks inject them per mustard.json#inject");
        println!("    .gitignore     — ephemeral harness state");
        println!("  (dry-run) would list what an older Mustard left in CLAUDE.md files and .claude/settings.json (nothing leaves without a yes)");
        println!(
            "  (dry-run) would write git-flow + commands + runtime/version + inject declarations to {}",
            project_path.join("mustard.json").display()
        );
        println!("  (dry-run) content payload (commands/skills/agents/refs) now ships in the `mustard` plugin — not written");
        return Ok(InitOutcome::DryRun);
    }

    // Step 0, private only: hide the footprint BEFORE any of it exists — before
    // `.claude/` is created, and before the backup-and-overwrite branch below
    // can leave a `.claude.backup.<stamp>/` beside it. A refusal here writes
    // nothing at all, not even a directory.
    if mode.is_private() {
        seeding::hide_footprint(&project_path)?;
    }

    // Decide how to treat an existing `.claude/`. A fresh project is a plain
    // overwrite of an empty tree.
    let overwrite = if claude_path.exists() {
        match questions::decide_existing_action(&claude_path, options)? {
            ExistingAction::Cancel => {
                println!("\n  Cancelled.\n");
                // Cancelled, NOT installed. The caller reads the difference: an
                // `Ok(())` here used to let `cli::dispatch` run the tool
                // installers after an explicit refusal (found in review).
                return Ok(InitOutcome::Cancelled);
            }
            ExistingAction::Merge => false,
            ExistingAction::Overwrite => true,
        }
    } else {
        true
    };

    mfs::create_dir_all(&claude_path)
        .with_context(|| format!("creating {}", claude_path.display()))?;

    // The `inject` migrations (idempotent, fail-open): they only touch
    // `mustard.json`, which is Mustard's own file.
    seeding::report_migration(&mustard_core::migrate_inject_declarations(&project_path, &claude_path));

    // The settings: the reduced seed, rtk's hook as `mustard.json#rtk` says and
    // Claude Code's signature off — the core engine owns the content, the merge
    // rules and the destination (the untracked local layer).
    let settings_name = if mode.is_private() {
        ".claude/settings.local.json"
    } else {
        ".claude/settings.json"
    };
    let rtk = ProjectConfig::load(&project_path).rtk();
    let outcome = mustard_core::seed_settings(&claude_path, overwrite, mode, rtk)
        .with_context(|| format!("seeding {settings_name}"))?;
    seeding::report_seed(settings_name, outcome);
    // The injectable instruction files — the harness's own rules, so the answer
    // to "merge or overwrite?" does not reach them: the seeder takes no such
    // argument and always lays the shipped text down again.
    for (name, outcome) in mustard_core::seed_injectable_files(&claude_path)
        .context("seeding .claude/mustard/ injectables")?
    {
        seeding::report_seed(&format!(".claude/mustard/{name}"), outcome);
    }
    // The ephemeral-state .gitignore.
    let outcome = mustard_core::seed_gitignore(&claude_path, overwrite)
        .context("seeding .claude/.gitignore")?;
    seeding::report_seed(".claude/.gitignore", outcome);

    // Project-root `.github/` scaffolding (PR template) — skipped by a private
    // install: it lands outside `.claude/`, where nothing else covers it, and
    // writing it into a client's repository is the visible trace the mode
    // exists to avoid.
    if mode.is_private() {
        println!("  skipped .github/ templates (private install — the host repository stays untouched)");
    } else {
        let gh = seeding::install_github_templates(templates_dir, &project_path)?;
        if gh > 0 {
            println!("  wrote {gh} GitHub template(s) at .github/");
        }
    }

    // The single project-root mustard.json: git-flow + detected commands +
    // runtime/version stamp. One file, one write. A re-run re-stamps `version`.
    project_config::write_project_config(&project_path, &runtime, !options.yes)?;

    // What an older Mustard left in files that are not its own: listed, never
    // taken out from here.
    seeding::report_cleanup(&mustard_core::platform::project_seed::cleanup::plan(&project_path));

    print_next_steps();
    Ok(InitOutcome::Installed)
}

/// Run `mustard init` against `project_path`.
///
/// This is the library entry point. The binary passes the process working
/// directory; a caller may pass any folder. The bundled `templates/` directory
/// is located by the seeding part; callers that already know its location use
/// [`init_with_templates`].
pub fn init(project_path: &Path, options: &InitOptions) -> Result<InitOutcome> {
    let templates_dir = seeding::resolve_templates_dir()?;
    init_with_templates(project_path, &templates_dir, options)
}

/// Print the closing "next steps" block.
///
/// This surface has to stand on its own, because `mustard init` is most often
/// run DIRECTLY in a project, with no installer around it and no document open.
/// It therefore prints the two commands verbatim: the placeholder this replaced
/// (`add <mustard repo or local directory>` → `install mustard`) typed as
/// written answers `Plugin "mustard" not found in any marketplace`.
///
/// The Linux installer also runs `mustard init --yes` at the end of a
/// `curl … | sh`. There this block is NOT the last thing on screen — the
/// installer prints its own closing block after it — so the two would otherwise
/// teach the same plugin step twice, in English and then in Portuguese. The
/// installer resolves that on its side: when it ran init itself it points back
/// at these lines instead of reprinting them (`packaging/installer/install.sh`).
/// NOTE: in a DIRECT run this may not be the last thing on screen — the
/// ripgrep setup lines follow it, because that installer lives in
/// `cli::dispatch` and runs after `init` returns. The block is still the last
/// word of the INSTALL; what trails it is tool setup, not project state.
/// Keep these two commands here regardless; they are what the direct run needs.
fn print_next_steps() {
    println!("\nDone!\n");
    println!("Next:");
    println!("  1. Install the plugin INSIDE Claude Code — type these two lines there,");
    println!("     not in this terminal (already installed? nothing to do):");
    println!("     /plugin marketplace add rubensrpj/mustard");
    println!("     /plugin install mustard@mustard-local");
    println!("  2. Reload Claude Code, then run /scan to analyze your codebase.\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use tempfile::tempdir;

    /// Build a minimal fake `templates/` tree and return its path. Tests point
    /// `init_with_templates` at this so they never touch the real payload. The
    /// four harness seeds (settings, injectables, `.gitignore`) come from the
    /// COMPILED-IN core constants now — this fixture only carries what the
    /// templates dir still owns for init (`.github/`, manifests) plus a
    /// `commands/` decoy: the thin init must NOT copy it into `.claude/`.
    fn fake_templates(root: &Path) -> PathBuf {
        let templates = root.join("templates");
        fs::create_dir_all(templates.join("commands")).unwrap();
        fs::write(templates.join("commands/feature.md"), "feature").unwrap();
        templates
    }

    #[test]
    fn init_seeds_harness_and_enables_plugin() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("project");
        fs::create_dir_all(&project).unwrap();

        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        let claude = project.join(".claude");
        // The seed files are laid down — injectables replace the planted orchestrator.
        // The LOCAL layer, always: the install has no shared mode any more, so
        // the versioned twin must never appear.
        assert!(
            claude.join("settings.local.json").exists(),
            ".claude/settings.local.json seeded",
        );
        assert!(
            !claude.join("settings.json").exists(),
            "the versioned settings file is never created",
        );
        assert!(
            claude.join("mustard").join("orchestrator.md").exists(),
            ".claude/mustard/orchestrator.md seeded"
        );
        assert!(
            !claude.join("CLAUDE.md").exists(),
            "init must NOT plant .claude/CLAUDE.md — the orchestrator is injected now"
        );
        assert!(claude.join(".gitignore").exists(), ".claude/.gitignore seeded");

        // The content payload is the plugin's now — init must NOT copy it.
        assert!(
            !claude.join("commands").exists(),
            "commands/skills/agents/refs ship in the mustard plugin, never .claude/"
        );
        // The harness declares no MCP server, so init writes no `.mcp.json`.
        assert!(
            !project.join(".mcp.json").exists(),
            "init must not write .mcp.json — the harness declares no MCP server"
        );

        // settings.json carries the reduced seed keys and NO plugin enablement —
        // that choice lives at user scope, never planted into the project.
        // Content now comes from the compiled-in core seed, so assert on a
        // stable key the real seed carries.
        let settings = crate::fs_ops::read_json_object(&claude.join("settings.local.json"));
        assert_eq!(
            settings
                .get("env")
                .and_then(|e| e.get("MUSTARD_SPEC_SIZE_MODE"))
                .and_then(|v| v.as_str()),
            Some("warn"),
            "the compiled-in seed's env is laid down verbatim"
        );
        assert!(settings.get("statusLine").is_some(), "seed statusLine present");
        assert!(
            settings
                .get("enabledPlugins")
                .and_then(|p| p.get("mustard@mustard"))
                .is_none(),
            "init must not plant enabledPlugins in the project"
        );
        assert!(
            settings
                .get("extraKnownMarketplaces")
                .and_then(|m| m.get("mustard"))
                .is_none(),
            "init must not plant a marketplace entry in the project"
        );

        // .gitignore covers the ephemeral harness state.
        assert!(
            fs::read_to_string(claude.join(".gitignore")).unwrap().contains(".events/"),
            ".gitignore covers the ephemeral .events/ dir"
        );

        // The SINGLE project-root mustard.json carries git-flow, the version
        // stamp and runtime, and NO language: the install ran without asking,
        // so none was chosen and none is written. There is NO
        // .claude/mustard.json.
        let cfg = crate::fs_ops::read_json_object(&project.join("mustard.json"));
        assert_eq!(
            cfg.get("version").and_then(|v| v.as_str()),
            Some(mustard_core::harness_version().as_str()),
            "the stamp is the harness version, not the CLI crate's"
        );
        assert!(cfg.get("runtime").is_some(), "runtime block written");
        assert!(cfg.get("git").is_some(), "git-flow block written");
        for key in ["language", "specLang", "tone"] {
            assert!(cfg.get(key).is_none(), "a non-interactive install writes no {key}: {cfg:?}");
        }
        // The default inject declarations are seeded: the router's three parts,
        // each on its OWN sibling hook. The cap is per hook RESPONSE, not per
        // event, so siblings share no budget — a part that outgrows the ceiling
        // is SPLIT and given another hook, never compressed until a rule drops
        // out. The response style is a plugin output-style now, not a
        // per-project injectable.
        let inject = cfg.get("inject").and_then(|v| v.as_array()).expect("inject seeded");
        assert_eq!(inject.len(), 3, "the router's three parts: {inject:?}");
        for (i, file) in [
            ".claude/mustard/orchestrator.md",
            ".claude/mustard/dispatch.md",
            ".claude/mustard/material.md",
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(inject[i].get("file").and_then(|v| v.as_str()), Some(*file));
            // Every part rides `userPromptSubmit`: that event self-heals (the
            // `once` markers are per session_id, so a fork/resume re-delivers on
            // the next prompt), which `sessionStart` cannot do.
            assert_eq!(
                inject[i].get("on").and_then(|v| v.as_str()),
                Some("userPromptSubmit"),
                "{file} must ride the self-healing event",
            );
            assert_eq!(inject[i].get("once").and_then(|v| v.as_bool()), Some(true));
        }
        assert!(
            !claude.join("mustard.json").exists(),
            "no .claude/mustard.json — config lives only at the project root"
        );

        // init seeds no entity-registry — the repo model is grain's
        // `.claude/grain.model.json`, produced on demand by `mustard-rt run scan`.
        assert!(!claude.join("entity-registry.json").exists());
    }

    /// Run git in `dir`, failing the test loudly — a half-built repository
    /// would make the assertion below prove nothing.
    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
        assert!(out.status.success(), "git {args:?} failed: {out:?}");
    }


    /// Um repositório que versiona o `mustard.json` recebe o selo novo, e o
    /// instalador não faz commit nenhum: o histórico fica igual, nada é
    /// preparado, e a mudança fica para a pessoa, junto com a dela.
    #[test]
    fn install_never_commits_anything() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("project");
        fs::create_dir_all(&project).unwrap();

        git(&project, &["init", "--initial-branch=main"]);
        git(&project, &["config", "user.email", "t@example.com"]);
        git(&project, &["config", "user.name", "t"]);
        fs::write(project.join("mustard.json"), "{\n  \"version\": \"0.0.0-old\"\n}\n").unwrap();
        fs::write(project.join("notes.md"), "draft\n").unwrap();
        git(&project, &["add", "."]);
        git(&project, &["commit", "-m", "seed"]);
        let head = rev_parse(&project);
        // The operator's own uncommitted edit, present BEFORE the install runs.
        fs::write(project.join("notes.md"), "draft, still being written\n").unwrap();

        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        let cfg = crate::fs_ops::read_json_object(&project.join("mustard.json"));
        assert_eq!(
            cfg.get("version").and_then(|v| v.as_str()),
            Some(mustard_core::harness_version().as_str()),
            "the stamp really moved, or there was nothing to leave uncommitted",
        );
        assert_eq!(rev_parse(&project), head, "the install made a commit");
        let changed = git_out(&project, &["diff", "--name-only"]);
        assert_eq!(changed, "mustard.json\nnotes.md", "the stamp and the operator's edit are both left uncommitted");
        assert_eq!(git_out(&project, &["diff", "--cached", "--name-only"]), "", "nothing is staged either");
    }

    fn rev_parse(dir: &Path) -> String {
        git_out(dir, &["rev-parse", "HEAD"])
    }

    fn git_out(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git").args(args).current_dir(dir).output().expect("git runs");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[test]
    fn init_dry_run_writes_nothing() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let dry = work.path().join("dry");
        fs::create_dir_all(&dry).unwrap();

        init_with_templates(
            &dry,
            &templates,
            &InitOptions { yes: true, dry_run: true, ..InitOptions::default() },
        )
        .unwrap();

        assert!(!dry.join(".claude").exists(), "dry-run wrote nothing");
    }

    /// Regression guard for the `.claude/.claude/` nesting bug (I1 rule): even
    /// if `templates/` carries a stray `.claude/` sub-directory, the thin init —
    /// whose harness seeds are compiled-in constants, not directory copies —
    /// must never propagate it.
    #[test]
    fn init_does_not_create_nested_claude_dir() {
        let work = tempdir().unwrap();

        let templates = work.path().join("templates");
        fs::create_dir_all(templates.join("commands")).unwrap();
        // Inject the offending .claude/ inside templates/.
        fs::create_dir_all(templates.join(".claude/commands")).unwrap();
        fs::write(templates.join(".claude/commands/notes.md"), "boilerplate").unwrap();

        let project = work.path().join("project");
        fs::create_dir_all(&project).unwrap();

        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        let nested = project.join(".claude").join(".claude");
        assert!(!nested.exists(), ".claude/.claude/ must not be created — I1 rule");
    }

    #[test]
    fn init_merge_rewrites_the_injectable_and_backfills() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("project");
        let claude = project.join(".claude");
        // A diverged injectable already present in .claude/mustard/.
        fs::create_dir_all(claude.join("mustard")).unwrap();
        fs::write(claude.join("mustard/orchestrator.md"), "USER EDIT").unwrap();

        // Non-interactive existing-dir path resolves to a merge.
        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        // The injectable is the harness's own rules, so merge mode does not
        // reach it: the seed is laid down again whatever was there…
        assert_eq!(
            fs::read_to_string(claude.join("mustard/orchestrator.md")).unwrap(),
            mustard_core::ORCHESTRATOR_MD,
            "merge must still rewrite the injectable — it is not project configuration"
        );
        // …while a seed the user does not have is backfilled…
        assert!(
            claude.join(".gitignore").exists(),
            "merge backfills a missing seed"
        );
        // …and no plugin enablement is planted on the merge path either.
        let settings = crate::fs_ops::read_json_object(&claude.join("settings.local.json"));
        assert!(
            settings
                .get("enabledPlugins")
                .and_then(|p| p.get("mustard@mustard"))
                .is_none(),
            "merge must not plant plugin enablement"
        );
    }

    /// Um orquestrador plantado por uma instalação antiga e as marcas do
    /// Mustard num `CLAUDE.md` são só listados: o instalador não tira nada, e
    /// o `.claude/CLAUDE.md` que não é do Mustard nem aparece.
    #[test]
    fn init_lists_what_an_older_mustard_left_and_removes_nothing() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("project");
        let claude = project.join(".claude");
        fs::create_dir_all(&claude).unwrap();
        fs::write(claude.join("CLAUDE.md"), "# Orchestrator Rules\n\nYou are the router.\n").unwrap();
        let root_md = "@.claude/scan-map.md\n\n# (root)\n\n## Guards\n\n<!-- mustard:guards -->\n- keep this guard\n<!-- /mustard:guards -->\n";
        fs::write(project.join("CLAUDE.md"), root_md).unwrap();

        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        assert!(claude.join("CLAUDE.md").exists(), "nothing leaves without a yes");
        assert_eq!(fs::read_to_string(project.join("CLAUDE.md")).unwrap(), root_md);
        let plan = mustard_core::platform::project_seed::cleanup::plan(&project);
        let listed: Vec<&str> = plan.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(listed, [".claude/CLAUDE.md", "CLAUDE.md"], "the install left the list for the door");

        fs::write(claude.join("CLAUDE.md"), "MY OWN NOTES\n").unwrap();
        let plan = mustard_core::platform::project_seed::cleanup::plan(&project);
        assert!(
            !plan.files.iter().any(|f| f.path == ".claude/CLAUDE.md"),
            "a .claude/CLAUDE.md without the marker is the user's and never listed",
        );
    }
    #[test]
    fn init_preserves_user_inject_entries() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("project");
        fs::create_dir_all(&project).unwrap();
        // The user already curated their own inject list.
        fs::write(
            project.join("mustard.json"),
            r#"{"inject":[{"on":"sessionStart","file":"docs/my-rules.md","once":false}]}"#,
        )
        .unwrap();

        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        let cfg = crate::fs_ops::read_json_object(&project.join("mustard.json"));
        let inject = cfg.get("inject").and_then(|v| v.as_array()).expect("inject present");
        assert_eq!(inject.len(), 1, "the curated list is preserved, not replaced: {inject:?}");
        assert_eq!(
            inject[0].get("file").and_then(|v| v.as_str()),
            Some("docs/my-rules.md"),
            "user entry survives verbatim"
        );
    }

    #[test]
    fn init_refuses_inside_git_repo_when_not_at_its_root() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        // `work` is a git repository root; the init target is a subdirectory.
        fs::create_dir_all(work.path().join(".git")).unwrap();
        let project = work.path().join("apps").join("dashboard");
        fs::create_dir_all(&project).unwrap();

        let err = init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap_err();

        let msg = format!("{err:#}");
        assert!(
            msg.contains("not the repository's root"),
            "refusal must be didactic, got: {msg}"
        );
        assert!(
            msg.contains("repository root:"),
            "refusal must name the repository root, got: {msg}"
        );
        // Refusal happens before any disk write.
        assert!(!project.join(".claude").exists(), "refusal wrote .claude/");
        assert!(!project.join("mustard.json").exists(), "refusal wrote mustard.json");
    }

    #[test]
    fn init_allows_at_git_repo_root() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("project");
        fs::create_dir_all(project.join(".git")).unwrap(); // project IS a repo root

        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        assert!(project.join(".claude").join("settings.local.json").exists());
        assert!(project.join("mustard.json").exists());
    }

    #[test]
    fn init_allows_at_submodule_root_with_git_file() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        // Outer repository root…
        fs::create_dir_all(work.path().join(".git")).unwrap();
        // …and a submodule below it: `.git` is a FILE with a `gitdir:` pointer.
        let sub = work.path().join("backend").join("service");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join(".git"), "gitdir: ../../.git/modules/service\n").unwrap();

        init_with_templates(
            &sub,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        assert!(
            sub.join(".claude").join("settings.local.json").exists(),
            "a submodule root (.git file) is a legitimate init target"
        );
    }

    #[test]
    fn init_allows_in_git_less_tree() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("plain");
        fs::create_dir_all(&project).unwrap(); // no .git anywhere up the tempdir

        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        assert!(project.join(".claude").join("settings.local.json").exists());
    }
}
