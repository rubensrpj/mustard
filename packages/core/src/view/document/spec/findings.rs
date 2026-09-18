//! O que o plano achou e as anotações. As anotações que a conferência do
//! plano gravou, reconhecidas pelo rótulo do achado, saem numa seção só delas,
//! um grupo por assunto; as outras ficam nas anotações.

use serde_json::Value;

use super::Page;
use crate::domain::mustard_id;
use crate::domain::spec_events::{Block, SpecEvent, TYPES};
use crate::platform::i18n::{translate, Locale};
use crate::view::document::Node;

/// A anotação que nasceu de um achado da conferência do plano: é a que leva o
/// rótulo do achado, escrito pelo `plan` no idioma do projeto. O rótulo é
/// reconhecido nos dois idiomas, para a página achar a anotação gravada antes
/// de o projeto trocar de idioma.
pub(super) fn is_plan_finding(event: &SpecEvent) -> bool {
    event.event_type == "note"
        && event.str_field("label").is_some_and(|label| {
            [Locale::PtBr, Locale::EnUs].iter().any(|lang| translate("plan.finding.label", *lang) == label)
        })
}

/// Os assuntos do que o plano achou, na ordem da página: o tipo do item de
/// que o achado fala e o nome do grupo.
pub(super) const SUBJECTS: &[(&str, &str)] = &[
    ("task", "tasks"),
    ("wave", "waves"),
    ("criterion", "criteria"),
    ("rule", "rules"),
    ("decision", "decisions"),
    ("skill", "skills"),
    ("", "others"),
];

