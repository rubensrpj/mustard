//! `mustard-rt run scope-decompose` — a port of `scripts/scope-decompose.js`.
//!
//! Decides whether a feature spec should be decomposed into multiple waves.
//!
//! ## Two input paths
//!
//! 1. **stdin (legacy / override).** Reads a signals JSON object
//!    (`fileCount` / `layerCount` / `newEntityCount` / `estimatedTouchPoints` /
//!    `knowledgeMatches` / `text`) and decides. The caller (a SKILL / the LLM)
//!    pre-computes the counts.
//! 2. **`--from-spec <path>` (deterministic, F5-a item 1).** Computes the
//!    structural signals **in Rust** from the spec itself — no LLM glob/grep:
//!    - `fileCount` / `layerCount` from the spec's `## Files` section via
//!      [`crate::commands::wave::wave_lib::parse_files_section`] +
//!      [`crate::commands::wave::wave_lib::detect_role_with`] (the same
//!      classifier the wave gates use, with `mustard.json#rolePatterns`
//!      overrides);
//!    - `newEntityCount` by **diffing the entity registry**: PascalCase entity
//!      tokens referenced in the spec text that are *not yet* present in
//!      `.claude/entity-registry.json` (exact key lookup via [`EntityRegistry`]).
//!    The spec body is still passed as `text` so the roadmap-signal detection
//!    runs identically. The result is the same [`decide`] verdict the stdin path
//!    would emit for the equivalent signals.
//!
//! Fail-open: any error emits `{ "decompose": false, "reason": "error-fallback" }`.

use crate::commands::spec::prd_build::pascal_tokens;
use crate::commands::wave::wave_lib::{detect_role_with, load_role_patterns, parse_files_section};
use mustard_core::domain::entity_registry::EntityRegistry;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Roadmap-signal detection across spec text.
struct RoadmapSignal {
    hit: bool,
    matches: Vec<String>,
}

/// Detect roadmap signals in free-text. Mirrors the three JS regex patterns.
fn detect_roadmap_signal(text: &str) -> RoadmapSignal {
    let mut matches: Vec<String> = Vec::new();

    // plans-ref: `\.claude/plans/[^\s"'`)\]]+\.md`
    let lower = text;
    let mut search = 0;
    while let Some(rel) = lower[search..].find(".claude/plans/") {
        let at = search + rel;
        let after = &lower[at..];
        let end = after
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '`' | ')' | ']'))
            .unwrap_or(after.len());
        let token = &after[..end];
        if token.ends_with(".md") {
            matches.push(format!("plans-ref:{token}"));
        }
        search = at + ".claude/plans/".len();
    }

    // wave-numbered: `\b(Wave|W|Etapa|Fase|Phase)\s*\d+\b` (case-insensitive).
    for kw in ["wave", "etapa", "fase", "phase", "w"] {
        find_keyword_number(text, kw, "wave-numbered", &mut matches);
    }

    // roadmap-keyword: `\b(roadmap|multi[-\s]?wave)\b` (case-insensitive).
    let tl = text.to_lowercase();
    for (idx, _) in tl.match_indices("roadmap") {
        if word_boundary(&tl, idx, idx + 7) {
            matches.push(format!("roadmap-keyword:{}", &text[idx..idx + 7]));
        }
    }
    for needle in ["multi-wave", "multi wave", "multiwave"] {
        for (idx, _) in tl.match_indices(needle) {
            if word_boundary(&tl, idx, idx + needle.len()) {
                matches.push(format!(
                    "roadmap-keyword:{}",
                    &text[idx..idx + needle.len()]
                ));
            }
        }
    }

    let has_plans_ref = matches.iter().any(|m| m.starts_with("plans-ref:"));
    let other_hits = matches.iter().filter(|m| !m.starts_with("plans-ref:")).count();
    RoadmapSignal {
        hit: has_plans_ref || other_hits >= 2,
        matches,
    }
}

/// Find `<keyword>\s*<digits>` occurrences with word boundaries.
fn find_keyword_number(text: &str, keyword: &str, label: &str, out: &mut Vec<String>) {
    let tl = text.to_lowercase();
    let mut search = 0;
    while let Some(rel) = tl[search..].find(keyword) {
        let at = search + rel;
        let kw_end = at + keyword.len();
        // `\b` before the keyword.
        let boundary_before = at == 0
            || !is_word_char(tl.as_bytes()[at - 1] as char);
        if boundary_before {
            let after = &tl[kw_end..];
            let ws = after.len() - after.trim_start_matches([' ', '\t']).len();
            let digits_part = &after[ws..];
            let dig_end = digits_part
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(digits_part.len());
            if dig_end > 0 {
                let matched = &text[at..kw_end + ws + dig_end];
                out.push(format!("{label}:{matched}"));
            }
        }
        search = kw_end;
    }
}

