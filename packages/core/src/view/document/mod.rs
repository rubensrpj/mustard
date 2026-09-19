//! `view::document` — a árvore de blocos de uma página do Mustard.
//!
//! Toda página (a de uma spec, a do projeto, uma avulsa escrita em markdown)
//! é esta árvore antes de virar texto. Quem monta a árvore não sabe de HTML
//! nem de markdown; quem a escreve (o motor de página do `mustard-rt`) não sabe
//! de eventos. Assim a mesma árvore sai como `.md` e como `.html`, e toda
//! página tem o mesmo desenho.
//!
//! A página abre curta: cada seção junta os itens em grupos recolhidos, e
//! cada item mostra numa linha o código, o título, a situação e a data; o
//! texto e os campos abrem por baixo.
//!
//! O texto guardado na árvore é markdown de linha: código entre crases,
//! negrito entre asteriscos duplos e link. [`Item::text`] e
//! [`Node::Markdown`] são a exceção e podem ter parágrafos, listas e títulos.
//!
//! Tudo aqui é puro: sem disco, sem relógio e sem caminho da máquina. A mesma
//! entrada dá sempre a mesma árvore.

use std::collections::BTreeSet;

mod spec;

pub use spec::{conversation_len, cut_oldest_conversation, owner_label, owner_rule_key, owners_page, RtkDay, WaveState, WaveStates};

/// Uma página inteira.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    /// O idioma da página, no formato BCP-47 (`pt-BR`, `en-US`).
    pub lang: String,
    /// O que vem depois de `Mustard · ` na faixa do cabeçalho; sem ele, a
    /// faixa diz só `Mustard`.
    pub kind: Option<String>,
    /// O título, na aba e no cabeçalho.
    pub title: String,
    /// A linha de dados do cabeçalho, na ordem.
    pub meta: Vec<Meta>,
    /// O corpo, na ordem.
    pub body: Vec<Node>,
    /// A linha do rodapé, em markdown de linha.
    pub footer: Option<String>,
}

/// Um dado da linha do cabeçalho.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Meta {
    /// Um rótulo e um valor em destaque, como `spec mustard-enxuto`.
    Pair { label: String, value: String },
    /// Um texto solto.
    Note(String),
}

/// Um bloco do corpo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// Uma seção com título.
    Section(Section),
    /// Um subtítulo dentro de uma seção; `level` 3 é o primeiro nível abaixo
    /// do título da seção.
    Heading { level: u8, text: String },
    /// Um parágrafo, em markdown de linha.
    Paragraph(String),
    /// Uma lista; cada item é uma sequência de blocos.
    List { ordered: bool, items: Vec<Vec<Node>> },
    /// Uma tabela; cada célula em markdown de linha.
    Table(Table),
    /// Um bloco de texto monoespaçado, mostrado como está.
    Code(String),
    /// Um documento em markdown dentro da página, como o pedido enviado a
    /// um agente: a página o mostra formatado, como um arquivo `.md`, e o
    /// `.md` o traz cercado, linha por linha.
    Markdown(String),
    /// Um trecho recolhido: o resumo aparece, e os blocos abrem ao clicar.
    /// `owner` é o código do item de quem o trecho é, quando é de um: o texto
    /// enviado de um pedido é do envio que o gravou.
    Details { summary: String, body: Vec<Node>, owner: Option<String> },
    /// Um destaque, com os blocos dentro.
    Quote(Vec<Node>),
    /// Um traço de separação.
    Rule,
    /// Um item com código, como uma regra ou um critério de uma spec.
    Item(Item),
    /// Um grupo de uma seção, recolhido numa linha só.
    Group(Group),
    /// A visão geral no topo de uma seção: uma ficha por parte, com o estado
    /// dela e o atalho para o grupo dela.
    Overview(Overview),
}

/// Uma seção: o título, o endereço e os blocos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// O endereço da seção na página, só com letras, números e hífen.
    pub anchor: Option<String>,
    /// O título, em markdown de linha.
    pub heading: String,
    pub body: Vec<Node>,
}

