//! `mustard install-nerd-font` — install a Nerd Font on the host.
//!
//! Powerline-style statusline themes (`tokyo-night`, `catppuccin`, etc.)
//! need a Nerd Font so the U+E0B0 transition glyphs render as crisp 1-cell
//! triangles instead of tofu that collides with neighbour text. This command
//! is the one-stop installer.
//!
//! ## Platform handling
//!
//! - **Windows:** Scoop is the only path. We add `nerd-fonts` bucket
//!   (idempotent) and `scoop install nerd-fonts/<pkg>`. If Scoop is not on
//!   PATH we bail with a one-line install hint instead of trying to bootstrap
//!   PowerShell from Rust.
//! - **macOS:** Homebrew cask. Bail with install hint if `brew` is missing.
//! - **Linux:** Download the font zip from the official Nerd Fonts
//!   GitHub release into `~/.local/share/fonts/<family>NerdFont/` and refresh
//!   the fontconfig cache. Requires `curl` and `unzip` on PATH.
//!
//! The command is **idempotent**: it probes the OS font directories first and
//! short-circuits when the requested family is already installed. `--force`
//! skips the probe.

use anyhow::{Context, Result, bail};
use mustard_core::io::fs as mfs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Flags accepted by `mustard install-nerd-font`.
#[derive(Debug, Default, Clone)]
pub struct InstallNerdFontOptions {
    /// Font family. Defaults to [`DEFAULT_FONT`]. Case-insensitive against
    /// the `display` column of [`KNOWN_FONTS`].
    pub font: Option<String>,
    /// Reinstall even if the family is already detected on disk.
    pub force: bool,
    /// Print intended actions and exit without invoking any package manager.
    pub dry_run: bool,
}

/// The font installed when `--font` is not given.
pub const DEFAULT_FONT: &str = "JetBrainsMono";

/// Known font families and their package names per platform.
///
/// Columns: `(display, scoop_pkg, brew_cask, github_zip_basename)`.
const KNOWN_FONTS: &[(&str, &str, &str, &str)] = &[
    (
        "JetBrainsMono",
        "JetBrainsMono-NF",
        "font-jetbrains-mono-nerd-font",
        "JetBrainsMono",
    ),
    (
        "CaskaydiaCove",
        "CascadiaCode-NF",
        "font-caskaydia-cove-nerd-font",
        "CascadiaCode",
    ),
    (
        "FiraCode",
        "FiraCode-NF",
        "font-fira-code-nerd-font",
        "FiraCode",
    ),
    ("Hack", "Hack-NF", "font-hack-nerd-font", "Hack"),
];

fn known_list() -> String {
    KNOWN_FONTS
        .iter()
        .map(|(n, _, _, _)| *n)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Run `mustard install-nerd-font` in `project_path` (currently unused —
/// install is host-scoped, not project-scoped).
pub fn install_nerd_font(_project_path: &Path, options: &InstallNerdFontOptions) -> Result<()> {
    let family = options.font.as_deref().unwrap_or(DEFAULT_FONT);
    let pkg = KNOWN_FONTS
        .iter()
        .find(|(f, _, _, _)| f.eq_ignore_ascii_case(family))
        .with_context(|| {
            format!(
                "unknown font family '{family}' — known: {}",
                known_list()
            )
        })?;
    let (display, scoop_pkg, brew_cask, gh_zip) = *pkg;

    println!("Mustard — install {display} Nerd Font\n");

    if !options.force && is_installed(display) {
        println!(
            "{display} Nerd Font is already installed. Re-run with --force to reinstall."
        );
        return Ok(());
    }

    if options.dry_run {
        println!("[dry-run] would install {display} via the platform package manager.");
        return Ok(());
    }

    // `cfg!()` (macro, not the `#[cfg]` attribute) keeps every arm in the AST,
    // so all three installers are type-checked on every target even though
    // only the matching one runs. That is what lets a Windows build catch a
    // Rust-level error in the macOS / Linux paths.
    if cfg!(target_os = "windows") {
        install_windows(scoop_pkg, display)
    } else if cfg!(target_os = "macos") {
        install_macos(brew_cask, display)
    } else if cfg!(target_os = "linux") {
        install_linux(gh_zip, display)
    } else {
        bail!(
            "unsupported platform — install a Nerd Font manually from https://www.nerdfonts.com/font-downloads"
        )
    }
}

// ---------------------------------------------------------------------------
// Probe — has the font already been installed?
// ---------------------------------------------------------------------------

fn is_installed(family: &str) -> bool {
    let needle = family.to_ascii_lowercase();
    for dir in font_dirs() {
        if scan_for_nerd_font(&dir, &needle) {
            return true;
        }
    }
    // Linux: fontconfig is the authoritative source if available.
    #[cfg(target_os = "linux")]
    {
        if let Ok(output) = Command::new("fc-list").output() {
            if output.status.success() {
                let listing = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
                if listing.contains(&needle) && listing.contains("nerd") {
                    return true;
                }
            }
        }
    }
    false
}

fn font_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    #[cfg(target_os = "windows")]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            dirs.push(
                std::path::PathBuf::from(local)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Fonts"),
            );
        }
        dirs.push(std::path::PathBuf::from("C:/Windows/Fonts"));
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(std::path::PathBuf::from(home).join("Library").join("Fonts"));
        }
        dirs.push(std::path::PathBuf::from("/Library/Fonts"));
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(std::path::PathBuf::from(home).join(".local/share/fonts"));
        }
        dirs.push(std::path::PathBuf::from("/usr/share/fonts"));
    }
    dirs
}

