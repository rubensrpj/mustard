//! The scanner subsystem — a single, language-agnostic interpreter that
//! consumes the Wave 1 single-pass profile and (when an LLM round-trip is
//! available) augments it with model-interpreted nodes and edges.
//!
//! ## Why the per-language scanners are gone
//!
//! Wave 2 of `project-profiler` removed the eight hand-written
//! `*_scanner.rs` files (`dotnet`, `typescript`, `dart`, `php`, `python`,
//! `java`, `go`, `rust`). Each one encoded fixed conventions — `Entities/`
//! and `Domain/` folders for `.NET`, `pgTable` calls for TypeScript, derive
//! macros for Rust — and silently missed projects that used neighbouring
//! conventions (`Features/`+`DbSet`, `mysqlTable`/`sqliteTable`, structs
//! without ORM derives, …).
//!
//! In their place, [`load_scanner`] now returns a single generic scanner
//! whose [`Scanner::scan`] implementation:
//!
//! 1. Visits the subproject once (Wave 1 [`file_utils::visit`]).
//! 2. Runs the deterministic [`structural_extract`] pass (primary source).
//! 3. Optionally calls [`interpret::interpret_with`] for model-assisted
//!    entity/enum/edge extraction — an **opt-in, default-OFF** fallback gated
//!    on `MUSTARD_SCAN_LLM` (cached cold-path; fail-open when no model). On the
//!    default path no `claude` subprocess is spawned.
//!
//! The contract data types ([`EntityInfo`], [`EnumInfo`], …) are unchanged —
//! callers in [`crate::commands::scan::sync_entity_registry`] consume the same shapes as
//! before, so the registry JSON stays byte-stable across the rewrite.

pub mod architecture;
pub mod cluster_discovery;
pub mod file_utils;
pub mod graph;
pub mod interpret;
pub mod pluralize;
pub mod project_conventions;
pub mod refs_installer;
pub mod resolve;
pub mod structural_extract;
pub mod subproject_discovery;

use mustard_core::io::fs as mfs;
use std::collections::BTreeMap;
use std::path::Path;

/// A scanned entity (model / domain object).
///
/// Mirrors the `EntityInfo` typedef in the legacy `scanner-contract.js`.
/// Optional fields are `None` when omitted so serialisation can
/// `skip_serializing_if` to reproduce the JSON shape.
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

/// A scanned route group — mirrors the legacy `RouteInfo` typedef.
///
/// The contract types mirror the legacy typedefs field-for-field; the generic
/// interpreter does not populate routes today, so the rest of the surface is
/// deliberately future-proof — hence the `dead_code` allow.
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

/// A scanned DTO / schema / view-model — mirrors the legacy `DtoInfo` typedef.
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

/// A scanned service class — mirrors the legacy `ServiceInfo` typedef.
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

/// The combined output of a full scan — mirrors the legacy object shape so
/// `sync_entity_registry::build_registry` consumes the same fields as before.
///
/// `routes` / `dtos` / `services` / `architecture` are carried but no longer
/// populated by the generic interpreter (the JS scanners that populated them
/// are gone). They stay on the struct so the registry consumer and any
/// future enrichment pass keep the same shape — hence the field-level
/// `dead_code` allows.
#[derive(Debug, Clone, Default)]
pub struct ScanResult {
    /// Entities keyed by entity name.
    pub entities: BTreeMap<String, EntityInfo>,
    /// Enums keyed by enum name.
    pub enums: BTreeMap<String, EnumInfo>,
    /// Route groups keyed by route key.
    #[allow(dead_code)]
    pub routes: BTreeMap<String, RouteInfo>,
    /// DTOs keyed by DTO name.
    #[allow(dead_code)]
    pub dtos: BTreeMap<String, DtoInfo>,
    /// Services keyed by service class name.
    #[allow(dead_code)]
    pub services: BTreeMap<String, ServiceInfo>,
    /// The detected architectural style (`clean` / `hexagonal` / `layered` /
    /// `ddd` / `unknown`). Populated deterministically (F1-b) by
    /// [`architecture::detect_subproject_architecture`] from folder-role signals
    /// + import-graph direction; `unknown` only when no role signal surfaced.
    pub architecture: String,
    /// Inferred `_patterns.{stack}` object — a `serde_json::Value::Object`.
    pub patterns: serde_json::Value,
}

