//! Evidence packs derived from the scan. Static links describe candidates,
//! never execution order, authorization, business intent or test coverage.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::normalize::{Languages, Normalizer};
use super::text::{CutAt, cut};
use super::project_map::{ProjectMap, UseSite, file_history};

pub mod annotation;
pub mod resources;
pub mod references;
pub mod capabilities;
pub mod evidence;
pub mod selection;
pub mod projection;
pub mod investigation;
mod retrieval;
pub use annotation::Annotation;

pub const VERSION: u64 = 4;

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
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub syntax: Value,
    #[serde(default)]
    pub documentation: String,
    #[serde(default)]
    pub body_comment: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub identifiers: String,
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
    /// A test declaration is searchable evidence, not production behavior.
    #[serde(default)]
    pub test_only: bool,
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
        "target":edge["target"],"source":edge["source"],"call_line":edge["call_line"],"resolution":edge["resolution"],
    })).collect();
    let callers: Vec<_> = card.callers.iter().take(2).map(|edge| json!({
        "file":edge["file"],"line":edge["line"],"from":edge["from"],"source":edge["source"],"resolution":edge["resolution"],
        "candidate_count":edge["candidates"].as_array().map_or(0,Vec::len),
    })).collect();
    let mut projection = json!({"id":card.id,"name":card.name,"kind":card.kind,"source":card.source,
        "signature":short(&card.signature,320),"documentation":short(&card.documentation,320),
        "file_documentation":short(&card.file_documentation,220),
        "parse_complete":card.parse_complete,"contracts":card.contracts.iter().take(3).collect::<Vec<_>>(),"routes":card.routes.iter().take(3).collect::<Vec<_>>(),
        "outgoing":outgoing,"callers":callers,"inline_tests":card.inline_tests,"test_only":card.test_only,"unresolved_calls":card.unresolved_calls,
        "detail_counts":{"literals":card.literals.len(),"tests":card.tests.len(),"body_comment_chars":card.body_comment.chars().count()},
        "text_compacted":compacted(&card.signature,320) || compacted(&card.documentation,320) || compacted(&card.file_documentation,220),
    });
    if !card.syntax.is_null() {
        let mut syntax=card.syntax.clone();
        if let Some(object)=syntax.as_object_mut() {object.remove("source_chunks");}
        if syntax.as_object().is_none_or(|object|!object.is_empty()) {projection["syntax"]=syntax;}
    }
    if !card.annotations.is_empty() {
        projection["annotations"] = json!(annotations);
        projection["annotation_status"] = json!("author-assertion; not semantic proof");
        projection["detail_counts"]["annotations"] = json!(card.annotations.len());
        projection["annotations_compacted"] = json!(
            card.annotations.len() > 6
                || card
                    .annotations
                    .iter()
                    .any(|item| compacted(&item.text, 240))
        );
    }
    if !card.identifiers.is_empty() && card.kind=="source-file" {
        projection["identifiers"]=json!(short(&card.identifiers,320));
        projection["identifiers_compacted"]=json!(compacted(&card.identifiers,320));
    }
    projection["detail_counts"]["identifiers"]=json!(card.identifiers.split_whitespace().count());
    projection["detail_counts"]["contracts"]=json!(card.contracts.len());
    projection["detail_counts"]["routes"]=json!(card.routes.len());
    if card.contracts.len()>3 || card.routes.len()>3 {projection["contracts_or_routes_compacted"]=json!(true);}
    projection
}

/// `text` com até `max` caracteres antes das reticências. O texto que passa
/// do teto por um caractere só cabe no lugar delas e sai inteiro.
fn short(text: &str, max: usize) -> String {
    cut(text, max + 1, CutAt::Char)
}

/// Se [`short`] corta `text` no teto `max`: é o sinal `*_compacted` do
/// cartão, que manda pedir `--detail` para ler o texto inteiro.
fn compacted(text: &str, max: usize) -> bool {
    text.chars().count() > max + 1
}

