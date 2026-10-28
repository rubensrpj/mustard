//! Os envios sem volta: as ondas cujo pedido despachado ainda espera a
//! entrega do agente. É a leitura única que a rodada, a gravação de eventos e
//! a revisão fazem para saber se o agente de uma onda ainda trabalha e qual
//! onda saiu do ar por ficar sem tarefa; mora abaixo de todas elas, que só a
//! leem.

use std::collections::{BTreeMap, BTreeSet};

use super::{Block, BlockQuery, SpecEvent, SpecLog};

impl SpecLog {
    /// As ondas do plano ([`Self::planned_waves`]) com pedido despachado e sem
    /// volta, cada uma com o número do pedido, inclusive a que ganhou versão
    /// nova do plano depois do pedido: a onda que o agente ainda trabalha
    /// continua dona da cópia dela, mesmo com o plano mudado, até sair de novo
    /// ou voltar.
    #[must_use]
    pub fn unanswered_sends(&self) -> BTreeMap<u64, u64> {
        let planned = self.planned_waves();
        self.sends_without_return().into_iter().filter(|(n, _)| planned.contains(n)).collect()
    }

    /// `true` quando o agente da onda `wave` ainda pode entregar: ela tem envio
    /// sem volta ([`Self::unanswered_sends`]), mesmo com o plano mudado depois
    /// dele, e nenhuma entrega esperando a rodada ([`Self::waves_returned`]).
    /// As duas leituras ficam: a volta que o agente grava sai da leitura até a
    /// rodada assumi-la, e até lá o envio segue sem volta, guardando a vaga e
    /// a cópia da onda (a volta que a junção segurou por conflito ainda
    /// precisa dela). É a leitura da gravação de uma tarefa, que não pode
    /// tirar da onda o que o agente entrega. Ao contrário do andamento que a
    /// rodada lê, não olha se o Claude Code que mandou o pedido segue aberto:
    /// a onda cujo agente caiu sem voltar segue dona das tarefas dela até sair
    /// de novo ou voltar.
    #[must_use]
    pub fn agent_working(&self, wave: u64) -> bool {
        self.unanswered_sends().contains_key(&wave) && !self.waves_returned().contains(&wave)
    }

    /// As ondas que saíram do ar por ficar sem tarefa, cada uma com o número do
    /// pedido dela: a onda tem pedido sem volta, continua gravada, e nenhuma
    /// tarefa viva aponta mais o número dela — a tarefa foi removida, ou
    /// regravada fora da onda. Sem tarefa ela sai do plano
    /// ([`Self::planned_waves`]) e, pela mesma leitura, do andamento da rodada
    /// em toda chamada: rodada com relatório ou com despacho, retomada e fila.
    #[must_use]
    pub fn waves_left_without_tasks(&self) -> BTreeMap<u64, u64> {
        let planned = self.planned_waves();
        let recorded: BTreeSet<u64> =
            self.block(BlockQuery::Block(Block::Waves)).into_iter().filter(|e| e.event_type == "wave").filter_map(SpecEvent::wave).collect();
        self.sends_without_return().into_iter().filter(|(n, _)| recorded.contains(n) && !planned.contains(n)).collect()
    }

    /// As ondas com pedido despachado e sem volta, cada uma com o número do
    /// pedido, estejam ou não no plano: a leitura que
    /// [`Self::unanswered_sends`] restringe às do plano e que
    /// [`Self::waves_left_without_tasks`] restringe às que saíram dele.
    fn sends_without_return(&self) -> BTreeMap<u64, u64> {
        let verdicts = self.verdicts_by_wave();
        let dispatches = self.last_dispatch_by_wave();
        let mut deliveries: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
        for delivered in self.block(BlockQuery::Block(Block::Waves)).into_iter().filter(|e| e.event_type == "delivered") {
            if let Some(n) = delivered.wave() {
                deliveries.entry(n).or_default().push(delivered.id);
            }
        }
        self.last_by_wave("send")
            .into_iter()
            .filter(|(n, sent)| {
                // A entrega e o veredito se comparam com o lugar em que o envio
                // despachou a onda, e não com a versão mais nova dele: a versão
                // que só traz o consumo, gravada depois de uma entrega ou de uma
                // reprovação, não reabre a onda.
                let sent = dispatches.get(n).copied().unwrap_or(*sent);
                let ids = deliveries.get(n).map(Vec::as_slice).unwrap_or_default();
                if ids.iter().any(|id| *id > sent) {
                    return false;
                }
                let judged_before = verdicts.get(n).and_then(|list| list.iter().rev().find(|v| v.id < sent)).and_then(|v| v.str_field("result"));
                !ids.iter().any(|id| *id < sent) || judged_before == Some("rejected")
            })
            .collect()
    }
}
