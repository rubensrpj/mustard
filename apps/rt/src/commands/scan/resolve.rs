//! Concept-node graph resolver (Wave 4 — project-profiler).
//!
//! Unifies the legacy context loaders (`skill-match`, `refs`, `context-slice`)
//! behind a single deterministic BFS over the concept-node graph produced by
//! Wave 3 ([`super::graph::build_index`]).
//!
//! Given a [`ResolveScope`] — `{entities, operation, layer, role}` — the
//! resolver:
//!
//! 1. Builds the in-memory [`super::graph::GraphIndex`] from
//!    `<project>/.claude/graph/`.
//! 2. Translates the scope into seed ids using the graph's `id → path` table:
//!    each entity becomes a `*.entity.<slug>` seed; an explicit `[[id]]` token
//!    in the scope is accepted verbatim.
//! 3. Runs a deterministic breadth-first walk over the outbound `[[id]]` edges
//!    from every seed, dedup'ing by id and keeping the **minimum** distance.
//!    Distance-0 = the seeds themselves.
//! 4. Dereferences every reached id to its on-disk markdown body, strips raw
//!    `[[id]]` brackets from the body so the consuming agent never sees the
//!    wikilink wire format.
//! 5. Truncates the closure by the role's prompt budget (read from
//!    [`crate::hooks::budget::role_prompt_budget`]). The farthest-distance
//!    nodes are dropped first; ties break on lexicographic id.
//!
//! ## Output schema (byte-stable)
//!
//! ```json
//! {
//!   "scope_hash": "<sha256-hex>",
//!   "role": "<lower-case label or null>",
//!   "budget_chars": <usize or null>,
//!   "total_chars": <usize>,
//!   "truncated": <bool>,
//!   "dropped": ["<id>", ...],
//!   "warnings": ["<line>", ...],
//!   "closure": [
//!     { "id": "<id>", "path": "<rel>", "distance": <usize>, "content": "<resolved body>" },
//!     ...
//!   ]
//! }
//! ```
//!
//! The four legacy loaders call into [`resolve_closure`] (library face) and
//! re-format the result back into their own byte-stable JSON shape — that way
//! the public-facing JSON of each existing subcommand stays unchanged while
//! all four share one walk + one budget cut.
//!
//! ## Cache
//!
//! Successful resolutions are cached at `.claude/.resolve-cache.json`, keyed
//! by the SHA-256 of the canonical scope JSON. A second invocation with the
//! same scope returns the cached blob without walking the graph again.
//! `MUSTARD_RESOLVE_CACHE=off` bypasses both read + write (used by tests).

use crate::shared::context::project_dir;
use crate::commands::scan::graph::{self, GraphIndex};
use crate::util::now_iso8601;
use crate::util::sha256::Sha256;
use mustard_core::fs as mfs;
use mustard_core::metrics::{MetricLine, emit_metric};
use mustard_core::ClaudePaths;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

/// Cache toggle env var — `off` bypasses both read and write paths.
const CACHE_TOGGLE_ENV: &str = "MUSTARD_RESOLVE_CACHE";

/// On-disk cache schema version. Bump when the output shape changes.
const RESOLVE_CACHE_VERSION: u64 = 1;

/// The scope a caller wants resolved into a minimal context closure.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResolveScope {
    /// Entity slugs the work touches (e.g. `["user", "order"]`). Each becomes
    /// a `*.entity.<slug>` seed when a matching id is in the graph.
    #[serde(default)]
    pub entities: Vec<String>,
    /// Operation slug (e.g. `"create"`, `"update"`); reserved for future
    /// seeding strategies — currently no-op.
    #[serde(default)]
    pub operation: Option<String>,
    /// Logical layer (e.g. `"backend"`, `"ui"`) — used as a soft tag in the
    /// scope hash; not seeded directly today.
    #[serde(default)]
    pub layer: Option<String>,
    /// Role label (e.g. `"explore"`, `"general-purpose"`, `"plan"`). Drives
    /// the budget cut via [`crate::hooks::budget::role_prompt_budget`].
    #[serde(default)]
    pub role: Option<String>,
    /// Explicit seed ids — bypass the entity/operation translation and
    /// inject these nodes verbatim. Accepts bare ids (`foo.entity.bar`) or
    /// `[[id]]` wikilinks.
    #[serde(default)]
    pub seeds: Vec<String>,
}

