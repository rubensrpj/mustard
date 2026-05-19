//! `mustard auto-update` — check npm for a newer CLI and install it.
//!
//! Ported from `commands/auto-update.ts`. The flow:
//!
//! 1. query the npm registry for the latest published version;
//! 2. compare it against this build's [`crate::VERSION`];
//! 3. when newer, confirm (unless `--yes`) and run `npm install -g`;
//! 4. `--check-only` reports the gap and stops before installing.
//!
//! The "current version" is [`crate::VERSION`] — the compiled-in
//! `CARGO_PKG_VERSION`. The JS port read it back from `package.json`; the
//! native binary has no such file, and the package version is already the
//! single source of truth (see `lib.rs`).

use std::cmp::Ordering;
use std::io::IsTerminal;

use anyhow::{Context, Result};
use dialoguer::Confirm;
use dialoguer::theme::ColorfulTheme;

use crate::npm;

/// Flags accepted by `mustard auto-update`.
#[derive(Debug, Default, Clone)]
pub struct AutoUpdateOptions {
    /// Report whether an update exists, but do not install it.
    pub check_only: bool,
    /// Skip the confirmation prompt.
    pub yes: bool,
}

/// Run `mustard auto-update`.
pub fn auto_update(options: &AutoUpdateOptions) -> Result<()> {
    println!("\nMustard CLI - Auto Update\n");

    println!("  Checking for updates...");
    let latest = npm::get_latest_version().context("failed to check for updates")?;
    let current = crate::VERSION;

    println!("  Current version: {current}");
    println!("  Latest version:  {latest}");
    println!();

    let has_update = npm::compare_versions(current, &latest) == Ordering::Less;
    if !has_update {
        println!("You are running the latest version!\n");
        return Ok(());
    }

    println!("Update available: {current} -> {latest}\n");

    if options.check_only {
        println!("  Run `mustard auto-update` to install the update.\n");
        return Ok(());
    }

    // Confirm — interactive only. A non-TTY stdin proceeds without blocking.
    if !options.yes && std::io::stdin().is_terminal() {
        let proceed = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Install update now?")
            .default(true)
            .interact()
            .context("reading the update confirmation")?;
        if !proceed {
            println!("\n  Cancelled.\n");
            return Ok(());
        }
    }

    println!("  Installing update...");
    npm::update_global()?;
    println!("\nUpdated to v{latest}!\n");
    println!("  Run `mustard update` in your projects to update .claude/ files.\n");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_latest_is_an_update() {
        // compare_versions drives the has-update decision.
        assert_eq!(npm::compare_versions("1.0.0", "1.0.1"), Ordering::Less);
        assert_eq!(npm::compare_versions("2.0.0", "1.9.9"), Ordering::Greater);
        assert_eq!(npm::compare_versions("1.2.3", "1.2.3"), Ordering::Equal);
    }
}
