//! Git-flow + locale configuration for the project-root `mustard.json`.
//!
//! Probes the repository (default branch, current branch, remote branches,
//! submodules), collects the user's choices (production / dev branch, provider,
//! **spec language**, **tone**), detects the build/test/lint/type-check command
//! set agnostically (no hardcoded `npm`), and folds all of it into the single
//! [`ProjectConfig`] written at the project root. There is no private config
//! struct here any more — the one schema lives in `mustard_core`.
//!
//! Two entry points:
//! - [`configure`] — the `mustard config` command: load → (preserve | collect)
//!   → write.
//! - [`collect_choices`] + [`apply_choices`] — the building blocks `init` uses
//!   so it can fold the same git-flow/locale data into the config it stamps with
//!   `runtime`/`version`, keeping a single write.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use dialoguer::theme::ColorfulTheme;
use dialoguer::{Input, Select};
use mustard_core::{detect_commands, GitConfig, ProjectConfig, SupportedLocale, Tone};

/// Facts probed from the repository, all fail-open.
pub struct GitFacts {
    default_branch: String,
    current_branch: Option<String>,
    has_submodules: bool,
    remote_branches: Vec<String>,
}

impl GitFacts {
    /// Name of the detected shared dev branch (`dev` or `develop`), if any.
    fn dev_branch(&self) -> Option<&'static str> {
        if self.remote_branches.iter().any(|b| b == "dev") {
            Some("dev")
        } else if self.remote_branches.iter().any(|b| b == "develop") {
            Some("develop")
        } else {
            None
        }
    }
}

/// The user's git-flow + locale choices, resolved either from prompts or from
/// sensible defaults (`--yes` / non-TTY).
pub struct Choices {
    production: String,
    dev_branch: String,
    provider: String,
    spec_lang: String,
    tone: String,
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

    let remote_branches = git(project_path, &["branch", "-r", "--format=%(refname:short)"])
        .map(|out| {
            out.lines()
                .filter(|l| !l.is_empty())
                .map(|l| l.trim().trim_start_matches("origin/").to_string())
                .collect()
        })
        .unwrap_or_default();

