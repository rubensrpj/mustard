//! `mustard-rt run knowledge glossary` — entity-registry glossary browser.
//!
//! Reads `<root>/.claude/entity-registry.json`, iterates entities (skipping
//! `_`-prefixed metadata keys), and renders a table or JSON of name +
//! description + first ref. Optional `--filter TERM` narrows by case-insensitive
//! substring match on name or description.
//!
//! ## Fail-open contract
//!
//! Missing or unparseable registry → `{"entities":[],"totalWithDescription":0,"totalScanned":0}`.
//! Process exits 0 in all paths.

use mustard_core::io::fs;
use mustard_core::ClaudePaths;
use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

pub struct GlossaryOpts {
    pub filter: Option<String>,
    pub format: String,
    pub root: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct EntityEntry {
    pub name: String,
    pub description: String,
    pub reference: String,
}

#[derive(Debug, Serialize)]
pub struct GlossaryOutput {
    pub entities: Vec<EntityEntry>,
    #[serde(rename = "totalWithDescription")]
    pub total_with_description: usize,
    #[serde(rename = "totalScanned")]
    pub total_scanned: usize,
}

// ---------------------------------------------------------------------------
// Registry parsing
// ---------------------------------------------------------------------------

fn load_entities(root: &std::path::Path, filter: Option<&str>) -> GlossaryOutput {
    let Ok(paths) = ClaudePaths::for_project(root) else {
        return GlossaryOutput {
            entities: Vec::new(),
            total_with_description: 0,
            total_scanned: 0,
        };
    };
    let path = paths.entity_registry_json_path();
    let Ok(text) = fs::read_to_string(&path) else {
        return GlossaryOutput {
            entities: Vec::new(),
            total_with_description: 0,
            total_scanned: 0,
        }
    };
    let Ok(registry): Result<Value, _> = serde_json::from_str(&text) else {
        return GlossaryOutput {
            entities: Vec::new(),
            total_with_description: 0,
            total_scanned: 0,
        }
    };

    let Some(obj) = registry.as_object() else {
        return GlossaryOutput {
            entities: Vec::new(),
            total_with_description: 0,
            total_scanned: 0,
        }
    };

    let filter_lower = filter.map(|f| f.to_ascii_lowercase());

    let mut entries: Vec<EntityEntry> = Vec::new();
    let mut total_scanned = 0usize;
    let mut total_with_description = 0usize;

    for (key, val) in obj {
        // Skip metadata keys
        if key.starts_with('_') {
            continue;
        }
        total_scanned += 1;

        let description = val
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("(no description)")
            .to_string();

        if description != "(no description)" {
            total_with_description += 1;
        }

        // Extract first ref
        let reference = extract_first_ref(val);

        // Apply filter
        if let Some(ref fl) = filter_lower {
            let name_lower = key.to_ascii_lowercase();
            let desc_lower = description.to_ascii_lowercase();
            if !name_lower.contains(fl.as_str()) && !desc_lower.contains(fl.as_str()) {
                continue;
            }
        }

        entries.push(EntityEntry {
            name: key.clone(),
            description,
            reference,
        });
    }

    // Sort by name ASC
    entries.sort_by(|a, b| a.name.cmp(&b.name));

    GlossaryOutput {
        entities: entries,
        total_with_description,
        total_scanned,
    }
}

/// Extract the first ref from an entity value. Tries common patterns:
/// `refs[0]`, `ref`, or the first string value under a `refs` array.
fn extract_first_ref(val: &Value) -> String {
    // Try `refs` array
    if let Some(refs) = val.get("refs").and_then(Value::as_array) {
        for r in refs {
            let candidate = r
                .get("path")
                .and_then(Value::as_str)
                .or_else(|| r.as_str())
                .unwrap_or("")
                .to_string();
            if !candidate.is_empty() {
                return candidate;
            }
        }
    }
    // Try single `ref` string
    if let Some(r) = val.get("ref").and_then(Value::as_str) {
        if !r.is_empty() {
            return r.to_string();
        }
    }
    // Try `file` field (used in some v4 schemas)
    if let Some(f) = val.get("file").and_then(Value::as_str) {
        if !f.is_empty() {
            return f.to_string();
        }
    }
    String::new()
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn truncate(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        let t: String = chars[..max - 1].iter().collect();
        format!("{t}…")
    }
}

fn render_table(output: &GlossaryOutput) -> String {
    let header = "| Entity                              | Description                                                          | Ref                                    |";
    let sep    = "|-------------------------------------|----------------------------------------------------------------------|----------------------------------------|";

    let mut lines = vec![header.to_string(), sep.to_string()];

    for e in &output.entities {
        let name_col = format!("{:<35}", &e.name);
        let desc_col = format!("{:<68}", truncate(&e.description, 68));
        let ref_col  = format!("{:<38}", truncate(&e.reference, 38));
        lines.push(format!("| {name_col} | {desc_col} | {ref_col} |"));
    }

    lines.push(String::new());
    lines.push(format!(
        "Scanned: {} | With description: {} | Shown: {}",
        output.total_scanned, output.total_with_description, output.entities.len()
    ));

    lines.join("\n")
}

fn render_json(output: &GlossaryOutput) -> String {
    serde_json::to_string_pretty(output)
        .unwrap_or_else(|_| r#"{"entities":[],"totalWithDescription":0,"totalScanned":0}"#.to_string())
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run(opts: GlossaryOpts) {
    let output = load_entities(&opts.root, opts.filter.as_deref());
    match opts.format.as_str() {
        "json" => println!("{}", render_json(&output)),
        _ => println!("{}", render_table(&output)),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_registry(root: &std::path::Path, content: &str) {
        let dir = root.join(".claude");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("entity-registry.json"), content).unwrap();
    }

    #[test]
    fn glossary_skips_meta_keys() {
        let td = tempdir().unwrap();
        write_registry(
            td.path(),
            r#"{"_meta":{"version":"4.0"},"User":{"description":"A user entity"},"Post":{"description":"A post entity"}}"#,
        );
        let output = load_entities(td.path(), None);
        assert_eq!(output.total_scanned, 2);
        assert!(output.entities.iter().all(|e| !e.name.starts_with('_')));
    }

    #[test]
    fn glossary_filter_case_insensitive() {
        let td = tempdir().unwrap();
        write_registry(
            td.path(),
            r#"{"Alpha":{"description":"Alpha entity"},"Beta":{"description":"Beta entity"},"Gamma":{"description":"Gamma entity"}}"#,
        );
        let output = load_entities(td.path(), Some("BETA"));
        assert_eq!(output.entities.len(), 1);
        assert_eq!(output.entities[0].name, "Beta");
    }

    #[test]
    fn glossary_filter_matches_description() {
        let td = tempdir().unwrap();
        write_registry(
            td.path(),
            r#"{"Alpha":{"description":"contains special keyword"},"Beta":{"description":"no match here"}}"#,
        );
        let output = load_entities(td.path(), Some("special"));
        assert_eq!(output.entities.len(), 1);
        assert_eq!(output.entities[0].name, "Alpha");
    }

    #[test]
    fn glossary_sorted_by_name() {
        let td = tempdir().unwrap();
        write_registry(
            td.path(),
            r#"{"Zeta":{},"Alpha":{},"Mango":{}}"#,
        );
        let output = load_entities(td.path(), None);
        let names: Vec<&str> = output.entities.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["Alpha", "Mango", "Zeta"]);
    }

    #[test]
    fn glossary_missing_registry_returns_empty() {
        let td = tempdir().unwrap();
        let output = load_entities(td.path(), None);
        assert_eq!(output.total_scanned, 0);
        assert!(output.entities.is_empty());
    }

    #[test]
    fn glossary_extracts_first_ref() {
        let td = tempdir().unwrap();
        write_registry(
            td.path(),
            r#"{"User":{"description":"A user","refs":[{"path":"src/models/user.ts"}]}}"#,
        );
        let output = load_entities(td.path(), None);
        assert_eq!(output.entities[0].reference, "src/models/user.ts");
    }

    #[test]
    fn render_json_has_entities_array() {
        let output = GlossaryOutput {
            entities: vec![EntityEntry {
                name: "User".to_string(),
                description: "desc".to_string(),
                reference: "path/to".to_string(),
            }],
            total_with_description: 1,
            total_scanned: 1,
        };
        let json_str = render_json(&output);
        let parsed: Value = serde_json::from_str(&json_str).unwrap();
        assert!(parsed["entities"].as_array().is_some());
        assert_eq!(parsed["entities"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn total_with_description_counts_correctly() {
        let td = tempdir().unwrap();
        write_registry(
            td.path(),
            r#"{"Alpha":{"description":"has desc"},"Beta":{},"Gamma":{"description":"also has desc"}}"#,
        );
        let output = load_entities(td.path(), None);
        assert_eq!(output.total_scanned, 3);
        assert_eq!(output.total_with_description, 2);
    }
}
