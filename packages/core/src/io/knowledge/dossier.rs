//! Compose a topic report from current native evidence and recorded reviews.
//! Topics are supplied by the responsible person/model; no prose is inferred.
use super::{Query, invalid, query_with_selector};
use crate::domain::knowledge::{self, selection::SymbolSelector};
use crate::domain::project_map::{MapRefusal, ProjectMap};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub title: String,
    pub topics: Vec<Topic>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Topic {
    pub id: String,
    pub title: String,
    pub query: String,
    #[serde(default)]
    pub file: Option<String>,
}

impl Plan {
    pub fn validate(&self) -> Result<(), MapRefusal> {
        if self.title.trim().is_empty() || self.title.len() > 400 || !(1..=24).contains(&self.topics.len()) {
            return Err(invalid("knowledge-invalid-topics"));
        }
        let mut seen = BTreeSet::new();
        for topic in &self.topics {
            if topic.id.is_empty()
                || topic.id.len() > 120
                || !topic.id.chars().all(|c| c.is_ascii_alphanumeric() || "-_".contains(c))
                || !seen.insert(&topic.id)
                || topic.title.trim().is_empty()
                || topic.title.len() > 400
                || topic.query.trim().is_empty()
                || topic.query.len() > 4000
            {
                return Err(invalid("knowledge-invalid-topic"));
            }
        }
        Ok(())
    }
}

pub fn assemble(
    root: &Path,
    tree: &Path,
    plan: &Plan,
    options: &Query<'_>,
    selector: Option<&dyn SymbolSelector>,
    responsibility: bool,
) -> Result<(Value, ProjectMap), MapRefusal> {
    plan.validate()?;
    let mut topics = Vec::new();
    let mut map = None;
    let mut calls = Some(0u64);
    let before = super::generation(root)?;
    for topic in &plan.topics {
        let query = Query { text: &topic.query, file: topic.file.as_deref(), symbol: None, refresh: false, ..*options };
        let (evidence, source) = if responsibility { query_with_selector(root, tree, &query, selector) } else {
            super::query_for(root, tree, &query, knowledge::investigation::Task { intent: "", purpose: knowledge::investigation::Purpose::Understand })
        }?;
        calls = calls.zip(evidence["remote_model_calls"].as_u64()).map(|(a, b)| a + b);
        if map.is_none() {
            map = Some(source);
        }
        let reviewed: Vec<_> =
            evidence["interpretations"].as_array().into_iter().flatten().filter(|note| note["status"] == "reviewed").map(|note| note["id"].clone()).collect();
        topics.push(json!({"id":topic.id,"title":topic.title,"reviewed_interpretations":reviewed,"evidence":evidence}));
    }
    let after = super::generation(root)?;
    if before != after {
        return Err(invalid("knowledge-concurrent-scan; retry topic assembly after the writer finishes"));
    }
    let local = if responsibility && crate::domain::config::ProjectConfig::load(tree).ai_vectors_enabled() { Value::Null } else { json!(0) };
    Ok((
        json!({"ok":true,"title":plan.title,"topics":topics,"remote_model_calls":calls,"local_model_calls":local,
        "generation":"native-composition; reviewed interpretations remain author claims, not automatic semantic proof"}),
        map.ok_or_else(|| invalid("knowledge-empty-topics"))?,
    ))
}

pub fn markdown(report: &Value, map: &ProjectMap) -> String {
    use std::fmt::Write as _;
    let mut out = format!(
        "# {}\n\nNative evidence and current reviewed interpretations. Missing evidence is not proof of missing behavior.\n\n",
        report["title"].as_str().unwrap_or("Project report")
    );
    for topic in report["topics"].as_array().into_iter().flatten() {
        let _ = writeln!(out, "## {}\n", topic["title"].as_str().unwrap_or_default());
        let _ = writeln!(out, "Reviewed references: {}.\n", topic["reviewed_interpretations"]);
        // Existing renderer keeps source ranges, hashes and author/hypothesis
        // status. Demote its headings under the selected topic.
        for line in knowledge::markdown(&topic["evidence"], map).lines() {
            if line.starts_with('#') {
                out.push_str("##");
            }
            out.push_str(line);
            out.push('\n');
        }
        out.push('\n');
    }
    out
}