impl Page<'_> {
    /// "O que o plano achou": as anotações que a conferência do plano gravou,
    /// um grupo por assunto. Sem achado nenhum, não há seção.
    pub(super) fn findings(&self) -> Option<Node> {
        let events: Vec<&SpecEvent> =
            self.of_block(Block::Notes).into_iter().filter(|e| is_plan_finding(e)).collect();
        if events.is_empty() {
            return None;
        }
        let body = SUBJECTS
            .iter()
            .filter_map(|(_, subject)| {
                let of: Vec<&SpecEvent> = events.iter().copied().filter(|e| self.subject(e) == *subject).collect();
                let title = self.t(&format!("page.findings.{subject}")).to_string();
                self.group(format!("findings-{subject}"), title, &of)
            })
            .collect();
        Some(self.section("findings", self.t("page.findings.heading"), body))
    }

    /// De que o achado fala: da skill ou das ondas, pelo tipo do achado; senão,
    /// do primeiro item que o texto cita; senão, de outra coisa.
    fn subject(&self, event: &SpecEvent) -> &'static str {
        let kind = event
            .fields
            .get("keys")
            .and_then(Value::as_array)
            .and_then(|keys| keys.first())
            .and_then(Value::as_str)
            .unwrap_or_default();
        if kind.starts_with("skill-") {
            return "skills";
        }
        if kind.starts_with("wave-") || kind.starts_with("waves-") {
            return "waves";
        }
        let text = event.str_field("text").unwrap_or_default();
        let cited = mustard_id::find(text)
            .into_iter()
            .find_map(|(start, end)| mustard_id::parse(&text[start..end]).map(|(code, _)| code))
            .and_then(|code| TYPES.iter().find(|t| t.code == code))
            .map_or("", |t| t.name);
        SUBJECTS.iter().find(|(name, _)| !name.is_empty() && *name == cited).map_or("others", |(_, subject)| subject)
    }

    /// As anotações que não são achado do plano, num grupo só.
    pub(super) fn notes(&self) -> Node {
        let rest: Vec<&SpecEvent> = self.of_block(Block::Notes).into_iter().filter(|e| !is_plan_finding(e)).collect();
        let title = self.t("page.block.notes");
        let body = self.group("notes-all".to_string(), title.to_string(), &rest).into_iter().collect();
        self.section(Block::Notes.name(), title, body)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::*;
    use crate::domain::spec_events::parse_log;
    use crate::platform::i18n::Locale;
    use crate::view::document::{spec_document, Node, Section, WavePrompts};

    /// A anotação com o rótulo do achado do plano sai numa seção só dela,
    /// logo antes das anotações, num grupo por assunto; a anotação sem o
    /// rótulo continua nas anotações. Sem achado nenhum, a seção não aparece.
    #[test]
    fn what_the_plan_found_gets_its_own_section_right_before_the_notes() {
        let found = ",\"label\":\"achado do plano\"";
        let content = [
            line(1, "message", ",\"author\":\"user\",\"text\":\"combine\""),
            line(2, "note", ",\"text\":\"Anotação de sempre.\",\"keys\":[\"k\"],\"origin\":1"),
            line(3, "note", &format!(",\"author\":\"binary\",\"text\":\"A tarefa não diz o arquivo.\",\"keys\":[\"k\"],\"origin\":1{found}")),
        ]
        .concat();
        let doc = spec_document("s", &parse_log(&content), &WavePrompts::new(), Locale::PtBr);
        let named: Vec<(&str, &str)> = sections(&doc)
            .iter()
            .map(|s| (s.anchor.as_deref().unwrap_or_default(), s.heading.as_str()))
            .collect();
        assert_eq!(named[6], ("findings", "O que o plano achou"), "{named:?}");
        assert_eq!(named[7], ("notes", "Anotações"), "a seção vem logo antes das anotações");
        let texts = |section: &Section| -> Vec<String> { items(section).iter().map(|i| i.text.clone()).collect() };
        assert_eq!(texts(section(&doc, "findings")), ["A tarefa não diz o arquivo."]);
        assert_eq!(texts(section(&doc, "notes")), ["Anotação de sempre."]);
        let found = items(section(&doc, "findings"))[0];
        assert!(
            found.fields.iter().all(|f| f.value != "achado do plano"),
            "o rótulo já é o título da seção: {found:?}"
        );

        // Em inglês a seção sai com o título de lá, e a mesma anotação nela.
        let english = spec_document("s", &parse_log(&content), &WavePrompts::new(), Locale::EnUs);
        assert_eq!(section(&english, "findings").heading, "What the plan found");

        // Sem achado nenhum, a página volta a ter oito seções.
        let only_notes = [line(1, "message", ",\"author\":\"user\",\"text\":\"combine\""), line(2, "note", ",\"text\":\"Anotação de sempre.\",\"keys\":[\"k\"],\"origin\":1")].concat();
        let doc = spec_document("s", &parse_log(&only_notes), &WavePrompts::new(), Locale::PtBr);
        assert_eq!(sections(&doc).len(), 8, "sem achado, nenhuma seção a mais");
    }

    /// O que o plano achou sai num grupo por assunto, na ordem da página:
    /// o assunto é o do item que o achado cita primeiro; o achado de skill e
    /// o das ondas vão pelo tipo do achado; o resto, para os outros.
    #[test]
    fn the_findings_are_grouped_by_subject() {
        let finding = |id: u64, key: &str, text: &str| {
            line(id, "note", &format!(",\"author\":\"binary\",\"label\":\"achado do plano\",\"keys\":[\"{key}\"],\"text\":\"{text}\",\"origin\":1"))
        };
        let content = [
            line(1, "message", ",\"author\":\"user\",\"text\":\"combine\""),
            line(2, "rule", ",\"text\":\"Regra.\",\"keys\":[\"k\"],\"example\":\"e\",\"origin\":1"),
            line(3, "wave", ",\"n\":1,\"text\":\"Onda.\",\"criteria\":[],\"done_when\":\"d\",\"origin\":1"),
            line(4, "task", ",\"wave\":1,\"text\":\"Tarefa.\",\"origin\":1"),
            finding(5, "item-without-task", "Nenhuma tarefa diz que cobre o item MSTD-RULE-0001."),
            finding(6, "task-without-file", "A tarefa MSTD-TASK-0001 não diz em que arquivo ela mexe."),
            finding(7, "skill-missing-path", "add-run-command: A skill cita caminhos que não existem."),
            finding(8, "wave-prompt-too-long", "O pedido da onda 1 tem 558 linhas."),
            finding(9, "odd", "Algo sem código."),
        ]
        .concat();
        let doc = spec_document("s", &parse_log(&content), &WavePrompts::new(), Locale::PtBr);
        let found: Vec<(String, String, Vec<String>)> = groups(section(&doc, "findings"))
            .iter()
            .map(|g| (g.anchor.clone(), g.title.clone(), Node::items(&g.body).iter().map(|i| i.code.clone()).collect()))
            .collect();
        let row = |a: &str, t: &str, code: &str| (a.to_string(), t.to_string(), vec![code.to_string()]);
        assert_eq!(
            found,
            [
                row("findings-tasks", "Sobre tarefas", "MSTD-NOTE-0002"),
                row("findings-waves", "Sobre ondas", "MSTD-NOTE-0004"),
                row("findings-rules", "Sobre regras", "MSTD-NOTE-0001"),
                row("findings-skills", "Sobre skills", "MSTD-NOTE-0003"),
                row("findings-others", "Outros", "MSTD-NOTE-0005"),
            ]
        );
    }
}
