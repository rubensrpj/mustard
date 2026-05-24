//! `mustard-rt run sync-registry` — a port of `scripts/sync-registry.js` plus
//! `registry/schema-builder.js` and `registry/description-enricher.js`.
//!
//! Generates `.claude/entity-registry.json` v4.0 by orchestrating the per-stack
//! scanners: it discovers subprojects (sharing Wave 1's `sync-detect` logic),
//! runs each stack's scanner, layers in agnostic cluster discovery, folder
//! frequency and naming conventions, assembles the v4.0 JSON shape, then
//! enriches each entity with a doc-comment description.
//!
//! ## Hash-skip parity with the JS script
//!
//! `sync-registry.js` itself has no SHA-256 gate — its only skip is "registry
//! already populated, no `--force`". That populated-check is ported faithfully.
//! The genuine incremental-skip in the pipeline lives one layer down, in
//! `cluster-discovery`'s per-subproject `.cluster-cache.json` (a tunable-aware
//! SHA-256 of the scanned file-set) — that cache *is* ported, in
//! `scan::cluster_discovery`. So "SHA256 hash skips recompilation when content
//! unchanged" still holds: cluster discovery is the expensive step, and it
//! self-skips. Wave 1's omission of `sync-detect`'s 5-minute cache gate does
//! not affect `sync-registry` correctness — `sync-detect` is only consulted for
//! the subproject list, which is cheap and must always be fresh.

use super::scan::cluster_discovery::{compute_folder_frequency, discover_clusters};
use super::scan::file_utils;
use super::scan::interpret;
use super::scan::project_conventions::compute_project_conventions;
use super::scan::{load_scanner, EntityInfo, EnumInfo, ScanResult};
use mustard_core::fs;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

/// The registry `_meta` block — a `Serialize` struct so the key order
/// (`version`, `generated`, `generator`) is byte-stable, matching the JS
/// `buildRegistry` output rather than `serde_json`'s alphabetical `Map` order.
#[derive(Debug, Serialize)]
struct RegistryMeta {
    version: &'static str,
    generated: String,
    generator: &'static str,
}

/// The whole `entity-registry.json` v4.0 document, in JS key order.
#[derive(Debug, Serialize)]
struct Registry {
    #[serde(rename = "_meta")]
    meta: RegistryMeta,
    #[serde(rename = "_patterns")]
    patterns: Value,
    #[serde(rename = "_enums")]
    enums: Value,
    e: Value,
}

/// One subproject as discovered for the registry scan.
struct Subproject {
    name: String,
    /// Relative path from the monorepo root (forward slashes).
    rel_path: String,
}