/// One node in the resolved closure.
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedNode {
    pub id: String,
    pub path: String,
    pub distance: usize,
    pub content: String,
}

/// The full resolver output, byte-stable when serialised pretty.
#[derive(Debug, Clone, Serialize)]
pub struct ResolveOutput {
    pub scope_hash: String,
    pub role: Option<String>,
    pub budget_chars: Option<usize>,
    pub total_chars: usize,
    pub truncated: bool,
    pub dropped: Vec<String>,
    pub warnings: Vec<String>,
    pub closure: Vec<ResolvedNode>,
}

/// The library face: resolve a scope into its minimum closure under the
/// role budget. Pure function over the on-disk vault — never touches the
/// network, never opens SQLite.
#[must_use]
pub fn resolve_closure(project_root: &Path, scope: &ResolveScope) -> ResolveOutput {
    let scope_hash = compute_scope_hash(scope);

    // Fast-path: the on-disk cache. A `MUSTARD_RESOLVE_CACHE=off` env bypass
    // is honoured to keep the test fixtures deterministic.
    if !cache_disabled() {
        if let Some(cached) = read_cache(project_root, &scope_hash) {
            return cached;
        }
    }

    let index = graph::build_index(project_root);
    let seeds = resolve_seeds(&index, scope);

    let mut warnings: Vec<String> = Vec::new();
    if seeds.is_empty() {
        warnings.push(format!(
            "warning: scope produced no seeds — entities={:?} operation={:?} layer={:?}",
            scope.entities, scope.operation, scope.layer
        ));
    }

    let walked = bfs_walk(&index, &seeds);
    let mut nodes = materialise_nodes(project_root, &index, &walked, &mut warnings);

    // Sort by (distance asc, id asc) so byte-stable output is deterministic
    // and the truncation step drops genuinely-farthest nodes first.
    nodes.sort_by(|a, b| a.distance.cmp(&b.distance).then_with(|| a.id.cmp(&b.id)));

    let budget = scope
        .role
        .as_deref()
        .and_then(crate::hooks::budget::role_prompt_budget);

    let (kept, dropped, total_chars, truncated) = apply_budget(nodes, budget);

    let output = ResolveOutput {
        scope_hash: scope_hash.clone(),
        role: scope.role.clone(),
        budget_chars: budget,
        total_chars,
        truncated,
        dropped,
        warnings,
        closure: kept,
    };

    // Wave-5 telemetry: emit the closure size on the same metric surface the
    // budget hook uses (`.claude/.metrics/resolve-closure.jsonl`). This gives
    // the A/B comparison ("tokens injected per agent, before vs after the
    // resolver") visible to `metrics report` without touching the budget table.
    // Fail-silent — a metric write never affects the resolver's contract.
    emit_resolve_prompt_metric(project_root, &output);

    if !cache_disabled() {
        write_cache(project_root, &scope_hash, &output);
    }
    output
}

/// Emit the per-resolve closure size as one `resolve-closure` metric line.
///
/// Lives next to the `budget-check` / `output-budget` shards so a future
/// `metrics report` pass can correlate "what the budget gate measured" with
/// "what the resolver actually injected" on a single timeline. The role
/// label is forwarded verbatim from the scope so cross-shard joins are
/// trivial.
fn emit_resolve_prompt_metric(project_root: &Path, output: &ResolveOutput) {
    // Token estimate uses the same 4-chars-per-token heuristic the budget
    // hook applies, so the two surfaces are directly comparable.
    #[allow(clippy::cast_possible_wrap)]
    let tokens_affected = (output.total_chars / 4) as i64;
    let note = if output.truncated { "truncated" } else { "passed" };
    let role_label = output.role.clone().unwrap_or_else(|| "unknown".to_string());
    let line = MetricLine::new(now_iso8601(), "resolve-closure")
        .tokens_affected(tokens_affected)
        .tokens_saved(0)
        .note(note)
        .extras(json!({
            "role": role_label,
            "scope_hash": output.scope_hash,
            "node_count": output.closure.len(),
            "total_chars": output.total_chars,
            "budget_chars": output.budget_chars,
            "truncated": output.truncated,
            "dropped_count": output.dropped.len(),
            "category": "context-injection",
        }));
    let _ = emit_metric(project_root, &line);
}

