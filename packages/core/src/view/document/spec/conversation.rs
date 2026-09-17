//! A conversa: um grupo por dia, com as mensagens, as respostas, o que os
//! ganchos e os comandos fizeram, as remoções e, marcada como substituída, a
//! versão antiga de cada item revisto. A página grande demais para o
//! claude.ai perde os registros mais antigos dela, e só eles.

use std::collections::BTreeMap;

use super::{group_of, Page};
use crate::domain::spec_events::{Block, Hidden, SpecEvent};
use crate::platform::i18n::{translate, Locale};
use crate::view::document::{Document, Item, Node, Section, Status, Tone};

/// Quantos registros a conversa da página mostra.
#[must_use]
pub fn conversation_len(doc: &Document) -> usize {
    conversation(doc).map_or(0, |section| Node::items(&section.body).len())
}

/// Tira da página os `count` registros mais antigos da conversa e diz, no
/// começo dela, quantos ficaram só no `.md`. Devolve quantos saíram; o dia
/// que fica sem registro sai junto. A página que passaria do tamanho que o
/// claude.ai aceita é a única que perde algo.
pub fn cut_oldest_conversation(doc: &mut Document, count: usize, lang: Locale) -> usize {
    let Some(section) = conversation_mut(doc) else {
        return 0;
    };
    let mut cut = 0;
    for node in &mut section.body {
        if let Node::Group(day) = node {
            day.body.retain(|entry| {
                if cut < count && matches!(entry, Node::Item(_)) {
                    cut += 1;
                    false
                } else {
                    true
                }
            });
        }
    }
    if cut == 0 {
        return 0;
    }
    section.body.retain(|node| !matches!(node, Node::Group(day) if Node::items(&day.body).is_empty()));
    section
        .body
        .insert(0, Node::Paragraph(translate("page.conversation.cut", lang).replace("{count}", &cut.to_string())));
    cut
}

fn conversation(doc: &Document) -> Option<&Section> {
    doc.body.iter().find_map(|node| match node {
        Node::Section(section) if section.anchor.as_deref() == Some(Block::Conversation.name()) => Some(section),
        _ => None,
    })
}

fn conversation_mut(doc: &mut Document) -> Option<&mut Section> {
    doc.body.iter_mut().find_map(|node| match node {
        Node::Section(section) if section.anchor.as_deref() == Some(Block::Conversation.name()) => Some(section),
        _ => None,
    })
}

