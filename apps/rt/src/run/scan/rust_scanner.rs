//! Rust stack scanner — a port of `registry/scanners/rust-scanner.js`.
//!
//! Scans Rust projects for entities (ORM-derived structs) and enums, and
//! infers the architecture. The JS scanner used regular expressions; the
//! `mustard-rt` crate has no `regex` dependency, so the extraction is rewritten
//! with hand-written string scanning that preserves the same decision logic
//! and output fields.

use super::file_utils::{collect_files, infer_common_folder, read_file_safe, relative_path};
use super::{detect_value_convention, EntityInfo, EnumInfo, Scanner};
use std::collections::BTreeMap;
use std::path::Path;

/// Rust scanner — selected when a `Cargo.toml` is present.
pub struct RustScanner;

/// Parse `Cargo.toml` and return the set of dependency names (lower-cased).
///
/// A line-by-line parser over `[dependencies]`, `[dev-dependencies]` and
/// `[build-dependencies]` — a faithful port of `parseCargoToml()`.
fn parse_cargo_toml(content: &str) -> Vec<String> {
    let mut deps = Vec::new();
    let mut in_deps = false;
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_deps = matches!(
                line,
                "[dependencies]" | "[dev-dependencies]" | "[build-dependencies]"
            );
            continue;
        }
        if !in_deps || line.is_empty() || line.starts_with('#') {
            continue;
        }
        // `package = "version"` or `package = { … }` — capture the key.
        if let Some(eq) = line.find('=') {
            let key = line[..eq].trim();
            if !key.is_empty()
                && key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                deps.push(key.to_ascii_lowercase());
            }
        }
    }
    deps
}

/// `true` if `deps` contains `pkg` (case-insensitive exact match).
fn has_dep(deps: &[String], pkg: &str) -> bool {
    let needle = pkg.to_ascii_lowercase();
    deps.contains(&needle)
}

/// Extract the body of the first balanced `{ … }` pair at or after `start`.
pub(crate) fn extract_brace_body(content: &str, start: usize) -> Option<String> {
    let bytes = content.as_bytes();
    let mut depth = 0usize;
    let mut body_start = 0usize;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if b == b'{' {
            if depth == 0 {
                body_start = i + 1;
            }
            depth += 1;
        } else if b == b'}' {
            depth -= 1;
            if depth == 0 {
                return content.get(body_start..i).map(str::to_string);
            }
        }
    }
    None
}

/// Read all `#[derive(...)]` macro names that appear in `attr_block`.
fn extract_derives(attr_block: &str) -> Vec<String> {
    let Some(idx) = attr_block.find("#[derive(") else {
        return Vec::new();
    };
    let after = &attr_block[idx + "#[derive(".len()..];
    let Some(close) = after.find(')') else {
        return Vec::new();
    };
    after[..close]
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// The attribute block (`#[...]` lines) immediately preceding `pos`.
fn attr_block_before(content: &str, pos: usize) -> String {
    let before = &content[..pos];
    // Walk backwards over whitespace and `#[...]` lines.
    let mut start = before.len();
    let bytes = before.as_bytes();
    loop {
        // Skip trailing whitespace.
        while start > 0 && bytes[start - 1].is_ascii_whitespace() {
            start -= 1;
        }
        if start == 0 || bytes[start - 1] != b']' {
            break;
        }
        // Find the matching `#[`.
        match before[..start].rfind("#[") {
            Some(open) => start = open,
            None => break,
        }
    }
    before[start..].to_string()
}

impl RustScanner {
    /// `true` if `attr_block` marks the struct as an ORM entity.
    fn is_orm_entity(attr_block: &str, orm: &str) -> bool {
        match orm {
            "diesel" => {
                ["Queryable", "Insertable", "AsChangeset", "Identifiable"]
                    .iter()
                    .any(|d| attr_block.contains(d))
                    || attr_block.contains("diesel(")
            }
            "sqlx" => attr_block.contains("FromRow"),
            "sea-orm" => ["DeriveEntityModel", "DeriveModel", "DeriveActiveModel"]
                .iter()
                .any(|d| attr_block.contains(d)),
            _ => ["Queryable", "Insertable", "FromRow", "DeriveEntityModel"]
                .iter()
                .any(|d| attr_block.contains(d)),
        }
    }

    /// Extract `name: Type` pairs from a struct body — a port of `_extractStructFields`.
    fn extract_struct_fields(body: &str) -> Vec<String> {
        let mut fields = Vec::new();
        for raw in body.split([',', '\n']) {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            let line = line.strip_prefix("pub ").unwrap_or(line).trim();
            if let Some(colon) = line.find(':') {
                let name = line[..colon].trim();
                let ty = line[colon + 1..].trim().trim_end_matches(',');
                if name.is_empty() || name == "pub" || name == "fn" || ty.is_empty() {
                    continue;
                }
                if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    fields.push(format!("{name}: {ty}"));
                }
            }
        }
        fields
    }

    /// Read `Cargo.toml` deps for the subproject.
    fn cargo_deps(root: &Path) -> Vec<String> {
        read_file_safe(&root.join("Cargo.toml"))
            .map(|c| parse_cargo_toml(&c))
            .unwrap_or_default()
    }

    /// Detect the ORM in use — a port of `_detectORM`.
    fn detect_orm(root: &Path) -> &'static str {
        let deps = Self::cargo_deps(root);
        if has_dep(&deps, "diesel") {
            "diesel"
        } else if has_dep(&deps, "sqlx") {
            "sqlx"
        } else if has_dep(&deps, "sea-orm") {
            "sea-orm"
        } else {
            "none"
        }
    }
}

