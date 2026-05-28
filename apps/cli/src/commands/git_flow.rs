//! Git-flow configuration — the project-root `mustard.json`.
//!
//! Ported from `generateMustardJson` in `init.ts`. The routine inspects the
//! repository (default branch, current branch, remote branches, submodules)
//! and writes a `mustard.json` describing the branch promotion flow plus the
//! optional build/test/lint/type-check commands the close-gate reads.
//!
//! Two modes:
//!
//! - **non-interactive** (`yes`): derive a sensible config and write it,
//!   preserving an existing file untouched;
//! - **interactive**: show what was detected, prompt for the production and
//!   development branches and the git provider (pre-filled from any existing
//!   config), then write.
//!
//! Git facts are gathered by shelling out to `git` (the JS port used
//! `execSync`); every probe fails open to a default when `git` is absent or
//! the directory is not a repository.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use mustard_core::io::fs as mfs;
use dialoguer::theme::ColorfulTheme;
use dialoguer::{Confirm, Input, Select};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::fs_ops::read_json_object;

/// The `git` block of `mustard.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitConfig {
    /// Branch promotion map: `"*" → dev`, `dev → production`.
    pub flow: std::collections::BTreeMap<String, String>,
    /// Hosting provider — `github`, `gitlab`, or `bitbucket`.
    pub provider: String,
    /// Whether the repository uses git submodules.
    pub submodules: bool,
}

/// The full `mustard.json` document written at the project root.
///
/// The four `*_command` fields feed the Wave 9 close-gate strict gates. They
/// are optional — an absent command means "skip that stage".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MustardConfig {
    pub git: GitConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lint_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_check_command: Option<String>,
}

