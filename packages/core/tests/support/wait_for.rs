//! A espera dos testes pelo evento de outro processo ou de outra linha de
//! execução: o arquivo que um processo em segundo plano escreve, o pedido que
//! chega a um serviço falso.
//!
//! O evento é a prova, e a máquina carregada só alonga o teste. A espera tem
//! um limite de segurança largo ([`SAFETY_LIMIT`]), que não afirma nada sobre
//! o tempo: só impede que o defeito que barra o evento deixe a suíte inteira
//! parada, sem mensagem. No limite, o teste cai dizendo o evento que
//! esperava. O comando que o teste segura até soltá-lo (`held_command`)
//! desiste no mesmo limite. A pausa entre uma olhada e outra só poupa o
//! processador.
//!
//! Um arquivo para todos os pacotes: os testes da pasta `tests/` o trazem
//! pelo caminho, e os de dentro de `src/`, pelo módulo que a raiz do pacote
//! declara só para teste.

use std::time::{Duration, Instant};

/// O limite de segurança de toda espera dos testes: largo o bastante para que
/// nenhuma máquina carregada chegue a ele sem defeito.
pub const SAFETY_LIMIT: Duration = Duration::from_secs(120);

/// A pausa entre uma olhada e outra.
const PAUSE: Duration = Duration::from_millis(10);

/// O que `look` acha, olhando de novo até ele achar alguma coisa. No limite
/// de segurança, o teste cai com `what`, o evento esperado.
#[track_caller]
pub fn wait_for<T>(what: &str, look: impl FnMut() -> Option<T>) -> T {
    wait_for_within(SAFETY_LIMIT, what, look)
}

/// [`wait_for`] com o limite `limit` no lugar do limite de segurança.
#[track_caller]
pub fn wait_for_within<T>(limit: Duration, what: &str, mut look: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + limit;
    loop {
        if let Some(found) = look() {
            return found;
        }
        assert!(Instant::now() < deadline, "o evento esperado não veio em {limit:?}, o limite de segurança da espera: {what}");
        std::thread::sleep(PAUSE);
    }
}
