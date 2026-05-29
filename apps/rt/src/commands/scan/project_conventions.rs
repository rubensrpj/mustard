//! Declarative project-convention inference — a port of
//! `registry/project-conventions.js`.
//!
//! Fully agnostic: every value emerges from the user's own filenames. The only
//! convention computed today is the dominant naming style of the primary
//! extension's basenames.

use super::file_utils::{collect_files, dominant_source_extension};
use serde_json::json;
use std::path::Path;

/// Lower bound / upper bound for the dominance threshold — mirrors the JS
/// `Math.max(0.5, Math.min(0.95, …))` clamp on `MUSTARD_NAMING_DOMINANCE`.
fn dominance_threshold() -> f64 {
    let raw = std::env::var("MUSTARD_NAMING_DOMINANCE")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.6);
    raw.clamp(0.5, 0.95)
}

/// Primary file extension for a known stack — mirrors `_primaryExtForStack`.
///
/// Returns `None` for an unknown stack id. Callers that must never zero on an
/// unknown stack should use [`resolve_primary_ext`], which derives the
/// project's dominant extension as an agnostic fallback.
#[must_use]
pub fn primary_ext_for_stack(stack_id: &str) -> Option<&'static str> {
    match stack_id {
        "dotnet" => Some(".cs"),
        "typescript" => Some(".ts"),
        "dart" => Some(".dart"),
        "java" => Some(".java"),
        "kotlin" => Some(".kt"),
        "go" => Some(".go"),
        "rust" => Some(".rs"),
        "python" => Some(".py"),
        "php" => Some(".php"),
        _ => None,
    }
}

/// Resolve the primary extension the cluster / convention gates operate on,
/// **never zeroing on an unknown stack** (F0-e).
///
/// Resolution order, first hit wins:
/// 1. `mustard.json#primaryExt` (explicit user pin — wins over everything).
/// 2. [`primary_ext_for_stack`] for a known stack id (known stacks are
///    byte-identical to pre-F0-e — no regression).
/// 3. [`dominant_source_extension`] of the subproject — the most frequent
///    source extension actually present, so an unknown stack discovers its own
///    dominant language instead of returning `None`.
///
/// Returns `None` only when the subproject has literally no source file (so
/// there is genuinely nothing to cluster) — that is the one legitimate empty.
#[must_use]
pub fn resolve_primary_ext(subproject_path: &Path, stack_id: &str) -> Option<String> {
    if let Some(ext) = mustard_core::ProjectConfig::load(subproject_path).primary_ext() {
        return Some(ext);
    }
    if let Some(ext) = primary_ext_for_stack(stack_id) {
        return Some(ext.to_string());
    }
    dominant_source_extension(subproject_path)
}

/// Classify a basename into a naming bucket — a port of `classifyName()`.
#[must_use]
pub fn classify_name(base: &str) -> &'static str {
    if base.is_empty() {
        return "mixed";
    }
    let first_lower = base.chars().next().is_some_and(|c| c.is_ascii_lowercase());
    let first_upper = base.chars().next().is_some_and(|c| c.is_ascii_uppercase());
    let all_alnum = base.chars().all(|c| c.is_ascii_alphanumeric());
    // kebab-case: ^[a-z][a-z0-9]*(-[a-z0-9]+)+$
    if first_lower
        && base.contains('-')
        && base
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !base.ends_with('-')
        && !base.contains("--")
    {
        return "kebab-case";
    }
    // PascalCase: ^[A-Z][a-zA-Z0-9]*$
    if first_upper && all_alnum {
        return "PascalCase";
    }
    // snake_case: ^[a-z][a-z0-9_]*$ with at least one underscore
    if first_lower
        && base.contains('_')
        && base
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return "snake_case";
    }
    // camelCase: ^[a-z][a-zA-Z0-9]*$ with at least one uppercase
    if first_lower && all_alnum && base.chars().any(|c| c.is_ascii_uppercase()) {
        return "camelCase";
    }
    // lowercase: ^[a-z][a-z0-9]*$
    if first_lower
        && base
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    {
        return "lowercase";
    }
    "mixed"
}

/// Compute the dominant naming convention — a port of `computeProjectConventions()`.
///
/// Returns a JSON object `{ "naming": { dominant, distribution, total } }`,
/// or `None` when the stack has no primary extension or no files were found
/// (the JS returns an object with `total: 0`, which the registry then drops).
#[must_use]
pub fn compute_project_conventions(subproject_path: &Path, stack_id: &str) -> serde_json::Value {
    let empty = json!({ "naming": { "dominant": serde_json::Value::Null, "distribution": {}, "total": 0 } });
    // F0-e: never zero on an unknown stack — fall back to the project's own
    // dominant source extension instead of returning `None` here.
    let Some(ext) = resolve_primary_ext(subproject_path, stack_id) else {
        return empty;
    };
    let ext = ext.as_str();
    let files = collect_files(subproject_path, ext, &[]);
    if files.is_empty() {
        return empty;
    }
    // BTreeMap keeps the distribution deterministic for tests / JSON output.
    let mut distribution: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for file in &files {
        let base = file
            .file_name()
            .and_then(|n| n.to_str())
            .map_or("", |n| n.strip_suffix(ext).unwrap_or(n));
        *distribution.entry(classify_name(base)).or_insert(0) += 1;
    }
    let total = files.len();
    // First bucket (insertion order in JS = object key order) whose share
    // crosses the threshold wins. BTreeMap iterates alphabetically; the JS
    // object iterated by first-seen order, but only one bucket can ever exceed
    // a >0.5 threshold, so the choice is unambiguous either way.
    let threshold = dominance_threshold();
    let mut dominant: serde_json::Value = serde_json::Value::Null;
    for (bucket, count) in &distribution {
        if (*count as f64) / (total as f64) >= threshold {
            dominant = serde_json::Value::String((*bucket).to_string());
            break;
        }
    }
    let dist_obj: serde_json::Map<String, serde_json::Value> = distribution
        .into_iter()
        .map(|(k, v)| (k.to_string(), serde_json::Value::from(v)))
        .collect();
    json!({
        "naming": {
            "dominant": dominant,
            "distribution": dist_obj,
            "total": total,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn classify_name_buckets() {
        assert_eq!(classify_name("user-service"), "kebab-case");
        assert_eq!(classify_name("UserService"), "PascalCase");
        assert_eq!(classify_name("user_service"), "snake_case");
        assert_eq!(classify_name("userService"), "camelCase");
        assert_eq!(classify_name("user"), "lowercase");
        assert_eq!(classify_name("User.Service"), "mixed");
    }

    #[test]
    fn dominant_when_majority_share() {
        let dir = tempdir().unwrap();
        for name in ["a-b.ts", "c-d.ts", "e-f.ts", "g-h.ts"] {
            std::fs::write(dir.path().join(name), "").unwrap();
        }
        let conv = compute_project_conventions(dir.path(), "typescript");
        assert_eq!(conv["naming"]["dominant"], "kebab-case");
        assert_eq!(conv["naming"]["total"], 4);
    }

    #[test]
    fn no_dominant_when_mixed() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a-b.ts"), "").unwrap();
        std::fs::write(dir.path().join("CdEf.ts"), "").unwrap();
        let conv = compute_project_conventions(dir.path(), "typescript");
        assert!(conv["naming"]["dominant"].is_null());
    }
}
