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
    /// A structural outline, not a guessed business-domain document. The
    /// reviewed topic plan remains available when a precise domain scope is needed.
    pub fn from_scan(root:&Path)->Result<Self,MapRefusal> {
        let map=crate::io::project_map::read(root)?;
        let mut scopes=BTreeSet::new();
        let mut topics=Vec::new();
        for layer in &map.skeleton {
            if !scopes.insert(layer.dir.clone()) {continue;}
            topics.push(Topic {id:format!("area-{}",topics.len()+1),title:format!("{} · {}",layer.dir,layer.role),
                query:format!("{} {}",layer.dir.replace(['/', '\\']," "),layer.role),file:None});
            if topics.len()==12 {break;}
        }
        if topics.is_empty() {
            topics.push(Topic {id:"project".into(),title:"Projeto / Project".into(),query:map.projects.iter().map(|p|p.name.as_str()).collect::<Vec<_>>().join(" "),file:None});
            if topics[0].query.trim().is_empty() {topics[0].query="project structure".into();}
        }
        Ok(Self {title:"Projeto — áreas, evidências e interpretações / Project evidence".into(),topics})
    }
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
        topics.push(json!({"id":topic.id,"title":topic.title,"query":topic.query,
            "analysis_needed":evidence["interpretations"].as_array().is_none_or(Vec::is_empty),
            "reviewed_interpretations":reviewed,"evidence":evidence}));
    }
    let after = super::generation(root)?;
    if before != after {
        return Err(invalid("knowledge-concurrent-scan; retry topic assembly after the writer finishes"));
    }
    let local = if responsibility && crate::domain::config::ProjectConfig::load(tree).ai_vectors_enabled() { Value::Null } else { json!(0) };
    Ok((
        json!({"ok":true,"title":plan.title,"coverage":{"topics":topics.len(),
            "topics_with_analysis":topics.iter().filter(|t|t["analysis_needed"]==false).count(),
            "project_completeness":"unknown; retrieved evidence is not exhaustive business coverage"},
        "topics":topics,"remote_model_calls":calls,"local_model_calls":local,
        "generation":"native-composition; reviewed interpretations remain author claims, not automatic semantic proof"}),
        map.ok_or_else(|| invalid("knowledge-empty-topics"))?,
    ))
}

pub fn markdown(report: &Value, map: &ProjectMap) -> String {
    use std::fmt::Write as _;
    let mut sources=std::collections::BTreeMap::new();
    let mut out = format!(
        "# {}\n\nEvidência estática atual e interpretações referenciadas / Current static evidence and cited interpretations.\n\n",
        report["title"].as_str().unwrap_or("Project report")
    );
    let _=writeln!(out,"Base / snapshot: `{}`. Cobertura / coverage: {}.\n",map.state.head,report["coverage"]);
    out.push_str("## Mapa do projeto / Project map\n\n");
    for project in &map.projects {let _=writeln!(out,"- `{}` · `{}` · {} arquivos / files",project.name,project.dir,project.code_files);}
    for language in &map.languages {let _=writeln!(out,"- {} · {} arquivos / files · {} linhas / lines",language.language,language.files,language.loc);}
    out.push_str("\nLigações são estáticas: não provam ordem de execução, autorização ou testes. Ausência de evidência não prova ausência de comportamento. / Static links do not prove runtime order, authorization or tests. Missing evidence is not missing behavior.\n\n");
    for topic in report["topics"].as_array().into_iter().flatten() {
        let _ = writeln!(out, "## {}\n", topic["title"].as_str().unwrap_or_default());
        let evidence=&topic["evidence"];
        for note in evidence["interpretations"].as_array().into_iter().flatten() {
            let _=writeln!(out,"### {}\n\n{}\n\nEstado / status: `{}` · origem / origin: `{}`.\n",note["title"].as_str().unwrap_or_default(),note["text"].as_str().unwrap_or_default(),note["status"].as_str().unwrap_or("hypothesis"),note["origin"].as_str().unwrap_or("unknown"));
            for source in note["sources"].as_array().into_iter().flatten() {source_line(&mut out,source,&mut sources);}
        }
        if topic["analysis_needed"]==true {
            let _=writeln!(out,"Interpretação ainda ausente / Interpretation missing. Pergunta / question: {}.\n\nUse `knowledge --prepare-session claude` ou `codex` com essa pergunta somente se precisar explicá-la; reutilize análises atuais.\n",topic["query"].as_str().unwrap_or_default());
        }
        out.push_str("### Recursos e relações / Resources and relationships\n\n");
        for card in evidence["cards"].as_array().into_iter().flatten() {
            let _=writeln!(out,"- **{}** · `{}`",card["name"].as_str().unwrap_or_default(),card["kind"].as_str().unwrap_or_default());
            source_line(&mut out,&card["source"],&mut sources);
            for field in ["signature","documentation","body_comment"] {
                if let Some(text)=card[field].as_str().filter(|s|!s.is_empty()) {let _=writeln!(out,"  {field}: {}",text.replace('\n',"\n  "));}
            }
            for direction in ["callers","outgoing"] {for edge in card[direction].as_array().into_iter().flatten() {
                let name=edge["target"].as_str().or_else(||edge["file"].as_str()).unwrap_or("unresolved");
                let line=edge["call_line"].as_u64().or_else(||edge["line"].as_u64()).map_or_else(||"unknown".into(),|n|n.to_string());
                let _=writeln!(out,"  - {direction}: `{name}` · linha / line {line} · `{}`",edge["resolution"].as_str().unwrap_or("unknown"));
                if edge["source"].is_object() {source_line(&mut out,&edge["source"],&mut sources);}
            }}
            if [("current_outgoing_count","outgoing"),("current_caller_count","callers")].iter().any(|(count,field)|card[count].as_u64().is_some_and(|n|n>card[field].as_array().map_or(0,Vec::len) as u64)) {
                let _=writeln!(out,"  Relações adicionais omitidas / More relationships omitted: `knowledge --symbol {} --detail`.\n",card["id"].as_str().unwrap_or_default());
            }
        }
        for gap in evidence["gaps"].as_array().into_iter().flatten() {let _=writeln!(out,"- Lacuna / gap: {gap}");}
        for resource in evidence["resources"].as_array().into_iter().flatten() {
            let _=writeln!(out,"### {}\n",resource["title"].as_str().unwrap_or("Recurso / Resource"));
            source_line(&mut out,&resource["source"],&mut sources);
            let body=resource["text"].as_str().unwrap_or_default();
            let fence="`".repeat(body.split(|c|c!='`').map(str::len).max().unwrap_or(0).max(2)+1);
            let _=writeln!(out,"\nTexto literal da fonte / Verbatim source:\n\n{fence}text\n{body}\n{fence}\n");
        }
        if evidence["cards"].as_array().is_none_or(Vec::is_empty) {out.push_str("Nenhum símbolo atual recuperado para este tópico / No current symbol retrieved for this topic.\n");}
        if evidence["catalog"]["has_more_candidates"]==true || evidence["navigation"]["omitted_destinations"].as_u64().is_some_and(|n|n>0) {
            out.push_str("Há candidatos ou relações omitidos: expanda o tópico antes de concluir / Candidates or relationships omitted: expand before concluding.\n");
        }
        out.push('\n');
    }
    if !sources.is_empty() {
        out.push_str("## Fontes / Sources\n\nReferências F<n>:linha–fim / References F<n>:start–end.\n\n");
        let mut ordered=sources.into_iter().collect::<Vec<_>>();ordered.sort_by_key(|(_,id)|*id);
        for ((file,hash),id) in ordered {let _=writeln!(out,"- F{id}: `{file}` · SHA-256 `{hash}`");}
    }
    out
}

