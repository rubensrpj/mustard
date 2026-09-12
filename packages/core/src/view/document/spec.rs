//! A página de uma spec, montada dos eventos do `spec.ndjson`.
//!
//! Os dez blocos saem sempre, na ordem aprovada: estado, painel de medição,
//! combinado, especificação, critérios, ondas, revisão e QA, andamento,
//! anotações e conversa, esta recolhida. Um bloco sem nada diz que está vazio.
//!
//! Só aparece o que a leitura mostra: um item removido some da página, e a
//! versão antiga de um item revisto aparece só na conversa, marcada como
//! substituída. Cada item leva o seu código (`MSTD-RULE-0005`), que é também o
//! endereço dele, e toda referência a outro evento sai como o código dele.
//! Nada vem do relógio: a hora mostrada é a que cada evento gravou.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::{Document, Field, Item, Meta, Node, Section, Table};
use crate::domain::spec_events::{type_spec, Block, Hidden, Kind, SpecEvent, SpecLog, TYPES};
use crate::platform::i18n::{translate, Locale};

/// Campos com o número de um evento: saem como o código dele.
const EVENT_REF: &[&str] = &["reply_to", "closes", "criterion"];

/// Campos com uma lista de números de eventos.
const EVENT_REFS: &[&str] = &["criteria", "covers", "items", "result", "targets", "contracts"];

/// Campos com números de ondas.
const WAVE_NUMBERS: &[&str] = &["wave", "waves", "depends_on"];

/// Campos cujo valor é um nome do código, um comando ou um identificador:
/// saem entre crases.
const LITERAL: &[&str] =
    &["sha", "proof", "command", "hook", "tool", "mustard", "branch", "base", "repo", "skill", "name"];

/// A página da spec `spec`, com os rótulos no idioma `lang`.
#[must_use]
pub fn spec_document(spec: &str, log: &SpecLog, lang: Locale) -> Document {
    let page = Page::new(log, lang);
    Document {
        lang: lang.as_str().to_string(),
        kind: Some(page.t("page.kind.spec").to_string()),
        title: spec.to_string(),
        meta: page.meta(spec),
        body: Block::ALL.iter().map(|block| page.section(*block)).collect(),
        footer: None,
    }
}

struct Page<'a> {
    log: &'a SpecLog,
    lang: Locale,
    codes: BTreeMap<u64, String>,
    visible: Vec<&'a SpecEvent>,
}

impl<'a> Page<'a> {
    fn new(log: &'a SpecLog, lang: Locale) -> Self {
        Self { log, lang, codes: log.codes(), visible: log.visible() }
    }