/// Run `mustard-rt run sync-registry` rooted at `root`.
///
/// `force` mirrors the JS `--force`: regenerate even when the registry is
/// already populated. Fail-open — discovery / scan errors degrade to a smaller
/// (or empty) registry rather than aborting.
pub fn run(root: &Path, force: bool) {
    let registry_path = root.join(".claude").join("entity-registry.json");

    // 1. Read the current registry (for the populated-check + version upgrade).
    let current: Option<Value> = fs::read_to_string(&registry_path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok());

    // Auto-force when the on-disk registry predates v4.0.
    let mut force = force;
    if let Some(version) = current
        .as_ref()
        .and_then(|r| r.get("_meta"))
        .and_then(|m| m.get("version"))
        .and_then(Value::as_str)
    {
        if version < "4.0" {
            println!("Registry at v{version} — upgrading to v4.0.");
            force = true;
        }
    }

    // 2. Skip when already populated and not forced.
    if let Some(ref reg) = current {
        if !force {
            let entity_count = reg
                .get("e")
                .and_then(Value::as_object)
                .map_or(0, |e| e.keys().filter(|k| !k.starts_with('_')).count());
            if entity_count > 0 {
                let version = reg
                    .get("_meta")
                    .and_then(|m| m.get("version"))
                    .and_then(Value::as_str)
                    .unwrap_or("?");
                println!(
                    "Registry v{version} populated ({entity_count} entities). \
                     Use --force to regenerate."
                );
                return;
            }
        }
    }

    // 3. Discover subprojects (same CLAUDE.md scan `sync-detect` uses).
    let subprojects = discover_subprojects(root);
    let names: Vec<&str> = subprojects.iter().map(|s| s.name.as_str()).collect();
    println!(
        "Detected {} subproject(s): {}",
        subprojects.len(),
        names.join(", ")
    );

    // 4. Scan each subproject; merge results keyed by stack id.
    //
    // Wave 1 (`project-profiler`) — install a single-pass file-content cache
    // per subproject so the scanner, cluster discovery, folder-frequency and
    // project-conventions all share one disk read per file instead of
    // performing ~6 independent walks each. The visited file map is also
    // retained per subproject so the description-enricher (step 5b below) can
    // reuse the same in-memory contents instead of re-reading each entity
    // file from disk.
    let mut scan_results: BTreeMap<String, MergedStack> = BTreeMap::new();
    let mut visited_by_sub: Vec<(std::path::PathBuf, Vec<file_utils::VisitedFile>)> = Vec::new();
    for sub in &subprojects {
        let abs = if sub.rel_path == "." {
            root.to_path_buf()
        } else {
            root.join(&sub.rel_path)
        };
        // Wave 2 — `load_scanner` is total: it always returns the generic
        // interpreter (with `stack_id = "unknown"` when no manifest signal
        // matched). The pre-Wave-2 `None` arm could never trigger.
        let scanner = load_scanner(&abs, None);
        let stack_id = match super::scan::detect_stack(&abs) {
            Some(s) => s.to_string(),
            None => "unknown".to_string(),
        };
        let visited = file_utils::visit(&abs, &[]);
        let (result, discovered, folder_frequency, conventions) =
            file_utils::with_cache(&abs, visited.clone(), || {
                // Wave 2 — `Scanner::scan_with_visited` consumes the visit we
                // already paid for, skipping the trait default's redundant
                // walk. The interpreter then runs cluster discovery + model
                // interpretation through the active cache.
                let result = scanner.scan_with_visited(&abs, &visited);
                // Agnostic enrichment layers, tagged per subproject — all
                // resolve their file reads through the active cache.
                let discovered = discover_clusters(&abs, &stack_id, Some(&sub.name));
                let folder_frequency = compute_folder_frequency(&abs, &stack_id);
                let conventions = compute_project_conventions(&abs, &stack_id);
                (result, discovered, folder_frequency, conventions)
            });

        // Wave 3 (project-profiler) — emit concept-nodes into the vault under
        // `<root>/.claude/graph/` BEFORE merging into the registry, while the
        // `discovered` vector is still in scope. `interpret::interpret` is
        // cached per-subproject so this is a hash lookup, not a second model
        // round-trip. Fail-open — the registry is the source of truth, the
        // vault is enrichment.
        let interpreted = interpret::interpret(&abs, &stack_id, &visited, &discovered);
        let _ = interpret::emit_concept_nodes(root, &sub.name, &interpreted);

        let entry = scan_results.entry(stack_id.clone()).or_default();
        entry.merge(result, discovered, folder_frequency, conventions);

        let e = entry.entities.len();
        let en = entry.enums.len();
        println!("    {e} entities, {en} enums");

        visited_by_sub.push((abs, visited));
    }

    // 5. Build the v4.0 registry document.
    let mut registry = build_registry(&scan_results);

    // 5b. Enrich entities with doc-comment descriptions (the glossary).
    //
    // Wave 1 — the enricher consults the per-subproject visit cache before
    // reaching for disk, so each entity file is read at most once for the
    // whole `sync-registry` run (scanners + enrichment share the same bytes).
    let mut visit_contents: BTreeMap<std::path::PathBuf, String> = BTreeMap::new();
    for (_, files) in &visited_by_sub {
        for v in files {
            if let Some(content) = &v.content {
                visit_contents.insert(v.abs.clone(), content.clone());
            }
        }
    }
    let (enriched, scanned) = enrich_descriptions(&mut registry.e, root, &visit_contents);

    // 6. Write the output.
    match serde_json::to_string_pretty(&registry) {
        Ok(json) => {
            if fs::write_atomic(&registry_path, format!("{json}\n").as_bytes()).is_err() {
                eprintln!("sync-registry: failed to write {}", registry_path.display());
                return;
            }
        }
        Err(_) => {
            eprintln!("sync-registry: failed to serialize registry");
            return;
        }
    }

    let e_count = registry.e.as_object().map_or(0, serde_json::Map::len);
    let enum_count = registry.enums.as_object().map_or(0, serde_json::Map::len);
    let stacks: Vec<String> = registry
        .patterns
        .as_object()
        .map(|p| p.keys().cloned().collect())
        .unwrap_or_default();
    println!("\nGenerated entity-registry.json v4.0");
    println!(
        "  {e_count} entities, {enum_count} enums, patterns: [{}]",
        stacks.join(", ")
    );
    if scanned > 0 {
        println!(
            "  Glossary: {enriched}/{scanned} entities enriched with doc-comment descriptions"
        );
    }
    println!("  Written to: {}", registry_path.display());
}

