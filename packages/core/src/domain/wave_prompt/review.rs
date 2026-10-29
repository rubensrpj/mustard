//! O pedido do revisor final e a lista dos itens que ele lista.
//!
//! Como o pedido da onda ([`super::request`]), as duas coisas saem da mesma
//! passagem sobre o material ([`ReviewListing`]): a que imprime as linhas e a
//! que diz, para o veredito conferir, quais itens o revisor precisa ter lido.
//! Nenhuma calcula a lista por conta própria, então o que o pedido mostra e o
//! que o veredito cobra nunca discordam.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use serde_json::Value;

use super::{code_of, item_proofs, item_title, language_line, project_rules_section, Material, Writer};
use crate::domain::spec_events::{SpecEvent, SpecLog};

/// A cadeia de uma tarefa, que o revisor confere no conserto: a regra que ela
/// faz valer, os lugares dessa regra, achados pelo mapa, e as provas dos
/// itens que ela cobre, que a onda roda.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Chain {
    /// A regra: o código e o título da regra da spec que a tarefa cita, ou o
    /// texto da regra nova. `None` na tarefa sem regra.
    pub rule: Option<String>,
    /// Os lugares da regra, como a tarefa os grava.
    pub places: Vec<String>,
    /// As provas dos itens cobertos ([`item_proofs`]), sem repetir.
    pub proofs: Vec<String>,
}

/// A cadeia de cada tarefa de `tasks`, lida em `log`, pelo número dela, com o
/// comando de teste que o projeto declara (`test`). A regra citada pelo
/// código sai com o título da versão vigente; a citada pelo texto sai como
/// está. A tarefa sem regra, sem lugar e sem prova fica de fora.
#[must_use]
pub fn task_chains<'a>(log: &SpecLog, tasks: impl IntoIterator<Item = &'a SpecEvent>, test: Option<&str>) -> BTreeMap<u64, Chain> {
    let codes = log.codes();
    let named = |said: &str| -> String {
        let rule = codes.iter().filter(|(_, code)| code.as_str() == said).map(|(id, _)| *id).max().and_then(|id| log.current(id));
        match rule.and_then(item_title) {
            Some(title) => format!("{said} — {title}"),
            None => said.to_string(),
        }
    };
    let mut out = BTreeMap::new();
    for task in tasks.into_iter().filter(|e| e.event_type == "task") {
        let rule = task.str_field("rule").map(str::trim).filter(|said| !said.is_empty()).map(named);
        let places: Vec<String> =
            task.fields.get("places").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::trim).filter(|place| !place.is_empty()).map(str::to_string).collect();
        let mut proofs: Vec<String> = Vec::new();
        for proof in task.ints("covers").into_iter().filter_map(|id| log.current(id)).flat_map(|item| item_proofs(item, test)) {
            if !proofs.contains(&proof) {
                proofs.push(proof);
            }
        }
        let chain = Chain { rule, places, proofs };
        if chain != Chain::default() {
            out.insert(task.id, chain);
        }
    }
    out
}

/// O que já tem prova verde na spec, para a revisão final ler só o resto: o
/// critério cuja execução mais nova no fechamento passou, a tarefa com passo
/// de término gravado — o passo sai com a prova verde — e o item combinado
/// que uma onda cobre, citado no `covers` de uma tarefa terminada. A resposta
/// `met` da entrega é a palavra do agente, não prova. Tarefa e combinado vão
/// pelo código, que é o mesmo em toda versão; o critério vai pelo número,
/// porque a versão nova pede execução nova. A última aprovação final também
/// conta ([`super::last_final_approval`]): o item cuja versão vigente veio
/// antes do pedido que ela respondeu já foi conferido por ela, e o ajuste
/// feito depois é revisado sozinho.
pub struct Proven {
    criteria: BTreeSet<u64>,
    tasks: BTreeSet<String>,
    agreed: BTreeSet<String>,
    codes: BTreeMap<u64, String>,
    /// O número a partir do qual o item escapa da última aprovação final; 0
    /// sem aprovação.
    approved_until: u64,
}

