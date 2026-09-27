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
//!    - `mustard/session-map.md`, `mustard/pages/*.html` and
//!      `agents/mustard/*.md` — the session map, the two page templates and
//!      the three agents, always rewritten, in the language of
//!      `language.text`;
//!    - `.gitignore` — covers the ephemeral harness state;
//!
//!    Before those, the single project-root `mustard.json` is written
//!    ([`project_config`]): git-flow + text language + detected commands + the
//!    `runtime`/`version` stamp + the default `inject` declaration (seeded
//!    only when the user has none). It comes first because its language
//!    decides which texts are seeded;
//! 5. recompute, once, the search of every spec and of the lesson bank that an
//!    older Mustard wrote under another rule ([`refresh_search`]): the
//!    ordinary write to a spec only appends, so this is where an old line is
//!    brought up to date. A failure is a warning, never an abort;
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
//! visible to the host repository's git, and nothing is written outside
//! `.claude/` but `mustard.json`. There is no flag and no prompt for it.
//!
//! Everything laid down is compiled into the binary: `init` looks up no folder
//! of molds, so it runs in an empty project with no extra setting.
//!
//! What is NOT a step of `init`, though it still happens on a `mustard init`
//! run: the rtk gate, the ripgrep installer and the code-tool step ([`tools`]).
//! They act on the MACHINE, so they live in `cli::dispatch`, and a library call
//! never takes them on its caller's behalf. `apps/cli/tests/library_is_pure.rs`
//! measures it. The code-tool step itself is
//! `mustard_core::platform::code_tools::ensure_code_tools`, the one the project
//! update runs too; [`tools`] only hands it the machine.
//!
//! ## The parts
//!
//! - this file — the flow and its library entry point;
//! - [`questions`] — what to do with an existing `.claude/`;
//! - [`seeding`] — the guard, the footprint and the narration;
//! - [`project_config`] — the one write of `mustard.json`;
//! - [`tools`] — the rtk gate, the ripgrep installer and the call into the
//!   code-tool step.

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use mustard_core::io::fs as mfs;
use mustard_core::{InstallMode, ProjectConfig, Runtime};

mod project_config;
mod questions;
mod seeding;
mod tools;

pub(crate) use tools::{ensure_code_tools, ensure_ripgrep, probe_rtk};

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

