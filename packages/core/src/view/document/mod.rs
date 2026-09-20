//! `view::document` — a árvore de blocos de uma página avulsa em markdown.
//!
//! A página avulsa (análise, relatório, plano) escrita em markdown passa por
//! esta árvore antes de virar HTML: quem monta a árvore não sabe de HTML, e
//! quem a escreve (o motor de página do `mustard-rt`) não sabe de markdown.
//!
//! O motor que montava a página inteira de uma spec e a do projeto, item por
//! item, saiu com o comando que só ele servia: hoje essas páginas são só
//! template mais banco de dados, e esta árvore carrega apenas os blocos que
//! um markdown solto pode ter (título, parágrafo, lista, tabela, código e
//! citação).
//!
//! Tudo aqui é puro: sem disco, sem relógio e sem caminho da máquina. A mesma
//! entrada dá sempre a mesma árvore.

mod spec;

pub use spec::{RtkDay, WaveState, WaveStates};

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

/// Uma tabela: o cabeçalho e as linhas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl Document {
    /// Os endereços que existem na página: o de cada seção.
    #[must_use]
    pub fn anchors(&self) -> std::collections::BTreeSet<String> {
        let mut out = std::collections::BTreeSet::new();
        collect_anchors(&self.body, &mut out);
        out
    }
}

fn collect_anchors(nodes: &[Node], out: &mut std::collections::BTreeSet<String>) {
    for node in nodes {
        match node {
            Node::Section(section) => {
                if let Some(anchor) = &section.anchor {
                    out.insert(anchor.clone());
                }
                collect_anchors(&section.body, out);
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