/// Facts probed from the repository, all fail-open.
struct GitFacts {
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

/// Run a `git` subcommand in `cwd`, returning trimmed stdout on success.
///
/// Any failure — `git` missing, non-zero exit, not a repository — yields
/// `None`. This is the fail-open `try { execSync } catch {}` of the JS port.
fn git(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).current_dir(cwd).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Probe the repository at `project_path`. Mirrors the four `detect*`
/// helpers in `init.ts`.
fn probe_git(project_path: &Path) -> GitFacts {
    // Default branch: the remote HEAD symbolic ref, else main/master if either
    // remote branch exists, else "main".
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

    let current_branch = git(project_path, &["rev-parse", "--abbrev-ref", "HEAD"])
        .filter(|s| !s.is_empty());

    let has_submodules = project_path.join(".gitmodules").exists();

    let remote_branches = git(project_path, &["branch", "-r", "--format=%(refname:short)"])
        .map(|out| {
            out.lines()
                .filter(|l| !l.is_empty())
                .map(|l| l.trim().trim_start_matches("origin/").to_string())
                .collect()
        })
        .unwrap_or_default();

    GitFacts {
        default_branch,
        current_branch,
        has_submodules,
        remote_branches,
    }
}

/// Build the flow map from a dev branch and production branch. An empty dev
/// branch yields an empty map (no shared dev branch).
fn build_flow(
    dev_branch: &str,
    production: &str,
) -> std::collections::BTreeMap<String, String> {
    let mut flow = std::collections::BTreeMap::new();
    if !dev_branch.is_empty() {
        flow.insert("*".to_string(), dev_branch.to_string());
        flow.insert(dev_branch.to_string(), production.to_string());
    }
    flow
}

/// The default close-gate command set written into every fresh `mustard.json`.
fn default_commands() -> MustardConfig {
    MustardConfig {
        git: GitConfig {
            flow: std::collections::BTreeMap::new(),
            provider: "github".to_string(),
            submodules: false,
        },
        test_command: Some("npm test".to_string()),
        build_command: Some("npm run build".to_string()),
        lint_command: Some("npm run lint".to_string()),
        type_check_command: Some("tsc --noEmit".to_string()),
    }
}

/// Generate (or reconfigure) the project-root `mustard.json`.
///
/// `interactive` mirrors the JS `!options.yes` path. When `false`, an existing
/// file is preserved verbatim and a missing one is derived from `git` probes.
/// When `true`, the user is prompted (defaults pre-filled from any existing
/// config) — unless stdin is not a TTY, in which case it falls back to the
/// non-interactive derivation so scripted/test runs never block.
pub fn generate_mustard_json(project_path: &Path, interactive: bool) -> Result<()> {
    let config_path = project_path.join("mustard.json");
    let existing = load_existing(&config_path);

    // Non-interactive with an existing file: preserve it untouched.
    if !interactive && existing.is_some() {
        println!("  mustard.json already exists - preserved");
        return Ok(());
    }

    let facts = probe_git(project_path);

    let config = if interactive && console_is_tty() {
        prompt_config(&facts, existing.as_ref())?
    } else {
        derive_config(&facts)
    };

    let mut serialized = serde_json::to_string_pretty(&config).context("serializing mustard.json")?;
    serialized.push('\n');
    mfs::write_atomic(&config_path, serialized.as_bytes())
        .with_context(|| format!("writing {}", config_path.display()))?;
    println!("  created mustard.json");
    Ok(())
}

/// Load and parse an existing `mustard.json`; `None` if absent or malformed.
fn load_existing(path: &Path) -> Option<MustardConfig> {
    let map = read_json_object(path);
    if map.is_empty() {
        return None;
    }
    serde_json::from_value(Value::Object(map)).ok()
}

/// Whether stdin is an interactive terminal. Prompts are skipped when it is
/// not so non-interactive runs (CI, tests, the Tauri backend) never hang.
fn console_is_tty() -> bool {
    std::io::IsTerminal::is_terminal(&std::io::stdin())
}

/// Derive a config from git facts with no prompting (the `yes` path).
fn derive_config(facts: &GitFacts) -> MustardConfig {
    let mut config = default_commands();
    config.git.submodules = facts.has_submodules;
    if let Some(dev) = facts.dev_branch() {
        config.git.flow = build_flow(dev, &facts.default_branch);
    }
    config
}

/// Prompt the user for the git flow, pre-filling defaults from `existing`.
fn prompt_config(facts: &GitFacts, existing: Option<&MustardConfig>) -> Result<MustardConfig> {
    let theme = ColorfulTheme::default();

    let existing_dev = existing.and_then(|c| c.git.flow.get("*").cloned());
    let existing_prod = existing_dev
        .as_ref()
        .and_then(|dev| existing.and_then(|c| c.git.flow.get(dev).cloned()));
    let existing_provider = existing
        .map_or_else(|| "github".to_string(), |c| c.git.provider.clone());

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
    let provider_default = providers
        .iter()
        .position(|p| *p == existing_provider)
        .unwrap_or(0);
    let provider_idx = Select::with_theme(&theme)
        .with_prompt("Git provider")
        .items(providers)
        .default(provider_default)
        .interact()
        .context("reading git provider")?;

    let mut config = default_commands();
    config.git.submodules = facts.has_submodules;
    config.git.provider = providers[provider_idx].to_string();
    config.git.flow = build_flow(dev_branch.trim(), production.trim());
    Ok(config)
}

/// Interactively review and optionally reconfigure an existing config — the
/// JS "Reconfigure git flow?" confirm. Returns `true` when the user opted to
/// reconfigure. Used by the Wave 2 `config` subcommand; kept here next to the
/// flow logic it gates.
pub fn confirm_reconfigure(existing: &MustardConfig) -> Result<bool> {
    println!("\n  Current git flow:");
    if existing.git.flow.is_empty() {
        println!("    (no flow configured)");
    } else {
        for (from, to) in &existing.git.flow {
            println!("    {from} -> {to}");
        }
    }
    println!("    provider: {}", existing.git.provider);
    println!("    submodules: {}", existing.git.submodules);

    if !console_is_tty() {
        return Ok(false);
    }
    Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Reconfigure git flow?")
        .default(false)
        .interact()
        .context("reading reconfigure confirmation")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

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
    fn derive_config_carries_submodule_flag() {
        let facts = GitFacts {
            default_branch: "main".to_string(),
            current_branch: None,
            has_submodules: true,
            remote_branches: vec!["dev".to_string()],
        };
        let config = derive_config(&facts);
        assert!(config.git.submodules);
        assert_eq!(config.git.flow.get("*"), Some(&"dev".to_string()));
    }

    #[test]
    fn generate_writes_default_config_in_clean_dir() {
        let dir = tempdir().unwrap();
        // Non-interactive: clean dir, no git -> derived defaults.
        generate_mustard_json(dir.path(), false).unwrap();
        let written = std::fs::read_to_string(dir.path().join("mustard.json")).unwrap();
        let parsed: MustardConfig = serde_json::from_str(&written).unwrap();
        assert_eq!(parsed.git.provider, "github");
        assert_eq!(parsed.build_command.as_deref(), Some("npm run build"));
    }

    #[test]
    fn generate_preserves_existing_file_when_non_interactive() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("mustard.json");
        std::fs::write(&path, r#"{"git":{"flow":{},"provider":"gitlab","submodules":false}}"#)
            .unwrap();
        generate_mustard_json(dir.path(), false).unwrap();
        let parsed: MustardConfig =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(parsed.git.provider, "gitlab");
    }
}
