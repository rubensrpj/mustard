//! Agnostic build/test/lint/type-check command detection.
//!
//! `mustard init` used to hardcode an `npm` command set into every fresh
//! `mustard.json` — wrong for a workspace that has no `npm` (the Mustard repo
//! itself is Rust + pnpm). This module replaces that assumption with a small,
//! stack-agnostic probe over the project's manifests/lockfiles. It lives in the
//! core (not the CLI) because the `scan` consumes the same `buildCommand`, so
//! the heuristic is shared domain logic, not UI.
//!
//! Two entry points share the probe:
//!
//! - [`detect_commands`] — used by `mustard init` to seed `mustard.json`. An
//!   unknown stack yields the neutral [`BUILD_COMMAND_FALLBACK`] placeholder so
//!   a fresh config always has a `buildCommand` field to edit.
//! - [`detect_commands_for_unit`] — used by `scan --full` per subproject. It
//!   ascends from the unit toward the scan root to find a JS package-manager
//!   signal that, in a monorepo, only exists at the root; it prefers the unit's
//!   own mined scripts over conventional names; and when no real signal exists
//!   at all it returns an empty set (no placeholder), so the generated
//!   `## Commands` section is simply omitted.

use std::path::{Path, PathBuf};

use crate::domain::config::{Commands, BUILD_COMMAND_FALLBACK};
use crate::io::fs;

/// Detect the command set for the project rooted at `root`, seeding a neutral
/// placeholder build command for an unknown stack.
///
/// This is the `mustard init` entry point: a fresh `mustard.json` always gets a
/// `buildCommand` field (the [`BUILD_COMMAND_FALLBACK`] placeholder when nothing
/// is recognised) for the user to edit. The package-manager probe is local to
/// `root` only — `init` runs at the workspace anchor, so no ascent is needed.
///
/// Probed in order: Rust (`Cargo.toml`), JS/TS (lockfile or `packageManager`),
/// Go (`go.mod`), Make (`Makefile`).
#[must_use]
pub fn detect_commands(root: &Path) -> Commands {
    if let Some(c) = detect_native(root) {
        return c;
    }
    if let Some(pm) = detect_js_package_manager(root) {
        return js_commands(&pm, &[]);
    }
    Commands {
        build: Some(BUILD_COMMAND_FALLBACK.into()),
        test: None,
        lint: None,
        type_check: None,
    }
}

/// Detect the command set for one scanned subproject.
///
/// `dir` is the subproject root; `workspace_root` bounds the upward search; and
/// `scripts` are the build/codegen scripts the scan mined from this unit's
/// manifests. Native stacks (Rust/Go/Make) are probed at `dir` directly. For
/// JS/TS the probe ascends from `dir` toward `workspace_root` to find the
/// package-manager signal (a monorepo leaf often has only a bare
/// `package.json`, the lockfile living at the root) and then maps the mined
/// scripts to the build/test/lint/type-check stages by their conventional
/// names, preferring a real script over a guessed one.
///
/// When no native manifest and no JS signal are found, this returns an empty
/// set (every stage `None`) — no placeholder — so the caller omits the
/// `## Commands` section rather than emitting a non-runnable stub.
#[must_use]
pub fn detect_commands_for_unit(
    dir: &Path,
    workspace_root: &Path,
    scripts: &[String],
) -> Commands {
    if let Some(c) = detect_native(dir) {
        return c;
    }
    if let Some((_, pm)) = ascend_to_js_root(dir, workspace_root) {
        return js_commands(&pm, scripts);
    }
    Commands::default()
}

/// Probe a single directory for a native (non-JS) toolchain manifest. Returns
/// the conventional command set for the first match, or `None`.
fn detect_native(dir: &Path) -> Option<Commands> {
    if dir.join("Cargo.toml").is_file() {
        return Some(Commands {
            build: Some("cargo build".into()),
            test: Some("cargo test".into()),
            lint: Some("cargo clippy".into()),
            type_check: Some("cargo check".into()),
        });
    }
    if dir.join("go.mod").is_file() {
        return Some(Commands {
            build: Some("go build ./...".into()),
            test: Some("go test ./...".into()),
            lint: None,
            type_check: Some("go vet ./...".into()),
        });
    }
    if dir.join("Makefile").is_file() || dir.join("makefile").is_file() {
        return Some(Commands {
            build: Some("make".into()),
            test: Some("make test".into()),
            lint: None,
            type_check: None,
        });
    }
    None
}

/// Walk from `dir` upward toward `stop_at` (inclusive), returning the first
/// directory carrying a JS/TS package-manager signal together with the resolved
/// manager name. The ascent is bounded by `stop_at` (the scan root) and never
/// climbs past it — Mustard does not probe directories outside the scanned
/// workspace. This is intentionally a local ancestor probe (not the
/// `mustard.json`/`.claude` anchor) so it works for any nested JS monorepo.
fn ascend_to_js_root(dir: &Path, stop_at: &Path) -> Option<(PathBuf, String)> {
    let mut current = dir;
    loop {
        if let Some(pm) = detect_js_package_manager(current) {
            return Some((current.to_path_buf(), pm));
        }
        if current == stop_at {
            return None;
        }
        match current.parent() {
            Some(parent) if parent.starts_with(stop_at) || parent == stop_at => {
                current = parent;
            }
            _ => return None,
        }
    }
}

