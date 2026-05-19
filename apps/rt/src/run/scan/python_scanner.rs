//! Python stack scanner — a port of `registry/scanners/python-scanner.js`.
//!
//! Detects SQLAlchemy / Django / SQLModel entities and `Enum` / `IntEnum`
//! classes. The JS scanner used regular expressions; the extraction here is
//! rewritten with hand-written string scanning preserving the same logic.

use super::file_utils::{collect_files, read_file_safe, relative_path};
use super::{detect_value_convention, EntityInfo, EnumInfo, Scanner};
use std::collections::BTreeMap;
use std::path::Path;

/// Python scanner — selected when a Python project manifest is present.
pub struct PythonScanner;

/// Scan a `class Name(Base):` declaration head and return the class name when
/// `base_names` contains the parenthesised base — a shared head parser.
fn class_with_base(content: &str, idx: usize, base_names: &[&str]) -> Option<String> {
    let after = &content[idx + "class ".len()..];
    let name: String = after
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() {
        return None;
    }
    let rest = after[name.len()..].trim_start();
    let inside = rest.strip_prefix('(')?;
    let close = inside.find(')')?;
    let bases = &inside[..close];
    let normalized: Vec<&str> = bases.split(',').map(str::trim).collect();
    if base_names
        .iter()
        .any(|b| normalized.iter().any(|n| n == b))
    {
        Some(name)
    } else {
        None
    }
}

impl Scanner for PythonScanner {
    fn detect(&self, root: &Path) -> bool {
        ["pyproject.toml", "setup.py", "requirements.txt", "manage.py"]
            .iter()
            .any(|f| root.join(f).exists())
    }

    fn detect_architecture(&self, root: &Path) -> String {
        let mut dirs: Vec<String> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(root) {
            for e in entries.flatten() {
                if e.path().is_dir() {
                    if let Some(name) = e.file_name().to_str() {
                        dirs.push(name.to_string());
                        // One level deeper, for src-layout projects.
                        if let Ok(nested) = std::fs::read_dir(e.path()) {
                            for n in nested.flatten() {
                                if n.path().is_dir() {
                                    if let Some(nn) = n.file_name().to_str() {
                                        dirs.push(nn.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        let has = |d: &str| dirs.iter().any(|x| x == d);
        if has("domain") && has("application") && has("infrastructure") {
            "clean-architecture".to_string()
        } else if has("repositories") && has("services") {
            "repository-service".to_string()
        } else if (has("routers") || has("views")) && has("models") && has("schemas") {
            "layered".to_string()
        } else {
            "minimal".to_string()
        }
    }

    fn scan_entities(&self, root: &Path) -> BTreeMap<String, EntityInfo> {
        let mut entities = BTreeMap::new();
        for file in collect_files(root, ".py", &[]) {
            let Some(content) = read_file_safe(&file) else {
                continue;
            };
            let rel = relative_path(root, &file);
            let mut search = 0;
            while let Some(rel_idx) = content[search..].find("class ") {
                let idx = search + rel_idx;
                search = idx + "class ".len();
                // SQLAlchemy: class X(Base | DeclarativeBase | Model).
                if let Some(name) = class_with_base(&content, idx, &["Base", "DeclarativeBase"]) {
                    entities.entry(name).or_insert_with(|| EntityInfo {
                        file: rel.clone(),
                        base_class: Some("Base".to_string()),
                        decorators: vec!["SQLAlchemy".to_string()],
                        ..EntityInfo::default()
                    });
                    continue;
                }
                // Django: class X(models.Model).
                if let Some(name) = class_with_base(&content, idx, &["models.Model"]) {
                    entities.entry(name).or_insert_with(|| EntityInfo {
                        file: rel.clone(),
                        base_class: Some("models.Model".to_string()),
                        decorators: vec!["Django".to_string()],
                        ..EntityInfo::default()
                    });
                }
            }
        }
        entities
    }

    fn scan_enums(&self, root: &Path) -> BTreeMap<String, EnumInfo> {
        let mut enums = BTreeMap::new();
        for file in collect_files(root, ".py", &[]) {
            let Some(content) = read_file_safe(&file) else {
                continue;
            };
            let rel = relative_path(root, &file);
            let mut search = 0;
            while let Some(rel_idx) = content[search..].find("class ") {
                let idx = search + rel_idx;
                search = idx + "class ".len();
                let Some(name) =
                    class_with_base(&content, idx, &["Enum", "IntEnum", "str, Enum"])
                else {
                    continue;
                };
                // Members are indented `NAME = value` lines after the head.
                let head_end = content[idx..].find(':').map_or(content.len(), |c| idx + c);
                let body = &content[head_end..];
                let mut values = Vec::new();
                for line in body.lines().skip(1) {
                    if !line.starts_with(char::is_whitespace) || line.trim().is_empty() {
                        if !values.is_empty() {
                            break;
                        }
                        continue;
                    }
                    let trimmed = line.trim();
                    let ident: String = trimmed
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .collect();
                    let rest = trimmed[ident.len()..].trim_start();
                    if !ident.is_empty() && !ident.starts_with('_') && rest.starts_with('=') {
                        values.push(ident);
                    }
                }
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
    fn detect_requires_python_manifest() {
        let dir = tempdir().unwrap();
        assert!(!PythonScanner.detect(dir.path()));
        std::fs::write(dir.path().join("pyproject.toml"), "").unwrap();
        assert!(PythonScanner.detect(dir.path()));
    }

    #[test]
    fn scan_entities_extracts_sqlalchemy_model() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "").unwrap();
        std::fs::write(
            dir.path().join("models.py"),
            "class User(Base):\n    id = Column(Integer)\n",
        )
        .unwrap();
        let entities = PythonScanner.scan_entities(dir.path());
        let user = entities.get("User").expect("User entity");
        assert_eq!(user.base_class.as_deref(), Some("Base"));
    }

    #[test]
    fn scan_enums_extracts_enum_members() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "").unwrap();
        std::fs::write(
            dir.path().join("status.py"),
            "class Status(str, Enum):\n    ACTIVE = 'active'\n    CLOSED = 'closed'\n",
        )
        .unwrap();
        let enums = PythonScanner.scan_enums(dir.path());
        let status = enums.get("Status").expect("Status enum");
        assert_eq!(status.values, vec!["ACTIVE", "CLOSED"]);
    }
}
