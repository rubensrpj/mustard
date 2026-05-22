//! Dart / Flutter stack scanner — a port of `registry/scanners/dart-scanner.js`.
//!
//! Detects models, enums, abstract interfaces, routes (GoRouter / AutoRoute /
//! GetX / Navigator 2.0), DTOs and services, and infers patterns (state
//! management, navigation, serialization strategy). The JS scanner used regular
//! expressions; the extraction here is rewritten with hand-written string
//! scanning that preserves the same decision logic.
//!
//! The JS `EntityInfo` carried Dart-specific keys (`mixins`, `interfaces`,
//! `enhanced`). The shared [`EntityInfo`] / [`EnumInfo`] contract has no such
//! fields, so this port folds that information into the contract: `with`/
//! `implements` clauses join the `decorators` list (prefixed `with:` /
//! `impl:`), and an "enhanced" enum is recorded with an `enhanced` decorator.
//! Every name/file/decorator decision matches the JS scanner field-for-field.

use super::file_utils::{collect_files, infer_common_folder, read_file_safe, relative_path};
use super::{detect_value_convention, DtoInfo, EntityInfo, EnumInfo, ScanResult, Scanner};
use mustard_core::fs as mfs;
use std::collections::BTreeMap;
use std::path::Path;

/// Dart scanner — selected when a `pubspec.yaml` is present.
pub struct DartScanner;

/// The `.dart` ignore list shared by every scan — mirrors the JS argument
/// `['.dart_tool', 'build']` passed to `collectFiles`.
const DART_IGNORE: &[&str] = &[".dart_tool", "build"];

/// `true` if a directory named `segment` exists anywhere under `base` —
/// a port of `hasDirRecursive()`. Case-insensitive; skips dot-dirs and
/// `node_modules`. Fail-open: an unreadable tree yields `false`.
fn has_dir_recursive(base: &Path, segment: &str) -> bool {
    let lower = segment.to_lowercase();
    let Ok(entries) = mfs::read_dir(base) else {
        return false;
    };
    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        let name = entry.file_name.as_str();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        if name.to_lowercase() == lower {
            return true;
        }
        if has_dir_recursive(&entry.path, segment) {
            return true;
        }
    }
    false
}

/// Read `pubspec.yaml` at `root`, returning its raw text — a port of `readPubspec`.
fn read_pubspec(root: &Path) -> Option<String> {
    read_file_safe(&root.join("pubspec.yaml"))
}

/// `true` if `pkg` appears as a dependency key in `pubspec` — a port of
/// `pubspecHas`. Matches a line of the form `  pkg:` (any indentation).
fn pubspec_has(pubspec: &str, pkg: &str) -> bool {
    pubspec.lines().any(|line| {
        let trimmed = line.trim_start();
        if trimmed.len() == line.len() {
            return false; // requires leading indentation
        }
        match trimmed.strip_prefix(pkg) {
            Some(rest) => rest.trim_start().starts_with(':'),
            None => false,
        }
    })
}

