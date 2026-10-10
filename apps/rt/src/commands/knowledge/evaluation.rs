//! Explicit retrieval evaluation. Bytes are context size, not billed tokens.
use mustard_core::domain::knowledge::selection::SymbolSelector;
use mustard_core::domain::knowledge::investigation::{Purpose,Task};
use mustard_core::io::knowledge::{Query, query_with_selector};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::{Component, Path};
use std::time::Instant;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Plan {
    pub source_commit: Option<String>,
    pub provenance: Option<String>,
    pub spec: Option<String>,
    pub questions: Vec<Question>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Question {
    pub id: String,
    pub query: String,
    pub expected: Vec<String>,
    #[serde(default)]
    pub symbols: Vec<String>,
    #[serde(default)]
    pub intent: String,
    #[serde(default)]
    pub purpose: Purpose,
}

impl Plan {
    fn validate(&self, tree: &Path) -> Result<(), String> {
        if !(1..=128).contains(&self.questions.len()) {
            return Err("knowledge-evaluation-question-count".into());
        }
        let mut ids = BTreeSet::new();
        for q in &self.questions {
            if q.id.is_empty()
                || q.id.len() > 120
                || !ids.insert(&q.id)
                || q.query.trim().is_empty()
                || q.query.len() > 4000
                || q.intent.len() > 4000
                || q.expected.is_empty()
                || q.expected
                    .iter()
                    .any(|file| file.is_empty() || file.contains('\\') || Path::new(file).components().any(|c| !matches!(c, Component::Normal(_))))
            {
                return Err("knowledge-evaluation-invalid-question".into());
            }
        }
        if let Some(expected) = &self.source_commit {
            let head = mustard_core::platform::git::run(tree, &["rev-parse", "HEAD"]);
            if !head.ok || head.stdout.trim() != expected {
                return Err("knowledge-evaluation-source-commit-mismatch".into());
            }
        }
        Ok(())
    }
}

pub(super) fn evaluate(root: &Path, tree: &Path, plan: &Plan, options: &Query<'_>, selector: Option<&dyn SymbolSelector>) -> Result<Value, String> {
    plan.validate(tree)?;
    let generation = || mustard_core::io::knowledge::generation(root).map_err(|e| format!("{e:?}"));
    let initial = generation()?;
    let mut variants = Vec::new();
    let mut modes = vec![("native-compact", false, None), ("native-detail", true, None), ("native-investigation", false, None), ("native-responsibility-experimental", false, None)];
    if selector.is_some() {
        modes.push(("configured-choice", false, selector));
    }
    for (mode, detail, select) in modes {
        let mut rows = Vec::new();
        let (mut file_hits, mut symbol_hits) = (0, 0);
        let mut calls = Some(0u64);
        for q in &plan.questions {
            let query = Query { text: &q.query, file: None, symbol: None, all: false, refresh: false, detail, ..*options };
            let start = Instant::now();
            let (report, _) = if mode == "native-investigation" {
                mustard_core::io::knowledge::query_for(root,tree,&query,Task {intent:&q.intent,purpose:q.purpose})
            } else if mode == "native-responsibility-experimental" || mode == "configured-choice" {
                query_with_selector(root, tree, &query, select)
            } else {
                mustard_core::io::knowledge::query_with(root, tree, &query)
            }
            .map_err(|e| format!("{e:?}"))?;
            let ms = start.elapsed().as_millis();
            let sources: Vec<_> = report["cards"].as_array().into_iter().flatten().map(|c| c["source"]["file"].as_str().unwrap_or_default()).collect();
            let selected: Vec<_> = report["cards"].as_array().into_iter().flatten().map(|c| c["id"].as_str().unwrap_or_default()).collect();
            let file_hit = q.expected.iter().any(|file| sources.contains(&file.as_str()));
            let matches = |card:&Value| q.expected.iter().any(|file|card["source"]["file"]==*file)
                && q.symbols.iter().any(|name|card["id"]==*name || card["name"]==*name);
            let symbol_hit = (!q.symbols.is_empty()).then(|| report["cards"].as_array().into_iter().flatten().any(&matches));
            let alternative_hit = (!q.symbols.is_empty()).then(|| report["cards"].as_array().into_iter().flatten()
                .flat_map(|card|card["alternatives"].as_array().into_iter().flatten()).any(&matches));
            let current_excerpt = report["cards"].as_array().into_iter().flatten().filter(|card|matches(card))
                .any(|card|card["source_excerpt"]["text"].as_str().is_some_and(|text|!text.is_empty()));
            file_hits += usize::from(file_hit);
            symbol_hits += usize::from(symbol_hit == Some(true));
            calls = calls.zip(report["remote_model_calls"].as_u64()).map(|(a, b)| a + b);
            rows.push(json!({"id":q.id,"query":q.query,"file_hit":file_hit,"symbol_hit":symbol_hit,"alternative_symbol_hit":alternative_hit,
                "target_source_excerpt":current_excerpt,"implementation_sufficiency":"unverified","selected":selected,
                "response_bytes":report.to_string().len(),"elapsed_ms":ms,"remote_model_calls":report["remote_model_calls"],
                "selection":report["responsibility_selection"],"llm_tokens":null}));
        }
        variants.push(json!({"mode":mode,"questions":rows,"file_hits":file_hits,"symbol_hits":symbol_hits,
            "symbol_questions":plan.questions.iter().filter(|q|!q.symbols.is_empty()).count(),"remote_model_calls":calls}));
    }
    if initial != generation()? {
        return Err("knowledge-evaluation-concurrent-scan; retry after writer finishes".into());
    }
    let observation = plan.spec.as_deref().map(|name| {
        let panel = super::super::panel::snapshot(tree, Some(name));
        json!({"spec":name,"usage":panel["specs"].as_array().into_iter().flatten().find(|s|s["name"]==name).map(|s|s["usage"].clone()),
            "project_jev":panel["jev"],"basis":"existing observations only; no real host execution performed by this command"})
    });
    let calls = variants.iter().try_fold(0u64, |sum, v| v["remote_model_calls"].as_u64().map(|n| sum + n));
    let local = if mustard_core::ProjectConfig::load(tree).ai_vectors_enabled() { Value::Null } else { json!(0) };
    let head = mustard_core::platform::git::run(tree, &["rev-parse", "HEAD"]);
    Ok(json!({"ok":true,"provenance":plan.provenance,"source_commit":plan.source_commit,"actual_source_commit":head.ok.then(||head.stdout.trim().to_string()),
        "scan_generation":initial,"variants":variants,
        "remote_model_calls":calls,"local_model_calls":local,
        "spec_observation":observation,"host_execution":false,"llm_tokens":null,
        "meaning":"retrieval relevance and response bytes; not billed token savings, official benchmark scoring or implementation acceptance"}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_manifest_never_reaches_the_paid_selector_or_opens_a_map() {
        struct Never;
        impl SymbolSelector for Never {
            fn select(&self, _: &str, _: &[mustard_core::domain::knowledge::selection::Ambiguity]) -> mustard_core::domain::knowledge::selection::Decisions {
                panic!("Invalid evaluation must be rejected before selection")
            }
        }
        let root = tempfile::tempdir().unwrap();
        let plan: Plan = serde_json::from_value(json!({"questions":[{"id":"q","query":"quartz beacon","expected":["../outside"]}]})).unwrap();
        let options = Query {
            text: "",
            file: None,
            symbol: None,
            all: false,
            detail: false,
            refresh: false,
            limit: 8,
            depth: 0,
            direction: mustard_core::io::knowledge::Direction::Outgoing,
        };
        assert_eq!(evaluate(root.path(), root.path(), &plan, &options, Some(&Never)).unwrap_err(), "knowledge-evaluation-invalid-question");
        assert!(!root.path().join(".claude").exists());
    }
}
