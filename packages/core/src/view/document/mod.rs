//! `view::document` — a árvore de blocos de uma página do Mustard.
//!
//! Toda página (a de uma spec, uma avulsa escrita em markdown)
//! é esta árvore antes de virar texto. Quem monta a árvore não sabe de HTML
//! nem de markdown; quem a escreve (o motor de página do `mustard-rt`) não sabe
//! de eventos. Assim a mesma árvore sai como `.md` e como `.html`, e toda
//! página tem o mesmo desenho.
//!
//! O texto guardado na árvore é markdown de linha: código entre crases,
//! negrito entre asteriscos duplos e link. [`Item::text`] é a exceção e pode
//! ter parágrafos e listas.
//!
//! Tudo aqui é puro: sem disco, sem relógio e sem caminho da máquina. A mesma
//! entrada dá sempre a mesma árvore.

use std::collections::BTreeSet;

mod spec;

pub use spec::{spec_document, WavePrompts};

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
            Node::Quote(inner) => collect_anchors(inner, out),
            _ => {}
        }
    }
}
