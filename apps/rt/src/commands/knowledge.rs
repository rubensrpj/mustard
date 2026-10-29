use serde_json::json;
use std::path::Path;

pub struct Modes<'a> { pub profile:bool,
    pub prepare_session: Option<&'a str>, pub session_model: Option<&'a str>, pub complete_session: Option<&'a Path>,
    pub cancel_session: Option<&'a str>,
    pub coverage: bool,
    pub task: mustard_core::domain::knowledge::investigation::Task<'a> }

/// O teto, em bytes, da resposta de uma consulta ou de uma busca que vai para
/// a conversa. O terminal do Claude Code guarda fora da conversa a saída
/// acima de 30.000 caracteres, e o agente vê só o começo dela. Um caractere
/// ocupa ao menos um byte, então a resposta dentro do teto cabe nesse limite,
/// com folga para o que o terminal junta a ela.
pub(crate) const CONVERSATION_BYTES: usize = 25_000;

/// Os campos que o cartão encolhido guarda: quem ele é, como foi achado e
/// onde está a fonte, para ler as linhas dela ou expandi-lo com `--symbol`.
const CARD_IDENTITY: [&str; 8] = ["id", "name", "kind", "signature", "source", "documentation", "test_only", "retrieval"];

/// A resposta da consulta, encolhida até caber em `cap` bytes. A que já cabe
/// sai como era. A maior encolhe do que menos falta faz ao que mais faz e
/// para assim que cabe: primeiro sai o trecho de código das alternativas, que
/// guardam o arquivo e as linhas; depois os cartões, do último ao primeiro,
/// ficam só com a identidade e a fonte; por fim saem os documentos e as
/// interpretações, do último ao primeiro. Todo cartão fica, na mesma ordem.
/// `conversation_cap` conta o que saiu, e uma lacuna diz como pedir a
/// resposta inteira. Se nem assim cabe, a resposta sai como ficou.
fn fit_conversation(mut report: serde_json::Value, cap: usize) -> String {
    fn count(report: &mut serde_json::Value, field: &str) {
        let counted = report["conversation_cap"][field].as_u64().unwrap_or(0);
        report["conversation_cap"][field] = json!(counted + 1);
    }
    let whole = report.to_string();
    if whole.len() <= cap {
        return whole;
    }
    report["conversation_cap"] = json!({"cap_bytes": cap, "original_bytes": whole.len(), "alternative_excerpts_omitted": 0,
        "compacted_cards": 0, "omitted_resources": 0, "omitted_interpretations": 0});
    if let Some(gaps) = report["gaps"].as_array_mut() {
        gaps.push(json!(format!("Answer compacted to fit the conversation cap of {cap} bytes (see conversation_cap); compacted cards keep their identity and source range to read. Repeat with --out <file>.json for the whole answer, or expand one card with --symbol <id>.")));
    }
    let mut excerpts = 0;
    for card in report["cards"].as_array_mut().into_iter().flatten() {
        for alternative in card["alternatives"].as_array_mut().into_iter().flatten() {
            excerpts += usize::from(alternative.as_object_mut().and_then(|fields| fields.remove("source_excerpt")).is_some());
        }
    }
    report["conversation_cap"]["alternative_excerpts_omitted"] = json!(excerpts);
    let mut text = report.to_string();
    let cards = report["cards"].as_array().map_or(0, Vec::len);
    for at in (0..cards).rev() {
        if text.len() <= cap {
            return text;
        }
        if let Some(fields) = report["cards"][at].as_object_mut() {
            fields.retain(|key, _| CARD_IDENTITY.contains(&key.as_str()));
            fields.insert("conversation_compacted".into(), json!(true));
        }
        count(&mut report, "compacted_cards");
        text = report.to_string();
    }
    for (list, field) in [("resources", "omitted_resources"), ("interpretations", "omitted_interpretations")] {
        while text.len() > cap && report[list].as_array_mut().and_then(Vec::pop).is_some() {
            count(&mut report, field);
            text = report.to_string();
        }
    }
    text
}

fn read_manifest<T:serde::de::DeserializeOwned>(path:&Path)->Result<T,String> {
    if std::fs::metadata(path).map_err(|e|e.to_string())?.len()>1_000_000 {return Err("knowledge-manifest-too-large".into());}
    serde_json::from_str(&std::fs::read_to_string(path).map_err(|e|e.to_string())?).map_err(|e|e.to_string())
}

