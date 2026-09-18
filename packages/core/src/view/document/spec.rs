//! A página de uma spec, montada dos eventos do `spec.ndjson`.
//!
//! As seções saem sempre, na ordem aprovada: andamento, especificação,
//! combinado, critérios, ondas, revisão e QA, o que o plano achou (só quando
//! há achado), anotações e conversa. Cada seção junta os itens em grupos
//! recolhidos; uma seção sem nada diz que está vazia.
//!
//! - O andamento abre com a visão das ondas, cada uma com o estado dela, e
//!   traz a medição (o único grupo aberto), as fases e publicações e os
//!   commits.
//! - A especificação e o combinado têm um grupo por tipo; os critérios, um
//!   para os critérios e outro para as execuções.
//! - As ondas e a revisão têm um grupo por onda; o que o plano achou, um por
//!   assunto; a conversa, um por dia.
//!
//! O estado de cada onda chega pronto, lido por quem decide o que a rodada
//! despacha: a página não tem regra própria para ele. Cada onda mostra o
//! estado, o commit que a fechou, o que o agente dela recebe e o pedido,
//! recolhido: o que foi enviado, com o texto exato, ou, antes do envio, o
//! que o binário montou para enviar.
//!
//! O painel de medição sai só dos eventos, mais a economia do rtk nos dias da
//! spec, que chega pronta de quem roda o rtk.
//!
//! "O que o plano achou" junta as anotações que a conferência do plano
//! gravou, reconhecidas pelo rótulo do achado do plano: quem vai aprovar vê o
//! que o plano encontrou sem procurar no meio das outras anotações. Sem
//! achado nenhum, ela não aparece, e a anotação sem esse rótulo continua nas
//! anotações.
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
//!
//! ## Onde cada assunto mora
//!
//! Esta porta guarda o que as seções dividem e o que sai para fora: o que a
//! página recebe pronto, a montagem na ordem aprovada, a leitura dos eventos,
//! o cabeçalho, a seção e o grupo, e os pedaços de texto que todas usam. Cada
//! assunto mora numa parte da pasta ao lado: o andamento e o painel de
//! medição (`progress`), os blocos que saem um grupo por tipo (`blocks`), as
//! ondas e a revisão (`waves`), o que o plano achou e as anotações
//! (`findings`), a conversa (`conversation`), um item e os campos dele
//! (`item`) e a lista dos itens sem dono (`owners`).

mod blocks;
mod conversation;
mod findings;
mod item;
mod owners;
mod progress;
mod waves;

use std::collections::BTreeMap;

use super::{Document, Group, Meta, Node, Section, Tone};
use crate::domain::spec_events::{Block, SpecEvent, SpecLog};
use crate::domain::spec_state::approval_boundary;
use crate::platform::i18n::{translate, Locale};

pub use conversation::{conversation_len, cut_oldest_conversation};
pub use owners::{owner_label, owner_rule_key, owners_page};

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

/// Em que pé está uma onda, pela leitura que decide o que a rodada despacha.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaveState {
    /// Nada saiu ainda, ou o que saiu não vale mais.
    Todo,
    /// O pedido saiu e a entrega ainda não voltou.
    Running,
    /// A entrega voltou e espera a revisão.
    Delivered,
    /// Entregue e aprovada.
    Approved,
    /// A última revisão reprovou.
    Rejected,
}

impl WaveState {
    /// A chave do nome do estado no catálogo de textos.
    fn key(self) -> &'static str {
        match self {
            Self::Todo => "page.value.wave_todo",
            Self::Running => "page.value.wave_running",
            Self::Delivered => "page.value.wave_delivered",
            Self::Approved => "page.value.wave_approved",
            Self::Rejected => "page.value.wave_rejected",
        }
    }

    fn tone(self) -> Tone {
        match self {
            Self::Todo => Tone::Todo,
            Self::Running => Tone::Running,
            Self::Delivered => Tone::Done,
            Self::Approved => Tone::Good,
            Self::Rejected => Tone::Bad,
        }
    }
}

/// O estado de cada onda, pelo número dela. A onda que não está aqui está
/// por fazer.
pub type WaveStates = BTreeMap<u64, WaveState>;

/// O que a página recebe pronto de quem lê o disco e roda o rtk.
#[derive(Debug, Clone, Copy)]
pub struct SpecInputs<'a> {
    /// O pedido montado de cada onda.
    pub prompts: &'a WavePrompts,
    /// A economia do rtk no projeto, um dia por linha.
    pub rtk: &'a [RtkDay],
    /// O estado de cada onda.
    pub waves: &'a WaveStates,
}

