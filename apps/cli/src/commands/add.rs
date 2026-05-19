//! `mustard add` — install a community template into `.claude/`.
//!
//! Ported from `commands/add.ts`. A template is fetched from one of two
//! sources, in order:
//!
//! 1. **GitHub** — `git clone --depth 1` of `mustard-templates/<name>`. Kept
//!    as a `git` shell-out: cloning speaks the git smart-HTTP protocol, which
//!    a plain HTTP client cannot drive.
//! 2. **npm** — `mustard-template-<name>`, downloaded as a `.tgz` tarball.
//!    The JS port shelled out to `npm pack`; the Rust port fetches the tarball
//!    directly over HTTP (`ureq`) from the npm registry, then gunzips
//!    (`flate2`) and untars (`tar`) it. npm packs into a `package/` subdir, so
//!    that becomes the fetched root.
//!
//! Once fetched, a `mustard-template.json` manifest (or, absent one, the set
//! of known `.claude/` subdirectories) drives the copy into `.claude/`.
//! Existing files are skipped unless `--force`. Manifest `hooks_additions` are
//! merged into `.claude/settings.json`.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::fs_ops::read_json_object;

/// Flags accepted by `mustard add`.
#[derive(Debug, Default, Clone)]
pub struct AddOptions {
    /// Overwrite files that already exist in `.claude/`.
    pub force: bool,
}

/// A `mustard-template.json` manifest. All fields but `files` are optional;
/// `serde` defaults cover a minimal manifest.
#[derive(Debug, Deserialize)]
struct TemplateManifest {
    #[serde(default)]
    name: String,
    #[serde(default = "default_version")]
    version: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    hooks_additions: Vec<HookAddition>,
}

fn default_version() -> String {
    "0.0.0".to_string()
}

/// One `hooks_additions` entry — a hook to register in `settings.json`.
#[derive(Debug, Deserialize)]
struct HookAddition {
    event: String,
    matcher: String,
    command: String,
    #[serde(default = "default_timeout")]
    timeout: u64,
}

fn default_timeout() -> u64 {
    5
}

/// Run `mustard add <template_spec>` in `cwd`.
///
/// `template_spec` is `"template:<name>"` or just `"<name>"`.
pub fn add(cwd: &Path, template_spec: &str, options: &AddOptions) -> Result<()> {
    let name = template_spec.strip_prefix("template:").unwrap_or(template_spec);
    validate_name(name)?;

    let claude_dir = cwd.join(".claude");
    if !claude_dir.is_dir() {
        bail!("no .claude/ directory found - run `mustard init` first");
    }

    println!("Installing template: {name}");

    let work = TempDir::new(name)?;
    let fetched = fetch_template(name, work.path())?;

    let manifest = load_manifest(&fetched, name);
    println!("Template: {} v{}", manifest.name, manifest.version);
    if let Some(desc) = &manifest.description {
        println!("  {desc}");
    }

    let (copied, skipped) = copy_files(&fetched, &claude_dir, &manifest, options.force)?;
    merge_hook_additions(&claude_dir, &manifest)?;

    println!("\nTemplate installed: {copied} file(s) copied, {skipped} skipped.");
    if skipped > 0 {
        println!("Use --force to overwrite existing files.");
    }
    Ok(())
}

/// Reject names with traversal sequences or characters outside
/// `[A-Za-z0-9_-]`. Mirrors the JS regex guard.
fn validate_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && !name.contains("..")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !valid {
        bail!("invalid template name: \"{name}\" - use alphanumeric, hyphens, underscores only");
    }
    Ok(())
}

