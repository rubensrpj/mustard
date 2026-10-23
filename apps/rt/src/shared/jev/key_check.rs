//! A conferência da chave no serviço: a lista de modelos (`GET /v1/models`),
//! que não leva texto nenhum e não pergunta nada ao modelo. A chave que o
//! serviço aceita fica conferida; a que ele recusa sai com o motivo; sem rede,
//! ou com o serviço fora do ar, a chave fica sem conferência, e quem chama
//! diz isso.

use std::io::ErrorKind;
use std::time::{Duration, Instant};

use mustard_core::domain::map_filter::FilterError;

use super::{CONNECT_TIMEOUT, JEV_URL, JevKey, URL_ENV, transport_error};

/// O que o serviço disse da chave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KeyCheck {
    /// O serviço listou os modelos com a chave.
    Verified,
    /// O serviço recusou a chave, com o motivo (`key_refused`, `no_credit`).
    Refused(&'static str),
    /// O serviço não respondeu sobre a chave, com o motivo (`network`,
    /// `timeout`, `busy`, [`SERVICE_ERROR`]).
    Unchecked(&'static str),
}

/// O motivo da lista de modelos que voltou com erro do serviço (endereço
/// errado, 404, ou falha dele, 5xx): nada diz da chave, e a palavra não se
/// confunde com a recusa dela.
const SERVICE_ERROR: &str = "service_error";

/// O endereço do serviço: o que [`URL_ENV`] diz ou, sem ele, [`JEV_URL`].
pub(crate) fn service_url() -> String {
    std::env::var(URL_ENV).ok().map(|url| url.trim().to_string()).filter(|url| !url.is_empty()).unwrap_or_else(|| JEV_URL.to_string())
}

/// O endereço da lista de modelos ao lado do endereço `endpoint` das
/// perguntas: o último trecho do caminho troca por `models`.
fn models_url(endpoint: &str) -> String {
    let endpoint = endpoint.trim_end_matches('/');
    let path_from = endpoint.find("://").map_or(0, |at| at + 3);
    match endpoint[path_from..].rfind('/') {
        Some(at) => format!("{}/models", &endpoint[..path_from + at]),
        None => format!("{endpoint}/v1/models"),
    }
}

/// Confere a chave `key` na lista de modelos do serviço em `endpoint`,
/// esperando no máximo `limit`.
pub(crate) fn check_key(key: &JevKey, endpoint: &str, limit: Duration) -> KeyCheck {
    let sent = past_interruptions(Instant::now() + limit, |left| {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(CONNECT_TIMEOUT.min(left)))
            .timeout_global(Some(left))
            .http_status_as_error(false)
            .build()
            .new_agent();
        agent.get(models_url(endpoint)).header("Authorization", &format!("Bearer {}", key.0)).call()
    });
    match sent {
        Ok(response) if response.status().is_success() => KeyCheck::Verified,
        Ok(response) => match response.status().as_u16() {
            status @ 401..=403 => KeyCheck::Refused(FilterError::Refused { status }.reason()),
            status @ 429 => KeyCheck::Unchecked(FilterError::Refused { status }.reason()),
            _ => KeyCheck::Unchecked(SERVICE_ERROR),
        },
        Err(error) => KeyCheck::Unchecked(transport_error(error).reason()),
    }
}

/// `call` com o tempo que falta até `deadline`, de novo enquanto a leitura
/// volta interrompida por um sinal do processo e o prazo não acabou. A
/// interrupção não é falha da rede: no Linux, a leitura de um socket com
/// prazo volta interrompida a cada sinal tratado ou a cada processo parado e
/// retomado, mesmo com o pedido de recomeçar sozinho.
fn past_interruptions<T>(deadline: Instant, mut call: impl FnMut(Duration) -> Result<T, ureq::Error>) -> Result<T, ureq::Error> {
    let mut left = deadline.saturating_duration_since(Instant::now());
    loop {
        let result = call(left);
        left = deadline.saturating_duration_since(Instant::now());
        let interrupted = matches!(&result, Err(ureq::Error::Io(io)) if io.kind() == ErrorKind::Interrupted);
        if !interrupted || left.is_zero() {
            return result;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A leitura interrompida por um sinal sai de novo dentro do prazo, e a
    /// resposta da segunda vez é a que vale; depois do prazo, a interrupção
    /// volta como está, sem outra tentativa.
    #[test]
    fn an_interrupted_read_is_asked_again_only_within_the_wait() {
        let interrupted = || ureq::Error::Io(std::io::Error::from(ErrorKind::Interrupted));
        let mut calls = 0;
        let answered = past_interruptions(Instant::now() + Duration::from_secs(5), |_| {
            calls += 1;
            if calls == 1 { Err(interrupted()) } else { Ok("models") }
        });
        assert_eq!(answered.ok(), Some("models"));
        assert_eq!(calls, 2, "one interruption, one more call");

        let mut late = 0;
        let gave_up = past_interruptions(Instant::now(), |_| {
            late += 1;
            Err::<(), _>(interrupted())
        });
        assert!(matches!(gave_up, Err(ureq::Error::Io(io)) if io.kind() == ErrorKind::Interrupted));
        assert_eq!(late, 1, "past the wait, the interruption is the answer");
    }

    #[test]
    fn the_models_list_sits_beside_the_question_endpoint() {
        assert_eq!(models_url(JEV_URL), "https://api.typesafe.ai/v1/models");
        assert_eq!(models_url("http://127.0.0.1:9/v1/systemone/"), "http://127.0.0.1:9/v1/models");
        assert_eq!(models_url("http://127.0.0.1:9"), "http://127.0.0.1:9/v1/models");
    }
}
