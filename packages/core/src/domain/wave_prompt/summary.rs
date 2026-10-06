//! O resumo que acompanha as tarefas de uma onda: a entrega de um agente que
//! encheu a conversa, em cinco blocos — estado, feito, decidido, fatos e
//! dúvidas —, com as tarefas que ele não começou. Todo pedido que leva uma
//! dessas tarefas abre com ele num bloco em destaque, só pelo código da
//! entrega — o texto do resumo nunca entra —, manda lê-lo antes de tudo e
//! não refazer o que ele dá como feito e decidido; o resumo é um item da lista
//! de leitura, e a entrega é recusada enquanto ele não foi lido de dentro da
//! cópia.

use std::fmt::Write as _;

use super::{code_of, Writer};
use crate::domain::spec_events::{BlockQuery, SpecEvent, SpecLog};

/// O resumo que a onda `wave` continua: a entrega que o envio mais novo dela
/// cita (`summary`) ou, antes do envio, a que o evento da onda traz, na
/// versão vigente. `None` na onda que não continua trabalho nenhum, e quando
/// o número gravado já não aponta uma entrega que a leitura mostra.
#[must_use]
pub fn summary_of(log: &SpecLog, wave: u64) -> Option<&SpecEvent> {
    let sent = log.last_by_wave("send").get(&wave).and_then(|id| log.get(*id)).and_then(|send| send.int("summary"));
    let formed = || {
        log.block(BlockQuery::Wave(wave))
            .into_iter()
            .find(|event| event.event_type == "wave")
            .and_then(|event| event.int("summary"))
    };
    sent.or_else(formed).and_then(|id| log.current(id)).filter(|event| event.event_type == "delivered")
}