/// Um grupo: a linha recolhida com o título, o resumo e a contagem dos
/// itens, e os blocos que abrem por baixo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// O endereço do grupo na página, único nela.
    pub anchor: String,
    /// O título, em texto.
    pub title: String,
    /// A situação que abre o resumo, como o estado de uma onda.
    pub status: Option<Status>,
    /// O resumo, em markdown de linha: o nome da onda ou a conta das
    /// situações dos itens.
    pub summary: String,
    /// `true` no grupo que a página abre aberto.
    pub open: bool,
    pub body: Vec<Node>,
}

/// A situação de um item ou de um grupo: o que se lê e o tom da cor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub label: String,
    pub tone: Tone,
}

/// O tom de uma situação. Só aprovado, reprovado e em andamento têm cor; a
/// coisa por fazer e a entregue têm só o traço, e a versão antiga de um item
/// sai apagada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Plain,
    Good,
    Bad,
    Running,
    Todo,
    Done,
    Old,
}

/// A visão geral: o título, a conta das situações e uma ficha por parte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overview {
    pub title: String,
    /// A conta das situações, como "3 entregues · 1 a fazer".
    pub legend: String,
    pub cards: Vec<Card>,
}

/// Uma ficha da visão geral.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    /// O endereço do grupo que a ficha abre.
    pub target: String,
    /// O que a ficha mostra em destaque, como o número da onda.
    pub label: String,
    pub status: Status,
    /// O nome da parte, em markdown de linha, mostrado ao passar o mouse.
    pub hint: String,
}

/// Uma tabela: o cabeçalho e as linhas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// Um item com código (`MSTD-RULE-0005`), a linha recolhida, o texto e os
/// campos dele.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// O código do item.
    pub code: String,
    /// `true` quando o código é o endereço deste item na página. A versão
    /// antiga de um item revisto mostra o código sem ser o endereço dele.
    pub anchored: bool,
    /// O título da linha recolhida, em markdown de linha.
    pub title: String,
    /// A situação da linha recolhida.
    pub status: Option<Status>,
    /// De que é e de quem é o item, como "mensagem · usuário".
    pub who: Option<String>,
    /// Uma marca que só o `.md` diz depois do código, como "depois da
    /// aprovação".
    pub mark: Option<String>,
    /// A hora do item, até o minuto: "2026-09-11 21:03".
    pub date: Option<String>,
    /// O texto do item, em markdown; pode ter parágrafos e listas.
    pub text: String,
    /// Os outros campos, na ordem.
    pub fields: Vec<Field>,
}

impl Item {
    /// A marca curta que o `.md` põe depois do código: de que e de quem é, a
    /// marca e a hora. Sem nada a dizer além da hora, nada.
    #[must_use]
    pub fn note(&self) -> Option<String> {
        if self.who.is_none() && self.mark.is_none() {
            return None;
        }
        let parts: Vec<&str> =
            [&self.who, &self.mark, &self.date].into_iter().filter_map(|part| part.as_deref()).collect();
        Some(parts.join(" · "))
    }
}

impl Node {
    /// Os itens destes blocos, também os de dentro de um grupo, na ordem.
    #[must_use]
    pub fn items(nodes: &[Self]) -> Vec<&Item> {
        let mut out = Vec::new();
        for node in nodes {
            match node {
                Self::Item(item) => out.push(item),
                Self::Group(group) => out.extend(Self::items(&group.body)),
                _ => {}
            }
        }
        out
    }
}

/// Um campo de um item: o rótulo e o valor em markdown de linha.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub label: String,
    pub value: String,
}

impl Document {
    /// Os endereços que existem na página: o de cada seção e o de cada item
    /// que é o dono do seu código.
    #[must_use]
    pub fn anchors(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        collect_anchors(&self.body, &mut out);
        out
    }

    /// Troca por `mark`, em todo texto da página, cada trecho que `find`
    /// acha; o resto do texto fica. Devolve o código de cada item em que um
    /// trecho foi trocado, uma vez só e na ordem da página, e quantos textos
    /// fora de item foram mexidos. O trecho recolhido que é de um item conta
    /// pelo código do item.
    pub fn redact(&mut self, find: &dyn Fn(&str) -> Vec<String>, mark: &str) -> (Vec<String>, usize) {
        let mut codes = Vec::new();
        let mut loose = 0;
        let redactor = Redactor { find, mark };
        let mut plain = |text: &mut String| {
            if redactor.apply(text) {
                loose += 1;
            }
        };
        plain(&mut self.title);
        if let Some(kind) = self.kind.as_mut() {
            plain(kind);
        }
        for meta in &mut self.meta {
            match meta {
                Meta::Pair { label, value } => {
                    plain(label);
                    plain(value);
                }
                Meta::Note(text) => plain(text),
            }
        }
        if let Some(footer) = self.footer.as_mut() {
            plain(footer);
        }
        redact_nodes(&mut self.body, &redactor, &mut codes, &mut loose);
        // A versão substituída de um item mostra o mesmo código da vigente:
        // o código sai uma vez só.
        let mut seen = BTreeSet::new();
        codes.retain(|code| seen.insert(code.clone()));
        (codes, loose)
    }
}

