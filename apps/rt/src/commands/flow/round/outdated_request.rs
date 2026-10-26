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
//! A conferência confronta o envio que o veredito responde — o pedido aberto
//! ou o que a aprovação final vigente respondeu — com o pedido que o
//! fechamento montaria agora: o item que o pedido de agora lista e que foi
//! gravado depois do envio deixa o envio para trás ([`find`]). O fechamento
//! então grava o pedido novo no lugar de esperar o veredito do antigo, e a
//! aprovação do pedido antigo não fecha a obra. A rodada e o passo do painel
//! fazem a mesma conferência antes de mandar esperar o veredito
//! ([`awaits_verdict`]): com o envio para trás, eles mandam fechar.

use std::path::Path;

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::domain::spec_state::{answered_request, final_approval};
use mustard_core::platform::i18n::{Locale, translate};

use super::queue::awaited_review;

/// O envio de revisão final que ficou para trás, com os itens que a spec
/// ganhou depois dele e que o pedido de agora lista.
pub(crate) struct Outdated {
    /// O número do envio que ficou para trás.
    send: u64,
    /// O código de cada item, na ordem em que foi gravado.
    items: Vec<String>,
}

impl Outdated {
    /// O aviso de que o pedido foi refeito, com os itens novos e o envio
    /// antigo, no idioma `lang`.
    pub(crate) fn hint(&self, lang: Locale) -> String {
        translate("close.review_request_renewed", lang).replace("{items}", &self.items.join(", ")).replace("{send}", &self.send.to_string())
    }
}

/// O envio de revisão final que o veredito responde — o que a aprovação final
/// vigente respondeu ou, sem ela, o pedido aberto que ainda espera veredito —,
/// na posição da versão que gravou o pedido ([`SpecLog::dispatch_position`]),
/// como [`answered_request`] já a dá. O lançamento do revisor grava uma
/// versão nova do envio, com o modelo que valeu, e ela não refaz o pedido: o
/// item gravado entre o pedido e o lançamento continua fora do que o revisor
/// recebe.
fn answered_send(log: &SpecLog) -> Option<u64> {
    match final_approval(log) {
        Some(verdict) => answered_request(log, verdict.id),
        None => awaited_review(log).map(|asked| log.dispatch_position(asked)),
    }
}

/// O envio de revisão final de `spec` que ficou para trás: algum item que o
/// pedido montado agora lista foi gravado — ou regravado — depois dele.
/// `None` sem envio que o veredito responda, ou quando o envio já trazia tudo
/// o que o pedido de agora lista. O veredito não conta como item novo: é a
/// resposta ao próprio envio.
pub(crate) fn find(root: &Path, spec: &str, log: &SpecLog, lang: Locale) -> Option<Outdated> {
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

/// Se a obra de `spec` espera o veredito do pedido de revisão final aberto: o
/// pedido é mais novo que a última mudança ([`awaited_review`]) e não ficou
/// para trás de um item gravado depois dele ([`find`]). Com o pedido velho ou
/// para trás, o veredito dele já não aprovaria a obra, e o passo é fechar, que
/// grava o pedido novo.
pub(crate) fn awaits_verdict(root: &Path, spec: &str, log: &SpecLog, lang: Locale) -> bool {
    awaited_review(log).is_some() && find(root, spec, log, lang).is_none()
}
