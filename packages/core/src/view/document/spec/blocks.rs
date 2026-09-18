//! Os blocos que saem um grupo por tipo: a especificação e o combinado, na
//! ordem dos tipos, com cada ponto do levantamento já fechado mostrando o
//! ponto que o fechou, e os critérios, cada um com o resultado da última
//! execução, e depois as execuções.

use std::collections::BTreeMap;

use super::{group_of, Page};
use crate::domain::spec_events::{Block, TYPES};
use crate::domain::survey;
use crate::view::document::Node;

impl Page<'_> {
    /// Um grupo por tipo do bloco, na ordem dos tipos.
    pub(super) fn by_type(&self, block: Block) -> Node {
        let body = TYPES
            .iter()
            .filter(|t| t.block == block)
            .filter_map(|spec| {
                let title = self.t(&format!("page.group.{}", spec.name)).to_string();
                let mut node =
                    self.tallied(format!("{}-{}", block.name(), spec.name), title, &self.of_type(spec.name))?;
                if spec.name == "point" {
                    self.link_closings(&mut node);
                }
                Some(node)
            })
            .collect();
        self.section(block.name(), self.t(&format!("page.block.{}", block.name())), body)
    }

    /// O ponto do levantamento que outro fechou mostra o código do ponto que o
    /// fechou, que a página escreve como link, do mesmo jeito que o critério
    /// mostra a última execução. Os pares saem da leitura única dos pontos, a
    /// mesma que conta os abertos e os fechados.
    fn link_closings(&self, node: &mut Node) {
        let Node::Group(group) = node else {
            return;
        };
        let closed_by: BTreeMap<String, String> = survey::points(self.log)
            .into_iter()
            .filter_map(|point| Some((self.code(point.original()?.id), self.code(point.closing()?.id))))
            .collect();
        for child in &mut group.body {
            if let Node::Item(item) = child
                && let Some(closing) = closed_by.get(&item.code)
            {
                item.fields.push(self.field("page.field.closed_by", closing.clone()));
            }
        }
    }

    /// Os critérios, cada um com o resultado da última execução, e depois as
    /// execuções, cada parte no seu grupo.
    pub(super) fn criteria(&self) -> Node {
        let runs = self.of_type("criterion_run");
        let criteria: Vec<Node> = self
            .of_type("criterion")
            .into_iter()
            .map(|criterion| {
                let mut item = self.item(criterion, true);
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
        let mut body = Vec::new();
        if !criteria.is_empty() {
            body.push(group_of("criteria-criterion".into(), self.t("page.group.criterion").into(), criteria));
        }
        body.extend(self.group("criteria-runs".into(), self.t("page.group.criterion_run").into(), &runs));
        self.section(Block::Criteria.name(), self.t("page.block.criteria"), body)
    }
}
