//! O pedido de revisão final que a spec deixou para trás.
//!
//! O pedido do revisor é montado e gravado no envio uma vez, quando o
//! fechamento o abre. O item que a spec ganha depois disso — o critério
//! gravado entre os dois fechamentos, por exemplo — não chega ao revisor: o
//! pedido gravado não o lista, e quem o segue aprova sem ler nem conferir o
//! item novo. A entrega e o commit já invalidam o pedido aberto
//! ([`mustard_core::domain::spec_state::last_change`]); o item gravado na spec
//! não, porque não muda o código da obra.
//!
//! Aqui o fechamento confere o envio que o veredito responde — o pedido
//! aberto ou o que a aprovação final vigente respondeu — contra o pedido que
//! ele montaria agora: o item que o pedido de agora lista e que foi gravado
//! depois do envio deixa o envio para trás. O fechamento então grava o pedido
//! novo no lugar de esperar o veredito do antigo, e a aprovação do pedido
//! antigo não fecha a obra.

use std::path::Path;

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::domain::spec_state::{answered_request, final_approval};
use mustard_core::platform::i18n::{Locale, translate};

/// O envio de revisão final que ficou para trás, com os itens que a spec
/// ganhou depois dele e que o pedido de agora lista.
pub(super) struct Outdated {
    /// O número do envio que ficou para trás.
    send: u64,
    /// O código de cada item, na ordem em que foi gravado.
    items: Vec<String>,
}

impl Outdated {
    /// O aviso de que o pedido foi refeito, com os itens novos e o envio
    /// antigo, no idioma `lang`.
    pub(super) fn hint(&self, lang: Locale) -> String {
        translate("close.review_request_renewed", lang).replace("{items}", &self.items.join(", ")).replace("{send}", &self.send.to_string())
    }
}

/// O envio de revisão final que o veredito responde: o que a aprovação final
/// vigente respondeu ou, sem ela, o pedido aberto que ainda espera veredito.
fn answered_send(log: &SpecLog) -> Option<u64> {
    match final_approval(log) {
        Some(verdict) => answered_request(log, verdict.id),
        None => crate::commands::flow::round::awaited_review(log),
    }
}

/// O envio de revisão final de `spec` que ficou para trás: algum item que o
/// pedido montado agora lista foi gravado — ou regravado — depois dele.
/// `None` sem envio que o veredito responda, ou quando o envio já trazia tudo
/// o que o pedido de agora lista. O veredito não conta como item novo: é a
/// resposta ao próprio envio.
pub(super) fn find(root: &Path, spec: &str, log: &SpecLog, lang: Locale) -> Option<Outdated> {
    let send = answered_send(log)?;
    let listed = mustard_core::io::wave_prompt::final_review(root, spec, log, lang).listed;
    let codes = log.codes();
    let mut items: Vec<String> = Vec::new();
    for event in log.visible().into_iter().filter(|event| event.id > send && event.event_type != "verdict") {
        if let Some(code) = codes.get(&event.id).filter(|code| listed.contains(code) && !items.contains(code)) {
            items.push(code.clone());
        }
    }
    (!items.is_empty()).then_some(Outdated { send, items })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use mustard_core::domain::spec_events::SpecLog;
    use mustard_core::io::spec_events as store;
    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::super::CloseOpts;
    use super::super::tests::{approve, close_for, ready_with_waves, review_prompt, verdict_written};
    use crate::commands::spec_events::write::{WriteOpts, seed_at};

    fn log_of(root: &Path) -> SpecLog {
        store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap()
    }

    fn close(root: &Path, report: Option<String>) -> Value {
        close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report, ..Default::default() }, None)
    }

    /// Quantos pedidos de revisão final a spec gravou.
    fn review_sends(root: &Path) -> usize {
        log_of(root).visible().into_iter().filter(|e| e.event_type == "send" && e.str_field("role") == Some("review")).count()
    }

    /// Grava um critério novo na spec `x`, com a obra já pedida ao revisor, e
    /// devolve o código dele.
    fn criterion_after_the_request(root: &Path) -> String {
        let said = log_of(root).visible().into_iter().find(|e| e.event_type == "message").map(|e| e.id).unwrap();
        let body = json!({"when": "a busca roda", "then": "a citação sai com a pasta", "proof": "git --version",
            "form": "ubiquitous", "origin": said});
        let wrote = seed_at(&WriteOpts { root: root.to_path_buf(), spec: Some("x".into()), event_type: "criterion".into(), json: body.to_string() });
        let id = wrote["id"].as_u64().unwrap_or_else(|| panic!("o critério não gravou: {wrote}"));
        log_of(root).codes().get(&id).cloned().unwrap()
    }

    /// O texto do aviso de pedido refeito na resposta `out`, vazio sem ele.
    fn renewed_hint(out: &Value) -> String {
        let warnings = out["warnings"].as_array().cloned().unwrap_or_default();
        warnings.iter().filter(|w| w["reason"] == json!("review-request-renewed")).filter_map(|w| w["hint"].as_str()).collect::<Vec<_>>().join("\n")
    }

    /// O critério gravado depois do pedido de revisão aberto chega ao
    /// revisor: o fechamento seguinte não espera o veredito do pedido antigo,
    /// grava o pedido novo, que lista o critério, e avisa por quê.
    #[test]
    fn a_criterion_recorded_after_the_review_request_is_listed_by_the_next_close() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_with_waves(root, "x", &["git --version"], 1);
        assert_eq!(close(root, None)["review"]["final"], json!(true));
        // A revisão reprova sem apontar onda: o pedido de volta lista o que
        // mudou desde o veredito.
        let rejected = json!({"final": true, "result": "rejected", "text": "Refazer a conferência."});
        let asked = close(root, verdict_written(root, "x", rejected));
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        let sent = review_sends(root);

        let code = criterion_after_the_request(root);
        let renewed = close(root, None);
        assert_eq!(renewed["review"]["final"], json!(true), "the close renews the request instead of waiting: {renewed}");
        assert_eq!(review_sends(root), sent + 1, "{renewed}");
        let prompt = review_prompt(&renewed);
        assert!(prompt.contains(&code), "the new request lists {code}: {prompt}");
        assert!(renewed_hint(&renewed).contains(&code), "{renewed}");
    }

    /// A aprovação do pedido gravado antes de um critério novo não fecha a
    /// obra: o fechamento grava o pedido novo, que lista o critério, e só a
    /// aprovação dele fecha.
    #[test]
    fn an_approval_of_the_request_before_a_new_criterion_does_not_close_the_work() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_with_waves(root, "x", &["git --version"], 1);
        assert_eq!(close(root, None)["review"]["final"], json!(true));

        let code = criterion_after_the_request(root);
        let renewed = close(root, approve(root, "x"));
        assert_eq!(renewed["phase"], json!("running"), "{renewed}");
        assert_eq!(renewed["review"]["final"], json!(true), "{renewed}");
        assert!(review_prompt(&renewed).contains(&code), "{renewed}");

        let closed = close(root, approve(root, "x"));
        assert_eq!(closed["phase"], json!("closed"), "the approval of the new request closes the work: {closed}");
        assert!(renewed_hint(&closed).is_empty(), "{closed}");
    }
}
