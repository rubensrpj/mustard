//! O resumo que uma onda continua: a entrega de uma onda que parou, com o que
//! ela fez, o que aprendeu do código e as tarefas que ficaram por fazer. O
//! pedido da onda seguinte abre com ele num bloco em destaque, só pelo código
//! da entrega — o texto do resumo nunca entra —, manda lê-lo antes de tudo e
//! fazer só o que falta; o resumo é um item da lista de leitura, e a entrega é
//! recusada enquanto ele não foi lido de dentro da cópia.

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
    /// e de fazer só o que falta. Sem resumo, nada.
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