/// A página da spec `spec`, com os rótulos no idioma `lang`, mostrando em cada
/// onda o pedido que `prompts` traz para ela. O painel sai sem a economia do
/// rtk e toda onda sai por fazer; [`spec_page`] recebe os dois.
#[must_use]
pub fn spec_document(spec: &str, log: &SpecLog, prompts: &WavePrompts, lang: Locale) -> Document {
    spec_page(spec, log, SpecInputs { prompts, rtk: &[], waves: &WaveStates::new() }, lang)
}

/// A página da spec `spec` com tudo o que `inputs` traz pronto.
#[must_use]
pub fn spec_page(spec: &str, log: &SpecLog, inputs: SpecInputs<'_>, lang: Locale) -> Document {
    let page = Page::new(log, inputs, lang);
    let mut body = vec![
        page.progress(),
        page.by_type(Block::Specification),
        page.by_type(Block::Agreed),
        page.criteria(),
        page.waves(),
        page.review(),
    ];
    // O que o plano achou vem logo antes das anotações, e só quando há achado.
    body.extend(page.findings());
    body.push(page.notes());
    body.push(page.conversation());
    Document {
        lang: lang.as_str().to_string(),
        kind: Some(page.t("page.kind.spec").to_string()),
        title: spec.to_string(),
        meta: page.meta(spec),
        body,
        footer: None,
    }
}

struct Page<'a> {
    log: &'a SpecLog,
    prompts: &'a WavePrompts,
    rtk: &'a [RtkDay],
    waves: &'a WaveStates,
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
            waves: inputs.waves,
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

    /// Uma seção da página; sem bloco nenhum, ela diz que está vazia.
    fn section(&self, anchor: &str, heading: &str, mut body: Vec<Node>) -> Node {
        if body.is_empty() {
            body.push(Node::Paragraph(self.t("page.empty").to_string()));
        }
        Node::Section(Section { anchor: Some(anchor.to_string()), heading: heading.to_string(), body })
    }

    /// Um grupo recolhido com os itens de `events`; sem item, nenhum grupo.
    fn group(&self, anchor: String, title: String, events: &[&SpecEvent]) -> Option<Node> {
        (!events.is_empty()).then(|| group_of(anchor, title, self.items(events)))
    }

    /// Como [`Self::group`], resumido pela conta das situações dos itens, na
    /// ordem em que cada uma aparece: é o resumo dos grupos por tipo e dos da
    /// revisão.
    fn tallied(&self, anchor: String, title: String, events: &[&SpecEvent]) -> Option<Node> {
        let mut node = self.group(anchor, title, events)?;
        if let Node::Group(group) = &mut node {
            let mut tally: Vec<(&str, usize)> = Vec::new();
            for status in Node::items(&group.body).into_iter().filter_map(|item| item.status.as_ref()) {
                match tally.iter_mut().find(|(label, _)| *label == status.label) {
                    Some((_, n)) => *n += 1,
                    None => tally.push((&status.label, 1)),
                }
            }
            let summary = tally.iter().map(|(label, n)| format!("{n} {label}")).collect::<Vec<_>>().join(" · ");
            group.summary = summary;
        }
        Some(node)
    }

    fn items(&self, events: &[&SpecEvent]) -> Vec<Node> {
        events.iter().map(|e| Node::Item(self.item(e, true))).collect()
    }
}

/// O primeiro parágrafo de um texto em markdown, em uma linha, sem a marca de
/// título nem a de item de lista: é o título da linha recolhida.
fn first_paragraph(text: &str) -> String {
    let first = one_line(text.trim().split("\n\n").next().unwrap_or_default());
    let line = first.trim_start_matches('#').trim_start();
    line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")).unwrap_or(line).to_string()
}

/// Um grupo recolhido com os blocos `body`, sem resumo.
fn group_of(anchor: String, title: String, body: Vec<Node>) -> Node {
    Node::Group(Group { anchor, title, status: None, summary: String::new(), open: false, body })
}

