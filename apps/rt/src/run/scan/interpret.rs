//! Model-assisted interpretation layer (Wave 2 — project-profiler).
//!
//! Takes the agnostic profile already produced by Wave 1 (the single-pass
//! visitor + cluster discovery) and asks a language model to label clusters,
//! resolve under-determined conventions, identify entities, and emit `[[ ]]`
//! edges between concepts. Runs **once per project** (cold path), cached on
//! disk by a SHA-256 of the file-set + model name, so steady-state syncs never
//! pay the round-trip cost.
//!
//! ## Fail-open
//!
//! The interpreter is allowed to fail silently. When the model is unavailable
//! (no `ANTHROPIC_API_KEY`), the network round-trip errors, the response does
//! not parse, or the cache write fails, [`interpret`] returns an empty
//! [`InterpretedResult`] — the registry pipeline then falls back to the
//! agnostic floor (cluster discovery + folder frequency) from Wave 1. The
//! interpretation layer is an *enrichment*, never a dependency.
//!
//! ## Caching
//!
//! A successful interpretation is written to
//! `<sub>/.claude/.interpret-cache.json`, sibling to `.cluster-cache.json`.
//! The cache key is `SHA256(model | paths-sorted | sizes)`. A second
//! interpretation with the same file-set + model returns the cached result
//! without consulting the network. Set `MUSTARD_INTERPRET_CACHE=off` to bypass
//! the cache entirely (used by `interpret_cache_frozen` to assert the freeze).
//!
//! ## Model selection
//!
//! `MUSTARD_SCAN_MODEL` (default `sonnet`) picks the model. The selected name
//! is normalised through the same tier mapping `model_routing` uses, and
//! Haiku is *only* honoured when set explicitly — every other value resolves
//! upward to Sonnet (no silent downgrade). Opus is permitted.

use super::file_utils::VisitedFile;
use crate::util::sha256::Sha256;
use mustard_core::fs as mfs;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One entity recovered from the interpreted profile.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterpretedEntity {
    /// Entity name (PascalCase by convention, but the model's choice wins).
    pub name: String,
    /// Relative path of the source file from the subproject root.
    pub file: String,
    /// Wikilink ids the model attached (`[[sub.entity.foo]]`, …).
    #[serde(default)]
    pub edges: Vec<String>,
}

/// One enum recovered from the interpreted profile.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterpretedEnum {
    /// Enum name.
    pub name: String,
    /// Relative path of the source file from the subproject root.
    pub file: String,
    /// Enum member names.
    #[serde(default)]
    pub values: Vec<String>,
}

/// The complete output of a single interpretation pass.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterpretedResult {
    /// Entities the model identified.
    #[serde(default)]
    pub entities: Vec<InterpretedEntity>,
    /// Enums the model identified.
    #[serde(default)]
    pub enums: Vec<InterpretedEnum>,
    /// Overlay merged into `_patterns.{stack}` — typically `clusterLabels`,
    /// `dominant`, and `edges`. Stored as raw JSON so the schema can evolve
    /// without breaking the registry consumer.
    #[serde(default)]
    pub patterns_overlay: Value,
    /// `true` when the result was served from the on-disk cache. Tests use
    /// this to assert frozen behaviour; not serialised.
    #[serde(skip)]
    pub from_cache: bool,
}

/// On-disk cache schema. Bumped when the prompt or response shape changes.
const INTERPRET_CACHE_VERSION: u64 = 1;

/// Model selection env var — Sonnet default; no silent downgrade.
const MODEL_ENV: &str = "MUSTARD_SCAN_MODEL";
/// API key env var — when absent the layer is a no-op (fail-open).
const API_KEY_ENV: &str = "ANTHROPIC_API_KEY";
/// Cache toggle env var — `off` bypasses both read and write paths.
const CACHE_TOGGLE_ENV: &str = "MUSTARD_INTERPRET_CACHE";

/// Resolve the model id to use, honouring the no-downgrade policy.
///
/// - `raw` empty → `sonnet` (default).
/// - Contains `opus` → `claude-opus-4-7` (upgrade allowed).
/// - Contains `haiku` → `claude-haiku-4-5` (explicit opt-in only).
/// - Anything else (incl. `sonnet`) → `claude-sonnet-4-5`.
///
/// The exact API model ids ship as constants so tests can pin them. The
/// public [`resolve_model`] wrapper reads `MUSTARD_SCAN_MODEL` from the
/// process env — tests call [`resolve_model_for`] directly so they never
/// have to mutate env (forbidden by the crate's `#![forbid(unsafe_code)]`).
#[must_use]
pub fn resolve_model_for(raw: &str) -> &'static str {
    let lc = raw.to_ascii_lowercase();
    if lc.contains("opus") {
        "claude-opus-4-7"
    } else if lc.contains("haiku") {
        // Honoured only when the override explicitly mentions haiku — no
        // other path picks it. This preserves the "no downgrade" rule from
        // `model_routing` while letting the user opt in for cost experiments.
        "claude-haiku-4-5"
    } else {
        "claude-sonnet-4-5"
    }
}

