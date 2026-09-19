//! A lista dos itens combinados sem dono, que o usuário confere antes de os
//! donos serem gravados: cada item na mesma linha da página da spec, com o
//! dono que recebe e de onde ele veio, num grupo por regra.

use super::{code_span, join, one_line, Page};
use crate::domain::spec_events::{SpecEvent, SpecLog};
use crate::domain::wave_prompt::{Owner, OwnerFrom, OwnerLine};
use crate::platform::i18n::{translate, Locale};
use crate::view::document::{Document, Group, Item, Meta, Node, Status, Tone};

/// A lista dos itens combinados sem dono, para o usuário conferir antes de
/// os donos serem gravados: cada item na mesma linha da página da spec, com o
/// dono que recebe e de onde ele veio, num grupo por regra. Sem item sem
/// dono, a página diz isso.
#[must_use]
pub fn owners_page(spec: &str, log: &SpecLog, lines: &[OwnerLine], lang: Locale) -> Document {
    let page = Page::new(log, lang);
    let mut body = vec![Node::Paragraph(page.t("page.owners.intro").to_string())];
    if lines.is_empty() {
        body.push(Node::Paragraph(page.t("page.owners.none").to_string()));
    }
    for rule in ["tasks", "cited", "files", "orchestrator", "nothing"] {
        let items: Vec<Node> = lines
            .iter()
            .filter(|line| owner_rule_key(&line.from) == rule)
            .filter_map(|line| Some(Node::Item(page.owner_item(log.get(line.item)?, line))))
            .collect();
        if items.is_empty() {
            continue;
        }
        body.push(Node::Group(Group {
            anchor: format!("owners-{rule}"),
            title: page.t(&format!("page.owners.group.{rule}")).to_string(),
            status: None,
            summary: String::new(),
            open: false,
            body: items,
        }));
    }
    let count = |rule: &str| lines.iter().filter(|line| owner_rule_key(&line.from) == rule).count();
    let proposed = count("tasks") + count("cited") + count("files");
    let tally = page
        .t("page.owners.tally")
        .replace("{unowned}", &lines.len().to_string())
        .replace("{proposed}", &proposed.to_string())
        .replace("{given}", &count("orchestrator").to_string())
        .replace("{left}", &count("nothing").to_string());
    Document {
        lang: lang.as_str().to_string(),
        kind: Some(page.t("page.kind.owners").to_string()),
        title: spec.to_string(),
        meta: vec![
            Meta::Pair { label: page.t("page.meta.spec").to_string(), value: spec.to_string() },
            Meta::Note(tally),
        ],
        body: vec![page.section("owners", page.t("page.owners.heading"), body)],
        footer: None,
    }
}

/// O nome da regra de onde veio o dono: o do grupo na página e o da saída do
/// comando.
#[must_use]
pub fn owner_rule_key(from: &OwnerFrom) -> &'static str {
    match from {
        OwnerFrom::Tasks(_) => "tasks",
        OwnerFrom::Cited => "cited",
        OwnerFrom::Files(_) => "files",
        OwnerFrom::Orchestrator(_) => "orchestrator",
        OwnerFrom::Nothing => "nothing",
    }
}

/// O dono em palavras: "projeto", "onda 14" ou "ondas 14, 15".
#[must_use]
pub fn owner_label(owner: &Owner, lang: Locale) -> String {
    match owner {
        Owner::Project => translate("page.owners.project", lang).to_string(),
        Owner::Waves(waves) if waves.len() == 1 => {
            let n = waves.iter().next().map(u64::to_string).unwrap_or_default();
            translate("page.owners.wave", lang).replace("{n}", &n)
        }
        Owner::Waves(waves) => {
            translate("page.owners.waves", lang).replace("{waves}", &join(waves.iter().map(u64::to_string)))
        }
    }
}

impl Page {
    /// Um item da lista dos sem dono: a linha da página da spec, com o dono
    /// como situação e, antes dos outros campos, o dono e de onde ele veio.
    fn owner_item(&self, event: &SpecEvent, line: &OwnerLine) -> Item {
        let mut item = self.item(event, true);
        let (owner, tone) = match &line.owner {
            Some(owner) => (owner_label(owner, self.lang), Tone::Plain),
            None => (self.t("page.owners.missing").to_string(), Tone::Bad),
        };
        let from = match &line.from {
            OwnerFrom::Tasks(tasks) => {
                self.t("page.owners.from.tasks").replace("{tasks}", &join(tasks.iter().map(|id| self.code(*id))))
            }
            OwnerFrom::Files(files) => {
                self.t("page.owners.from.files").replace("{files}", &join(files.iter().map(|f| code_span(f))))
            }
            OwnerFrom::Orchestrator(why) => self.t("page.owners.from.orchestrator").replace("{why}", &one_line(why)),
            OwnerFrom::Cited => self.t("page.owners.from.cited").to_string(),
            OwnerFrom::Nothing => self.t("page.owners.from.nothing").to_string(),
        };
        item.status = Some(Status { label: owner.clone(), tone });
        item.fields.splice(0..0, [self.field("page.owners.owner", owner), self.field("page.owners.from", from)]);
        item
    }
}
