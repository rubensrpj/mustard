//! A página de uma spec, montada dos eventos do `spec.ndjson`.
//!
//! Os dez blocos saem sempre, na ordem aprovada: estado, painel de medição,
//! combinado, especificação, critérios, ondas, revisão e QA, andamento,
//! anotações e conversa, esta recolhida. Um bloco sem nada diz que está vazio.
//!
//! Cada onda mostra o estado dela, o commit que a fechou, o que o agente dela
//! recebe e o pedido, recolhido: o que foi enviado, com o texto exato, ou,
//! antes do envio, o que o binário montou para enviar.
//!
//! O painel de medição sai só dos eventos, mais a economia do rtk nos dias da
//! spec, que chega pronta de quem roda o rtk.
//!
//! Entre o andamento e as anotações entra "O que o plano achou", montada com
//! as anotações que a conferência do plano gravou, reconhecidas pelo rótulo
//! do achado do plano: quem vai aprovar vê o que o plano encontrou sem
//! procurar no meio das outras anotações. Sem achado nenhum, ela não aparece,
//! e a anotação sem esse rótulo continua nas anotações.
//!
//! Só aparece o que a leitura mostra: um item removido some da página, e a
//! versão antiga de um item revisto aparece só na conversa, marcada como
//! substituída. Cada item leva o seu código (`MSTD-RULE-0005`), que é também o
//! endereço dele, e toda referência a outro evento sai como o código dele.
//! Nada vem do relógio: a hora mostrada é a que cada evento gravou.
//!
//! Numa spec aprovada, o item do combinado, da especificação, dos critérios,
//! das ondas e das anotações gravado depois da aprovação que vale sai marcado
//! "depois da aprovação", com a hora dele: é o que mudou sem aprovação nova.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::{Document, Field, Item, Meta, Node, Section, Table};
use crate::domain::spec_events::{type_spec, Block, Hidden, Kind, SpecEvent, SpecLog, TYPES};
use crate::domain::spec_state::{approval_boundary, original_of};
use crate::domain::survey;
use crate::platform::i18n::{translate, Locale};

/// Os registros da execução: não mudam o que foi aprovado, e não levam a
/// marca de "depois da aprovação".
const EXECUTION_RECORDS: &[&str] = &["criterion_run", "send", "delivered"];

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

/// O pedido montado de cada onda, pelo número dela. Quem monta o pedido lê o
/// disco (o banco de lições e os arquivos das skills), então ele chega pronto:
/// a página continua sendo função só do que recebe.
pub type WavePrompts = BTreeMap<u64, String>;

/// Um dia da economia do rtk neste projeto, como o próprio rtk a conta.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RtkDay {
    /// O dia, `2026-09-11`.
    pub date: String,
    /// Quantos comandos passaram pelo rtk nesse dia.
    pub commands: u64,
    /// Os tokens que a saída dos comandos teria sem o rtk.
    pub input: u64,
    /// Os tokens que o rtk tirou da saída.
    pub saved: u64,
}

/// O que a página recebe pronto de quem lê o disco e roda o rtk.
#[derive(Debug, Clone, Copy)]
pub struct SpecInputs<'a> {
    /// O pedido montado de cada onda.
    pub prompts: &'a WavePrompts,
    /// A economia do rtk no projeto, um dia por linha.
    pub rtk: &'a [RtkDay],
}

/// A página da spec `spec`, com os rótulos no idioma `lang`, mostrando em cada
/// onda o pedido que `prompts` traz para ela. O painel sai sem a economia do
/// rtk; [`spec_page`] a recebe.
#[must_use]
pub fn spec_document(spec: &str, log: &SpecLog, prompts: &WavePrompts, lang: Locale) -> Document {
    spec_page(spec, log, SpecInputs { prompts, rtk: &[] }, lang)
}

/// A página da spec `spec` com tudo o que `inputs` traz pronto.
#[must_use]
pub fn spec_page(spec: &str, log: &SpecLog, inputs: SpecInputs<'_>, lang: Locale) -> Document {
    let page = Page::new(log, inputs, lang);
    let mut body = Vec::new();
    for block in Block::ALL {
        // O que o plano achou vem logo antes das anotações, e só quando há
        // achado.
        if block == Block::Notes
            && let Some(section) = page.findings()
        {
            body.push(section);
        }
        body.push(page.section(block));
    }
    Document {
        lang: lang.as_str().to_string(),
        kind: Some(page.t("page.kind.spec").to_string()),
        title: spec.to_string(),
        meta: page.meta(spec),
        body,
        footer: None,
    }
}

/// Quantos registros a conversa da página mostra.
#[must_use]
pub fn conversation_len(doc: &Document) -> usize {
    conversation(doc).map_or(0, |section| section.body.iter().filter(|n| matches!(n, Node::Item(_))).count())
}

/// Tira da página os `count` registros mais antigos da conversa e diz, no
/// começo dela, quantos ficaram só no `.md`. Devolve quantos saíram. A página
/// que passaria do tamanho que o claude.ai aceita é a única que perde algo.
pub fn cut_oldest_conversation(doc: &mut Document, count: usize, lang: Locale) -> usize {
    let Some(section) = conversation_mut(doc) else {
        return 0;
    };
    let mut cut = 0;
    section.body.retain(|node| {
        if cut < count && matches!(node, Node::Item(_)) {
            cut += 1;
            false
        } else {
            true
        }
    });
    if cut == 0 {
        return 0;
    }
    let left = section.body.iter().filter(|n| matches!(n, Node::Item(_))).count();
    section.collapsed = Some(translate("page.conversation.summary", lang).replace("{count}", &left.to_string()));
    section
        .body
        .insert(0, Node::Paragraph(translate("page.conversation.cut", lang).replace("{count}", &cut.to_string())));
    cut
}

fn conversation(doc: &Document) -> Option<&Section> {
    doc.body.iter().find_map(|node| match node {
        Node::Section(section) if section.anchor.as_deref() == Some(Block::Conversation.name()) => Some(section),
        _ => None,
    })
}

