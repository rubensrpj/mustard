//! Generic, technology-agnostic structural-cluster discovery — a port of
//! `registry/cluster-discovery.js`.
//!
//! Discovers recurring code structures (shared filename suffixes, base classes,
//! decorators, function prefixes, repeated basenames) purely from the
//! filesystem — Mustard knows zero technology names; every label emerges from
//! the user's own source. Clusters are emitted as `serde_json::Value` objects
//! whose key set is byte-identical to the JS descriptors, so they drop straight
//! into `_patterns.{stack}.discovered[]`.
//!
//! A per-subproject SHA-256 cache (`<sub>/.claude/.cluster-cache.json`) skips
//! re-discovery when the scanned file-set is unchanged — faithfully ported,
//! including the tunable-aware cache key.

use super::file_utils::{collect_files, read_file_safe, relative_path};
use super::project_conventions::{primary_ext_for_stack, resolve_primary_ext};
use crate::util::sha256::Sha256;
use mustard_core::domain::vocabulary::language_caps::{self, LanguageCapabilities};
use mustard_core::io::fs as mfs;
use mustard_core::ClaudePaths;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

// --- Tunables (env-overridable, with a numeric floor) ----------------------

fn env_usize(key: &str, default: usize, floor: usize) -> usize {
    let raw = std::env::var(key)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(default);
    raw.max(floor)
}

fn min_files_per_suffix() -> usize {
    env_usize("MUSTARD_CLUSTER_MIN_FILES", 5, 2)
}
fn min_suffix_length() -> usize {
    env_usize("MUSTARD_CLUSTER_MIN_SUFFIX_LEN", 6, 2)
}
fn min_base_class_inheritors() -> usize {
    env_usize("MUSTARD_CLUSTER_MIN_BASE_INHERITORS", 3, 2)
}
fn min_decorator_usage() -> usize {
    env_usize("MUSTARD_DECORATOR_MIN", 3, 2)
}
fn min_function_prefix_usage() -> usize {
    env_usize("MUSTARD_FN_PREFIX_MIN", 5, 2)
}
fn min_function_prefix_len() -> usize {
    env_usize("MUSTARD_FN_PREFIX_MIN_LEN", 2, 2)
}
fn min_filename_folders() -> usize {
    env_usize("MUSTARD_FILENAME_MIN_FOLDERS", 3, 2)
}
fn max_clusters() -> usize {
    env_usize("MUSTARD_CLUSTER_MAX", 30, 1)
}
fn max_enrichment_samples() -> usize {
    env_usize("MUSTARD_ENRICHMENT_MAX", 5, 1)
}
fn cluster_cache_disabled() -> bool {
    std::env::var("MUSTARD_CLUSTER_CACHE")
        .is_ok_and(|v| v.to_lowercase() == "off")
}

/// Cache schema version — bumped when the cluster shape changes (JS is at v3).
const CLUSTER_CACHE_VERSION: u64 = 3;

/// Universal comment-line prefixes — covers most modern languages.
const COMMENT_PREFIXES: &[&str] = &["//", "#", "--", "/*", ";", "%"];

/// Structural basenames skipped from filename-cluster detection.
const STRUCTURAL_BASENAMES: &[&str] = &[
    "page",
    "layout",
    "loading",
    "error",
    "not-found",
    "route",
    "middleware",
    "template",
    "default",
    "global-error",
    "index",
    "main",
    "config",
    "types",
    "constants",
];

// --- Public entry points ----------------------------------------------------

