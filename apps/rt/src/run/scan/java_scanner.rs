//! Java / Spring Boot stack scanner — a port of
//! `registry/scanners/java-scanner.js`.
//!
//! Detects JPA `@Entity` classes and `public enum` declarations. The JS scanner
//! used regular expressions; the extraction here is rewritten with hand-written
//! string scanning preserving the same decision logic.

use super::file_utils::{collect_files, read_file_safe, relative_path};
use super::{detect_value_convention, EntityInfo, EnumInfo, Scanner};
use mustard_core::fs as mfs;
use std::collections::BTreeMap;
use std::path::Path;

/// Java scanner — selected when a Maven / Gradle build file is present.
pub struct JavaScanner;

impl Scanner for JavaScanner {
    fn detect(&self, root: &Path) -> bool {
        ["pom.xml", "build.gradle", "build.gradle.kts"]
            .iter()
            .any(|f| root.join(f).exists())
    }

    fn detect_architecture(&self, root: &Path) -> String {
        // Collect every directory name up to a few levels deep.
        let mut names: Vec<String> = Vec::new();
        collect_dir_names(root, 4, &mut names);
        let has = |d: &str| names.iter().any(|n| n == d);
        if has("domain") && has("application") && has("infrastructure") && has("ports") {
            "hexagonal".to_string()
        } else if has("domain") && has("application") && has("infrastructure") {
            "layered".to_string()
        } else if has("controller") && has("service") && has("repository") {
            "layered".to_string()
        } else {
            "minimal".to_string()
        }
    }

    fn scan_entities(&self, root: &Path) -> BTreeMap<String, EntityInfo> {
        let mut entities = BTreeMap::new();
        for file in collect_files(root, ".java", &[]) {
            let Some(content) = read_file_safe(&file) else {
                continue;
            };
            if !content.contains("@Entity") {
                continue;
            }
            let rel = relative_path(root, &file);
            let mut search = 0;
            while let Some(rel_idx) = content[search..].find("@Entity") {
                let idx = search + rel_idx;
                search = idx + "@Entity".len();
                // The first `class Name` after the annotation.
                if let Some(class_off) = content[idx..].find("class ") {
                    let after = &content[idx + class_off + "class ".len()..];
                    let name: String = after
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .collect();
                    if !name.is_empty() {
                        entities.entry(name).or_insert_with(|| EntityInfo {
                            file: rel.clone(),
                            decorators: vec!["Entity".to_string()],
                            ..EntityInfo::default()
                        });
                    }
                }
            }
        }
        entities
    }

    fn scan_enums(&self, root: &Path) -> BTreeMap<String, EnumInfo> {
        let mut enums = BTreeMap::new();
        for file in collect_files(root, ".java", &[]) {
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
                if !content[..idx].trim_end().ends_with("public") {
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
                // Constants live before the first `;` (or close brace).
                let body_end = content[body_start..]
                    .find(';')
                    .map_or(content.len(), |s| body_start + s);
                let block = &content[body_start..body_end];
                let values: Vec<String> = block
                    .split(',')
                    .filter_map(|raw| {
                        let t = raw.trim();
                        let ident: String = t
                            .chars()
                            .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
                            .collect();
                        (!ident.is_empty()
                            && ident.starts_with(|c: char| c.is_ascii_uppercase()))
                        .then_some(ident)
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

/// Recursively collect directory names up to `depth` levels — skips dot-dirs.
fn collect_dir_names(dir: &Path, depth: usize, out: &mut Vec<String>) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = mfs::read_dir(dir) else {
        return;
    };
    for e in entries {
        if e.is_dir {
            if e.file_name.starts_with('.') {
                continue;
            }
            out.push(e.file_name.clone());
            collect_dir_names(&e.path, depth - 1, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn detect_requires_build_file() {
        let dir = tempdir().unwrap();
        assert!(!JavaScanner.detect(dir.path()));
        std::fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();
        assert!(JavaScanner.detect(dir.path()));
    }

    #[test]
    fn scan_entities_extracts_jpa_entity() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();
        std::fs::write(
            dir.path().join("User.java"),
            "@Entity\n@Table(name=\"users\")\npublic class User {\n}\n",
        )
        .unwrap();
        let entities = JavaScanner.scan_entities(dir.path());
        assert!(entities.contains_key("User"));
    }

    #[test]
    fn scan_enums_extracts_constants() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();
        std::fs::write(
            dir.path().join("Status.java"),
            "public enum Status {\n  ACTIVE, CLOSED;\n}\n",
        )
        .unwrap();
        let enums = JavaScanner.scan_enums(dir.path());
        let status = enums.get("Status").expect("Status enum");
        assert_eq!(status.values, vec!["ACTIVE", "CLOSED"]);
    }
}
