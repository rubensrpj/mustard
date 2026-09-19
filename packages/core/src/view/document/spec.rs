//! O que a página de uma spec e a lista dos itens sem dono compartilham: o
//! estado de uma onda, o pedido montado para ela, um dia da economia do rtk,
//! e o `Page`, que lê o `spec.ndjson` uma vez e monta um item (`item`), o
//! corte da conversa grande demais (`conversation`) e a lista dos itens sem
//! dono (`owners`).
//!
//! O motor que montava a página inteira da spec e a do projeto a partir do
//! arquivo de eventos saiu com o comando que só ele servia: as duas só
//! existem hoje como template mais banco de dados. Cada item leva o seu
//! código (`MSTD-RULE-0005`), que é também o endereço dele na página nova, e
//! toda referência a outro evento sai como o código dele. Numa spec aprovada,
//! o item gravado depois da aprovação que vale sai marcado "depois da
//! aprovação", com a hora dele: é o que mudou sem aprovação nova.

mod conversation;
mod item;
mod owners;

use std::collections::BTreeMap;

use crate::domain::spec_events::SpecLog;
use crate::domain::spec_state::approval_boundary;
use crate::platform::i18n::{translate, Locale};
use crate::view::document::{Node, Section};

pub use conversation::{conversation_len, cut_oldest_conversation};
pub use owners::{owner_label, owner_rule_key, owners_page};

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
/// Nenhuma onda fica como `Delivered` hoje: a rodada não pede revisão de onda
/// nenhuma, e a entrega já vale como aprovada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaveState {
    /// Nada saiu ainda, ou o que saiu não vale mais.
    Todo,
    /// O pedido saiu e a entrega ainda não voltou.
    Running,
    /// Entregue e aprovada.
    Approved,
    /// A última revisão reprovou.
    Rejected,
}

/// O estado de cada onda, pelo número dela. A onda que não está aqui está
/// por fazer.
pub type WaveStates = BTreeMap<u64, WaveState>;

struct Page {
    lang: Locale,
    codes: BTreeMap<u64, String>,
    /// O número da aprovação que vale, a mesma que o leitor da aprovação e o
    /// aviso de crescimento das ondas leem.
    approval: Option<u64>,
}

impl Page {
    fn new(log: &SpecLog, lang: Locale) -> Self {
        Self { lang, codes: log.codes(), approval: approval_boundary(log) }
    }

    fn t(&self, key: &str) -> &'static str {
        translate(key, self.lang)
    }

    fn code(&self, id: u64) -> String {
        self.codes.get(&id).cloned().unwrap_or_else(|| id.to_string())
    }

    /// Uma seção da página; sem bloco nenhum, ela diz que está vazia.
    fn section(&self, anchor: &str, heading: &str, mut body: Vec<Node>) -> Node {
        if body.is_empty() {
            body.push(Node::Paragraph(self.t("page.empty").to_string()));
        }
        Node::Section(Section { anchor: Some(anchor.to_string()), heading: heading.to_string(), body })
    }
}

/// O primeiro parágrafo de um texto em markdown, em uma linha, sem a marca de
/// título nem a de item de lista: é o título da linha recolhida.
fn first_paragraph(text: &str) -> String {
    let first = one_line(text.trim().split("\n\n").next().unwrap_or_default());
    let line = first.trim_start_matches('#').trim_start();
    line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")).unwrap_or(line).to_string()
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

    /// Uma linha de evento, para os testes de `item`, `conversation` e
    /// `owners`: o código do bloco, não da spec.
    pub(super) fn line(id: u64, event_type: &str, extra: &str) -> String {
        format!("{{\"v\":1,\"id\":{id},\"at\":\"2026-09-12T10:0{}:00-03:00\",\"type\":\"{event_type}\",\"author\":\"assistant\"{extra}}}\n", id % 10)
    }

    /// Nenhum arquivo da página da spec passa do teto de linhas de código: a
    /// porta e cada parte da pasta dela, pela medida única do núcleo.
    #[test]
    fn no_file_of_the_spec_page_goes_over_the_code_line_cap() {
        let gate = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("view").join("document").join("spec.rs");
        assert_eq!(crate::io::fs::files_over_code_line_cap(&gate), Ok(Vec::new()));
    }
}