/// Discover structural clusters in a subproject — a port of `discoverClusters()`.
///
/// `subproject_name`, when given, tags every emitted cluster (the orchestrator
/// slices clusters per agent by this tag). Fail-open: any error yields `[]`.
#[must_use]
pub fn discover_clusters(
    subproject_path: &Path,
    stack_id: &str,
    subproject_name: Option<&str>,
) -> Vec<Value> {
    // F0-e: an unknown stack must not early-return empty. Derive the project's
    // dominant source extension (or honour a `mustard.json#primaryExt` pin) so
    // suffix / folder / co-occurrence clustering still runs agnostically.
    let Some(ext) = resolve_primary_ext(subproject_path, stack_id) else {
        return Vec::new();
    };
    let ext = ext.as_str();
    // Language capabilities drive every per-language gate below (decorators,
    // fn-prefix, base-class syntax) from DATA rather than a hard-coded
    // `matches!(stack_id, "...")`. Fail-open: a missing/malformed override
    // degrades to the built-in base, so the gates keep today's values.
    let caps =
        LanguageCapabilities::load(language_caps::DEFAULT_LANGUAGE_CAPS_NAME, subproject_path)
            .unwrap_or_else(|_| LanguageCapabilities::builtin().unwrap_or_default());
    let all_files = collect_files(subproject_path, ext, &[]);
    let all_files: Vec<String> = all_files
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();

    // --- Cache lookup -------------------------------------------------------
    let mut file_set_hash = String::new();
    if !cluster_cache_disabled() {
        file_set_hash = compute_file_set_hash(stack_id, &all_files);
        if !file_set_hash.is_empty() {
            if let Some(cached) = read_cluster_cache(subproject_path) {
                if let Some(entry) = cached
                    .get("entries")
                    .and_then(|e| e.get(stack_id))
                    .filter(|e| e.get("hash").and_then(Value::as_str) == Some(&file_set_hash))
                {
                    if let Some(clusters) = entry.get("clusters").and_then(Value::as_array) {
                        let mut out: Vec<Value> =
                            clusters.iter().take(max_clusters()).cloned().collect();
                        // Re-apply subprojectName on cache hit: a cold-path
                        // caller (no name) may have populated the cache first;
                        // the warm caller (sync_entity_registry) re-runs with the
                        // real name and expects the tag in the output.
                        if let Some(name) = subproject_name {
                            for cluster in &mut out {
                                if let Value::Object(map) = cluster {
                                    map.insert("subprojectName".to_string(), json!(name));
                                }
                            }
                        }
                        return out;
                    }
                }
            }
        }
    }

    // Step 1 — global suffix scan; Step 2 — per-folder suffix clusters.
    let global = discover_global_suffix_clusters(subproject_path, &all_files, ext);
    let folder = discover_folder_clusters(subproject_path, &all_files, ext);
    let (consolidated, remaining) = consolidate_clusters(folder);
    // Step 3 — base-class clusters; Step 5 — decorator; Step 6 — fn-prefix.
    // GATE is driven by capabilities: base-class clustering runs only for a
    // stack that declares a `(class_kw, extends_kw)` pair, using those keywords
    // instead of literal "class "/"extends ".
    let base_class = match caps.base_class_syntax(stack_id) {
        Some((class_kw, extends_kw)) => {
            discover_base_class_clusters(subproject_path, &all_files, ext, class_kw, extends_kw)
        }
        None => Vec::new(),
    };
    let decorator = discover_decorator_clusters(subproject_path, &all_files, stack_id, &caps);
    let fn_prefix =
        discover_function_prefix_clusters(subproject_path, &all_files, stack_id, &caps);
    // Step 7 — filename clusters. Some stacks have a secondary source extension
    // the pass should also scan (e.g. typescript's `.tsx`); driven by the
    // language-capabilities vocab (DATA), not a hard-coded stack literal.
    let extra: Vec<String> = caps
        .secondary_exts(stack_id)
        .iter()
        .flat_map(|ext| collect_files(subproject_path, ext, &[]))
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    let filename = discover_filename_clusters(subproject_path, &all_files, &extra);

    let mut all: Vec<Value> = Vec::new();
    all.extend(global);
    all.extend(consolidated);
    all.extend(remaining);
    all.extend(base_class);
    all.extend(decorator);
    all.extend(fn_prefix);
    all.extend(filename);
    let mut merged = merge_clusters(all);
    merged.sort_by_key(|b| std::cmp::Reverse(file_count(b)));

    let mut kept: Vec<Value> = merged.into_iter().take(max_clusters()).collect();

    // T3.2 fallback — when no cluster qualified but the subproject has ≥5
    // source files, emit one coarse "folder" cluster per parent folder with
    // ≥3 sibling files of the primary extension. Keeps the contract that
    // every subproject with a meaningful surface area surfaces at least one
    // cluster in `_patterns.{stack}.discovered[]`.
    if kept.is_empty() && all_files.len() >= 5 {
        let mut by_folder: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for f in &all_files {
            let rel = relative_path(subproject_path, Path::new(f));
            let folder = parent_dir(&rel);
            by_folder.entry(folder).or_default().push(basename(&rel));
        }
        for (folder, files) in &by_folder {
            if files.len() < 3 || folder == "." {
                continue;
            }
            let label = folder
                .rsplit('/')
                .find(|seg| !seg.is_empty())
                .unwrap_or(folder)
                .to_string();
            let samples: Vec<String> = files.iter().take(3).cloned().collect();
            kept.push(json!({
                "kind": "folder-fallback-cluster",
                "label": label,
                "suffix": label,
                "ext": ext,
                "fileCount": files.len(),
                "folders": [folder.clone()],
                "folderPattern": format!("{folder}/"),
                "samples": samples,
            }));
            if kept.len() >= max_clusters() {
                break;
            }
        }
    }

    // Enrichment — universal metadata extracted from samples, once per cluster.
    // The grammar loader (in-crate grammars + discovered externals) is built
    // once and shared across all clusters so the agnostic decl-count uses the
    // same `mustard_core::domain::ast::extract_entities` the registry does.
    let loader = mustard_core::domain::ast::GrammarLoader::with_builtins(subproject_path);
    for cluster in &mut kept {
        enrich_cluster(cluster, subproject_path, &loader);
        if let Some(name) = subproject_name {
            if let Value::Object(map) = cluster {
                map.insert("subprojectName".to_string(), json!(name));
            }
        }
    }

    // --- Cache write --------------------------------------------------------
    if !cluster_cache_disabled() && !file_set_hash.is_empty() {
        write_cluster_cache(subproject_path, stack_id, &file_set_hash, &kept);
    }

    kept
}

/// Compute folder-segment frequency — a port of `computeFolderFrequency()`.
#[must_use]
pub fn compute_folder_frequency(subproject_path: &Path, stack_id: &str) -> Value {
    // F0-e: agnostic fallback so an unknown stack still reports its folders.
    let Some(ext) = resolve_primary_ext(subproject_path, stack_id) else {
        return json!({ "totalFolders": 0, "segments": {} });
    };
    let ext = ext.as_str();
    let files = collect_files(subproject_path, ext, &[]);
    let mut folder_set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for f in &files {
        let rel = relative_path(subproject_path, f);
        folder_set.insert(parent_dir(&rel));
    }
    let mut segments: BTreeMap<String, u64> = BTreeMap::new();
    for folder in &folder_set {
        let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        for part in folder.split('/').filter(|p| !p.is_empty()) {
            if seen.insert(part) {
                *segments.entry(part.to_string()).or_insert(0) += 1;
            }
        }
    }
    let seg_obj: serde_json::Map<String, Value> =
        segments.into_iter().map(|(k, v)| (k, json!(v))).collect();
    json!({ "totalFolders": folder_set.len(), "segments": seg_obj })
}

// --- Cache helpers ----------------------------------------------------------

fn cluster_cache_path(subproject_path: &Path) -> std::path::PathBuf {
    // Each subproject has its own .claude/ dir; use ClaudePaths as the catalog
    // anchor so the path stays consistent with the broader accessor contract.
    ClaudePaths::for_project(subproject_path)
        .map(|p| p.claude_dir().join(".cluster-cache.json"))
        .unwrap_or_default()
}

fn compute_file_set_hash(stack_id: &str, files: &[String]) -> String {
    let tunables = format!(
        "{},{},{},{},{},{},{},{}",
        min_files_per_suffix(),
        min_suffix_length(),
        min_base_class_inheritors(),
        max_clusters(),
        min_decorator_usage(),
        min_function_prefix_usage(),
        min_function_prefix_len(),
        min_filename_folders(),
    );
    let mut hash = Sha256::new();
    hash.update(format!("v{CLUSTER_CACHE_VERSION}|{stack_id}|t={tunables}|").as_bytes());
    let mut sorted: Vec<&String> = files.iter().collect();
    sorted.sort();
    for f in sorted {
        match mfs::modified(Path::new(f)) {
            Ok(t) => {
                let mtime = t
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .map_or(0, |d| d.as_millis());
                hash.update(format!("{f}|{mtime}\n").as_bytes());
            }
            Err(_) => hash.update(format!("{f}|missing\n").as_bytes()),
        }
    }
    hash.hex_digest().chars().take(16).collect()
}

