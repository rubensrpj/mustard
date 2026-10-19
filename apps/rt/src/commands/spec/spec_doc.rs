//! O endereço em que a página da unidade foi publicada.
//!
//! O gerador antigo da página saiu com o comando que o chamava: a página e o
//! `.md` saem hoje do motor do arquivo de eventos. Ficou só a leitura do
//! endereço publicado, que a barra de status transforma em link — e ela lê o
//! próprio arquivo de eventos, onde a publicação é gravada como evento, e não
//! um arquivo à parte na pasta da spec, que a regra de três arquivos proíbe.

use mustard_core::domain::spec_events::SpecLog;

/// O endereço da última publicação da página da spec cujo arquivo de eventos
/// é `log`, ou nada quando a unidade nunca foi publicada. Quem chama já leu o
/// arquivo: a barra de status o lê uma vez só para o endereço, a fase e as
/// ondas.
///
/// É a mesma leitura da linha da spec no índice: a última publicação da
/// página da spec que deu certo. Republicar troca o endereço mostrado sem que
/// nada precise ser apagado, e a publicação da página do projeto, gravada na
/// mesma spec, não conta.
#[must_use]
pub fn published_url(log: &SpecLog) -> Option<String> {
    mustard_core::domain::spec_index::spec_page_url(log).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::published_url;
    use crate::shared::spec_state::{DiskSpecState, seed_event};
    use mustard_core::domain::spec_state::SpecState as _;
    use serde_json::json;
    use tempfile::tempdir;

    /// Sem publicação nenhuma não há endereço; publicada duas vezes, vale o
    /// endereço da última. A publicação da página do projeto, gravada na
    /// mesma spec, não troca o link da página da spec.
    #[test]
    fn address_of_the_last_publication_counts() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let url = || published_url(&DiskSpecState::new(root).log("x").unwrap());
        seed_event(root, "x", "state", json!({"phase": "running"}));
        assert_eq!(url(), None, "sem publicação, nenhum endereço");

        seed_event(root, "x", "publish", json!({"page": "spec", "milestone": "round", "ok": true, "url": "https://claude.ai/a"}));
        seed_event(root, "x", "publish", json!({"page": "spec", "milestone": "round", "ok": true, "url": "https://claude.ai/b"}));
        assert_eq!(url().as_deref(), Some("https://claude.ai/b"));
        seed_event(root, "x", "publish", json!({"page": "project", "milestone": "round", "ok": true, "url": "https://claude.ai/p"}));
        assert_eq!(url().as_deref(), Some("https://claude.ai/b"));
    }
}
