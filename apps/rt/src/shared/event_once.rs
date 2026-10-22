//! O arquivo de marcas da vez de um evento: abre, trava, lê, grava.
//!
//! A DECISÃO não mora aqui. Qual é a chave do evento, quanto tempo ela vale e
//! quem fica com a vez é regra, e regra mora no núcleo
//! ([`mustard_core::domain::event_once`]), onde se testa sem tocar o disco.
//! Este módulo só faz o que precisa de disco e de relógio.
//!
//! QUEM É DESEMPATADO. Só o gancho que o hospedeiro chamou por uma raiz de
//! plugin, que é quem tem `CLAUDE_PLUGIN_ROOT` no ambiente. A chamada de
//! terminal e o teste não têm essa variável e nunca são travados: fora do
//! hospedeiro, repetir o mesmo evento com o mesmo pedido é legítimo, e há
//! teste que conta com isso.
//!
//! FALHA ABERTA, como todo caminho deste binário: sem sessão, sem raiz de
//! plugin, sem pasta temporária ou com a trava presa, a vez é de quem
//! perguntou.

use mustard_core::domain::event_once::{decide, key_of, marks_name, session_of};
use mustard_core::domain::model::contract::HookInput;
use mustard_core::io::fs::lock::LockedFile;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Quanto se espera a trava do lado soltar. A gravação é uma linha curta: o
/// lado termina em microssegundos, e esperar por ele é mais barato que perder
/// a própria vez.
const PATIENCE: Duration = Duration::from_millis(250);

/// A vez de tratar `event` é de quem pergunta.
///
/// `false` só quando outra raiz de plugin já tratou este mesmo evento, da
/// mesma sessão, sobre o mesmo pedido, agora. Quem recebe `false` não grava,
/// não injeta e não barra: sai calado.
pub(crate) fn first_run(event: &str, input: &HookInput) -> bool {
    first_run_from(event, input, plugin_root())
}

/// [`first_run`] com a raiz de plugin já lida: é por aqui que o teste prova a
/// vez sem mexer no ambiente, que este binário não deixa.
fn first_run_from(event: &str, input: &HookInput, root: Option<String>) -> bool {
    let Some(root) = root else { return true };
    let Some(session) = session_of(input) else { return true };
    let key = key_of(event, input);
    claim(&marks_path(&session), &key, &root, now_millis()).unwrap_or(true)
}

/// A raiz de plugin que o hospedeiro deu a esta execução. `None` fora do
/// hospedeiro: chamada de terminal, teste, agente.
fn plugin_root() -> Option<String> {
    let root = std::env::var("CLAUDE_PLUGIN_ROOT").ok()?;
    let root = root.trim().to_string();
    (!root.is_empty()).then_some(root)
}

/// O arquivo de marcas desta sessão: na pasta temporária, para não deixar nada
/// no projeto e sair no reinício da máquina.
fn marks_path(session: &str) -> PathBuf {
    std::env::temp_dir().join(marks_name(session))
}

/// Lê as marcas sob a trava, pergunta ao núcleo de quem é a vez e grava o que
/// ele devolveu. `None` quando o disco não colabora: quem chama falha aberto.
///
/// A leitura, a decisão e a escrita acontecem sob a trava do arquivo, porque
/// as duas raízes chegam juntas: sem ela as duas leriam o arquivo vazio e as
/// duas se dariam a vez.
fn claim(path: &Path, key: &str, root: &str, now: u128) -> Option<bool> {
    let mut held = LockedFile::exclusive_within(path, PATIENCE).ok()??;
    let marks = held.read_to_string().ok()?;
    let turn = decide(&marks, key, root, now);
    // O arquivo podado vale a escrita mesmo quando a vez não é nossa: a
    // próxima leitura começa menor.
    let wrote = held.replace(turn.marks.as_bytes());
    if !turn.mine {
        return Some(false);
    }
    wrote.ok()?;
    Some(true)
}

fn now_millis() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|since| since.as_millis()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Fora do hospedeiro — terminal, teste, agente — não há vez a disputar, e
    /// todo evento roda, mesmo repetido. É o que deixa os testes dos outros
    /// ganchos repetirem o mesmo evento de propósito.
    #[test]
    fn outside_the_host_there_is_no_turn_to_take() {
        let call = HookInput {
            tool_name: Some("Bash".into()),
            hook_event_name: Some("PreToolUse".into()),
            session_id: Some("s-no-root".into()),
            raw: json!({"session_id": "s-no-root", "tool_use_id": "call-1"}),
            ..HookInput::default()
        };
        assert!(first_run_from("PreToolUse", &call, None));
        assert!(first_run_from("PreToolUse", &call, None), "repetido, roda de novo");
    }

    /// Dentro do hospedeiro, a segunda raiz com o mesmo pedido sai calada.
    #[test]
    fn inside_the_host_the_second_root_stays_quiet() {
        let call = HookInput {
            tool_name: Some("Bash".into()),
            hook_event_name: Some("PreToolUse".into()),
            session_id: Some("s-two-roots".into()),
            raw: json!({"session_id": "s-two-roots", "tool_use_id": "call-two-roots"}),
            ..HookInput::default()
        };
        assert!(first_run_from("PreToolUse", &call, Some("/plugins/installed".into())));
        assert!(!first_run_from("PreToolUse", &call, Some("/plugins/development".into())));
    }

    /// O arquivo guarda o que a decisão devolveu, e a segunda raiz o lê.
    #[test]
    fn the_marks_file_carries_the_turn_from_one_root_to_the_other() {
        let dir = tempfile::tempdir().expect("temp");
        let path = dir.path().join("marks.log");
        assert_eq!(claim(&path, "PreToolUse-abc", "/plugins/installed", 1_000), Some(true));
        assert_eq!(claim(&path, "PreToolUse-abc", "/plugins/development", 1_004), Some(false));
        let marks = std::fs::read_to_string(&path).expect("read");
        assert!(marks.contains("PreToolUse-abc"), "a marca ficou no arquivo: {marks}");
    }
}
