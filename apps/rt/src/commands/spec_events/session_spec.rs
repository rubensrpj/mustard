//! A ligação da conversa com a spec: a abertura e a retomada a gravam, e a
//! fala, a resposta, as chamadas e a gravação sem `--spec` a leem. As duas
//! pontas passam por aqui e pela mesma raiz, a do projeto em que as specs
//! moram ([`store::spec_root`]): a ligação gravada pela abertura na raiz é a
//! que se lê de uma subpasta, e a gravada de um worktree é a que o checkout
//! principal lê. A spec da branch não vale aqui: por ela, uma conversa
//! paralela na mesma branch gravaria na obra da outra.

use std::path::Path;

use mustard_core::domain::spec_events::Refusal;
use mustard_core::io::spec_events as store;

/// Liga a conversa da sessão `session` à spec `spec` do projeto visto de
/// `root`: a fala, a resposta e as chamadas dela passam a ir para essa spec.
/// Sem sessão, fora de uma conversa, nada é ligado.
pub(crate) fn bind(root: &Path, session: Option<&str>, spec: &str) {
    if let Some(session) = session {
        crate::shared::context::session::bind_session_spec(&store::spec_root(root).to_string_lossy(), session, spec);
    }
}

/// Solta a ligação da conversa da sessão `session` com a spec do projeto visto
/// de `root`, quando a spec fecha ou é descartada: a marca que a abertura
/// gravou na raiz do projeto é a que sai, mesmo com o comando rodado de uma
/// subpasta. Sem sessão, fora de uma conversa, nada foi ligado e nada sai.
pub(crate) fn unbind(root: &Path, session: Option<&str>) {
    if let Some(session) = session {
        crate::shared::context::session::unbind_session_spec(&store::spec_root(root).to_string_lossy(), session);
    }
}

/// A spec ligada à conversa da sessão `session`, no projeto visto de `root`,
/// ou a variável `MUSTARD_ACTIVE_SPEC`, quando dita. Sem sessão nenhuma, fora
/// de uma conversa, não há spec da conversa: a branch do checkout não a
/// decide.
pub(crate) fn bound(root: &Path, session: Option<&str>) -> Option<String> {
    crate::shared::spec_state::conversation_bound_spec(&store::spec_root(root).to_string_lossy(), session)
}

/// A spec em que a gravação do tipo `event_type` sem `--spec` entra, no
/// projeto em `root`: a ligada à conversa da sessão `session`. A conversa sem
/// ligação é recusada com o comando de retomar; sem sessão nenhuma, fora de
/// uma conversa, a recusa pede o `--spec`, como sempre.
pub(crate) fn of_conversation(root: &Path, session: Option<&str>, event_type: &str) -> Result<String, Refusal> {
    let event_type = event_type.to_string();
    let Some(session) = session else {
        return Err(Refusal::SpecRequired { event_type });
    };
    bound(root, Some(session)).ok_or(Refusal::SessionUnbound { event_type })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sem sessão, a recusa de sempre; com a sessão sem ligação, a que manda
    /// retomar; com a ligação, a spec dela.
    #[test]
    fn a_write_without_the_spec_goes_to_the_spec_of_its_conversation() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        bind(root, Some("s-ligada"), "obra");
        assert_eq!(of_conversation(root, Some("s-ligada"), "context"), Ok("obra".to_string()));
        assert_eq!(of_conversation(root, Some("s-solta"), "context"), Err(Refusal::SessionUnbound { event_type: "context".into() }));
        assert_eq!(of_conversation(root, None, "context"), Err(Refusal::SpecRequired { event_type: "context".into() }));
    }
}
