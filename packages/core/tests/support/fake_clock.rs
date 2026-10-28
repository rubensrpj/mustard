//! O relógio falso dos testes: parte da hora em que nasce e só anda quando o
//! teste o manda andar. O código que recebe o relógio de fora mede com ele o
//! tempo que gastou e confere com ele o prazo, e o teste afirma o que o
//! relógio falso diz, sem depender da carga da máquina.
//!
//! Um arquivo para todos os pacotes: os testes de dentro de `src/` o trazem
//! pelo módulo que a raiz do pacote declara só para teste.

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// Um relógio que só anda por [`FakeClock::advance`].
pub struct FakeClock {
    origin: Instant,
    passed: Mutex<Duration>,
}

impl Default for FakeClock {
    fn default() -> Self {
        Self { origin: Instant::now(), passed: Mutex::new(Duration::ZERO) }
    }
}

impl FakeClock {
    /// A hora do relógio.
    pub fn now(&self) -> Instant {
        self.origin + *self.passed.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Faz o relógio andar `by`.
    pub fn advance(&self, by: Duration) {
        *self.passed.lock().unwrap_or_else(PoisonError::into_inner) += by;
    }
}
