//! Que specs estão abertas, lido pelo arquivo de eventos de cada uma.
//!
//! O comando que listava as specs saiu — quem lista hoje é o `read` sem spec,
//! pelo índice. Ficou a leitura que o portão da base faz: quais unidades
//! ainda estão em andamento.
//!
//! Uma regra só, a mesma que a trava da aprovação usa: a fase gravada no
//! `spec.ndjson`. Uma spec sem fase terminal está aberta; fechada, com pull
//! request aberto, entregue ou descartada, não está. Enquanto isso era lido do
//! cabeçalho do `.md` e de um arquivo de metadados ao lado, a contagem que
//! barrava uma edição e a lista que o portão relatava podiam discordar sobre a
//! mesma spec.

use std::path::Path;

use mustard_core::domain::spec_state::SpecState as _;
use mustard_core::io::claude_paths::ClaudePaths;

use crate::shared::spec_state::DiskSpecState;

/// As fases em que uma spec já não está em andamento.
const TERMINAIS: &[&str] = &["closed", "pr_open", "delivered", "discarded"];

/// O nome de uma pasta de spec sem o prefixo de data que as antigas carregam
/// (`2026-05-27-`), para comparar com um slug derivado do pedido.
///
/// `pub(crate)` porque quem COMPARA um nome de diretório de spec com um slug
/// derivado do intent precisa da mesma regra: a derivação nunca produz a data,
/// então uma comparação byte a byte com o diretório erra em toda spec datada —
/// e a unidade passa a se acusar de sobrepor a si mesma.
pub(crate) fn without_spec_date_prefix(name: &str) -> &str {
    let bytes = name.as_bytes();
    let datado = bytes.len() > 11
        && bytes[..10]
            .iter()
            .enumerate()
            .all(|(i, b)| if i == 4 || i == 7 { *b == b'-' } else { b.is_ascii_digit() })
        && bytes.get(10) == Some(&b'-');
    if datado {
        &name[11..]
    } else {
        name
    }
}

/// Os nomes das specs ainda em andamento, em ordem, na árvore de trabalho.
///
/// `pub(crate)` porque o portão da base pergunta o que está aberto para cruzar
/// com o pedido da unidade sendo aberta, e um segundo enumerador ali seria uma
/// terceira leitura de "o que está ativo" neste repositório.
#[must_use]
pub(crate) fn active_spec_names(root: &Path) -> Vec<String> {
    let Ok(paths) = ClaudePaths::for_project(root) else {
        return Vec::new();
    };
    let Ok(entradas) = std::fs::read_dir(paths.spec_dir()) else {
        return Vec::new();
    };
    let estado = DiskSpecState::new(root);
    let mut nomes: Vec<String> = entradas
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|nome| {
            let Some(log) = estado.log(nome) else {
                return false;
            };
            let fase = mustard_core::domain::spec_state::lock_state_of(Some(&log))
                .and_then(|state| state.phase);
            match fase {
                Some(f) => !TERMINAIS.contains(&f),
                None => true,
            }
        })
        .collect();
    nomes.sort();
    nomes
}

#[cfg(test)]
mod tests {
    use super::{active_spec_names, without_spec_date_prefix};
    use crate::shared::spec_state::seed_event;
    use serde_json::json;
    use tempfile::tempdir;

    /// A data na frente do nome sai; um nome sem ela volta inteiro.
    #[test]
    fn o_prefixo_de_data_sai_do_nome() {
        assert_eq!(without_spec_date_prefix("2026-05-27-uma-spec"), "uma-spec");
        assert_eq!(without_spec_date_prefix("uma-spec"), "uma-spec");
        assert_eq!(without_spec_date_prefix("2026-05-27"), "2026-05-27");
    }

    /// Uma spec em andamento conta; uma fechada não, e a lista sai ordenada.
    #[test]
    fn so_conta_a_spec_que_ainda_nao_terminou() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_event(root, "b-aberta", "state", json!({ "phase": "running" }));
        seed_event(root, "a-aberta", "state", json!({ "phase": "plan" }));
        seed_event(root, "c-fechada", "state", json!({ "phase": "closed" }));

        assert_eq!(active_spec_names(root), vec!["a-aberta", "b-aberta"]);
    }
}