/// Scan an identifier (alphanumeric + `_`) starting at byte offset `start`.
fn ident_at(content: &str, start: usize) -> String {
    content[start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect()
}

/// Split a comma list into trimmed, non-empty tokens — for `with`/`implements`.
fn split_clause(clause: &str) -> Vec<String> {
    clause
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// One `class Name extends Base with Mixins implements Ifaces {` head.
struct ClassHead {
    name: String,
    base_class: Option<String>,
    with_clause: Vec<String>,
    implements_clause: Vec<String>,
    /// Byte offset of the `class` keyword.
    index: usize,
}

/// Parse every concrete `class … {` head in `content` — a hand-written port of
/// the JS `classRe` regex. Abstract classes/interfaces are filtered by the
/// caller (they belong to `scan_interfaces`).
fn class_heads(content: &str) -> Vec<ClassHead> {
    let mut heads = Vec::new();
    let mut search = 0;
    while let Some(rel_idx) = content[search..].find("class ") {
        let idx = search + rel_idx;
        search = idx + "class ".len();
        // `class ` must be a word boundary on the left (else it is e.g. `subclass`).
        if idx > 0 {
            let prev = content[..idx].chars().next_back();
            if prev.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
                continue;
            }
        }
        let name = ident_at(content, search);
        if name.is_empty() {
            continue;
        }
        // The class body opens at the next `{`; everything up to it is the
        // `extends`/`with`/`implements` header. Bail if there is no `{`.
        let Some(brace_rel) = content[search..].find('{') else {
            break;
        };
        let header = &content[search + name.len()..search + brace_rel];
        // Ignore generic/parametised heads that are actually expressions, not
        // declarations — a declaration header only holds the three keywords.
        let base_class = clause_value(header, "extends ").and_then(|c| c.split_whitespace().next().map(str::to_string));
        let with_clause = clause_value(header, "with ").map(|c| split_clause(&c)).unwrap_or_default();
        let implements_clause =
            clause_value(header, "implements ").map(|c| split_clause(&c)).unwrap_or_default();
        heads.push(ClassHead {
            name,
            base_class,
            with_clause,
            implements_clause,
            index: idx,
        });
    }
    heads
}

/// Extract the text following `keyword` up to the next clause keyword in a
/// class header (`extends`/`with`/`implements`).
fn clause_value(header: &str, keyword: &str) -> Option<String> {
    let start = header.find(keyword)? + keyword.len();
    let rest = &header[start..];
    let mut end = rest.len();
    for stop in [" extends ", " with ", " implements "] {
        if let Some(pos) = rest.find(stop) {
            end = end.min(pos);
        }
    }
    let value = rest[..end].trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// `true` if a class head at `head_index` is declared `abstract`.
///
/// `class_heads` already located the `class` keyword, so the modifier (if any)
/// is the whitespace-separated token(s) immediately before it. Dart spells the
/// abstract forms `abstract class X` and `abstract interface class X`; either
/// way the token just before `class` is `abstract` or `interface`, and an
/// `interface` is itself preceded by `abstract`. Scanning the trailing tokens
/// of the 100-char window is a faithful port of the JS `/abstract\s+(?:class|
/// interface)/` test without the false positives of a plain `contains`.
fn is_abstract_before(content: &str, head_index: usize) -> bool {
    let from = head_index.saturating_sub(100);
    let window = &content[from..head_index];
    let last = window.split_whitespace().next_back();
    let prev = {
        let mut it = window.split_whitespace().rev();
        it.next();
        it.next()
    };
    matches!(last, Some("abstract"))
        || (matches!(last, Some("interface")) && matches!(prev, Some("abstract")))
}

impl Scanner for DartScanner {
    fn detect(&self, root: &Path) -> bool {
        root.join("pubspec.yaml").exists()
    }

    /// A port of `detectArchitecture()` — Clean Architecture / BLoC / MVVM /
    /// MVC, falling back to `minimal` for a flat `lib/`.
    fn detect_architecture(&self, root: &Path) -> String {
        let lib = root.join("lib");
        if !lib.exists() {
            return "minimal".to_string();
        }
        if has_dir_recursive(&lib, "domain")
            && has_dir_recursive(&lib, "data")
            && has_dir_recursive(&lib, "presentation")
        {
            return "clean-architecture".to_string();
        }
        if has_dir_recursive(&lib, "bloc") || has_dir_recursive(&lib, "cubit") {
            return "bloc".to_string();
        }
        let has_view_models = has_dir_recursive(&lib, "view_model")
            || has_dir_recursive(&lib, "viewmodel")
            || has_dir_recursive(&lib, "viewmodels");
        if has_view_models && has_dir_recursive(&lib, "model") && has_dir_recursive(&lib, "view") {
            return "mvvm".to_string();
        }
        if has_dir_recursive(&lib, "controller") || has_dir_recursive(&lib, "controllers") {
            return "mvc".to_string();
        }
        "minimal".to_string()
    }

    /// A port of `scanEntities()` — concrete model/entity classes.
    fn scan_entities(&self, root: &Path) -> BTreeMap<String, EntityInfo> {
        let mut entities = BTreeMap::new();
        for file in collect_files(root, ".dart", DART_IGNORE) {
            let Some(content) = read_file_safe(&file) else {
                continue;
            };
            let rel = relative_path(root, &file);
            let rel_lower = rel.to_lowercase();
            let is_focused = rel_lower.contains("/models/")
                || rel_lower.contains("/model/")
                || rel_lower.contains("/entities/")
                || rel_lower.contains("/entity/")
                || rel_lower.contains("/domain/");
            let is_freezed = content.contains("@freezed") && content.contains("factory");
            let is_json_serializable = content.contains("@JsonSerializable()");

            for head in class_heads(&content) {
                // Abstract classes belong to `scan_interfaces`.
                if is_abstract_before(&content, head.index) {
                    continue;
                }
                let is_equatable = head.base_class.as_deref() == Some("Equatable")
                    || head.base_class.as_deref() == Some("EquatableMixin");
                // Focus heuristic — register only meaningful model classes.
                let is_model_class = is_focused
                    || head.name.ends_with("Model")
                    || head.name.ends_with("Entity")
                    || head.name.ends_with("Dto")
                    || head.name.ends_with("Request")
                    || head.name.ends_with("Response")
                    || is_freezed
                    || is_equatable;
                if !is_model_class {
                    continue;
                }
                let mut decorators = Vec::new();
                if is_freezed {
                    decorators.push("@freezed".to_string());
                }
                if is_json_serializable {
                    decorators.push("@JsonSerializable".to_string());
                }
                if is_equatable {
                    decorators.push("Equatable".to_string());
                }
                // `with`/`implements` clauses fold into `decorators` (prefixed),
                // since the shared contract has no `mixins`/`interfaces` fields.
                for mixin in &head.with_clause {
                    decorators.push(format!("with:{mixin}"));
                }
                for iface in &head.implements_clause {
                    decorators.push(format!("impl:{iface}"));
                }
                entities.insert(
                    head.name.clone(),
                    EntityInfo {
                        file: rel.clone(),
                        decorators,
                        base_class: head.base_class.clone(),
                        ..EntityInfo::default()
                    },
                );
            }
        }
        entities
    }

    /// A port of `scanEnums()` — plain and enhanced (Dart 3) enums.
    fn scan_enums(&self, root: &Path) -> BTreeMap<String, EnumInfo> {
        let mut enums = BTreeMap::new();
        for file in collect_files(root, ".dart", DART_IGNORE) {
            let Some(content) = read_file_safe(&file) else {
                continue;
            };
            let rel = relative_path(root, &file);
            let mut search = 0;
            while let Some(rel_idx) = content[search..].find("enum ") {
                let idx = search + rel_idx;
                search = idx + "enum ".len();
                // `enum ` must be a word boundary on the left.
                if idx > 0 {
                    let prev = content[..idx].chars().next_back();
                    if prev.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
                        continue;
                    }
                }
                let name = ident_at(&content, search);
                if name.is_empty() {
                    continue;
                }
                let Some(brace_rel) = content[search..].find('{') else {
                    continue;
                };
                let brace = search + brace_rel;
                let Some(close_rel) = content[brace..].find('}') else {
                    continue;
                };
                let body = &content[brace + 1..brace + close_rel];
                // Values: split on `,`/`;`, drop comments / consts / methods.
                let values: Vec<String> = body
                    .split([',', ';'])
                    .map(str::trim)
                    .filter(|v| {
                        !v.is_empty()
                            && !v.starts_with("//")
                            && !v.starts_with("const ")
                            && !v.starts_with("final ")
                            && !v.contains('(')
                            && !v.contains('{')
                    })
                    .filter_map(|v| {
                        // Strip a trailing `// comment`, keep the leading ident.
                        let cut = v.split("//").next().unwrap_or("").trim();
                        let ident = ident_at(cut, 0);
                        (!ident.is_empty()).then_some(ident)
                    })
                    .collect();
                // Enhanced enum (Dart 3): a constructor, getter, or method body.
                let enhanced = body.contains("const ")
                    || body.contains(" get ")
                    || body.contains("();")
                    || body.contains("() {");
                let mut decorators = Vec::new();
                if enhanced {
                    decorators.push("enhanced".to_string());
                }
                let convention = detect_value_convention(&values);
                enums.insert(
                    name,
                    EnumInfo {
                        values,
                        file: rel.clone(),
                        decorators,
                        value_convention: Some(convention),
                    },
                );
            }
        }
        enums
    }

    /// A port of `scanDtos()` — `*Dto/Model/Request/Response` classes.
    fn scan_dtos(&self, root: &Path) -> BTreeMap<String, DtoInfo> {
        const SUFFIXES: &[&str] = &["Dto", "Request", "Response", "Model"];
        let mut dtos = BTreeMap::new();
        for file in collect_files(root, ".dart", DART_IGNORE) {
            let Some(content) = read_file_safe(&file) else {
                continue;
            };
            let rel = relative_path(root, &file);
            let validation_pattern = if content.contains("@JsonSerializable()") {
                "json_serializable"
            } else if content.contains("@freezed") {
                "freezed"
            } else {
                "manual"
            };
            for head in class_heads(&content) {
                if !SUFFIXES.iter().any(|s| head.name.ends_with(s)) {
                    continue;
                }
                let entity = SUFFIXES.iter().find_map(|s| {
                    head.name.strip_suffix(s).filter(|stem| !stem.is_empty()).map(str::to_string)
                });
                dtos.insert(
                    head.name.clone(),
                    DtoInfo {
                        file: rel.clone(),
                        entity,
                        validation_pattern: validation_pattern.to_string(),
                    },
                );
            }
        }
        dtos
    }

    /// Infer the `_patterns.dart` object — a port of `inferPatterns()`.
    fn infer_patterns(&self, root: &Path, result: &ScanResult) -> serde_json::Value {
        let pubspec = read_pubspec(root).unwrap_or_default();

        // State management — pubspec dependency first, then a source fallback.
        let mut state_management = "none";
        if pubspec_has(&pubspec, "flutter_bloc") || pubspec_has(&pubspec, "bloc") {
            state_management = "bloc";
        } else if pubspec_has(&pubspec, "flutter_riverpod")
            || pubspec_has(&pubspec, "riverpod")
            || pubspec_has(&pubspec, "hooks_riverpod")
        {
            state_management = "riverpod";
        } else if pubspec_has(&pubspec, "provider") {
            state_management = "provider";
        } else if pubspec_has(&pubspec, "get") {
            state_management = "getx";
        } else if pubspec_has(&pubspec, "mobx") || pubspec_has(&pubspec, "flutter_mobx") {
            state_management = "mobx";
        } else {
            for file in collect_files(root, ".dart", DART_IGNORE) {
                let Some(content) = read_file_safe(&file) else {
                    continue;
                };
                if content.contains("extends Bloc<") || content.contains("extends Cubit<") {
                    state_management = "bloc";
                    break;
                }
                if content.contains("ChangeNotifier") || content.contains("Provider<") {
                    state_management = "provider";
                    break;
                }
                if content.contains("GetxController") || content.contains("Obx(") {
                    state_management = "getx";
                    break;
                }
                if content.contains("@observable") || content.contains("@action") {
                    state_management = "mobx";
                    break;
                }
            }
        }

        // Navigation — derived from detected routes + pubspec packages.
        let routes = result.routes.keys().cloned().collect::<Vec<_>>();
        let has_route = |key: &str| routes.iter().any(|r| r == key);
        let navigation = if has_route("go_router") || pubspec_has(&pubspec, "go_router") {
            "go_router"
        } else if has_route("auto_route") || pubspec_has(&pubspec, "auto_route") {
            "auto_route"
        } else if has_route("getx") || state_management == "getx" {
            "getx"
        } else if has_route("navigator") {
            "navigator"
        } else {
            "none"
        };

        // Serialization strategy.
        let has_freezed =
            pubspec_has(&pubspec, "freezed") || pubspec_has(&pubspec, "freezed_annotation");
        let has_json_ser = pubspec_has(&pubspec, "json_serializable");
        let entity_has = |needle: &str| {
            result
                .entities
                .values()
                .any(|e| e.decorators.iter().any(|d| d == needle))
        };
        let serialization = if has_freezed {
            "freezed"
        } else if has_json_ser {
            "json_serializable"
        } else if entity_has("@freezed") {
            "freezed"
        } else if entity_has("@JsonSerializable") {
            "json_serializable"
        } else if result.entities.is_empty() {
            "none"
        } else {
            "manual"
        };

        // Entity patterns.
        let entity_files: Vec<String> = result
            .entities
            .values()
            .map(|e| e.file.clone())
            .filter(|f| !f.is_empty())
            .collect();
        let entity_folder = infer_common_folder(&entity_files);
        let base_pattern = if entity_has("@freezed") {
            "freezed"
        } else if entity_has("Equatable") {
            "equatable"
        } else {
            "plain"
        };

        // Enum patterns.
        let enum_files: Vec<String> = result
            .enums
            .values()
            .map(|e| e.file.clone())
            .filter(|f| !f.is_empty())
            .collect();
        let enum_folder = infer_common_folder(&enum_files);
        let has_enhanced_enums = result
            .enums
            .values()
            .any(|e| e.decorators.iter().any(|d| d == "enhanced"));
        let enum_in_enum_dir =
            enum_files.iter().filter(|f| f.to_lowercase().contains("enum")).count();
        let enum_separate_files = !enum_files.is_empty()
            && (enum_in_enum_dir as f64 / enum_files.len() as f64) > 0.5;

        serde_json::json!({
            "stateManagement": state_management,
            "navigation": navigation,
            "serialization": serialization,
            "entity": {
                "folder": entity_folder,
                "basePattern": base_pattern,
                "namingConvention": "PascalCase",
            },
            "enum": {
                "folder": enum_folder,
                "enhanced": has_enhanced_enums,
                "separateFiles": enum_separate_files,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn detect_requires_pubspec() {
        let dir = tempdir().unwrap();
        assert!(!DartScanner.detect(dir.path()));
        std::fs::write(dir.path().join("pubspec.yaml"), "name: app").unwrap();
        assert!(DartScanner.detect(dir.path()));
    }

    #[test]
    fn scan_entities_extracts_model_class() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("pubspec.yaml"), "name: app").unwrap();
        std::fs::write(
            dir.path().join("user_model.dart"),
            "class UserModel extends Equatable {\n  final String id;\n}\n",
        )
        .unwrap();
        let entities = DartScanner.scan_entities(dir.path());
        let user = entities.get("UserModel").expect("UserModel entity");
        assert_eq!(user.base_class.as_deref(), Some("Equatable"));
        assert!(user.decorators.contains(&"Equatable".to_string()));
    }

    #[test]
    fn scan_entities_extracts_freezed_with_mixin() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("pubspec.yaml"), "name: app").unwrap();
        std::fs::write(
            dir.path().join("order.dart"),
            "@freezed\nclass Order with _$Order {\n  factory Order() = _Order;\n}\n",
        )
        .unwrap();
        let entities = DartScanner.scan_entities(dir.path());
        let order = entities.get("Order").expect("Order entity");
        assert!(order.decorators.contains(&"@freezed".to_string()));
        assert!(order.decorators.contains(&"with:_$Order".to_string()));
    }

    #[test]
    fn scan_entities_skips_abstract_class() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("pubspec.yaml"), "name: app").unwrap();
        std::fs::write(
            dir.path().join("repo.dart"),
            "abstract class UserModel {\n  void load();\n}\n",
        )
        .unwrap();
        let entities = DartScanner.scan_entities(dir.path());
        assert!(entities.is_empty());
    }

    #[test]
    fn scan_enums_extracts_plain_and_enhanced() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("pubspec.yaml"), "name: app").unwrap();
        std::fs::write(
            dir.path().join("status.dart"),
            "enum Status { active, closed }\n\
             enum Planet {\n  earth(5.9),\n  mars(6.4);\n  const Planet(this.mass);\n  final double mass;\n}\n",
        )
        .unwrap();
        let enums = DartScanner.scan_enums(dir.path());
        let status = enums.get("Status").expect("Status enum");
        assert_eq!(status.values, vec!["active", "closed"]);
        assert!(status.decorators.is_empty());
        // Faithful to the JS scanner: an enhanced enum is flagged `enhanced`,
        // and member entries carrying constructor args (`earth(5.9)`) are
        // dropped by the `(`-filter — so an arg-bearing enhanced enum has no
        // extracted values.
        let planet = enums.get("Planet").expect("Planet enum");
        assert!(planet.decorators.contains(&"enhanced".to_string()));
        assert!(planet.values.is_empty());
    }

    #[test]
    fn scan_dtos_links_entity_by_suffix() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("pubspec.yaml"), "name: app").unwrap();
        std::fs::write(
            dir.path().join("login.dart"),
            "@JsonSerializable()\nclass LoginRequest {\n  final String email;\n}\n",
        )
        .unwrap();
        let dtos = DartScanner.scan_dtos(dir.path());
        let dto = dtos.get("LoginRequest").expect("LoginRequest dto");
        assert_eq!(dto.entity.as_deref(), Some("Login"));
        assert_eq!(dto.validation_pattern, "json_serializable");
    }

    #[test]
    fn infer_patterns_reads_pubspec_state_management() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("pubspec.yaml"),
            "name: app\ndependencies:\n  flutter_bloc: ^8.0.0\n  freezed_annotation: ^2.0.0\n",
        )
        .unwrap();
        let result = ScanResult::default();
        let patterns = DartScanner.infer_patterns(dir.path(), &result);
        assert_eq!(patterns["stateManagement"], "bloc");
        assert_eq!(patterns["serialization"], "freezed");
    }

    #[test]
    fn detect_architecture_classifies_clean() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("pubspec.yaml"), "name: app").unwrap();
        for layer in ["lib/domain", "lib/data", "lib/presentation"] {
            std::fs::create_dir_all(dir.path().join(layer)).unwrap();
        }
        assert_eq!(DartScanner.detect_architecture(dir.path()), "clean-architecture");
    }
}