/// Accumulated scan output for a single stack across multiple subprojects.
#[derive(Default)]
struct MergedStack {
    entities: BTreeMap<String, EntityInfo>,
    enums: BTreeMap<String, EnumInfo>,
    /// The inferred `_patterns.{stack}` object (shallow-merged, last writer wins).
    patterns: serde_json::Map<String, Value>,
    /// Concatenated `discovered[]` clusters across subprojects.
    discovered: Vec<Value>,
    /// Last-seen folder frequency / conventions (they describe the stack).
    folder_frequency: Value,
    conventions: Value,
}

impl MergedStack {
    fn merge(
        &mut self,
        result: ScanResult,
        discovered: Vec<Value>,
        folder_frequency: Value,
        conventions: Value,
    ) {
        for (k, v) in result.entities {
            self.entities.insert(k, v);
        }
        for (k, v) in result.enums {
            self.enums.insert(k, v);
        }
        if let Value::Object(map) = result.patterns {
            for (k, v) in map {
                self.patterns.insert(k, v);
            }
        }
        self.discovered.extend(discovered);
        if folder_frequency
            .get("totalFolders")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            > 0
        {
            self.folder_frequency = folder_frequency;
        }
        if conventions
            .get("naming")
            .and_then(|n| n.get("total"))
            .and_then(Value::as_u64)
            .unwrap_or(0)
            > 0
        {
            self.conventions = conventions;
        }
    }
}

/// Build the `entity-registry.json` v4.0 document — a port of `buildRegistry()`.
fn build_registry(scan_results: &BTreeMap<String, MergedStack>) -> Registry {
    let mut patterns = serde_json::Map::new();
    let mut enums = serde_json::Map::new();
    let mut entities = serde_json::Map::new();

    for (stack_id, stack) in scan_results {
        // _patterns.{stack}: the inferred patterns + discovered + frequency + conventions.
        let mut stack_patterns = stack.patterns.clone();
        if !stack.discovered.is_empty() {
            stack_patterns.insert(
                "discovered".to_string(),
                Value::Array(stack.discovered.clone()),
            );
        }
        if stack.folder_frequency.is_object() {
            stack_patterns.insert("folderFrequency".to_string(), stack.folder_frequency.clone());
        }
        if stack.conventions.is_object() {
            stack_patterns.insert("conventions".to_string(), stack.conventions.clone());
        }
        if !stack_patterns.is_empty() {
            patterns.insert(stack_id.clone(), Value::Object(stack_patterns));
        }

        // _enums: rich object when file/decorators present, else a bare array.
        for (name, info) in &stack.enums {
            if !info.file.is_empty() || !info.decorators.is_empty() {
                let mut entry = serde_json::Map::new();
                entry.insert("values".to_string(), json!(compress_values(&info.values)));
                if !info.file.is_empty() {
                    entry.insert("file".to_string(), json!(info.file));
                }
                if !info.decorators.is_empty() {
                    entry.insert("decorators".to_string(), json!(info.decorators));
                }
                if let Some(conv) = &info.value_convention {
                    entry.insert("valueConvention".to_string(), json!(conv));
                }
                enums.insert(name.clone(), Value::Object(entry));
            } else {
                enums.insert(name.clone(), json!(compress_values(&info.values)));
            }
        }

        // e: compact entity entries (omit empty fields).
        for (name, info) in &stack.entities {
            let mut entry = serde_json::Map::new();
            if !info.file.is_empty() {
                entry.insert("file".to_string(), json!(info.file));
            }
            if let Some(base) = &info.base_class {
                entry.insert("baseClass".to_string(), json!(base));
            }
            if !info.decorators.is_empty() {
                entry.insert("decorators".to_string(), json!(info.decorators));
            }
            if !info.refs.is_empty() {
                let mut refs = info.refs.clone();
                refs.sort();
                entry.insert("refs".to_string(), json!(refs));
            }
            if !info.sub.is_empty() {
                let mut sub = info.sub.clone();
                sub.sort();
                entry.insert("sub".to_string(), json!(sub));
            }
            if !info.enums.is_empty() {
                let mut e = info.enums.clone();
                e.sort();
                entry.insert("enums".to_string(), json!(e));
            }
            if let Some(table) = &info.table_name {
                entry.insert("tableName".to_string(), json!(table));
            }
            entities.insert(name.clone(), Value::Object(entry));
        }
    }

    // BTreeMap iteration above is alphabetical, so `_enums` / `e` keys come out
    // sorted (matching the JS `sortKeys`). The top-level + `_meta` key order is
    // pinned by the `Registry` / `RegistryMeta` structs.
    Registry {
        meta: RegistryMeta {
            version: "4.0",
            generated: current_date(),
            generator: "mustard-rt run sync-registry",
        },
        patterns: Value::Object(patterns),
        enums: Value::Object(enums),
        e: Value::Object(entities),
    }
}