/// Run `mustard init` against `project_path`.
///
/// This is the library entry point. The binary passes the process working
/// directory; a caller may pass any folder. Everything the install lays down
/// is compiled into the binary: no folder of molds is looked up, so the
/// installer runs in an empty project with no extra setting.
pub fn init(project_path: &Path, options: &InitOptions) -> Result<InitOutcome> {
    // NO RTK PROBE HERE.
    //
    // Probing `PATH` is an environment concern, which a library entry point
    // deliberately does not carry. It used to call `probe_rtk()`, and that
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
        println!("  (dry-run) would install PRIVATELY: the harness settings would go to settings.local.json,");
        println!("            and the footprint would be added to this clone's git exclude file");
        println!("  (dry-run) would seed the harness into {}:", claude_path.display());
        println!("    settings.local.json — reduced seed, rtk's hook per mustard.json#rtk, Claude Code's signature off");
        println!("    mustard/session-map.md — the session map, delivered at session start per mustard.json#inject");
        println!("    mustard/pages/*.html — the spec page and project page templates, in the project's text language");
        println!("    agents/mustard/*.md — the mustard-wave, mustard-review and mustard-skill agents, in the project's text language");
        println!("    .gitignore     — ephemeral harness state");
        println!("  (dry-run) would list what an older Mustard left in CLAUDE.md files and .claude/settings.json (nothing leaves without a yes)");
        println!(
            "  (dry-run) would write git-flow + commands + runtime/version + inject declarations to {}",
            project_path.join("mustard.json").display()
        );
        println!("  (dry-run) content payload (commands/skills/agents/refs) now ships in the `mustard` plugin — not written");
        return Ok(InitOutcome::DryRun);
    }

    // Step 0: hide the footprint BEFORE any of it exists — before `.claude/` is
    // created, and before the backup-and-overwrite branch below can leave a
    // `.claude.backup.<stamp>/` beside it. A refusal here writes nothing at
    // all, not even a directory.
    seeding::hide_footprint(&project_path)?;

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

    // The single project-root mustard.json first: git-flow + text language +
    // detected commands + runtime/version stamp, in one write. Its answers —
    // the text language above all — decide what is seeded next.
    project_config::write_project_config(&project_path, &runtime, !options.yes)?;
    let config = ProjectConfig::load(&project_path);
    let text = config.language().text_or_default();

    // The settings: the reduced seed, rtk's hook as `mustard.json#rtk` says,
    // the response style of the text language and Claude Code's signature off
    // — the core engine owns the content, the merge rules and the destination
    // (the untracked local layer).
    // The local settings also receive the folder of the project's separate
    // copies, in either mode — in shared mode, that is all they receive.
    let seeded = mustard_core::seed_settings(&claude_path, overwrite, mode, config.rtk(), text)
        .context("seeding the settings under .claude/")?;
    for (name, outcome) in seeded {
        seeding::report_seed(name, outcome, false);
    }
    // Mustard's own texts — so the answer to "merge or overwrite?" does not
    // reach them: the seeder takes no such argument and always lays the
    // shipped text down again, in the text language.
    for (rel, outcome) in mustard_core::seed_harness_texts(&claude_path, text)
        .context("seeding Mustard's texts under .claude/")?
    {
        seeding::report_seed(&format!(".claude/{rel}"), outcome, true);
    }
    // The ephemeral-state .gitignore.
    let outcome = mustard_core::seed_gitignore(&claude_path, overwrite)
        .context("seeding .claude/.gitignore")?;
    seeding::report_seed(".claude/.gitignore", outcome, false);

    // A busca das specs e do banco de lições, acertada uma vez, depois de
    // tudo semeado: a gravação comum só acrescenta no fim do arquivo.
    refresh_search(&project_path, &mut std::io::stdout());

    // What an older Mustard left in files that are not its own: listed, never
    // taken out from here.
    seeding::report_cleanup(
        &mut std::io::stdout(),
        &mustard_core::platform::project_seed::cleanup::plan(&project_path),
    );

    print_next_steps();
    Ok(InitOutcome::Installed)
}

