//! A pausa antes de o teste afirmar que uma coisa não aconteceu: o processo
//! em segundo plano que não devia nascer ganha a chance de aparecer.
//!
//! A pausa nunca decide que o teste passa, nem que cai pelo tempo: na máquina
//! carregada, o processo errado só pode deixar de aparecer a tempo, e o teste
//! deixa de acusar o erro, sem nunca cair sem defeito. Nenhuma afirmação sobre
//! o que aconteceu espera por ela; essa espera o evento (`wait_for`).
//!
//! Um arquivo para todos os pacotes: os testes da pasta `tests/` o trazem
//! pelo caminho, e os de dentro de `src/`, pelo módulo que a raiz do pacote
//! declara só para teste.

use std::time::Duration;

/// O tempo que o processo que não devia nascer tem para aparecer.
const ROOM: Duration = Duration::from_millis(400);

/// Dá ao que não devia acontecer a chance de aparecer.
pub fn room_for_what_must_not_happen() {
    std::thread::sleep(ROOM);
}
