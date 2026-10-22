//! A spec da gravação sem `--spec`: a que a abertura ou a retomada ligou à
//! conversa que grava. A spec da branch não vale aqui: por ela, uma conversa
//! paralela na mesma branch gravaria na obra da outra.

use std::path::Path;

use mustard_core::domain::spec_events::Refusal;

/// A spec em que a gravação do tipo `event_type` sem `--spec` entra, no
/// projeto em `root`: a ligada à conversa da sessão `session`. A conversa sem
/// ligação é recusada com o comando de retomar; sem sessão nenhuma, fora de
/// uma conversa, a recusa pede o `--spec`, como sempre.
pub(crate) fn of_conversation(root: &Path, session: Option<&str>, event_type: &str) -> Result<String, Refusal> {
    let event_type = event_type.to_string();
    let Some(session) = session else {
        return Err(Refusal::SpecRequired { event_type });
    };
    crate::shared::spec_state::conversation_bound_spec(&root.to_string_lossy(), Some(session))
        .ok_or(Refusal::SessionUnbound { event_type })
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
        crate::shared::context::session::bind_session_spec(&root.to_string_lossy(), "s-ligada", "obra");
        assert_eq!(of_conversation(root, Some("s-ligada"), "context"), Ok("obra".to_string()));
        assert_eq!(of_conversation(root, Some("s-solta"), "context"), Err(Refusal::SessionUnbound { event_type: "context".into() }));
        assert_eq!(of_conversation(root, None, "context"), Err(Refusal::SpecRequired { event_type: "context".into() }));
    }
}