/// CLI entry point for `mustard-rt run context-resolve`. Reads the scope JSON
/// from `--scope '<json>'` (or `--scope-file <path>`) and prints the
/// byte-stable JSON envelope to stdout. Fail-open: bad input emits an empty
/// envelope and exits `0`.
pub fn run(scope_arg: Option<&str>, scope_file: Option<&Path>) {
    let project = PathBuf::from(project_dir());
    let raw = match (scope_arg, scope_file) {
        (Some(s), _) if !s.is_empty() => s.to_string(),
        (_, Some(p)) => mfs::read_to_string(p).unwrap_or_default(),
        _ => String::new(),
    };
    let scope: ResolveScope = if raw.trim().is_empty() {
        ResolveScope::default()
    } else {
        serde_json::from_str(&raw).unwrap_or_default()
    };
    let output = resolve_closure(&project, &scope);
    let pretty = serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string());
    println!("{pretty}");
}

/// Translate a scope's entity / operation / explicit-seed fields into a set
/// of seed ids that exist in the graph. Unknown seeds are silently dropped —
/// the caller surfaces an "empty seeds" warning when *all* of them miss.
fn resolve_seeds(index: &GraphIndex, scope: &ResolveScope) -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();

    // Explicit seeds win — accept bare ids or `[[id]]` wikilinks.
    for raw in &scope.seeds {
        let id = strip_wikilink(raw);
        if index.nodes.contains_key(&id) {
            out.insert(id);
        }
    }

    // Entities → `*.entity.<slug>` (any subproject).
    for entity in &scope.entities {
        let slug = slugify(entity);
        for id in index.nodes.keys() {
            if let Some(suffix) = id.split('.').nth(1) {
                if suffix == "entity" && id.ends_with(&format!(".entity.{slug}")) {
                    out.insert(id.clone());
                }
            }
        }
    }

    // `scope.operation` is reserved for future seeding strategies — currently
    // no-op. The recipe concept it used to seed has been removed.
    let _ = scope.operation;

    out.into_iter().collect()
}

/// Deterministic BFS from every seed over outbound `[[id]]` edges. Returns a
/// map of `id → min-distance`. Seeds are at distance 0. Unknown edge targets
/// are skipped (the graph index already records them as orphan warnings).
fn bfs_walk(index: &GraphIndex, seeds: &[String]) -> BTreeMap<String, usize> {
    let mut distance: BTreeMap<String, usize> = BTreeMap::new();
    let mut queue: VecDeque<(String, usize)> = VecDeque::new();
    for seed in seeds {
        if distance.insert(seed.clone(), 0).is_none() {
            queue.push_back((seed.clone(), 0));
        }
    }
    while let Some((node, dist)) = queue.pop_front() {
        let Some(neighbours) = index.edges.get(&node) else {
            continue;
        };
        for next in neighbours {
            if !index.nodes.contains_key(next) {
                // Orphan edge — surface via `graph::build_index` warnings, skip here.
                continue;
            }
            let next_dist = dist + 1;
            let already_shorter = distance.get(next).is_some_and(|prev| *prev <= next_dist);
            if !already_shorter {
                distance.insert(next.clone(), next_dist);
                queue.push_back((next.clone(), next_dist));
            }
        }
    }
    distance
}

/// Resolve each walked id to its on-disk content, stripping raw `[[id]]`
/// tokens. Missing files turn into a single warning + a zero-content node so
/// the caller still sees the id in the closure.
fn materialise_nodes(
    project_root: &Path,
    index: &GraphIndex,
    walked: &BTreeMap<String, usize>,
    warnings: &mut Vec<String>,
) -> Vec<ResolvedNode> {
    let Ok(paths) = ClaudePaths::for_project(project_root) else {
        warnings.push("warning: invalid project root for graph resolution".to_string());
        return Vec::new();
    };
    let graph_root = paths.graph_dir();
    let mut out: Vec<ResolvedNode> = Vec::with_capacity(walked.len());
    for (id, distance) in walked {
        let Some(rel) = index.nodes.get(id) else {
            warnings.push(format!("warning: walked id {id} missing from id→path table"));
            continue;
        };
        let abs = graph_root.join(rel);
        let body = match mfs::read_to_string(&abs) {
            Ok(s) => s,
            Err(_) => {
                warnings.push(format!(
                    "warning: failed to read body for {id} at {}",
                    rel.as_str()
                ));
                String::new()
            }
        };
        out.push(ResolvedNode {
            id: id.clone(),
            path: rel.clone(),
            distance: *distance,
            content: dereference_wikilinks(&body),
        });
    }
    out
}

