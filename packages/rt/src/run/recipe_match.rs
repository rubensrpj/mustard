//! `mustard-rt run recipe-match` — a port of `scripts/recipe-match.js`.
//!
//! Matches a recipe from `.claude/recipes/` by entity and operation, then
//! resolves the recipe's file-path placeholders. Outputs the matched recipe as
//! pretty JSON; emits nothing (exit 0) when there is no match or no recipes
//! directory.

use serde_json::{json, Value};
use std::path::Path;

/// Uppercase the first letter (input is assumed PascalCase already).
fn to_pascal_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Look for a directory at `cwd` level matching a placeholder convention.
fn find_dir_by_convention(cwd: &Path, placeholder: &str) -> Option<String> {
    let candidates: &[&str] = match placeholder {
        "backend" => &["backend", "Backend", "api", "Api", "server", "Server", "src"],
        "frontend" => &[
            "frontend", "Frontend", "web", "Web", "client", "Client", "app", "App",
        ],
        "admin" => &["admin", "Admin", "dashboard", "Dashboard"],
        _ => &[],
    };
    let list: Vec<&str> = if candidates.is_empty() {
        vec![placeholder]
    } else {
        candidates.to_vec()
    };
    for name in list {
        let candidate = cwd.join(name);
        if candidate.is_dir() {
            return Some(name.to_string());
        }
    }
    None
}

/// Resolve `{Entity}`, `{entity}`, `{subproject}`, `{backend}` etc. placeholders.
fn resolve_pattern(pattern: &str, entity: &str, subproject: Option<&str>, cwd: &Path) -> String {
    let entity_pascal = to_pascal_case(entity);
    let entity_lower = entity.to_lowercase();
    let mut resolved = pattern
        .replace("{Entity}", &entity_pascal)
        .replace("{entity}", &entity_lower);
    if let Some(sub) = subproject {
        resolved = resolved.replace("{subproject}", sub);
    }
    for placeholder in ["backend", "frontend", "admin"] {
        let token = format!("{{{placeholder}}}");
        if resolved.contains(&token) {
            if let Some(found) = find_dir_by_convention(cwd, placeholder) {
                resolved = resolved.replace(&token, &found);
            }
        }
    }
    resolved
}

/// Dispatch `mustard-rt run recipe-match`.
pub fn run(entity: Option<&str>, operation: Option<&str>, subproject: Option<&str>) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let (Some(entity), Some(operation)) = (entity, operation) else {
        return; // exit 0 silently
    };

    let recipes_dir = cwd.join(".claude").join("recipes");
    if !recipes_dir.exists() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(&recipes_dir) else {
        return;
    };

    let operation_lower = operation.to_lowercase();
    let mut json_files: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect();
    json_files.sort();

    let mut matched: Option<Value> = None;
    for file in json_files {
        let Ok(raw) = std::fs::read_to_string(&file) else {
            continue;
        };
        let Ok(recipe) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let Some(operations) = recipe.get("operations").and_then(Value::as_array) else {
            continue;
        };
        let op_matches = operations
            .iter()
            .filter_map(Value::as_str)
            .any(|op| op.to_lowercase() == operation_lower);
        if !op_matches {
            continue;
        }
        // `requires_entity` with no entity → skip; entity is always present here.
        matched = Some(recipe);
        break;
    }

    let Some(matched) = matched else {
        return; // no match — exit 0 silently
    };

    let resolved_files: Vec<Value> = matched
        .get("files")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|f| {
                    let pattern = f.get("pattern").and_then(Value::as_str).unwrap_or("");
                    json!({
                        "resolved_path": resolve_pattern(pattern, entity, subproject, &cwd),
                        "action": f.get("action").cloned().unwrap_or(Value::Null),
                        "hint": f.get("hint").cloned().unwrap_or(Value::Null),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let output = json!({
        "recipe": matched.get("name").cloned().unwrap_or(Value::Null),
        "entity": entity,
        "operation": operation,
        "description": matched.get("description").and_then(Value::as_str).unwrap_or(""),
        "files": resolved_files,
        "checklist": matched
            .get("checklist")
            .filter(|v| v.is_array())
            .cloned()
            .unwrap_or_else(|| json!([])),
    });

    println!(
        "{}",
        serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string())
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn pascal_case_uppercases_first() {
        assert_eq!(to_pascal_case("user"), "User");
        assert_eq!(to_pascal_case("Order"), "Order");
    }

    #[test]
    fn resolve_pattern_substitutes_entity() {
        let dir = tempdir().unwrap();
        let out = resolve_pattern("src/{entity}/{Entity}.ts", "user", None, dir.path());
        assert_eq!(out, "src/user/User.ts");
    }
}
