//! npm-registry version queries and semver comparison.
//!
//! Ported from `services/npm.ts`. The JS module shelled out to `npm view` for
//! the latest version and compared three-part versions by hand; this keeps
//! both behaviours.
//!
//! Wave 1 needs only [`compare_versions`] (and exposes [`get_latest_version`]
//! for the Wave 2 `auto-update` port). No HTTP client is pulled in: the
//! version query still shells out to the `npm` binary, which avoids a registry
//! API dependency in this wave.

use std::process::Command;

use anyhow::{Context, Result, bail};

/// The npm package name the CLI is published under.
pub const PACKAGE_NAME: &str = "mustard-claude";

/// Query the npm registry for the latest published version of the CLI.
///
/// Shells out to `npm view <pkg> version`, exactly as `services/npm.ts` did.
/// Returns an error if `npm` is missing, the network is down, or the registry
/// reports nothing.
pub fn get_latest_version() -> Result<String> {
    let output = Command::new("npm")
        .args(["view", PACKAGE_NAME, "version"])
        .output()
        .context("failed to run `npm` — is it installed and on PATH?")?;

    if !output.status.success() {
        bail!("failed to check npm registry. Are you online?");
    }

    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if version.is_empty() {
        bail!("npm registry returned no version for {PACKAGE_NAME}");
    }
    Ok(version)
}

/// Install the latest published CLI globally via `npm install -g`.
///
/// Ported from `updateGlobal` in `services/npm.ts`. Shells out to `npm`; a
/// non-zero exit becomes an error advising elevated permissions, matching the
/// JS message.
pub fn update_global() -> Result<()> {
    let status = Command::new("npm")
        .args(["install", "-g", &format!("{PACKAGE_NAME}@latest")])
        .status()
        .context("failed to run `npm` — is it installed and on PATH?")?;

    if !status.success() {
        bail!("failed to update. Try running with sudo or as administrator.");
    }
    Ok(())
}

/// Compare two dotted versions by their first three numeric components.
///
/// Returns [`std::cmp::Ordering`]: `Less` when `a < b`, `Greater` when
/// `a > b`, `Equal` otherwise. Missing components count as `0`, and any
/// non-numeric component also counts as `0` — matching the JS `Number(x) || 0`
/// coercion (`NaN` there collapsed to a falsy `0` in the comparison).
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parts_a = numeric_parts(a);
    let parts_b = numeric_parts(b);
    for i in 0..3 {
        match parts_a[i].cmp(&parts_b[i]) {
            std::cmp::Ordering::Equal => {}
            non_equal => return non_equal,
        }
    }
    std::cmp::Ordering::Equal
}

/// Parse a dotted version string into exactly three `u64` components, padding
/// with `0` and treating any non-numeric component as `0`.
fn numeric_parts(version: &str) -> [u64; 3] {
    let mut parts = [0u64; 3];
    for (slot, raw) in parts.iter_mut().zip(version.split('.')) {
        *slot = raw.trim().parse().unwrap_or(0);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    #[test]
    fn equal_versions() {
        assert_eq!(compare_versions("1.2.3", "1.2.3"), Ordering::Equal);
    }

    #[test]
    fn ordered_versions() {
        assert_eq!(compare_versions("1.2.3", "1.2.4"), Ordering::Less);
        assert_eq!(compare_versions("2.0.0", "1.9.9"), Ordering::Greater);
    }

    #[test]
    fn missing_components_count_as_zero() {
        assert_eq!(compare_versions("1.2", "1.2.0"), Ordering::Equal);
        assert_eq!(compare_versions("1", "1.0.1"), Ordering::Less);
    }

    #[test]
    fn non_numeric_components_collapse_to_zero() {
        assert_eq!(compare_versions("1.x.3", "1.0.3"), Ordering::Equal);
    }
}