/// Whether a char is a JS `\w` word character.
fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Whether `[start, end)` of `s` is bounded by non-word chars.
fn word_boundary(s: &str, start: usize, end: usize) -> bool {
    let before = start == 0 || !is_word_char(s.as_bytes()[start - 1] as char);
    let after = end >= s.len() || !is_word_char(s.as_bytes()[end] as char);
    before && after
}

/// Build the signals object for the result.
fn signals_obj(
    file_count: i64,
    layer_count: i64,
    new_entity_count: i64,
    touch_points: i64,
    historical: usize,
) -> Value {
    json!({
        "fileCount": file_count,
        "layerCount": layer_count,
        "newEntityCount": new_entity_count,
        "estimatedTouchPoints": touch_points,
        "historicalMatches": historical,
    })
}

/// Compute the decomposition decision for an input JSON value.
pub fn decide(input: &Value) -> Value {
    let file_count = input.get("fileCount").and_then(Value::as_i64).unwrap_or(0);
    let layer_count = input.get("layerCount").and_then(Value::as_i64).unwrap_or(0);
    let new_entity_count = input.get("newEntityCount").and_then(Value::as_i64).unwrap_or(0);
    let touch_points = input
        .get("estimatedTouchPoints")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let knowledge_matches = input
        .get("knowledgeMatches")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let text = input.get("text").and_then(Value::as_str).unwrap_or("");

    let roadmap = detect_roadmap_signal(text);
    if roadmap.hit {
        return json!({
            "decompose": true,
            "reason": "roadmap-signal",
            "roadmapMatches": roadmap.matches,
            "signals": signals_obj(file_count, layer_count, new_entity_count, touch_points, 0),
        });
    }

    if !knowledge_matches.is_empty() {
        let id = knowledge_matches[0]
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        return json!({
            "decompose": true,
            "reason": format!("history-match:{id}"),
            "signals": signals_obj(
                file_count, layer_count, new_entity_count, touch_points,
                knowledge_matches.len()
            ),
        });
    }

    if layer_count >= 2 {
        return json!({
            "decompose": true,
            "reason": "multi-layer",
            "signals": signals_obj(file_count, layer_count, new_entity_count, touch_points, 0),
        });
    }

    if file_count > 10 && new_entity_count >= 2 {
        return json!({
            "decompose": true,
            "reason": "wide-and-new-entities",
            "signals": signals_obj(file_count, layer_count, new_entity_count, touch_points, 0),
        });
    }

    json!({
        "decompose": false,
        "reason": "single-layer",
        "signals": signals_obj(file_count, layer_count, new_entity_count, touch_points, 0),
    })
}

/// Compute the deterministic signals JSON for `spec_text`, resolving overrides
/// and the entity registry under `project_root`.
///
/// Mirrors the signals object the stdin path consumes, so the verdict from
/// `decide(&compute_signals_from_spec(...))` equals the stdin verdict for the
/// equivalent inputs. Structural-only; no LLM.
///
/// - `fileCount` = number of paths in the spec's `## Files` section.
/// - `layerCount` = distinct architectural roles across those paths
///   ([`detect_role_with`] with `mustard.json#rolePatterns`). A lone `lib`
///   bucket counts as 1 (matches `exec-rewave-check`).
/// - `newEntityCount` = PascalCase entity tokens referenced in the spec that are
///   **not** already in the registry (registry diff via exact key lookup).
/// - `text` = the full spec body, so [`detect_roadmap_signal`] runs unchanged.
#[must_use]
pub fn compute_signals_from_spec(spec_text: &str, project_root: &Path) -> Value {
    let role_patterns = load_role_patterns(project_root);

    let file_paths = parse_files_section(spec_text).unwrap_or_default();
    let file_count = file_paths.len() as i64;

    let roles: BTreeSet<String> = file_paths
        .iter()
        .map(|f| detect_role_with(f, &role_patterns))
        .collect();
    let layer_count: i64 = if roles.len() == 1 && roles.contains("lib") {
        1
    } else {
        roles.len() as i64
    };

    let new_entity_count = new_entity_count_from_registry(spec_text, project_root);

    json!({
        "fileCount": file_count,
        "layerCount": layer_count,
        "newEntityCount": new_entity_count,
        "text": spec_text,
    })
}

