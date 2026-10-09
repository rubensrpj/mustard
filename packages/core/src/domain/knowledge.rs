//! Evidence packs derived from the scan. Static links describe candidates,
//! never execution order, authorization, business intent or test coverage.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::normalize::{Languages, Normalizer};
use super::project_map::{ProjectMap, UseSite, file_history};

pub mod annotation;
pub use annotation::Annotation;

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
    #[serde(default)]
    pub signature: String,
    #[serde(default)]
    pub documentation: String,
    #[serde(default)]
    pub body_comment: String,
    #[serde(default)]
    pub literals: Vec<Value>,
    #[serde(default)]
    pub file_documentation: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<Annotation>,
    pub source: Source,
    pub parse_complete: Option<bool>,
    #[serde(default)]
    pub contracts: Vec<String>,
    #[serde(default)]
    pub routes: Vec<Value>,
    #[serde(default)]
    pub tests: Vec<String>,
    #[serde(default)]
    pub inline_tests: bool,
    #[serde(default)]
    pub outgoing: Vec<Value>,
    #[serde(default)]
    pub callers: Vec<Value>,
    #[serde(default)]
    pub unresolved_calls: usize,
}

/// A first-read projection. The stored evidence is untouched; a detailed
/// query expands the same symbols. Contracts and routes stay visible.
pub fn summary(card: &Card) -> Value {
    let annotations: Vec<_> = card.annotations.iter().take(6).map(|item| json!({
        "tag":item.tag,"text":short(&item.text,240),"line":item.line,"end_line":item.end_line,
    })).collect();
    let outgoing: Vec<_> = card.outgoing.iter().take(3).map(|edge| json!({
        "target":edge["target"],"call_line":edge["call_line"],"resolution":edge["resolution"],
    })).collect();
    let callers: Vec<_> = card.callers.iter().take(2).map(|edge| json!({
        "file":edge["file"],"line":edge["line"],"from":edge["from"],"resolution":edge["resolution"],
        "candidate_count":edge["candidates"].as_array().map_or(0,Vec::len),
    })).collect();
    let mut projection = json!({"id":card.id,"name":card.name,"kind":card.kind,"source":card.source,
        "signature":short(&card.signature,320),"documentation":short(&card.documentation,320),
        "file_documentation":short(&card.file_documentation,220),
        "parse_complete":card.parse_complete,"contracts":card.contracts,"routes":card.routes,
        "outgoing":outgoing,"callers":callers,"inline_tests":card.inline_tests,"unresolved_calls":card.unresolved_calls,
        "detail_counts":{"literals":card.literals.len(),"tests":card.tests.len(),"body_comment_chars":card.body_comment.chars().count()},
        "text_compacted":card.signature.chars().count()>320 || card.documentation.chars().count()>320 || card.file_documentation.chars().count()>220,
    });
    if !card.annotations.is_empty() {
        projection["annotations"] = json!(annotations);
        projection["annotation_status"] = json!("author-assertion; not semantic proof");
        projection["detail_counts"]["annotations"] = json!(card.annotations.len());
        projection["annotations_compacted"] = json!(
            card.annotations.len() > 6
                || card
                    .annotations
                    .iter()
                    .any(|item| item.text.chars().count() > 240)
        );
    }
    projection
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
                annotations:declaration["annotations"].as_array().into_iter().flatten()
                    .filter_map(|item|serde_json::from_value(item.clone()).ok()).collect(),
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
    if query.trim().is_empty() {
        return (0..cards.len()).collect();
    }
    let mut normalizer = Normalizer::new(languages);
    let asked = normalizer.query(query);
    let documents: Vec<BTreeSet<String>> = cards
        .iter()
        .map(|card| {
            normalizer
                .forms(&format!(
                    "{} {} {} {} {} {} {} {} {}",
                    card.name,
                    card.source.file,
                    card.signature,
                    card.documentation,
                    card.body_comment,
                    card.file_documentation,
                    serde_json::to_string(&card.routes).unwrap_or_default(),
                    serde_json::to_string(&card.literals).unwrap_or_default(),
                    annotation_text(card)
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

/// Rank responsibility evidence while keeping the winning declaration's identity.
/// Repeated file headers count once; a declaration's own evidence carries
/// more weight. File selection must not replace that winner with its first field.
pub fn intent_cards(cards: &[Card], query: &str, languages: &Languages) -> Vec<usize> {
    let mut normalizer = Normalizer::new(languages);
    let asked = normalizer.query(query);
    if asked.is_empty() {
        return vec![];
    }
    let documents: Vec<_> = cards
        .iter()
        .map(|card| {
            let header: BTreeSet<_> = normalizer
                .forms(&format!("{} {}", card.source.file, card.file_documentation))
                .into_iter()
                .flatten()
                .collect();
            let own: BTreeSet<_> = normalizer
                .forms(&format!(
                    "{} {} {} {}",
                    card.name,
                    card.documentation,
                    card.body_comment,
                    annotation_text(card)
                ))
                .into_iter()
                .flatten()
                .collect();
            (header, own)
        })
        .collect();
    let files: BTreeSet<_> = cards.iter().map(|card| &card.source.file).collect();
    let weights: Vec<_> = asked
        .iter()
        .map(|forms| {
            let seen: BTreeSet<_> = documents
                .iter()
                .enumerate()
                .filter(|(_, (header, own))| {
                    forms
                        .iter()
                        .any(|form| header.contains(form) || own.contains(form))
                })
                .map(|(i, _)| &cards[i].source.file)
                .collect();
            ((files.len() + 1) as f64 / (seen.len() + 1) as f64).ln() + 1.0
        })
        .collect();
    let mut scored: Vec<_> = documents
        .iter()
        .enumerate()
        .map(|(i, (header, own))| {
            let score = asked
                .iter()
                .zip(&weights)
                .map(|(forms, weight)| {
                    if forms.iter().any(|form| own.contains(form)) {
                        weight * 2.0
                    } else if forms.iter().any(|form| header.contains(form)) {
                        *weight
                    } else {
                        0.0
                    }
                })
                .sum::<f64>();
            (i, score)
        })
        .filter(|(_, score)| *score > 0.0)
        .collect();
    scored.sort_by(|(a, x), (b, y)| {
        y.total_cmp(x)
            .then_with(|| cards[*a].source.file.cmp(&cards[*b].source.file))
            .then_with(|| {
                cards[*a]
                    .source
                    .end_line
                    .saturating_sub(cards[*a].source.line)
                    .cmp(
                        &cards[*b]
                            .source
                            .end_line
                            .saturating_sub(cards[*b].source.line),
                    )
            })
            .then_with(|| cards[*a].id.cmp(&cards[*b].id))
    });
    scored.into_iter().map(|(i, _)| i).collect()
}

/// Interpretations assert one topic. Require all informative query terms in
/// their prose; a shared source path or one generic word is insufficient.
/// Missing lexical evidence falls back to source discovery, not a false claim.
pub fn interpretation_matches(card: &Card, query: &str, languages: &Languages) -> bool {
    if query.trim().is_empty() {
        return true;
    }
    let mut normalizer = Normalizer::new(languages);
    let asked = normalizer.query(query);
    let own: BTreeSet<_> = normalizer
        .forms(&format!("{} {}", card.name, card.documentation))
        .into_iter()
        .flatten()
        .collect();
    !asked.is_empty()
        && asked
            .iter()
            .all(|forms| forms.iter().any(|form| own.contains(form)))
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
    for item in report["refresh_candidates"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let _ = writeln!(
            text,
            "## Revisar interpretação / review interpretation: {}\n\n`{}` · interpretação antiga, revisão necessária / stale, review required.\n",
            item["title"].as_str().unwrap_or_default(),
            item["id"].as_str().unwrap_or_default()
        );
        if let Some(previous) = item["previous_text"].as_str() {
            let _ = writeln!(
                text,
                "Texto anterior, sem validade atual / previous text, not current evidence:\n\n{previous}\n"
            );
        }
        for change in item["changed_sources"].as_array().into_iter().flatten() {
            let _ = writeln!(
                text,
                "- `{}` · {} · hash anterior / previous hash `{}` · atual / current `{}`. Intervalo atual desconhecido / current range unknown.",
                change["previous_source"]["file"]
                    .as_str()
                    .unwrap_or_default(),
                change["reason"].as_str().unwrap_or_default(),
                change["previous_source"]["sha256"]
                    .as_str()
                    .unwrap_or_default(),
                change["current_file"]["sha256"]
                    .as_str()
                    .unwrap_or("unavailable")
            );
        }
        for source in item["unchanged_sources"].as_array().into_iter().flatten() {
            let _ = writeln!(
                text,
                "- Fonte preservada para conferir contexto / unchanged context source: `{}`:{}–{} · SHA-256 `{}`",
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
        if !card.annotations.is_empty() {
            text.push_str("Intenção e regras declaradas pelo autor; sem prova semântica / author assertions, not semantic proof:\n\n");
            for item in &card.annotations {
                let _ = writeln!(
                    text,
                    "- `@{}` · linhas / lines {}–{}: {}",
                    item.tag, item.line, item.end_line, item.text
                );
            }
            text.push('\n');
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
        for edge in &card.callers {
            let _ = writeln!(
                text,
                "- Consumidor / consumer: `{}`:{} · `{}` · {}",
                edge["file"].as_str().unwrap_or_default(),
                edge["line"],
                edge["from"].as_str().unwrap_or_default(),
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

fn annotation_text(card: &Card) -> String {
    card.annotations
        .iter()
        .map(|item| item.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
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
    #[test]
    fn intent_keeps_function_instead_of_a_file_header_match() {
        let mut raw = json!({"modules":[{"path":"orders/service.rs","doc":"Order operations", "declarations":[
            {"name":"client","kind":"field","line":2,"end_line":2},
            {"name":"persist","kind":"method","line":8,"end_line":14,"doc":"Restores the order backup after validation."},
            {"name":"Settings","kind":"struct","line":1,"end_line":30}]}]});
        enrich(&mut raw);
        let map: ProjectMap = serde_json::from_value(raw).unwrap();
        let cards = cards(&map);
        let ranking = intent_cards(&cards, "restore order backup", &Languages::new(["en-US"]));
        assert_eq!(cards[ranking[0]].name, "persist");
    }

    #[test]
    fn interpretation_requires_topic_evidence_instead_of_a_shared_path() {
        let mut raw = json!({"modules":[{"path":"pcp/plan.rs","declarations":[
            {"name":"Creation of PCP plan", "doc":"Copies site from PI to create the PCP plan", "line":1,"end_line":2},
            {"name":"PI officialization", "doc":"Officializes the PI plan. PCP plans do not run this operation", "line":3,"end_line":4}]}]});
        enrich(&mut raw);
        let map: ProjectMap = serde_json::from_value(raw).unwrap();
        let cards = cards(&map);
        let languages = Languages::new(["en-US"]);
        assert!(interpretation_matches(
            &cards[0],
            "creation PCP plan",
            &languages
        ));
        assert!(!interpretation_matches(
            &cards[1],
            "creation PCP plan",
            &languages
        ));
    }
}