/// Põe o campo de busca que falta nas linhas das specs e do banco de lições
/// de `project`, pela mesma função da atualização pelo plugin
/// ([`mustard_core::io::spec_index::refresh_search`]), e diz em `out` quantas
/// linhas o ganharam. A falha vira um aviso com o comando que o põe depois,
/// e a instalação segue. Uma escrita em `out` que falha é descartada.
fn refresh_search(project: &Path, out: &mut impl Write) {
    match mustard_core::io::spec_index::refresh_search(project) {
        Ok(0) => {}
        Ok(lines) => {
            let noun = if lines == 1 { "line" } else { "lines" };
            let _ = writeln!(out, "  filled the search of {lines} {noun} an older Mustard wrote without it");
        }
        Err(failed) => {
            let _ = writeln!(out, "  warning: {failed}");
        }
    }
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
    println!("  2. Reload Claude Code, then describe the work you want done.\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;
    use tempfile::tempdir;


    #[test]
    fn init_seeds_harness_and_enables_plugin() {
        let work = tempdir().unwrap();
        let project = work.path().join("project");
        fs::create_dir_all(&project).unwrap();

        init(
            &project,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        let claude = project.join(".claude");
        // The seed files are laid down — the session map replaces the planted orchestrator.
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
            claude.join("mustard").join("session-map.md").exists(),
            ".claude/mustard/session-map.md seeded"
        );
        for name in ["wave.md", "review.md", "skill.md"] {
            assert!(claude.join("agents/mustard").join(name).exists(), "agent {name} seeded");
        }
        assert!(
            !claude.join("CLAUDE.md").exists(),
            "init must NOT plant .claude/CLAUDE.md — the orchestrator is injected now"
        );
        assert!(claude.join(".gitignore").exists(), ".claude/.gitignore seeded");
        // Sem specs, a busca não tem o que acertar e a pasta delas não nasce.
        assert!(!claude.join("spec").exists(), "a fresh install must not plant the specs folder");

        // The content payload is the plugin's now — init must NOT copy it.
        assert!(
            !claude.join("commands").exists(),
            "commands ship in the mustard plugin, never .claude/"
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
                .and_then(|e| e.get("MUSTARD_BOUNDARY_MODE"))
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
        // The default inject declaration is seeded: the session map, delivered
        // at session start, the one event that delivers declared text.
        let inject = cfg.get("inject").and_then(|v| v.as_array()).expect("inject seeded");
        assert_eq!(inject.len(), 1, "only the session map: {inject:?}");
        assert_eq!(
            inject[0].get("file").and_then(|v| v.as_str()),
            Some(".claude/mustard/session-map.md")
        );
        assert_eq!(inject[0].get("on").and_then(|v| v.as_str()), Some("sessionStart"));
        // Without a declared language the local settings get the pt-BR style.
        assert_eq!(
            settings.get("outputStyle").and_then(|v| v.as_str()),
            Some("mustard:mustard-pt-BR"),
        );
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

        init(
            &project,
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
        let dry = work.path().join("dry");
        fs::create_dir_all(&dry).unwrap();

        init(
            &dry,
            &InitOptions { yes: true, dry_run: true, ..InitOptions::default() },
        )
        .unwrap();

        assert!(!dry.join(".claude").exists(), "dry-run wrote nothing");
    }

    /// Regression guard for the `.claude/.claude/` nesting bug (the project root
    /// is never a `.claude` folder): the thin init — whose harness seeds are
    /// compiled-in constants, not directory copies — never creates it.
    #[test]
    fn init_does_not_create_nested_claude_dir() {
        let work = tempdir().unwrap();

        let project = work.path().join("project");
        fs::create_dir_all(&project).unwrap();

        init(
            &project,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        let nested = project.join(".claude").join(".claude");
        assert!(!nested.exists(), ".claude/.claude/ must not be created — I1 rule");
    }

    #[test]
    fn init_merge_rewrites_the_session_map_and_backfills() {
        let work = tempdir().unwrap();
        let project = work.path().join("project");
        let claude = project.join(".claude");
        // A diverged session map already present in .claude/mustard/.
        fs::create_dir_all(claude.join("mustard")).unwrap();
        fs::write(claude.join("mustard/session-map.md"), "USER EDIT").unwrap();

        // Non-interactive existing-dir path resolves to a merge.
        init(
            &project,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        // The session map is the harness's own text, so merge mode does not
        // reach it: the seed is laid down again whatever was there…
        assert_eq!(
            fs::read_to_string(claude.join("mustard/session-map.md")).unwrap(),
            mustard_core::session_map(mustard_core::platform::i18n::Locale::PtBr),
            "merge must still rewrite the session map — it is not project configuration"
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

    /// Uma versão anterior gravou uma decisão numa spec com a busca de outra
    /// regra, outra decisão sem busca nenhuma e uma lição no banco sem busca.
    /// A instalação põe o campo só onde ele falta: a busca de outra regra
    /// fica byte a byte, a linha sem busca ganha as palavras do título e do texto, e a
    /// instalação seguinte não tem mais o que pôr.
    #[test]
    fn an_install_fills_the_missing_search_of_the_specs_and_the_lessons_once() {
        use mustard_core::domain::spec_events::refresh_search_lines;
        use serde_json::{json, Map, Value};

        let work = tempdir().unwrap();
        let project = work.path().join("project");
        let paths = mustard_core::ClaudePaths::for_project(&project).unwrap();
        let object = |value: Value| -> Map<String, Value> { value.as_object().cloned().unwrap() };
        let at = "2026-09-11T10:00:00-03:00";
        let events = paths.spec_dir().join("teste").join("spec.ndjson");
        mustard_core::io::spec_events::write_at(
            &events,
            "message",
            object(json!({"author": "user", "text": "combine"})),
            &[],
            at,
        )
        .unwrap();
        let mut spec = fs::read_to_string(&events).unwrap();
        spec.push_str(
            "{\"v\":1,\"id\":2,\"at\":\"2026-09-11T10:01:00-03:00\",\"type\":\"decision\",\"author\":\"assistant\",\
             \"title\":\"Somar a fatura\",\"text\":\"A fatura soma centavos.\",\"keys\":[\"soma\"],\"origin\":1,\
             \"search\":\"fatur som centav\"}\n",
        );
        spec.push_str(
            "{\"v\":1,\"id\":3,\"at\":\"2026-09-11T10:02:00-03:00\",\"type\":\"decision\",\"author\":\"assistant\",\
             \"title\":\"Arredondar a fatura\",\"text\":\"A fatura arredonda centavos.\",\"keys\":[\"soma\"],\"origin\":1}\n",
        );
        fs::write(&events, &spec).unwrap();
        let bank = paths.lessons_path();
        let lesson = json!({"class": "defect", "text": "Apagar a pasta.", "keys": ["apagar"],
            "applies_to": {"subproject": "apps/rt"}, "found_in": {"spec": "teste"}});
        mustard_core::io::lessons::write_at(&bank, object(lesson), None, at).unwrap();
        let lessons = fs::read_to_string(&bank).unwrap();
        let (head, _) = lessons.trim_end().rsplit_once(",\"search\":").unwrap();
        fs::write(&bank, format!("{head}}}\n")).unwrap();
        let missing = |text: &str| refresh_search_lines(text).1;
        assert_eq!((missing(&spec), missing(&fs::read_to_string(&bank).unwrap())), (1, 1), "the fixture lacks two fields");

        let opts = InitOptions { yes: true, ..InitOptions::default() };
        assert_eq!(init(&project, &opts).unwrap(), InitOutcome::Installed);

        let fixed = fs::read_to_string(&events).unwrap();
        assert_eq!(missing(&fixed), 0, "the install left a line without search in the spec:\n{fixed}");
        let (before, after): (Vec<&str>, Vec<&str>) = (spec.lines().collect(), fixed.lines().collect());
        assert_eq!(after.len(), before.len(), "{fixed}");
        assert_eq!(after[0], before[0], "the line with today's search stays byte for byte");
        assert_eq!(after[1], before[1], "the search of another rule stays byte for byte");
        let parse = |line: &str| object(serde_json::from_str(line).unwrap());
        let (old, mut new) = (parse(before[2]), parse(after[2]));
        let search = new.remove("search").unwrap();
        let words: Vec<&str> = search.as_str().unwrap().split(' ').collect();
        assert!(words.contains(&"arredondar") && words.contains(&"arredonda"), "{search}");
        assert_eq!(old, new, "only the search field was added to the line without it");
        assert_eq!(fs::read_to_string(&bank).unwrap(), lessons, "the lesson got its search");

        assert_eq!(init(&project, &opts).unwrap(), InitOutcome::Installed);
        assert_eq!(fs::read_to_string(&events).unwrap(), fixed, "a second install has nothing left to fill");
    }

    /// Quando a busca não pode ser refeita — aqui, o índice das specs é uma
    /// pasta —, a instalação termina assim mesmo, e a saída traz um aviso com
    /// o comando que refaz a busca depois.
    #[test]
    fn an_install_whose_search_cannot_be_recomputed_warns_and_finishes() {
        let work = tempdir().unwrap();
        let project = work.path().join("project");
        let paths = mustard_core::ClaudePaths::for_project(&project).unwrap();
        fs::create_dir_all(paths.spec_index_path()).unwrap();

        let outcome = init(&project, &InitOptions { yes: true, ..InitOptions::default() });
        assert_eq!(outcome.unwrap(), InitOutcome::Installed, "a failed search must not abort the install");
        assert!(project.join(".claude").join("settings.local.json").exists(), "the seeding happened");

        let mut out = Vec::new();
        refresh_search(&project, &mut out);
        let out = String::from_utf8(out).unwrap();
        assert!(out.starts_with("  warning: ") && out.contains("mustard-rt run index"), "{out}");
    }

    /// A instalação sobre um projeto de uma versão antiga, que ainda tem o
    /// agente de onda de tarefa única, tira esse arquivo e deixa os três
    /// agentes de hoje; o agente do projeto com o mesmo nome, fora da pasta do
    /// Mustard, fica intocado.
    #[test]
    fn an_install_over_an_older_project_removes_the_retired_single_task_wave_agent() {
        let work = tempdir().unwrap();
        let project = work.path().join("project");
        let claude = project.join(".claude");
        fs::create_dir_all(claude.join("agents/mustard")).unwrap();
        fs::write(claude.join("agents/mustard/wave-solo.md"), "---\nname: mustard-wave-solo\n---\n").unwrap();
        let own = "---\nname: wave-solo\n---\n\nO agente do projeto.\n";
        fs::write(claude.join("agents/wave-solo.md"), own).unwrap();

        init(&project, &InitOptions { yes: true, ..InitOptions::default() }).unwrap();

        let mut agents: Vec<String> = fs::read_dir(claude.join("agents/mustard"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        agents.sort();
        assert_eq!(agents, ["review.md", "skill.md", "wave.md"], "the install left another set of agents");
        assert_eq!(fs::read_to_string(claude.join("agents/wave-solo.md")).unwrap(), own, "the project's agent changed");
    }

    /// Um orquestrador plantado por uma instalação antiga e as marcas do
    /// Mustard num `CLAUDE.md` são só listados: o instalador não tira nada, e
    /// o `.claude/CLAUDE.md` que não é do Mustard nem aparece.
    #[test]
    fn init_lists_what_an_older_mustard_left_and_removes_nothing() {
        let work = tempdir().unwrap();
        let project = work.path().join("project");
        let claude = project.join(".claude");
        fs::create_dir_all(&claude).unwrap();
        fs::write(claude.join("CLAUDE.md"), "# Orchestrator Rules\n\nYou are the router.\n").unwrap();
        let root_md = "@.claude/scan-map.md\n\n# (root)\n\n## Guards\n\n<!-- mustard:guards -->\n- keep this guard\n<!-- /mustard:guards -->\n";
        fs::write(project.join("CLAUDE.md"), root_md).unwrap();

        init(
            &project,
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
        let project = work.path().join("project");
        fs::create_dir_all(&project).unwrap();
        // The user already curated their own inject list.
        fs::write(
            project.join("mustard.json"),
            r#"{"inject":[{"on":"sessionStart","file":"docs/my-rules.md","once":false}]}"#,
        )
        .unwrap();

        init(
            &project,
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
        // `work` is a git repository root; the init target is a subdirectory.
        fs::create_dir_all(work.path().join(".git")).unwrap();
        let project = work.path().join("apps").join("dashboard");
        fs::create_dir_all(&project).unwrap();

        let err = init(
            &project,
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
        let project = work.path().join("project");
        fs::create_dir_all(project.join(".git")).unwrap(); // project IS a repo root

        init(
            &project,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        assert!(project.join(".claude").join("settings.local.json").exists());
        assert!(project.join("mustard.json").exists());
    }

    #[test]
    fn init_allows_at_submodule_root_with_git_file() {
        let work = tempdir().unwrap();
        // Outer repository root…
        fs::create_dir_all(work.path().join(".git")).unwrap();
        // …and a submodule below it: `.git` is a FILE with a `gitdir:` pointer.
        let sub = work.path().join("backend").join("service");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join(".git"), "gitdir: ../../.git/modules/service\n").unwrap();

        init(
            &sub,
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
        let project = work.path().join("plain");
        fs::create_dir_all(&project).unwrap(); // no .git anywhere up the tempdir

        init(
            &project,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        assert!(project.join(".claude").join("settings.local.json").exists());
    }
}