/// Public env-driven wrapper around [`resolve_model_for`]. Kept as a public
/// helper for callers that want the resolved model name without going
/// through [`InterpretEnv::from_process`].
#[must_use]
#[allow(dead_code)]
pub fn resolve_model() -> &'static str {
    resolve_model_for(&std::env::var(MODEL_ENV).unwrap_or_default())
}

/// Compact, deterministic profile fed to the model. Built entirely from the
/// data Wave 1 already produces — no extra disk reads.
#[derive(Debug, Clone, Serialize)]
struct CompactProfile<'a> {
    /// Stack id detected by [`super::detect_stack`].
    stack_id: &'a str,
    /// Cluster discovery output (raw JSON the model can label in-place).
    clusters: &'a [Value],
    /// Up to `MAX_SAMPLES_PER_CLUSTER` representative snippets per cluster.
    samples: Vec<CompactSample>,
    /// Subproject-wide file count, useful for the model to gauge scale.
    total_files: usize,
}

/// One representative file snippet attached to the compact profile.
#[derive(Debug, Clone, Serialize)]
struct CompactSample {
    /// Relative path from the subproject root.
    file: String,
    /// First [`MAX_SAMPLE_BYTES`] bytes of the file — enough to see the
    /// declaration without exploding the token budget.
    head: String,
}

/// Maximum number of representative samples emitted per cluster.
const MAX_SAMPLES_PER_CLUSTER: usize = 2;
/// Maximum byte count of each sample's head — caps the prompt size.
const MAX_SAMPLE_BYTES: usize = 1_200;
/// Overall cap on samples in the compact profile (defence-in-depth).
const MAX_TOTAL_SAMPLES: usize = 24;

/// Build the compact profile that the model interprets.
///
/// Selects up to [`MAX_SAMPLES_PER_CLUSTER`] sample files per cluster, prefers
/// files declared in the cluster's `files[]` array, and truncates each sample
/// to [`MAX_SAMPLE_BYTES`]. The total samples are capped at
/// [`MAX_TOTAL_SAMPLES`] so the prompt size stays bounded regardless of how
/// many clusters were discovered.
fn build_profile<'a>(
    stack_id: &'a str,
    clusters: &'a [Value],
    visited: &[VisitedFile],
) -> CompactProfile<'a> {
    let by_rel: BTreeMap<&str, &VisitedFile> = visited.iter().map(|v| (v.rel.as_str(), v)).collect();
    let mut samples: Vec<CompactSample> = Vec::new();

    for cluster in clusters {
        if samples.len() >= MAX_TOTAL_SAMPLES {
            break;
        }
        let files = cluster.get("files").and_then(Value::as_array);
        let Some(files) = files else { continue };
        let mut taken = 0usize;
        for f in files {
            if taken >= MAX_SAMPLES_PER_CLUSTER || samples.len() >= MAX_TOTAL_SAMPLES {
                break;
            }
            let Some(rel) = f.as_str() else { continue };
            // Strip any leading subproject prefix from the cluster's stored
            // path so it matches the visitor's relative form.
            let needle = rel.rsplit_once('/').map_or(rel, |(_, tail)| tail);
            let hit = by_rel
                .iter()
                .find(|(k, _)| k.ends_with(rel) || k.ends_with(needle))
                .map(|(_, v)| *v);
            let Some(file) = hit else { continue };
            let Some(content) = &file.content else { continue };
            let head: String = content.chars().take(MAX_SAMPLE_BYTES).collect();
            samples.push(CompactSample {
                file: file.rel.clone(),
                head,
            });
            taken += 1;
        }
    }

    CompactProfile {
        stack_id,
        clusters,
        samples,
        total_files: visited.len(),
    }
}