/// Base contract for the scanner subsystem.
///
/// Today there is one concrete implementor — [`InterpretedScanner`] — but the
/// trait surface stays so the `sync_entity_registry` pipeline can swap in
/// alternative interpreters (e.g. a future "fast path" or "offline"
/// implementation) without touching its call sites.
pub trait Scanner {
    /// Run the full scan pipeline and return the merged [`ScanResult`].
    ///
    /// The default implementation runs [`file_utils::visit`] itself; callers
    /// that already visited the tree (e.g. `sync_entity_registry`) should use
    /// [`scan_with_visited`](Scanner::scan_with_visited) to avoid double work.
    ///
    /// Production callers always go through `scan_with_visited`; the default
    /// is kept for ergonomics + test parity coverage — hence `dead_code`.
    #[allow(dead_code)]
    fn scan(&self, root: &Path) -> ScanResult {
        let visited = file_utils::visit(root, &[]);
        self.scan_with_visited(root, &visited)
    }

    /// Run the scan against a pre-visited file vector. Useful when the outer
    /// caller already paid the visit cost (the registry pipeline visits once
    /// per subproject and shares the result across scanners + enrichment).
    fn scan_with_visited(&self, root: &Path, visited: &[file_utils::VisitedFile])
        -> ScanResult;
}

/// The generic interpreter — Wave 2 replacement for the eight per-language
/// scanners.
///
/// Wraps the visit-cache, cluster discovery, and model interpretation into a
/// single [`Scanner::scan`] call. The stack id is carried so the cluster
/// discovery and interpret cache can key on it; the rest of the pipeline is
/// fully agnostic. `env_override` lets tests inject a synthetic
/// [`interpret::InterpretEnv`] (empty API key ⇒ no network) without touching
/// process env, which is `unsafe` on edition 2024 and forbidden in this
/// crate.
pub struct InterpretedScanner {
    /// Stack id resolved by [`detect_stack`] (or supplied as a hint by the
    /// caller); flows into the cluster cache + interpret cache keys.
    pub stack_id: String,
    /// Test-only env override. Production code leaves it `None` and the
    /// scanner reads [`interpret::InterpretEnv::from_process`].
    pub env_override: Option<interpret::InterpretEnv>,
}