/// Apply the role budget: keep the nearest-first nodes whose cumulative
/// `content.len()` stays under `budget`. Returns `(kept, dropped_ids,
/// total_chars, truncated)`.
fn apply_budget(
    nodes: Vec<ResolvedNode>,
    budget: Option<usize>,
) -> (Vec<ResolvedNode>, Vec<String>, usize, bool) {
    let total_chars: usize = nodes.iter().map(|n| n.content.len()).sum();
    let Some(limit) = budget else {
        return (nodes, Vec::new(), total_chars, false);
    };
    if total_chars <= limit {
        return (nodes, Vec::new(), total_chars, false);
    }

    let mut kept: Vec<ResolvedNode> = Vec::new();
    let mut dropped: Vec<String> = Vec::new();
    let mut running = 0usize;
    for node in nodes {
        let next = running.saturating_add(node.content.len());
        if next <= limit {
            running = next;
            kept.push(node);
        } else {
            dropped.push(node.id);
        }
    }
    // Stable ordering for the dropped tail so the JSON envelope is
    // byte-stable regardless of insertion order.
    dropped.sort();
    (kept, dropped, total_chars, true)
}

/// Strip the wikilink brackets from a `[[id]]` token, leaving bare `id`.
/// Accepts already-bare ids unchanged.
fn strip_wikilink(raw: &str) -> String {
    let trimmed = raw.trim();
    let s = trimmed.strip_prefix("[[").unwrap_or(trimmed);
    s.strip_suffix("]]").unwrap_or(s).to_string()
}