/// Compress an enum value list — a port of `_compressValues` (>8 ⇒ first 5 + count).
fn compress_values(values: &[String]) -> Vec<String> {
    if values.len() > 8 {
        let mut out: Vec<String> = values.iter().take(5).cloned().collect();
        out.push(format!("...({} total)", values.len()));
        out
    } else {
        values.to_vec()
    }
}

/// Today's date as `YYYY-MM-DD` (UTC) — matches the JS `new Date().toISOString()`.
fn current_date() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    // Days since 1970-01-01, converted with the civil-from-days algorithm.
    let days = (secs / 86_400) as i64;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Convert a days-since-epoch count to a `(year, month, day)` — Howard Hinnant's
/// civil-from-days algorithm (public domain).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

// --- description-enricher --------------------------------------------------

/// Max description length / max scan size — mirror `MAX_LEN` / `MAX_SCAN_LINES`.
const MAX_DESC_LEN: usize = 200;
const MAX_SCAN_LINES: usize = 10_000;

/// Walk the registry's `e` map, adding a `description` from the entity's first
/// ref file — a port of `enrichDescriptions()`. Returns `(enriched, scanned)`.
///
/// Wave 1 — `visited` carries the per-subproject file contents read by the
/// scanner pass; the enricher prefers an in-memory hit (matching against
/// every cached absolute path) before falling back to a fresh disk read.
fn enrich_descriptions(
    entities_value: &mut Value,
    project_root: &Path,
    visited: &BTreeMap<std::path::PathBuf, String>,
) -> (usize, usize) {
    let mut enriched = 0;
    let mut scanned = 0;
    let Some(entities) = entities_value.as_object_mut() else {
        return (0, 0);
    };
    for (name, entry) in entities.iter_mut() {
        let Value::Object(map) = entry else {
            continue;
        };
        if map.contains_key("description") {
            continue;
        }
        // `refs[0]` is the canonical file; entities here carry only `file`,
        // so fall back to that — both are relative paths.
        let ref_path = map
            .get("refs")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(Value::as_str)
            .or_else(|| map.get("file").and_then(Value::as_str));
        let Some(ref_path) = ref_path else {
            continue;
        };
        scanned += 1;
        // Resolve the cached content. Entity `file` paths are relative to
        // their subproject root, so the absolute form may differ across
        // monorepo subprojects — match by suffix against every visited path.
        let content = lookup_visited(visited, ref_path, project_root);
        if let Some(desc) = extract_description_from(name, content.as_deref(), ref_path, project_root) {
            map.insert("description".to_string(), json!(desc));
            enriched += 1;
        }
    }
    (enriched, scanned)
}

