//! Agnostic build/test/lint/type-check command detection.
//!
//! `mustard init` used to hardcode an `npm` command set into every fresh
//! `mustard.json` — wrong for a workspace that has no `npm` (the Mustard repo
//! itself is Rust + pnpm). This module replaces that assumption with a small,
//! stack-agnostic probe over the project's manifests/lockfiles. It lives in the
//! core (not the CLI) because the `scan` consumes the same `buildCommand`, so
//! the heuristic is shared domain logic, not UI.
//!
//! Detection is best-effort and editable: it seeds a sensible default the user
//! can override in `mustard.json`. Unknown stacks fall back to the neutral
//! [`BUILD_COMMAND_FALLBACK`] placeholder (not a runnable command), so nothing
//! stack-specific is ever assumed.

use std::path::Path;

use crate::domain::config::{Commands, BUILD_COMMAND_FALLBACK};
use crate::io::fs;

/// Detect the command set for the project rooted at `root`.
///
/// Probed in order: Rust (`Cargo.toml`), JS/TS (lockfile or `packageManager`),
/// Go (`go.mod`), Make (`Makefile`). No match ⇒ a neutral placeholder build
/// command and no other stage (absent command = "skip that stage" in the gate).
#[must_use]
pub fn detect_commands(root: &Path) -> Commands {
    if root.join("Cargo.toml").is_file() {
        return Commands {
            build: Some("cargo build".into()),
            test: Some("cargo test".into()),
            lint: Some("cargo clippy".into()),
            type_check: Some("cargo check".into()),
        };
    }

    if let Some(pm) = detect_js_package_manager(root) {
        return js_commands(&pm);
    }

    if root.join("go.mod").is_file() {
        return Commands {
            build: Some("go build ./...".into()),
            test: Some("go test ./...".into()),
            lint: None,
            type_check: Some("go vet ./...".into()),
        };
    }

    if root.join("Makefile").is_file() || root.join("makefile").is_file() {
        return Commands {
            build: Some("make".into()),
            test: Some("make test".into()),
            lint: None,
            type_check: None,
        };
    }

    Commands {
        build: Some(BUILD_COMMAND_FALLBACK.into()),
        test: None,
        lint: None,
        type_check: None,
    }
}

/// Resolve the JS/TS package manager for `root`, if any.
///
/// Lockfiles win (they are authoritative); failing that, the `packageManager`
/// field in `package.json` is honoured (`"pnpm@9.1.0"` → `pnpm`). A bare
/// `package.json` with neither signal yields `None` — Mustard does not guess a
/// manager (it will not assume `npm`).
fn detect_js_package_manager(root: &Path) -> Option<String> {
    if root.join("pnpm-lock.yaml").is_file() {
        return Some("pnpm".into());
    }
    if root.join("yarn.lock").is_file() {
        return Some("yarn".into());
    }
    if root.join("package-lock.json").is_file() {
        return Some("npm".into());
    }
    if root.join("bun.lockb").is_file() {
        return Some("bun".into());
    }
    if root.join("package.json").is_file() {
        return package_manager_field(root);
    }
    None
}

/// Read `package.json#packageManager` and strip the version suffix
/// (`"pnpm@9.1.0"` → `"pnpm"`). `None` when absent / unreadable / blank.
fn package_manager_field(root: &Path) -> Option<String> {
    let text = fs::read_to_string(root.join("package.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let raw = json.get("packageManager").and_then(serde_json::Value::as_str)?;
    let name = raw.split('@').next().unwrap_or("").trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// The conventional script set for a JS/TS package manager. `build`/`lint` go
/// through `run` (universal across npm/pnpm/yarn); `test` is the universal
/// shortcut; `type_check` is the manager-agnostic `tsc --noEmit`.
fn js_commands(pm: &str) -> Commands {
    Commands {
        build: Some(format!("{pm} run build")),
        test: Some(format!("{pm} test")),
        lint: Some(format!("{pm} run lint")),
        type_check: Some("tsc --noEmit".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn touch(dir: &Path, name: &str, body: &str) {
        std::fs::write(dir.join(name), body).unwrap();
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
    fn unknown_stack_is_neutral_fallback() {
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
}
