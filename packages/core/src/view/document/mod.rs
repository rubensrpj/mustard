//! `view::document` — a árvore de blocos de uma página do Mustard.
//!
//! Toda página (a de uma spec, a do projeto, uma avulsa escrita em markdown)
//! é esta árvore antes de virar texto. Quem monta a árvore não sabe de HTML
//! nem de markdown; quem a escreve (o motor de página do `mustard-rt`) não sabe
//! de eventos. Assim a mesma árvore sai como `.md` e como `.html`, e toda
//! página tem o mesmo desenho.
//!
//! O texto guardado na árvore é markdown de linha: código entre crases,
//! negrito entre asteriscos duplos e link. [`Item::text`] é a exceção e pode
//! ter parágrafos, listas e títulos.
//!
//! Tudo aqui é puro: sem disco, sem relógio e sem caminho da máquina. A mesma
//! entrada dá sempre a mesma árvore.

use std::collections::BTreeSet;

mod project;
mod spec;

pub use project::project_document;
pub use spec::{conversation_len, cut_oldest_conversation, spec_document, spec_page, RtkDay, SpecInputs, WavePrompts};

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
    /// Um trecho recolhido: o resumo aparece, e os blocos abrem ao clicar.
    Details { summary: String, body: Vec<Node> },
    /// Um destaque, com os blocos dentro.
    Quote(Vec<Node>),
    /// Um traço de separação.
    Rule,
    /// Um item com código, como uma regra ou um critério de uma spec.
    Item(Item),
}

/// Uma seção: o título, o endereço e os blocos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// O endereço da seção na página, só com letras, números e hífen.
    pub anchor: Option<String>,
    /// O título, em markdown de linha.
    pub heading: String,
    /// Quando existe, a seção vem recolhida e este é o texto que a abre.
    pub collapsed: Option<String>,
    pub body: Vec<Node>,
}

/// Uma tabela: o cabeçalho e as linhas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// Um item com código (`MSTD-RULE-0005`), o texto e os campos dele.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// O código do item.
    pub code: String,
    /// `true` quando o código é o endereço deste item na página. A versão
    /// antiga de um item revisto mostra o código sem ser o endereço dele.
    pub anchored: bool,
    /// Uma marca curta depois do código, como o autor e a hora.
    pub note: Option<String>,
    /// O texto do item, em markdown; pode ter parágrafos e listas.
    pub text: String,
    /// Os outros campos, na ordem.
    pub fields: Vec<Field>,
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

    /// Tira da página todo trecho em que `hit` acha algo, e põe `notice` no
    /// lugar: o item inteiro sai, com o texto e os campos, e fica só o código
    /// dele com o aviso; fora de um item, sai o trecho. Devolve o código de
    /// cada item retido e quantos trechos fora de item saíram.
    pub fn withhold(&mut self, hit: &dyn Fn(&str) -> bool, notice: &str) -> (Vec<String>, usize) {
        let mut codes = Vec::new();
        let mut loose = 0;
        let mut plain = |text: &mut String| {
            if hit(text) {
                *text = notice.to_string();
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
        withhold_nodes(&mut self.body, hit, notice, &mut codes, &mut loose);
        (codes, loose)
    }
}

fn withhold_nodes(nodes: &mut [Node], hit: &dyn Fn(&str) -> bool, notice: &str, codes: &mut Vec<String>, loose: &mut usize) {
    let plain = |text: &mut String, loose: &mut usize| {
        if hit(text) {
            *text = notice.to_string();
            *loose += 1;
        }
    };
    for node in nodes {
        match node {
            Node::Section(section) => {
                plain(&mut section.heading, loose);
                if let Some(summary) = section.collapsed.as_mut() {
                    plain(summary, loose);
                }
                withhold_nodes(&mut section.body, hit, notice, codes, loose);
            }
            Node::Heading { text, .. } | Node::Paragraph(text) | Node::Code(text) => {
                plain(text, loose);
            }
            Node::List { items, .. } => {
                for item in items {
                    withhold_nodes(item, hit, notice, codes, loose);
                }
            }
            Node::Table(table) => {
                for cell in table.headers.iter_mut().chain(table.rows.iter_mut().flatten()) {
                    plain(cell, loose);
                }
            }
            Node::Quote(inner) => withhold_nodes(inner, hit, notice, codes, loose),
            Node::Details { summary, body } => {
                plain(summary, loose);
                withhold_nodes(body, hit, notice, codes, loose);
            }
            Node::Rule => {}
            Node::Item(item) => {
                let found = hit(&item.text)
                    || item.note.as_deref().is_some_and(hit)
                    || item.fields.iter().any(|f| hit(&f.label) || hit(&f.value));
                if found {
                    item.text = notice.to_string();
                    item.note = None;
                    item.fields.clear();
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
