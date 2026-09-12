//! Git-flow + language configuration for the project-root `mustard.json`.
//!
//! Probes the repository (default branch, current branch, submodules),
//! collects the user's choices (the **text language** and the **code
//! language**), detects the build/test/lint/type-check command set
//! agnostically (no hardcoded `npm`), and folds all of it into the single
//! [`ProjectConfig`] written at the project root. There is no private config
//! struct here any more — the one schema lives in `mustard_core`.
//!
//! A language is written only when the operator chose it. Outside the
//! interactive mode nothing is asked, so nothing is written: a default written
//! for them would read, later, exactly like a choice.
//!
//! Two entry points:
//! - [`configure`] — the `mustard config` command: load → (preserve | collect)
//!   → write.
//! - [`collect_choices`] + [`apply_choices`] — the building blocks `init` uses
//!   so it can fold the same git-flow/language data into the config it stamps
//!   with `runtime`/`version`, keeping a single write.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use dialoguer::theme::ColorfulTheme;
use dialoguer::Select;
use mustard_core::{detect_commands, GitConfig, ProjectConfig, SupportedLocale};

/// Facts probed from the repository, all fail-open.
///
/// It used to carry the remote branch list too, read by a `dev_branch()` helper
/// that guessed `dev`/`develop` for the branch prompts. Those prompts are gone
/// (see [`collect_choices`]) — nothing asks which branches the project promotes
/// through any more — so both the guess and the `git branch -r` call that fed it
/// went with them.
pub struct GitFacts {
    default_branch: String,
    current_branch: Option<String>,
    has_submodules: bool,
}

/// The user's git-flow + language choices, resolved either from prompts or,
/// outside the interactive mode (`--yes` / non-TTY), from what the project
/// already declared.
pub struct Choices {
    production: String,
    dev_branch: String,
    provider: String,
    /// The text language the operator picked; `None` when nothing was asked.
    text_language: Option<String>,
    /// The code language the operator picked; `None` when nothing was asked.
    code_language: Option<String>,
}