impl Writer<'_> {
    /// O bloco em destaque do começo do pedido: "trabalho já começado", o
    /// código do resumo e a ordem de lê-lo antes de tudo, de dentro da cópia,
    /// e de não refazer o que ele dá como feito e decidido. Sem resumo, nada.
    pub(super) fn started(&self, out: &mut String, summary: Option<&SpecEvent>) {
        let Some(summary) = summary else { return };
        let execution = &self.material.execution;
        let root = if execution.copy.is_some() && !execution.root.is_empty() {
            format!("--root {} ", execution.root)
        } else {
            String::new()
        };
        let line = self
            .t("wave_prompt.summary.read")
            .replace("{code}", &code_of(self.material, summary))
            .replace("{root}", &root)
            .replace("{spec}", &self.material.spec);
        let _ = writeln!(out, "## {}\n\n{line}\n", self.t("wave_prompt.summary.title"));
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::*;
    use crate::domain::spec_events::{normalize, parse_log, render_line, stamp};
    use crate::domain::wave_prompt::{language_line, listed, write, Material, WaveCopy};
    use crate::platform::i18n::{translate, Locale};

    /// O que a entrega que parou conta de si: o texto inteiro dela nunca vai
    /// no pedido da onda seguinte.
    const WHAT_IT_SAID: &str = "O painel ficou pela metade, falta o total do dia.";

    /// Um arquivo de eventos escrito à mão, uma linha por evento.
    fn log(events: &[(&str, Value)]) -> SpecLog {
        let mut content = String::new();
        for (i, (event_type, body)) in events.iter().enumerate() {
            let mut map = normalize(body.as_object().cloned().unwrap_or_default(), event_type);
            map.insert("type".into(), json!(event_type));
            content.push_str(&render_line(&stamp(map, i as u64 + 1, None, "2026-10-03T10:00:00-03:00")));
            content.push('\n');
        }
        parse_log(&content)
    }

    /// A onda 1 parou e deixou uma tarefa por fazer (a entrega 4, o resumo);
    /// a onda 2 a continua: a de número 5, com o resumo no evento da onda.
    fn after_a_stop(sent: Option<Value>) -> SpecLog {
        let mut events = vec![
            ("wave", json!({"n": 1, "text": "Uma.", "criteria": [], "done_when": "pronto"})),
            ("task", json!({"wave": 1, "text": "Somar o dia.", "files": [{"path": "src/a.rs"}]})),
            ("task", json!({"wave": 1, "text": "Mostrar o total.", "files": [{"path": "src/b.rs"}]})),
            ("delivered", json!({"wave": 1, "text": WHAT_IT_SAID, "files": ["src/a.rs"], "undone": ["MSTD-TASK-0003"]})),
            ("wave", json!({"n": 2, "text": "Duas.", "criteria": [], "done_when": "pronto", "summary": 4})),
            ("task", json!({"wave": 2, "text": "Mostrar o total.", "files": [{"path": "src/b.rs"}]})),
        ];
        if let Some(send) = sent {
            events.push(("send", send));
        }
        log(&events)
    }

    fn material_of<'a>(log: &'a SpecLog, wave: u64, summary: Option<&'a SpecEvent>) -> Material<'a> {
        Material {
            spec: "teste".into(),
            wave,
            block: log.block(BlockQuery::Wave(wave)),
            codes: log.codes(),
            summary,
            ..Material::default()
        }
    }

    /// O pedido da onda que continua um resumo abre, logo depois da linha dos
    /// idiomas e antes de tudo, com o bloco "trabalho já começado": o código
    /// da entrega e o comando que a lê, nos dois idiomas, e nunca o texto
    /// dela; na cópia, o comando leva o repositório principal, de onde a spec
    /// se lê. O resumo é o primeiro item da lista de leitura; a onda sem
    /// resumo não tem o bloco nem o item.
    #[test]
    fn the_request_opens_with_the_summary_code_and_never_its_text() {
        let log = after_a_stop(None);
        let summary = summary_of(&log, 2).expect("the wave continues the summary");
        let code = log.codes()[&summary.id].clone();
        for lang in [Locale::PtBr, Locale::EnUs] {
            let text = write(&material_of(&log, 2, Some(summary)), lang);
            let title = format!("## {}", translate("wave_prompt.summary.title", lang));
            let started = text.find(&title).unwrap_or_else(|| panic!("no block: {text}"));
            let delivers = text.find(&format!("## {}", translate("prompt.part.delivers", lang))).unwrap_or(text.len());
            let languages = text.find(&language_line(&Default::default())).expect("the languages line");
            assert!(languages < started && started < delivers, "{lang:?}: the block comes right after the languages: {text}");
            assert!(text.contains(&format!("run read item-{code} --spec teste")), "{lang:?}: {text}");
            assert!(!text.contains(WHAT_IT_SAID), "{lang:?}: the text of the summary never goes in: {text}");

            let mut in_copy = material_of(&log, 2, Some(summary));
            in_copy.execution.root = "/repo".into();
            in_copy.execution.copy = Some(WaveCopy { path: "/repo/copy-1".into(), reused: None });
            let text = write(&in_copy, lang);
            assert!(text.contains(&format!("run read item-{code} --root /repo --spec teste")), "{lang:?}: {text}");

            let bare = write(&material_of(&log, 2, None), lang);
            assert!(!bare.contains(&title), "{lang:?}: no summary, no block: {bare}");
        }
        let with = listed(&material_of(&log, 2, Some(summary)));
        assert_eq!(with.first(), Some(&code), "the summary is the first item to read: {with:?}");
        assert!(!listed(&material_of(&log, 2, None)).contains(&code));
    }

    /// O resumo da onda é o que o envio mais novo cita; antes do envio, o do
    /// evento da onda. Só a entrega vale: o número que aponta outra coisa,
    /// ou uma entrega removida da leitura, deixa a onda sem resumo.
    #[test]
    fn the_send_names_the_summary_before_the_wave_event_does_and_only_a_delivery_counts() {
        let formed = after_a_stop(None);
        assert_eq!(summary_of(&formed, 2).map(|event| event.id), Some(4), "before the send, the wave event names it");
        assert_eq!(summary_of(&formed, 1), None, "the first wave continues nothing");

        let sent = after_a_stop(Some(json!({"wave": 2, "role": "wave", "text": "Pedido.", "summary": 1})));
        assert_eq!(summary_of(&sent, 2), None, "the send wins, and the number it names is not a delivery");

        let same = after_a_stop(Some(json!({"wave": 2, "role": "wave", "text": "Pedido.", "summary": 4})));
        assert_eq!(summary_of(&same, 2).map(|event| event.id), Some(4));
    }
}