/// O endereço do grupo da onda `n` numa seção: `waves-3`.
fn wave_anchor(section: &str, n: u64) -> String {
    format!("{section}-{n}")
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
    use std::path::Path;

    use super::findings::SUBJECTS;
    use super::*;
    use crate::domain::spec_events::{parse_log, Kind, TYPES};
    use crate::platform::i18n::translate;
    use crate::view::document::Item;

    pub(super) fn line(id: u64, event_type: &str, extra: &str) -> String {
        format!("{{\"v\":1,\"id\":{id},\"at\":\"2026-09-12T10:0{}:00-03:00\",\"type\":\"{event_type}\",\"author\":\"assistant\"{extra}}}\n", id % 10)
    }

    pub(super) fn sections(doc: &Document) -> Vec<&Section> {
        doc.body
            .iter()
            .map(|n| match n {
                Node::Section(s) => s,
                other => panic!("the body holds only sections: {other:?}"),
            })
            .collect()
    }

    /// A seção de endereço `anchor`.
    pub(super) fn section<'a>(doc: &'a Document, anchor: &str) -> &'a Section {
        sections(doc)
            .into_iter()
            .find(|s| s.anchor.as_deref() == Some(anchor))
            .unwrap_or_else(|| panic!("no {anchor} section"))
    }

    /// Os itens da seção, também os de dentro dos grupos.
    pub(super) fn items(section: &Section) -> Vec<&Item> {
        Node::items(&section.body)
    }

    /// Os grupos da seção, na ordem.
    pub(super) fn groups(section: &Section) -> Vec<&Group> {
        section.body.iter().filter_map(|n| if let Node::Group(g) = n { Some(g) } else { None }).collect()
    }

    pub(super) fn group<'a>(section: &'a Section, anchor: &str) -> &'a Group {
        groups(section).into_iter().find(|g| g.anchor == anchor).unwrap_or_else(|| panic!("no {anchor} group"))
    }

    /// As seções saem sempre, na ordem da página, cada uma com o seu
    /// endereço, e uma seção vazia diz que está vazia.
    #[test]
    fn the_sections_come_out_in_page_order_even_when_empty() {
        let doc = spec_document("vazia", &parse_log(""), &WavePrompts::new(), Locale::PtBr);
        let got: Vec<(&str, &str)> = sections(&doc)
            .iter()
            .map(|s| (s.anchor.as_deref().unwrap_or_default(), s.heading.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("progress", "Andamento"),
                ("specification", "Especificação"),
                ("agreed", "Combinado"),
                ("criteria", "Critérios"),
                ("waves", "Ondas"),
                ("review", "Revisão e QA"),
                ("notes", "Anotações"),
                ("conversation", "Conversa"),
            ]
        );
        assert!(sections(&doc).iter().all(|s| s.body == [Node::Paragraph("Nada registrado ainda.".into())]));
    }

    /// Cada seção junta os itens em grupos recolhidos: o combinado e a
    /// especificação por tipo, os critérios e as execuções, a revisão por
    /// onda, as anotações num grupo só; o andamento traz a medição aberta, as
    /// fases e publicações e os commits. Cada grupo resume as situações dos
    /// itens dele.
    #[test]
    fn each_section_groups_its_items_and_only_the_measurement_opens() {
        let content = [
            line(1, "state", ",\"author\":\"binary\",\"phase\":\"survey\""),
            line(2, "message", ",\"author\":\"user\",\"text\":\"combine\""),
            line(3, "rule", ",\"text\":\"Regra.\",\"keys\":[\"k\"],\"example\":\"e\",\"origin\":2"),
            line(4, "decision", ",\"text\":\"Decisão.\",\"keys\":[\"k\"],\"why\":\"w\",\"origin\":2"),
            line(5, "context", ",\"text\":\"Contexto.\",\"origin\":2"),
            line(6, "criterion", ",\"when\":\"w\",\"then\":\"t\",\"proof\":\"p\",\"origin\":2"),
            line(7, "criterion_run", ",\"author\":\"binary\",\"criterion\":6,\"result\":\"fail\",\"exit\":1,\"ms\":5"),
            line(8, "verdict", ",\"author\":\"review\",\"wave\":3,\"result\":\"rejected\",\"text\":\"Faltou.\",\"criteria\":[]"),
            line(9, "verdict", ",\"author\":\"review\",\"wave\":3,\"result\":\"approved\",\"text\":\"Pronto.\",\"criteria\":[]"),
            line(10, "commit", ",\"author\":\"binary\",\"sha\":\"abc\",\"title\":\"t\",\"waves\":[3],\"files\":[],\"repo\":\".\""),
            line(11, "note", ",\"text\":\"Anotação.\",\"keys\":[\"k\"],\"origin\":2"),
            line(12, "call", ",\"author\":\"binary\",\"command\":\"grill\",\"ms\":5,\"result\":\"ok\""),
            line(13, "publish", ",\"page\":\"spec\",\"milestone\":\"approval\",\"ok\":true,\"url\":\"https://claude.ai/code/artifact/x\""),
        ]
        .concat();
        let doc = spec_document("s", &parse_log(&content), &WavePrompts::new(), Locale::PtBr);
        let anchors = |s: &Section| groups(s).iter().map(|g| (g.anchor.clone(), g.title.clone())).collect::<Vec<_>>();
        let pair = |a: &str, t: &str| (a.to_string(), t.to_string());
        assert_eq!(
            anchors(section(&doc, "progress")),
            [
                pair("progress-metrics", "Medição"),
                pair("progress-state", "Fases e publicações"),
                pair("progress-commits", "Commits"),
            ]
        );
        assert_eq!(anchors(section(&doc, "agreed")), [pair("agreed-rule", "Regras"), pair("agreed-decision", "Decisões")]);
        assert_eq!(anchors(section(&doc, "specification")), [pair("specification-context", "Contexto")]);
        assert_eq!(
            anchors(section(&doc, "criteria")),
            [pair("criteria-criterion", "Critérios de aceite"), pair("criteria-runs", "Execuções")]
        );
        assert_eq!(anchors(section(&doc, "review")), [pair("review-3", "Onda 3")]);
        assert_eq!(anchors(section(&doc, "notes")), [pair("notes-all", "Anotações")]);

        let open: Vec<&str> =
            sections(&doc).iter().flat_map(|s| groups(s)).filter(|g| g.open).map(|g| g.anchor.as_str()).collect();
        assert_eq!(open, ["progress-metrics"], "only the measurement opens with the page");
        assert_eq!(group(section(&doc, "review"), "review-3").summary, "1 reprovada · 1 aprovada");
        assert_eq!(group(section(&doc, "agreed"), "agreed-rule").summary, "", "no status, no tally");
        for (section_anchor, anchor) in [("criteria", "criteria-runs"), ("progress", "progress-state")] {
            let quiet = group(section(&doc, section_anchor), anchor);
            assert!(Node::items(&quiet.body).iter().any(|i| i.status.is_some()), "{anchor}");
            assert_eq!(quiet.summary, "", "only the groups by type and the review tally: {anchor}");
        }
    }

    /// Um item removido some de todas as seções; a remoção fica na conversa,
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
        let texts: Vec<&str> = sections(&doc).into_iter().flat_map(items).map(|i| i.text.as_str()).collect();
        assert!(!texts.contains(&"Regra que sai."), "{texts:?}");
        assert!(texts.contains(&"Regra que fica."));
        let removal = items(section(&doc, "conversation"))[0];
        assert_eq!(removal.code, "MSTD-RMV-0001");
        let values: Vec<&str> = removal.fields.iter().map(|f| f.value.as_str()).collect();
        assert_eq!(values, ["engano", "MSTD-RULE-0001"]);
    }

    /// Todo rótulo que a página usa existe nos dois idiomas: seções, grupos,
    /// tipos, campos, valores, fases, autores e a moldura.
    #[test]
    fn every_label_the_page_uses_exists_in_both_languages() {
        let mut keys: Vec<String> = Vec::new();
        for block in Block::ALL.iter().filter(|b| !matches!(b, Block::State | Block::Metrics)) {
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
        for (_, subject) in SUBJECTS {
            keys.push(format!("page.findings.{subject}"));
        }
        for state in [WaveState::Todo, WaveState::Running, WaveState::Delivered, WaveState::Approved, WaveState::Rejected] {
            keys.push(state.key().to_string());
            keys.push(format!("{}.many", state.key()));
        }
        for key in [
            "page.group.criterion_run",
            "page.group.criterion",
            "page.group.skill",
            "page.group.metrics",
            "page.group.state",
            "page.group.commits",
            "page.group.day",
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
            "page.kind.spec",
            "page.meta.spec",
            "page.meta.phase",
            "page.meta.branch",
            "page.meta.base",
            "page.empty",
            "page.replaced",
            "page.old_version",
            "page.after_approval",
            "page.wave.heading",
            "page.findings.heading",
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
            "page.metrics.col.chars",
            "page.metrics.col.items",
            "page.metrics.col.delivery",
            "page.metrics.col.rejected",
            "page.metrics.col.last",
            "page.field.wave_commit",
            "page.field.wave_receives",
            "page.field.url",
            "page.wave.prompt",
            "page.wave.prompt.summary",
            "page.wave.sent",
            "page.conversation.cut",
            "page.purge_pending",
            "page.withheld_found",
            "page.too_big",
            "page.sections",
            "page.search.placeholder",
            "page.search.label",
            "page.open_all",
            "page.close_all",
            "page.not_found",
            "page.of",
            "page.count.one",
            "page.count.many",
            "page.request",
            "project.kind",
            "project.specs",
            "project.stages",
            "project.no_phase",
            "project.col.state",
            "project.col.branch",
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

    /// Nenhum arquivo da página da spec passa do teto de linhas de código: a
    /// porta e cada parte da pasta dela, pela medida única do núcleo.
    #[test]
    fn no_file_of_the_spec_page_goes_over_the_code_line_cap() {
        let gate = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("view").join("document").join("spec.rs");
        assert_eq!(crate::io::fs::files_over_code_line_cap(&gate), Ok(Vec::new()));
    }
}