    GitFacts { default_branch, current_branch, has_submodules, remote_branches }
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

/// Collect the git-flow + locale choices, pre-filling defaults from `existing`.
///
/// Interactive (and a real TTY) prompts the user; otherwise it derives sensible
/// defaults — preserving any values already present in `existing`.
///
/// # Errors
/// Propagates a prompt read failure.
pub fn collect_choices(
    facts: &GitFacts,
    existing: &ProjectConfig,
    interactive: bool,
) -> Result<Choices> {
    let i18n = existing.i18n();
    let existing_lang = i18n.lang.as_str().to_string();
    let existing_tone = i18n.tone.as_str().to_string();
    let existing_provider =
        if existing.git.provider.is_empty() { "github".to_string() } else { existing.git.provider.clone() };
    let existing_dev = existing.git.flow.get("*").cloned();
    let existing_prod =
        existing_dev.as_ref().and_then(|d| existing.git.flow.get(d).cloned());

    if !(interactive && console_is_tty()) {
        return Ok(Choices {
            production: existing_prod.unwrap_or_else(|| facts.default_branch.clone()),
            dev_branch: existing_dev
                .or_else(|| facts.dev_branch().map(String::from))
                .unwrap_or_default(),
            provider: existing_provider,
            spec_lang: existing_lang,
            tone: existing_tone,
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

    let production: String = Input::with_theme(&theme)
        .with_prompt("Production branch")
        .default(existing_prod.unwrap_or_else(|| facts.default_branch.clone()))
        .interact_text()
        .context("reading production branch")?;

    let dev_default = existing_dev
        .or_else(|| facts.dev_branch().map(String::from))
        .unwrap_or_default();
    let dev_branch: String = Input::with_theme(&theme)
        .with_prompt("Development branch (shared, leave empty to skip)")
        .allow_empty(true)
        .default(dev_default)
        .interact_text()
        .context("reading development branch")?;

    let providers = ["github", "gitlab", "bitbucket"];
    let provider_idx = Select::with_theme(&theme)
        .with_prompt("Git provider")
        .items(providers)
        .default(providers.iter().position(|p| *p == existing_provider).unwrap_or(0))
        .interact()
        .context("reading git provider")?;

    let langs = ["pt-BR", "en-US"];
    let lang_idx = Select::with_theme(&theme)
        .with_prompt("Spec language (user-facing specs, waves and banners)")
        .items(langs)
        .default(langs.iter().position(|l| *l == existing_lang).unwrap_or(0))
        .interact()
        .context("reading spec language")?;

    let tones = ["didactic", "technical", "concise"];
    let tone_idx = Select::with_theme(&theme)
        .with_prompt("Tone (user-facing output)")
        .items(tones)
        .default(tones.iter().position(|t| *t == existing_tone).unwrap_or(0))
        .interact()
        .context("reading tone")?;

    Ok(Choices {
        production,
        dev_branch,
        provider: providers[provider_idx].to_string(),
        spec_lang: langs[lang_idx].to_string(),
        tone: tones[tone_idx].to_string(),
    })
}

/// Fold `choices` + detected commands into `config`.
///
/// Git flow, provider, language and tone come from `choices` (a prompt or a
/// default). The command set is detected agnostically from the project's
/// manifests, but **never overwrites** a command the user already set — only
/// absent fields are filled.
///
/// Takes no [`GitFacts`]: probed facts inform the PROMPT (they seed defaults and
/// are shown to the user), never the written config. What lands in
/// `mustard.json` is what the project decided — see [`GitConfig`].
pub fn apply_choices(config: &mut ProjectConfig, choices: &Choices, root: &Path) {
    config.git = GitConfig {
        flow: build_flow(choices.dev_branch.trim(), choices.production.trim()),
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

    // Canonicalise language/tone to the catalogue spelling.
    config.spec_lang = Some(
        choices.spec_lang.parse::<SupportedLocale>().unwrap_or_default().as_str().to_string(),
    );
    config.tone = Some(Tone::parse(&choices.tone).unwrap_or_default().as_str().to_string());
}

/// Run `mustard config` against `project_path`: (re)configure git flow + locale
/// in `<root>/mustard.json`.
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

    fn facts(dev: Option<&str>, submodules: bool) -> GitFacts {
        GitFacts {
            default_branch: "main".to_string(),
            current_branch: None,
            has_submodules: submodules,
            remote_branches: dev.map(|d| vec![d.to_string()]).unwrap_or_default(),
        }
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
    fn apply_choices_fills_git_lang_tone_and_detects_commands() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        let mut config = ProjectConfig::default();
        let f = facts(Some("dev"), true);
        let choices = Choices {
            production: "main".into(),
            dev_branch: "dev".into(),
            provider: "gitlab".into(),
            spec_lang: "en-US".into(),
            tone: "technical".into(),
        };
        apply_choices(&mut config, &choices, dir.path());

        assert_eq!(config.git.provider, "gitlab");
        assert_eq!(config.git.flow.get("*"), Some(&"dev".to_string()));
        // Cargo project → cargo build, never npm.
        assert_eq!(config.build_command.as_deref(), Some("cargo build"));
        assert_eq!(config.spec_lang.as_deref(), Some("en-US"));
        assert_eq!(config.tone.as_deref(), Some("technical"));
    }

    #[test]
    fn apply_choices_preserves_existing_commands() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        let mut config = ProjectConfig::default();
        config.build_command = Some("custom build".into());
        let f = facts(None, false);
        let choices = Choices {
            production: "main".into(),
            dev_branch: String::new(),
            provider: "github".into(),
            spec_lang: "pt-BR".into(),
            tone: "didactic".into(),
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
        assert_eq!(cfg.git.provider, "github");
        assert_eq!(cfg.build_command.as_deref(), Some("cargo build"));
        assert_eq!(cfg.spec_lang.as_deref(), Some("pt-BR"));
        assert_eq!(cfg.tone.as_deref(), Some("didactic"));
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
