//! The questions `mustard init` asks: what to do with a `.claude/` that is
//! already there, and the backup that one of the answers takes.
//!
//! The git-flow questions live in [`crate::commands::git_flow`], which
//! [`super::project_config`] calls when it writes `mustard.json`.

use std::io::IsTerminal;
use std::path::Path;

use anyhow::{Context, Result};
use dialoguer::theme::ColorfulTheme;
use dialoguer::Select;

use crate::fs_ops::copy_dir;

use super::InitOptions;

/// What to do with an already-present `.claude/` directory.
pub(super) enum ExistingAction {
    /// Overwrite (the `--force` path, or the interactive "backup" choice once
    /// the backup has been taken).
    Overwrite,
    /// Keep user edits: seed only the files that are absent, but still merge the
    /// plugin-enable keys into `settings.json`.
    Merge,
    /// Abort without writing.
    Cancel,
}

/// Decide how to treat an existing `.claude/`, prompting if no flag settles
/// it. On the interactive "backup" choice the backup is taken here, so the
/// returned action is then [`ExistingAction::Overwrite`].
pub(super) fn decide_existing_action(claude_path: &Path, options: &InitOptions) -> Result<ExistingAction> {
    if options.force {
        return Ok(ExistingAction::Overwrite);
    }
    if options.yes {
        println!("  .claude/ exists - updating without overwriting user files");
        return Ok(ExistingAction::Merge);
    }
    // Non-interactive stdin (CI, tests, a library caller): default to the
    // safe merge
    // rather than blocking on a prompt that can never be answered.
    if !std::io::stdin().is_terminal() {
        println!("  .claude/ exists - merging (non-interactive)");
        return Ok(ExistingAction::Merge);
    }

    let choices = ["Backup and overwrite", "Merge (keep my files)", "Cancel"];
    let choice = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(".claude/ already exists")
        .items(choices)
        .default(1)
        .interact()
        .context("reading the .claude/ conflict choice")?;

    match choice {
        0 => {
            backup_claude_dir(claude_path)?;
            Ok(ExistingAction::Overwrite)
        }
        1 => Ok(ExistingAction::Merge),
        _ => Ok(ExistingAction::Cancel),
    }
}

/// Copy `.claude/` to a timestamped `.backup.` sibling.
fn backup_claude_dir(claude_path: &Path) -> Result<()> {
    let stamp = mustard_core::time::filename_safe_now();
    let backup = claude_path.with_file_name(format!(
        "{}.backup.{stamp}",
        claude_path
            .file_name()
            .map_or_else(|| ".claude".to_string(), |n| n.to_string_lossy().into_owned())
    ));
    copy_dir(claude_path, &backup, true, &[])?;
    println!("  Backup: {}", backup.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn timestamp_slug_has_expected_shape() {
        let slug = mustard_core::time::filename_safe_now();
        // YYYY-MM-DDTHH-MM-SS
        assert_eq!(slug.len(), 19);
        assert_eq!(&slug[4..5], "-");
        assert_eq!(&slug[10..11], "T");
    }
}