/// Look for any file under `dir` (one level deep) whose lowercased name
/// contains `family_lower` and either "nerd" or "nf-". Fail-open: read
/// errors return `false`.
fn scan_for_nerd_font(dir: &Path, family_lower: &str) -> bool {
    let Ok(entries) = mfs::read_dir(dir) else {
        return false;
    };
    for entry in entries {
        let name = entry.file_name.to_ascii_lowercase();
        if file_matches(&name, family_lower) {
            return true;
        }
        if entry.is_dir {
            let Ok(sub) = mfs::read_dir(&entry.path) else {
                continue;
            };
            for s in sub {
                let sn = s.file_name.to_ascii_lowercase();
                if file_matches(&sn, family_lower) {
                    return true;
                }
            }
        }
    }
    false
}

fn file_matches(name: &str, family_lower: &str) -> bool {
    name.contains(family_lower) && (name.contains("nerd") || name.contains("nf-"))
}

// ---------------------------------------------------------------------------
// Platform installers
// ---------------------------------------------------------------------------

fn install_windows(scoop_pkg: &str, display: &str) -> Result<()> {
    // On Windows the canonical entry point is `scoop.cmd` (the bare `scoop`
    // shim is a Unix-style shell script and `CreateProcess` will not run it).
    // Either shim on PATH means Scoop is present; always spawn the `.cmd`.
    if !which("scoop.cmd") && !which("scoop") {
        bail!(
            "Scoop is required on Windows.\n\
             Install Scoop with PowerShell 7:\n  pwsh -NoProfile -Command \"Set-ExecutionPolicy -Scope CurrentUser RemoteSigned -Force; iwr -useb get.scoop.sh | iex\"\n\
             Then re-run: mustard install-nerd-font"
        );
    }
    let scoop = "scoop.cmd";

    println!("→ adding nerd-fonts bucket (idempotent)…");
    // `scoop bucket add` returns non-zero if the bucket already exists; ignore.
    let _ = Command::new(scoop)
        .args(["bucket", "add", "nerd-fonts"])
        .status();

    println!("→ {scoop} install nerd-fonts/{scoop_pkg}…");
    let status = Command::new(scoop)
        .args(["install", &format!("nerd-fonts/{scoop_pkg}")])
        .status()
        .context("running `scoop install`")?;
    if !status.success() {
        bail!("scoop install failed (exit {})", status.code().unwrap_or(-1));
    }

    println!("\n{display} Nerd Font installed.");
    println!(
        "Restart your terminal and set its font to '{display} Nerd Font' (or a *NF Mono variant)."
    );
    Ok(())
}

fn install_macos(brew_cask: &str, display: &str) -> Result<()> {
    if !which("brew") {
        bail!(
            "Homebrew is required on macOS.\n\
             Install: /bin/bash -c \"$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\"\n\
             Then re-run: mustard install-nerd-font"
        );
    }
    println!("→ brew install --cask {brew_cask}…");
    let status = Command::new("brew")
        .args(["install", "--cask", brew_cask])
        .status()
        .context("running `brew install`")?;
    if !status.success() {
        bail!("brew install failed (exit {})", status.code().unwrap_or(-1));
    }
    println!("\n{display} Nerd Font installed.");
    println!("Restart your terminal and set its font to '{display} Nerd Font'.");
    Ok(())
}