/// Find the cached content for `ref_path` (which is relative to a subproject)
/// among the visited files of every subproject in the monorepo. Falls through
/// to a direct `project_root.join(ref_path)` lookup as a last attempt.
fn lookup_visited(
    visited: &BTreeMap<std::path::PathBuf, String>,
    ref_path: &str,
    project_root: &Path,
) -> Option<String> {
    if Path::new(ref_path).is_absolute() {
        return visited.get(Path::new(ref_path)).cloned();
    }
    // Try `<project_root>/<ref_path>` first (single-root projects).
    let direct = project_root.join(ref_path);
    if let Some(hit) = visited.get(&direct) {
        return Some(hit.clone());
    }
    // Fall back to suffix matching — handles `apps/rt/src/foo.rs` vs an entity
    // file recorded as `src/foo.rs` (relative to the `apps/rt` subproject).
    let needle = ref_path.replace('\\', "/");
    for (abs, content) in visited {
        let abs_str = abs.to_string_lossy().replace('\\', "/");
        if abs_str.ends_with(&needle) {
            return Some(content.clone());
        }
    }
    None
}

/// Extract a description with an optional pre-read content. Falls back to
/// reading `<project_root>/<ref_path>` when the cache had nothing for it.
fn extract_description_from(
    entity_name: &str,
    cached: Option<&str>,
    ref_path: &str,
    project_root: &Path,
) -> Option<String> {
    if entity_name.is_empty() {
        return None;
    }
    let owned: String;
    let raw: &str = if let Some(c) = cached {
        c
    } else {
        // Disk fallback — preserves the historical behaviour on paths the
        // visitor never saw (e.g. cross-subproject `refs[0]` pointers).
        owned = fs::read_to_string(if Path::new(ref_path).is_absolute() {
            std::path::PathBuf::from(ref_path)
        } else {
            project_root.join(ref_path)
        })
        .ok()?;
        &owned
    };
    extract_description_inner(raw, entity_name)
}

/// Extract a doc-comment description for `entity_name` from the contents of a
/// source file — a faithful port of `extractDescription()`. Operates on an
/// already-read string so the caller can supply cached bytes.
fn extract_description_inner(raw: &str, entity_name: &str) -> Option<String> {
    let lines: Vec<&str> = raw.split('\n').collect();
    if lines.len() > MAX_SCAN_LINES {
        return None;
    }
    // Find the declaration line. The JS uses three regexes; here the same
    // intent is expressed with token checks: a declaration keyword followed by
    // the entity name, or a table-constructor call naming it.
    let decl_keywords = [
        "class",
        "interface",
        "struct",
        "enum",
        "type",
        "def",
        "function",
        "fn",
        "const",
        "let",
        "var",
    ];
    let table_ctors = ["pgTable", "sqliteTable", "mysqlTable", "Table", "@Entity"];
    let lower_name = entity_name.to_lowercase();
    let mut decl_line: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        let has_decl = decl_keywords.iter().any(|kw| {
            line.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .collect::<Vec<_>>()
                .windows(2)
                .any(|w| w[0] == *kw && w[1] == entity_name)
        });
        let has_table = table_ctors.iter().any(|ctor| {
            if let Some(idx) = line.find(ctor) {
                let after = &line[idx + ctor.len()..];
                let after = after.trim_start();
                if let Some(after) = after.strip_prefix('(') {
                    let arg = after.trim_start().trim_start_matches(['\'', '"']);
                    return arg.starts_with(entity_name)
                        || arg.to_lowercase().starts_with(&lower_name);
                }
            }
            false
        });
        if has_decl || has_table {
            decl_line = Some(i);
            break;
        }
    }
    let decl_line = decl_line?;

    // Walk backward past one blank line to the preceding doc-comment block.
    let mut i = decl_line as isize - 1;
    while i >= 0 && lines[i as usize].trim().is_empty() {
        i -= 1;
    }
    if i < 0 {
        return None;
    }
    let i = i as usize;
    let line = lines[i];

    if line.contains("*/") {
        // `/** ... */` block — collect upward to the opener.
        let mut collected: Vec<&str> = Vec::new();
        let mut j = i as isize;
        while j >= 0 {
            collected.insert(0, lines[j as usize]);
            if lines[j as usize].contains("/**") || lines[j as usize].contains("/*") {
                break;
            }
            j -= 1;
        }
        return clean_doc_block(&collected.join("\n"), DocKind::Jsdoc);
    }
    let trimmed = line.trim_start();
    if trimmed.starts_with("///") || trimmed.starts_with("//!") {
        let mut collected: Vec<&str> = Vec::new();
        let mut j = i as isize;
        while j >= 0 {
            let t = lines[j as usize].trim_start();
            if !(t.starts_with("///") || t.starts_with("//!")) {
                break;
            }
            collected.insert(0, lines[j as usize]);
            j -= 1;
        }
        return clean_doc_block(&collected.join("\n"), DocKind::TripleSlash);
    }
    if trimmed.starts_with("//") {
        let mut collected: Vec<&str> = Vec::new();
        let mut j = i as isize;
        while j >= 0 {
            let t = lines[j as usize].trim_start();
            if !t.starts_with("//") || t.starts_with("///") || t.starts_with("//!") {
                break;
            }
            collected.insert(0, lines[j as usize]);
            j -= 1;
        }
        return clean_doc_block(&collected.join("\n"), DocKind::Line);
    }
    if trimmed.starts_with('#') && !trimmed.starts_with("#!") {
        let mut collected: Vec<&str> = Vec::new();
        let mut j = i as isize;
        while j >= 0 {
            let t = lines[j as usize].trim_start();
            if !t.starts_with('#') || t.starts_with("#!") {
                break;
            }
            collected.insert(0, lines[j as usize]);
            j -= 1;
        }
        return clean_doc_block(&collected.join("\n"), DocKind::Hash);
    }
    None
}