impl Scanner for RustScanner {
    fn detect(&self, root: &Path) -> bool {
        root.join("Cargo.toml").exists()
    }

    fn detect_architecture(&self, root: &Path) -> String {
        let src = root.join("src");
        let has = |dir: &str| src.join(dir).exists() || root.join(dir).exists();
        if has("domain") && has("application") && has("infrastructure") {
            return "clean-architecture".to_string();
        }
        if has("handlers") && has("services") && has("repositories") {
            return "layered".to_string();
        }
        let main_content = read_file_safe(&src.join("main.rs"))
            .or_else(|| read_file_safe(&src.join("lib.rs")))
            .unwrap_or_default();
        let mod_count = main_content
            .lines()
            .filter(|l| l.trim_start().starts_with("mod ") || l.trim_start().starts_with("pub mod "))
            .count();
        if mod_count >= 3 {
            "modular".to_string()
        } else {
            "minimal".to_string()
        }
    }

    fn scan_entities(&self, root: &Path) -> BTreeMap<String, EntityInfo> {
        let mut entities = BTreeMap::new();
        let orm = Self::detect_orm(root);
        for file in collect_files(root, ".rs", &[]) {
            let Some(content) = read_file_safe(&file) else {
                continue;
            };
            let rel = relative_path(root, &file);
            // Find every `pub struct Name {` occurrence.
            let mut search = 0;
            while let Some(rel_idx) = content[search..].find("pub struct ") {
                let idx = search + rel_idx;
                let after = &content[idx + "pub struct ".len()..];
                let name: String = after
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                search = idx + "pub struct ".len();
                if name.is_empty() {
                    continue;
                }
                // The struct must be followed (after optional generics) by `{`.
                let brace = match content[idx..].find('{') {
                    Some(b) => idx + b,
                    None => continue,
                };
                let attr_block = attr_block_before(&content, idx);
                if !Self::is_orm_entity(&attr_block, orm) {
                    continue;
                }
                let derives = extract_derives(&attr_block);
                let properties = extract_brace_body(&content, brace)
                    .map(|b| Self::extract_struct_fields(&b))
                    .unwrap_or_default();
                let mut info = EntityInfo {
                    file: rel.clone(),
                    decorators: derives,
                    properties,
                    ..EntityInfo::default()
                };
                // `table_name = "..."` in a diesel / sea-orm attribute.
                if let Some(tn) = attr_block.find("table_name") {
                    if let Some(q1) = attr_block[tn..].find('"') {
                        let rest = &attr_block[tn + q1 + 1..];
                        if let Some(q2) = rest.find('"') {
                            info.table_name = Some(rest[..q2].to_string());
                        }
                    }
                }
                entities.insert(name, info);
            }
        }
        entities
    }