/// Fetch the template into a working directory, returning the directory that
/// holds its files. Tries GitHub, then npm.
fn fetch_template(name: &str, work: &Path) -> Result<PathBuf> {
    let repo_url = format!("https://github.com/mustard-templates/{name}.git");
    println!("Fetching from {repo_url}...");

    let cloned = Command::new("git")
        .args(["clone", "--depth", "1", &repo_url, &work.to_string_lossy()])
        .output();
    if matches!(&cloned, Ok(out) if out.status.success()) {
        return Ok(work.to_path_buf());
    }

    // Fall back to the npm package.
    println!("  GitHub repo not found. Trying npm: mustard-template-{name}...");
    fetch_from_npm(name, work).with_context(|| {
        format!(
            "template \"{name}\" not found on GitHub (mustard-templates/{name}) \
             or npm (mustard-template-{name})"
        )
    })
}

/// Download `mustard-template-<name>` from the npm registry and extract it.
/// Returns the `package/` subdirectory npm tarballs are rooted at.
fn fetch_from_npm(name: &str, work: &Path) -> Result<PathBuf> {
    let package = format!("mustard-template-{name}");
    let tarball_url = npm_tarball_url(&package)?;

    let tgz = ureq::get(&tarball_url)
        .call()
        .with_context(|| format!("downloading {tarball_url}"))?
        .body_mut()
        .read_to_vec()
        .context("reading the npm tarball body")?;

    let decoder = flate2::read::GzDecoder::new(&tgz[..]);
    tar::Archive::new(decoder)
        .unpack(work)
        .context("extracting the npm tarball")?;

    // npm tarballs root every file under `package/`.
    let package_dir = work.join("package");
    if package_dir.is_dir() {
        Ok(package_dir)
    } else {
        Ok(work.to_path_buf())
    }
}

/// Resolve the latest tarball URL for `package` from the npm registry.
fn npm_tarball_url(package: &str) -> Result<String> {
    let url = format!("https://registry.npmjs.org/{package}");
    let meta: Value = ureq::get(&url)
        .call()
        .with_context(|| format!("querying npm registry for {package}"))?
        .body_mut()
        .read_json()
        .context("parsing the npm registry response")?;

    let latest = meta
        .get("dist-tags")
        .and_then(|t| t.get("latest"))
        .and_then(Value::as_str)
        .context("npm registry response has no dist-tags.latest")?;

    meta.get("versions")
        .and_then(|v| v.get(latest))
        .and_then(|v| v.get("dist"))
        .and_then(|d| d.get("tarball"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .context("npm registry response has no tarball URL for the latest version")
}

/// Load `mustard-template.json` from `fetched`, or synthesise a manifest from
/// the known `.claude/` subdirectories present.
fn load_manifest(fetched: &Path, name: &str) -> TemplateManifest {
    let manifest_path = fetched.join("mustard-template.json");
    if let Ok(raw) = std::fs::read_to_string(&manifest_path) {
        if let Ok(manifest) = serde_json::from_str::<TemplateManifest>(&raw) {
            return manifest;
        }
    }
    TemplateManifest {
        name: name.to_string(),
        version: default_version(),
        description: None,
        files: detect_files(fetched),
        hooks_additions: Vec::new(),
    }
}

/// The known `.claude/` subdirectories an unmanifested template may carry.
fn detect_files(dir: &Path) -> Vec<String> {
    ["commands", "skills", "hooks", "context", "scripts"]
        .iter()
        .filter(|sub| dir.join(sub).is_dir())
        .map(|sub| (*sub).to_string())
        .collect()
}

/// Copy each manifest `files` entry from `fetched` into `claude_dir`.
/// Returns `(copied, skipped)`.
fn copy_files(
    fetched: &Path,
    claude_dir: &Path,
    manifest: &TemplateManifest,
    force: bool,
) -> Result<(usize, usize)> {
    let mut copied = 0usize;
    let mut skipped = 0usize;

    for pattern in &manifest.files {
        let src = fetched.join(pattern);
        if !src.exists() {
            continue;
        }
        let dest_base = claude_dir.join(pattern);

        if src.is_dir() {
            for file in walk_dir(&src) {
                let rel = file.strip_prefix(&src).unwrap_or(&file);
                let dest = dest_base.join(rel);
                if dest.exists() && !force {
                    println!("  Skipping existing: {}", dest_base.join(rel).display());
                    skipped += 1;
                    continue;
                }
                copy_one(&file, &dest)?;
                println!("  Copied: {}", Path::new(pattern).join(rel).display());
                copied += 1;
            }
        } else {
            if dest_base.exists() && !force {
                println!("  Skipping existing: {pattern}");
                skipped += 1;
                continue;
            }
            copy_one(&src, &dest_base)?;
            println!("  Copied: {pattern}");
            copied += 1;
        }
    }
    Ok((copied, skipped))
}

/// Copy a single file, creating its parent directory.
fn copy_one(src: &Path, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::copy(src, dest)
        .with_context(|| format!("copying {} -> {}", src.display(), dest.display()))?;
    Ok(())
}

/// Recursively collect file paths under `dir`, skipping `.git`/`node_modules`.
fn walk_dir(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if name == ".git" || name == "node_modules" {
                continue;
            }
            out.extend(walk_dir(&path));
        } else {
            out.push(path);
        }
    }
    out
}