/// The doc-comment marker style — selects how `clean_doc_block` strips markers.
enum DocKind {
    Jsdoc,
    TripleSlash,
    Line,
    Hash,
}

/// Strip comment markers, collapse whitespace, truncate — a port of `cleanDocBlock`.
fn clean_doc_block(text: &str, kind: DocKind) -> Option<String> {
    let mut out_lines: Vec<String> = Vec::new();
    for raw in text.lines() {
        let mut line = raw.to_string();
        match kind {
            DocKind::Jsdoc => {
                line = line.replace("/**", "").replace("/*", "").replace("*/", "");
                let t = line.trim_start();
                line = t
                    .strip_prefix("* ")
                    .or_else(|| t.strip_prefix('*'))
                    .unwrap_or(t)
                    .to_string();
                // Drop JSDoc tag lines (`@param`, `@returns`, …).
                if line.trim_start().starts_with('@') {
                    continue;
                }
            }
            DocKind::TripleSlash => {
                let t = line.trim_start();
                line = t
                    .strip_prefix("/// ")
                    .or_else(|| t.strip_prefix("///"))
                    .or_else(|| t.strip_prefix("//! "))
                    .or_else(|| t.strip_prefix("//!"))
                    .unwrap_or(t)
                    .to_string();
            }
            DocKind::Line => {
                let t = line.trim_start();
                line = t
                    .strip_prefix("// ")
                    .or_else(|| t.strip_prefix("//"))
                    .unwrap_or(t)
                    .to_string();
            }
            DocKind::Hash => {
                let t = line.trim_start();
                line = t
                    .strip_prefix("# ")
                    .or_else(|| t.strip_prefix('#'))
                    .unwrap_or(t)
                    .to_string();
            }
        }
        out_lines.push(line);
    }
    let collapsed: String = out_lines.join(" ");
    let normalized: String = collapsed.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return None;
    }
    if normalized.chars().count() > MAX_DESC_LEN {
        let truncated: String = normalized.chars().take(MAX_DESC_LEN - 1).collect();
        return Some(format!("{truncated}\u{2026}"));
    }
    Some(normalized)
}

// --- subproject discovery (CLAUDE.md scan) ---------------------------------