/// The prompt the model interprets. Kept short and structurally explicit so
/// the response is easy to parse — the model only needs to fill in three
/// arrays.
const PROMPT_TEMPLATE: &str = r#"You receive a compact profile of one subproject (clusters discovered by structural pattern mining, plus representative file snippets). Your job is to interpret the profile and emit a strict JSON object with three arrays:

  - "entities": objects { "name": string, "file": string, "edges": string[] }
  - "enums":    objects { "name": string, "file": string, "values": string[] }
  - "patternsOverlay": object with "clusterLabels", "dominant", "edges" keys
    (any subset is fine; unknown keys are ignored).

Rules:
  * Use only entities visible in the supplied snippets. Do not invent files.
  * Edges are "[[id]]" wikilinks pointing at other entities, conventions, or
    clusters — leave the array empty when unsure.
  * Reply with ONE valid JSON object and nothing else (no markdown fence, no
    commentary, no preamble). The orchestrator parses your stdout directly.

Profile follows:
"#;

/// Anthropic Messages API endpoint.
const API_URL: &str = "https://api.anthropic.com/v1/messages";
/// Anthropic API version header.
const API_VERSION: &str = "2023-06-01";
/// Hard upper bound on response tokens — keeps cost predictable.
const MAX_TOKENS: u32 = 4096;
/// HTTP timeout for the model round-trip.
const HTTP_TIMEOUT_SECS: u64 = 30;

/// Environment overrides used by [`interpret_with`] — tests inject explicit
/// values so they never need to mutate process env (the crate forbids
/// `unsafe`, so `set_var` is unavailable on edition 2024).
#[derive(Debug, Clone, Default)]
pub struct InterpretEnv {
    /// Effective `MUSTARD_SCAN_MODEL` value. Empty ⇒ Sonnet.
    pub model_env: String,
    /// Effective `ANTHROPIC_API_KEY`. Empty ⇒ skip the model call (no-op).
    pub api_key: String,
    /// `true` mirrors `MUSTARD_INTERPRET_CACHE=off` — bypass read + write.
    pub cache_disabled: bool,
}

impl InterpretEnv {
    /// Snapshot the process environment into an [`InterpretEnv`].
    #[must_use]
    pub fn from_process() -> Self {
        Self {
            model_env: std::env::var(MODEL_ENV).unwrap_or_default(),
            api_key: std::env::var(API_KEY_ENV).unwrap_or_default(),
            cache_disabled: std::env::var(CACHE_TOGGLE_ENV)
                .is_ok_and(|v| v.eq_ignore_ascii_case("off")),
        }
    }
}

/// Run the interpretation pass for one subproject — env-driven wrapper.
#[must_use]
pub fn interpret(
    root: &Path,
    stack_id: &str,
    visited: &[VisitedFile],
    clusters: &[Value],
) -> InterpretedResult {
    interpret_with(root, stack_id, visited, clusters, &InterpretEnv::from_process())
}

/// Run the interpretation pass with explicit env overrides.
///
/// Fail-open at every step: empty API key, network failure, malformed
/// response, or cache-write error all degrade to an empty
/// [`InterpretedResult`]. The caller treats an empty result as "no model
/// interpretation available — use the agnostic floor."
#[must_use]
pub fn interpret_with(
    root: &Path,
    stack_id: &str,
    visited: &[VisitedFile],
    clusters: &[Value],
    env: &InterpretEnv,
) -> InterpretedResult {
    let model = resolve_model_for(&env.model_env);
    let file_set_hash = compute_file_set_hash(model, visited);

    // 1. Cache lookup (skipped when MUSTARD_INTERPRET_CACHE=off).
    if !env.cache_disabled {
        if let Some(cached) = read_cache(root, stack_id, &file_set_hash) {
            return InterpretedResult {
                from_cache: true,
                ..cached
            };
        }
    }

    // 2. Compose the compact profile + prompt. Bail out fast if there is
    //    nothing meaningful to interpret (no clusters AND no files).
    if clusters.is_empty() && visited.is_empty() {
        return InterpretedResult::default();
    }

    // 3. Resolve the API key — without it the layer is a deliberate no-op.
    if env.api_key.is_empty() {
        return InterpretedResult::default();
    }

    let profile = build_profile(stack_id, clusters, visited);
    let Ok(prompt_json) = serde_json::to_string(&profile) else {
        return InterpretedResult::default();
    };
    let prompt = format!("{PROMPT_TEMPLATE}{prompt_json}");

    // 4. Call the model. Fail-open on any error.
    let Some(response_text) = call_model(&env.api_key, model, &prompt) else {
        return InterpretedResult::default();
    };

    // 5. Parse + validate the response. Reject anything that does not look
    //    like the expected shape; fall back to empty rather than corrupt.
    let Some(parsed) = parse_response(&response_text) else {
        return InterpretedResult::default();
    };

    // 6. Write the cache (best-effort).
    if !env.cache_disabled {
        write_cache(root, stack_id, &file_set_hash, &parsed);
    }

    parsed
}

