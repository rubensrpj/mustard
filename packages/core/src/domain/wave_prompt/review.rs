//! O pedido do revisor final e a lista dos itens que ele lista.
//!
//! Como o pedido da onda ([`super::request`]), as duas coisas saem da mesma
//! passagem sobre o material ([`ReviewListing`]): a que imprime as linhas e a
//! que diz, para o veredito conferir, quais itens o revisor precisa ter lido.
//! Nenhuma calcula a lista por conta própria, então o que o pedido mostra e o
//! que o veredito cobra nunca discordam.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use super::{code_of, language_line, project_rules_section, Material, Writer};
use crate::domain::spec_events::SpecEvent;

/// Tudo o que o pedido do revisor lista, na ordem em que o imprime: cada
/// parte com o título dela e os itens que ela traz. A parte sem item não
/// aparece no texto.
struct ReviewListing<'m, 'a> {
    parts: Vec<(&'static str, &'m [&'a SpecEvent])>,
}

impl<'m, 'a> ReviewListing<'m, 'a> {
    fn of(material: &'m Material<'a>) -> Self {
        Self {
            parts: vec![
                ("prompt.part.since_verdict", material.since_verdict.as_slice()),
                ("prompt.part.waves", material.block.as_slice()),
                ("prompt.part.agreed", material.agreed.as_slice()),
                ("prompt.part.each_delivered", material.own_delivered.as_slice()),
                ("prompt.part.criteria", material.criteria.as_slice()),
                ("prompt.part.validation", material.validation.as_slice()),
                ("prompt.part.branch_changes", material.changes.as_slice()),
            ],
        }
    }

    /// Os itens de todas as partes, na ordem em que o pedido os imprime; o que
    /// mais de uma parte traz aparece em cada uma.
    fn items(&self) -> impl Iterator<Item = &'a SpecEvent> + '_ {
        self.parts.iter().flat_map(|(_, items)| items.iter().copied())
    }
}

/// O que o pedido do revisor lista, para o veredito conferir a leitura: o
/// código de cada item, uma vez só, na ordem em que o pedido os imprime. É a
/// mesma lista que o texto do pedido percorre.
#[must_use]
pub fn listed_final_review(material: &Material) -> Vec<String> {
    let mut seen: BTreeSet<u64> = BTreeSet::new();
    ReviewListing::of(material)
        .items()
        .filter(|item| seen.insert(item.id))
        .map(|item| code_of(material, item))
        .collect()
}

impl Writer<'_> {
    /// O pedido do agente de teste dedicado, que o fechamento pede a toda
    /// obra: as instruções fixas dele, o que olhar — a obra inteira na
    /// primeira revisão; na de volta, o que mudou desde o veredito que
    /// reprovou e o encaixe disso no resto —, como ler cada item e, em
    /// seguida, uma linha por item: o que mudou, as ondas com as tarefas, as
    /// emendas gravadas para elas, o que cada uma entregou, os critérios e os
    /// commits que já entraram na branch. Por fim, como revisar numa cópia
    /// separada. O veredito que reprovou aparece uma vez só, na parte do que
    /// mudou. Com as regras do projeto no material, a seção delas fecha o
    /// pedido.
    pub(super) fn final_review_text(&self) -> String {
        let m = self.material;
        let listing = ReviewListing::of(m);
        let mut out = String::new();
        let _ = writeln!(out, "# {}\n", self.t("prompt.final.title").replace("{spec}", &m.spec));
        let _ = writeln!(out, "{}\n", language_line(&m.execution.language));
        out.push_str(self.t("prompt.final.fixed"));
        out.push_str("\n\n");
        let look = if m.since_verdict.is_empty() { "prompt.final.look" } else { "prompt.final.look_again" };
        out.push_str(self.t(look));
        out.push_str("\n\n");
        let _ = writeln!(out, "## {}\n", self.t("prompt.part.read"));
        self.read_example(&mut out, "prompt.read.final", true);
        for (key, items) in &listing.parts {
            self.part(&mut out, key, items);
        }
        self.review_execution(&mut out);
        self.prepared_sources(&mut out);
        while out.ends_with("\n\n") {
            out.pop();
        }
        if let Some(section) = project_rules_section(&m.project_rules, self.lang) {
            let _ = writeln!(out, "\n{section}");
        }
        out
    }

    /// Uma parte do pedido: o título e uma linha por item, na ordem da spec.
    /// A parte sem nenhum item não aparece.
    fn part(&self, out: &mut String, key: &str, items: &[&SpecEvent]) {
        if items.is_empty() {
            return;
        }
        let _ = writeln!(out, "## {}\n", self.t(key));
        for item in items {
            let _ = writeln!(out, "- {}", self.item_text(item));
        }
        out.push('\n');
    }
}
