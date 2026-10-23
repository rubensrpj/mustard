//! As entregas que esperam a rodada: a leitura única que a rodada assume e
//! que a retomada e o bloco de retomada contam como voltas à espera.

use super::{SpecEvent, SpecLog};

impl SpecLog {
    /// As entregas que o agente gravou e que esperam a rodada, em ordem de
    /// número: a última volta de cada onda que ninguém assumiu
    /// ([`Self::unassumed_returns`]), gravada depois do envio que despachou a
    /// onda ([`Self::last_dispatch_by_wave`]), de onda que segue no plano
    /// ([`Self::planned_waves`]). A volta de antes do envio, superada por um
    /// reenvio, não espera. A da onda que saiu do plano — a de lote que ficou
    /// sem tarefa depois da volta, ou a removida — também não: fica na
    /// história, a rodada nunca a assume, e a retomada não a conta.
    #[must_use]
    pub fn returns_awaiting_round(&self) -> Vec<&SpecEvent> {
        let planned = self.planned_waves();
        let dispatched = self.last_dispatch_by_wave();
        self.unassumed_returns()
            .into_iter()
            .filter(|event| event.event_type == "delivered")
            .filter(|event| event.wave().is_some_and(|n| planned.contains(&n) && event.id > dispatched.get(&n).copied().unwrap_or_default()))
            .collect()
    }
}