impl Scanner for InterpretedScanner {
    fn scan_with_visited(&self, root: &Path, visited: &[file_utils::VisitedFile]) -> ScanResult {
        // Step 1 — agnostic cluster discovery. Output is used both as input
        // to the model and as the raw cluster surface in `_patterns.{stack}`.
        let clusters = cluster_discovery::discover_clusters(root, &self.stack_id, None);

        // Step 2 — STRUCTURAL extraction is the *primary* source (F1-a). It
        // pulls entities/enums with names, fields, refs, decorators, base class
        // and table name **deterministically and offline** via the in-crate
        // grammars + textual floor (tree-sitter / Aho-Corasick), no `claude`
        // binary required. Everything downstream treats this as authoritative.
        let structural = structural_extract::extract(root, visited);
        let mut entities = structural.entities;
        let mut enums = structural.enums;
        // Derive the value convention for each structurally-found enum from its
        // member casing (the cold path could not compute this).
        for info in enums.values_mut() {
            if info.value_convention.is_none() && !info.values.is_empty() {
                info.value_convention = Some(detect_value_convention(&info.values));
            }
        }

        // Step 3 — model interpretation, now an OPT-IN, DEFAULT-OFF fallback
        // (F1-c). Resolve the env first so we know whether the LLM gate is on.
        let env = self
            .env_override
            .clone()
            .unwrap_or_else(interpret::InterpretEnv::from_process);

        // The model only runs as a COMPLEMENT when the gate is ON *and* the
        // structural floor came back empty (an exotic stack with no grammar and
        // an empty textual floor). On the default path the gate is OFF, so
        // `interpret_with` short-circuits before probing/spawning `claude` and
        // returns the empty floor — zero subprocesses. The savings telemetry is
        // emitted one layer up in `sync_entity_registry`, which owns the
        // monorepo project root the `.events` sink is keyed on (the scanner only
        // sees the subproject root).
        let run_llm = env.llm_enabled && entities.is_empty();
        let interpreted = if run_llm {
            interpret::interpret_with(root, &self.stack_id, visited, &clusters, &env)
        } else {
            interpret::InterpretedResult::default()
        };

        // LLM entities only fill gaps the structural pass left (additive).
        for e in interpreted.entities {
            if e.name.is_empty() || entities.contains_key(&e.name) {
                continue;
            }
            entities.insert(
                e.name.clone(),
                EntityInfo {
                    file: e.file,
                    refs: e.edges,
                    ..EntityInfo::default()
                },
            );
        }
        for e in interpreted.enums {
            if e.name.is_empty() || enums.contains_key(&e.name) {
                continue;
            }
            enums.insert(
                e.name.clone(),
                EnumInfo {
                    values: e.values,
                    file: e.file,
                    ..EnumInfo::default()
                },
            );
        }

        // F1-b — the `patternsOverlay` (`clusterLabels` / `dominant` / `edges`)
        // is now built DETERMINISTICALLY here instead of coming from the LLM:
        //   * clusterLabels ← `label` of every discovered cluster,
        //   * dominant      ← the dominant naming convention,
        //   * edges         ← the type/import join across the structural
        //                     entities.
        // The outer caller (`sync_entity_registry`) still layers in the raw
        // `discovered[]` / `folderFrequency` / `conventions`; this overlay is
        // the same `{clusterLabels,dominant,edges}` shape the model used to
        // emit, so `_patterns.{stack}` stays byte-stable. The (default-OFF)
        // LLM `interpreted.patterns_overlay` is intentionally dropped — the
        // model no longer owns the overlay.
        let conventions =
            project_conventions::compute_project_conventions(root, &self.stack_id);
        let patterns = architecture::build_patterns_overlay(&clusters, &conventions, &entities);

        // F1-b — populate the architecture tag deterministically (was always
        // `"unknown"`). Honours a `mustard.json#architecture` pin; otherwise
        // infers the style from folder-role signals + import-graph direction.
        let architecture =
            architecture::detect_subproject_architecture(root, visited, &entities, &enums);

        ScanResult {
            entities,
            enums,
            routes: BTreeMap::new(),
            dtos: BTreeMap::new(),
            services: BTreeMap::new(),
            architecture,
            patterns,
        }
    }
}

/// Manifest / file-presence signals used to detect the dominant stack of a
/// subproject. Agnostic: the list is "manifests common across the language
/// communities Mustard sees", not "languages Mustard knows scanners for".
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

/// Detect which stack a subproject uses via file-presence heuristics.
///
/// Returns the stack id, or `None` when no manifest signal matched. The
/// returned id is purely a label flowing into the cluster cache + interpret
/// cache; it never gates which entities the interpreter recovers.
#[must_use]
pub fn detect_stack(root: &Path) -> Option<&'static str> {
    for (stack_id, signals) in STACK_SIGNALS {
        if signals.iter().any(|pattern| signal_present(root, pattern)) {
            return Some(stack_id);
        }
    }
    None
}