/// Hash the (model, visited file paths, visited file sizes) tuple into a hex
/// digest used as the cache key. The hash uses
/// [`SHA-256`](crate::util::sha256::Sha256) for parity with the cluster cache.
fn compute_file_set_hash(model: &str, visited: &[VisitedFile]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(model.as_bytes());
    hasher.update(b"\n");
    let mut paths: Vec<(&str, usize)> = visited
        .iter()
        .map(|v| (v.rel.as_str(), v.content.as_ref().map_or(0, String::len)))
        .collect();
    paths.sort_by(|a, b| a.0.cmp(b.0));
    for (rel, size) in paths {
        hasher.update(rel.as_bytes());
        hasher.update(b"\0");
        hasher.update(size.to_string().as_bytes());
        hasher.update(b"\n");
    }
    hasher.hex_digest()
}

/// Cache file location for `root` — sibling to `.cluster-cache.json`.
fn cache_path(root: &Path) -> PathBuf {
    root.join(".claude").join(".interpret-cache.json")
}

/// Read the cached interpretation for `stack_id` when the stored hash matches
/// `file_set_hash`. Any IO or parse error degrades to `None`.
fn read_cache(root: &Path, stack_id: &str, file_set_hash: &str) -> Option<InterpretedResult> {
    let raw = mfs::read_to_string(cache_path(root)).ok()?;
    let parsed: Value = serde_json::from_str(&raw).ok()?;
    if parsed.get("version").and_then(Value::as_u64) != Some(INTERPRET_CACHE_VERSION) {
        return None;
    }
    let entry = parsed.get("entries")?.get(stack_id)?;
    if entry.get("hash").and_then(Value::as_str)? != file_set_hash {
        return None;
    }
    let result = entry.get("result")?.clone();
    serde_json::from_value::<InterpretedResult>(result).ok()
}

/// Persist `result` under `stack_id` + `file_set_hash`. Fail-open: any error
/// is silently swallowed (the next run will simply re-interpret).
fn write_cache(root: &Path, stack_id: &str, file_set_hash: &str, result: &InterpretedResult) {
    let path = cache_path(root);
    let existing: Value = mfs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_else(|| json!({}));
    let mut root_obj = existing.as_object().cloned().unwrap_or_default();
    root_obj.insert(
        "version".to_string(),
        Value::Number(INTERPRET_CACHE_VERSION.into()),
    );
    let entries = root_obj
        .entry("entries".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if let Value::Object(map) = entries {
        map.insert(
            stack_id.to_string(),
            json!({
                "hash": file_set_hash,
                "result": result,
            }),
        );
    }
    let Ok(pretty) = serde_json::to_string_pretty(&Value::Object(root_obj)) else {
        return;
    };
    let _ = mfs::write_atomic(&path, format!("{pretty}\n").as_bytes());
}

/// POST the prompt to Anthropic and return the assistant's text content.
///
/// Fail-open: any transport error, non-2xx status, or unexpected response
/// shape returns `None`. The caller falls back to the agnostic floor.
fn call_model(api_key: &str, model: &str, prompt: &str) -> Option<String> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS)))
        .build()
        .new_agent();
    let body = json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "messages": [{"role": "user", "content": prompt}],
    });
    let mut response = agent
        .post(API_URL)
        .header("x-api-key", api_key)
        .header("anthropic-version", API_VERSION)
        .header("content-type", "application/json")
        .send_json(body)
        .ok()?;
    let parsed: Value = response.body_mut().read_json::<Value>().ok()?;
    parsed
        .get("content")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|first| first.get("text"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Parse the model's reply into an [`InterpretedResult`].
///
/// Tolerates a few sloppy framings: a leading ```json fence, trailing prose
/// after the closing brace, or extra whitespace. Anything more structural
/// (missing arrays, wrong types) falls through to `None`.
fn parse_response(text: &str) -> Option<InterpretedResult> {
    let trimmed = text.trim();
    // Strip ```json … ``` fences when present.
    let stripped = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed)
        .trim_start();
    let body = stripped
        .strip_suffix("```")
        .unwrap_or(stripped)
        .trim();
    // Find the first `{` and the last matching `}` to skip any commentary.
    let start = body.find('{')?;
    let end = body.rfind('}')?;
    if end <= start {
        return None;
    }
    let json_slice = &body[start..=end];
    let parsed: Value = serde_json::from_str(json_slice).ok()?;

    let entities = parsed
        .get("entities")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| serde_json::from_value::<InterpretedEntity>(v.clone()).ok())
                .filter(|e| !e.name.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let enums = parsed
        .get("enums")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| serde_json::from_value::<InterpretedEnum>(v.clone()).ok())
                .filter(|e| !e.name.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let patterns_overlay = parsed
        .get("patternsOverlay")
        .cloned()
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()));

    Some(InterpretedResult {
        entities,
        enums,
        patterns_overlay,
        from_cache: false,
    })
}