fn source_line(out:&mut String,source:&Value,sources:&mut std::collections::BTreeMap<(String,String),usize>) {
    use std::fmt::Write as _;
    let next=sources.len()+1;
    let id=sources.entry((source["file"].as_str().unwrap_or_default().into(),source["sha256"].as_str().unwrap_or_default().into())).or_insert(next);
    let _=writeln!(out,"  - F{id}:{}–{}",source["line"],source["end_line"]);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dossier_preserves_cited_interpretations_and_gaps_without_repeating_project_inventory() {
        let map=ProjectMap::default();
        let source=json!({"file":"src/hook.ts","line":10,"end_line":20,"sha256":"current-hash"});
        let report=json!({"title":"Workflow","coverage":{"topics":2},"topics":[
            {"title":"Completion","query":"completion hook","analysis_needed":false,"evidence":{"interpretations":[{"title":"Completion hook","text":"Completion writes files [s0].","status":"hypothesis","origin":"session:claude:haiku","sources":[source]}],"cards":[],"gaps":["runtime not observed"]}},
            {"title":"Storage","query":"storage links","analysis_needed":true,"evidence":{"cards":[{"id":"src/hook.ts:10:complete","name":"complete","source":source,
                "outgoing":[{"target":"src/store.ts:30:write","call_line":12,"resolution":"unique-static-target","source":{"file":"src/store.ts","line":30,"end_line":40,"sha256":"store-hash"}}],
                "callers":[{"file":"src/entry.ts","line":4,"resolution":"ambiguous"}],"current_caller_count":3}],"gaps":[]}}
        ]});
        let text=markdown(&report,&map);
        assert_eq!(text.matches("## Mapa do projeto").count(),1);
        assert!(text.contains("Completion writes files [s0]"));
        assert!(text.contains("hypothesis") && text.contains("src/hook.ts") && text.contains("current-hash"));
        assert!(text.contains("runtime not observed") && text.contains("Interpretation missing"));
        assert!(text.contains("src/store.ts:30:write") && text.contains("line 12") && text.contains("store-hash"));
        assert_eq!(text.matches("SHA-256 `current-hash`").count(),1,"the same file/hash is catalogued once");
        assert_eq!(text.matches("F1:10–20").count(),2,"both source ranges remain referenced");
        assert!(text.contains("src/entry.ts") && text.contains("line 4") && text.contains("ambiguous"));
        assert!(text.contains("More relationships omitted"));
        assert!(!text.contains("behavior verified"));
    }
}
