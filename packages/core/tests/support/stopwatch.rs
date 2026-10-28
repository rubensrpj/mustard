//! O cronômetro das rodadas de medição: o tempo de verdade que uma medição
//! grava no relato dela, como a demora de cada busca medida.
//!
//! Nenhum teste afirma sobre o que ele lê: o número só vai para o relato da
//! medição, que a pessoa lê. O teste que precisa afirmar sobre o tempo usa o
//! relógio falso (`fake_clock`).

use std::time::{Duration, Instant};

/// Um cronômetro que parte na hora em que nasce.
pub struct Stopwatch(Instant);

impl Stopwatch {
    /// O cronômetro, partindo agora.
    #[must_use]
    pub fn start() -> Self {
        Self(Instant::now())
    }

    /// O tempo desde a partida.
    #[must_use]
    pub fn read(&self) -> Duration {
        self.0.elapsed()
    }
}