/// A troca de trechos de uma página: quem acha e o que fica no lugar.
struct Redactor<'a> {
    find: &'a dyn Fn(&str) -> Vec<String>,
    mark: &'a str,
}

impl Redactor<'_> {
    /// Troca os trechos de `text`; `true` quando trocou algum.
    fn apply(&self, text: &mut String) -> bool {
        let mut found = (self.find)(text);
        if found.is_empty() {
            return false;
        }
        found.sort_by_key(|excerpt| std::cmp::Reverse(excerpt.len()));
        for excerpt in found {
            if !excerpt.is_empty() {
                *text = text.replace(&excerpt, self.mark);
            }
        }
        true
    }
}

fn redact_nodes(nodes: &mut [Node], redactor: &Redactor<'_>, codes: &mut Vec<String>, loose: &mut usize) {
    let plain = |text: &mut String, loose: &mut usize| {
        if redactor.apply(text) {
            *loose += 1;
        }
    };
    for node in nodes {
        match node {
            Node::Section(section) => {
                plain(&mut section.heading, loose);
                redact_nodes(&mut section.body, redactor, codes, loose);
            }
            Node::Group(group) => {
                plain(&mut group.title, loose);
                plain(&mut group.summary, loose);
                redact_nodes(&mut group.body, redactor, codes, loose);
            }
            Node::Overview(overview) => {
                for card in &mut overview.cards {
                    plain(&mut card.hint, loose);
                }
            }
            Node::Heading { text, .. } | Node::Paragraph(text) | Node::Code(text) | Node::Markdown(text) => {
                plain(text, loose);
            }
            Node::List { items, .. } => {
                for item in items {
                    redact_nodes(item, redactor, codes, loose);
                }
            }
            Node::Table(table) => {
                for cell in table.headers.iter_mut().chain(table.rows.iter_mut().flatten()) {
                    plain(cell, loose);
                }
            }
            Node::Quote(inner) => redact_nodes(inner, redactor, codes, loose),
            Node::Details { summary, body, owner: None } => {
                plain(summary, loose);
                redact_nodes(body, redactor, codes, loose);
            }
            Node::Details { summary, body, owner: Some(owner) } => {
                let mut inside = 0;
                plain(summary, &mut inside);
                redact_nodes(body, redactor, codes, &mut inside);
                if inside > 0 {
                    codes.push(owner.clone());
                }
            }
            Node::Rule => {}
            Node::Item(item) => {
                let mut found = redactor.apply(&mut item.text);
                found |= redactor.apply(&mut item.title);
                for part in [&mut item.who, &mut item.mark].into_iter().flatten() {
                    found |= redactor.apply(part);
                }
                for field in &mut item.fields {
                    found |= redactor.apply(&mut field.label);
                    found |= redactor.apply(&mut field.value);
                }
                if found {
                    codes.push(item.code.clone());
                }
            }
        }
    }
}

fn collect_anchors(nodes: &[Node], out: &mut BTreeSet<String>) {
    for node in nodes {
        match node {
            Node::Section(section) => {
                if let Some(anchor) = &section.anchor {
                    out.insert(anchor.clone());
                }
                collect_anchors(&section.body, out);
            }
            Node::Group(group) => {
                out.insert(group.anchor.clone());
                collect_anchors(&group.body, out);
            }
            Node::Item(item) if item.anchored => {
                out.insert(item.code.clone());
            }
            Node::List { items, .. } => {
                for item in items {
                    collect_anchors(item, out);
                }
            }
            Node::Quote(inner) | Node::Details { body: inner, .. } => collect_anchors(inner, out),
            _ => {}
        }
    }
}