fn read_cluster_cache(subproject_path: &Path) -> Option<Value> {
    let raw = mfs::read_to_string(cluster_cache_path(subproject_path)).ok()?;
    let parsed: Value = serde_json::from_str(&raw).ok()?;
    if parsed.get("cacheVersion").and_then(Value::as_u64) != Some(CLUSTER_CACHE_VERSION) {
        return None;
    }
    Some(parsed)
}

fn write_cluster_cache(subproject_path: &Path, stack_id: &str, hash: &str, clusters: &[Value]) {
    let path = cluster_cache_path(subproject_path);
    let mut payload = read_cluster_cache(subproject_path)
        .unwrap_or_else(|| json!({ "cacheVersion": CLUSTER_CACHE_VERSION, "entries": {} }));
    if let Value::Object(map) = &mut payload {
        map.insert("cacheVersion".to_string(), json!(CLUSTER_CACHE_VERSION));
        let entries = map
            .entry("entries".to_string())
            .or_insert_with(|| json!({}));
        if let Value::Object(entries_map) = entries {
            entries_map.insert(
                stack_id.to_string(),
                json!({ "hash": hash, "clusters": clusters }),
            );
        }
    }
    if let Ok(serialized) = serde_json::to_string(&payload) {
        let _ = mfs::write_atomic(&path, serialized.as_bytes());
    }
}

// --- Suffix / PascalCase helpers -------------------------------------------

fn parent_dir(rel: &str) -> String {
    let norm = rel.replace('\\', "/");
    match norm.rfind('/') {
        Some(idx) => norm[..idx].to_string(),
        None => ".".to_string(),
    }
}

fn file_count(c: &Value) -> u64 {
    c.get("fileCount").and_then(Value::as_u64).unwrap_or(0)
}

