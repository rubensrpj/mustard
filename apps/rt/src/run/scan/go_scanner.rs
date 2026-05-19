//! Go stack scanner — a port of `registry/scanners/go-scanner.js`.
//!
//! Detects GORM entities and `iota` / string-const enum families. The JS
//! scanner used regular expressions; the extraction here is rewritten with
//! hand-written string scanning preserving the same decision logic.

use super::file_utils::{collect_files, read_file_safe, relative_path};
use super::rust_scanner::extract_brace_body;
use super::{detect_value_convention, EntityInfo, EnumInfo, Scanner};
use std::collections::BTreeMap;
use std::path::Path;

/// Go scanner — selected when a `go.mod` is present.
pub struct GoScanner;

/// Locate every `type Name struct {` head and yield `(name, brace_index)`.
fn struct_heads(content: &str) -> Vec<(String, usize)> {
    let mut heads = Vec::new();
    let mut search = 0;
    while let Some(rel_idx) = content[search..].find("type ") {
        let idx = search + rel_idx;
        search = idx + "type ".len();
        let after = &content[search..];
        let name: String = after
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() {
            continue;
        }
        let rest = after[name.len()..].trim_start();
        if let Some(body) = rest.strip_prefix("struct") {
            if body.trim_start().starts_with('{') {
                if let Some(brace) = content[idx..].find('{') {
                    heads.push((name, idx + brace));
                }
            }
        }
    }
    heads
}

/// Extract the body of the first balanced `( … )` pair at or after `start`.
fn extract_paren_body(content: &str, start: usize) -> Option<String> {
    let bytes = content.as_bytes();
    let mut depth = 0usize;
    let mut body_start = 0usize;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if b == b'(' {
            if depth == 0 {
                body_start = i + 1;
            }
            depth += 1;
        } else if b == b')' {
            depth -= 1;
            if depth == 0 {
                return content.get(body_start..i).map(str::to_string);
            }
        }
    }
    None
}

impl Scanner for GoScanner {
    fn detect(&self, root: &Path) -> bool {
        root.join("go.mod").exists()
    }

    fn detect_architecture(&self, root: &Path) -> String {
        let has = |p: &str| root.join(p).exists();
        if has("internal/domain") && has("internal/service") && has("internal/repository") {
            "clean-architecture".to_string()
        } else if has("handler") && has("service") && has("repository") {
            "solid".to_string()
        } else if has("cmd") && has("internal") {
            "standard-layout".to_string()
        } else {
            "minimal".to_string()
        }
    }

    fn scan_entities(&self, root: &Path) -> BTreeMap<String, EntityInfo> {
        let mut entities = BTreeMap::new();
        for file in collect_files(root, ".go", &[]) {
            let Some(content) = read_file_safe(&file) else {
                continue;
            };
            // GORM: structs with a `gorm:` tag or an embedded `gorm.Model`.
            if !content.contains("gorm:") && !content.contains("gorm.Model") {
                continue;
            }
            let rel = relative_path(root, &file);
            for (name, brace) in struct_heads(&content) {
                let Some(body) = extract_brace_body(&content, brace) else {
                    continue;
                };
                let has_gorm_tag = body.contains("gorm:\"");
                let has_gorm_model = body.contains("gorm.Model");
                if !has_gorm_tag && !has_gorm_model {
                    continue;
                }
                // Tagged fields: `Field Type \`gorm:"..."\``.
                let properties: Vec<String> = body
                    .lines()
                    .filter(|l| l.contains("gorm:\""))
                    .filter_map(|l| {
                        let mut parts = l.trim().split_whitespace();
                        let field = parts.next()?;
                        let ty = parts.next()?;
                        Some(format!("{field} {ty}"))
                    })
                    .collect();
                entities.insert(
                    name,
                    EntityInfo {
                        file: rel.clone(),
                        base_class: has_gorm_model.then(|| "gorm.Model".to_string()),
                        properties,
                        ..EntityInfo::default()
                    },
                );
            }
        }
        entities
    }