/// Merge a manifest's `hooks_additions` into `.claude/settings.json`. A hook
/// whose `command` is already registered for its event is skipped. Fail-open:
/// a malformed or absent `settings.json` is reported, not fatal.
fn merge_hook_additions(claude_dir: &Path, manifest: &TemplateManifest) -> Result<()> {
    if manifest.hooks_additions.is_empty() {
        return Ok(());
    }
    let settings_path = claude_dir.join("settings.json");
    if !settings_path.is_file() {
        return Ok(());
    }

    let mut settings = read_json_object(&settings_path);
    let hooks = settings
        .entry("hooks")
        .or_insert_with(|| json!({}));
    let Some(hooks) = hooks.as_object_mut() else {
        eprintln!("  Could not merge hooks: settings.json `hooks` is not an object");
        return Ok(());
    };

    for hook in &manifest.hooks_additions {
        let bucket = hooks
            .entry(hook.event.clone())
            .or_insert_with(|| json!([]));
        let Some(bucket) = bucket.as_array_mut() else {
            continue;
        };
        let already = bucket.iter().any(|entry| {
            entry
                .get("hooks")
                .and_then(Value::as_array)
                .is_some_and(|inner| {
                    inner
                        .iter()
                        .any(|h| h.get("command").and_then(Value::as_str) == Some(&hook.command))
                })
        });
        if !already {
            bucket.push(json!({
                "matcher": hook.matcher,
                "hooks": [{
                    "type": "command",
                    "command": hook.command,
                    "timeout": hook.timeout,
                }],
            }));
            let basename = Path::new(&hook.command)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| hook.command.clone());
            println!("  Registered hook: {} -> {basename}", hook.event);
        }
    }

    let mut serialized = serde_json::to_string_pretty(&Value::Object(settings))
        .context("serializing settings.json")?;
    serialized.push('\n');
    std::fs::write(&settings_path, serialized)
        .with_context(|| format!("writing {}", settings_path.display()))?;
    Ok(())
}