    fn t(&self, key: &str) -> &'static str {
        translate(key, self.lang)
    }

    fn code(&self, id: u64) -> String {
        self.codes.get(&id).cloned().unwrap_or_else(|| id.to_string())
    }

    fn of_block(&self, block: Block) -> Vec<&'a SpecEvent> {
        self.visible.iter().copied().filter(|e| e.block() == Some(block)).collect()
    }

    fn of_type(&self, event_type: &str) -> Vec<&'a SpecEvent> {
        self.visible.iter().copied().filter(|e| e.event_type == event_type).collect()
    }

    // -----------------------------------------------------------------------
    // Cabeçalho e seções
    // -----------------------------------------------------------------------

    fn meta(&self, spec: &str) -> Vec<Meta> {
        let pair = |key: &str, value: String| Meta::Pair { label: self.t(key).to_string(), value };
        let mut meta = vec![pair("page.meta.spec", spec.to_string())];
        let states = self.of_type("state");
        if let Some(phase) = states.last().and_then(|e| e.str_field("phase")) {
            meta.push(pair("page.meta.phase", self.phase(phase)));
        }
        if let Some(branch) = states.iter().rev().find_map(|e| e.str_field("branch")) {
            meta.push(pair("page.meta.branch", branch.to_string()));
        }
        if let Some(base) = states.iter().rev().find_map(|e| e.str_field("base")) {
            meta.push(pair("page.meta.base", base.to_string()));
        }
        meta
    }

    fn section(&self, block: Block) -> Node {
        let mut body = match block {
            Block::Metrics => self.metrics(),
            Block::Agreed | Block::Specification => self.grouped(block),
            Block::Criteria => self.criteria(),
            Block::Waves => self.waves(),
            Block::Conversation => self.conversation(),
            Block::State | Block::Review | Block::Progress | Block::Notes => {
                self.items(&self.of_block(block))
            }
        };
        let shown = body.iter().filter(|n| matches!(n, Node::Item(_))).count();
        if body.is_empty() {
            body.push(Node::Paragraph(self.t("page.empty").to_string()));
        }
        let collapsed = (block == Block::Conversation)
            .then(|| self.t("page.conversation.summary").replace("{count}", &shown.to_string()));
        Node::Section(Section {
            anchor: Some(block.name().to_string()),
            heading: self.t(&format!("page.block.{}", block.name())).to_string(),
            collapsed,
            body,
        })
    }

    fn items(&self, events: &[&SpecEvent]) -> Vec<Node> {
        events.iter().map(|e| Node::Item(self.item(e, true, None))).collect()
    }

    /// Os tipos do bloco, cada um com o seu subtítulo, na ordem dos tipos.
    fn grouped(&self, block: Block) -> Vec<Node> {
        let mut out = Vec::new();
        for spec in TYPES.iter().filter(|t| t.block == block) {
            let events = self.of_type(spec.name);
            if events.is_empty() {
                continue;
            }
            out.push(self.heading(&format!("page.group.{}", spec.name)));
            out.extend(self.items(&events));
        }
        out
    }

    fn heading(&self, key: &str) -> Node {
        Node::Heading { level: 3, text: self.t(key).to_string() }
    }

    /// Os critérios, cada um com o resultado da última execução, e depois as
    /// execuções.
    fn criteria(&self) -> Vec<Node> {
        let runs = self.of_type("criterion_run");
        let mut out: Vec<Node> = self
            .of_type("criterion")
            .into_iter()
            .map(|criterion| {
                let mut item = self.item(criterion, true, None);
                let last = runs.iter().rev().find(|r| {
                    r.int("criterion").is_some_and(|id| self.code(id) == self.code(criterion.id))
                });
                if let Some(run) = last {
                    let result = run.str_field("result").map_or_else(String::new, |r| self.value_label(r));
                    item.fields.push(self.field("page.field.last_run", format!("{result} ({})", self.code(run.id))));
                }
                Node::Item(item)
            })
            .collect();
        if !runs.is_empty() {
            out.push(self.heading("page.group.criterion_run"));
            out.extend(self.items(&runs));
        }
        out
    }

    /// Uma parte por onda, em ordem de número, com a onda, as tarefas, os
    /// envios e os entregou dela; as skills vêm no fim.
    fn waves(&self) -> Vec<Node> {
        let events = self.of_block(Block::Waves);
        let numbers: BTreeSet<u64> = events.iter().filter_map(|e| e.wave()).collect();
        let mut out = Vec::new();
        for n in numbers {
            out.push(Node::Heading {
                level: 3,
                text: self.t("page.wave.heading").replace("{n}", &n.to_string()),
            });
            for event in events.iter().filter(|e| e.wave() == Some(n)) {
                let mut item = self.item(event, true, None);
                if event.event_type == "wave" {
                    item.fields.push(self.field("page.field.wave_state", self.t(self.wave_state(n)).to_string()));
                }
                out.push(Node::Item(item));
            }
        }
        let skills: Vec<&SpecEvent> = events.iter().copied().filter(|e| e.wave().is_none()).collect();
        if !skills.is_empty() {
            out.push(self.heading("page.group.skill"));
            out.extend(self.items(&skills));
        }
        out
    }

    /// Sem envio, a onda está por fazer; com envio e sem commit, em execução;
    /// com commit, pronta; com veredito aprovado, revisada.
    fn wave_state(&self, n: u64) -> &'static str {
        let reviewed = self
            .of_type("verdict")
            .iter()
            .any(|v| v.wave() == Some(n) && v.str_field("result") == Some("approved"));
        let committed = self.of_type("commit").iter().any(|c| c.ints("waves").contains(&n));
        let sent = self.of_type("send").iter().any(|s| s.wave() == Some(n));
        if reviewed {
            "page.value.wave_reviewed"
        } else if committed {
            "page.value.wave_done"
        } else if sent {
            "page.value.wave_running"
        } else {
            "page.value.wave_todo"
        }
    }

    /// O painel: uma linha por medida que tem dado.
    fn metrics(&self) -> Vec<Node> {
        let count = |events: &[&SpecEvent], field: &str, word: &str| {
            events.iter().filter(|e| e.str_field(field) == Some(word)).count().to_string()
        };
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut row = |key: &str, value: String| rows.push(vec![self.t(key).to_string(), value]);

        let calls = self.of_type("call");
        if !calls.is_empty() {
            row(
                "page.metrics.calls",
                self.t("page.metrics.calls.value")
                    .replace("{count}", &calls.len().to_string())
                    .replace("{refused}", &count(&calls, "result", "refused")),
            );
        }
        let hooks = self.of_type("hook");
        if !hooks.is_empty() {
            row(
                "page.metrics.hooks",
                self.t("page.metrics.hooks.value")
                    .replace("{blocks}", &count(&hooks, "action", "block"))
                    .replace("{warns}", &count(&hooks, "action", "warn")),
            );
        }
        let injections = self.of_type("injection");
        if !injections.is_empty() {
            let chars: u64 = injections.iter().filter_map(|e| e.int("chars")).sum();
            row(
                "page.metrics.injected",
                self.t("page.metrics.injected.value")
                    .replace("{chars}", &chars.to_string())
                    .replace("{tokens}", &(chars / 4).to_string()),
            );
        }
        let sends = self.of_type("send");
        if !sends.is_empty() {
            let largest = sends.iter().filter_map(|e| e.int("lines")).max().unwrap_or(0);
            row(
                "page.metrics.sends",
                self.t("page.metrics.sends.value")
                    .replace("{count}", &sends.len().to_string())
                    .replace("{lines}", &largest.to_string()),
            );
        }
        let verdicts = self.of_type("verdict");
        if !verdicts.is_empty() {
            row(
                "page.metrics.verdicts",
                self.t("page.metrics.verdicts.value")
                    .replace("{approved}", &count(&verdicts, "result", "approved"))
                    .replace("{rejected}", &count(&verdicts, "result", "rejected")),
            );
        }
        let points = self.of_type("point");
        if !points.is_empty() {
            let closed: BTreeSet<u64> = points.iter().filter_map(|p| p.int("closes")).collect();
            let open = points
                .iter()
                .filter(|p| p.str_field("status") == Some("open") && !closed.contains(&p.id))
                .count();
            row(
                "page.metrics.points",
                self.t("page.metrics.points.value")
                    .replace("{open}", &open.to_string())
                    .replace("{closed}", &closed.len().to_string()),
            );
        }
        if rows.is_empty() {
            return Vec::new();
        }
        vec![Node::Table(Table {
            headers: vec![
                self.t("page.metrics.col.measure").to_string(),
                self.t("page.metrics.col.value").to_string(),
            ],
            rows,
        })]
    }

    /// A conversa, em ordem de número: as mensagens, as respostas, o que os
    /// ganchos e os comandos fizeram, as remoções e, marcada como
    /// substituída, a versão antiga de cada item revisto que ainda vale.
    fn conversation(&self) -> Vec<Node> {
        let hidden = self.log.hidden();
        let mut entries: Vec<(u64, Item)> = self
            .of_block(Block::Conversation)
            .into_iter()
            .map(|e| {
                let author = e.str_field("author").map(|a| self.t(&format!("page.author.{a}")));
                (e.id, self.item(e, true, Some(self.note(e, author))))
            })
            .collect();
        for event in &self.log.events {
            let replaced = matches!(hidden.get(&event.id), Some(Hidden::Replaced { .. }));
            if replaced && self.log.current(event.id).is_some() {
                let note = self.note(event, Some(self.t("page.replaced")));
                entries.push((event.id, self.item(event, false, Some(note))));
            }
        }
        entries.sort_by_key(|(id, _)| *id);
        entries.into_iter().map(|(_, item)| Node::Item(item)).collect()
    }

    /// "mensagem · usuário · 2026-09-11 21:03".
    fn note(&self, event: &SpecEvent, who: Option<&str>) -> String {
        let mut parts = vec![self.t(&format!("page.type.{}", event.event_type)).to_string()];
        parts.extend(who.map(str::to_string));
        let at = event.at();
        if !at.is_empty() {
            parts.push(at.get(..16).unwrap_or(at).replace('T', " "));
        }
        parts.join(" · ")
    }

    // -----------------------------------------------------------------------
    // Um item e os campos dele
    // -----------------------------------------------------------------------

    fn item(&self, event: &SpecEvent, anchored: bool, note: Option<String>) -> Item {
        Item {
            code: self.code(event.id),
            anchored,
            note,
            text: event.str_field("text").unwrap_or_default().trim().to_string(),
            fields: self.fields(event),
        }
    }

    fn field(&self, key: &str, value: String) -> Field {
        Field { label: self.t(key).to_string(), value }
    }

    /// Os campos próprios do tipo, na ordem em que o tipo os declara, e
    /// depois o rótulo do rascunho e a mensagem de origem. O texto vai no
    /// item; as palavras-chave e o campo de busca nunca aparecem.
    fn fields(&self, event: &SpecEvent) -> Vec<Field> {
        let Some(spec) = type_spec(&event.event_type) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for field in spec.fields {
            if matches!(field.name, "text" | "keys") || in_the_heading(spec.name, field.name) {
                continue;
            }
            let Some(value) = event.fields.get(field.name) else {
                continue;
            };
            let shown = self.value(field.name, field.kind, value);
            if !shown.is_empty() {
                out.push(self.field(&format!("page.field.{}", field.name), shown));
            }
        }
        if let Some(label) = event.str_field("label") {
            out.push(self.field("page.field.label", label.to_string()));
        }
        if let Some(origin) = event.int("origin") {
            out.push(self.field("page.field.origin", self.code(origin)));
        }
        out
    }

    fn value(&self, name: &str, kind: Kind, value: &Value) -> String {
        if EVENT_REF.contains(&name)
            && let Some(id) = value.as_u64()
        {
            return self.code(id);
        }
        if EVENT_REFS.contains(&name) && matches!(kind, Kind::Ints | Kind::Refs) {
            let each = |v: &Value| v.as_u64().map_or_else(|| self.plain(v), |id| self.code(id));
            return join(value.as_array().into_iter().flatten().map(each));
        }
        if WAVE_NUMBERS.contains(&name) {
            return value.as_u64().map_or_else(|| join(ints(value).into_iter().map(|n| n.to_string())), |n| n.to_string());
        }
        match kind {
            Kind::OneOf(_) => {
                let word = value.as_str().unwrap_or_default();
                return if name == "phase" { self.phase(word) } else { self.value_label(word) };
            }
            Kind::ManyOf(_) => {
                return join(value.as_array().into_iter().flatten().filter_map(Value::as_str).map(|w| self.value_label(w)));
            }
            _ => {}
        }
        match name {
            "files" => join(value.as_array().into_iter().flatten().map(|f| self.file(f))),
            "facts" => value
                .as_array()
                .into_iter()
                .flatten()
                .map(|fact| {
                    let text = one_line(fact.get("text").and_then(Value::as_str).unwrap_or_default());
                    match fact.get("source").and_then(Value::as_str) {
                        Some(source) => format!("{text} ({})", code_span(source)),
                        None => text,
                    }
                })
                .collect::<Vec<_>>()
                .join("; "),
            "examples" => join(value.as_array().into_iter().flatten().map(|ex| {
                let path = ex.get("path").and_then(Value::as_str).unwrap_or_default();
                let why = one_line(ex.get("why").and_then(Value::as_str).unwrap_or_default());
                format!("{}: {why}", code_span(path))
            })),
            "skills" => join(value.as_array().into_iter().flatten().map(|s| {
                let name = s.get("name").and_then(Value::as_str).unwrap_or_default();
                let sha = s.get("sha").and_then(Value::as_str).unwrap_or_default();
                format!("{} ({})", code_span(name), code_span(sha))
            })),
            "criteria" => join(value.as_array().into_iter().flatten().map(|c| {
                let id = c.get("criterion").and_then(Value::as_u64).map_or_else(String::new, |id| self.code(id));
                let key = if c.get("tests_rule").and_then(Value::as_bool) == Some(true) {
                    "page.value.tests_rule"
                } else {
                    "page.value.not_tests_rule"
                };
                format!("{id} ({})", self.t(key))
            })),
            "lessons" if kind == Kind::Objects => join(value.as_array().into_iter().flatten().map(|l| {
                let id = l.get("lesson").map(|v| self.plain(v)).unwrap_or_default();
                let key = if l.get("repeated").and_then(Value::as_bool) == Some(true) {
                    "page.value.repeated"
                } else {
                    "page.value.not_repeated"
                };
                format!("{id} ({})", self.t(key))
            })),
            "witness" => {
                let question = value.get("question").and_then(Value::as_str).unwrap_or_default();
                let answer = value.get("answer").and_then(Value::as_str).unwrap_or_default();
                format!("{} → {}", one_line(question), one_line(answer))
            }
            "filter" => {
                let get = |k: &str| value.get(k).and_then(Value::as_str).unwrap_or_default();
                format!("{} {} → {}", self.t(&format!("page.type.{}", get("type"))), get("from"), get("to"))
            }
            "url" => value.as_str().map_or_else(String::new, |url| format!("[{url}]({url})")),
            // Um valor que já traz as próprias crases é markdown escrito por
            // quem gravou, como uma prova que cita o comando no meio da
            // frase: sai como está, senão as crases de dentro apareceriam na
            // página. Sem crase nenhuma, o valor inteiro é o código.
            _ if LITERAL.contains(&name) => value.as_str().map_or_else(
                || self.plain(value),
                |text| if text.contains('`') { one_line(text) } else { code_span(text) },
            ),
            _ => self.plain(value),
        }
    }

    /// Um arquivo de uma tarefa (`{"path", "new"}`) ou de um entregou (o
    /// caminho puro).
    fn file(&self, file: &Value) -> String {
        let path = file.as_str().or_else(|| file.get("path").and_then(Value::as_str)).unwrap_or_default();
        let new = file.get("new").and_then(Value::as_bool) == Some(true);
        if new {
            format!("{} ({})", code_span(path), self.t("page.value.new"))
        } else {
            code_span(path)
        }
    }

    /// Qualquer valor em uma linha: lista separada por vírgula, objeto como
    /// pares `chave: valor`.
    fn plain(&self, value: &Value) -> String {
        match value {
            Value::Null => String::new(),
            Value::Bool(b) => self.t(if *b { "page.value.yes" } else { "page.value.no" }).to_string(),
            Value::Number(n) => n.to_string(),
            Value::String(s) => one_line(s),
            Value::Array(items) => join(items.iter().map(|v| self.plain(v))),
            Value::Object(map) => map
                .iter()
                .map(|(k, v)| format!("{k}: {}", self.plain(v)))
                .collect::<Vec<_>>()
                .join("; "),
        }
    }

    fn phase(&self, phase: &str) -> String {
        self.t(&format!("page.phase.{phase}")).to_string()
    }

    fn value_label(&self, word: &str) -> String {
        self.t(&format!("page.value.{word}")).to_string()
    }
}

