//! The scanner subsystem — a port of `registry/scanner-contract.js`,
//! `scanner-loader.js`, and the seven `*-scanner.js` language scanners.
//!
//! The JS design used a dynamic `require` to load `scanners/{stack}-scanner.js`
//! at runtime. Rust has no dynamic loading, so the mechanism is adapted
//! idiomatically: every scanner implements the [`Scanner`] trait, and
//! [`load_scanner`] is a static `match` over the detected stack id. Adding a
//! stack is a new struct plus one `match` arm — the JS "drop a file in
//! `scanners/`" extension point becomes a compile-checked enum-like dispatch.
//!
//! The contract data types ([`EntityInfo`], [`EnumInfo`], …) mirror the JSDoc
//! typedefs in `scanner-contract.js` field-for-field, so the registry assembled
//! in a later wave consumes the same shapes the JS scanners produced.
//!
//! Wave 2 wires the subsystem into `run sync-registry`, which consumes the
//! scanners, the route/dto/service maps, and per-stack pattern inference.

pub mod cluster_discovery;
pub mod file_utils;
pub mod pluralize;
pub mod project_conventions;

mod dart_scanner;
mod dotnet_scanner;
mod go_scanner;
mod java_scanner;
mod php_scanner;
mod python_scanner;
mod rust_scanner;
mod typescript_scanner;

use mustard_core::fs as mfs;
use std::collections::BTreeMap;
use std::path::Path;

/// A scanned entity (model / domain object).
///
/// Mirrors the `EntityInfo` typedef in `scanner-contract.js`. Optional fields
/// are `None` when the JS object omitted the key, so a later `serde`
/// serialization can `skip_serializing_if` to reproduce the JSON shape.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntityInfo {
    /// Relative path from the subproject root.
    pub file: String,
    /// Class-level decorators / attributes / derive macros.
    pub decorators: Vec<String>,
    /// Key property names (often `name: Type`).
    pub properties: Vec<String>,
    /// Referenced entities (foreign keys / navigation).
    pub refs: Vec<String>,
    /// Child / collection entities.
    pub sub: Vec<String>,
    /// Enum types used by the entity.
    pub enums: Vec<String>,
    /// Base class, when the entity extends one.
    pub base_class: Option<String>,
    /// Backing table name, when explicitly declared.
    pub table_name: Option<String>,
}

/// A scanned enum / value type. Mirrors the `EnumInfo` typedef.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnumInfo {
    /// Enum member names.
    pub values: Vec<String>,
    /// Relative path from the subproject root.
    pub file: String,
    /// Enum-level decorators / derive macros.
    pub decorators: Vec<String>,
    /// Detected value convention (`UPPER_CASE` / `PascalCase` / `camelCase` / …).
    pub value_convention: Option<String>,
}

/// A scanned route group — mirrors the `RouteInfo` typedef.
///
/// The contract types mirror the JS typedefs field-for-field; `infer_patterns`
/// reads only the fields a given stack's pattern inference needs, so the rest
/// are a deliberate, future-proof surface — hence the `dead_code` allow.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct RouteInfo {
    /// Relative path from the subproject root.
    pub file: String,
    /// Route group prefix (e.g. `/contracts`).
    pub prefix: String,
    /// Endpoint descriptors (`method`, `path`, optional `name`).
    pub endpoints: Vec<EndpointInfo>,
}

/// One endpoint within a [`RouteInfo`].
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct EndpointInfo {
    /// HTTP method (`GET`, `POST`, …).
    pub method: String,
    /// Full route path.
    pub path: String,
    /// Handler name, when one could be extracted.
    pub name: Option<String>,
}

/// A scanned DTO / schema / view-model — mirrors the `DtoInfo` typedef.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct DtoInfo {
    /// Relative path from the subproject root.
    pub file: String,
    /// Linked entity name, when inferable from the DTO name.
    pub entity: Option<String>,
    /// Validation pattern (`zod`, `class-validator`, `none`).
    pub validation_pattern: String,
}

/// A scanned service class — mirrors the `ServiceInfo` typedef.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct ServiceInfo {
    /// Relative path from the subproject root.
    pub file: String,
    /// Linked entity name, when inferable from the service name.
    pub entity: Option<String>,
    /// Injected dependency type names.
    pub dependencies: Vec<String>,
}