/// Run a `git` subcommand in `cwd`, returning trimmed stdout on success.
/// Any failure — `git` missing, non-zero exit, not a repository — yields `None`.
fn git(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).current_dir(cwd).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Probe the repository at `project_path`.
#[must_use]
pub fn probe_git(project_path: &Path) -> GitFacts {
    let default_branch = git(project_path, &["symbolic-ref", "refs/remotes/origin/HEAD"])
        .map(|r| r.replace("refs/remotes/origin/", ""))
        .or_else(|| {
            let branches = git(project_path, &["branch", "-r"]).unwrap_or_default();
            if branches.contains("origin/main") {
                Some("main".to_string())
            } else if branches.contains("origin/master") {
                Some("master".to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "main".to_string());

    let current_branch =
        git(project_path, &["rev-parse", "--abbrev-ref", "HEAD"]).filter(|s| !s.is_empty());

    let has_submodules = project_path.join(".gitmodules").exists();

    GitFacts { default_branch, current_branch, has_submodules }
}

/// Build the branch-promotion flow map. An empty dev branch yields an empty map.
fn build_flow(dev_branch: &str, production: &str) -> std::collections::BTreeMap<String, String> {
    let mut flow = std::collections::BTreeMap::new();
    if !dev_branch.is_empty() {
        flow.insert("*".to_string(), dev_branch.to_string());
        flow.insert(dev_branch.to_string(), production.to_string());
    }
    flow
}

/// Whether stdin is an interactive terminal.
fn console_is_tty() -> bool {
    std::io::IsTerminal::is_terminal(&std::io::stdin())
}

/// Collect the git-flow + language choices, pre-filling the prompts from
/// `existing`.
///
/// Interactive (and a real TTY) prompts the user; otherwise nothing is asked,
/// and the choices only carry forward what `existing` already declared — no
/// language among them, so none is written.
///
/// # Errors
/// Propagates a prompt read failure.
pub fn collect_choices(
    facts: &GitFacts,
    existing: &ProjectConfig,
    interactive: bool,
) -> Result<Choices> {
    // Carried forward VERBATIM, empty included. It used to coerce empty to
    // "github", which is precisely what would defeat detection: an install
    // would write the override on every run and the remote would never be
    // consulted again.
    let existing_provider = existing.git.provider.clone();
    let existing_dev = existing.git.flow.get("*").cloned();
    let existing_prod =
        existing_dev.as_ref().and_then(|d| existing.git.flow.get(d).cloned());

    if !(interactive && console_is_tty()) {
        // Preserve what the project already declared; invent nothing. The old
        // code fell back to the probed default branch and to a `dev`/`develop`
        // guess, which is how a fresh install acquired a flow nobody asked for.
        // The language went the same way — `pt-BR` written for every project
        // that never answered — and goes the same way now: nothing is asked,
        // so no language is chosen, and the project keeps the one it declared.
        return Ok(Choices {
            production: existing_prod.unwrap_or_default(),
            dev_branch: existing_dev.unwrap_or_default(),
            provider: existing_provider,
            text_language: None,
            code_language: None,
        });
    }

    let theme = ColorfulTheme::default();
    println!("\nGit Flow Configuration\n");
    if let Some(branch) = &facts.current_branch {
        println!(
            "  Detected: branch={branch}, default={}, submodules={}",
            facts.default_branch, facts.has_submodules
        );
    }

    // The two branch prompts are GONE. They asked the operator to declare, at
    // install time, which branches the project promotes through — and that
    // answer then decided both where a unit could be cut from and where a
    // direct commit was refused, for the whole life of the install. In a client
    // repository the answer was wrong within a week: branches appear, release
    // lines are cut, and nobody re-runs `mustard init` to tell us.
    //
    // Both questions are asked of git now, at the moment they matter:
    // `run base-candidates` lists what really exists when a unit opens, and
    // `protected_branches` reads the remote's own default branch. Nothing here
    // needs an answer any more, so nothing here asks for one.
    //
    // `existing_prod` / `existing_dev` are still READ above: a project that
    // already declared a flow keeps it, because it still pre-selects a row in
    // the picker. What stops is CREATING one.
    let production = existing_prod.unwrap_or_default();
    let dev_branch = existing_dev.unwrap_or_default();

    // The provider menu is GONE, for the reason the branch prompts went: the
    // answer is written in the `origin` remote, and freezing it at install time
    // made every project carry a decision taken before anyone knew the project.
    // It is detected now (`mustard_core::resolve_provider`), and what survives
    // in the config is an override for the self-hosted case — which is why an
    // EXISTING declaration is carried forward untouched here.
    let provider = existing_provider;

    // The two languages, each on its own key: the one people read and the one
    // the names in the code are written in. A language the project already
    // declared pre-selects its row.
    let declared = existing.language();
    let texts = ["pt-BR", "en-US"];
    let text_idx = Select::with_theme(&theme)
        .with_prompt("Text language (conversation, specs, pages, code comments, commit messages)")
        .items(texts)
        .default(declared.text.and_then(|l| texts.iter().position(|t| *t == l.as_str())).unwrap_or(0))
        .interact()
        .context("reading the text language")?;

    let codes = ["en", "pt"];
    let code_idx = Select::with_theme(&theme)
        .with_prompt("Code language (names of variables, functions, files, commands)")
        .items(codes)
        .default(declared.code.as_deref().and_then(|c| codes.iter().position(|x| *x == c)).unwrap_or(0))
        .interact()
        .context("reading the code language")?;

    Ok(Choices {
        production,
        dev_branch,
        provider,
        text_language: Some(texts[text_idx].to_string()),
        code_language: Some(codes[code_idx].to_string()),
    })
}

/// Fold `choices` + detected commands into `config`.
///
/// Git flow and provider come from `choices` (a prompt or what the project
/// declared); a language comes from them only when the operator chose one, and
/// is otherwise left as the project declared it. The command set is detected agnostically from the project's
/// manifests, but **never overwrites** a command the user already set — only
/// absent fields are filled.
///
/// Takes no [`GitFacts`]: probed facts inform the PROMPT (they seed defaults and
/// are shown to the user), never the written config. What lands in
/// `mustard.json` is what the project decided — see [`GitConfig`].
pub fn apply_choices(config: &mut ProjectConfig, choices: &Choices, root: &Path) {
    config.git = GitConfig {
        flow: build_flow(choices.dev_branch.trim(), choices.production.trim()),
        // `protected` is NOT written by an install: the remote's default branch
        // is protected by probe, and this list is the escape hatch a team fills
        // in by hand. Seeding it would re-create the stale declaration the
        // probe replaced.
        //
        // Every field is named rather than filled by `..Default::default()`.
        // The struct-update shorthand silently dropped `provider` here when the
        // `protected` field was added — the operator's answer was discarded and
        // nothing complained until a test asked. A field added later should
        // break the build, not the behaviour.
        protected: Vec::new(),
        provider: choices.provider.clone(),
    };

    let cmds = detect_commands(root);
    if config.build_command.is_none() {
        config.build_command = cmds.build;
    }
    if config.test_command.is_none() {
        config.test_command = cmds.test;
    }
    if config.lint_command.is_none() {
        config.lint_command = cmds.lint;
    }
    if config.type_check_command.is_none() {
        config.type_check_command = cmds.type_check;
    }

    // Only a language the operator chose is written, in the catalogue
    // spelling. Nothing is asked outside the interactive mode, and then the
    // project keeps what it declared — or stays without a language, which is
    // never supposed for it.
    if let Some(text) = choices.text_language.as_deref().and_then(|t| t.parse::<SupportedLocale>().ok()) {
        config.language.text = Some(text.as_str().to_string());
    }
    if let Some(code) = choices.code_language.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
        config.language.code = Some(code.to_string());
    }
}

/// Run `mustard config` against `project_path`: (re)configure git flow and
/// the languages in `<root>/mustard.json`.
///
/// Non-interactive over an existing file preserves it verbatim; otherwise the
/// choices are collected (prompt or default) and folded in.
///
/// # Errors
/// Propagates prompt-read and write failures.
pub fn configure(project_path: &Path, interactive: bool) -> Result<()> {
    let mut config = ProjectConfig::load(project_path);

    if !interactive && ProjectConfig::exists(project_path) {
        println!("  mustard.json already exists - preserved");
        return Ok(());
    }

    let facts = probe_git(project_path);
    let choices = collect_choices(&facts, &config, interactive)?;
    apply_choices(&mut config, &choices, project_path);
    config.write(project_path)?;
    println!("  created mustard.json");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn facts(submodules: bool) -> GitFacts {
        GitFacts {
            default_branch: "main".to_string(),
            current_branch: None,
            has_submodules: submodules,
        }
    }

    /// The install stops asking which branches the project promotes through,
    /// and stops writing an answer nobody gave.
    ///
    /// Both halves are asserted, because either one alone would pass while the
    /// feature was half-done: a run that asks nothing but still seeds a flow
    /// from probed facts leaves the same stale declaration behind, and that
    /// declaration is what used to refuse real branches.
    #[test]
    fn init_does_not_ask_for_branches() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();

        // A repository whose probe supplies a default branch the old code would
        // have seeded the flow from. It is not used. (The remote branch list the
        // `dev`/`develop` guess read is gone from `GitFacts` altogether — no
        // fixture can hand it over any more.)
        let probed = facts(false);
        let fresh = ProjectConfig::default();
        let choices = collect_choices(&probed, &fresh, true).expect("no prompt to fail");
        assert!(
            choices.production.is_empty() && choices.dev_branch.is_empty(),
            "nothing was asked, so nothing is answered: production={:?} dev={:?}",
            choices.production,
            choices.dev_branch,
        );

        // …and what lands on disk carries no `git.flow` key at all — not an
        // empty object, no key, so a reader cannot mistake it for a decision.
        let mut config = fresh;
        apply_choices(&mut config, &choices, root);
        assert!(config.git.flow.is_empty(), "no flow was created");
        let written = serde_json::to_string(&config).expect("serialises");
        assert!(
            !written.contains("\"flow\""),
            "an empty flow is written as NO key: {written}",
        );

        // A project that already declared one keeps it — the install stopped
        // creating flows, it did not start deleting them.
        let mut existing = ProjectConfig::default();
        existing.git.flow.insert("*".to_string(), "trunk".to_string());
        let kept = collect_choices(&probed, &existing, true).expect("no prompt to fail");
        assert_eq!(kept.dev_branch, "trunk", "the declared base survives untouched");
    }

    /// The install stops asking who hosts the repository, and stops writing an
    /// answer nobody gave.
    ///
    /// Both halves: an install that asks nothing but still writes `"github"`
    /// leaves a permanent override behind, and the remote would never be
    /// consulted again.
    #[test]
    fn init_does_not_ask_for_the_provider() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();

        let probed = facts(false);
        let choices = collect_choices(&probed, &ProjectConfig::default(), true)
            .expect("no prompt to fail");
        assert!(
            choices.provider.is_empty(),
            "nothing was asked, so nothing is answered: {:?}",
            choices.provider,
        );

        let mut config = ProjectConfig::default();
        apply_choices(&mut config, &choices, root);
        assert!(config.git.provider.is_empty(), "no override was created");
        let written = serde_json::to_string(&config).expect("serialises");
        assert!(
            !written.contains("\"provider\""),
            "an empty provider is written as NO key: {written}",
        );

        // An existing override survives — the install stopped creating them, it
        // did not start deleting them. This is the self-hosted case.
        let mut declared = ProjectConfig::default();
        declared.git.provider = "gitlab".to_string();
        let kept = collect_choices(&probed, &declared, true).expect("no prompt to fail");
        assert_eq!(kept.provider, "gitlab", "the declared override is untouched");
    }

    #[test]
    fn build_flow_empty_dev_yields_empty_map() {
        assert!(build_flow("", "main").is_empty());
    }

    #[test]
    fn build_flow_links_dev_to_production() {
        let flow = build_flow("dev", "main");
        assert_eq!(flow.get("*"), Some(&"dev".to_string()));
        assert_eq!(flow.get("dev"), Some(&"main".to_string()));
    }

    #[test]
    fn apply_choices_fills_git_language_and_detects_commands() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        let mut config = ProjectConfig::default();
        let choices = Choices {
            production: "main".into(),
            dev_branch: "dev".into(),
            provider: "gitlab".into(),
            text_language: Some("en-US".into()),
            code_language: Some("en".into()),
        };
        apply_choices(&mut config, &choices, dir.path());

        assert_eq!(config.git.provider, "gitlab");
        assert_eq!(config.git.flow.get("*"), Some(&"dev".to_string()));
        // Cargo project → cargo build, never npm.
        assert_eq!(config.build_command.as_deref(), Some("cargo build"));
        assert_eq!(config.language().text, Some(SupportedLocale::EnUs));
        assert_eq!(config.language().code.as_deref(), Some("en"));
        let written = serde_json::to_string(&config).expect("serialises");
        assert!(written.contains(r#""language":{"text":"en-US","code":"en"}"#), "{written}");
        assert!(!written.contains("specLang") && !written.contains("tone"), "{written}");
    }

    /// Fora do modo interativo nada é perguntado, e nenhum idioma é gravado
    /// por padrão: nem num projeto novo, nem num projeto que só tem a chave
    /// antiga de idioma, que continua no arquivo sem ser lida. O idioma que o
    /// projeto já declarou fica como está.
    #[test]
    fn a_non_interactive_install_writes_no_language_by_default() {
        let probed = facts(false);

        // A project that only carries the old language key.
        let old = tempdir().unwrap();
        std::fs::write(old.path().join("mustard.json"), r#"{"specLang":"pt-BR"}"#).unwrap();
        let mut config = ProjectConfig::load(old.path());
        let choices = collect_choices(&probed, &config, false).expect("nothing is asked");
        assert!(choices.text_language.is_none() && choices.code_language.is_none());
        apply_choices(&mut config, &choices, old.path());
        config.write(old.path()).unwrap();
        let raw = std::fs::read_to_string(old.path().join("mustard.json")).unwrap();
        assert!(!raw.contains("\"language\""), "no language is written by default: {raw}");
        assert_eq!(
            ProjectConfig::load(old.path()).language(),
            mustard_core::Language::default(),
            "the old key declares nothing",
        );

        // A fresh project.
        let fresh = tempdir().unwrap();
        configure(fresh.path(), false).unwrap();
        let raw = std::fs::read_to_string(fresh.path().join("mustard.json")).unwrap();
        for key in ["\"language\"", "\"specLang\"", "\"tone\""] {
            assert!(!raw.contains(key), "a fresh install writes no {key}: {raw}");
        }

        // A language the project declared survives a non-interactive run.
        let declared = tempdir().unwrap();
        std::fs::write(
            declared.path().join("mustard.json"),
            r#"{"language":{"text":"en-US","code":"en"}}"#,
        )
        .unwrap();
        let mut config = ProjectConfig::load(declared.path());
        let choices = collect_choices(&probed, &config, false).expect("nothing is asked");
        apply_choices(&mut config, &choices, declared.path());
        assert_eq!(config.language().text, Some(SupportedLocale::EnUs));
        assert_eq!(config.language().code.as_deref(), Some("en"));
    }

    #[test]
    fn apply_choices_preserves_existing_commands() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        let mut config = ProjectConfig {
            build_command: Some("custom build".into()),
            ..Default::default()
        };
        let choices = Choices {
            production: "main".into(),
            dev_branch: String::new(),
            provider: "github".into(),
            text_language: None,
            code_language: None,
        };
        apply_choices(&mut config, &choices, dir.path());
        // User's command survives; detection does not clobber it.
        assert_eq!(config.build_command.as_deref(), Some("custom build"));
    }

    #[test]
    fn configure_writes_default_config_in_clean_dir() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        // Non-interactive, fresh dir → derived defaults written.
        configure(dir.path(), false).unwrap();
        let cfg = ProjectConfig::load(dir.path());
        // The provider is NOT among the derived defaults any more: it is
        // detected from the remote when it is needed, and what a fresh install
        // writes is nothing. The commands below still ARE derived — they come
        // from probing the project, not from asking.
        assert_eq!(cfg.git.provider, "", "a fresh install writes no override");
        assert_eq!(cfg.build_command.as_deref(), Some("cargo build"));
        // Nor is a language: nothing was asked, so nothing was chosen.
        assert_eq!(cfg.language(), mustard_core::Language::default());
    }

    #[test]
    fn configure_preserves_existing_file_when_non_interactive() {
        let dir = tempdir().unwrap();
        // The `submodules` key is deliberately still here: it was dropped from
        // `GitConfig` (written by init, read by nobody, stale the moment a
        // submodule is added), and every mustard.json in the wild still carries
        // it. Loading must ignore the unknown key, not choke on it.
        std::fs::write(
            dir.path().join("mustard.json"),
            r#"{"git":{"flow":{},"provider":"gitlab","submodules":false}}"#,
        )
        .unwrap();
        configure(dir.path(), false).unwrap();
        let cfg = ProjectConfig::load(dir.path());
        assert_eq!(cfg.git.provider, "gitlab");
    }
}
