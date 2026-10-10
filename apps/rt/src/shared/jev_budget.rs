//! `jev_budget` — o que sobra do teto de gasto do Jev no mês, dentro de um
//! processo. É a porta de toda chamada ao serviço: o filtro só manda o pedido
//! depois de [`Budget::reserve`], e a reserva que passa do que sobra recusa o
//! pedido com [`FilterError::OverBudget`]. Quem chamou segue como seguiria sem
//! chave, e a recusa fica marcada ([`Budget::refused`]) para o aviso.
//!
//! O que sobra se lê do disco uma vez, ao abrir ([`Budget::open`]): o teto do
//! `mustard.json` menos o gasto do mês nas specs do projeto e no arquivo do
//! gasto da máquina. Dali em diante cada reserva desconta do que sobra, e
//! chamadas ao mesmo tempo, como as dos itens de várias ondas, nunca passam
//! juntas do teto. Outro processo que chame ao mesmo tempo só aparece no gasto
//! gravado, depois: o teto se segura pelo gasto que já está gravado.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use mustard_core::ProjectConfig;
use mustard_core::domain::map_filter::FilterError;
use mustard_core::io::{jev_gate, spend};

/// O que sobra do teto do mês, em milionésimos de dólar, dividido entre as
/// cópias do mesmo filtro, e se ele já recusou uma chamada.
#[derive(Debug, Clone)]
pub struct Budget {
    left: Arc<Mutex<u64>>,
    refused: Arc<AtomicBool>,
}

impl Budget {
    /// O que sobra do teto deste mês para o projeto em `root`: o teto de
    /// `config` menos o gasto do mês, nas specs dele e no arquivo do gasto da
    /// máquina em `ledger_dir`, quando há.
    #[must_use]
    pub fn open(root: &Path, config: &ProjectConfig, ledger_dir: Option<&Path>) -> Self {
        Self::of(jev_gate::left_in_month(root, config, ledger_dir, &spend::this_month()))
    }

    /// O teto com `left_micro_usd` ainda por gastar.
    #[must_use]
    pub fn of(left_micro_usd: u64) -> Self {
        Self { left: Arc::new(Mutex::new(left_micro_usd)), refused: Arc::new(AtomicBool::new(false)) }
    }

    /// Se o teto já acabou: nada mais sobra para uma chamada.
    #[must_use]
    pub fn is_spent(&self) -> bool {
        self.left() == 0
    }

    /// Reserva `micro_usd` para uma chamada que vai sair. A que custaria mais
    /// do que sobra não reserva nada e volta [`FilterError::OverBudget`].
    ///
    /// # Errors
    /// O gasto do mês mais a chamada passaria do teto.
    pub fn reserve(&self, micro_usd: u64) -> Result<(), FilterError> {
        let mut left = self.left.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if micro_usd > *left {
            self.refused.store(true, Ordering::Relaxed);
            return Err(FilterError::OverBudget);
        }
        *left -= micro_usd;
        Ok(())
    }

    /// Release an estimate reserved for a miss that another process filled
    /// before this process acquired the cache lock. No HTTP call took place.
    pub(crate) fn release_cached(&self, micro_usd: u64) {
        let mut left = self.left.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *left = left.saturating_add(micro_usd);
    }

    /// Se o teto já recusou uma chamada que passaria do que sobra: o Jev
    /// ficou de fora dela, e quem chamou seguiu sem ele.
    #[must_use]
    pub fn refused(&self) -> bool {
        self.refused.load(Ordering::Relaxed)
    }

    fn left(&self) -> u64 {
        *self.left.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