impl Page<'_> {
    /// A conversa, um grupo por dia, em ordem de número: as mensagens, as
    /// respostas, o que os ganchos e os comandos fizeram, as remoções e,
    /// marcada como substituída, a versão antiga de cada item revisto que
    /// ainda vale.
    pub(super) fn conversation(&self) -> Node {
        let hidden = self.log.hidden();
        let mut entries: Vec<(u64, &str, Item)> = self
            .of_block(Block::Conversation)
            .into_iter()
            .map(|e| {
                let mut item = self.item(e, true);
                let author = e.str_field("author").map(|a| self.t(&format!("page.author.{a}")));
                item.who = Some(self.kind_and(e, author));
                (e.id, e.at(), item)
            })
            .collect();
        for event in &self.log.events {
            let replaced = matches!(hidden.get(&event.id), Some(Hidden::Replaced { .. }));
            if replaced && self.log.current(event.id).is_some() {
                let mut item = self.item(event, false);
                item.who = Some(self.kind_and(event, None));
                item.mark = Some(self.t("page.replaced").to_string());
                item.status = Some(Status { label: self.t("page.old_version").to_string(), tone: Tone::Old });
                entries.push((event.id, event.at(), item));
            }
        }
        entries.sort_by_key(|(id, _, _)| *id);
        let mut days: BTreeMap<&str, Vec<Node>> = BTreeMap::new();
        for (_, at, item) in entries {
            days.entry(at.get(..10).unwrap_or(at)).or_default().push(Node::Item(item));
        }
        let body = days
            .into_iter()
            .map(|(day, items)| {
                let shown = day.get(8..10).zip(day.get(5..7)).map_or_else(|| day.to_string(), |(d, m)| format!("{d}/{m}"));
                let title = self.t("page.group.day").replace("{day}", &shown);
                group_of(format!("conversation-{day}"), title, items)
            })
            .collect();
        self.section(Block::Conversation.name(), self.t("page.block.conversation"), body)
    }

    /// "mensagem · usuário": o tipo do evento e, quando há, quem.
    fn kind_and(&self, event: &SpecEvent, who: Option<&str>) -> String {
        let mut parts = vec![self.t(&format!("page.type.{}", event.event_type))];
        parts.extend(who);
        parts.join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::*;
    use super::{conversation_len, cut_oldest_conversation};
    use crate::domain::spec_events::parse_log;
    use crate::platform::i18n::Locale;
    use crate::view::document::{spec_document, Node, Status, Tone, WavePrompts};

    /// Uma decisão revista mostra só a versão nova no combinado; a antiga
    /// aparece só na conversa, marcada como substituída, apagada e sem ser o
    /// endereço do código.
    #[test]
    fn a_revised_decision_shows_the_new_text_and_the_old_one_only_in_the_conversation() {
        let content = [
            line(1, "message", ",\"author\":\"user\",\"text\":\"decida\""),
            line(2, "decision", ",\"text\":\"Texto antigo.\",\"keys\":[\"k\"],\"why\":\"w\",\"origin\":1"),
            line(3, "decision", ",\"text\":\"Texto novo.\",\"keys\":[\"k\"],\"why\":\"w\",\"origin\":1,\"replaces\":2"),
        ]
        .concat();
        let doc = spec_document("s", &parse_log(&content), &WavePrompts::new(), Locale::PtBr);
        let agreed = items(section(&doc, "agreed"));
        assert_eq!(agreed.len(), 1);
        assert_eq!((agreed[0].code.as_str(), agreed[0].text.as_str()), ("MSTD-DEC-0001", "Texto novo."));
        assert!(agreed[0].anchored);

        let talk = items(section(&doc, "conversation"));
        let old = talk.iter().find(|i| i.text == "Texto antigo.").expect("the old version is in the conversation");
        assert_eq!(old.code, "MSTD-DEC-0001");
        assert!(!old.anchored, "the address belongs to the new version");
        assert_eq!(old.note().as_deref(), Some("decisão · versão substituída · 2026-09-12 10:02"));
        assert_eq!(old.status, Some(Status { label: "versão antiga".into(), tone: Tone::Old }));
        for section in sections(&doc).into_iter().filter(|s| s.anchor.as_deref() != Some("conversation")) {
            assert!(items(section).iter().all(|i| i.text != "Texto antigo."), "{}", section.heading);
        }
    }

    /// A conversa sai num grupo por dia, do mais antigo ao mais novo, em
    /// ordem de número dentro de cada dia.
    #[test]
    fn the_conversation_is_grouped_by_day() {
        let said = |id: u64, day: &str| {
            format!("{{\"v\":1,\"id\":{id},\"at\":\"2026-09-{day}T10:00:00-03:00\",\"type\":\"message\",\"author\":\"user\",\"text\":\"mensagem {id}\"}}\n")
        };
        let content = [said(1, "11"), said(2, "11"), said(3, "12")].concat();
        let doc = spec_document("s", &parse_log(&content), &WavePrompts::new(), Locale::PtBr);
        let days: Vec<(String, String, usize)> = groups(section(&doc, "conversation"))
            .iter()
            .map(|g| (g.anchor.clone(), g.title.clone(), Node::items(&g.body).len()))
            .collect();
        assert_eq!(
            days,
            [
                ("conversation-2026-09-11".to_string(), "Dia 11/09".to_string(), 2),
                ("conversation-2026-09-12".to_string(), "Dia 12/09".to_string(), 1),
            ]
        );
    }

    /// Cortar a conversa tira os registros mais antigos, e o dia que fica sem
    /// registro sai junto; ela diz quantos ficaram só no `.md`, e o resto da
    /// página fica igual.
    #[test]
    fn cutting_the_conversation_drops_the_oldest_entries_and_says_how_many() {
        let said = |id: u64, day: &str| {
            format!("{{\"v\":1,\"id\":{id},\"at\":\"2026-09-{day}T10:0{id}:00-03:00\",\"type\":\"message\",\"author\":\"user\",\"text\":\"mensagem {id}\"}}\n")
        };
        let content = [said(1, "11"), said(2, "11"), said(3, "12"), said(4, "12"), said(5, "12")].concat();
        let full = spec_document("s", &parse_log(&content), &WavePrompts::new(), Locale::PtBr);
        assert_eq!(conversation_len(&full), 5);
        let mut doc = full.clone();
        assert_eq!(cut_oldest_conversation(&mut doc, 2, Locale::PtBr), 2);
        assert_eq!(conversation_len(&doc), 3);
        let talk = section(&doc, "conversation");
        let Node::Paragraph(said) = &talk.body[0] else { panic!("{talk:?}") };
        assert!(said.starts_with("Os 2 registros mais antigos da conversa ficaram só no `spec.md`"), "{said}");
        let days: Vec<&str> = groups(talk).iter().map(|g| g.anchor.as_str()).collect();
        assert_eq!(days, ["conversation-2026-09-12"], "the emptied day leaves too");
        let texts: Vec<&str> = items(talk).iter().map(|i| i.text.as_str()).collect();
        assert_eq!(texts, ["mensagem 3", "mensagem 4", "mensagem 5"]);
        let last = doc.body.len() - 1;
        assert_eq!(doc.body[..last], full.body[..last], "only the conversation changes");
        let mut none = full.clone();
        assert_eq!(cut_oldest_conversation(&mut none, 0, Locale::PtBr), 0);
        assert_eq!(none, full);
    }
}