/// Reduce a spec to its narrative prose for entity-reference extraction.
///
/// Markdown structure carries PascalCase tokens that are **not** entities —
/// headings (`# Spec`, `## Files`), file paths in list bullets
/// (`- src/Schema/User.ts`), and code fences. Running [`pascal_tokens`] over the
/// raw spec would count `Spec` / `Files` / path segments as "new entities". This
/// keeps only prose lines so the registry diff reflects real entity mentions:
/// drops heading lines, bullet/numbered list items, and fenced code blocks.
fn spec_prose(spec_text: &str) -> String {
    let mut out = String::new();
    let mut in_fence = false;
    for line in spec_text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || trimmed.starts_with('#') {
            continue;
        }
        // Drop list bullets (`- ...`, `* ...`, `+ ...`, `1. ...`) — these hold
        // file paths / checklist items, not entity prose.
        let is_bullet = matches!(trimmed.chars().next(), Some('-' | '*' | '+'))
            && trimmed[1..].starts_with([' ', '\t']);
        let is_numbered = {
            let digits: String = trimmed.chars().take_while(char::is_ascii_digit).collect();
            !digits.is_empty() && trimmed[digits.len()..].starts_with(". ")
        };
        if is_bullet || is_numbered {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Count PascalCase entity tokens referenced in `spec_text` that are **not yet**
/// present in the entity registry under `project_root`.
///
/// Reuses [`pascal_tokens`] (the same entity-reference heuristic `prd-build`
/// uses) over the spec's narrative prose ([`spec_prose`]) and the canonical
/// [`EntityRegistry`] exact key lookup, so "new" means "referenced but not
/// registered" — a deterministic stand-in for the `newEntityCount` the LLM used
/// to estimate. A missing / unreadable registry fails open to empty (every
/// referenced token then counts as new).
fn new_entity_count_from_registry(spec_text: &str, project_root: &Path) -> i64 {
    let registry = EntityRegistry::load(project_root);
    let known: BTreeSet<String> = registry
        .entity_names()
        .iter()
        .map(|n| n.to_ascii_lowercase())
        .collect();
    pascal_tokens(&spec_prose(spec_text))
        .into_iter()
        .filter(|tok| !known.contains(&tok.to_ascii_lowercase()))
        .count() as i64
}

/// Decide directly from a spec file: compute the deterministic signals, then
/// [`decide`]. Fail-open — an unreadable spec yields the `error-fallback`
/// verdict.
#[must_use]
pub fn decide_from_spec(spec_file: &Path) -> Value {
    let Ok(spec_text) = mustard_core::io::fs::read_to_string(spec_file) else {
        return json!({ "decompose": false, "reason": "error-fallback" });
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let spec_dir = spec_file.parent().map_or_else(|| cwd.clone(), Path::to_path_buf);
    let project_root =
        mustard_core::io::workspace::workspace_root(&spec_dir).unwrap_or_else(|_| cwd.clone());
    decide(&compute_signals_from_spec(&spec_text, &project_root))
}

/// Dispatch `mustard-rt run scope-decompose`.
///
/// With `--from-spec <path>`, computes the signals deterministically from the
/// spec (Rust); otherwise reads the signals JSON from stdin (legacy/override).
pub fn run(from_spec: Option<&str>) {
    if let Some(spec_arg) = from_spec {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let spec_file = if Path::new(spec_arg).is_absolute() {
            PathBuf::from(spec_arg)
        } else {
            cwd.join(spec_arg)
        };
        println!("{}", decide_from_spec(&spec_file));
        return;
    }

    let mut raw = String::new();
    let _ = std::io::stdin().read_to_string(&mut raw);
    let input: Value = if raw.trim().is_empty() {
        json!({})
    } else {
        match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(_) => {
                println!("{}", json!({ "decompose": false, "reason": "error-fallback" }));
                return;
            }
        }
    };
    println!("{}", decide(&input));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_layer_decomposes() {
        let d = decide(&json!({ "layerCount": 3, "fileCount": 8 }));
        assert_eq!(d["decompose"], json!(true));
        assert_eq!(d["reason"], json!("multi-layer"));
    }

    #[test]
    fn single_layer_keeps() {
        let d = decide(&json!({ "layerCount": 1, "fileCount": 3 }));
        assert_eq!(d["decompose"], json!(false));
        assert_eq!(d["reason"], json!("single-layer"));
    }

    #[test]
    fn history_match_decomposes() {
        let d = decide(&json!({
            "layerCount": 1,
            "knowledgeMatches": [{ "id": "heavy-pipeline-1" }],
        }));
        assert_eq!(d["reason"], json!("history-match:heavy-pipeline-1"));
    }

    #[test]
    fn roadmap_signal_from_plans_ref() {
        let d = decide(&json!({ "layerCount": 1, "text": "see .claude/plans/roadmap.md" }));
        assert_eq!(d["reason"], json!("roadmap-signal"));
    }

    /// Plant a workspace anchor (`mustard.json` + `.claude/`) so
    /// `workspace_root` accepts `root`, optionally with a v4 registry.
    fn plant_project(root: &std::path::Path, registry_json: Option<&str>) {
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        if let Some(body) = registry_json {
            std::fs::write(root.join(".claude").join("entity-registry.json"), body).unwrap();
        }
    }

    #[test]
    fn from_spec_computes_multi_layer_signals() {
        let dir = tempfile::tempdir().unwrap();
        plant_project(dir.path(), None);
        // Two distinct roles (schema + api) ⇒ layerCount 2 ⇒ multi-layer.
        let spec = "# Spec\n\n## Files\n- src/schema/user.ts\n- src/api/users.ts\n";
        let signals = compute_signals_from_spec(spec, dir.path());
        assert_eq!(signals["fileCount"], json!(2));
        assert_eq!(signals["layerCount"], json!(2));

        // The deterministic path agrees with the equivalent stdin path.
        let from_spec_decision = decide(&signals);
        let stdin_equiv = decide(&json!({
            "fileCount": 2, "layerCount": 2, "newEntityCount": 0, "text": spec,
        }));
        assert_eq!(from_spec_decision, stdin_equiv);
        assert_eq!(from_spec_decision["decompose"], json!(true));
        assert_eq!(from_spec_decision["reason"], json!("multi-layer"));
    }

    #[test]
    fn from_spec_single_layer_keeps() {
        let dir = tempfile::tempdir().unwrap();
        plant_project(dir.path(), None);
        // All files in one generic bucket ⇒ layerCount 1 ⇒ single-layer.
        let spec = "# Spec\n\n## Files\n- src/util/a.ts\n- src/util/b.ts\n";
        let signals = compute_signals_from_spec(spec, dir.path());
        assert_eq!(signals["layerCount"], json!(1));
        assert_eq!(decide(&signals)["reason"], json!("single-layer"));
    }

    #[test]
    fn from_spec_new_entity_count_diffs_registry() {
        let dir = tempfile::tempdir().unwrap();
        // Registry knows `User`; the spec references `User` (known) and
        // `Invoice` (new) ⇒ newEntityCount 1.
        plant_project(dir.path(), Some(r#"{"e":{"User":{}}}"#));
        let spec = "# Spec\nlink the Invoice to the User entity.\n\n## Files\n- src/util/a.ts\n";
        let signals = compute_signals_from_spec(spec, dir.path());
        assert_eq!(signals["newEntityCount"], json!(1), "Invoice new, User known");
    }

    #[test]
    fn from_spec_wide_and_new_entities_decomposes() {
        let dir = tempfile::tempdir().unwrap();
        plant_project(dir.path(), None); // empty registry ⇒ all referenced entities new
        // 11 files in one bucket (layerCount 1) + 2 new entities ⇒ wide-and-new.
        let mut files = String::from("# Spec\nadd the Invoice and the Payment models.\n\n## Files\n");
        for i in 0..11 {
            files.push_str(&format!("- src/util/f{i}.ts\n"));
        }
        let signals = compute_signals_from_spec(&files, dir.path());
        assert_eq!(signals["fileCount"], json!(11));
        assert_eq!(signals["layerCount"], json!(1));
        assert!(signals["newEntityCount"].as_i64().unwrap() >= 2);
        assert_eq!(decide(&signals)["reason"], json!("wide-and-new-entities"));
    }

    #[test]
    fn spec_prose_strips_headings_bullets_and_fences() {
        let spec = "# Title\nReal prose about Invoice.\n\n## Files\n- src/Foo.ts\n1. step\n```\nlet Bar = 1;\n```\nmore prose.\n";
        let prose = spec_prose(spec);
        assert!(prose.contains("Invoice"));
        assert!(prose.contains("more prose"));
        // Headings, list items, and fenced code dropped.
        assert!(!prose.contains("Title"));
        assert!(!prose.contains("Files"));
        assert!(!prose.contains("Foo"));
        assert!(!prose.contains("Bar"));
        assert!(!prose.contains("step"));
    }

    #[test]
    fn decide_from_spec_unreadable_is_fail_open() {
        let d = decide_from_spec(std::path::Path::new("/no/such/spec.md"));
        assert_eq!(d["decompose"], json!(false));
        assert_eq!(d["reason"], json!("error-fallback"));
    }
}