    fn scan_enums(&self, root: &Path) -> BTreeMap<String, EnumInfo> {
        let mut enums = BTreeMap::new();
        for file in collect_files(root, ".rs", &[]) {
            let Some(content) = read_file_safe(&file) else {
                continue;
            };
            let rel = relative_path(root, &file);
            let mut search = 0;
            while let Some(rel_idx) = content[search..].find("pub enum ") {
                let idx = search + rel_idx;
                let after = &content[idx + "pub enum ".len()..];
                let name: String = after
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                search = idx + "pub enum ".len();
                if name.is_empty() {
                    continue;
                }
                let brace = match content[idx..].find('{') {
                    Some(b) => idx + b,
                    None => continue,
                };
                let Some(body) = extract_brace_body(&content, brace) else {
                    continue;
                };
                // Variants: lines starting with an uppercase identifier.
                let variants: Vec<String> = body
                    .lines()
                    .filter_map(|line| {
                        let t = line.trim().trim_end_matches(',');
                        let ident: String = t
                            .chars()
                            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                            .collect();
                        if !ident.is_empty()
                            && ident.starts_with(|c: char| c.is_ascii_uppercase())
                            && (t == ident
                                || t[ident.len()..]
                                    .starts_with(['(', '{']))
                        {
                            Some(ident)
                        } else {
                            None
                        }
                    })
                    .collect();
                if variants.is_empty() {
                    continue;
                }
                let attr_block = attr_block_before(&content, idx);
                let convention = detect_value_convention(&variants);
                enums.insert(
                    name,
                    EnumInfo {
                        values: variants,
                        file: rel.clone(),
                        decorators: extract_derives(&attr_block),
                        value_convention: Some(convention),
                    },
                );
            }
        }
        enums
    }
}

/// Re-export so the registry wave can reuse the common-folder helper.
#[allow(dead_code)]
pub(crate) fn entity_folder(entities: &BTreeMap<String, EntityInfo>) -> Option<String> {
    let files: Vec<String> = entities.values().map(|e| e.file.clone()).collect();
    infer_common_folder(&files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn detect_requires_cargo_toml() {
        let dir = tempdir().unwrap();
        assert!(!RustScanner.detect(dir.path()));
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        assert!(RustScanner.detect(dir.path()));
    }

    #[test]
    fn scan_entities_extracts_diesel_struct() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[dependencies]\ndiesel = \"2\"\n",
        )
        .unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(
            src.join("models.rs"),
            "#[derive(Queryable, Debug)]\n#[diesel(table_name = \"users\")]\n\
             pub struct User {\n    pub id: i32,\n    pub name: String,\n}\n",
        )
        .unwrap();

        let entities = RustScanner.scan_entities(dir.path());
        let user = entities.get("User").expect("User entity");
        assert_eq!(user.file, "src/models.rs");
        assert!(user.decorators.contains(&"Queryable".to_string()));
        assert_eq!(user.table_name.as_deref(), Some("users"));
        assert!(user.properties.contains(&"id: i32".to_string()));
        assert!(user.properties.contains(&"name: String".to_string()));
    }

    #[test]
    fn scan_entities_skips_non_orm_struct() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(
            src.join("plain.rs"),
            "pub struct Helper {\n    pub x: i32,\n}\n",
        )
        .unwrap();
        assert!(RustScanner.scan_entities(dir.path()).is_empty());
    }

    #[test]
    fn scan_enums_extracts_variants() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(
            src.join("status.rs"),
            "#[derive(Debug, Clone)]\npub enum Status {\n    Active,\n    Pending,\n}\n",
        )
        .unwrap();
        let enums = RustScanner.scan_enums(dir.path());
        let status = enums.get("Status").expect("Status enum");
        assert_eq!(status.values, vec!["Active", "Pending"]);
        assert!(status.decorators.contains(&"Debug".to_string()));
    }
}