pub fn run(root: &Path, query: &mustard_core::io::knowledge::Query<'_>, markdown: bool, out: Option<&Path>, record: Option<&Path>, modes:Modes<'_>) {
    let started=std::time::Instant::now();
    let start = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
    let anchor = mustard_core::io::spec_events::spec_root(&start);
    let git = mustard_core::platform::git::run(&start, &["rev-parse", "--show-toplevel"]);
    let tree = if git.ok { std::path::PathBuf::from(git.stdout.trim()) } else { anchor.clone() };
    let root = if tree != anchor && mustard_core::io::project_map::model_path(&tree).is_file() {
        tree.as_path()
    } else {anchor.as_path()};
    let answer: Result<String, String> = (|| {
        if modes.profile {return Ok(mustard_core::io::project_profile::summary(root,&query.file.map(str::to_owned).into_iter().collect::<Vec<_>>()).to_string());}
        if let Some(id)=modes.cancel_session {
            return mustard_core::io::knowledge::enrichment::cancel_session(root,id).map(|v|v.to_string());
        }
        if let Some(host) = modes.prepare_session {
            let config = mustard_core::domain::config::ProjectConfig::load(&anchor);
            let model = modes.session_model.or_else(|| config.ai.session_models.get(host).map(String::as_str));
            return mustard_core::io::knowledge::enrichment::prepare_session(root, &tree, query.text, query.file, query.limit, host, model).map(|v|v.to_string());
        }
        if let Some(path) = modes.complete_session {
            let receipt = read_manifest(path)?;
            return mustard_core::io::knowledge::enrichment::complete_session(root, &tree, &receipt).map(|v|v.to_string());
        }
        if let Some(record) = record {
            let metadata = std::fs::metadata(record).map_err(|e| e.to_string())?;
            if metadata.len() > 1_000_000 {
                return Err("knowledge-receipt-too-large".into());
            }
            let text = std::fs::read_to_string(record).map_err(|e| e.to_string())?;
            let receipt = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            return mustard_core::io::knowledge::record_at(root, &tree, &receipt).map(|v| v.to_string()).map_err(|e| format!("{e:?}"));
        }
        // Validate the destination before retrieving or writing evidence.
        if let Some(out) = out {
            let absolute = std::path::absolute(out).map_err(|e| e.to_string())?;
            if absolute == mustard_core::io::project_map::model_path(root) || out.extension().is_none_or(|e| e != if markdown { "md" } else { "json" }) {
                return Err("knowledge-invalid-export-path".into());
            }
        }
        let (report,text)=if modes.coverage {
            let report=mustard_core::io::knowledge::coverage::report(root).map_err(|e|format!("{e:?}"))?;
            (report.clone(),report.to_string())
        } else {
            let (report,map)=mustard_core::io::knowledge::query_for(root,&tree,query,modes.task).map_err(|e|format!("{e:?}"))?;
            // A resposta que vai para a conversa respeita o teto; o arquivo de
            // `--out` e a exportação de `--all` saem inteiros.
            let text=if markdown {mustard_core::domain::knowledge::markdown(&report,&map)}
                else if out.is_none() && !query.all {fit_conversation(report.clone(),CONVERSATION_BYTES)}
                else {report.to_string()};
            (report,text)
        };
        if let Some(out) = out {
            mustard_core::io::fs::write_atomic(out, text.as_bytes()).map_err(|e| e.to_string())?;
            return Ok(json!({"ok":true,"file":out,"local_model_calls":report.get("local_model_calls").cloned().unwrap_or_else(||json!(0)),
                "remote_model_calls":report.get("remote_model_calls").cloned().unwrap_or_else(||json!(0))}).to_string());
        }
        Ok(text)
    })();
    match answer {
        Ok(text) => {
            let mut report:serde_json::Value=serde_json::from_str(&text).unwrap_or_else(|_|json!({"ok":true}));
            if report.get("ok").is_none() {report["ok"]=json!(true);}
            super::spec_events::conversation::record_output_call(&anchor,"knowledge",None,started,&report,&text);
            println!("{text}");
        }
        Err(detail) => {
            let report=json!({"ok":false,"reason":"knowledge-unavailable","detail":detail,"hint": "Refresh scan or use an exact search; missing evidence does not prove missing functionality." });
            let text=report.to_string();
            super::spec_events::conversation::record_output_call(&anchor,"knowledge",None,started,&report,&text);
            println!("{text}");
            std::process::exit(1);
        }
    }
}
