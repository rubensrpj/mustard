//! A conferência da chave no serviço: a lista de modelos (`GET /v1/models`),
//! que não leva texto nenhum e não pergunta nada ao modelo. A chave que o
//! serviço aceita fica conferida; a que ele recusa sai com o motivo; sem rede,
//! ou com o serviço fora do ar, a chave fica sem conferência, e quem chama
//! diz isso.

use std::time::{Duration, Instant};

use mustard_core::domain::map_filter::FilterError;

use super::{CONNECT_TIMEOUT, JEV_URL, JevKey, URL_ENV, interrupted_in_time, transport_error};

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
    check_key_through(key, endpoint, limit, ureq::Agent::new_with_config)
}

/// [`check_key`] com o agente que `agent` monta da configuração de cada
/// tentativa.
fn check_key_through(key: &JevKey, endpoint: &str, limit: Duration, agent: impl Fn(ureq::config::Config) -> ureq::Agent) -> KeyCheck {
    let sent = past_interruptions(Instant::now() + limit, |left| {
        let config = ureq::Agent::config_builder()
            .timeout_connect(Some(CONNECT_TIMEOUT.min(left)))
            .timeout_global(Some(left))
            .http_status_as_error(false)
            .build();
        agent(config).get(models_url(endpoint)).header("Authorization", &format!("Bearer {}", key.0)).call()
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
/// volta interrompida dentro do prazo ([`interrupted_in_time`]).
fn past_interruptions<T>(deadline: Instant, mut call: impl FnMut(Duration) -> Result<T, ureq::Error>) -> Result<T, ureq::Error> {
    loop {
        let result = call(deadline.saturating_duration_since(Instant::now()));
        if !matches!(&result, Err(error) if interrupted_in_time(error, deadline)) {
            return result;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, ErrorKind, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use ureq::unversioned::resolver::DefaultResolver;
    use ureq::unversioned::transport::{Buffers, ConnectionDetails, Connector, DefaultConnector, NextTimeout, Transport};

    /// A conexão de verdade cuja leitura da resposta volta interrompida
    /// enquanto `left` não chegou a zero, com o mesmo erro que a conexão TCP
    /// do `ureq` devolve quando um sinal interrompe a leitura do socket.
    #[derive(Debug)]
    struct InterruptedRead {
        inner: Box<dyn Transport>,
        left: Arc<AtomicUsize>,
    }

    impl Transport for InterruptedRead {
        fn buffers(&mut self) -> &mut dyn Buffers {
            self.inner.buffers()
        }

        fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
            self.inner.transmit_output(amount, timeout)
        }

        fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
            if self.left.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| left.checked_sub(1)).is_ok() {
                return Err(std::io::Error::from(ErrorKind::Interrupted).into());
            }
            self.inner.await_input(timeout)
        }

        fn is_open(&mut self) -> bool {
            self.inner.is_open()
        }
    }

    /// Põe [`InterruptedRead`] sobre a conexão que o conector padrão abriu.
    #[derive(Debug)]
    struct Interrupting(Arc<AtomicUsize>);

    impl Connector<Box<dyn Transport>> for Interrupting {
        type Out = InterruptedRead;

        fn connect(&self, _: &ConnectionDetails, chained: Option<Box<dyn Transport>>) -> Result<Option<InterruptedRead>, ureq::Error> {
            Ok(chained.map(|inner| InterruptedRead { inner, left: Arc::clone(&self.0) }))
        }
    }

    /// Um serviço em 127.0.0.1 que lista os modelos a todo pedido e conta as
    /// linhas de pedido que chegaram.
    fn models_service() -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1/systemone", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut lines = BufReader::new(&stream).lines().map_while(Result::ok);
                let Some(request) = lines.next() else { continue };
                lines.take_while(|line| !line.is_empty()).for_each(drop);
                log.lock().unwrap().push(request);
                let body = r#"{"data":[]}"#;
                let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            }
        });
        (url, seen)
    }

    /// A conferência da chave cuja leitura da lista de modelos volta
    /// interrompida pede a lista de novo, e a chave fica conferida pela
    /// segunda resposta; sem o pedido de novo, a interrupção virava falha da
    /// rede e a chave ficava sem conferência.
    #[test]
    fn the_key_check_asks_again_when_the_models_read_is_interrupted() {
        let (url, seen) = models_service();
        let left = Arc::new(AtomicUsize::new(1));
        let checked = check_key_through(&JevKey("sk-test".into()), &url, Duration::from_secs(5), |config| {
            ureq::Agent::with_parts(config, DefaultConnector::default().chain(Interrupting(Arc::clone(&left))), DefaultResolver::default())
        });
        assert_eq!(checked, KeyCheck::Verified);
        assert_eq!(left.load(Ordering::SeqCst), 0, "the first read was interrupted");
        assert_eq!(*seen.lock().unwrap(), ["GET /v1/models HTTP/1.1"; 2], "the list was asked twice");
    }

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
