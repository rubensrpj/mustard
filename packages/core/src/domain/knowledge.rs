//! Evidence packs derived from the scan. Static links describe candidates,
//! never execution order, authorization, business intent or test coverage.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::normalize::{Languages, Normalizer};
use super::project_map::{ProjectMap, UseSite, file_history};

pub const VERSION: u64 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub file: String,
    pub line: u64,
    pub end_line: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Card {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub signature: String,
    pub documentation: String,
    pub body_comment: String,
    pub literals: Vec<Value>,
    pub file_documentation: String,
    pub source: Source,
    pub parse_complete: Option<bool>,
    pub contracts: Vec<String>,
    pub routes: Vec<Value>,
    pub tests: Vec<String>,
    pub inline_tests: bool,
    pub outgoing: Vec<Value>,
    pub callers: Vec<Value>,
    pub unresolved_calls: usize,
}

fn short(text: &str, max: usize) -> String {
    let mut chars = text.chars();
    let mut out: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        out.push('…');
    }
    out
}

/// Enrich the raw scanner model before its atomic database transaction. The
/// content hash is calculated over the SAME text the parser saw, not a later
/// read of the file. Unchanged metadata produces identical bytes.
pub fn enrich(raw: &mut Value) {
    let Some(modules) = raw["modules"].as_array() else {
        return;
    };
    let hashes: BTreeMap<_, _> = modules
        .iter()
        .filter_map(|module| {
            Some((
                module["path"].as_str()?,
                module["analysis"]["content_sha256"]
                    .as_str()
                    .unwrap_or_default(),
            ))
        })
        .collect();
    let mut cards = BTreeMap::<String, Vec<Card>>::new();
    for module in modules {
        let file = module["path"].as_str().unwrap_or_default();
        if !module["file_class"].as_str().unwrap_or_default().is_empty() {
            continue;
        }
        let sha256 = module["analysis"]["content_sha256"]
            .as_str()
            .unwrap_or_default();
        for declaration in module["declarations"].as_array().into_iter().flatten() {
            let line = declaration["line"].as_u64().unwrap_or(0);
            let end_line = declaration["end_line"].as_u64().unwrap_or(0);
            let in_test = module["test_lines"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|range| {
                    range[0].as_u64().is_some_and(|start| start <= line)
                        && range[1].as_u64().is_some_and(|end| line <= end)
                });
            if in_test || super::ast::is_test_path(file) {
                continue;
            }
            let name = declaration["name"].as_str().unwrap_or_default();
            let strings = |key: &str| {
                declaration[key]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            };
            let callers = declaration["used_by"].as_array().into_iter().flatten().filter_map(|site| {
                let site: UseSite = serde_json::from_value(site.clone()).ok()?;
                Some(json!({"file":site.file,"line":site.line,"from":site.from,
                    "source":Source{file:site.file.clone(),line:site.line as u64,end_line:site.line as u64,sha256:hashes.get(site.file.as_str()).copied().unwrap_or_default().into()},
                    "resolution":if site.is_proven(){"unique-static-target"}else{"ambiguous"},"candidates":site.candidates}))
            }).collect();
            cards.entry(file.to_string()).or_default().push(Card {
                id: format!("{file}:{line}:{name}"), name:name.to_string(),
                kind:declaration["kind"].as_str().unwrap_or_default().to_string(),
                signature:short(declaration["signature"].as_str().unwrap_or_default(),1200),
                documentation:short(declaration["doc"].as_str().unwrap_or_default(),800),
                body_comment:short(declaration["body_comment"].as_str().unwrap_or_default(),600),
                literals:module["texts"].as_array().into_iter().flatten().filter(|text|
                    text["owner"].as_str()==Some(name) && text["line"].as_u64().is_some_and(|at|line<=at && at<=end_line))
                    .map(|text|json!({"line":text["line"],"kind":text["kind"],"value":short(text["value"].as_str().unwrap_or_default(),400)})).take(12).collect(),
                file_documentation:short(module["file_doc"].as_str().unwrap_or_default(),600),
                source:Source {file:file.to_string(),line,end_line,sha256:sha256.to_string()},
                parse_complete:module["analysis"]["parse_complete"].as_bool(),contracts:strings("contract"),
                routes:module["routes"].as_array().into_iter().flatten().filter(|route| route["handler"].as_str()==Some(name))
                    .map(|route|json!({"method":route["method"],"path":route["path"],"handler":route["handler"],"line":route["line"]})).collect(),
                tests:module["tests"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect(),
                inline_tests:module["has_tests"].as_bool().unwrap_or(false),outgoing:Vec::new(),callers,
                unresolved_calls:declaration["common_calls"].as_u64().unwrap_or(0) as usize,
            });
        }
    }
    // Reverse the scanner's edges once. Select the narrowest containing
    // caller; overloaded names elsewhere in a file must not acquire its call.
    let edges: Vec<_> = cards
        .values()
        .flatten()
        .flat_map(|target| {
            target
                .callers
                .iter()
                .map(move |site| (target.id.clone(), target.source.clone(), site.clone()))
        })
        .collect();
    for (target, source, site) in edges {
        if let Some(callers) = cards.get_mut(site["file"].as_str().unwrap_or_default()) {
            let at = site["line"].as_u64().unwrap_or(0);
            if let Some(caller) = callers
                .iter_mut()
                .filter(|card| {
                    Some(card.name.as_str()) == site["from"].as_str()
                        && card.source.line <= at
                        && at <= card.source.end_line
                })
                .min_by_key(|card| card.source.end_line - card.source.line)
            {
                caller.outgoing.push(json!({"target":target,"source":source,"call_line":at,"resolution":site["resolution"]}));
            }
        }
    }
    if let Some(modules) = raw["modules"].as_array_mut() {
        for module in modules {
            let file = module["path"].as_str().unwrap_or_default();
            let entries = cards.remove(file).unwrap_or_default();
            if !module["analysis"].is_object() {
                module["analysis"] = json!({});
            }
            module["analysis"]["knowledge"] = json!({"version":VERSION,"cards":entries});
        }
    }
}

/// Older scans remain readable. They have no evidence pack rather than an
/// invented hash or an automatically trusted interpretation.
pub fn cards(map: &ProjectMap) -> Vec<Card> {
    map.modules
        .iter()
        .flat_map(|module| {
            module
                .analysis
                .as_ref()
                .into_iter()
                .filter(|analysis| analysis["knowledge"]["version"] == VERSION)
                .flat_map(|analysis| {
                    analysis["knowledge"]["cards"]
                        .as_array()
                        .into_iter()
                        .flatten()
                })
                .filter_map(|value| serde_json::from_value(value.clone()).ok())
        })
        .collect()
}

pub fn ranked(cards: &[Card], query: &str, languages: &Languages) -> Vec<usize> {
    let mut normalizer = Normalizer::new(languages);
    let asked = normalizer.query(query);
    let documents: Vec<BTreeSet<String>> = cards
        .iter()
        .map(|card| {
            normalizer
                .forms(&format!(
                    "{} {} {} {} {} {} {} {}",
                    card.name,
                    card.source.file,
                    card.signature,
                    card.documentation,
                    card.body_comment,
                    card.file_documentation,
                    serde_json::to_string(&card.routes).unwrap_or_default(),
                    serde_json::to_string(&card.literals).unwrap_or_default()
                ))
                .into_iter()
                .flatten()
                .collect()
        })
        .collect();
    let weights: Vec<f64> = asked
        .iter()
        .map(|forms| {
            let n = documents
                .iter()
                .filter(|document| forms.iter().any(|form| document.contains(form)))
                .count();
            ((cards.len() + 1) as f64 / (n + 1) as f64).ln() + 1.0
        })
        .collect();
    let mut ranked: Vec<_> = documents
        .iter()
        .enumerate()
        .map(|(i, document)| {
            let score = asked
                .iter()
                .zip(&weights)
                .filter(|(forms, _)| forms.iter().any(|form| document.contains(form)))
                .map(|(_, weight)| weight)
                .sum::<f64>();
            (i, score)
        })
        .filter(|(_, score)| query.trim().is_empty() || *score > 0.0)
        .collect();
    ranked.sort_by(|(a, x), (b, y)| y.total_cmp(x).then_with(|| cards[*a].id.cmp(&cards[*b].id)));
    ranked.into_iter().map(|(i, _)| i).collect()
}

/// Native report from evidence, not fabricated business prose. A model or a
/// person can add interpretations with the separate multi-source receipt.
pub fn markdown(report: &Value, map: &ProjectMap) -> String {
    use std::fmt::Write as _;
    let mut text = String::from("# Scan — conhecimento do projeto / project knowledge\n\n");
    let _ = writeln!(
        text,
        "Fonte / source: `{}`. Consulta / query: {}.\n",
        map.state.head,
        report["query"].as_str().unwrap_or_default()
    );
    text.push_str("Relações estáticas são candidatas; não demonstram ordem de execução, autorização ou cobertura de testes. Static links are candidates, not runtime or test proofs.\n\n");
    text.push_str("## Catálogo da última passada / last scan catalog\n\nAs fontes recuperadas são conferidas por conteúdo; o catálogo geral retrata a última passada. Selected sources are content-checked; the catalog describes the last scan.\n\n");
    for project in &map.projects {
        let _ = writeln!(
            text,
            "- Projeto / project: `{}` · `{}` · {} · {} arquivos / files",
            project.name, project.dir, project.kind, project.code_files
        );
    }
    for language in &map.languages {
        let _ = writeln!(
            text,
            "- {}: {} arquivos / files · {} linhas / lines",
            language.language, language.files, language.loc
        );
    }
    for layer in &map.skeleton {
        let _ = writeln!(
            text,
            "- `{}`: {} (camada candidata / candidate layer)",
            layer.dir, layer.role
        );
    }
    text.push('\n');
    for item in report["interpretations"].as_array().into_iter().flatten() {
        let _ = writeln!(
            text,
            "## {}\n\n{}\n\nEstado / status: `{}`.\n",
            item["title"].as_str().unwrap_or_default(),
            item["text"].as_str().unwrap_or_default(),
            item["status"].as_str().unwrap_or("hypothesis")
        );
        for source in item["sources"].as_array().into_iter().flatten() {
            let _ = writeln!(
                text,
                "- `{}`:{}–{} · SHA-256 `{}`",
                source["file"].as_str().unwrap_or_default(),
                source["line"],
                source["end_line"],
                source["sha256"].as_str().unwrap_or_default()
            );
        }
        text.push('\n');
    }
    for item in report["cards"].as_array().into_iter().flatten() {
        let Ok(card) = serde_json::from_value::<Card>(item.clone()) else {
            continue;
        };
        let _ = writeln!(
            text,
            "## {}\n\n`{}`:{}–{} · `{}`\n\n```text\n{}\n```\n",
            card.name,
            card.source.file,
            card.source.line,
            card.source.end_line,
            card.source.sha256,
            card.signature
        );
        if !card.documentation.is_empty() {
            let _ = writeln!(
                text,
                "Comentário / documentation:\n\n{}\n",
                card.documentation
            );
        }
        if !card.body_comment.is_empty() {
            let _ = writeln!(
                text,
                "Comentários internos / body comments:\n\n{}\n",
                card.body_comment
            );
        }
        if !card.file_documentation.is_empty() {
            let _ = writeln!(
                text,
                "Contexto do arquivo / file documentation:\n\n{}\n",
                card.file_documentation
            );
        }
        for literal in &card.literals {
            let _ = writeln!(
                text,
                "- Texto extraído / extracted literal (`{}`, linha / line {}): {}",
                literal["kind"].as_str().unwrap_or_default(),
                literal["line"],
                literal["value"].as_str().unwrap_or_default()
            );
        }
        if !card.routes.is_empty() {
            let _ = writeln!(
                text,
                "Rotas / routes: `{}`\n",
                serde_json::to_string(&card.routes).unwrap_or_default()
            );
        }
        if !card.contracts.is_empty() {
            let _ = writeln!(
                text,
                "Contratos / contracts: {}\n",
                card.contracts.join(", ")
            );
        }
        for edge in &card.outgoing {
            let _ = writeln!(
                text,
                "- Chamada / call: `{}` · linha / line {} · {}",
                edge["target"].as_str().unwrap_or_default(),
                edge["call_line"],
                edge["resolution"].as_str().unwrap_or_default()
            );
        }
        let _ = writeln!(
            text,
            "\nTestes candidatos / test candidates: {} · inline: {}.\n",
            card.tests.join(", "),
            card.inline_tests
        );
        if let Some(history) = file_history(&map.history, &card.source.file) {
            let _ = writeln!(
                text,
                "Git (branch base): `{}` · {}\n",
                history.last_commit,
                history.titles.join("; ")
            );
        }
    }
    text.push_str("## Lacunas / gaps\n\n");
    for gap in report["gaps"].as_array().into_iter().flatten() {
        let _ = writeln!(text, "- {}", gap.as_str().unwrap_or_default());
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn calls_remain_ambiguous_and_test_declarations_stay_out() {
        let mut raw = json!({"modules":[{"path":"service.rs","analysis":{"content_sha256":"hash","parse_complete":true},"test_lines":[[20,50]],
            "declarations":[{"name":"save","line":1,"end_line":9},{"name":"test_save","line":22,"end_line":30}]},
            {"path":"db.rs","analysis":{"content_sha256":"db-hash"},"declarations":[{"name":"insert","line":1,"end_line":3,
                "used_by":[{"at":"service.rs:4:save","candidates":["db.rs:1:insert","other.rs:1:insert"]}]}]}]});
        enrich(&mut raw);
        let map: ProjectMap = serde_json::from_value(raw.clone()).unwrap();
        let cards = cards(&map);
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].outgoing[0]["resolution"], "ambiguous");
        assert_eq!(cards[0].outgoing[0]["source"]["sha256"], "db-hash");
        let before = raw.clone();
        enrich(&mut raw);
        assert_eq!(raw, before);
    }
    #[test]
    fn business_words_in_comments_find_the_named_function() {
        let mut raw = json!({"modules":[{"path":"x.rs","declarations":[{"name":"reconcile","line":1,"end_line":5,"doc":"Restaura o backup do plano e valida a sessão."},{"name":"list","line":6,"end_line":9,"doc":"Lista os planos."}]}]});
        enrich(&mut raw);
        let map: ProjectMap = serde_json::from_value(raw).unwrap();
        let cards = cards(&map);
        let ranking = ranked(
            &cards,
            "restaurar backup",
            &Languages::new(["pt-BR", "en-US"]),
        );
        assert_eq!(cards[ranking[0]].name, "reconcile");
        assert_eq!(ranking.len(), 1);
    }
}
