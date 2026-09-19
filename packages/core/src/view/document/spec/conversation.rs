//! A conta e o corte dos registros mais antigos da conversa: a página grande
//! demais para o claude.ai perde só eles.

use crate::domain::spec_events::Block;
use crate::platform::i18n::{translate, Locale};
use crate::view::document::{Document, Node, Section};

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

#[cfg(test)]
mod tests {
    use super::{conversation_len, cut_oldest_conversation};
    use crate::platform::i18n::Locale;
    use crate::view::document::{Document, Group, Item, Node, Section};

    fn message(code: &str, day: &str, text: &str) -> (String, Node) {
        (
            day.to_string(),
            Node::Item(Item {
                code: code.to_string(),
                anchored: true,
                title: String::new(),
                status: None,
                who: None,
                mark: None,
                date: None,
                text: text.to_string(),
                fields: Vec::new(),
            }),
        )
    }

    /// A forma que a página da spec dava à conversa: um grupo por dia dentro
    /// da seção "conversation".
    fn doc_with_days(entries: Vec<(String, Node)>) -> Document {
        let mut days: Vec<(String, Vec<Node>)> = Vec::new();
        for (day, item) in entries {
            match days.iter_mut().find(|(d, _)| *d == day) {
                Some((_, items)) => items.push(item),
                None => days.push((day.clone(), vec![item])),
            }
        }
        let groups = days
            .into_iter()
            .map(|(day, items)| {
                Node::Group(Group {
                    anchor: format!("conversation-{day}"),
                    title: format!("Dia {day}"),
                    status: None,
                    summary: String::new(),
                    open: false,
                    body: items,
                })
            })
            .collect();
        Document {
            lang: "pt-BR".into(),
            kind: None,
            title: "s".into(),
            meta: Vec::new(),
            footer: None,
            body: vec![Node::Section(Section { anchor: Some("conversation".into()), heading: "Conversa".into(), body: groups })],
        }
    }

    fn section(doc: &Document) -> &Section {
        let Some(Node::Section(section)) = doc.body.first() else { panic!("{doc:?}") };
        section
    }

    fn groups(section: &Section) -> Vec<&Group> {
        section.body.iter().filter_map(|n| if let Node::Group(g) = n { Some(g) } else { None }).collect()
    }

    /// Quantos registros a conversa tem, pela seção "conversation" da página.
    #[test]
    fn conversation_len_counts_every_item_across_the_days() {
        let doc = doc_with_days(vec![
            message("MSTD-MSG-0001", "11", "mensagem 1"),
            message("MSTD-MSG-0002", "11", "mensagem 2"),
            message("MSTD-MSG-0003", "12", "mensagem 3"),
        ]);
        assert_eq!(conversation_len(&doc), 3);
        let empty = Document { body: Vec::new(), ..doc };
        assert_eq!(conversation_len(&empty), 0, "a page without a conversation section has no entries");
    }

    /// Cortar a conversa tira os registros mais antigos, e o dia que fica sem
    /// registro sai junto; ela diz quantos ficaram só no `.md`, e o resto da
    /// página fica igual. Sem nada a cortar, a página não muda.
    #[test]
    fn cutting_the_conversation_drops_the_oldest_entries_and_says_how_many() {
        let full = doc_with_days(vec![
            message("MSTD-MSG-0001", "11", "mensagem 1"),
            message("MSTD-MSG-0002", "11", "mensagem 2"),
            message("MSTD-MSG-0003", "12", "mensagem 3"),
            message("MSTD-MSG-0004", "12", "mensagem 4"),
            message("MSTD-MSG-0005", "12", "mensagem 5"),
        ]);
        assert_eq!(conversation_len(&full), 5);
        let mut doc = full.clone();
        assert_eq!(cut_oldest_conversation(&mut doc, 2, Locale::PtBr), 2);
        assert_eq!(conversation_len(&doc), 3);
        let talk = section(&doc);
        let Node::Paragraph(said) = &talk.body[0] else { panic!("{talk:?}") };
        assert!(said.starts_with("Os 2 registros mais antigos da conversa ficaram só no `spec.md`"), "{said}");
        let days: Vec<&str> = groups(talk).iter().map(|g| g.anchor.as_str()).collect();
        assert_eq!(days, ["conversation-12"], "the emptied day leaves too");
        let texts: Vec<&str> = Node::items(&talk.body).iter().map(|i| i.text.as_str()).collect();
        assert_eq!(texts, ["mensagem 3", "mensagem 4", "mensagem 5"]);

        let mut none = full.clone();
        assert_eq!(cut_oldest_conversation(&mut none, 0, Locale::PtBr), 0);
        assert_eq!(none, full);
    }
}
