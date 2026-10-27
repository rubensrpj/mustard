//! As entregas que esperam a rodada: a leitura única que a rodada assume e
//! que a retomada e o bloco de retomada contam como voltas à espera.

use std::collections::BTreeSet;

use super::{SpecEvent, SpecLog};

impl SpecLog {
    /// As entregas que o agente gravou depois do envio que despachou a onda
    /// delas e que ninguém assumiu, em ordem de número: a última volta de cada
    /// onda ([`Self::unassumed_returns`]) gravada depois do envio mais novo
    /// que despachou a onda ([`Self::last_dispatch_by_wave`]). A volta de
    /// antes do envio, superada por um reenvio, não conta. A da onda que saiu
    /// do plano conta: o agente dela terminou, e a rodada não a lê como onda
    /// em andamento nem órfã. Quem conta só as que a rodada vai assumir lê
    /// [`Self::returns_awaiting_round`].
    #[must_use]
    pub fn returns_after_dispatch(&self) -> Vec<&SpecEvent> {
        let dispatched = self.last_dispatch_by_wave();
        self.unassumed_returns()
            .into_iter()
            .filter(|event| event.event_type == "delivered")
            .filter(|event| event.wave().is_some_and(|n| event.id > dispatched.get(&n).copied().unwrap_or_default()))
            .collect()
    }

    /// As ondas que voltaram e esperam a rodada: as das entregas depois do
    /// envio ([`Self::returns_after_dispatch`]) — a que pede novo plano sem o
    /// clique do usuário, a que uma conferência da própria volta segurou, ou a
    /// que a junção segurou por conflito. A onda que saiu do plano depois de
    /// voltar segue aqui: o agente dela terminou.
    #[must_use]
    pub fn waves_returned(&self) -> BTreeSet<u64> {
        self.returns_after_dispatch().into_iter().filter_map(SpecEvent::wave).collect()
    }

    /// As entregas que o agente gravou e que esperam a rodada, em ordem de
    /// número: as voltas depois do envio ([`Self::returns_after_dispatch`]) de
    /// onda que segue no plano ([`Self::planned_waves`]). A da onda que saiu
    /// do plano — a de lote que ficou sem tarefa depois da volta, ou a
    /// removida — não espera: fica na história, a rodada nunca a assume, e a
    /// retomada não a conta.
    #[must_use]
    pub fn returns_awaiting_round(&self) -> Vec<&SpecEvent> {
        let planned = self.planned_waves();
        self.returns_after_dispatch().into_iter().filter(|event| event.wave().is_some_and(|n| planned.contains(&n))).collect()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::SpecLog;
    use crate::domain::spec_events::parse_log;

    fn log_of(lines: &[Value]) -> SpecLog {
        parse_log(&lines.iter().map(Value::to_string).collect::<Vec<_>>().join("\n"))
    }

    fn ids(events: &[&super::SpecEvent]) -> Vec<u64> {
        events.iter().map(|event| event.id).collect()
    }

    /// A volta gravada antes de um reenvio da mesma onda foi superada por ele:
    /// nenhuma leitura de voltas a lista. A volta que o agente do reenvio
    /// grava depois dele espera a rodada.
    #[test]
    fn a_return_recorded_before_a_resend_of_its_wave_no_longer_awaits_the_round() {
        let mut lines = vec![
            json!({"v":1,"id":1,"at":"t","type":"wave","n":2,"text":"Duas."}),
            json!({"v":1,"id":2,"at":"t","type":"task","wave":2,"text":"Tarefa."}),
            json!({"v":1,"id":3,"at":"t","type":"send","wave":2,"role":"wave","text":"Pedido."}),
            json!({"v":1,"id":4,"at":"t","type":"delivered","wave":2,"text":"Saiu.","returned":true}),
        ];
        assert_eq!(ids(&log_of(&lines).returns_awaiting_round()), vec![4], "a volta depois do envio espera a rodada");

        lines.push(json!({"v":1,"id":5,"at":"t","type":"send","wave":2,"role":"wave","text":"Pedido.","resends":3}));
        let resent = log_of(&lines);
        assert_eq!(ids(&resent.returns_awaiting_round()), Vec::<u64>::new(), "o reenvio supera a volta de antes dele");
        assert_eq!(ids(&resent.returns_after_dispatch()), Vec::<u64>::new(), "o reenvio supera a volta de antes dele");

        lines.push(json!({"v":1,"id":6,"at":"t","type":"delivered","wave":2,"text":"Saiu de novo.","returned":true}));
        assert_eq!(ids(&log_of(&lines).returns_awaiting_round()), vec![6], "a volta do reenvio espera a rodada");
    }

    /// A onda de lote que ficou sem tarefa saiu do plano: a volta dela, gravada
    /// depois do envio, segue entre as voltas depois do envio, mas não espera
    /// a rodada.
    #[test]
    fn a_return_of_a_wave_off_the_plan_is_after_its_dispatch_but_does_not_await_the_round() {
        let log = log_of(&[
            json!({"v":1,"id":1,"at":"t","type":"wave","n":2,"text":"Duas."}),
            json!({"v":1,"id":2,"at":"t","type":"task","wave":2,"text":"Tarefa."}),
            json!({"v":1,"id":3,"at":"t","type":"wave","n":3,"text":"Lote.","author":"binary"}),
            json!({"v":1,"id":4,"at":"t","type":"send","wave":2,"role":"wave","text":"Pedido."}),
            json!({"v":1,"id":5,"at":"t","type":"send","wave":3,"role":"wave","text":"Pedido."}),
            json!({"v":1,"id":6,"at":"t","type":"delivered","wave":2,"text":"Saiu.","returned":true}),
            json!({"v":1,"id":7,"at":"t","type":"delivered","wave":3,"text":"Saiu.","returned":true}),
        ]);
        assert_eq!(ids(&log.returns_after_dispatch()), vec![6, 7]);
        assert_eq!(ids(&log.returns_awaiting_round()), vec![6], "a onda 3 não tem tarefa e saiu do plano");
    }
}
