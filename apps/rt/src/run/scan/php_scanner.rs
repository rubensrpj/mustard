//! PHP / Laravel stack scanner — a port of `registry/scanners/php-scanner.js`.
//!
//! Detects Eloquent models (`class X extends Model`) and PHP 8.1+ `enum`
//! declarations. The JS scanner used regular expressions; the extraction here
//! is rewritten with hand-written string scanning preserving the same logic.

use super::file_utils::{collect_files, read_file_safe, relative_path};
use super::{detect_value_convention, EntityInfo, EnumInfo, Scanner};
use std::collections::BTreeMap;
use std::path::Path;

/// PHP scanner — selected when `composer.json` or `artisan` is present.
pub struct PhpScanner;

impl Scanner for PhpScanner {
    fn detect(&self, root: &Path) -> bool {
        root.join("composer.json").exists() || root.join("artisan").exists()
    }

    fn detect_architecture(&self, root: &Path) -> String {
        let has = |p: &str| root.join(p).exists();
        if has("app/Domain") || has("src/Domain") {
            "layered".to_string()
        } else if has("artisan") {
            "laravel-default".to_string()
        } else {
            "minimal".to_string()
        }
    }

    fn scan_entities(&self, root: &Path) -> BTreeMap<String, EntityInfo> {
        let mut entities = BTreeMap::new();
        for file in collect_files(root, ".php", &[]) {
            let Some(content) = read_file_safe(&file) else {
                continue;
            };
            // Eloquent: `class Name extends Model` (or `Authenticatable`).
            if !content.contains("extends Model")
                && !content.contains("extends Authenticatable")
            {
                continue;
            }
            let rel = relative_path(root, &file);
            let mut search = 0;
            while let Some(rel_idx) = content[search..].find("class ") {
                let idx = search + rel_idx;
                search = idx + "class ".len();
                let after = &content[search..];
                let name: String = after
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if name.is_empty() {
                    continue;
                }
                let rest = after[name.len()..].trim_start();
                if !rest.starts_with("extends ") {
                    continue;
                }
                let base = rest["extends ".len()..]
                    .split_whitespace()
                    .next()
                    .unwrap_or("");
                if base == "Model" || base == "Authenticatable" {
                    entities.entry(name).or_insert_with(|| EntityInfo {
                        file: rel.clone(),
                        base_class: Some(base.to_string()),
                        decorators: vec!["Eloquent".to_string()],
                        ..EntityInfo::default()
                    });
                }
            }
        }
        entities
    }

    fn scan_enums(&self, root: &Path) -> BTreeMap<String, EnumInfo> {
        let mut enums = BTreeMap::new();
        for file in collect_files(root, ".php", &[]) {
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
                // Cases: `case NAME = 'value';` or `case NAME;`.
                let values: Vec<String> = content[body_start..body_end]
                    .lines()
                    .filter_map(|line| {
                        let t = line.trim();
                        let rest = t.strip_prefix("case ")?;
                        let ident: String = rest
                            .chars()
                            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                            .collect();
                        (!ident.is_empty()).then_some(ident)
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn detect_requires_composer_or_artisan() {
        let dir = tempdir().unwrap();
        assert!(!PhpScanner.detect(dir.path()));
        std::fs::write(dir.path().join("composer.json"), "{}").unwrap();
        assert!(PhpScanner.detect(dir.path()));
    }

    #[test]
    fn scan_entities_extracts_eloquent_model() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("composer.json"), "{}").unwrap();
        std::fs::write(
            dir.path().join("User.php"),
            "<?php\nclass User extends Model {\n}\n",
        )
        .unwrap();
        let entities = PhpScanner.scan_entities(dir.path());
        let user = entities.get("User").expect("User entity");
        assert_eq!(user.base_class.as_deref(), Some("Model"));
    }

    #[test]
    fn scan_enums_extracts_cases() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("composer.json"), "{}").unwrap();
        std::fs::write(
            dir.path().join("Status.php"),
            "<?php\nenum Status: string {\n  case Active = 'active';\n  case Closed = 'closed';\n}\n",
        )
        .unwrap();
        let enums = PhpScanner.scan_enums(dir.path());
        let status = enums.get("Status").expect("Status enum");
        assert_eq!(status.values, vec!["Active", "Closed"]);
    }
}