/// The combined output of a full scan — mirrors the object `ScannerContract.scan()`
/// returns. `patterns` carries the inferred `_patterns.{stack}` object as
/// JSON; the registry assembles it straight into the registry file.
///
/// `services` mirrors the JS `scan()` shape; like the JS `inferPatterns` it is
/// carried but not yet read by any consumer — hence the field-level allow.
#[derive(Debug, Clone, Default)]
pub struct ScanResult {
    /// Entities keyed by entity name.
    pub entities: BTreeMap<String, EntityInfo>,
    /// Enums keyed by enum name.
    pub enums: BTreeMap<String, EnumInfo>,
    /// Route groups keyed by route key.
    pub routes: BTreeMap<String, RouteInfo>,
    /// DTOs keyed by DTO name.
    pub dtos: BTreeMap<String, DtoInfo>,
    /// Services keyed by service class name.
    #[allow(dead_code)]
    pub services: BTreeMap<String, ServiceInfo>,
    /// The detected architecture pattern (`solid`, `layered`, `minimal`, …).
    pub architecture: String,
    /// Inferred `_patterns.{stack}` object — a `serde_json::Value::Object`.
    pub patterns: serde_json::Value,
}

/// Base contract for stack scanners — a port of the `ScannerContract` class.
///
/// Every scanner reports whether it [`detect`](Scanner::detect)s its stack and
/// runs a [`scan`](Scanner::scan). The default `scan` calls each `scan_*` method
/// then records the architecture, exactly like `ScannerContract.scan()`.
pub trait Scanner {
    /// `true` if this scanner applies to the subproject at `root`.
    fn detect(&self, root: &Path) -> bool;

    /// The high-level architecture of the project (`unknown` by default).
    fn detect_architecture(&self, _root: &Path) -> String {
        "unknown".to_string()
    }

    /// Scan entities (models / domain objects). Empty by default.
    fn scan_entities(&self, _root: &Path) -> BTreeMap<String, EntityInfo> {
        BTreeMap::new()
    }

    /// Scan enums / value types. Empty by default.
    fn scan_enums(&self, _root: &Path) -> BTreeMap<String, EnumInfo> {
        BTreeMap::new()
    }

    /// Scan routes / endpoints. Empty by default.
    fn scan_routes(&self, _root: &Path) -> BTreeMap<String, RouteInfo> {
        BTreeMap::new()
    }

    /// Scan DTOs / schemas / view-models. Empty by default.
    fn scan_dtos(&self, _root: &Path) -> BTreeMap<String, DtoInfo> {
        BTreeMap::new()
    }

    /// Scan service classes. Empty by default.
    fn scan_services(&self, _root: &Path) -> BTreeMap<String, ServiceInfo> {
        BTreeMap::new()
    }

    /// Infer the `_patterns.{stack}` object from the scanned data — a port of
    /// `inferPatterns()`. The default is an empty object (the scanner declined
    /// to infer patterns). Implementors return a `serde_json::Value::Object`.
    fn infer_patterns(&self, _root: &Path, _result: &ScanResult) -> serde_json::Value {
        serde_json::Value::Object(serde_json::Map::new())
    }

    /// Run the full scan pipeline — a port of `ScannerContract.scan()`.
    fn scan(&self, root: &Path) -> ScanResult {
        let mut result = ScanResult {
            entities: self.scan_entities(root),
            enums: self.scan_enums(root),
            routes: self.scan_routes(root),
            dtos: self.scan_dtos(root),
            services: self.scan_services(root),
            architecture: self.detect_architecture(root),
            patterns: serde_json::Value::Object(serde_json::Map::new()),
        };
        // `inferPatterns` runs after every scan_* method, then `scan()` records
        // `architecture` onto the patterns object (matching the JS contract).
        let mut patterns = self.infer_patterns(root, &result);
        if let serde_json::Value::Object(ref mut map) = patterns {
            map.insert(
                "architecture".to_string(),
                serde_json::Value::String(result.architecture.clone()),
            );
        }
        result.patterns = patterns;
        result
    }
}

/// The eight stack ids and their file-presence signals — a port of
/// `STACK_SIGNALS` in `scanner-loader.js`. Order matters: the first match wins,
/// so the list stays "most specific first" as the JS object did.
const STACK_SIGNALS: &[(&str, &[&str])] = &[
    ("dotnet", &["*.csproj", "*.sln"]),
    ("typescript", &["package.json", "tsconfig.json"]),
    ("dart", &["pubspec.yaml"]),
    ("php", &["composer.json", "artisan"]),
    (
        "python",
        &["pyproject.toml", "setup.py", "requirements.txt", "manage.py"],
    ),
    ("java", &["pom.xml", "build.gradle", "build.gradle.kts"]),
    ("go", &["go.mod"]),
    ("rust", &["Cargo.toml"]),
];

/// `true` if `root` contains a file matching `pattern` (supports a leading `*`).
fn signal_present(root: &Path, pattern: &str) -> bool {
    if let Some(ext) = pattern.strip_prefix('*') {
        // Glob-like `*.ext` — match any file ending with `ext`.
        match mfs::read_dir(root) {
            Ok(entries) => entries.iter().any(|e| e.file_name.ends_with(ext)),
            Err(_) => false,
        }
    } else {
        root.join(pattern).exists()
    }
}