/// O número da onda já está no subtítulo da parte dela: a onda não repete o
/// próprio número, e a tarefa, o envio e o entregou não repetem a onda.
fn in_the_heading(event_type: &str, field: &str) -> bool {
    matches!((event_type, field), ("wave", "n") | ("task" | "send" | "delivered", "wave"))
}

fn ints(value: &Value) -> Vec<u64> {
    value.as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default()
}

fn join(items: impl Iterator<Item = String>) -> String {
    items.collect::<Vec<_>>().join(", ")
}

/// Um texto em uma linha só.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Um trecho de código em markdown, com crases que o texto não usa.
fn code_span(text: &str) -> String {
    let text = one_line(text);
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest + 1);
    if longest == 0 {
        format!("{fence}{text}{fence}")
    } else {
        format!("{fence} {text} {fence}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::spec_events::parse_log;
    use crate::platform::i18n::translate;

    fn line(id: u64, event_type: &str, extra: &str) -> String {
        format!("{{\"v\":1,\"id\":{id},\"at\":\"2026-09-12T10:0{}:00-03:00\",\"type\":\"{event_type}\",\"author\":\"assistant\"{extra}}}\n", id % 10)
    }

    fn sections(doc: &Document) -> Vec<&Section> {
        doc.body
            .iter()
            .map(|n| match n {
                Node::Section(s) => s,
                other => panic!("the body holds only sections: {other:?}"),
            })
            .collect()
    }

    fn items(section: &Section) -> Vec<&Item> {
        section.body.iter().filter_map(|n| if let Node::Item(i) = n { Some(i) } else { None }).collect()
    }

    /// Os dez blocos saem sempre, na ordem da página, cada um com o seu
    /// endereço; a conversa vem recolhida e um bloco vazio diz que está vazio.
    #[test]
    fn the_ten_blocks_come_out_in_page_order_even_when_empty() {
        let doc = spec_document("vazia", &parse_log(""), Locale::PtBr);
        let got: Vec<(&str, &str)> = sections(&doc)
            .iter()
            .map(|s| (s.anchor.as_deref().unwrap_or_default(), s.heading.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("state", "Estado"),
                ("metrics", "Painel de medição"),
                ("agreed", "Combinado"),
                ("specification", "Especificação"),
                ("criteria", "Critérios"),
                ("waves", "Ondas"),
                ("review", "Revisão e QA"),
                ("progress", "Andamento"),
                ("notes", "Anotações"),
                ("conversation", "Conversa"),
            ]
        );
        let all = sections(&doc);
        assert!(all.iter().all(|s| s.body == [Node::Paragraph("Nada registrado ainda.".into())]));
        assert_eq!(all[9].collapsed.as_deref(), Some("0 registros"));
        assert!(all[..9].iter().all(|s| s.collapsed.is_none()));
    }

    /// Uma decisão revista mostra só a versão nova no combinado; a antiga
    /// aparece só na conversa, marcada como substituída e sem ser o endereço
    /// do código.
    #[test]
    fn a_revised_decision_shows_the_new_text_and_the_old_one_only_in_the_conversation() {
        let content = [
            line(1, "message", ",\"author\":\"user\",\"text\":\"decida\""),
            line(2, "decision", ",\"text\":\"Texto antigo.\",\"keys\":[\"k\"],\"why\":\"w\",\"origin\":1"),
            line(3, "decision", ",\"text\":\"Texto novo.\",\"keys\":[\"k\"],\"why\":\"w\",\"origin\":1,\"replaces\":2"),
        ]
        .concat();
        let doc = spec_document("s", &parse_log(&content), Locale::PtBr);
        let all = sections(&doc);
        let agreed = items(all[2]);
        assert_eq!(agreed.len(), 1);
        assert_eq!((agreed[0].code.as_str(), agreed[0].text.as_str()), ("MSTD-DEC-0001", "Texto novo."));
        assert!(agreed[0].anchored);

        let talk = items(all[9]);
        let old = talk.iter().find(|i| i.text == "Texto antigo.").expect("the old version is in the conversation");
        assert_eq!(old.code, "MSTD-DEC-0001");
        assert!(!old.anchored, "the address belongs to the new version");
        assert!(old.note.as_deref().unwrap_or_default().contains("versão substituída"), "{old:?}");
        for section in &all[..9] {
            assert!(items(section).iter().all(|i| i.text != "Texto antigo."), "{}", section.heading);
        }
    }

    /// Um item removido some de todos os blocos; a remoção fica na conversa,
    /// com o código do que tirou e sem o texto dele.
    #[test]
    fn a_removed_item_leaves_every_block() {
        let content = [
            line(1, "rule", ",\"text\":\"Regra que sai.\",\"keys\":[\"k\"],\"example\":\"e\",\"origin\":1"),
            line(2, "rule", ",\"text\":\"Regra que fica.\",\"keys\":[\"k\"],\"example\":\"e\",\"origin\":1"),
            line(3, "remove", ",\"targets\":[1],\"reason\":\"engano\""),
        ]
        .concat();
        let doc = spec_document("s", &parse_log(&content), Locale::PtBr);
        let all = sections(&doc);
        let texts: Vec<&str> = all.iter().flat_map(|s| items(s)).map(|i| i.text.as_str()).collect();
        assert!(!texts.contains(&"Regra que sai."), "{texts:?}");
        assert!(texts.contains(&"Regra que fica."));
        let removal = items(all[9])[0];
        assert_eq!(removal.code, "MSTD-RMV-0001");
        let values: Vec<&str> = removal.fields.iter().map(|f| f.value.as_str()).collect();
        assert_eq!(values, ["engano", "MSTD-RULE-0001"]);
    }

    /// Toda referência a outro evento sai como o código dele, e o número da
    /// onda vira o subtítulo da parte dela.
    #[test]
    fn references_come_out_as_codes_and_waves_get_their_heading() {
        let content = [
            line(1, "criterion", ",\"when\":\"w\",\"then\":\"t\",\"proof\":\"cargo test\",\"origin\":1"),
            line(2, "wave", ",\"n\":1,\"text\":\"Objetivo.\",\"criteria\":[1],\"done_when\":\"d\",\"origin\":1"),
            line(3, "task", ",\"wave\":1,\"text\":\"Tarefa.\",\"files\":[{\"path\":\"a.rs\",\"new\":true}],\"origin\":1"),
            line(4, "criterion_run", ",\"criterion\":1,\"result\":\"pass\",\"exit\":0,\"ms\":5"),
        ]
        .concat();
        let doc = spec_document("s", &parse_log(&content), Locale::PtBr);
        let all = sections(&doc);
        assert_eq!(all[5].body[0], Node::Heading { level: 3, text: "Onda 1".into() });
        let wave = items(all[5])[0];
        let criteria = wave.fields.iter().find(|f| f.label == "Critérios").expect("criteria field");
        assert_eq!(criteria.value, "MSTD-CRIT-0001");
        let state = wave.fields.iter().find(|f| f.label == "Estado da onda").expect("wave state");
        assert_eq!(state.value, "a fazer");
        assert!(wave.fields.iter().all(|f| f.label != "Número"), "the number is in the heading");
        let task = items(all[5])[1];
        assert_eq!(task.fields[0].value, "`a.rs` (novo)");
        let criterion = items(all[4])[0];
        assert!(criterion.fields.iter().any(|f| f.value == "`cargo test`"));
        assert!(criterion.fields.iter().any(|f| f.value == "passou (MSTD-CRUN-0001)"), "{criterion:?}");
    }

    /// Uma tarefa sem arquivo sai sem a linha dos arquivos; a que cita
    /// arquivo continua mostrando a linha.
    #[test]
    fn a_task_without_files_shows_no_files_line() {
        let content = [
            line(1, "wave", ",\"n\":1,\"text\":\"Objetivo.\",\"criteria\":[1],\"done_when\":\"d\",\"origin\":1"),
            line(2, "task", ",\"wave\":1,\"text\":\"Medir de novo.\",\"origin\":1"),
            line(3, "task", ",\"wave\":1,\"text\":\"Mudar o leitor.\",\"files\":[{\"path\":\"a.rs\"}],\"origin\":1"),
        ]
        .concat();
        let doc = spec_document("s", &parse_log(&content), Locale::PtBr);
        let all = sections(&doc);
        let tasks: Vec<&Item> = items(all[5]).into_iter().filter(|i| i.code.starts_with("MSTD-TASK-")).collect();
        let files = |item: &Item| item.fields.iter().any(|f| f.label == "Arquivos");
        assert_eq!((files(tasks[0]), files(tasks[1])), (false, true), "{tasks:?}");
    }

    /// Uma prova que já traz o comando entre crases no meio da frase sai
    /// como foi escrita, sem crase a mais em volta; uma prova sem crase
    /// nenhuma sai inteira como código.
    #[test]
    fn a_proof_with_its_own_code_marks_comes_out_as_written() {
        let content = [
            line(1, "criterion", ",\"when\":\"w\",\"then\":\"t\",\"proof\":\"`find . -type f | wc -l` = 3\",\"origin\":1"),
            line(2, "criterion", ",\"when\":\"w\",\"then\":\"t\",\"proof\":\"cargo test\",\"origin\":1"),
        ]
        .concat();
        let doc = spec_document("s", &parse_log(&content), Locale::PtBr);
        let all = sections(&doc);
        let proofs: Vec<&str> = items(all[4])
            .iter()
            .flat_map(|i| i.fields.iter())
            .filter(|f| f.label == "Prova")
            .map(|f| f.value.as_str())
            .collect();
        assert_eq!(proofs, ["`find . -type f | wc -l` = 3", "`cargo test`"]);
    }

    /// Todo rótulo que a página usa existe nos dois idiomas: blocos, tipos,
    /// campos, valores, fases e autores.
    #[test]
    fn every_label_the_page_uses_exists_in_both_languages() {
        let mut keys: Vec<String> = Vec::new();
        for block in Block::ALL {
            keys.push(format!("page.block.{}", block.name()));
        }
        for spec in TYPES {
            keys.push(format!("page.type.{}", spec.name));
            for field in spec.fields {
                if !matches!(field.name, "text" | "keys") {
                    keys.push(format!("page.field.{}", field.name));
                }
                if let Kind::OneOf(words) | Kind::ManyOf(words) = field.kind {
                    for word in words {
                        let prefix = if field.name == "phase" { "page.phase" } else { "page.value" };
                        keys.push(format!("{prefix}.{word}"));
                    }
                }
            }
        }
        for spec in TYPES.iter().filter(|t| matches!(t.block, Block::Agreed | Block::Specification)) {
            keys.push(format!("page.group.{}", spec.name));
        }
        for author in crate::domain::spec_events::AUTHORS {
            keys.push(format!("page.author.{author}"));
        }
        for key in [
            "page.group.criterion_run",
            "page.group.skill",
            "page.field.label",
            "page.field.origin",
            "page.field.last_run",
            "page.field.wave_state",
            "page.value.yes",
            "page.value.no",
            "page.value.new",
            "page.value.tests_rule",
            "page.value.not_tests_rule",
            "page.value.repeated",
            "page.value.not_repeated",
            "page.value.wave_todo",
            "page.value.wave_running",
            "page.value.wave_done",
            "page.value.wave_reviewed",
            "page.kind.spec",
            "page.kind.project",
            "page.meta.spec",
            "page.meta.phase",
            "page.meta.branch",
            "page.meta.base",
            "page.empty",
            "page.replaced",
            "page.wave.heading",
            "page.conversation.summary",
            "page.metrics.col.measure",
            "page.metrics.col.value",
            "page.metrics.calls",
            "page.metrics.calls.value",
            "page.metrics.hooks",
            "page.metrics.hooks.value",
            "page.metrics.injected",
            "page.metrics.injected.value",
            "page.metrics.sends",
            "page.metrics.sends.value",
            "page.metrics.verdicts",
            "page.metrics.verdicts.value",
            "page.metrics.points",
            "page.metrics.points.value",
            "page.project.specs",
            "page.project.col.spec",
            "page.project.col.phase",
            "page.project.col.page",
            "page.project.open",
            "page.project.none",
            "page.project.index",
        ] {
            keys.push(key.to_string());
        }
        for key in keys {
            for lang in [Locale::PtBr, Locale::EnUs] {
                assert_ne!(translate(&key, lang), "<missing-key>", "{key} missing in {}", lang.as_str());
            }
        }
    }
}