/// Discover subprojects via the `CLAUDE.md` BFS scan — the same discovery
/// `sync-detect` uses. Single-root projects fall back to `.`.
fn discover_subprojects(root: &Path) -> Vec<Subproject> {
    const IGNORE: &[&str] = &[
        "node_modules",
        "bin",
        "obj",
        "dist",
        ".next",
        "_backup",
        "migrations",
        ".claude",
        ".git",
    ];
    let mut rel_paths: Vec<String> = Vec::new();
    walk_for_claude_md(root, "", 0, IGNORE, &mut rel_paths);
    if rel_paths.is_empty() && root.join("CLAUDE.md").exists() {
        rel_paths.push(".".to_string());
    }
    rel_paths
        .into_iter()
        .filter_map(|rel| {
            let abs = if rel == "." {
                root.to_path_buf()
            } else {
                root.join(&rel)
            };
            if !abs.join("CLAUDE.md").exists() {
                return None;
            }
            let name = if rel == "." {
                root.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("project")
                    .to_string()
            } else {
                rel.rsplit('/').next().unwrap_or(&rel).to_string()
            };
            Some(Subproject { name, rel_path: rel })
        })
        .collect()
}

fn walk_for_claude_md(
    abs_dir: &Path,
    rel_dir: &str,
    depth: usize,
    ignore: &[&str],
    out: &mut Vec<String>,
) {
    if depth > 3 {
        return;
    }
    if depth > 0 && abs_dir.join("CLAUDE.md").exists() {
        out.push(rel_dir.replace('\\', "/"));
        return;
    }
    let Ok(entries) = fs::read_dir(abs_dir) else {
        return;
    };
    for e in entries {
        if !e.is_dir {
            continue;
        }
        let name = e.file_name.clone();
        if name.starts_with('.') || ignore.contains(&name.as_str()) {
            continue;
        }
        let next_rel = if rel_dir.is_empty() {
            name.clone()
        } else {
            format!("{rel_dir}/{name}")
        };
        walk_for_claude_md(&e.path, &next_rel, depth + 1, ignore, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn compress_values_summarizes_large_lists() {
        let vals: Vec<String> = (0..12).map(|i| format!("V{i}")).collect();
        let out = compress_values(&vals);
        assert_eq!(out.len(), 6);
        assert_eq!(out[5], "...(12 total)");
        let small = vec!["A".to_string(), "B".to_string()];
        assert_eq!(compress_values(&small), small);
    }

    #[test]
    fn extract_description_reads_jsdoc() {
        let raw = "/**\n * A registered user of the system.\n */\nexport class User {}\n";
        let desc = extract_description_inner(raw, "User");
        assert_eq!(desc.as_deref(), Some("A registered user of the system."));
    }

    #[test]
    fn extract_description_reads_triple_slash() {
        let raw = "/// A customer order.\nstruct Order;\n";
        assert_eq!(
            extract_description_inner(raw, "Order").as_deref(),
            Some("A customer order.")
        );
    }

    #[test]
    fn civil_from_days_known_epoch() {
        // 1970-01-01 is day 0.
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // 2026-05-19 — sanity check the format helper produces a 10-char date.
        assert_eq!(current_date().len(), 10);
    }

    #[test]
    fn build_registry_has_v4_shape() {
        let scan: BTreeMap<String, MergedStack> = BTreeMap::new();
        let reg = build_registry(&scan);
        assert_eq!(reg.meta.version, "4.0");
        assert!(reg.patterns.is_object());
        assert!(reg.enums.is_object());
        assert!(reg.e.is_object());
        // Top-level key order must be _meta, _patterns, _enums, e (JS order).
        let json = serde_json::to_string(&reg).unwrap();
        let meta = json.find("\"_meta\"").unwrap();
        let patterns = json.find("\"_patterns\"").unwrap();
        let enums = json.find("\"_enums\"").unwrap();
        let e = json.find("\"e\"").unwrap();
        assert!(meta < patterns && patterns < enums && enums < e);
    }

    #[test]
    fn discover_subprojects_falls_back_to_root() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "# root").unwrap();
        let subs = discover_subprojects(dir.path());
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].rel_path, ".");
    }
}
