//! O comando que o teste segura até soltá-lo: um laço que espera o arquivo
//! de soltura que o teste cria depois de afirmar o que acontece com o
//! comando ainda preso.
//!
//! O laço desiste no limite de segurança das esperas dos testes
//! ([`SAFETY_LIMIT`]), contado em voltas: cada volta dura ao menos a pausa,
//! então o comando fica preso ao menos o limite, por mais folgada que a
//! máquina esteja. O limite não afirma nada sobre o tempo: só impede que o
//! teste cuja soltura nunca vem deixe o processo para trás.
//!
//! Um arquivo para todos os pacotes: os testes de dentro de `src/` o trazem
//! pelo módulo que a raiz do pacote declara só para teste, ao lado do
//! `wait_for`, de onde vem o limite.

use std::path::Path;
use std::time::Duration;

use crate::wait_for::SAFETY_LIMIT;

/// A pausa do laço entre uma olhada e outra.
const PAUSE: Duration = Duration::from_millis(20);

/// As voltas do laço até o limite de segurança.
const ROUNDS: u128 = SAFETY_LIMIT.as_millis() / PAUSE.as_millis();

/// O laço do shell POSIX que segura o comando até o arquivo `release` existir.
#[cfg(not(windows))]
pub fn held_until_released(release: &Path) -> String {
    format!(
        "n=0; while [ ! -e '{}' ] && [ $n -lt {ROUNDS} ]; do sleep {}; n=$((n+1)); done",
        release.display(),
        PAUSE.as_secs_f64()
    )
}

/// O mesmo laço no PowerShell do Windows. Sem `$`, que o shell POSIX do
/// Windows expandiria dentro das aspas.
#[cfg(windows)]
pub fn held_until_released(release: &Path) -> String {
    format!(
        "powershell -NoProfile -Command \"1..{ROUNDS} | ForEach-Object {{ if (-not (Test-Path '{}')) {{ Start-Sleep -Milliseconds {} }} }}\"",
        release.display(),
        PAUSE.as_millis()
    )
}