/// Detect which stack a subproject uses via file-presence heuristics — a port
/// of `detectStack()`. Returns the stack id, or `None` when unrecognised.
#[must_use]
pub fn detect_stack(root: &Path) -> Option<&'static str> {
    for (stack_id, signals) in STACK_SIGNALS {
        if signals.iter().any(|pattern| signal_present(root, pattern)) {
            return Some(stack_id);
        }
    }
    None
}

/// Load the scanner for a subproject — a port of `loadScanner()`.
///
/// Resolution: use `stack_hint` (the `subprojectMeta.stack` field) when given,
/// else fall back to [`detect_stack`]; then map the stack id to its scanner via
/// a static `match` (the Rust replacement for the JS dynamic `require`). Returns
/// `None` when no stack is recognised or the scanner's `detect()` is false.
#[must_use]
pub fn load_scanner(root: &Path, stack_hint: Option<&str>) -> Option<Box<dyn Scanner>> {
    let stack_id = stack_hint.or_else(|| detect_stack(root))?;
    let scanner: Box<dyn Scanner> = match stack_id {
        "dotnet" => Box::new(dotnet_scanner::DotnetScanner),
        "typescript" => Box::new(typescript_scanner::TypeScriptScanner),
        "python" => Box::new(python_scanner::PythonScanner),
        "java" => Box::new(java_scanner::JavaScanner),
        "go" => Box::new(go_scanner::GoScanner),
        "rust" => Box::new(rust_scanner::RustScanner),
        "php" => Box::new(php_scanner::PhpScanner),
        "dart" => Box::new(dart_scanner::DartScanner),
        _ => return None,
    };
    if scanner.detect(root) {
        Some(scanner)
    } else {
        None
    }
}

/// List the stack ids that have a scanner available — a port of
/// `listAvailableScanners()`.
///
/// `sync-registry` resolves scanners per-subproject via [`load_scanner`], so it
/// never needs the flat list; kept as the faithful public-API counterpart of
/// the JS export.
#[must_use]
#[allow(dead_code)]
pub fn list_available_scanners() -> Vec<&'static str> {
    vec![
        "dotnet",
        "typescript",
        "python",
        "java",
        "go",
        "rust",
        "php",
        "dart",
    ]
}

/// Detect a value convention from enum member names.
///
/// Shared by several scanners — a port of the `detectValueConvention` helper
/// duplicated across `typescript-scanner.js`, `python-scanner.js`, etc.
#[must_use]
pub(crate) fn detect_value_convention(values: &[String]) -> String {
    if values.is_empty() {
        return "unknown".to_string();
    }
    let total = values.len() as f64;
    let is_upper = |v: &str| {
        let v = v.trim();
        let mut chars = v.chars();
        chars.next().is_some_and(|c| c.is_ascii_uppercase())
            && v.chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    };
    let is_pascal = |v: &str| {
        let v = v.trim();
        let mut chars = v.chars();
        chars.next().is_some_and(|c| c.is_ascii_uppercase())
            && v.chars().all(|c| c.is_ascii_alphanumeric())
    };
    let is_camel = |v: &str| {
        let v = v.trim();
        let mut chars = v.chars();
        chars.next().is_some_and(|c| c.is_ascii_lowercase())
            && v.chars().all(|c| c.is_ascii_alphanumeric())
    };
    let upper = values.iter().filter(|v| is_upper(v)).count() as f64;
    let pascal = values.iter().filter(|v| is_pascal(v)).count() as f64;
    let camel = values.iter().filter(|v| is_camel(v)).count() as f64;
    if upper / total > 0.6 {
        "UPPER_CASE".to_string()
    } else if pascal / total > 0.6 {
        "PascalCase".to_string()
    } else if camel / total > 0.6 {
        "camelCase".to_string()
    } else {
        "mixed".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn detect_stack_recognises_rust() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        assert_eq!(detect_stack(dir.path()), Some("rust"));
    }

    #[test]
    fn detect_stack_recognises_dotnet_via_glob() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("App.csproj"), "<Project/>").unwrap();
        assert_eq!(detect_stack(dir.path()), Some("dotnet"));
    }

    #[test]
    fn detect_stack_unknown_is_none() {
        let dir = tempdir().unwrap();
        assert_eq!(detect_stack(dir.path()), None);
    }

    #[test]
    fn load_scanner_returns_rust_scanner() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        assert!(load_scanner(dir.path(), None).is_some());
    }

    #[test]
    fn load_scanner_stack_hint_overrides_detection() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        // The hint says go, but there is no go.mod — detect() fails -> None.
        assert!(load_scanner(dir.path(), Some("go")).is_none());
    }

    #[test]
    fn detect_value_convention_classifies() {
        let upper = vec!["ACTIVE".to_string(), "INACTIVE".to_string()];
        assert_eq!(detect_value_convention(&upper), "UPPER_CASE");
        let pascal = vec!["Active".to_string(), "Pending".to_string()];
        assert_eq!(detect_value_convention(&pascal), "PascalCase");
        assert_eq!(detect_value_convention(&[]), "unknown");
    }
}