/// Resolve the JS/TS package manager for `dir`, if any.
///
/// Lockfiles win (they are authoritative); failing that, the `packageManager`
/// field in `package.json` is honoured (`"pnpm@9.1.0"` → `pnpm`); failing that,
/// a `pnpm-workspace.yaml` (workspace-root marker) implies `pnpm`. A bare
/// `package.json` with none of these yields `None` — Mustard does not guess a
/// manager (it will not assume `npm`).
fn detect_js_package_manager(dir: &Path) -> Option<String> {
    if dir.join("pnpm-lock.yaml").is_file() {
        return Some("pnpm".into());
    }
    if dir.join("yarn.lock").is_file() {
        return Some("yarn".into());
    }
    if dir.join("package-lock.json").is_file() {
        return Some("npm".into());
    }
    if dir.join("bun.lockb").is_file() {
        return Some("bun".into());
    }
    if dir.join("package.json").is_file() {
        if let Some(pm) = package_manager_field(dir) {
            return Some(pm);
        }
    }
    if dir.join("pnpm-workspace.yaml").is_file() {
        return Some("pnpm".into());
    }
    None
}

/// Read `package.json#packageManager` and strip the version suffix
/// (`"pnpm@9.1.0"` → `"pnpm"`). `None` when absent / unreadable / blank.
fn package_manager_field(dir: &Path) -> Option<String> {
    let text = fs::read_to_string(dir.join("package.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let raw = json.get("packageManager").and_then(serde_json::Value::as_str)?;
    let name = raw.split('@').next().unwrap_or("").trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Find the first mined script whose name matches any of `aliases`
/// (case-insensitive, exact). Returns the script's real name as declared.
fn find_script<'a>(scripts: &'a [String], aliases: &[&str]) -> Option<&'a str> {
    scripts.iter().map(String::as_str).find(|name| {
        aliases.iter().any(|alias| name.eq_ignore_ascii_case(alias))
    })
}

/// Build the JS/TS command set for package manager `pm`, preferring the unit's
/// real mined `scripts` over conventional names.
///
/// Each stage maps to a known script name (`build`; `test`; `lint`;
/// `typecheck`/`type-check`/`check`). When a script exists, the command runs it
/// (`{pm} test` for the universal `test` shortcut, `{pm} run <name>` otherwise);
/// when it does not, that stage stays `None` — no name is invented. With no
/// mined scripts at all (`scripts` empty), the conventional `build`/`test`/
/// `lint` set is seeded and `type_check` falls back to the agnostic `tsc
/// --noEmit`, mirroring the previous behaviour for the `init` seed.
fn js_commands(pm: &str, scripts: &[String]) -> Commands {
    if scripts.is_empty() {
        return Commands {
            build: Some(format!("{pm} run build")),
            test: Some(format!("{pm} test")),
            lint: Some(format!("{pm} run lint")),
            type_check: Some("tsc --noEmit".into()),
        };
    }

    let build = find_script(scripts, &["build"]).map(|name| format!("{pm} run {name}"));
    // `test` is the universal package-manager shortcut (no `run`).
    let test = find_script(scripts, &["test"]).map(|name| {
        if name.eq_ignore_ascii_case("test") {
            format!("{pm} test")
        } else {
            format!("{pm} run {name}")
        }
    });
    let lint = find_script(scripts, &["lint"]).map(|name| format!("{pm} run {name}"));
    let type_check = find_script(scripts, &["typecheck", "type-check", "check"])
        .map(|name| format!("{pm} run {name}"));

    Commands { build, test, lint, type_check }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn touch(dir: &Path, name: &str, body: &str) {
        std::fs::write(dir.join(name), body).unwrap();
    }

    fn no_scripts() -> Vec<String> {
        Vec::new()
    }

    #[test]
    fn rust_project_detects_cargo() {
        let d = tempdir().unwrap();
        touch(d.path(), "Cargo.toml", "[package]\nname=\"x\"");
        let c = detect_commands(d.path());
        assert_eq!(c.build.as_deref(), Some("cargo build"));
        assert_eq!(c.lint.as_deref(), Some("cargo clippy"));
    }

    #[test]
    fn pnpm_lockfile_detects_pnpm() {
        let d = tempdir().unwrap();
        touch(d.path(), "package.json", "{}");
        touch(d.path(), "pnpm-lock.yaml", "");
        let c = detect_commands(d.path());
        assert_eq!(c.build.as_deref(), Some("pnpm run build"));
        assert_eq!(c.test.as_deref(), Some("pnpm test"));
        assert_eq!(c.type_check.as_deref(), Some("tsc --noEmit"));
    }

    #[test]
    fn package_manager_field_when_no_lockfile() {
        let d = tempdir().unwrap();
        touch(d.path(), "package.json", r#"{"packageManager":"yarn@4.2.0"}"#);
        let c = detect_commands(d.path());
        assert_eq!(c.build.as_deref(), Some("yarn run build"));
    }

    #[test]
    fn bare_package_json_does_not_assume_npm() {
        let d = tempdir().unwrap();
        touch(d.path(), "package.json", "{}");
        let c = detect_commands(d.path());
        // No lockfile, no packageManager → neutral fallback, never npm.
        assert_eq!(c.build.as_deref(), Some(BUILD_COMMAND_FALLBACK));
        assert!(c.test.is_none());
    }

    #[test]
    fn makefile_detects_make() {
        let d = tempdir().unwrap();
        touch(d.path(), "Makefile", "build:\n\techo hi");
        let c = detect_commands(d.path());
        assert_eq!(c.build.as_deref(), Some("make"));
    }

    #[test]
    fn unknown_stack_is_neutral_fallback_for_init() {
        let d = tempdir().unwrap();
        let c = detect_commands(d.path());
        assert_eq!(c.build.as_deref(), Some(BUILD_COMMAND_FALLBACK));
        assert!(c.test.is_none() && c.lint.is_none() && c.type_check.is_none());
    }

    #[test]
    fn cargo_wins_over_js() {
        let d = tempdir().unwrap();
        touch(d.path(), "Cargo.toml", "[package]");
        touch(d.path(), "pnpm-lock.yaml", "");
        assert_eq!(detect_commands(d.path()).build.as_deref(), Some("cargo build"));
    }

    // --- detect_commands_for_unit (scan path) ------------------------------

    #[test]
    fn unit_unknown_stack_omits_commands() {
        // No native manifest, no JS signal anywhere up to the root → empty set,
        // never the placeholder. The caller drops the `## Commands` section.
        let d = tempdir().unwrap();
        let c = detect_commands_for_unit(d.path(), d.path(), &no_scripts());
        assert!(c.build.is_none(), "build must be omitted (no signal)");
        assert!(c.test.is_none() && c.lint.is_none() && c.type_check.is_none());
    }

    #[test]
    fn unit_rust_leaf_detects_cargo() {
        let root = tempdir().unwrap();
        let leaf = root.path().join("crates").join("inner");
        std::fs::create_dir_all(&leaf).unwrap();
        touch(&leaf, "Cargo.toml", "[package]");
        let c = detect_commands_for_unit(&leaf, root.path(), &no_scripts());
        assert_eq!(c.build.as_deref(), Some("cargo build"));
    }

    #[test]
    fn unit_monorepo_leaf_resolves_pnpm_from_root_lockfile() {
        // apps/web has only a bare package.json; the pnpm lockfile lives at the
        // scan root. The leaf must still resolve `pnpm` by ascending.
        let root = tempdir().unwrap();
        touch(root.path(), "pnpm-lock.yaml", "");
        let leaf = root.path().join("apps").join("web");
        std::fs::create_dir_all(&leaf).unwrap();
        touch(&leaf, "package.json", "{}");
        let scripts = vec!["build".to_string(), "test".to_string()];
        let c = detect_commands_for_unit(&leaf, root.path(), &scripts);
        assert_eq!(c.build.as_deref(), Some("pnpm run build"), "leaf must resolve pnpm");
        assert_eq!(c.test.as_deref(), Some("pnpm test"));
    }

    #[test]
    fn unit_ascent_is_bounded_by_workspace_root() {
        // A lockfile ABOVE the scan root must NOT be picked up — the ascent
        // stops at `workspace_root`.
        let outer = tempdir().unwrap();
        touch(outer.path(), "pnpm-lock.yaml", "");
        let scan_root = outer.path().join("repo");
        let leaf = scan_root.join("apps").join("web");
        std::fs::create_dir_all(&leaf).unwrap();
        touch(&leaf, "package.json", "{}");
        // stop_at = scan_root; the lockfile is outside it → no JS signal.
        let c = detect_commands_for_unit(&leaf, &scan_root, &no_scripts());
        assert!(c.build.is_none(), "must not climb past the scan root: {c:?}");
    }

    #[test]
    fn unit_prefers_real_mined_scripts_over_guessing() {
        // Real scripts present: use their actual names; never invent a stage.
        let root = tempdir().unwrap();
        touch(root.path(), "pnpm-lock.yaml", "");
        touch(root.path(), "package.json", "{}");
        let scripts = vec![
            "build".to_string(),
            "typecheck".to_string(),
            // no `test`, no `lint` script declared
        ];
        let c = detect_commands_for_unit(root.path(), root.path(), &scripts);
        assert_eq!(c.build.as_deref(), Some("pnpm run build"));
        assert_eq!(c.type_check.as_deref(), Some("pnpm run typecheck"), "real script name used");
        assert!(c.test.is_none(), "no test script → no test command (not guessed)");
        assert!(c.lint.is_none(), "no lint script → no lint command (not guessed)");
    }
}
