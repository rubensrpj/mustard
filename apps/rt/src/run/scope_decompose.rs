//! `mustard-rt run scope-decompose` — a port of `scripts/scope-decompose.js`.
//!
//! Decides whether a feature spec should be decomposed into multiple waves.
//! Reads signals from stdin (JSON), emits the decision to stdout (JSON).
//! Fail-open: any error emits `{ "decompose": false, "reason": "error-fallback" }`.

use serde_json::{json, Value};
use std::io::Read;

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

/// Dispatch `mustard-rt run scope-decompose`.
pub fn run() {
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
}
