//! A vez de um evento do hospedeiro, quando duas raízes de plugin do Mustard
//! estão ligadas e o mesmo evento chega duas vezes.
//!
//! POR QUE ISTO EXISTE. O hospedeiro registra os ganchos por raiz de plugin, e
//! uma máquina com duas raízes do Mustard ligadas recebe o mesmo evento duas
//! vezes, uma por raiz. Em campo (21/10/2026) cada aviso entrou duas vezes na
//! conversa e cada bloqueio foi julgado duas vezes, com as duas versões
//! injetando textos diferentes. O usuário leu tudo em dobro, e a medida de
//! consumo contou tudo em dobro.
//!
//! AQUI MORA SÓ A DECISÃO: qual é a chave do evento, quanto tempo ela vale, e
//! quem fica com a vez diante das marcas que já existem. Abrir arquivo, travar
//! e ler o relógio é trabalho de quem chama
//! (`mustard-rt`, `shared::event_once`). Assim a regra se testa sem tocar o
//! disco, como o resto do domínio.

use std::time::Duration;

use crate::domain::model::contract::HookInput;
use crate::io::sha256::Sha256;
use serde_json::Value;

/// Quanto tempo uma marca segura o evento.
///
/// As duas raízes chegaram com 3 a 10 milissegundos de diferença nos registros
/// de campo. A folga cobre uma máquina carregada; acima dela, o mesmo pedido é
/// tratado como evento novo.
pub const WINDOW: Duration = Duration::from_secs(5);

/// O que a leitura das marcas decidiu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    /// `true` quando a vez é de quem perguntou: ninguém tratou este evento
    /// dentro da janela.
    pub mine: bool,
    /// As marcas como ficam depois da decisão: as de dentro da janela, mais a
    /// nova quando a vez é de quem perguntou. Quem chama grava este texto.
    pub marks: String,
}

/// A sessão do hospedeiro, sem a qual duas conversas dividiriam as marcas.
#[must_use]
pub fn session_of(input: &HookInput) -> Option<String> {
    let typed = input.session_id.as_deref();
    let id = typed.or_else(|| input.raw.get("session_id").and_then(Value::as_str))?;
    let id = id.trim();
    (!id.is_empty()).then(|| id.to_string())
}

/// O que distingue um evento do seguinte, sem distinguir as raízes.
///
/// A chamada da ferramenta (`tool_use_id`, ou `call_id` no Codex) quando o
/// hospedeiro a manda, senão o resumo do pedido inteiro, que é idêntico nas
/// duas raízes porque é o mesmo texto que o hospedeiro escreveu para as duas.
/// A raiz não entra na chave, de propósito: é ela que as distingue.
#[must_use]
pub fn key_of(event: &str, input: &HookInput) -> String {
    let call = input
        .raw
        .get("tool_use_id")
        .or_else(|| input.raw.get("call_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty() && id.len() <= 256);
    match call {
        Some(call) => format!("{event}-{}", digest(call)),
        None => format!("{event}-{}", digest(&serde_json::to_string(input).unwrap_or_default())),
    }
}

/// O nome do arquivo de marcas de uma sessão. Quem chama diz em que pasta ele
/// mora.
#[must_use]
pub fn marks_name(session: &str) -> String {
    format!("mustard-event-once-{}.log", digest(session))
}

/// A vez de tratar o evento `key`, diante das marcas em `marks`, para quem
/// roda na raiz de plugin `root` no instante `now` (milissegundos).
///
/// As marcas fora da janela saem no caminho: a próxima leitura começa menor.
/// Quem chama grava [`Turn::marks`] nos dois casos.
#[must_use]
pub fn decide(marks: &str, key: &str, root: &str, now: u128) -> Turn {
    let mut kept = String::new();
    let mut taken = false;
    for line in marks.lines() {
        let Some((at, rest)) = line.split_once(' ') else { continue };
        let Ok(at) = at.parse::<u128>() else { continue };
        if now.saturating_sub(at) > WINDOW.as_millis() {
            continue;
        }
        if rest.split_once(' ').map_or(rest, |(mark, _)| mark) == key {
            taken = true;
        }
        kept.push_str(line);
        kept.push('\n');
    }
    if taken {
        return Turn { mine: false, marks: kept };
    }
    use std::fmt::Write;
    let _ = writeln!(kept, "{now} {key} {root}");
    Turn { mine: true, marks: kept }
}

fn digest(value: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(value.as_bytes());
    hash.hex_digest()[..16].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn input(session: &str, call: Option<&str>) -> HookInput {
        let mut raw = json!({"session_id": session});
        if let Some(call) = call {
            raw["tool_use_id"] = json!(call);
        }
        HookInput {
            tool_name: Some("Bash".into()),
            tool_input: json!({"command": "ls"}),
            hook_event_name: Some("PreToolUse".into()),
            session_id: Some(session.into()),
            raw,
            ..HookInput::default()
        }
    }

    #[test]
    fn the_second_plugin_root_to_bring_the_same_event_stays_quiet() {
        let key = key_of("PreToolUse", &input("s1", Some("call-1")));
        let first = decide("", &key, "/plugins/installed", 1_000);
        assert!(first.mine, "a primeira raiz trata");
        let second = decide(&first.marks, &key, "/plugins/development", 1_004);
        assert!(!second.mine, "a segunda sai calada");
    }

    #[test]
    fn the_same_request_made_again_after_the_window_runs() {
        let key = key_of("PreToolUse", &input("s1", Some("call-1")));
        let first = decide("", &key, "/plugins/installed", 1_000);
        let later = 1_000 + WINDOW.as_millis() + 1;
        let again = decide(&first.marks, &key, "/plugins/installed", later);
        assert!(again.mine, "passada a janela, o mesmo pedido é evento novo");
        assert_eq!(again.marks.lines().count(), 1, "a marca velha saiu no caminho");
    }

    #[test]
    fn another_call_of_the_same_event_is_its_own_event() {
        let first = key_of("PreToolUse", &input("s1", Some("call-1")));
        let second = key_of("PreToolUse", &input("s1", Some("call-2")));
        assert_ne!(first, second);
        let one = decide("", &first, "/plugins/installed", 1_000);
        let two = decide(&one.marks, &second, "/plugins/installed", 1_001);
        assert!(two.mine, "outra chamada do mesmo evento tem a vez dela");
        assert_eq!(two.marks.lines().count(), 2, "as duas marcas ficam na janela");
    }

    #[test]
    fn two_sessions_never_share_a_mark_file() {
        assert_ne!(marks_name("s1"), marks_name("s2"));
    }

    #[test]
    fn a_request_with_no_named_call_is_keyed_by_the_whole_request() {
        let one = key_of("SessionStart", &input("s1", None));
        let same = key_of("SessionStart", &input("s1", None));
        let other = key_of("SessionStart", &input("s2", None));
        assert_eq!(one, same, "o mesmo pedido dá a mesma chave nas duas raízes");
        assert_ne!(one, other);
    }

    /// Uma marca gravada à mão, que não se entende, não derruba a decisão nem
    /// fica no arquivo: a spec e as marcas são texto, e qualquer um as edita.
    #[test]
    fn a_mark_that_does_not_parse_is_dropped() {
        let key = key_of("PreToolUse", &input("s1", Some("call-1")));
        let turn = decide("lixo\nsem-tempo chave raiz\n", &key, "/plugins/installed", 1_000);
        assert!(turn.mine);
        assert_eq!(turn.marks.lines().count(), 1, "só a marca nova fica");
    }
}