/// A working directory under the system temp dir, removed on drop.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// Create a uniquely named working directory for template `name`.
    fn new(name: &str) -> Result<Self> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("mustard-template-{name}-{stamp}"));
        std::fs::create_dir_all(&path)
            .with_context(|| format!("creating temp dir {}", path.display()))?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // Best-effort cleanup — a leftover temp dir is harmless.
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn validate_name_accepts_plain_names() {
        assert!(validate_name("dotnet-clean-arch").is_ok());
        assert!(validate_name("my_template").is_ok());
    }

    #[test]
    fn validate_name_rejects_traversal_and_specials() {
        assert!(validate_name("../etc").is_err());
        assert!(validate_name("foo/bar").is_err());
        assert!(validate_name("").is_err());
    }

    #[test]
    fn add_errors_without_claude_dir() {
        let work = tempdir().unwrap();
        let err = add(work.path(), "template:foo", &AddOptions::default()).unwrap_err();
        assert!(err.to_string().contains("no .claude/ directory"));
    }

    #[test]
    fn detect_files_finds_known_subdirs() {
        let work = tempdir().unwrap();
        fs::create_dir_all(work.path().join("hooks")).unwrap();
        fs::create_dir_all(work.path().join("skills")).unwrap();
        let found = detect_files(work.path());
        assert!(found.contains(&"hooks".to_string()));
        assert!(found.contains(&"skills".to_string()));
        assert!(!found.contains(&"commands".to_string()));
    }

    #[test]
    fn copy_files_skips_existing_without_force() {
        let work = tempdir().unwrap();
        let fetched = work.path().join("fetched");
        let claude = work.path().join(".claude");
        fs::create_dir_all(fetched.join("hooks")).unwrap();
        fs::write(fetched.join("hooks/guard.js"), "new").unwrap();
        fs::create_dir_all(claude.join("hooks")).unwrap();
        fs::write(claude.join("hooks/guard.js"), "user-edit").unwrap();

        let manifest = TemplateManifest {
            name: "t".into(),
            version: "1.0.0".into(),
            description: None,
            files: vec!["hooks".into()],
            hooks_additions: Vec::new(),
        };
        let (copied, skipped) = copy_files(&fetched, &claude, &manifest, false).unwrap();

        assert_eq!((copied, skipped), (0, 1));
        assert_eq!(fs::read_to_string(claude.join("hooks/guard.js")).unwrap(), "user-edit");
    }

    #[test]
    fn copy_files_overwrites_with_force() {
        let work = tempdir().unwrap();
        let fetched = work.path().join("fetched");
        let claude = work.path().join(".claude");
        fs::create_dir_all(fetched.join("hooks")).unwrap();
        fs::write(fetched.join("hooks/guard.js"), "new").unwrap();
        fs::create_dir_all(claude.join("hooks")).unwrap();
        fs::write(claude.join("hooks/guard.js"), "user-edit").unwrap();

        let manifest = TemplateManifest {
            name: "t".into(),
            version: "1.0.0".into(),
            description: None,
            files: vec!["hooks".into()],
            hooks_additions: Vec::new(),
        };
        let (copied, skipped) = copy_files(&fetched, &claude, &manifest, true).unwrap();

        assert_eq!((copied, skipped), (1, 0));
        assert_eq!(fs::read_to_string(claude.join("hooks/guard.js")).unwrap(), "new");
    }

    #[test]
    fn merge_hook_additions_registers_new_hook() {
        let work = tempdir().unwrap();
        let claude = work.path().join(".claude");
        fs::create_dir_all(&claude).unwrap();
        fs::write(claude.join("settings.json"), r#"{"hooks":{}}"#).unwrap();

        let manifest = TemplateManifest {
            name: "t".into(),
            version: "1.0.0".into(),
            description: None,
            files: Vec::new(),
            hooks_additions: vec![HookAddition {
                event: "PreToolUse".into(),
                matcher: "Bash".into(),
                command: "node hooks/x.js".into(),
                timeout: 7,
            }],
        };
        merge_hook_additions(&claude, &manifest).unwrap();

        let settings = read_json_object(&claude.join("settings.json"));
        let pre = settings["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 1);
        assert_eq!(pre[0]["hooks"][0]["command"], "node hooks/x.js");
    }

    #[test]
    fn temp_dir_is_removed_on_drop() {
        let path = {
            let tmp = TempDir::new("drop-test").unwrap();
            assert!(tmp.path().is_dir());
            tmp.path().to_path_buf()
        };
        assert!(!path.exists(), "temp dir removed on drop");
    }
}
