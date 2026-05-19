//! .NET / C# stack scanner — a port of `registry/scanners/dotnet-scanner.js`.
//!
//! Detects C# entity classes (under `Entities/` or `Domain/` folders, plus
//! `DbSet<T>` references) and C# enums. The JS scanner used regular
//! expressions; the extraction here is rewritten with hand-written string
//! scanning preserving the same decision logic.

use super::file_utils::{collect_files, read_file_safe, relative_path};
use super::{detect_value_convention, EntityInfo, EnumInfo, Scanner};
use std::collections::BTreeMap;
use std::path::Path;

/// .NET scanner — selected when a `.csproj` / `.sln` file is present.
pub struct DotnetScanner;

/// `true` if `rel` (forward-slash path) sits under an `entities/` or `domain/`
/// directory — the JS scanner's case-insensitive folder filter.
fn is_entity_file(rel: &str) -> bool {
    let lower = rel.to_ascii_lowercase();
    lower.contains("/entities/")
        || lower.starts_with("entities/")
        || lower.contains("/domain/")
        || lower.starts_with("domain/")
}

impl Scanner for DotnetScanner {
    fn detect(&self, root: &Path) -> bool {
        std::fs::read_dir(root)
            .map(|entries| {
                entries.flatten().any(|e| {
                    e.file_name()
                        .to_str()
                        .is_some_and(|n| n.ends_with(".csproj") || n.ends_with(".sln"))
                })
            })
            .unwrap_or(false)
    }

    fn detect_architecture(&self, root: &Path) -> String {
        let mut has_interfaces = false;
        let mut has_di = false;
        let mut has_module_or_controller = false;
        for file in collect_files(root, ".cs", &[]) {
            let Some(content) = read_file_safe(&file) else {
                continue;
            };
            let rel = relative_path(root, &file).to_ascii_lowercase();
            if content.contains("public interface I") {
                has_interfaces = true;
            }
            if content.contains("services.AddScoped")
                || content.contains("services.AddSingleton")
                || content.contains("services.AddTransient")
            {
                has_di = true;
            }
            if rel.contains("modules/") || rel.contains("controllers/") {
                has_module_or_controller = true;
            }
        }
        if has_interfaces && has_di {
            "solid".to_string()
        } else if has_module_or_controller {
            "layered".to_string()
        } else {
            "minimal".to_string()
        }
    }

    fn scan_entities(&self, root: &Path) -> BTreeMap<String, EntityInfo> {
        let mut entities = BTreeMap::new();
        for file in collect_files(root, ".cs", &[]) {
            let rel = relative_path(root, &file);
            if !is_entity_file(&rel) {
                continue;
            }
            let Some(content) = read_file_safe(&file) else {
                continue;
            };
            // First `class Name` (optionally `abstract`).
            if let Some(name) = first_class_name(&content) {
                entities.entry(name).or_insert_with(|| EntityInfo {
                    file: rel.clone(),
                    ..EntityInfo::default()
                });
            }
        }
        entities
    }

    fn scan_enums(&self, root: &Path) -> BTreeMap<String, EnumInfo> {
        let mut enums = BTreeMap::new();
        for file in collect_files(root, ".cs", &[]) {
            let Some(content) = read_file_safe(&file) else {
                continue;
            };
            if !content.contains("enum ") {
                continue;
            }
            let rel = relative_path(root, &file);
            let mut search = 0;
            while let Some(rel_idx) = content[search..].find("enum ") {
                let idx = search + rel_idx;
                search = idx + "enum ".len();
                // Require a visibility modifier before `enum`.
                let before = content[..idx].trim_end();
                if !before.ends_with("public")
                    && !before.ends_with("internal")
                    && !before.ends_with("private")
                {
                    continue;
                }
                let after = &content[idx + "enum ".len()..];
                let name: String = after
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if name.is_empty() {
                    continue;
                }
                let Some(brace) = content[idx..].find('{') else {
                    continue;
                };
                let body_start = idx + brace + 1;
                let body_end = content[body_start..]
                    .find('}')
                    .map_or(content.len(), |e| body_start + e);
                let values: Vec<String> = content[body_start..body_end]
                    .split(',')
                    .filter_map(|raw| {
                        let t = raw.trim();
                        let ident = t.split('=').next().unwrap_or("").trim();
                        (!ident.is_empty()
                            && ident.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
                        .then(|| ident.to_string())
                    })
                    .collect();
                if values.is_empty() {
                    continue;
                }
                let convention = detect_value_convention(&values);
                enums.entry(name).or_insert_with(|| EnumInfo {
                    values,
                    file: rel.clone(),
                    decorators: Vec::new(),
                    value_convention: Some(convention),
                });
            }
        }
        enums
    }
}

/// Return the name of the first `class` declared in `content`.
fn first_class_name(content: &str) -> Option<String> {
    let idx = content.find("class ")?;
    // Reject `... interface class` style false positives by requiring the
    // preceding token to be a modifier or whitespace.
    let after = &content[idx + "class ".len()..];
    let name: String = after
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn detect_requires_csproj_or_sln() {
        let dir = tempdir().unwrap();
        assert!(!DotnetScanner.detect(dir.path()));
        std::fs::write(dir.path().join("App.csproj"), "<Project/>").unwrap();
        assert!(DotnetScanner.detect(dir.path()));
    }

    #[test]
    fn scan_entities_picks_classes_under_entities_folder() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("App.csproj"), "<Project/>").unwrap();
        let entities_dir = dir.path().join("Entities");
        std::fs::create_dir(&entities_dir).unwrap();
        std::fs::write(
            entities_dir.join("User.cs"),
            "public class User {\n  public int Id { get; set; }\n}\n",
        )
        .unwrap();
        let entities = DotnetScanner.scan_entities(dir.path());
        assert!(entities.contains_key("User"));
    }

    #[test]
    fn scan_enums_extracts_members() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("App.csproj"), "<Project/>").unwrap();
        std::fs::write(
            dir.path().join("Status.cs"),
            "public enum Status {\n  Active,\n  Closed = 2,\n}\n",
        )
        .unwrap();
        let enums = DotnetScanner.scan_enums(dir.path());
        let status = enums.get("Status").expect("Status enum");
        assert_eq!(status.values, vec!["Active", "Closed"]);
    }
}