fn install_linux(zip_basename: &str, display: &str) -> Result<()> {
    if !which("curl") {
        bail!("`curl` is required for the Linux install. Install via your package manager.");
    }
    if !which("unzip") {
        bail!("`unzip` is required for the Linux install. Install via your package manager.");
    }
    let home = std::env::var("HOME").context("HOME env var is unset")?;
    let target: PathBuf = PathBuf::from(home)
        .join(".local/share/fonts")
        .join(format!("{display}NerdFont"));
    mfs::create_dir_all(&target).context("creating ~/.local/share/fonts/<family>")?;
    let zip_path = target.join("_download.zip");
    let url = format!(
        "https://github.com/ryanoasis/nerd-fonts/releases/latest/download/{zip_basename}.zip"
    );
    println!("→ downloading {url}…");
    let status = Command::new("curl")
        .args(["-fsSL", "-o"])
        .arg(&zip_path)
        .arg(&url)
        .status()
        .context("running `curl`")?;
    if !status.success() {
        bail!("curl download failed");
    }
    println!("→ extracting to {}…", target.display());
    let status = Command::new("unzip")
        .args(["-o"])
        .arg(&zip_path)
        .arg("-d")
        .arg(&target)
        .status()
        .context("running `unzip`")?;
    if !status.success() {
        bail!("unzip failed");
    }
    let _ = mfs::remove_file(&zip_path);
    if which("fc-cache") {
        println!("→ refreshing fontconfig cache…");
        let _ = Command::new("fc-cache").args(["-f"]).arg(&target).status();
    }
    println!("\n{display} Nerd Font installed.");
    println!("Restart your terminal and set its font to '{display} Nerd Font'.");
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn which(binary: &str) -> bool {
    let path_var = std::env::var("PATH").unwrap_or_default();
    let sep = if cfg!(target_os = "windows") { ';' } else { ':' };
    for dir in path_var.split(sep) {
        let p = Path::new(dir);
        if p.join(binary).exists() {
            return true;
        }
        #[cfg(target_os = "windows")]
        {
            for ext in ["exe", "cmd", "bat", "ps1"] {
                if p.join(format!("{binary}.{ext}")).exists() {
                    return true;
                }
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_fonts_includes_default() {
        assert!(
            KNOWN_FONTS
                .iter()
                .any(|(f, _, _, _)| f.eq_ignore_ascii_case(DEFAULT_FONT)),
            "DEFAULT_FONT must be in KNOWN_FONTS"
        );
    }

    #[test]
    fn known_list_is_non_empty() {
        assert!(!known_list().is_empty());
        assert!(known_list().contains("JetBrainsMono"));
    }

    #[test]
    fn unknown_font_yields_error() {
        let tmp = tempfile::tempdir().unwrap();
        let opts = InstallNerdFontOptions {
            font: Some("Comic Sans".into()),
            ..Default::default()
        };
        let err = install_nerd_font(tmp.path(), &opts).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("Comic Sans"));
        assert!(msg.contains("JetBrainsMono"));
    }

    #[test]
    fn dry_run_does_not_invoke_installer() {
        let tmp = tempfile::tempdir().unwrap();
        let opts = InstallNerdFontOptions {
            font: Some("JetBrainsMono".into()),
            force: true, // bypass is_installed
            dry_run: true,
        };
        // Should succeed without any package manager call — the test
        // environment may not have scoop/brew at all.
        install_nerd_font(tmp.path(), &opts).expect("dry-run must succeed");
    }

    #[test]
    fn file_matches_requires_family_and_nerd_token() {
        assert!(file_matches("jetbrainsmononerdfont-regular.ttf", "jetbrainsmono"));
        assert!(file_matches("jbm nf-regular.otf", "jbm"));
        assert!(!file_matches("jetbrainsmono-regular.ttf", "jetbrainsmono"));
        assert!(!file_matches("nerdfont-regular.ttf", "jetbrainsmono"));
    }

    #[test]
    fn which_finds_a_universally_present_binary() {
        // `cargo` is always on PATH during `cargo test`.
        assert!(which("cargo"));
    }
}