/// Enrich the raw scanner model before its atomic database transaction. The
/// content hash is calculated over the SAME text the parser saw, not a later
/// read of the file. Unchanged metadata produces identical bytes.
///
/// Os cartões de um arquivo saem do que ele traz e do que os outros dizem
/// dele: o resumo de cada arquivo que o usa e as declarações de outros
/// arquivos que ele chama. Cada arquivo guarda com os cartões a marca dessas
/// entradas (`inputs`); o arquivo que chega com cartões desta versão e com a
/// mesma marca fica com eles, sem refazer, e sai igual ao que a leitura
/// inteira daria. A marca vale para a mesma compilação do scan, a única que
/// toma arquivos do mapa anterior. O arquivo que chega com a marca e sem os
/// cartões, que a leitura do mapa deixou no banco, também fica como está:
/// quem grava os mantém de lá ou, sem como, os faz refazer
/// ([`forget_unloaded`]). Devolve os arquivos cujos cartões se refizeram, em
/// ordem.
pub fn enrich(raw: &mut Value) -> Vec<String> {
    let Some(modules) = raw["modules"].as_array() else {
        return Vec::new();
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
    let path_of = |at: usize| modules[at]["path"].as_str().unwrap_or_default();
    // Só o arquivo escrito por gente tem cartões; os usos se invertem na
    // ordem dos caminhos, a dos cartões de cada arquivo.
    let mut carded: Vec<usize> =
        (0..modules.len()).filter(|&at| modules[at]["file_class"].as_str().unwrap_or_default().is_empty()).collect();
    carded.sort_by(|&a, &b| path_of(a).cmp(path_of(b)));
    let ids: BTreeMap<usize, Vec<String>> = carded.iter().map(|&at| (at, card_ids(path_of(at), &modules[at]))).collect();
    // Every use of each declaration of each file, read once.
    let uses: Vec<Vec<Vec<UseSite>>> = modules
        .iter()
        .map(|module| {
            module["declarations"].as_array().into_iter().flatten().map(|declaration| {
                declaration["used_by"].as_array().into_iter().flatten().filter_map(|site| serde_json::from_value(site.clone()).ok()).collect()
            }).collect()
        })
        .collect();
    // Each use of a declaration, by the file where it is written: the edge
    // that the caller's card shows as outgoing.
    let mut incoming = BTreeMap::<&str, Vec<(usize, usize, &UseSite)>>::new();
    for &at in &carded {
        for (di, sites) in uses[at].iter().enumerate() {
            for site in sites {
                let Some((file, _)) = hashes.get_key_value(site.file.as_str()) else { continue };
                incoming.entry(file).or_default().push((at, di, site));
            }
        }
    }
    let target = |at: usize, di: usize| -> (String, Source) {
        let declaration = &modules[at]["declarations"][di];
        let file = path_of(at);
        (
            ids[&at][di].clone(),
            Source {
                file: file.to_string(),
                line: declaration["line"].as_u64().unwrap_or(0),
                end_line: declaration["end_line"].as_u64().unwrap_or(0),
                sha256: hashes.get(file).copied().unwrap_or_default().to_string(),
            },
        )
    };
    let fingerprints: Vec<String> = modules
        .iter()
        .zip(&uses)
        .map(|(module, sites)| {
            let file = module["path"].as_str().unwrap_or_default();
            let mut inputs = Inputs::default();
            for key in ["path", "file_class", "declarations", "test_lines", "texts", "file_doc", "file_comment", "routes", "tests", "has_tests"] {
                inputs.value(&module[key]);
            }
            for (key, value) in module["analysis"].as_object().into_iter().flatten().filter(|(key, _)| *key != "knowledge") {
                inputs.text(key);
                inputs.value(value);
            }
            for site in sites.iter().flatten() {
                inputs.text(hashes.get(site.file.as_str()).copied().unwrap_or_default());
            }
            for (at, di, site) in incoming.get(file).into_iter().flatten() {
                let (id, source) = target(*at, *di);
                inputs.text(&id);
                inputs.text(&source.sha256);
                inputs.text(&format!("{}:{}:{}:{}:{}", source.line, source.end_line, site.line, site.from, site.is_proven()));
            }
            inputs.finish()
        })
        .collect();
    let kept = |at: usize| {
        let knowledge = &modules[at]["analysis"]["knowledge"];
        knowledge["version"] == VERSION
            && knowledge["inputs"] == fingerprints[at].as_str()
            && (knowledge["cards"].is_array() || knowledge.get("cards").is_none())
    };
    let mut fresh = BTreeMap::<usize, Vec<Card>>::new();
    for &at in carded.iter().filter(|&&at| !kept(at)) {
        let mut entries = file_cards(&modules[at], &ids[&at], &hashes);
        // Reverse the scanner's edges once. Select the narrowest containing
        // caller; overloaded names elsewhere in a file must not acquire its call.
        for (target_at, target_di, site) in incoming.get(path_of(at)).into_iter().flatten() {
            let (id, source) = target(*target_at, *target_di);
            let line = site.line as u64;
            let candidates: Vec<_> = entries
                .iter()
                .enumerate()
                .filter(|(_, card)| card.name == site.from && card.source.line <= line && line <= card.source.end_line)
                .map(|(i, card)| (i, card.source.end_line - card.source.line))
                .collect();
            let narrowest = candidates.iter().map(|(_, width)| *width).min();
            let candidates: Vec<_> = candidates.into_iter().filter(|(_, width)| Some(*width) == narrowest).map(|(i, _)| i).collect();
            let resolution = if candidates.len() == 1 && site.is_proven() { "unique-static-target" } else { "ambiguous" };
            for &i in &candidates {
                entries[i].outgoing.push(json!({"target":id,"source":source,"call_line":line,"resolution":resolution}));
            }
        }
        fresh.insert(at, entries);
    }
    // O arquivo sem cartões também guarda a marca, e só se regrava quando
    // ela muda.
    let stale: Vec<usize> = (0..modules.len()).filter(|&at| fresh.contains_key(&at) || (!ids.contains_key(&at) && !kept(at))).collect();
    let mut rebuilt = Vec::new();
    if let Some(modules) = raw["modules"].as_array_mut() {
        for at in stale {
            let module = &mut modules[at];
            rebuilt.push(module["path"].as_str().unwrap_or_default().to_string());
            if !module["analysis"].is_object() {
                module["analysis"] = json!({});
            }
            let entries = fresh.remove(&at).unwrap_or_default();
            module["analysis"]["knowledge"] = json!({"version":VERSION,"inputs":fingerprints[at],"cards":entries});
        }
    }
    rebuilt.sort();
    rebuilt
}

/// Tira a marca dos cartões de todo arquivo de `raw` que a traz sem os
/// cartões, deixados no banco pela leitura do mapa, para o [`enrich`]
/// seguinte os refazer: é o caminho da gravação que não tem de onde
/// mantê-los. Diz se tirou de algum.
pub fn forget_unloaded(raw: &mut Value) -> bool {
    let mut forgot = false;
    for module in raw["modules"].as_array_mut().into_iter().flatten() {
        let unloaded = module["analysis"]["knowledge"].as_object().is_some_and(|knowledge| !knowledge.contains_key("cards"));
        if let (true, Some(analysis)) = (unloaded, module["analysis"].as_object_mut()) {
            analysis.remove("knowledge");
            forgot = true;
        }
    }
    forgot
}

/// The id of each card of `module`, in the order of its declarations, or of
/// its one file card. Distinct declarations can share file, line and name
/// (for example, two type members on one line). Keep every declaration
/// addressable.
fn card_ids(file: &str, module: &Value) -> Vec<String> {
    let declarations = module["declarations"].as_array().map_or(&[][..], Vec::as_slice);
    if declarations.is_empty() {
        return vec![format!("{file}:1:@file")];
    }
    let base: Vec<String> = declarations
        .iter()
        .map(|declaration| {
            format!("{file}:{}:{}", declaration["line"].as_u64().unwrap_or(0), declaration["name"].as_str().unwrap_or_default())
        })
        .collect();
    let mut counts = BTreeMap::<&str, usize>::new();
    for id in &base {
        *counts.entry(id).or_default() += 1;
    }
    let mut ordinals = BTreeMap::<String, usize>::new();
    base.iter()
        .zip(declarations)
        .map(|(id, declaration)| {
            if counts.get(id.as_str()).copied().unwrap_or(0) < 2 {
                return id.clone();
            }
            let kind = declaration["kind"].as_str().unwrap_or_default();
            let base = format!("{id}:{kind}:{}", declaration["end_line"].as_u64().unwrap_or(0));
            let ordinal = ordinals.entry(base.clone()).or_default();
            let id = format!("{base}:{ordinal}");
            *ordinal += 1;
            id
        })
        .collect()
}

/// The cards of one file written by a person, with the ids `ids`, before the
/// edges reversed from the other files fill their outgoing calls.
/// Executable configuration and scripts may have no grammar-recognized
/// declarations. Keep file evidence addressable without inventing one.
fn file_cards(module: &Value, ids: &[String], hashes: &BTreeMap<&str, &str>) -> Vec<Card> {
    let file = module["path"].as_str().unwrap_or_default();
    let sha256 = module["analysis"]["content_sha256"].as_str().unwrap_or_default();
    let mut cards = Vec::new();
    for (declaration, id) in module["declarations"].as_array().into_iter().flatten().zip(ids) {
        let line = declaration["line"].as_u64().unwrap_or(0);
        let end_line = declaration["end_line"].as_u64().unwrap_or(0);
        let in_test = module["test_lines"].as_array().into_iter().flatten().any(|range| {
            range[0].as_u64().is_some_and(|start| start <= line) && range[1].as_u64().is_some_and(|end| line <= end)
        });
        let name = declaration["name"].as_str().unwrap_or_default();
        let strings = |key: &str| {
            declaration[key].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect()
        };
        let callers = declaration["used_by"].as_array().into_iter().flatten().filter_map(|site| {
            let site: UseSite = serde_json::from_value(site.clone()).ok()?;
            Some(json!({"file":site.file,"line":site.line,"from":site.from,
                "source":Source{file:site.file.clone(),line:site.line as u64,end_line:site.line as u64,sha256:hashes.get(site.file.as_str()).copied().unwrap_or_default().into()},
                "resolution":if site.is_proven(){"unique-static-target"}else{"ambiguous"},"candidates":site.candidates}))
        }).collect();
        cards.push(Card {
            id: id.clone(), name:name.to_string(),
            kind:declaration["kind"].as_str().unwrap_or_default().to_string(),
            signature:short(declaration["signature"].as_str().unwrap_or_default(),1200),
            documentation:short(declaration["doc"].as_str().unwrap_or_default(),800),
            body_comment:short(declaration["body_comment"].as_str().unwrap_or_default(),600),
            identifiers:declaration["body_names"].as_str().unwrap_or_default().to_string(),
            literals:module["texts"].as_array().into_iter().flatten().filter(|text|
                text["owner"].as_str()==Some(name) && text["line"].as_u64().is_some_and(|at|line<=at && at<=end_line))
                .map(evidence::source_literal).collect(),
            syntax:declaration["syntax"].clone(),file_documentation:short(module["file_doc"].as_str().unwrap_or_default(),600),
            annotations:declaration["annotations"].as_array().into_iter().flatten()
                .filter_map(|item|serde_json::from_value(item.clone()).ok()).collect(),
            source:Source {file:file.to_string(),line,end_line,sha256:sha256.to_string()},
            parse_complete:module["analysis"]["parse_complete"].as_bool(),contracts:strings("contract"),
            routes:module["routes"].as_array().into_iter().flatten().filter(|route| route["handler"].as_str()==Some(name))
                .map(|route|json!({"method":route["method"],"path":route["path"],"handler":route["handler"],"line":route["line"]})).collect(),
            tests:module["tests"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect(),
            inline_tests:module["has_tests"].as_bool().unwrap_or(false),test_only:in_test || super::ast::is_test_path(file),outgoing:Vec::new(),callers,
            unresolved_calls:declaration["common_calls"].as_u64().unwrap_or(0) as usize,
        });
    }
    if module["declarations"].as_array().is_some_and(Vec::is_empty)
        && module["analysis"]["origin"]=="tree-sitter" && !super::ast::is_test_path(file) {
        let end_line=module["analysis"]["end_line"].as_u64().unwrap_or(0);
        if end_line>0 {
            cards.push(Card {
                id:format!("{file}:1:@file"), name:std::path::Path::new(file).file_name().and_then(|name|name.to_str()).unwrap_or(file).into(),
                kind:"source-file".into(),signature:String::new(),syntax:Value::Null,documentation:String::new(),
                body_comment:short(module["file_comment"].as_str().unwrap_or_default(),600),
                identifiers:module["analysis"]["file_identifiers"].as_str().unwrap_or_default().into(),
                literals:module["texts"].as_array().into_iter().flatten().map(evidence::source_literal).collect(),
                file_documentation:short(module["file_doc"].as_str().unwrap_or_default(),600),annotations:vec![],
                source:Source{file:file.into(),line:1,end_line,sha256:sha256.into()},
                parse_complete:module["analysis"]["parse_complete"].as_bool(),contracts:vec![],routes:vec![],tests:vec![],inline_tests:false,test_only:false,
                outgoing:vec![],callers:vec![],unresolved_calls:0,
            });
        }
    }
    cards
}

/// A marca das entradas dos cartões de um arquivo: cada valor entra como o
/// texto JSON dele, separado do seguinte por um caractere que esse texto
/// nunca traz sem escapar.
#[derive(Default)]
struct Inputs(std::collections::hash_map::DefaultHasher);

impl Inputs {
    fn value(&mut self, value: &Value) {
        use std::fmt::Write as _;
        let _ = write!(self, "{value}\u{1}");
    }

    fn text(&mut self, text: &str) {
        std::hash::Hasher::write(&mut self.0, text.as_bytes());
        std::hash::Hasher::write_u8(&mut self.0, 1);
    }

    fn finish(&self) -> String {
        format!("{:016x}", std::hash::Hasher::finish(&self.0))
    }
}

impl std::fmt::Write for Inputs {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        std::hash::Hasher::write(&mut self.0, text.as_bytes());
        Ok(())
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
    let terms = retrieval::Terms::of(query, languages);
    let asked = &terms.asked;
    let documents: Vec<BTreeSet<String>> = cards
        .iter()
        .map(|card| {
            normalizer
                .forms(&evidence::searchable_text(card))
                .into_iter()
                .flatten()
                .collect()
        })
        .collect();
    let weights: Vec<f64> = asked
        .iter()
        .map(|forms| {
            let n = documents.iter().filter(|document| forms.iter().any(|form| document.contains(form))).count();
            ((cards.len() + 1) as f64 / (n + 1) as f64).ln() + 1.0
        })
        .collect();
    let mut ranked: Vec<_> = documents
        .iter()
        .enumerate()
        .map(|(i, document)| {
            let score = asked.iter().zip(&weights).filter(|(forms, _)| forms.iter().any(|form| document.contains(form))).map(|(_, weight)| weight).sum::<f64>();
            (i, score * terms.weight(&cards[i].kind))
        })
        .filter(|(_, score)| query.trim().is_empty() || *score > 0.0)
        .collect();
    ranked.sort_by(|(a, x), (b, y)| y.total_cmp(x).then_with(|| cards[*a].id.cmp(&cards[*b].id)));
    ranked.into_iter().map(|(i, _)| i).collect()
}

/// Rank responsibility evidence while keeping the winning declaration's identity.
/// Repeated file headers count once; a declaration's own evidence carries
/// more weight. File selection must not replace that winner with its first field.
pub struct IntentCandidate {
    pub card: usize,
    pub independent: bool,
}

pub fn intent_cards(cards: &[Card], query: &str, languages: &Languages) -> Vec<IntentCandidate> {
    intent_cards_with_weights(cards,query,languages,None)
}

/// Indexed retrieval supplies corpus-wide frequencies so candidate pruning
/// cannot change the meaning of rarity within a declaration's evidence.
pub fn intent_cards_with_weights(cards: &[Card], query: &str, languages: &Languages, corpus_weights: Option<&[f64]>) -> Vec<IntentCandidate> {
    let mut normalizer = Normalizer::new(languages);
    let terms = retrieval::Terms::of(query, languages);
    let asked = &terms.asked;
    if asked.is_empty() {
        return vec![];
    }
    let documents: Vec<_> = cards
        .iter()
        .map(|card| {
            let header: BTreeSet<_> = normalizer.forms(&format!("{} {}", card.source.file, card.file_documentation)).into_iter().flatten().collect();
            let own: BTreeSet<_> = normalizer
                .forms(&format!("{} {} {} {}", card.name, card.documentation, card.body_comment, annotation_text(card)))
                .into_iter()
                .flatten()
                .collect();
            let written:BTreeSet<_>=normalizer.forms(&evidence::secondary_text(card)).into_iter().flatten().collect();
            (header, own,written)
        })
        .collect();
    let files: BTreeSet<_> = cards.iter().map(|card| &card.source.file).collect();
    let weights: Vec<_> = corpus_weights.filter(|weights|weights.len()==asked.len()).map(<[f64]>::to_vec).unwrap_or_else(||asked
        .iter()
        .map(|forms| {
            let seen: BTreeSet<_> = documents
                .iter()
                .enumerate()
                .filter(|(_, (header, own,written))| forms.iter().any(|form| header.contains(form) || own.contains(form) || written.contains(form)))
                .map(|(i, _)| &cards[i].source.file)
                .collect();
            ((files.len() + 1) as f64 / (seen.len() + 1) as f64).ln() + 1.0
        })
        .collect());
    let mut scored: Vec<_> = documents
        .iter()
        .enumerate()
        .map(|(i, (header, own,written))| {
            let score = asked
                .iter()
                .zip(&weights)
                .map(|(forms, weight)| {
                    if forms.iter().any(|form| own.contains(form)) {
                        weight * 2.0
                    } else if forms.iter().any(|form| written.contains(form)) {
                        weight * 1.25
                    } else if forms.iter().any(|form| header.contains(form)) {
                        *weight
                    } else {
                        0.0
                    }
                })
                .sum::<f64>();
            (i, score * terms.weight(&cards[i].kind))
        })
        .filter(|(_, score)| *score > 0.0)
        .collect();
    scored.sort_by(|(a, x), (b, y)| {
        y.total_cmp(x)
            .then_with(|| cards[*a].source.file.cmp(&cards[*b].source.file))
            .then_with(|| cards[*a].source.end_line.saturating_sub(cards[*a].source.line).cmp(&cards[*b].source.end_line.saturating_sub(cards[*b].source.line)))
            .then_with(|| cards[*a].id.cmp(&cards[*b].id))
    });
    scored
        .into_iter()
        .map(|(card, _)| {
            let matches = asked.iter().filter(|slot| slot.iter().any(|form| documents[card].1.contains(form) || documents[card].2.contains(form))).count();
            IntentCandidate { card, independent: asked.len() >= 2 && matches >= 2 }
        })
        .collect()
}

/// Interpretations assert one topic. Require all informative query terms in
/// their prose; a shared source path or one generic word is insufficient.
/// Missing lexical evidence falls back to source discovery, not a false claim.
pub fn interpretation_matches(card: &Card, query: &str, languages: &Languages) -> bool {
    if query.trim().is_empty() {
        return true;
    }
    let mut normalizer = Normalizer::new(languages);
    let asked = retrieval::Terms::of(query, languages).asked;
    let own: BTreeSet<_> = normalizer.forms(&format!("{} {}", card.name, card.documentation)).into_iter().flatten().collect();
    !asked.is_empty() && asked.iter().all(|forms| forms.iter().any(|form| own.contains(form)))
}

/// A file without declarations can still provide all requested lexical
/// evidence. It remains file evidence, never a synthesized function.
pub fn source_file_matches(card:&Card,query:&str,languages:&Languages)->bool {
    if card.kind!="source-file" || query.trim().is_empty() {return false;}
    let mut normalizer=Normalizer::new(languages);
    let own:BTreeSet<_>=normalizer.forms(&evidence::searchable_text(card)).into_iter().flatten().collect();
    let asked=retrieval::Terms::of(query,languages).asked;
    !asked.is_empty() && asked.iter().all(|slot|slot.iter().any(|form|own.contains(form)))
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
    if let Some(purpose) = report["investigation"]["purpose"].as_str() {
        let _ = writeln!(text, "Finalidade / purpose: `{purpose}` · evidência parcial / partial: {}.\n", report["investigation"]["partial"]);
    }
    for hit in report["investigation"]["live_matches"].as_array().into_iter().flatten() {
        let _ = writeln!(text, "## Ocorrência atual sem símbolo confirmado / current unindexed occurrence\n\n`{}`:{} · SHA-256 `{}`\n", hit["source"]["file"].as_str().unwrap_or_default(), hit["source"]["line"], hit["source"]["sha256"].as_str().unwrap_or_default());
        render_excerpt(&mut text, &hit["excerpt"]);
    }
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
    for group in report["capability_candidates"].as_array().into_iter().flatten() {
        let _=writeln!(text,"## Grupo estrutural / structural group: {}\n\nSubgrafo das fontes selecionadas; significado de negócio e ordem de execução não inferidos.\n",group["label"].as_str().unwrap_or_default());
        for entry in group["entry_candidates"].as_array().into_iter().flatten() {
            let _=writeln!(text,"- Entrada candidata / candidate entry: `{}` · {}",entry["symbol"].as_str().unwrap_or_default(),entry["reason"].as_str().unwrap_or_default());
        }
        for edge in group["static_edges"].as_array().into_iter().flatten() {
            let _=writeln!(text,"- `{}` → `{}` · linha / line {} · vínculo estático único.",edge["from"].as_str().unwrap_or_default(),edge["to"].as_str().unwrap_or_default(),edge["line"]);
        }
        text.push('\n');
    }
    for item in report["resources"].as_array().into_iter().flatten() {
        let body = item["text"].as_str().unwrap_or_default();
        let fence = "`".repeat(body.split(|c| c != '`').map(str::len).max().unwrap_or(0).max(2) + 1);
        let source = &item["source"];
        let _ = writeln!(text, "## Recurso / resource: {}\n\n`{}`:{}–{} · SHA-256 `{}`\n\nTexto da fonte, sem validação do comportamento / verbatim source, behavior unverified.\n\n{fence}text\n{body}\n{fence}\n",
            item["title"].as_str().unwrap_or_default(), source["file"].as_str().unwrap_or_default(),
            source["line"], source["end_line"], source["sha256"].as_str().unwrap_or_default());
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
        render_excerpt(&mut text, &item["source_excerpt"]);
        for alternative in item["alternatives"].as_array().into_iter().flatten() {
            let _ = writeln!(text, "Alternativa a conferir / candidate to inspect: `{}` · `{}`:{}–{} · SHA-256 `{}`.\n", alternative["name"].as_str().unwrap_or_default(), alternative["source"]["file"].as_str().unwrap_or_default(), alternative["source"]["line"], alternative["source"]["end_line"], alternative["source"]["sha256"].as_str().unwrap_or_default());
            render_excerpt(&mut text, &alternative["source_excerpt"]);
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

fn render_excerpt(text: &mut String, excerpt: &Value) {
    use std::fmt::Write as _;
    let Some(body) = excerpt["text"].as_str() else { return; };
    let fence = "`".repeat(body.split(|c| c != '`').map(str::len).max().unwrap_or(0).max(2) + 1);
    let _ = writeln!(text, "Trecho atual da fonte / current source excerpt · truncado / truncated: {}.\n\n{fence}text\n{body}\n{fence}\n", excerpt["truncated"]);
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
    fn corpus_frequencies_keep_the_winner_when_the_candidate_subset_changes() {
        let mut raw=json!({"modules":[{"path":"main.rs","declarations":[
            {"name":"first","kind":"function","line":1,"end_line":1,"doc":"cobalt"},
            {"name":"second","kind":"function","line":2,"end_line":2,"doc":"quartz"}]}]});
        for n in 0..30 {raw["modules"].as_array_mut().unwrap().push(json!({"path":format!("noise{n}.rs"),
            "declarations":[{"name":"noise","kind":"function","line":1,"end_line":1,"doc":"cobalt"}]}));}
        enrich(&mut raw);let map:ProjectMap=serde_json::from_value(raw).unwrap();let cards=cards(&map);
        let language=Languages::new(["en-US"]);
        let whole=intent_cards(&cards,"cobalt quartz",&language);
        assert_eq!(cards[whole[0].card].name,"second");
        let subset:Vec<_>=cards.iter().filter(|c|c.source.file=="main.rs").cloned().collect();
        assert_eq!(subset[intent_cards(&subset,"cobalt quartz",&language)[0].card].name,"first");
        let weights=[1.0,16.0_f64.ln()+1.0];
        let indexed=intent_cards_with_weights(&subset,"cobalt quartz",&language,Some(&weights));
        assert_eq!(subset[indexed[0].card].name,"second");
    }
    #[test]
    fn calls_remain_ambiguous_and_test_declarations_remain_identified() {
        let mut raw = json!({"modules":[{"path":"service.rs","analysis":{"content_sha256":"hash","parse_complete":true},"test_lines":[[20,50]],
            "declarations":[{"name":"save","line":1,"end_line":9},{"name":"test_save","line":22,"end_line":30}]},
            {"path":"db.rs","analysis":{"content_sha256":"db-hash"},"declarations":[{"name":"insert","line":1,"end_line":3,
                "used_by":[{"at":"service.rs:4:save","candidates":["db.rs:1:insert","other.rs:1:insert"]}]}]}]});
        enrich(&mut raw);
        let map: ProjectMap = serde_json::from_value(raw.clone()).unwrap();
        let cards = cards(&map);
        assert_eq!(cards.len(), 3);
        let production=cards.iter().find(|card|card.name=="save").unwrap();
        assert!(!production.test_only);
        assert!(cards.iter().find(|card|card.name=="test_save").unwrap().test_only);
        assert_eq!(production.outgoing[0]["resolution"], "ambiguous");
        assert_eq!(production.outgoing[0]["source"]["sha256"], "db-hash");
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
        let ranking: Vec<_> = intent_cards(&cards, "restore order backup", &Languages::new(["en-US"])).into_iter().map(|item| item.card).collect();
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

    /// O resumo do cartão mostra até 320 caracteres da documentação antes das
    /// reticências e só se diz compactado quando cortou: a de 321 cabe no
    /// lugar delas e sai inteira; a de 322 fica com os 320 primeiros.
    #[test]
    fn a_card_summary_keeps_its_ceiling_before_the_ellipsis_and_flags_only_a_cut() {
        for (chars, shown, flagged) in [(321, "x".repeat(321), false), (322, format!("{}…", "x".repeat(320)), true)] {
            let mut raw = json!({"modules":[{"path":"main.rs","declarations":[{"name":"run","kind":"function","line":1,"end_line":1,"doc":"x".repeat(chars)}]}]});
            enrich(&mut raw);
            let map: ProjectMap = serde_json::from_value(raw).unwrap();
            let projected = summary(&cards(&map)[0]);
            assert_eq!(projected["documentation"], json!(shown), "{chars}");
            assert_eq!(projected["text_compacted"], json!(flagged), "{chars}");
        }
    }
}