    fn scan_enums(&self, root: &Path) -> BTreeMap<String, EnumInfo> {
        let mut enums = BTreeMap::new();
        let int_types = [
            "int", "int8", "int16", "int32", "int64", "uint", "uint8", "uint16", "uint32",
            "uint64", "string",
        ];
        for file in collect_files(root, ".go", &[]) {
            let Some(content) = read_file_safe(&file) else {
                continue;
            };
            let rel = relative_path(root, &file);
            let mut search = 0;
            while let Some(rel_idx) = content[search..].find("type ") {
                let idx = search + rel_idx;
                search = idx + "type ".len();
                let after = &content[search..];
                let type_name: String = after
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if type_name.is_empty() {
                    continue;
                }
                let rest = after[type_name.len()..].trim_start();
                let base = rest.split_whitespace().next().unwrap_or("");
                if !int_types.contains(&base) {
                    continue;
                }
                // Find a `const ( … )` block after this type.
                let Some(const_off) = content[search..].find("const (") else {
                    continue;
                };
                let const_idx = search + const_off + "const ".len();
                let Some(block) = extract_paren_body(&content, const_idx) else {
                    continue;
                };
                let mut values = Vec::new();
                if base == "string" {
                    // `Name TypeName = "value"` lines.
                    for line in block.lines() {
                        let t = line.trim();
                        if t.contains(&type_name) && t.contains('=') {
                            let ident: String = t
                                .chars()
                                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                                .collect();
                            if !ident.is_empty() {
                                values.push(ident);
                            }
                        }
                    }
                } else {
                    // iota: first `Name TypeName = iota` then bare identifiers.
                    let mut seen_iota = false;
                    for line in block.lines() {
                        let t = line.trim();
                        if t.is_empty() {
                            continue;
                        }
                        let ident: String = t
                            .chars()
                            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                            .collect();
                        if ident.is_empty() || ident == type_name {
                            continue;
                        }
                        if seen_iota {
                            values.push(ident);
                        } else if t.contains("iota") {
                            seen_iota = true;
                            values.push(ident);
                        }
                    }
                }
                if values.is_empty() {
                    continue;
                }
                let style = if base == "string" {
                    "string-const"
                } else {
                    "iota"
                };
                let convention = detect_value_convention(&values);
                enums.insert(
                    type_name.clone(),
                    EnumInfo {
                        values,
                        file: rel.clone(),
                        decorators: vec![format!("base:{base}"), format!("style:{style}")],
                        value_convention: Some(convention),
                    },
                );
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
    fn detect_requires_go_mod() {
        let dir = tempdir().unwrap();
        assert!(!GoScanner.detect(dir.path()));
        std::fs::write(dir.path().join("go.mod"), "module x").unwrap();
        assert!(GoScanner.detect(dir.path()));
    }

    #[test]
    fn scan_entities_extracts_gorm_struct() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module x").unwrap();
        std::fs::write(
            dir.path().join("user.go"),
            "type User struct {\n  ID uint `gorm:\"primaryKey\"`\n  Name string `gorm:\"size:255\"`\n}\n",
        )
        .unwrap();
        let entities = GoScanner.scan_entities(dir.path());
        let user = entities.get("User").expect("User entity");
        assert!(user.properties.contains(&"ID uint".to_string()));
        assert!(user.properties.contains(&"Name string".to_string()));
    }

    #[test]
    fn scan_enums_extracts_iota_enum() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module x").unwrap();
        std::fs::write(
            dir.path().join("status.go"),
            "type Status int\nconst (\n  Active Status = iota\n  Closed\n)\n",
        )
        .unwrap();
        let enums = GoScanner.scan_enums(dir.path());
        let status = enums.get("Status").expect("Status enum");
        assert_eq!(status.values, vec!["Active", "Closed"]);
    }
}
