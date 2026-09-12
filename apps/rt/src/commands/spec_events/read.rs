//! `mustard-rt run read <bloco> --spec <nome>` — devolve só o bloco pedido do
//! arquivo de eventos da spec, sem os itens removidos ou substituídos e sem o
//! campo `search`.
//!
//! A saída é um JSON com um evento por linha, na ordem do arquivo:
//!
//! ```text
//! {"ok":true,"spec":"teste","block":"wave-2","count":2,"events":[
//! {"v":1,"id":19,…,"type":"wave",…},
//! {"v":1,"id":20,…,"type":"task",…}
//! ]}
//! ```
//!
//! Uma linha do arquivo que não se entende entra em `warnings`, no idioma do
//! projeto, e o resto é lido.

use std::path::PathBuf;

use mustard_core::domain::spec_events::{search_terms, BlockQuery, Refusal, SpecEvent};
use mustard_core::io::spec_events as store;
use serde_json::{json, Value};

/// Options for `mustard-rt run read`.
pub struct ReadOpts {
    /// Qualquer pasta dentro do repositório.
    pub root: PathBuf,
    pub spec: String,
    pub block: String,
    pub term: Option<String>,
}

/// O núcleo testável de [`run`]: a saída pronta, ou a recusa. Nunca entra em
/// pânico.
pub(crate) fn read_at(opts: &ReadOpts) -> Result<String, Value> {
    let project = super::project(&opts.root);
    let lang = project.lang;
    let refuse = move |refusal: Refusal| super::refused(&refusal, lang);

    let block = opts.block.trim();
    let Some(query) = BlockQuery::parse(block) else {
        return Err(refuse(Refusal::UnknownBlock { found: block.to_string() }));
    };
    let path = store::spec_file(&project.root, &opts.spec).map_err(refuse)?;
    let Some(log) = store::read(&path).map_err(refuse)? else {
        return Err(refuse(Refusal::NoSpecFile { spec: opts.spec.clone() }));
    };
    let terms = opts.term.as_deref().map(search_terms).unwrap_or_default();
    let events: Vec<&SpecEvent> = log.block(query).into_iter().filter(|e| e.matches(&terms)).collect();
    let warnings: Vec<String> = log.skipped.iter().map(|s| s.message(lang)).collect();
    Ok(render(&opts.spec, block, &events, &warnings))
}

fn render(spec: &str, block: &str, events: &[&SpecEvent], warnings: &[String]) -> String {
    let mut out = format!(
        "{{\"ok\":true,\"spec\":{},\"block\":{},\"count\":{},\"events\":[",
        json!(spec),
        json!(block),
        events.len()
    );
    for (i, event) in events.iter().enumerate() {
        out.push_str(if i == 0 { "\n" } else { ",\n" });
        out.push_str(&event.shown());
    }
    if !events.is_empty() {
        out.push('\n');
    }
    out.push(']');
    if !warnings.is_empty() {
        out.push_str(",\"warnings\":");
        out.push_str(&json!(warnings).to_string());
    }
    out.push('}');
    out
}

/// Run `read` and print the block; exit 1 on a refusal.
pub fn run(opts: &ReadOpts) {
    match read_at(opts) {
        Ok(report) => println!("{report}"),
        Err(refusal) => {
            println!("{}", serde_json::to_string_pretty(&refusal).unwrap_or_else(|_| "{}".into()));
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::spec_events::write::{write_at, WriteOpts};
    use tempfile::tempdir;

    fn opts(root: &std::path::Path, block: &str, term: Option<&str>) -> ReadOpts {
        ReadOpts {
            root: root.to_path_buf(),
            spec: "teste".into(),
            block: block.into(),
            term: term.map(str::to_string),
        }
    }

    fn put(root: &std::path::Path, event_type: &str, fields: Value) -> u64 {
        let out = write_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: "teste".into(),
            event_type: event_type.into(),
            json: fields.to_string(),
        });
        assert_eq!(out["ok"], json!(true), "{out}");
        out["id"].as_u64().unwrap()
    }

    fn events(report: &str) -> Vec<Value> {
        let parsed: Value = serde_json::from_str(report).expect("the report is JSON");
        parsed["events"].as_array().cloned().unwrap_or_default()
    }

    #[test]
    fn reading_one_wave_brings_only_that_wave_and_never_the_search_field() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let c1 = put(root, "criterion", json!({"when": "a", "then": "b", "proof": "p", "origin": 1}));
        let c2 = put(root, "criterion", json!({"when": "c", "then": "d", "proof": "q", "origin": 1}));
        put(root, "wave", json!({"n": 1, "text": "Um.", "criteria": [c1], "done_when": "x", "origin": 1}));
        put(root, "task", json!({"wave": 1, "text": "T1.", "files": [{"path": "a.rs"}], "origin": 1}));
        put(root, "wave", json!({"n": 2, "text": "Dois.", "criteria": [c2], "done_when": "y", "origin": 1}));
        put(root, "task", json!({"wave": 2, "text": "T2.", "files": [{"path": "b.rs"}], "origin": 1}));

        let report = read_at(&opts(root, "wave-2", None)).unwrap();
        let got = events(&report);
        assert_eq!(got.len(), 2, "{report}");
        for event in &got {
            let wave = event.get("n").or_else(|| event.get("wave")).and_then(Value::as_u64);
            assert_eq!(wave, Some(2), "{event}");
        }
        assert!(!report.contains("\"search\""), "{report}");
    }

    #[test]
    fn a_term_filters_the_conversation() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        put(root, "message", json!({"author": "user", "text": "apagando a pasta"}));
        put(root, "message", json!({"author": "user", "text": "outro assunto"}));
        let got = events(&read_at(&opts(root, "conversation", Some("apagar"))).unwrap());
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["text"], json!("apagando a pasta"));
    }

    #[test]
    fn an_unknown_block_and_a_spec_without_file_are_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let unknown = read_at(&opts(root, "everything", None)).unwrap_err();
        assert_eq!(unknown["reason"], json!("unknown-block"));
        assert!(unknown["hint"].as_str().unwrap().contains("everything"));
        let missing = read_at(&opts(root, "state", None)).unwrap_err();
        assert_eq!(missing["reason"], json!("no-spec-file"));
    }

    #[test]
    fn a_broken_line_shows_as_a_warning() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        put(root, "message", json!({"author": "user", "text": "oi"}));
        let path = root.join(".claude").join("spec").join("teste").join("spec.ndjson");
        let mut raw = std::fs::read_to_string(&path).unwrap();
        raw.push_str("{quebrada\n");
        std::fs::write(&path, raw).unwrap();
        let report = read_at(&opts(root, "conversation", None)).unwrap();
        let parsed: Value = serde_json::from_str(&report).unwrap();
        assert_eq!(parsed["count"], json!(1));
        assert_eq!(parsed["warnings"].as_array().map(Vec::len), Some(1), "{report}");
    }
}
