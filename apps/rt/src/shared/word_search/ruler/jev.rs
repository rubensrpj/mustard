//! O que o filtro (o Jev) fez em cada busca da régua e a soma dele: quantas
//! buscas o chamaram, quantas falharam, quantos tokens e quantos dólares.
//!
//! O produto mede a chamada em `measured` ([`crate::shared::search_door::classify`])
//! e só a spec do projeto a guarda; a régua não tem spec. O que o gancho mede
//! chega aqui pelo mesmo caminho da ordem da triagem e das linhas achadas: uma
//! lembrança por linha de execução, que o gancho preenche durante a busca e
//! que a régua lê e esvazia ao fim dela.

use std::cell::RefCell;

use mustard_core::domain::map_filter::FilterCandidate;
use serde_json::{json, Map, Value};

use crate::shared::search_door::{Classified, Outcome};

/// O que o filtro fez numa busca.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct JevUse {
    /// O pedido ao filtro saiu: o banco trouxe candidato e o filtro foi
    /// chamado, mesmo que a chamada tenha falhado.
    pub(super) called: bool,
    /// Por que a chamada falhou, numa palavra; `None` quando ele respondeu.
    pub(super) failure: Option<&'static str>,
    /// Os tokens de entrada cobrados.
    pub(super) tokens: u64,
    /// O custo da chamada, em milionésimos de dólar.
    pub(super) cost_micro_usd: u64,
    /// As peças que o corte do filtro guardou.
    pub(super) kept: usize,
    /// As peças que as ligações das guardadas puxaram.
    pub(super) pulled: usize,
    /// O arquivo de cada candidato que foi ao filtro.
    pub(super) sent: Vec<String>,
}

impl JevUse {
    /// Os campos da busca no resultado da régua.
    pub(super) fn fields(&self) -> Map<String, Value> {
        let mut fields = Map::new();
        fields.insert("jev_called".to_string(), json!(self.called));
        fields.insert("jev_failure".to_string(), json!(self.failure));
        fields.insert("jev_tokens".to_string(), json!(self.tokens));
        fields.insert("jev_cost_micro_usd".to_string(), json!(self.cost_micro_usd));
        fields.insert("jev_kept".to_string(), json!(self.kept));
        fields.insert("jev_pulled".to_string(), json!(self.pulled));
        fields
    }
}

thread_local! {
    /// Os arquivos dos candidatos que o último pedido levou ao filtro.
    static SENT: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// O que o filtro fez na última busca; nada quando ela não o chamou.
    static LAST: RefCell<Option<JevUse>> = const { RefCell::new(None) };
}

/// Guarda os candidatos que a busca leva ao filtro, para a régua dizer se o
/// arquivo certo estava entre eles.
pub(crate) fn remember_candidates(candidates: &[FilterCandidate]) {
    let paths = candidates.iter().map(|candidate| candidate.path.clone()).collect();
    SENT.with(|sent| *sent.borrow_mut() = paths);
}

/// Guarda o que o filtro respondeu à busca: se foi chamado, a falha, o que
/// custou e as peças que voltaram.
pub(crate) fn remember_call(classified: &Classified) {
    let measured = |key: &str| classified.measured.get(key).and_then(Value::as_u64).unwrap_or(0);
    let (called, failure, pieces) = match &classified.outcome {
        Outcome::NoCandidates => (false, None, [].as_slice()),
        Outcome::Classified { pieces, .. } => (true, None, pieces.as_slice()),
        Outcome::Failed(error) => (true, Some(error.reason()), [].as_slice()),
    };
    let kept = pieces.iter().filter(|piece| piece.score.is_some()).count();
    let used = JevUse {
        called,
        failure,
        tokens: measured("tokens"),
        cost_micro_usd: measured("cost_micro_usd"),
        kept,
        pulled: pieces.len() - kept,
        sent: SENT.with(|sent| sent.take()),
    };
    LAST.with(|last| *last.borrow_mut() = Some(used));
}

/// Esvazia a lembrança, antes de a régua rodar a próxima busca.
pub(super) fn forget() {
    SENT.with(|sent| sent.borrow_mut().clear());
    LAST.with(|last| *last.borrow_mut() = None);
}

/// O que o filtro fez na busca que acabou de rodar, e esvazia a lembrança.
pub(super) fn take() -> JevUse {
    LAST.with(|last| last.borrow_mut().take()).unwrap_or_default()
}

/// A soma do filtro num grupo de buscas.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct JevSum {
    calls: usize,
    failures: usize,
    tokens: u64,
    cost_micro_usd: u64,
}

impl JevSum {
    /// Soma uma busca; a que não chamou o filtro não soma nada.
    pub(super) fn record(&mut self, jev: &JevUse) {
        if !jev.called {
            return;
        }
        self.calls += 1;
        self.failures += usize::from(jev.failure.is_some());
        self.tokens += jev.tokens;
        self.cost_micro_usd += jev.cost_micro_usd;
    }

    /// A soma como a linha do resultado a diz, com os dólares divididos pelas
    /// `searches` buscas do grupo.
    pub(super) fn show(&self, searches: usize) -> String {
        let dollars = self.cost_micro_usd as f64 / 1_000_000.0;
        let per_search = if searches == 0 { 0.0 } else { dollars / searches as f64 };
        format!(
            "Jev: chamaram {}, falharam {}, tokens {}, US$ {dollars:.4}, US$ {per_search:.6} por busca",
            self.calls, self.failures, self.tokens
        )
    }
}
