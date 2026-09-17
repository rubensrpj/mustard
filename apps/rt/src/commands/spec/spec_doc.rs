//! O endereço em que a página da unidade foi publicada.
//!
//! O gerador antigo da página saiu com o comando que o chamava: a página e o
//! `.md` saem hoje do motor do arquivo de eventos. Ficou só a leitura do
//! endereço publicado, que a barra de status transforma em link — e ela lê o
//! próprio arquivo de eventos, onde a publicação é gravada como evento, e não
//! um arquivo à parte na pasta da spec, que a regra de três arquivos proíbe.

use std::path::Path;

use mustard_core::domain::spec_events::{Block, BlockQuery};
use mustard_core::domain::spec_state::SpecState as _;

use crate::shared::spec_state::DiskSpecState;

/// O endereço da última publicação da página da spec `slug`, ou nada quando a
/// unidade nunca foi publicada.
///
/// Lê o bloco de estado do arquivo de eventos e fica com o último evento de
/// publicação que traz endereço: republicar troca o endereço mostrado sem que
/// nada precise ser apagado.
#[must_use]
pub fn published_url(root: &Path, slug: &str) -> Option<String> {
    let log = DiskSpecState::new(root).log(slug)?;
    log.block(BlockQuery::Block(Block::State))
        .iter()
        .filter(|event| event.event_type == "publish")
        .filter_map(|event| event.str_field("url"))
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .next_back()
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::published_url;
    use crate::shared::spec_state::seed_event;
    use serde_json::json;
    use tempfile::tempdir;

    /// Sem publicação nenhuma não há endereço; publicada duas vezes, vale o
    /// endereço da última.
    #[test]
    fn vale_o_endereco_da_ultima_publicacao() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_event(root, "x", "state", json!({"phase": "running"}));
        assert_eq!(published_url(root, "x"), None, "sem publicação, nenhum endereço");

        seed_event(root, "x", "publish", json!({"page": "spec", "milestone": "round", "ok": true, "url": "https://claude.ai/a"}));
        seed_event(root, "x", "publish", json!({"page": "spec", "milestone": "round", "ok": true, "url": "https://claude.ai/b"}));
        assert_eq!(published_url(root, "x").as_deref(), Some("https://claude.ai/b"));
    }
}
