//! A recusa do mapa enquanto a passada que o põe em dia segue em segundo
//! plano.

use mustard_core::domain::project_map::MapRefusal;
use mustard_core::platform::i18n::Locale;
use serde_json::{Value, json};

/// Se a recusa pode vir só de o mapa de antes ainda não ter o que a pergunta
/// pede: o mapa que falta, que não se lê ou que tem bloco vazio, e o arquivo
/// ou a declaração que ele não tem. A dica de cada uma manda rodar o scan.
fn waits_for_the_pass(refusal: &MapRefusal) -> bool {
    matches!(
        refusal,
        MapRefusal::MapMissing
            | MapRefusal::MapUnreadable { .. }
            | MapRefusal::MapUnfilled { .. }
            | MapRefusal::UnknownFile { .. }
            | MapRefusal::UnknownDeclaration { .. }
    )
}

/// A recusa como resposta. Com a passada do mapa em segundo plano
/// (`running`), a recusa que pode vir do mapa de antes troca a dica de rodar o
/// scan, que leria o projeto inteiro em primeiro plano ao mesmo tempo que a
/// passada, pela de repetir a pergunta quando ela terminar. Sem a passada, a
/// dica é a de sempre.
pub(super) fn refused(refusal: &MapRefusal, lang: Locale, running: bool) -> Value {
    let mut report = super::refused(refusal, lang);
    if running && waits_for_the_pass(refusal) {
        report["hint"] = json!(mustard_core::translate("map.refused_while_refreshing", lang));
    }
    report
}