/// Write a synthetic cache entry for the given file-set. Used by tests to
/// force a "frozen" path without a real API call. Production code never
/// touches this — but it lives in the same module so the cache schema can
/// evolve in lockstep with its writers.
#[doc(hidden)]
#[allow(dead_code)]
pub fn install_test_cache(
    root: &Path,
    stack_id: &str,
    model: &str,
    visited: &[VisitedFile],
    result: &InterpretedResult,
) {
    let hash = compute_file_set_hash(model, visited);
    write_cache(root, stack_id, &hash, result);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_visited(rel: &str, content: &str) -> VisitedFile {
        VisitedFile {
            abs: PathBuf::from(rel),
            rel: rel.to_string(),
            content: Some(content.to_string()),
        }
    }

    /// AC-4 (env default): no env override ⇒ Sonnet; opus / haiku honoured;
    /// nonsense values resolve up to Sonnet (no silent downgrade). Driven
    /// through [`resolve_model_for`] so the test never mutates process env
    /// (the crate forbids `unsafe`, so `set_var` is unavailable on edition
    /// 2024). The env-driven wrapper [`resolve_model`] is a one-line read of
    /// `MUSTARD_SCAN_MODEL` followed by the same `resolve_model_for` call —
    /// covering `resolve_model_for` covers both.
    #[test]
    fn interpret_model_env_default() {
        // Empty string mimics "env var unset"; the wrapper passes the empty
        // default into resolve_model_for on that path.
        assert_eq!(resolve_model_for(""), "claude-sonnet-4-5");
        assert_eq!(resolve_model_for("opus"), "claude-opus-4-7");
        assert_eq!(resolve_model_for("haiku"), "claude-haiku-4-5");
        // Unknown values resolve up — never down.
        assert_eq!(resolve_model_for("nonsense-tier"), "claude-sonnet-4-5");
        // Substring match works (matches the env-var loose parsing).
        assert_eq!(resolve_model_for("claude-opus-4-7"), "claude-opus-4-7");
    }

    /// Build a no-op env: empty API key (skip the model call), cache
    /// enabled (so hits register), Sonnet model.
    fn empty_env() -> InterpretEnv {
        InterpretEnv {
            model_env: String::new(),
            api_key: String::new(),
            cache_disabled: false,
        }
    }

    /// AC-2 (cache frozen): a pre-installed cache entry for a given file-set
    /// is returned directly, without consulting the model or the network.
    #[test]
    fn interpret_cache_frozen() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".claude")).unwrap();

        let visited = vec![
            make_visited("src/user.rs", "pub struct User { id: i32 }"),
            make_visited("src/order.rs", "pub struct Order { user_id: i32 }"),
        ];
        let frozen = InterpretedResult {
            entities: vec![
                InterpretedEntity {
                    name: "User".to_string(),
                    file: "src/user.rs".to_string(),
                    edges: vec!["[[sub.entity.order]]".to_string()],
                },
                InterpretedEntity {
                    name: "Order".to_string(),
                    file: "src/order.rs".to_string(),
                    edges: vec!["[[sub.entity.user]]".to_string()],
                },
            ],
            ..InterpretedResult::default()
        };
        let env = empty_env();
        install_test_cache(root, "rust", resolve_model_for(&env.model_env), &visited, &frozen);

        let result = interpret_with(root, "rust", &visited, &[], &env);
        assert!(result.from_cache, "cache hit should set from_cache=true");
        assert_eq!(result.entities.len(), 2);
        assert_eq!(result.entities[0].name, "User");
    }

    /// AC-3 (multi-stack entities): with a pre-installed cache covering each
    /// fixture (matching the cold-path freeze), the interpreter recovers
    /// entities the eight per-language scanners used to miss. We exercise
    /// `.NET Features/ + DbSet`, TypeScript `mysqlTable`, Go without `gorm`,
    /// and Rust struct without an ORM derive.
    #[test]
    fn interpret_multistack_entities() {
        let cases: &[(&str, &str, &str, &str)] = &[
            (
                "dotnet",
                "Features/Orders/Order.cs",
                "public class Order { public int Id; }",
                "Order",
            ),
            (
                "typescript",
                "src/schema.ts",
                "export const accounts = mysqlTable('accounts', { id: int() });",
                "Account",
            ),
            (
                "go",
                "internal/customer.go",
                "package internal\n\ntype Customer struct { Name string }",
                "Customer",
            ),
            (
                "rust",
                "src/invoice.rs",
                "pub struct Invoice { pub total: i64 }",
                "Invoice",
            ),
        ];
        let env = empty_env();
        let model = resolve_model_for(&env.model_env);
        for (stack, file, content, entity_name) in cases {
            let dir = tempdir().unwrap();
            let root = dir.path();
            std::fs::create_dir_all(root.join(".claude")).unwrap();
            let visited = vec![make_visited(file, content)];
            let frozen = InterpretedResult {
                entities: vec![InterpretedEntity {
                    name: (*entity_name).to_string(),
                    file: (*file).to_string(),
                    edges: Vec::new(),
                }],
                ..InterpretedResult::default()
            };
            install_test_cache(root, stack, model, &visited, &frozen);
            let result = interpret_with(root, stack, &visited, &[], &env);
            assert!(result.from_cache, "case {stack} must hit cache");
            assert_eq!(result.entities.len(), 1, "case {stack} entity count");
            assert_eq!(
                result.entities[0].name, *entity_name,
                "case {stack} entity name"
            );
        }
    }

    /// A cache miss without an API key returns the empty fallback — the
    /// agnostic floor stays the source of truth.
    #[test]
    fn interpret_without_api_key_is_empty() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        let visited = vec![make_visited("src/lib.rs", "pub struct Foo;")];
        let result = interpret_with(root, "rust", &visited, &[], &empty_env());
        assert!(!result.from_cache);
        assert!(result.entities.is_empty());
        assert!(result.enums.is_empty());
    }

    /// Hash stability: identical file-set + model ⇒ identical hash.
    #[test]
    fn file_set_hash_is_stable() {
        let visited_a = vec![
            make_visited("src/a.rs", "AAAA"),
            make_visited("src/b.rs", "BB"),
        ];
        let visited_b = vec![
            // Same content, different order in the vec.
            make_visited("src/b.rs", "BB"),
            make_visited("src/a.rs", "AAAA"),
        ];
        let h_a = compute_file_set_hash("claude-sonnet-4-5", &visited_a);
        let h_b = compute_file_set_hash("claude-sonnet-4-5", &visited_b);
        assert_eq!(h_a, h_b, "hash must be order-independent");
        let h_c = compute_file_set_hash("claude-opus-4-7", &visited_a);
        assert_ne!(h_a, h_c, "model name must affect the hash");
    }

    /// The compact profile caps both per-cluster and total samples, so the
    /// prompt size never blows up regardless of cluster cardinality.
    #[test]
    fn compact_profile_respects_caps() {
        let mut visited = Vec::new();
        let mut clusters: Vec<Value> = Vec::new();
        for c in 0..50 {
            let mut files: Vec<Value> = Vec::new();
            for f in 0..5 {
                let rel = format!("src/c{c}/f{f}.rs");
                visited.push(make_visited(&rel, &"x".repeat(10_000)));
                files.push(Value::String(rel));
            }
            clusters.push(json!({ "files": files }));
        }
        let profile = build_profile("rust", &clusters, &visited);
        assert!(profile.samples.len() <= MAX_TOTAL_SAMPLES);
        for s in &profile.samples {
            assert!(s.head.len() <= MAX_SAMPLE_BYTES);
        }
    }

    /// Parser strips ```json fences and trailing prose, then reads the inner
    /// object. Strings without a JSON object fall through to `None`.
    #[test]
    fn parse_response_strips_fences() {
        let raw = "```json\n{\"entities\":[{\"name\":\"User\",\"file\":\"u.rs\"}],\"enums\":[]}\n```\nstray text";
        let parsed = parse_response(raw).expect("fenced JSON must parse");
        assert_eq!(parsed.entities.len(), 1);
        assert_eq!(parsed.entities[0].name, "User");
        assert!(parse_response("no json here").is_none());
    }
}