/// Split a PascalCase identifier into its words — a port of `_splitPascalCase`.
fn split_pascal_case(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut words: Vec<String> = Vec::new();
    let mut cur = String::new();
    for i in 0..chars.len() {
        let c = chars[i];
        // Separators (`_`, `-`) split words and are dropped — this makes the
        // splitter agnostic to snake_case / kebab-case, not just PascalCase, so
        // `user_repository` → ["user", "repository"]. Without this, every
        // snake_case basename collapses to a single word and is dropped by the
        // `words.len() < 2` gate, blinding the suffix pass to Rust/Python/Ruby.
        if c == '_' || c == '-' {
            if !cur.is_empty() {
                words.push(std::mem::take(&mut cur));
            }
            continue;
        }
        let boundary = if i == 0 {
            false
        } else {
            let prev = chars[i - 1];
            // Before [A-Z][a-z], or after [a-z] before [A-Z].
            let upper_lower =
                c.is_ascii_uppercase() && chars.get(i + 1).is_some_and(|n| n.is_ascii_lowercase());
            let lower_upper = prev.is_ascii_lowercase() && c.is_ascii_uppercase();
            upper_lower || lower_upper
        };
        if boundary && !cur.is_empty() {
            words.push(std::mem::take(&mut cur));
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words.into_iter().filter(|w| !w.is_empty()).collect()
}

/// Common deepest folder segment shared by all folders — `_commonFolderSegment`.
fn common_folder_segment(folders: &[String]) -> Option<String> {
    let first = folders.first()?;
    let segs: Vec<&str> = first.split('/').filter(|s| !s.is_empty()).collect();
    let common: Vec<&str> = segs
        .iter()
        .copied()
        .filter(|seg| folders.iter().all(|f| f.split('/').any(|s| s == *seg)))
        .collect();
    common.last().map(|s| (*s).to_string())
}

/// Build a folder-pattern string from a list of folders.
fn folder_pattern(folders: &[String]) -> String {
    if folders.len() == 1 {
        format!("{}/", folders[0])
    } else {
        match common_folder_segment(folders) {
            Some(seg) => format!("**/{seg}/"),
            None => "(multiple)".to_string(),
        }
    }
}

/// Group basenames by shared trailing PascalCase word groups — `_groupBySuffix`.
fn group_by_suffix(basenames: &[String]) -> BTreeMap<String, Vec<String>> {
    let min_len = min_suffix_length();
    let mut result: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for name in basenames {
        let words = split_pascal_case(name);
        if words.len() < 2 {
            continue;
        }
        for word_count in 1..words.len() {
            let suffix: String = words[words.len() - word_count..].concat();
            if suffix.len() < min_len {
                continue;
            }
            result.entry(suffix).or_default().push(name.clone());
        }
    }
    result.retain(|_, names| names.len() >= 2);
    prune_suffix_subsets(result)
}

/// Drop shorter suffixes fully contained in a longer, equally-matched suffix.
fn prune_suffix_subsets(
    suffix_map: BTreeMap<String, Vec<String>>,
) -> BTreeMap<String, Vec<String>> {
    let keys: Vec<String> = suffix_map.keys().cloned().collect();
    let mut to_delete: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for shorter in &keys {
        for longer in &keys {
            if longer == shorter || !longer.ends_with(shorter.as_str()) {
                continue;
            }
            let shorter_set: std::collections::BTreeSet<&String> =
                suffix_map[shorter].iter().collect();
            let longer_set: std::collections::BTreeSet<&String> =
                suffix_map[longer].iter().collect();
            if longer_set.iter().all(|n| shorter_set.contains(n))
                && longer_set.len() == shorter_set.len()
            {
                to_delete.insert(shorter.clone());
            }
        }
    }
    suffix_map
        .into_iter()
        .filter(|(k, _)| !to_delete.contains(k))
        .collect()
}

// --- Step 1: per-folder suffix clusters ------------------------------------

fn discover_folder_clusters(subproject_path: &Path, all_files: &[String], ext: &str) -> Vec<Value> {
    let mut by_folder: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for f in all_files {
        let rel = relative_path(subproject_path, Path::new(f));
        let dir = parent_dir(&rel);
        let base = basename_no_ext(&rel, ext);
        by_folder.entry(dir).or_default().push(base);
    }
    let mut clusters = Vec::new();
    for (folder, bases) in &by_folder {
        if bases.len() < 2 {
            continue;
        }
        for (suffix, matching) in group_by_suffix(bases) {
            if matching.len() < 2 {
                continue;
            }
            let samples: Vec<String> = matching
                .iter()
                .take(3)
                .map(|b| format!("{b}{ext}"))
                .collect();
            clusters.push(json!({
                "kind": "folder-cluster",
                "folder": folder,
                "suffix": suffix,
                "ext": ext,
                "fileCount": matching.len(),
                "samples": samples,
                "label": suffix,
            }));
        }
    }
    clusters
}

// --- Step 1b: global suffix scan -------------------------------------------

fn discover_global_suffix_clusters(
    subproject_path: &Path,
    all_files: &[String],
    ext: &str,
) -> Vec<Value> {
    let min_len = min_suffix_length();
    let min_files = min_files_per_suffix();
    // suffix -> Vec<(base, folder, file)>
    let mut suffix_to_files: BTreeMap<String, Vec<(String, String, String)>> = BTreeMap::new();
    for f in all_files {
        let rel = relative_path(subproject_path, Path::new(f));
        let dir = parent_dir(&rel);
        let base = basename_no_ext(&rel, ext);
        let words = split_pascal_case(&base);
        if words.len() < 2 {
            continue;
        }
        let file = basename(&rel);
        for word_count in 1..words.len() {
            let suffix: String = words[words.len() - word_count..].concat();
            if suffix.len() < min_len {
                continue;
            }
            suffix_to_files.entry(suffix).or_default().push((
                base.clone(),
                dir.clone(),
                file.clone(),
            ));
        }
    }
    suffix_to_files.retain(|_, files| files.len() >= min_files);
    // Prune subset suffixes.
    let names: Vec<String> = suffix_to_files.keys().cloned().collect();
    let mut to_delete: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for shorter in &names {
        for longer in &names {
            if longer == shorter || !longer.ends_with(shorter.as_str()) {
                continue;
            }
            let shorter_bases: std::collections::BTreeSet<&String> =
                suffix_to_files[shorter].iter().map(|(b, _, _)| b).collect();
            let longer_bases: std::collections::BTreeSet<&String> =
                suffix_to_files[longer].iter().map(|(b, _, _)| b).collect();
            if longer_bases.iter().all(|b| shorter_bases.contains(b))
                && longer_bases.len() == shorter_bases.len()
            {
                to_delete.insert(shorter.clone());
            }
        }
    }
    for s in &to_delete {
        suffix_to_files.remove(s);
    }
    let mut clusters = Vec::new();
    for (suffix, files) in &suffix_to_files {
        let mut folders: Vec<String> = files.iter().map(|(_, d, _)| d.clone()).collect();
        folders.sort();
        folders.dedup();
        let pattern = if folders.len() == 1 {
            format!("{}/", folders[0])
        } else {
            match common_folder_segment(&folders) {
                Some(seg) => format!("**/{seg}/"),
                None => "(multiple)".to_string(),
            }
        };
        let samples: Vec<String> = files.iter().take(3).map(|(_, _, f)| f.clone()).collect();
        clusters.push(json!({
            "kind": "suffix-cluster",
            "suffix": suffix,
            "ext": ext,
            "fileCount": files.len(),
            "folders": folders,
            "folderPattern": pattern,
            "samples": samples,
            "label": suffix,
        }));
    }
    clusters
}

/// Merge cluster arrays, deduping by (kind, suffix-ish, ext) — `_mergeClusters`.
fn merge_clusters(clusters: Vec<Value>) -> Vec<Value> {
    let mut by_key: BTreeMap<String, Value> = BTreeMap::new();
    for c in clusters {
        let kind = c.get("kind").and_then(Value::as_str).unwrap_or("");
        let id = c
            .get("suffix")
            .or_else(|| c.get("commonBaseClass"))
            .or_else(|| c.get("decorator"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let ext = c.get("ext").and_then(Value::as_str).unwrap_or("");
        let key = format!("{kind}|{id}|{ext}");
        match by_key.get(&key) {
            Some(existing) if file_count(existing) >= file_count(&c) => {}
            _ => {
                by_key.insert(key, c);
            }
        }
    }
    by_key.into_values().collect()
}

// --- Step 2: consolidation across folders ----------------------------------

fn consolidate_clusters(folder_clusters: Vec<Value>) -> (Vec<Value>, Vec<Value>) {
    let min_files = min_files_per_suffix();
    let mut by_suffix: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for c in folder_clusters {
        let suffix = c.get("suffix").and_then(Value::as_str).unwrap_or("");
        let ext = c.get("ext").and_then(Value::as_str).unwrap_or("");
        by_suffix
            .entry(format!("{suffix}{ext}"))
            .or_default()
            .push(c);
    }
    let mut consolidated = Vec::new();
    let mut remaining = Vec::new();
    for (_, group) in by_suffix {
        let total: u64 = group.iter().map(file_count).sum();
        if total < min_files as u64 {
            continue;
        }
        if group.len() > 1 {
            let folders: Vec<String> = group
                .iter()
                .filter_map(|c| {
                    c.get("folder")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .collect();
            let ext = group[0].get("ext").and_then(Value::as_str).unwrap_or("");
            let suffix = group[0]
                .get("suffix")
                .and_then(Value::as_str)
                .unwrap_or("");
            let mut samples: Vec<String> = Vec::new();
            for c in &group {
                if let Some(arr) = c.get("samples").and_then(Value::as_array) {
                    for s in arr {
                        if let Some(s) = s.as_str() {
                            if !samples.contains(&s.to_string()) {
                                samples.push(s.to_string());
                            }
                        }
                    }
                }
            }
            samples.truncate(3);
            let pattern = match common_folder_segment(&folders) {
                Some(seg) => format!("**/{seg}/"),
                None => "(multiple)".to_string(),
            };
            consolidated.push(json!({
                "kind": "suffix-cluster",
                "suffix": suffix,
                "ext": ext,
                "fileCount": total,
                "folders": folders,
                "folderPattern": pattern,
                "samples": samples,
                "label": suffix,
            }));
        } else if file_count(&group[0]) >= min_files as u64 {
            remaining.push(group.into_iter().next().unwrap_or(Value::Null));
        }
    }
    (consolidated, remaining)
}

// --- Step 3b: base-class clusters -------------------------------------------

/// Discover base-class clusters for any stack whose capabilities declare a
/// `(class_kw, extends_kw)` pair. The GATE (whether this runs at all) and the
/// keyword pair both come from [`LanguageCapabilities::base_class_syntax`] —
/// nothing here is language-named. The extraction itself (scan for
/// `<class_kw>Name <extends_kw>Base`) is generic over the keyword pair.
fn discover_base_class_clusters(
    subproject_path: &Path,
    all_files: &[String],
    ext: &str,
    class_kw: &str,
    extends_kw: &str,
) -> Vec<Value> {
    // base -> Vec<(folder, file)>
    let mut inheritors: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for f in all_files {
        let Some(content) = read_file_safe(Path::new(f)) else {
            continue;
        };
        let rel = relative_path(subproject_path, Path::new(f));
        let folder = parent_dir(&rel);
        // Match `(export)? (abstract)? <class_kw>Name <extends_kw>Base`.
        let mut search = 0;
        while let Some(rel_idx) = content[search..].find(class_kw) {
            let idx = search + rel_idx;
            search = idx + class_kw.len();
            let after = &content[idx + class_kw.len()..];
            let cls: String = after
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if cls.is_empty() {
                continue;
            }
            let rest = after[cls.len()..].trim_start();
            let Some(rest) = rest.strip_prefix(extends_kw) else {
                continue;
            };
            let base: String = rest
                .trim_start()
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
                .collect();
            let bare = base.rsplit('.').next().unwrap_or(&base).to_string();
            if bare.is_empty() {
                continue;
            }
            inheritors
                .entry(bare)
                .or_default()
                .push((folder.clone(), basename(&rel)));
        }
    }
    materialize_base_class_clusters(inheritors, ext)
}

fn materialize_base_class_clusters(
    inheritors: BTreeMap<String, Vec<(String, String)>>,
    ext: &str,
) -> Vec<Value> {
    let min = min_base_class_inheritors();
    let mut clusters = Vec::new();
    for (base, classes) in inheritors {
        if classes.len() < min {
            continue;
        }
        let mut folders: Vec<String> = classes.iter().map(|(f, _)| f.clone()).collect();
        folders.sort();
        folders.dedup();
        let samples: Vec<String> = classes.iter().take(3).map(|(_, f)| f.clone()).collect();
        clusters.push(json!({
            "kind": "base-class-cluster",
            "commonBaseClass": base,
            "suffix": base,
            "ext": ext,
            "fileCount": classes.len(),
            "folders": folders,
            "folderPattern": folder_pattern(&folders),
            "samples": samples,
            "label": base,
        }));
    }
    clusters
}

// --- Step 5: decorator clusters --------------------------------------------

fn discover_decorator_clusters(
    subproject_path: &Path,
    all_files: &[String],
    stack_id: &str,
    caps: &LanguageCapabilities,
) -> Vec<Value> {
    // GATE driven by DATA: only stacks whose capabilities declare decorator
    // syntax (a leading `@Name` prefixing a declaration) are scanned. The
    // declaration keywords a decorator may prefix come from the same caps entry.
    if !caps.has_decorators(stack_id) {
        return Vec::new();
    }
    let decl_keywords = caps.decl_keywords(stack_id);
    let min = min_decorator_usage();
    // decorator -> Set<relFile>
    let mut usage: BTreeMap<String, std::collections::BTreeSet<String>> = BTreeMap::new();
    for f in all_files {
        let Some(content) = read_file_safe(Path::new(f)) else {
            continue;
        };
        let rel = relative_path(subproject_path, Path::new(f));
        // Find `@Name` immediately followed (after whitespace/newline + optional
        // modifiers) by `class`/`function`/`def`/`interface`/`fun`.
        let mut search = 0;
        while let Some(rel_idx) = content[search..].find('@') {
            let idx = search + rel_idx;
            search = idx + 1;
            let name: String = content[idx + 1..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
                .collect();
            if name.is_empty() {
                continue;
            }
            // Skip the `@Name(...)` argument list if present, then scan ahead.
            let mut rest = content[idx + 1 + name.len()..].trim_start();
            if rest.starts_with('(') {
                if let Some(close) = rest.find(')') {
                    rest = rest[close + 1..].trim_start();
                }
            }
            let declares = decl_keywords.iter().any(|kw| {
                rest.split_whitespace()
                    .take(4)
                    .any(|tok| tok == kw.as_str())
            });
            if declares {
                let bare = name.rsplit('.').next().unwrap_or(&name).to_string();
                usage.entry(bare).or_default().insert(rel.clone());
            }
        }
    }
    let ext = primary_ext_for_stack(stack_id).unwrap_or("");
    let mut clusters = Vec::new();
    for (decorator, file_set) in usage {
        if file_set.len() < min {
            continue;
        }
        let files: Vec<String> = file_set.into_iter().collect();
        let mut folders: Vec<String> = files.iter().map(|f| parent_dir(f)).collect();
        folders.sort();
        folders.dedup();
        let samples: Vec<String> = files.iter().take(3).map(|f| basename(f)).collect();
        clusters.push(json!({
            "kind": "decorator-cluster",
            "decorator": decorator,
            "suffix": decorator,
            "ext": ext,
            "fileCount": files.len(),
            "folders": folders,
            "folderPattern": folder_pattern(&folders),
            "samples": samples,
            "label": decorator,
        }));
    }
    clusters
}

// --- Step 6: function-prefix clusters --------------------------------------

/// Extract the leading prefix of a camelCase/snake_case name — `_extractFunctionPrefix`.
fn extract_function_prefix(name: &str) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    let leading_underscores = name.chars().take_while(|c| *c == '_').count();
    let stripped = &name[leading_underscores..];
    if let Some(snake_idx) = stripped.find('_') {
        if snake_idx > 0 {
            return Some(name[..leading_underscores + snake_idx].to_string());
        }
    }
    // camelCase: leading `[a-z]+` followed by an uppercase letter.
    let lower_run: String = stripped
        .chars()
        .take_while(|c| c.is_ascii_lowercase())
        .collect();
    if !lower_run.is_empty()
        && stripped[lower_run.len()..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_uppercase())
    {
        return Some(name[..leading_underscores + lower_run.len()].to_string());
    }
    None
}

fn discover_function_prefix_clusters(
    subproject_path: &Path,
    all_files: &[String],
    stack_id: &str,
    caps: &LanguageCapabilities,
) -> Vec<Value> {
    // GATE driven by DATA: only stacks whose capabilities declare fn-prefix
    // detection are scanned. The per-language declaration PARSING below
    // (`top_level_function_names`) still switches on the literal stack id; that
    // inner switch is acceptable for now — only the GATE moved to caps.
    if !caps.has_fn_prefix(stack_id) {
        return Vec::new();
    }
    let min = min_function_prefix_usage();
    let min_len = min_function_prefix_len();
    let mut usage: BTreeMap<String, std::collections::BTreeSet<String>> = BTreeMap::new();
    for f in all_files {
        let Some(content) = read_file_safe(Path::new(f)) else {
            continue;
        };
        let rel = relative_path(subproject_path, Path::new(f));
        for line in content.lines() {
            let names = top_level_function_names(line, stack_id);
            for fn_name in names {
                if let Some(prefix) = extract_function_prefix(&fn_name) {
                    if prefix.len() >= min_len {
                        usage.entry(prefix).or_default().insert(rel.clone());
                    }
                }
            }
        }
    }
    let ext = primary_ext_for_stack(stack_id).unwrap_or("");
    let mut clusters = Vec::new();
    for (prefix, file_set) in usage {
        if file_set.len() < min {
            continue;
        }
        let files: Vec<String> = file_set.into_iter().collect();
        let mut folders: Vec<String> = files.iter().map(|f| parent_dir(f)).collect();
        folders.sort();
        folders.dedup();
        let samples: Vec<String> = files.iter().take(3).map(|f| basename(f)).collect();
        clusters.push(json!({
            "kind": "function-prefix-cluster",
            "prefix": prefix,
            "suffix": prefix,
            "ext": ext,
            "fileCount": files.len(),
            "folders": folders,
            "folderPattern": folder_pattern(&folders),
            "samples": samples,
            "label": prefix,
        }));
    }
    clusters
}

/// Extract top-level function names declared on a single line.
fn top_level_function_names(line: &str, stack_id: &str) -> Vec<String> {
    let mut out = Vec::new();
    let read_ident = |s: &str| -> String {
        s.chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect()
    };
    match stack_id {
        "typescript" => {
            // `export? async? function NAME(`
            let mut rest = line.trim_start();
            if rest.starts_with("export ") {
                rest = rest["export ".len()..].trim_start();
            }
            if rest.starts_with("async ") {
                rest = rest["async ".len()..].trim_start();
            }
            if let Some(after) = rest.strip_prefix("function ") {
                let name = read_ident(after.trim_start());
                if !name.is_empty() && after.trim_start()[name.len()..].trim_start().starts_with('(')
                {
                    out.push(name);
                }
            }
            // `export? const NAME = async? (`
            let mut rest = line.trim_start();
            if rest.starts_with("export ") {
                rest = rest["export ".len()..].trim_start();
            }
            if let Some(after) = rest.strip_prefix("const ") {
                let name = read_ident(after.trim_start());
                let tail = after.trim_start()[name.len()..].trim_start();
                if !name.is_empty() {
                    // Skip an optional `: Type` annotation, then require `= ( | = async (`.
                    let tail = tail.split('=').nth(1).map_or("", str::trim_start);
                    let tail = tail.strip_prefix("async ").map_or(tail, str::trim_start);
                    if tail.starts_with('(') {
                        out.push(name);
                    }
                }
            }
        }
        "python" => {
            let trimmed = line;
            // Top-level only — `def`/`async def` at column 0.
            if let Some(after) = trimmed.strip_prefix("def ") {
                let name = read_ident(after);
                if !name.is_empty() {
                    out.push(name);
                }
            } else if let Some(after) = trimmed.strip_prefix("async def ") {
                let name = read_ident(after);
                if !name.is_empty() {
                    out.push(name);
                }
            }
        }
        _ => {}
    }
    out
}

// --- Step 7: filename clusters ---------------------------------------------

fn discover_filename_clusters(
    subproject_path: &Path,
    all_files: &[String],
    extra_files: &[String],
) -> Vec<Value> {
    let min_folders = min_filename_folders();
    // basename -> Vec<(folder, file, ext)>
    let mut by_basename: BTreeMap<String, Vec<(String, String, String)>> = BTreeMap::new();
    for f in all_files.iter().chain(extra_files.iter()) {
        let rel = relative_path(subproject_path, Path::new(f));
        let folder = parent_dir(&rel);
        let file_ext = Path::new(&rel)
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let base = basename_no_ext(&rel, &file_ext);
        if STRUCTURAL_BASENAMES.contains(&base.to_lowercase().as_str()) || base.len() < 3 {
            continue;
        }
        by_basename
            .entry(base)
            .or_default()
            .push((folder, basename(&rel), file_ext));
    }
    let mut clusters = Vec::new();
    for (basename_key, occ) in &by_basename {
        let mut folders: Vec<String> = occ.iter().map(|(d, _, _)| d.clone()).collect();
        folders.sort();
        folders.dedup();
        if folders.len() < min_folders {
            continue;
        }
        // Sibling rule: a shared basename is a *pattern* only when the folders
        // are siblings — they share the same immediate parent directory. A
        // merely shared *ancestor* (`src/`) spanning different depths is
        // homonymy, not a convention: `src/io/workspace.rs` and
        // `src/domain/model/view/workspace.rs` name the same concept across
        // different layers; `features/auth/repo.ts` + `features/billing/repo.ts`
        // are the real pattern (same parent `features`, one level apart).
        let parents: std::collections::BTreeSet<String> =
            folders.iter().map(|f| parent_dir(f)).collect();
        if parents.len() != 1 {
            continue;
        }
        let parent = parents.into_iter().next().unwrap_or_default();
        let mut ext_counts: BTreeMap<String, usize> = BTreeMap::new();
        for (_, _, e) in occ {
            *ext_counts.entry(e.clone()).or_insert(0) += 1;
        }
        let dominant_ext = ext_counts
            .into_iter()
            .max_by_key(|(_, c)| *c)
            .map(|(e, _)| e)
            .unwrap_or_default();
        let pattern = if parent == "." || parent.is_empty() {
            format!("**/{basename_key}{dominant_ext}")
        } else {
            format!("**/{parent}/*/{basename_key}{dominant_ext}")
        };
        let samples: Vec<String> = occ
            .iter()
            .take(3)
            .map(|(d, f, _)| format!("{d}/{f}"))
            .collect();
        clusters.push(json!({
            "kind": "filename-cluster",
            "suffix": basename_key,
            "ext": dominant_ext,
            "fileCount": folders.len(),
            "folders": folders,
            "folderPattern": pattern,
            "samples": samples,
            "label": basename_key,
        }));
    }
    clusters
}

// --- Path helpers -----------------------------------------------------------

fn basename(rel: &str) -> String {
    rel.rsplit('/').next().unwrap_or(rel).to_string()
}

fn basename_no_ext(rel: &str, ext: &str) -> String {
    let b = basename(rel);
    b.strip_suffix(ext).unwrap_or(&b).to_string()
}

// --- Enrichment -------------------------------------------------------------

/// Enrich a cluster with up to 5 universal metadata fields — `_enrichCluster`.
fn enrich_cluster(
    cluster: &mut Value,
    subproject_path: &Path,
    loader: &mustard_core::domain::ast::GrammarLoader,
) {
    let Value::Object(map) = cluster else {
        return;
    };
    // Default every enrichment field to null (the JS sets, not omits, them).
    for key in [
        "namingPattern",
        "declarationKeywords",
        "declarationSuffix",
        "topOfFileLines",
        "memberSuffixes",
    ] {
        map.insert(key.to_string(), Value::Null);
    }
    let sample_paths = resolve_sample_paths(map, subproject_path);
    if sample_paths.is_empty() {
        return;
    }
    let contents: Vec<(std::path::PathBuf, String)> = sample_paths
        .iter()
        .filter_map(|p| read_file_safe(p).map(|c| (p.clone(), c)))
        .collect();
    if contents.is_empty() {
        return;
    }
    let target = map
        .get("suffix")
        .or_else(|| map.get("label"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    if let Some(np) = extract_naming_pattern(&target, &contents) {
        map.insert("namingPattern".to_string(), json!(np));
    }
    let tof = extract_top_of_file_lines(&contents);
    if !tof.is_empty() {
        map.insert("topOfFileLines".to_string(), json!(tof));
    }
    let members = extract_member_suffixes(&contents);
    if !members.is_empty() {
        map.insert("memberSuffixes".to_string(), json!(members));
    }

    // T3.3 — attach agnostic decl counts derived from the sampled file set,
    // now via the shared `mustard_core::domain::ast::extract_entities` (AST when
    // a grammar resolves, agnostic textual floor otherwise) so the cluster
    // metadata and the entity-registry are computed by the same extractor — no
    // local duplicate. The counts describe "how many named type declarations
    // live in the sample" without claiming to recognise the user's stack.
    // `declByKind` is keyed by the extractor's `kind` (an AST node kind such as
    // `struct_item` / `class_declaration`, or the floor keyword that fired).
    // Empty samples ⇒ skip.
    use mustard_core::domain::ast::extract_entities;
    let mut decl_count: usize = 0;
    let mut by_kind: BTreeMap<String, u64> = BTreeMap::new();
    for (path, source) in &contents {
        let lang_id = loader.language_id_for_path(path).unwrap_or_default();
        for ent in extract_entities(loader, source, &lang_id) {
            decl_count += 1;
            *by_kind.entry(ent.kind).or_insert(0) += 1;
        }
    }
    if decl_count > 0 {
        map.insert("declCount".to_string(), json!(decl_count));
        let kind_obj: serde_json::Map<String, Value> =
            by_kind.into_iter().map(|(k, v)| (k, json!(v))).collect();
        map.insert("declByKind".to_string(), Value::Object(kind_obj));
    }
}

fn resolve_sample_paths(
    map: &serde_json::Map<String, Value>,
    subproject_path: &Path,
) -> Vec<std::path::PathBuf> {
    let max = max_enrichment_samples();
    let samples: Vec<String> = map
        .get("samples")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let folders: Vec<String> = match map.get("folders").and_then(Value::as_array) {
        Some(a) => a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
        None => map
            .get("folder")
            .and_then(Value::as_str)
            .map(|f| vec![f.to_string()])
            .unwrap_or_default(),
    };
    let mut out = Vec::new();
    for sample in samples {
        if out.len() >= max {
            break;
        }
        let direct = subproject_path.join(&sample);
        if direct.is_file() {
            out.push(direct);
            continue;
        }
        for folder in &folders {
            let candidate = subproject_path.join(folder).join(&sample);
            if candidate.is_file() {
                out.push(candidate);
                break;
            }
        }
    }
    out
}

fn is_comment_line(line: &str) -> bool {
    let t = line.trim();
    !t.is_empty() && COMMENT_PREFIXES.iter().any(|p| t.starts_with(p))
}

/// `suffix-after` / `suffix-before` / null — a port of `_extractNamingPattern`.
fn extract_naming_pattern(target: &str, contents: &[(std::path::PathBuf, String)]) -> Option<String> {
    if target.len() < 2 {
        return None;
    }
    let mut after = 0;
    let mut before = 0;
    for (p, _) in contents {
        let ext = p
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let base = p
            .file_name()
            .and_then(|n| n.to_str())
            .map_or("", |n| n.strip_suffix(&ext).unwrap_or(n));
        if base == target {
            continue;
        }
        if base.ends_with(target) {
            after += 1;
        } else if base.starts_with(target) {
            before += 1;
        }
    }
    if after == 0 && before == 0 || after == before {
        return None;
    }
    Some(if after > before {
        "suffix-after".to_string()
    } else {
        "suffix-before".to_string()
    })
}

/// Non-comment lines shared by all samples (first 20 lines) — `_extractTopOfFileLines`.
fn extract_top_of_file_lines(contents: &[(std::path::PathBuf, String)]) -> Vec<String> {
    if contents.is_empty() {
        return Vec::new();
    }
    let sets: Vec<std::collections::BTreeSet<String>> = contents
        .iter()
        .map(|(_, c)| {
            c.lines()
                .take(20)
                .map(str::trim)
                .filter(|l| !l.is_empty() && !is_comment_line(l))
                .map(str::to_string)
                .collect()
        })
        .collect();
    let mut shared: Vec<String> = sets[0]
        .iter()
        .filter(|line| sets.iter().all(|s| s.contains(*line)))
        .cloned()
        .collect();
    shared.truncate(5);
    shared
}

/// Top-3 trailing PascalCase words of indented `name(` members — `_extractMemberSuffixes`.
fn extract_member_suffixes(contents: &[(std::path::PathBuf, String)]) -> Vec<String> {
    let mut suffixes: Vec<String> = Vec::new();
    for (_, c) in contents {
        for line in c.lines() {
            if line.is_empty() || !line.starts_with(char::is_whitespace) {
                continue;
            }
            // First identifier directly followed by `(`.
            let trimmed = line.trim_start();
            let ident: String = trimmed
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if ident.is_empty() {
                continue;
            }
            if !trimmed[ident.len()..].trim_start().starts_with('(') {
                continue;
            }
            // Only a mixed-case identifier (camelCase / PascalCase) carries a
            // meaningful member *suffix*. An all-lowercase snake_case name like
            // `non_blank` is a plain function call, not a typed member, so its
            // trailing word ("blank") is noise. `split_pascal_case` now also
            // splits on `_`, so without this case guard snake_case calls would
            // leak their last word here. Compare to the lower-cased form: equal
            // ⇒ no upper-case anywhere ⇒ skip.
            if ident == ident.to_ascii_lowercase() {
                continue;
            }
            let words = split_pascal_case(&ident);
            if words.len() < 2 {
                continue;
            }
            if let Some(last) = words.last() {
                suffixes.push(last.clone());
            }
        }
    }
    top_n(&suffixes, 3)
}

/// Top-N most frequent items, ordered by count desc — a port of `_topN`.
fn top_n(items: &[String], n: usize) -> Vec<String> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for item in items {
        if let Some(e) = counts.iter_mut().find(|(v, _)| v == item) {
            e.1 += 1;
        } else {
            counts.push((item.clone(), 1));
        }
    }
    counts.sort_by_key(|b| std::cmp::Reverse(b.1));
    counts.into_iter().take(n).map(|(v, _)| v).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn split_pascal_case_words() {
        assert_eq!(split_pascal_case("QueryResolver"), vec!["Query", "Resolver"]);
        assert_eq!(
            split_pascal_case("ApikeyQueryResolver"),
            vec!["Apikey", "Query", "Resolver"]
        );
        // snake_case / kebab-case split on separators (which are dropped), so
        // non-Pascal naming (Rust/Python/Ruby) is no longer collapsed to one
        // word and the suffix pass can finally see it.
        assert_eq!(split_pascal_case("user_repository"), vec!["user", "repository"]);
        assert_eq!(
            split_pascal_case("graph_wirelink_gate"),
            vec!["graph", "wirelink", "gate"]
        );
        assert_eq!(split_pascal_case("api-client"), vec!["api", "client"]);
        // A single word stays a single word (correctly yields no suffix).
        assert_eq!(split_pascal_case("workspace"), vec!["workspace"]);
    }

    #[test]
    fn extract_function_prefix_boundaries() {
        assert_eq!(extract_function_prefix("useFooBar"), Some("use".to_string()));
        assert_eq!(
            extract_function_prefix("user_repository"),
            Some("user".to_string())
        );
        assert_eq!(extract_function_prefix("foobar"), None);
    }

    #[test]
    fn global_suffix_cluster_detected() {
        let dir = tempdir().unwrap();
        let svc = dir.path().join("src");
        std::fs::create_dir_all(&svc).unwrap();
        for n in [
            "UserService",
            "OrderService",
            "AuthService",
            "MailService",
            "BillingService",
        ] {
            std::fs::write(svc.join(format!("{n}.ts")), "export class X {}").unwrap();
        }
        // The cache is keyed by the temp dir's file-set hash, so the default
        // (enabled) cache simply writes a fresh entry — no env mutation needed.
        let clusters = discover_clusters(dir.path(), "typescript", Some("app"));
        assert!(clusters.iter().any(|c| {
            c.get("suffix").and_then(Value::as_str) == Some("Service")
                && c.get("subprojectName").and_then(Value::as_str) == Some("app")
        }));
    }

    #[test]
    fn folder_frequency_counts_segments() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/api")).unwrap();
        std::fs::create_dir_all(dir.path().join("src/lib")).unwrap();
        std::fs::write(dir.path().join("src/api/a.ts"), "").unwrap();
        std::fs::write(dir.path().join("src/lib/b.ts"), "").unwrap();
        let freq = compute_folder_frequency(dir.path(), "typescript");
        assert_eq!(freq["segments"]["src"], 2);
        assert_eq!(freq["totalFolders"], 2);
    }

    /// F0-e: an unknown stack (no manifest, exotic `.foo` extension) must still
    /// discover suffix clusters using the dominant derived extension instead of
    /// early-returning empty.
    #[test]
    fn unknown_stack_discovers_clusters_via_dominant_ext() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        for n in [
            "FooService",
            "BarService",
            "BazService",
            "QuxService",
            "ZapService",
        ] {
            std::fs::write(src.join(format!("{n}.foo")), "fn x() {}").unwrap();
        }
        let clusters = discover_clusters(dir.path(), "unknown", Some("app"));
        assert!(
            clusters.iter().any(|c| {
                c.get("suffix").and_then(Value::as_str) == Some("Service")
                    && c.get("ext").and_then(Value::as_str) == Some(".foo")
            }),
            "expected a Service suffix-cluster on .foo, got {clusters:?}"
        );
    }

    #[test]
    fn filename_cluster_requires_shared_folder_segment() {
        // Drive the pass directly with RELATIVE paths and an empty base: the
        // pass reads paths only (no disk I/O), and `relative_path` returns the
        // input verbatim against an empty base, so this targets the segment rule
        // without cache / primary-ext / platform path normalisation in the way.
        // Homonymy: same basename across DIFFERENT layers that merely share the
        // `src` ancestor at different depths — the real `core` case that must
        // NOT cluster (folders are not siblings).
        let homonym_files = vec![
            "src/domain/model/view/workspace.rs".to_string(),
            "src/io/workspace.rs".to_string(),
            "src/view/projection/workspace.rs".to_string(),
        ];
        let homonym = discover_filename_clusters(Path::new(""), &homonym_files, &[]);
        assert!(
            !homonym
                .iter()
                .any(|c| c.get("label").and_then(Value::as_str) == Some("workspace")),
            "homonymous basename across non-sibling layers must NOT cluster, got {homonym:?}"
        );

        // Real pattern: same basename in SIBLING folders (shared immediate
        // parent `features`). (`repository` is a domain convention, not a
        // framework structural basename like `route`/`page`, so it survives.)
        let pattern_files = vec![
            "features/auth/repository.ts".to_string(),
            "features/billing/repository.ts".to_string(),
            "features/users/repository.ts".to_string(),
        ];
        let pattern = discover_filename_clusters(Path::new(""), &pattern_files, &[]);
        assert!(
            pattern.iter().any(|c| {
                c.get("kind").and_then(Value::as_str) == Some("filename-cluster")
                    && c.get("label").and_then(Value::as_str) == Some("repository")
                    && c.get("folderPattern").and_then(Value::as_str)
                        == Some("**/features/*/repository.ts")
            }),
            "sibling folders must yield a filename-cluster, got {pattern:?}"
        );
    }
}