/// Replace every `[[id]]` occurrence in `body` with the bare id. The
/// resolver never hands the agent raw wikilink wire format — the body it
/// receives is already-resolved prose.
#[must_use]
pub fn dereference_wikilinks(body: &str) -> String {
    let edges = graph::extract_edges(body);
    if edges.is_empty() {
        return body.to_string();
    }
    let mut out = String::with_capacity(body.len());
    let bytes = body.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;
    while i < len {
        if bytes[i] == b'[' && i + 1 < len && bytes[i + 1] == b'[' {
            let start = i + 2;
            let mut j = start;
            while j < len {
                let c = bytes[j];
                if c.is_ascii_alphanumeric() || c == b'_' || c == b'-' || c == b'.' {
                    j += 1;
                    continue;
                }
                break;
            }
            if j > start && j + 1 < len && bytes[j] == b']' && bytes[j + 1] == b']' {
                // Substitute `[[id]]` → `id` (bare).
                if let Ok(name) = std::str::from_utf8(&bytes[start..j]) {
                    out.push_str(name);
                }
                i = j + 2;
                continue;
            }
        }
        // Pass-through every byte we did not consume above.
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Canonical scope JSON → SHA-256 hex. The canonicalisation sorts arrays so
/// `{entities:["a","b"]}` and `{entities:["b","a"]}` hash identically.
fn compute_scope_hash(scope: &ResolveScope) -> String {
    let mut entities = scope.entities.clone();
    entities.sort();
    let mut seeds: Vec<String> = scope.seeds.iter().map(|s| strip_wikilink(s)).collect();
    seeds.sort();
    let canonical = json!({
        "entities": entities,
        "operation": scope.operation.clone().unwrap_or_default(),
        "layer": scope.layer.clone().unwrap_or_default(),
        "role": scope.role.clone().unwrap_or_default(),
        "seeds": seeds,
        "v": RESOLVE_CACHE_VERSION,
    });
    let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
    let mut h = Sha256::new();
    h.update(&bytes);
    h.hex_digest()
}

/// Same slugifier the interpreter uses for ids — keeps seed translation in
/// lock-step with the id convention.
fn slugify(input: &str) -> String {
    super::interpret::slugify(input)
}

/// `true` when the env toggle disables cache reads + writes.
fn cache_disabled() -> bool {
    std::env::var(CACHE_TOGGLE_ENV).is_ok_and(|v| v.eq_ignore_ascii_case("off"))
}

/// Cache file lives next to the interpret cache, namespaced by hash.
fn cache_path(project_root: &Path) -> PathBuf {
    ClaudePaths::for_project(project_root)
        .map(|p| p.resolve_cache_path())
        .unwrap_or_default()
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheEnvelope {
    v: u64,
    entries: BTreeMap<String, Value>,
}

fn read_cache(project_root: &Path, key: &str) -> Option<ResolveOutput> {
    let body = mfs::read_to_string(cache_path(project_root)).ok()?;
    let env: CacheEnvelope = serde_json::from_str(&body).ok()?;
    if env.v != RESOLVE_CACHE_VERSION {
        return None;
    }
    let raw = env.entries.get(key)?;
    serde_json::from_value::<ResolveOutput>(raw.clone()).ok()
}

/// Collect every concept-node id present in any cached resolver closure.
///
/// Used by the post-EXECUTE write-back step: when a spec leaves EXECUTE, the
/// union of every closure the resolver produced during the session is written
/// back as `injected` backlinks. A missing or stale cache degrades to an
/// empty vector — the write-back then becomes a no-op.
#[must_use]
pub fn collect_cached_closure_ids(project_root: &Path) -> Vec<String> {
    let Some(body) = mfs::read_to_string(cache_path(project_root)).ok() else {
        return Vec::new();
    };
    let Some(env) = serde_json::from_str::<CacheEnvelope>(&body).ok() else {
        return Vec::new();
    };
    if env.v != RESOLVE_CACHE_VERSION {
        return Vec::new();
    }
    let mut ids: BTreeSet<String> = BTreeSet::new();
    for raw in env.entries.values() {
        let Some(output) = serde_json::from_value::<ResolveOutput>(raw.clone()).ok() else {
            continue;
        };
        for node in output.closure {
            ids.insert(node.id);
        }
    }
    ids.into_iter().collect()
}

fn write_cache(project_root: &Path, key: &str, output: &ResolveOutput) {
    let path = cache_path(project_root);
    let mut env: CacheEnvelope = mfs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .filter(|c: &CacheEnvelope| c.v == RESOLVE_CACHE_VERSION)
        .unwrap_or(CacheEnvelope {
            v: RESOLVE_CACHE_VERSION,
            entries: BTreeMap::new(),
        });
    let value = serde_json::to_value(output).unwrap_or(Value::Null);
    env.entries.insert(key.to_string(), value);
    let pretty = serde_json::to_string_pretty(&env).unwrap_or_default();
    let _ = mfs::write_atomic(&path, pretty.as_bytes());
}

// Manual Deserialize for ResolvedNode so the cache round-trip works (Serialize
// is enough for stdout, but the cache reads `ResolveOutput` back).
impl<'de> serde::Deserialize<'de> for ResolvedNode {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            id: String,
            path: String,
            distance: usize,
            content: String,
        }
        let raw = Raw::deserialize(d)?;
        Ok(ResolvedNode {
            id: raw.id,
            path: raw.path,
            distance: raw.distance,
            content: raw.content,
        })
    }
}

impl<'de> serde::Deserialize<'de> for ResolveOutput {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            scope_hash: String,
            role: Option<String>,
            budget_chars: Option<usize>,
            total_chars: usize,
            truncated: bool,
            dropped: Vec<String>,
            warnings: Vec<String>,
            closure: Vec<ResolvedNode>,
        }
        let raw = Raw::deserialize(d)?;
        Ok(ResolveOutput {
            scope_hash: raw.scope_hash,
            role: raw.role,
            budget_chars: raw.budget_chars,
            total_chars: raw.total_chars,
            truncated: raw.truncated,
            dropped: raw.dropped,
            warnings: raw.warnings,
            closure: raw.closure,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, body).unwrap();
    }

    /// Helper: stamp a small graph with `entity.user`, `conv.repo-pattern`,
    /// and `conv.shared` connected so BFS has work to do.
    fn build_fixture(root: &Path) {
        let graph_dir = root.join(".claude").join("graph");
        write(
            &graph_dir.join("rt.entity.user.md"),
            "---\nid: rt.entity.user\nkind: entity\n---\n# User\nrequires [[rt.conv.repo-pattern]] and [[rt.conv.shared]]\n",
        );
        write(
            &graph_dir.join("rt.conv.repo-pattern.md"),
            "---\nid: rt.conv.repo-pattern\nkind: conv\n---\n# RepoPattern\nrelies on [[rt.conv.shared]]\n",
        );
        write(
            &graph_dir.join("rt.conv.shared.md"),
            "---\nid: rt.conv.shared\nkind: conv\n---\n# Shared\nleaf node.\n",
        );
        // Unrelated island the resolver must NOT pull in.
        write(
            &graph_dir.join("rt.conv.unrelated.md"),
            "---\nid: rt.conv.unrelated\nkind: conv\n---\n# Unrelated\n",
        );
    }

    /// AC-1: the resolver returns a closure strictly smaller than the full
    /// node set, and convention nodes shared by two seeds appear exactly
    /// once.
    #[test]
    fn resolve_closure_is_minimal() {
        // SAFETY: edition 2024 forbids the unsafe std::env::set_var. The
        // env toggle is only used to switch the cache off — we instead point
        // the resolver at a fresh project root each test, which has no
        // cache file, so the bypass is unnecessary.
        let dir = tempdir().unwrap();
        let root = dir.path();
        build_fixture(root);

        let scope = ResolveScope {
            entities: vec!["user".to_string()],
            ..ResolveScope::default()
        };
        let out = resolve_closure(root, &scope);

        // 4 nodes total in the fixture (entity + 3 conventions), closure must
        // be a strict subset.
        let total_in_graph = 4;
        assert!(
            out.closure.len() < total_in_graph,
            "closure {} must be smaller than total {total_in_graph}",
            out.closure.len()
        );

        // The closure must contain user + repo-pattern + shared, but NOT the
        // unrelated island.
        let ids: BTreeSet<&str> = out.closure.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains("rt.entity.user"));
        assert!(ids.contains("rt.conv.repo-pattern"));
        assert!(ids.contains("rt.conv.shared"));
        assert!(!ids.contains("rt.conv.unrelated"));

        // `rt.conv.shared` is reachable both via direct entity edge and via
        // repo-pattern — dedup must keep it exactly once at distance 1
        // (the shorter of the two paths).
        let shared = out
            .closure
            .iter()
            .find(|n| n.id == "rt.conv.shared")
            .expect("shared in closure");
        assert_eq!(shared.distance, 1);
        let shared_count = out
            .closure
            .iter()
            .filter(|n| n.id == "rt.conv.shared")
            .count();
        assert_eq!(shared_count, 1, "shared convention must appear exactly once");
    }

    /// AC-2: when the role budget would be exceeded, the farthest-distance
    /// nodes are dropped first and `truncated=true`.
    #[test]
    fn resolve_respects_budget() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let graph_dir = root.join(".claude").join("graph");
        // 3 nodes: seed (small), middle (small), far (large).
        write(
            &graph_dir.join("rt.entity.seed.md"),
            "---\nid: rt.entity.seed\nkind: entity\n---\nA [[rt.conv.middle]]",
        );
        write(
            &graph_dir.join("rt.conv.middle.md"),
            "---\nid: rt.conv.middle\nkind: conv\n---\nB [[rt.conv.far]]",
        );
        // Far node body is large enough that any non-trivial budget keeps
        // closer nodes but drops this one.
        let far_body = format!(
            "---\nid: rt.conv.far\nkind: conv\n---\n{}",
            "x".repeat(20_000)
        );
        write(&graph_dir.join("rt.conv.far.md"), &far_body);

        let scope = ResolveScope {
            seeds: vec!["rt.entity.seed".to_string()],
            // The "explore" role budget is 10_000 chars — the far node alone
            // is 20K+, so it must be dropped, while seed + middle stay.
            role: Some("explore".to_string()),
            ..ResolveScope::default()
        };
        let out = resolve_closure(root, &scope);

        assert!(out.truncated, "explore budget must trigger truncation");
        assert_eq!(out.budget_chars, Some(10_000));
        let kept_ids: BTreeSet<&str> = out.closure.iter().map(|n| n.id.as_str()).collect();
        assert!(kept_ids.contains("rt.entity.seed"));
        assert!(kept_ids.contains("rt.conv.middle"));
        assert!(!kept_ids.contains("rt.conv.far"), "far node must be dropped");
        assert!(out.dropped.iter().any(|id| id == "rt.conv.far"));
    }

    /// AC-3: the resolver dereferences every `[[id]]` token in the body —
    /// the agent never sees the raw wikilink wire format.
    #[test]
    fn resolve_dereferences_ids() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let graph_dir = root.join(".claude").join("graph");
        write(
            &graph_dir.join("rt.entity.user.md"),
            "---\nid: rt.entity.user\nkind: entity\n---\nSee [[rt.conv.shared]] for details.\n",
        );
        write(
            &graph_dir.join("rt.conv.shared.md"),
            "---\nid: rt.conv.shared\nkind: conv\n---\nA shared convention.\n",
        );

        let scope = ResolveScope {
            seeds: vec!["rt.entity.user".to_string()],
            ..ResolveScope::default()
        };
        let out = resolve_closure(root, &scope);

        for node in &out.closure {
            assert!(
                !node.content.contains("[["),
                "raw [[ must not appear in resolved body: {} → {:?}",
                node.id,
                node.content
            );
            assert!(
                !node.content.contains("]]"),
                "raw ]] must not appear in resolved body: {} → {:?}",
                node.id,
                node.content
            );
        }
        // The bare id replaces the wikilink in the user body.
        let user = out
            .closure
            .iter()
            .find(|n| n.id == "rt.entity.user")
            .expect("user in closure");
        assert!(user.content.contains("rt.conv.shared"));
    }

    #[test]
    fn dereference_wikilinks_replaces_brackets_with_bare_ids() {
        let raw = "alpha [[foo.entity.bar]] beta [[baz.conv.qux]] gamma";
        let out = dereference_wikilinks(raw);
        assert_eq!(out, "alpha foo.entity.bar beta baz.conv.qux gamma");
    }

    #[test]
    fn scope_hash_is_order_independent() {
        let a = ResolveScope {
            entities: vec!["b".to_string(), "a".to_string()],
            seeds: vec!["x.entity.q".to_string()],
            ..ResolveScope::default()
        };
        let b = ResolveScope {
            entities: vec!["a".to_string(), "b".to_string()],
            seeds: vec!["[[x.entity.q]]".to_string()],
            ..ResolveScope::default()
        };
        assert_eq!(compute_scope_hash(&a), compute_scope_hash(&b));
    }

    /// AC-4: a successful resolve emits a `resolve-closure` metric line under
    /// `.claude/.metrics/`, carrying the closure's node count + total chars
    /// so the A/B "tokens injected per agent, before vs after" comparison
    /// is visible via the same surface the budget hook uses.
    #[test]
    fn resolve_emits_prompt_metric() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        build_fixture(root);

        let scope = ResolveScope {
            entities: vec!["user".to_string()],
            role: Some("explore".to_string()),
            ..ResolveScope::default()
        };
        let out = resolve_closure(root, &scope);
        assert!(!out.closure.is_empty(), "fixture produces a non-empty closure");

        let shard = root
            .join(".claude")
            .join(".metrics")
            .join("resolve-closure.jsonl");
        assert!(shard.exists(), "metric shard must be written");
        let contents = std::fs::read_to_string(&shard).unwrap();
        let line = contents
            .lines()
            .next()
            .expect("at least one metric line");
        let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(parsed["event"], serde_json::json!("resolve-closure"));
        assert_eq!(parsed["role"], serde_json::json!("explore"));
        assert_eq!(parsed["node_count"], serde_json::json!(out.closure.len()));
        assert_eq!(parsed["total_chars"], serde_json::json!(out.total_chars));
        assert_eq!(parsed["category"], serde_json::json!("context-injection"));
        assert_eq!(
            parsed["scope_hash"],
            serde_json::json!(out.scope_hash),
            "scope_hash is the join key with the budget shard"
        );
    }

    #[test]
    fn bfs_walk_keeps_shortest_distance() {
        // a → b → c   AND   a → c   (direct).
        // c must end up at distance 1 (the direct edge), not 2 (via b).
        let mut edges = BTreeMap::new();
        edges.insert("a".to_string(), vec!["b".to_string(), "c".to_string()]);
        edges.insert("b".to_string(), vec!["c".to_string()]);
        let mut nodes = BTreeMap::new();
        nodes.insert("a".to_string(), "a.md".to_string());
        nodes.insert("b".to_string(), "b.md".to_string());
        nodes.insert("c".to_string(), "c.md".to_string());
        let index = GraphIndex {
            nodes,
            edges,
            warnings: Vec::new(),
            aliased_skills: Vec::new(),
        };
        let walked = bfs_walk(&index, &["a".to_string()]);
        assert_eq!(walked.get("a"), Some(&0));
        assert_eq!(walked.get("b"), Some(&1));
        assert_eq!(walked.get("c"), Some(&1));
    }
}