fn conversation_mut(doc: &mut Document) -> Option<&mut Section> {
    doc.body.iter_mut().find_map(|node| match node {
        Node::Section(section) if section.anchor.as_deref() == Some(Block::Conversation.name()) => Some(section),
        _ => None,
    })
}

/// A anotação que nasceu de um achado da conferência do plano: é a que leva o
/// rótulo do achado, escrito pelo `plan` no idioma do projeto. O rótulo é
/// reconhecido nos dois idiomas, para a página achar a anotação gravada antes
/// de o projeto trocar de idioma.
fn is_plan_finding(event: &SpecEvent) -> bool {
    event.event_type == "note"
        && event.str_field("label").is_some_and(|label| {
            [Locale::PtBr, Locale::EnUs].iter().any(|lang| translate("plan.finding.label", *lang) == label)
        })
}

struct Page<'a> {
    log: &'a SpecLog,
    prompts: &'a WavePrompts,
    rtk: &'a [RtkDay],
    lang: Locale,
    codes: BTreeMap<u64, String>,
    visible: Vec<&'a SpecEvent>,
    /// O número da aprovação que vale, a mesma que o leitor da aprovação e o
    /// aviso de crescimento das ondas leem.
    approval: Option<u64>,
}

impl<'a> Page<'a> {
    fn new(log: &'a SpecLog, inputs: SpecInputs<'a>, lang: Locale) -> Self {
        let approval = approval_boundary(log);
        Self {
            log,
            prompts: inputs.prompts,
            rtk: inputs.rtk,
            lang,
            codes: log.codes(),
            visible: log.visible(),
            approval,
        }
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
            Block::State | Block::Review | Block::Progress => self.items(&self.of_block(block)),
            // O achado do plano tem seção própria; o resto das anotações fica
            // onde sempre esteve.
            Block::Notes => {
                let rest: Vec<&SpecEvent> =
                    self.of_block(block).into_iter().filter(|e| !is_plan_finding(e)).collect();
                self.items(&rest)
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

    /// "O que o plano achou": as anotações que a conferência do plano gravou,
    /// na ordem delas. Sem achado nenhum, não há seção.
    fn findings(&self) -> Option<Node> {
        let events: Vec<&SpecEvent> =
            self.of_block(Block::Notes).into_iter().filter(|e| is_plan_finding(e)).collect();
        if events.is_empty() {
            return None;
        }
        Some(Node::Section(Section {
            anchor: Some("findings".to_string()),
            heading: self.t("page.findings.heading").to_string(),
            collapsed: None,
            body: self.items(&events),
        }))
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
    ///
    /// A onda leva o estado, o commit que a fechou e o que o agente dela
    /// recebe. O pedido vem recolhido e literal, linha por linha: cada envio
    /// mostra o texto exato que foi injetado; a onda ainda não enviada mostra
    /// o pedido que o binário montou, porque é por esta página que a spec é
    /// aprovada, e quem aprova tem de ver cada linha que o agente vai ler, as
    /// instruções fixas incluídas.
    fn waves(&self) -> Vec<Node> {
        let events = self.of_block(Block::Waves);
        let numbers: BTreeSet<u64> = events.iter().filter_map(|e| e.wave()).collect();
        let sends = self.of_type("send");
        let mut out = Vec::new();
        for n in numbers {
            out.push(Node::Heading {
                level: 3,
                text: self.t("page.wave.heading").replace("{n}", &n.to_string()),
            });
            let sent: Vec<&SpecEvent> = sends.iter().copied().filter(|s| s.wave() == Some(n)).collect();
            let last_wave_send = sent.iter().rev().find(|s| s.str_field("role") == Some("wave"));
            let prompt = last_wave_send
                .and_then(|s| s.str_field("text"))
                .or_else(|| self.prompts.get(&n).map(String::as_str));
            for event in events.iter().filter(|e| e.wave() == Some(n)) {
                let mut item = self.item(event, true, None);
                if event.event_type == "wave" {
                    item.fields.push(self.field("page.field.wave_state", self.t(self.wave_state(n)).to_string()));
                    let commits = self.wave_commits(n);
                    if !commits.is_empty() {
                        item.fields.push(self.field("page.field.wave_commit", commits));
                    }
                    if let Some(parts) = prompt.map(receives).filter(|p| !p.is_empty()) {
                        item.fields.push(self.field("page.field.wave_receives", parts));
                    }
                }
                if event.event_type == "send" {
                    // O texto enviado vem logo abaixo, recolhido.
                    let text = std::mem::take(&mut item.text);
                    let item_code = item.code.clone();
                    out.push(Node::Item(item));
                    if !text.is_empty() {
                        let role = event.str_field("role").map_or_else(String::new, |r| self.value_label(r));
                        out.push(Node::Details {
                            summary: self
                                .t("page.wave.sent")
                                .replace("{role}", &role)
                                .replace("{lines}", &count_lines(&text).to_string()),
                            body: vec![Node::Code(text)],
                            owner: Some(item_code),
                        });
                    }
                    continue;
                }
                out.push(Node::Item(item));
            }
            if sent.is_empty()
                && let Some(prompt) = self.prompts.get(&n)
            {
                let heading = self.t("page.wave.prompt").replace("{n}", &n.to_string());
                let lines = self.t("page.wave.prompt.summary").replace("{lines}", &count_lines(prompt).to_string());
                out.push(Node::Details {
                    summary: format!("{heading} · {lines}"),
                    body: vec![Node::Code(prompt.clone())],
                    owner: None,
                });
            }
        }
        let skills: Vec<&SpecEvent> = events.iter().copied().filter(|e| e.wave().is_none()).collect();
        if !skills.is_empty() {
            out.push(self.heading("page.group.skill"));
            out.extend(self.items(&skills));
        }
        out
    }

    /// Os commits que fecharam a onda `n`: o `sha` e o código de cada um.
    fn wave_commits(&self, n: u64) -> String {
        join(
            self.of_type("commit")
                .into_iter()
                .filter(|c| c.ints("waves").contains(&n))
                .map(|c| format!("{} ({})", code_span(c.str_field("sha").unwrap_or_default()), self.code(c.id))),
        )
    }

    /// Sem envio, a onda está por fazer; com envio e sem commit, em execução;
    /// com commit, pronta; com veredito aprovado, revisada.
    fn wave_state(&self, n: u64) -> &'static str {
        let reviewed = self
            .of_type("verdict")
            .iter()
            .any(|v| v.wave() == Some(n) && v.str_field("result") == Some("approved"));
        let committed = self.wave_committed(n);
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

    /// A onda `n` tem commit.
    fn wave_committed(&self, n: u64) -> bool {
        self.of_type("commit").iter().any(|c| c.ints("waves").contains(&n))
    }

    /// O painel: uma linha por medida que tem dado, e depois o tamanho do
    /// pedido de cada onda ao lado do que a revisão achou dela.
    fn metrics(&self) -> Vec<Node> {
        let count = |events: &[&SpecEvent], field: &str, word: &str| {
            events.iter().filter(|e| e.str_field(field) == Some(word)).count()
        };
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut row = |key: &str, value: String| rows.push(vec![self.t(key).to_string(), value]);

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
        let hooks = self.of_type("hook");
        if !hooks.is_empty() {
            let names: BTreeSet<&str> = hooks.iter().filter_map(|e| e.str_field("hook")).collect();
            let hook_value = |events: &[&SpecEvent]| {
                self.t("page.metrics.hooks.value")
                    .replace("{blocks}", &count(events, "action", "block").to_string())
                    .replace("{warns}", &count(events, "action", "warn").to_string())
            };
            let by_hook = names
                .into_iter()
                .map(|name| {
                    let of: Vec<&SpecEvent> =
                        hooks.iter().copied().filter(|e| e.str_field("hook") == Some(name)).collect();
                    format!("{}: {}", code_span(name), hook_value(&of))
                })
                .collect::<Vec<_>>()
                .join("; ");
            row("page.metrics.hooks", format!("{} — {by_hook}", hook_value(&hooks)));
        }
        let calls = self.of_type("call");
        if !calls.is_empty() {
            let mut by_command: BTreeMap<&str, usize> = BTreeMap::new();
            for call in &calls {
                *by_command.entry(call.str_field("command").unwrap_or("?")).or_default() += 1;
            }
            let done = self.of_type("wave").iter().filter_map(|w| w.wave()).filter(|n| self.wave_committed(*n)).count();
            row(
                "page.metrics.calls",
                format!(
                    "{} — {}",
                    self.t("page.metrics.calls.value")
                        .replace("{count}", &calls.len().to_string())
                        .replace("{refused}", &count(&calls, "result", "refused").to_string())
                        .replace("{done}", &done.to_string()),
                    join(by_command.into_iter().map(|(command, n)| format!("{} {n}", code_span(command)))),
                ),
            );
        }
        if let Some(phases) = self.phase_times() {
            row("page.metrics.phases", phases);
        }
        let verdicts = self.of_type("verdict");
        if !verdicts.is_empty() {
            let rejected: BTreeSet<u64> = verdicts
                .iter()
                .filter(|v| v.str_field("result") == Some("rejected"))
                .filter_map(|v| v.wave())
                .collect();
            let mut value = self
                .t("page.metrics.verdicts.value")
                .replace("{approved}", &count(&verdicts, "result", "approved").to_string())
                .replace("{rejected}", &count(&verdicts, "result", "rejected").to_string());
            if !rejected.is_empty() {
                value.push_str(
                    &self
                        .t("page.metrics.rework")
                        .replace("{waves}", &join(rejected.iter().map(u64::to_string))),
                );
            }
            row("page.metrics.verdicts", value);
        }
        // Os pontos contam pela mesma leitura do levantamento: o ponto revisto
        // e fechado pela primeira versão aparece fechado aqui e no `grill`.
        let points = self.of_type("point");
        if !points.is_empty() {
            let open = survey::open_points(self.log).len();
            let closed = survey::closed_points(self.log).len();
            row(
                "page.metrics.points",
                self.t("page.metrics.points.value")
                    .replace("{open}", &open.to_string())
                    .replace("{closed}", &closed.to_string()),
            );
            let reminders: usize = points
                .iter()
                .filter_map(|p| p.fields.get("reminders").and_then(Value::as_array))
                .map(Vec::len)
                .sum();
            row("page.metrics.reminders", self.t("page.metrics.reminders.value").replace("{count}", &reminders.to_string()));
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
        if let Some(rtk) = self.rtk_savings() {
            row("page.metrics.rtk", rtk);
        }
        if rows.is_empty() {
            return Vec::new();
        }
        let mut out = vec![Node::Table(Table {
            headers: vec![
                self.t("page.metrics.col.measure").to_string(),
                self.t("page.metrics.col.value").to_string(),
            ],
            rows,
        })];
        if let Some(table) = self.size_against_review(&sends, &verdicts) {
            out.push(self.heading("page.metrics.by_wave"));
            out.push(table);
        }
        out
    }

    /// Para cada onda enviada ao agente dela: as linhas do último pedido,
    /// quantas vezes a revisão a reprovou e o resultado da última revisão.
    fn size_against_review(&self, sends: &[&SpecEvent], verdicts: &[&SpecEvent]) -> Option<Node> {
        let mut last_lines: BTreeMap<u64, u64> = BTreeMap::new();
        for send in sends.iter().filter(|s| s.str_field("role") == Some("wave")) {
            if let (Some(n), Some(lines)) = (send.wave(), send.int("lines")) {
                last_lines.insert(n, lines);
            }
        }
        if last_lines.is_empty() {
            return None;
        }
        let rows = last_lines
            .into_iter()
            .map(|(n, lines)| {
                let of: Vec<&SpecEvent> = verdicts.iter().copied().filter(|v| v.wave() == Some(n)).collect();
                let rejected = of.iter().filter(|v| v.str_field("result") == Some("rejected")).count();
                let last = of
                    .last()
                    .and_then(|v| v.str_field("result"))
                    .map_or_else(|| "—".to_string(), |r| self.value_label(r));
                vec![n.to_string(), lines.to_string(), rejected.to_string(), last]
            })
            .collect();
        Some(Node::Table(Table {
            headers: ["page.metrics.col.wave", "page.metrics.col.lines", "page.metrics.col.rejected", "page.metrics.col.last"]
                .iter()
                .map(|k| self.t(k).to_string())
                .collect(),
            rows,
        }))
    }

    /// O tempo que a spec passou em cada fase, na ordem em que as fases
    /// apareceram. Cada fase vai do estado que a abriu ao estado seguinte que
    /// mudou a fase; a fase atual vai até o último evento da spec. Um estado
    /// revisto conta do lugar e da hora da primeira versão dele, como na
    /// leitura do estado.
    fn phase_times(&self) -> Option<String> {
        let mut states: Vec<(u64, &SpecEvent)> = self
            .of_type("state")
            .into_iter()
            .map(|state| (original_of(self.log, state), state))
            .collect();
        states.sort_by_key(|(first, state)| (*first, state.id));
        let mut spans: Vec<(&str, &str)> = Vec::new();
        for (first, state) in states {
            let Some(phase) = state.str_field("phase") else {
                continue;
            };
            if spans.last().is_none_or(|(current, _)| *current != phase) {
                let at = self.log.get(first).map_or_else(|| state.at(), SpecEvent::at);
                spans.push((phase, at));
            }
        }
        let end = self.log.events.last()?.at();
        let mut totals: Vec<(&str, i64)> = Vec::new();
        for (i, (phase, from)) in spans.iter().enumerate() {
            let to = spans.get(i + 1).map_or(end, |(_, at)| *at);
            let secs = seconds_between(from, to)?;
            match totals.iter_mut().find(|(p, _)| p == phase) {
                Some((_, total)) => *total += secs,
                None => totals.push((phase, secs)),
            }
        }
        (!totals.is_empty())
            .then(|| join(totals.into_iter().map(|(phase, secs)| format!("{} {}", self.phase(phase), duration(secs)))))
    }

    /// A economia do rtk nos dias em que a spec teve eventos, somada dos
    /// números que o próprio rtk dá, com o primeiro e o último dia que
    /// contaram. Sem comando nenhum nesses dias, nada.
    fn rtk_savings(&self) -> Option<String> {
        let from = self.log.events.first()?.at().get(..10)?;
        let to = self.log.events.last()?.at().get(..10)?;
        let days: Vec<&RtkDay> =
            self.rtk.iter().filter(|d| d.date.as_str() >= from && d.date.as_str() <= to).collect();
        let commands: u64 = days.iter().map(|d| d.commands).sum();
        if commands == 0 {
            return None;
        }
        let input: u64 = days.iter().map(|d| d.input).sum();
        let saved: u64 = days.iter().map(|d| d.saved).sum();
        let pct = (saved * 100 + input / 2).checked_div(input).unwrap_or(0);
        let counted: Vec<&str> = days.iter().filter(|d| d.commands > 0).map(|d| d.date.as_str()).collect();
        Some(
            self.t("page.metrics.rtk.value")
                .replace("{commands}", &commands.to_string())
                .replace("{saved}", &saved.to_string())
                .replace("{pct}", &pct.to_string())
                .replace("{from}", counted.iter().min().copied().unwrap_or(from))
                .replace("{to}", counted.iter().max().copied().unwrap_or(to)),
        )
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
        parts.extend(when(event));
        parts.join(" · ")
    }

    /// "depois da aprovação · 2026-09-12 11:06": a marca do item do
    /// combinado, da especificação, dos critérios, das ondas ou das anotações
    /// gravado depois da aprovação que vale. Os registros da execução não a
    /// levam.
    fn after_approval(&self, event: &SpecEvent) -> Option<String> {
        let approval = self.approval?;
        let marked = matches!(
            event.block(),
            Some(Block::Agreed | Block::Specification | Block::Criteria | Block::Waves | Block::Notes)
        );
        if !marked || event.id <= approval || EXECUTION_RECORDS.contains(&event.event_type.as_str()) {
            return None;
        }
        let mut parts = vec![self.t("page.after_approval").to_string()];
        parts.extend(when(event));
        Some(parts.join(" · "))
    }

    // -----------------------------------------------------------------------
    // Um item e os campos dele
    // -----------------------------------------------------------------------

    /// Um item; sem nota dada, o que mudou depois da aprovação leva a marca.
    fn item(&self, event: &SpecEvent, anchored: bool, note: Option<String>) -> Item {
        Item {
            code: self.code(event.id),
            anchored,
            note: note.or_else(|| self.after_approval(event)),
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
        // O rótulo do achado do plano já é o título da seção dele: o item não
        // o repete.
        if let Some(label) = event.str_field("label").filter(|_| !is_plan_finding(event)) {
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

/// O que o agente de uma onda recebe, lido do próprio pedido: cada parte, com
/// quantos itens ela traz, na ordem em que o pedido as escreve.
fn receives(prompt: &str) -> String {
    let mut parts: Vec<(String, usize)> = Vec::new();
    for line in prompt.lines() {
        if let Some(title) = line.strip_prefix("## ") {
            parts.push((title.trim().to_string(), 0));
        } else if line.starts_with("- ")
            && let Some((_, count)) = parts.last_mut()
        {
            *count += 1;
        }
    }
    join(parts.into_iter().filter(|(_, n)| *n > 0).map(|(title, n)| format!("{title} ({n})")))
}

/// Quantas linhas um texto tem; a última conta mesmo sem quebra no fim.
fn count_lines(text: &str) -> usize {
    text.lines().count()
}

/// A hora que o evento gravou, até o minuto: "2026-09-11 21:03".
fn when(event: &SpecEvent) -> Option<String> {
    let at = event.at();
    (!at.is_empty()).then(|| at.get(..16).unwrap_or(at).replace('T', " "))
}

/// Os segundos entre duas horas gravadas com o fuso; `None` se uma delas não
/// se lê.
fn seconds_between(from: &str, to: &str) -> Option<i64> {
    let from = chrono::DateTime::parse_from_rfc3339(from).ok()?;
    let to = chrono::DateTime::parse_from_rfc3339(to).ok()?;
    Some((to - from).num_seconds().max(0))
}

/// Uma duração curta de ler: "< 1 min", "35 min", "2 h 05 min",
/// "3 d 4 h".
fn duration(secs: i64) -> String {
    let minutes = secs / 60;
    match minutes {
        0 => "< 1 min".to_string(),
        1..=59 => format!("{minutes} min"),
        60..=1439 => format!("{} h {:02} min", minutes / 60, minutes % 60),
        _ => format!("{} d {} h", minutes / 1440, minutes % 1440 / 60),
    }
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
        let doc = spec_document("vazia", &parse_log(""), &WavePrompts::new(), Locale::PtBr);
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
        let doc = spec_document("s", &parse_log(&content), &WavePrompts::new(), Locale::PtBr);
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
        let doc = spec_document("s", &parse_log(&content), &WavePrompts::new(), Locale::PtBr);
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
        let doc = spec_document("s", &parse_log(&content), &WavePrompts::new(), Locale::PtBr);
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
        let doc = spec_document("s", &parse_log(&content), &WavePrompts::new(), Locale::PtBr);
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
        let doc = spec_document("s", &parse_log(&content), &WavePrompts::new(), Locale::PtBr);
        let all = sections(&doc);
        let proofs: Vec<&str> = items(all[4])
            .iter()
            .flat_map(|i| i.fields.iter())
            .filter(|f| f.label == "Prova")
            .map(|f| f.value.as_str())
            .collect();
        assert_eq!(proofs, ["`find . -type f | wc -l` = 3", "`cargo test`"]);
    }

    /// O que a spec ganhou depois da aprovação que vale sai marcado, com a
    /// hora do próprio item: a regra e a onda novas e o pedido. O que veio
    /// antes, os registros da execução e a conversa não levam a marca, e uma
    /// spec nunca aprovada não marca nada.
    #[test]
    fn the_page_marks_what_changed_after_the_approval_with_its_time() {
        let before_approval = [
            line(1, "message", ",\"author\":\"user\",\"text\":\"combine\""),
            line(2, "rule", ",\"text\":\"Regra antiga.\",\"keys\":[\"k\"],\"example\":\"e\",\"origin\":1"),
            line(3, "wave", ",\"n\":1,\"text\":\"Onda um.\",\"criteria\":[2],\"done_when\":\"d\",\"origin\":1"),
            line(4, "state", ",\"author\":\"binary\",\"phase\":\"plan\""),
        ]
        .concat();
        let after_approval = [
            line(5, "state", ",\"author\":\"user\",\"phase\":\"approved\",\"witness\":{\"question\":\"Aprovar?\",\"answer\":\"Aprovar\"}"),
            line(6, "rule", ",\"text\":\"Regra nova.\",\"keys\":[\"k\"],\"example\":\"e\",\"origin\":1"),
            line(7, "wave", ",\"n\":2,\"text\":\"Onda dois.\",\"criteria\":[2],\"done_when\":\"d\",\"origin\":1"),
            line(8, "send", ",\"author\":\"binary\",\"wave\":2,\"role\":\"wave\",\"lines\":1,\"chars\":1,\"items\":[7],\"mustard\":\"0.2.0\""),
            line(9, "request", ",\"text\":\"Mais uma onda.\",\"keys\":[\"k\"],\"effect\":\"new_waves\",\"origin\":1"),
        ]
        .concat();
        let notes = |content: &str, lang: Locale| -> Vec<(String, Option<String>)> {
            let doc = spec_document("s", &parse_log(content), &WavePrompts::new(), lang);
            sections(&doc)
                .into_iter()
                .filter(|s| s.anchor.as_deref() != Some("conversation"))
                .flat_map(|s| items(s).into_iter().map(|i| (i.code.clone(), i.note.clone())).collect::<Vec<_>>())
                .collect()
        };
        let marked = |got: &[(String, Option<String>)]| -> Vec<(String, String)> {
            got.iter().filter_map(|(code, note)| note.clone().map(|n| (code.clone(), n))).collect()
        };

        let approved = format!("{before_approval}{after_approval}");
        assert_eq!(
            marked(&notes(&approved, Locale::PtBr)),
            [
                ("MSTD-RULE-0002".to_string(), "depois da aprovação · 2026-09-12 10:06".to_string()),
                ("MSTD-WAVE-0002".to_string(), "depois da aprovação · 2026-09-12 10:07".to_string()),
                ("MSTD-REQ-0001".to_string(), "depois da aprovação · 2026-09-12 10:09".to_string()),
            ]
        );
        let english = marked(&notes(&approved, Locale::EnUs));
        assert_eq!(english[0].1, "after the approval · 2026-09-12 10:06");

        let doc = spec_document("s", &parse_log(&approved), &WavePrompts::new(), Locale::PtBr);
        let talk = items(sections(&doc)[9]);
        assert_eq!(talk[0].note.as_deref(), Some("mensagem · usuário · 2026-09-12 10:01"), "the conversation keeps its note");

        let never = before_approval.replace("\"phase\":\"plan\"", "\"phase\":\"survey\"") + &after_approval.replace("approved", "plan");
        assert!(marked(&notes(&never, Locale::PtBr)).is_empty(), "a spec never approved marks nothing");
    }

    /// A anotação com o rótulo do achado do plano sai numa seção só dela,
    /// logo antes das anotações; a anotação sem o rótulo continua nas
    /// anotações. Sem achado nenhum, a seção não aparece.
    #[test]
    fn what_the_plan_found_gets_its_own_section_right_before_the_notes() {
        let found = ",\"label\":\"achado do plano\"";
        let content = [
            line(1, "message", ",\"author\":\"user\",\"text\":\"combine\""),
            line(2, "note", ",\"text\":\"Anotação de sempre.\",\"keys\":[\"k\"],\"origin\":1"),
            line(3, "note", &format!(",\"author\":\"binary\",\"text\":\"A tarefa não diz o arquivo.\",\"keys\":[\"k\"],\"origin\":1{found}")),
        ]
        .concat();
        let doc = spec_document("s", &parse_log(&content), &WavePrompts::new(), Locale::PtBr);
        let all = sections(&doc);
        let named: Vec<(&str, &str)> = all
            .iter()
            .map(|s| (s.anchor.as_deref().unwrap_or_default(), s.heading.as_str()))
            .collect();
        assert_eq!(named[8], ("findings", "O que o plano achou"), "{named:?}");
        assert_eq!(named[9], ("notes", "Anotações"), "a seção vem logo antes das anotações");
        let texts = |section: &Section| -> Vec<String> {
            items(section).iter().map(|i| i.text.clone()).collect()
        };
        assert_eq!(texts(all[8]), ["A tarefa não diz o arquivo."]);
        assert_eq!(texts(all[9]), ["Anotação de sempre."]);
        let found = items(all[8])[0];
        assert!(
            found.fields.iter().all(|f| f.value != "achado do plano"),
            "o rótulo já é o título da seção: {found:?}"
        );

        // Em inglês a seção sai com o título de lá, e a mesma anotação nela.
        let english = spec_document("s", &parse_log(&content), &WavePrompts::new(), Locale::EnUs);
        assert_eq!(sections(&english)[8].heading, "What the plan found");

        // Sem achado nenhum, a página volta a ter dez seções.
        let only_notes = [line(1, "message", ",\"author\":\"user\",\"text\":\"combine\""), line(2, "note", ",\"text\":\"Anotação de sempre.\",\"keys\":[\"k\"],\"origin\":1")].concat();
        let doc = spec_document("s", &parse_log(&only_notes), &WavePrompts::new(), Locale::PtBr);
        assert_eq!(sections(&doc).len(), 10, "sem achado, nenhuma seção a mais");
    }

    /// Numa aprovação revista (a spec que nasceu aprovada e ganhou a branch
    /// depois), a marca parte da primeira versão da aprovação: a regra
    /// gravada entre ela e a revisão continua marcada.
    #[test]
    fn a_revised_approval_keeps_marking_from_its_first_version() {
        let witness = ",\"witness\":{\"question\":\"Aprovar?\",\"answer\":\"Aprovar\"}";
        let content = [
            line(1, "message", ",\"author\":\"user\",\"text\":\"combine\""),
            line(2, "state", &format!(",\"author\":\"user\",\"phase\":\"approved\"{witness}")),
            line(3, "rule", ",\"text\":\"Entre as duas.\",\"keys\":[\"k\"],\"example\":\"e\",\"origin\":1"),
            line(4, "state", &format!(",\"author\":\"binary\",\"phase\":\"approved\",\"branch\":\"feature/x\",\"replaces\":2{witness}")),
            line(5, "rule", ",\"text\":\"Depois da revisão.\",\"keys\":[\"k\"],\"example\":\"e\",\"origin\":1"),
        ]
        .concat();
        let doc = spec_document("s", &parse_log(&content), &WavePrompts::new(), Locale::PtBr);
        let rules: Vec<(&str, Option<&str>)> =
            items(sections(&doc)[2]).iter().map(|i| (i.text.as_str(), i.note.as_deref())).collect();
        assert_eq!(
            rules,
            [
                ("Entre as duas.", Some("depois da aprovação · 2026-09-12 10:03")),
                ("Depois da revisão.", Some("depois da aprovação · 2026-09-12 10:05")),
            ]
        );
    }

    fn field<'a>(item: &'a Item, label: &str) -> Option<&'a str> {
        item.fields.iter().find(|f| f.label == label).map(|f| f.value.as_str())
    }

    /// Cada onda mostra o estado, o commit que a fechou e o que o agente
    /// recebe, lido do próprio pedido. A onda enviada traz o texto exato do
    /// envio recolhido logo abaixo dele; a que ainda não foi enviada traz,
    /// recolhido, o pedido que o binário montou.
    #[test]
    fn each_wave_shows_its_state_commit_what_it_receives_and_the_request() {
        let sent = "# s — onda 1\n\n**O que é isto.** A lista.\n\n## Especificação\n\n- MSTD-CTX-0001 (contexto) — `ler`\n\n## Critérios\n\n- MSTD-CRIT-0001 (critério) — `ler`\n- MSTD-CRIT-0002 (critério) — `ler`\n";
        let content = [
            line(1, "message", ",\"author\":\"user\",\"text\":\"combine\""),
            line(2, "wave", ",\"n\":1,\"text\":\"Um.\",\"criteria\":[1],\"done_when\":\"d\",\"origin\":1"),
            line(3, "wave", ",\"n\":2,\"text\":\"Dois.\",\"criteria\":[1],\"done_when\":\"d\",\"origin\":1"),
            line(4, "send", &format!(",\"author\":\"binary\",\"wave\":1,\"role\":\"wave\",\"text\":{},\"lines\":12,\"chars\":200,\"items\":[2],\"mustard\":\"0.2.0\"", serde_json::to_string(sent).unwrap())),
            line(5, "commit", ",\"author\":\"binary\",\"sha\":\"5e0c7a91\",\"title\":\"t\",\"waves\":[1],\"files\":[\"a.rs\"],\"repo\":\".\""),
        ]
        .concat();
        let mut prompts = WavePrompts::new();
        prompts.insert(1, "não aparece: a onda já foi enviada".into());
        prompts.insert(2, "# s — onda 2\n\n## Combinado\n\n- MSTD-RULE-0001 (regra) — `ler`\n".into());
        let doc = spec_document("s", &parse_log(&content), &prompts, Locale::PtBr);
        let waves = sections(&doc)[5];

        let one = items(waves).into_iter().find(|i| i.code == "MSTD-WAVE-0001").unwrap();
        assert_eq!(field(one, "Estado da onda"), Some("pronta"));
        assert_eq!(field(one, "Commit"), Some("`5e0c7a91` (MSTD-COMMIT-0001)"));
        assert_eq!(field(one, "Recebe"), Some("Especificação (1), Critérios (2)"), "{one:?}");
        let send = items(waves).into_iter().find(|i| i.code == "MSTD-SEND-0001").unwrap();
        assert!(send.text.is_empty(), "the sent text goes in the collapsed part");
        let position = waves.body.iter().position(|n| matches!(n, Node::Item(i) if i.code == "MSTD-SEND-0001")).unwrap();
        assert_eq!(
            waves.body[position + 1],
            Node::Details {
                summary: "Pedido enviado (agente de onda) · 12 linhas, como o agente o recebeu".into(),
                body: vec![Node::Code(sent.trim().into())],
                owner: Some("MSTD-SEND-0001".into()),
            }
        );
        assert!(
            !waves.body.iter().any(|n| matches!(n, Node::Details { body, .. } if body == &[Node::Code("não aparece: a onda já foi enviada".into())])),
            "a sent wave does not show the assembled request again"
        );

        let two = items(waves).into_iter().find(|i| i.code == "MSTD-WAVE-0002").unwrap();
        assert_eq!(field(two, "Estado da onda"), Some("a fazer"));
        assert_eq!(field(two, "Commit"), None);
        assert_eq!(field(two, "Recebe"), Some("Combinado (1)"));
        let prompt = waves.body.last().unwrap();
        let Node::Details { summary, body, owner } = prompt else { panic!("{prompt:?}") };
        assert_eq!(owner, &None, "the assembled request belongs to no single item");
        assert_eq!(summary, "O pedido da onda 2 · 5 linhas, como o agente as recebe");
        assert_eq!(body, &[Node::Code(prompts[&2].clone())]);
    }

    /// O painel sai dos eventos: o texto colocado, os bloqueios de cada
    /// gancho, as chamadas dos passos contra as ondas prontas, o tempo de cada
    /// fase, o retrabalho, os lembretes, o tamanho de cada pedido contra a
    /// revisão e a economia do rtk nos dias da spec.
    #[test]
    fn the_panel_measures_hooks_steps_phases_rework_reminders_sizes_and_rtk() {
        let at = |id: u64, when: &str, event_type: &str, extra: &str| {
            format!("{{\"v\":1,\"id\":{id},\"at\":\"{when}\",\"type\":\"{event_type}\",\"author\":\"binary\"{extra}}}\n")
        };
        let content = [
            at(1, "2026-09-11T08:00:00-03:00", "state", ",\"phase\":\"survey\""),
            at(2, "2026-09-11T08:00:30-03:00", "injection", ",\"hook\":\"session_start_inject\",\"chars\":400,\"text\":\"t\""),
            at(3, "2026-09-11T08:01:00-03:00", "hook", ",\"hook\":\"command_guard\",\"action\":\"block\",\"tool\":\"Bash\",\"reason\":\"r\""),
            at(4, "2026-09-11T08:02:00-03:00", "hook", ",\"hook\":\"command_guard\",\"action\":\"block\",\"tool\":\"Bash\",\"reason\":\"r\""),
            at(5, "2026-09-11T08:03:00-03:00", "hook", ",\"hook\":\"write_gate\",\"action\":\"warn\",\"tool\":\"Edit\",\"reason\":\"r\""),
            at(6, "2026-09-11T08:04:00-03:00", "point", ",\"author\":\"assistant\",\"block\":\"b\",\"gap\":\"g\",\"from\":\"gap\",\"status\":\"open\",\"facts\":[{\"text\":\"f\",\"source\":\"a.rs:1\"}],\"reminders\":[1,2],\"origin\":1"),
            at(7, "2026-09-11T08:05:00-03:00", "call", ",\"command\":\"grill\",\"ms\":5,\"result\":\"ok\""),
            at(8, "2026-09-11T10:05:00-03:00", "state", ",\"phase\":\"plan\""),
            at(9, "2026-09-11T10:06:00-03:00", "call", ",\"command\":\"plan\",\"ms\":5,\"result\":\"refused\""),
            at(10, "2026-09-11T10:35:00-03:00", "call", ",\"command\":\"plan\",\"ms\":5,\"result\":\"ok\""),
            at(11, "2026-09-11T10:35:00-03:00", "wave", ",\"author\":\"assistant\",\"n\":1,\"text\":\"w\",\"criteria\":[1],\"done_when\":\"d\",\"origin\":1"),
            at(12, "2026-09-11T10:36:00-03:00", "state", ",\"author\":\"user\",\"phase\":\"approved\",\"witness\":{\"question\":\"q\",\"answer\":\"a\"}"),
            at(13, "2026-09-11T10:36:00-03:00", "state", ",\"phase\":\"running\""),
            at(14, "2026-09-11T10:37:00-03:00", "send", ",\"wave\":1,\"role\":\"wave\",\"text\":\"p\",\"lines\":300,\"chars\":9,\"items\":[11],\"mustard\":\"0\""),
            at(15, "2026-09-12T11:00:00-03:00", "verdict", ",\"author\":\"review\",\"wave\":1,\"result\":\"rejected\",\"text\":\"x\",\"criteria\":[]"),
            at(16, "2026-09-12T11:30:00-03:00", "send", ",\"wave\":1,\"role\":\"wave\",\"text\":\"p\",\"lines\":320,\"chars\":9,\"items\":[11],\"mustard\":\"0\""),
            at(17, "2026-09-12T12:00:00-03:00", "commit", ",\"sha\":\"abc\",\"title\":\"t\",\"waves\":[1],\"files\":[],\"repo\":\".\""),
            at(18, "2026-09-12T13:00:00-03:00", "verdict", ",\"author\":\"review\",\"wave\":1,\"result\":\"approved\",\"text\":\"x\",\"criteria\":[]"),
            at(19, "2026-09-13T12:36:00-03:00", "call", ",\"command\":\"round\",\"ms\":5,\"result\":\"ok\""),
        ]
        .concat();
        let day = |date: &str, commands: u64, input: u64, saved: u64| RtkDay { date: date.into(), commands, input, saved };
        let rtk = [
            day("2026-09-10", 99, 9_999, 9_999),
            day("2026-09-11", 3, 1_000, 300),
            day("2026-09-13", 1, 1_000, 100),
            day("2026-09-14", 99, 9_999, 9_999),
        ];
        let prompts = WavePrompts::new();
        let doc = spec_page("s", &parse_log(&content), SpecInputs { prompts: &prompts, rtk: &rtk }, Locale::PtBr);
        let panel = sections(&doc)[1];
        let Node::Table(table) = &panel.body[0] else { panic!("{panel:?}") };
        let rows: BTreeMap<&str, &str> = table.rows.iter().map(|r| (r[0].as_str(), r[1].as_str())).collect();
        assert_eq!(rows["Texto colocado pelos ganchos"], "400 caracteres, cerca de 100 tokens");
        assert_eq!(
            rows["Bloqueios por gancho"],
            "2 bloqueios, 1 avisos — `command_guard`: 2 bloqueios, 0 avisos; `write_gate`: 0 bloqueios, 1 avisos"
        );
        assert_eq!(
            rows["Passos do fluxo contra trabalho"],
            "4 chamadas, 1 recusadas, para 1 ondas prontas — `grill` 1, `plan` 2, `round` 1"
        );
        assert_eq!(rows["Tempo por fase"], "levantamento 2 h 05 min, plano 31 min, aprovada < 1 min, em execução 2 d 2 h");
        assert_eq!(rows["Revisões"], "1 aprovadas, 1 reprovadas; voltaram da revisão as ondas 1");
        assert_eq!(rows["Lembretes que apareceram"], "2 mensagens antigas lembradas nos pontos");
        assert_eq!(rows["Pedidos enviados aos agentes"], "2, o maior com 320 linhas");
        assert_eq!(rows["Economia do rtk"], "4 comandos, 400 tokens a menos na saída (20%), de 2026-09-11 a 2026-09-13");
        assert_eq!(panel.body[1], Node::Heading { level: 3, text: "Tamanho do pedido e revisão, por onda".into() });
        let Node::Table(by_wave) = &panel.body[2] else { panic!("{panel:?}") };
        assert_eq!(by_wave.headers, ["Onda", "Linhas do pedido", "Reprovações", "Última revisão"]);
        assert_eq!(by_wave.rows, [["1", "320", "1", "aprovada"]]);

        // Sem comando do rtk nos dias da spec, a linha não aparece.
        let quiet = [day("2026-09-10", 5, 10, 1)];
        let doc = spec_page("s", &parse_log(&content), SpecInputs { prompts: &prompts, rtk: &quiet }, Locale::PtBr);
        let Node::Table(table) = &sections(&doc)[1].body[0] else { panic!() };
        assert!(table.rows.iter().all(|r| r[0] != "Economia do rtk"), "{table:?}");
    }

    /// Um estado revisto depois conta do lugar e da hora da primeira versão:
    /// corrigir a branch do primeiro estado não muda o tempo das fases.
    #[test]
    fn a_revised_state_keeps_the_time_of_its_first_version() {
        let at = |id: u64, when: &str, extra: &str| {
            format!("{{\"v\":1,\"id\":{id},\"at\":\"2026-09-11T{when}:00-03:00\",\"type\":\"state\",\"author\":\"binary\"{extra}}}\n")
        };
        let content = [
            at(1, "08:00", ",\"phase\":\"survey\""),
            at(2, "09:00", ",\"phase\":\"plan\""),
            at(3, "09:30", ",\"phase\":\"survey\",\"branch\":\"feature/x\",\"replaces\":1"),
        ]
        .concat();
        let doc = spec_document("s", &parse_log(&content), &WavePrompts::new(), Locale::PtBr);
        let Node::Table(table) = &sections(&doc)[1].body[0] else { panic!() };
        let phases = table.rows.iter().find(|r| r[0] == "Tempo por fase").map(|r| r[1].as_str());
        assert_eq!(phases, Some("levantamento 1 h 00 min, plano 30 min"));
    }

    /// Cortar a conversa tira os registros mais antigos, diz quantos ficaram
    /// só no `.md` e acerta a contagem do resumo; o resto da página fica igual.
    #[test]
    fn cutting_the_conversation_drops_the_oldest_entries_and_says_how_many() {
        let content = (1..=5)
            .map(|id| line(id, "message", &format!(",\"author\":\"user\",\"text\":\"mensagem {id}\"")))
            .collect::<String>();
        let full = spec_document("s", &parse_log(&content), &WavePrompts::new(), Locale::PtBr);
        assert_eq!(conversation_len(&full), 5);
        let mut doc = full.clone();
        assert_eq!(cut_oldest_conversation(&mut doc, 2, Locale::PtBr), 2);
        assert_eq!(conversation_len(&doc), 3);
        let talk = sections(&doc)[9];
        assert_eq!(talk.collapsed.as_deref(), Some("3 registros"));
        let Node::Paragraph(said) = &talk.body[0] else { panic!("{talk:?}") };
        assert!(said.starts_with("Os 2 registros mais antigos da conversa ficaram só no `spec.md`"), "{said}");
        let texts: Vec<&str> = items(talk).iter().map(|i| i.text.as_str()).collect();
        assert_eq!(texts, ["mensagem 3", "mensagem 4", "mensagem 5"]);
        assert_eq!(doc.body[..9], full.body[..9], "only the conversation changes");
        let mut none = full.clone();
        assert_eq!(cut_oldest_conversation(&mut none, 0, Locale::PtBr), 0);
        assert_eq!(none, full);
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
            "page.meta.spec",
            "page.meta.phase",
            "page.meta.branch",
            "page.meta.base",
            "page.empty",
            "page.replaced",
            "page.after_approval",
            "page.wave.heading",
            "page.findings.heading",
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
            "page.metrics.phases",
            "page.metrics.rework",
            "page.metrics.reminders",
            "page.metrics.reminders.value",
            "page.metrics.rtk",
            "page.metrics.rtk.value",
            "page.metrics.by_wave",
            "page.metrics.col.wave",
            "page.metrics.col.lines",
            "page.metrics.col.rejected",
            "page.metrics.col.last",
            "page.field.wave_commit",
            "page.field.wave_receives",
            "page.wave.prompt",
            "page.wave.prompt.summary",
            "page.wave.sent",
            "page.conversation.cut",
            "page.purge_pending",
            "page.withheld_found",
            "page.too_big",
            "project.kind",
            "project.specs",
            "project.stages",
            "project.no_phase",
            "project.col.spec",
            "project.col.state",
            "project.col.branch",
            "project.col.goal",
            "project.col.created",
            "project.col.updated",
            "project.stalled",
            "project.stalled.line",
            "project.titles",
            "project.titles.summary",
            "project.meta.specs",
            "project.meta.today",
            "project.footer",
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