/// Load the scanner for a subproject — Wave 2 returns a single generic
/// [`InterpretedScanner`] regardless of the detected stack.
///
/// `stack_hint` (the `subprojectMeta.stack` field) wins over [`detect_stack`]
/// when both are present. When neither resolves to a known signal, the
/// scanner is still returned with `stack_id = "unknown"` — the interpreter
/// runs against the agnostic profile and may still recover entities. The
/// pre-Wave-2 `Option` return is gone (the function is now total) since the
/// per-language gate it once expressed no longer exists.
#[must_use]
pub fn load_scanner(root: &Path, stack_hint: Option<&str>) -> Box<dyn Scanner> {
    let stack_id = stack_hint
        .map(str::to_string)
        .or_else(|| detect_stack(root).map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string());
    Box::new(InterpretedScanner {
        stack_id,
        env_override: None,
    })
}

/// Detect a value convention from enum member names — used to tag every
/// structurally-extracted enum (`UPPER_CASE` / `PascalCase` / `camelCase` /
/// `mixed`) so the registry's `valueConvention` field reflects real casing.
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
    fn load_scanner_returns_generic_interpreter() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        // The interpreter is returned for every detected stack — Wave 2 no
        // longer dispatches per language; calling `.scan_with_visited` must
        // simply run without panicking.
        let scanner = load_scanner(dir.path(), None);
        let _ = scanner.scan_with_visited(dir.path(), &[]);
    }

    #[test]
    fn load_scanner_uses_stack_hint() {
        let dir = tempdir().unwrap();
        // No manifest present; the hint still wins and a scanner is returned.
        let scanner = load_scanner(dir.path(), Some("python"));
        let _ = scanner.scan_with_visited(dir.path(), &[]);
    }

    #[test]
    fn load_scanner_unknown_stack_still_returns() {
        let dir = tempdir().unwrap();
        // Wave 2: no per-language gating — even an unknown stack yields a
        // scanner that runs against the agnostic profile.
        let scanner = load_scanner(dir.path(), None);
        let _ = scanner.scan_with_visited(dir.path(), &[]);
    }

    #[test]
    fn detect_value_convention_classifies() {
        let upper = vec!["ACTIVE".to_string(), "INACTIVE".to_string()];
        assert_eq!(detect_value_convention(&upper), "UPPER_CASE");
        let pascal = vec!["Active".to_string(), "Pending".to_string()];
        assert_eq!(detect_value_convention(&pascal), "PascalCase");
        assert_eq!(detect_value_convention(&[]), "unknown");
    }

    // --- Wave 1 (`project-profiler`) single-pass behaviour ----------------
    //
    // Wave 2 removed the per-language scanners but the single-pass guarantee
    // still holds: the generic interpreter (cluster discovery + interpret)
    // resolves every file read through the active visit cache. These tests
    // assert that contract on the new code path.

    fn build_multi_stack_fixture(dir: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
        let rust = dir.join("rust-app");
        std::fs::create_dir_all(rust.join("src")).unwrap();
        std::fs::write(
            rust.join("Cargo.toml"),
            "[package]\nname = \"app\"\n\n[dependencies]\ndiesel = \"2\"\n",
        )
        .unwrap();
        std::fs::write(
            rust.join("src").join("models.rs"),
            "/// A registered user.\n#[derive(Queryable, Debug)]\n\
             pub struct User {\n    pub id: i32,\n    pub name: String,\n}\n\n\
             #[derive(Debug, Clone)]\npub enum Status {\n    Active,\n    Pending,\n}\n",
        )
        .unwrap();
        let ts = dir.join("ts-app");
        std::fs::create_dir_all(ts.join("src")).unwrap();
        std::fs::write(
            ts.join("package.json"),
            r#"{"name":"ts-app","dependencies":{"drizzle-orm":"0.1"}}"#,
        )
        .unwrap();
        std::fs::write(
            ts.join("src").join("schema.ts"),
            "export const users = pgTable('users', {\n  id: serial(),\n  name: text(),\n});\n\
             export enum Role {\n  Admin = 'admin',\n  User = 'user',\n}\n",
        )
        .unwrap();
        (rust, ts)
    }

    /// Build a generic interpreter with a no-op `InterpretEnv` so unit tests
    /// never invoke the real `claude` CLI. `claude_bin` is set to a path that
    /// is guaranteed not to exist, making `probe_claude_binary` return `false`
    /// and keeping the model layer a deliberate no-op (the crate forbids
    /// `unsafe`, so `set_var`/`remove_var` are unavailable on edition 2024).
    fn test_scanner(stack: &str) -> InterpretedScanner {
        InterpretedScanner {
            stack_id: stack.to_string(),
            env_override: Some(interpret::InterpretEnv {
                // Point at a path that cannot exist — keeps the model a no-op.
                claude_bin: "/dev/null/no-such-claude".to_string(),
                cache_disabled: true,
                ..interpret::InterpretEnv::default()
            }),
        }
    }

    /// Parent AC-P-4 — paridade pós-W1. Wave 2 reinterprets the parity
    /// claim: `scan()` (which now visits internally) and
    /// `scan_with_visited()` (which reuses a pre-visited vector) must
    /// produce a byte-equal `ScanResult` on the same fixture. The cluster
    /// discovery output flows through the cache identically either way.
    #[test]
    fn single_pass_parity() {
        let dir = tempfile::tempdir().unwrap();
        let (rust_root, ts_root) = build_multi_stack_fixture(dir.path());
        // Empty InterpretEnv ⇒ interpret is a no-op (empty API key) for
        // both calls; the parity check is between two purely deterministic
        // code paths.
        let rust_scanner = test_scanner("rust");
        let ts_scanner = test_scanner("typescript");

        let rust_default = rust_scanner.scan(&rust_root);
        let ts_default = ts_scanner.scan(&ts_root);

        let rust_visited = file_utils::visit(&rust_root, &[]);
        let ts_visited = file_utils::visit(&ts_root, &[]);
        let rust_handed = rust_scanner.scan_with_visited(&rust_root, &rust_visited);
        let ts_handed = ts_scanner.scan_with_visited(&ts_root, &ts_visited);

        let sig = |r: &ScanResult| {
            format!(
                "arch={}\nentities={:?}\nenums={:?}\npatterns={}",
                r.architecture,
                r.entities,
                r.enums,
                serde_json::to_string(&r.patterns).unwrap_or_default()
            )
        };
        assert_eq!(sig(&rust_default), sig(&rust_handed));
        assert_eq!(sig(&ts_default), sig(&ts_handed));
    }

    /// AC-2 (Wave 1) — during a single `Scanner::scan_with_visited` call no
    /// source file is re-opened via `read_file_safe` after the visit pass
    /// has cached it. Wave 2's generic interpreter still routes every read
    /// through the cache.
    #[test]
    fn single_pass_reads_once() {
        let dir = tempfile::tempdir().unwrap();
        let (rust_root, _) = build_multi_stack_fixture(dir.path());
        let scanner = test_scanner("rust");

        let visited = file_utils::visit(&rust_root, &[]);
        assert!(!visited.is_empty(), "fixture must contain source files");

        file_utils::reset_disk_read_count();
        let _ = file_utils::with_cache(&rust_root, visited.clone(), || {
            scanner.scan_with_visited(&rust_root, &visited)
        });
        let hits = file_utils::disk_hit_count();
        assert_eq!(
            hits, 0,
            "expected zero disk hits through `read_file_safe`, got {hits}"
        );
    }
}
pub mod scan_finalize;
pub mod scan_md_validate;
pub mod scan_orchestrate;
pub mod scan_precompute;
pub mod scan_recipes_validate;
pub mod scan_structural;
pub mod sync_detect;
pub mod sync_entity_registry;
pub mod recipe_match;