impl Proven {
    #[must_use]
    pub fn of(log: &SpecLog) -> Self {
        let codes = log.codes();
        // O item citado pelo número, como a rodada grava, ou pelo código,
        // como o agente escreve.
        let shown = |reference: Option<&Value>| -> Option<String> {
            match reference? {
                Value::Number(n) => n.as_u64().and_then(|id| codes.get(&id).cloned()),
                Value::String(text) => {
                    let text = text.trim();
                    Some(text.parse::<u64>().ok().and_then(|id| codes.get(&id).cloned()).unwrap_or_else(|| text.to_string()))
                }
                _ => None,
            }
        };
        let visible = log.visible();
        let mut runs: BTreeMap<u64, bool> = BTreeMap::new();
        for run in visible.iter().filter(|e| e.event_type == "criterion_run") {
            if let Some(id) = run.int("criterion") {
                runs.insert(id, run.str_field("result") == Some("pass"));
            }
        }
        let criteria = runs.into_iter().filter(|(_, passed)| *passed).map(|(id, _)| id).collect();
        let tasks: BTreeSet<String> = visible.iter().filter(|e| e.event_type == "step").filter_map(|e| shown(e.fields.get("item"))).collect();
        let agreed: BTreeSet<String> = visible
            .iter()
            .filter(|e| e.event_type == "task" && codes.get(&e.id).is_some_and(|code| tasks.contains(code)))
            .flat_map(|task| task.ints("covers"))
            .filter_map(|id| codes.get(&id).cloned())
            .collect();
        let approved_until = super::last_final_approval(log).map_or(0, |(_, since)| since);
        Self { criteria, tasks, agreed, codes, approved_until }
    }

    fn code(&self, item: &SpecEvent) -> Option<&String> {
        self.codes.get(&item.id)
    }

    /// Se o item já tem prova verde: o critério, a tarefa ou o combinado
    /// conforme o tipo, ou qualquer um deles conferido pela última aprovação
    /// final. Os outros tipos nunca têm.
    #[must_use]
    pub fn covers(&self, item: &SpecEvent) -> bool {
        if item.id < self.approved_until {
            return true;
        }
        match item.event_type.as_str() {
            "criterion" => self.criteria.contains(&item.id),
            "task" => self.code(item).is_some_and(|code| self.tasks.contains(code)),
            _ => self.code(item).is_some_and(|code| self.agreed.contains(code)),
        }
    }
}

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
    /// primeira revisão; na de volta, só a cadeia de cada conserto feito
    /// desde o veredito que reprovou ou aprovou: a regra, os lugares e a
    /// prova, sem caçar achado no resto da obra —, os princípios do
    /// produto que o revisor recebe, como ler cada item e, em
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
        // O primeiro item do que mudou é o veredito de onde a revisão volta.
        let look = match m.since_verdict.first() {
            None => "prompt.final.look",
            Some(verdict) if super::approved(verdict) => "prompt.final.look_since_approval",
            Some(_) => "prompt.final.look_again",
        };
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
    /// A tarefa com cadeia ([`Chain`]) a traz logo abaixo da linha dela. A
    /// parte sem nenhum item não aparece.
    fn part(&self, out: &mut String, key: &str, items: &[&SpecEvent]) {
        if items.is_empty() {
            return;
        }
        let _ = writeln!(out, "## {}\n", self.t(key));
        for item in items {
            let _ = writeln!(out, "- {}", self.item_text(item));
            if let Some(chain) = self.material.chains.get(&item.id) {
                let _ = writeln!(out, "  {}", self.chain_text(chain));
            }
        }
        out.push('\n');
    }

    /// A cadeia de uma tarefa numa linha: a regra, os lugares e as provas,
    /// cada parte só quando a tarefa a tem; o lugar e a prova entre crases.
    fn chain_text(&self, chain: &Chain) -> String {
        let quoted = |all: &[String]| all.iter().map(|one| format!("`{one}`")).collect::<Vec<_>>().join(", ");
        let mut said: Vec<String> = Vec::new();
        if let Some(rule) = &chain.rule {
            said.push(self.t("prompt.final.chain.rule").replace("{rule}", rule.trim_end_matches('.')));
        }
        if !chain.places.is_empty() {
            said.push(self.t("prompt.final.chain.places").replace("{places}", &quoted(&chain.places)));
        }
        if !chain.proofs.is_empty() {
            said.push(self.t("prompt.final.chain.proof").replace("{proofs}", &quoted(&chain.proofs)));
        }
        said.join(" ")
    }
}
