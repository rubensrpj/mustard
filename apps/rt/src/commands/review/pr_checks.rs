//! A esteira do provedor no pull request, como as portas a relatam.
//!
//! O merge pergunta as verificações do provedor antes de juntar, e a porta de
//! conserto pergunta se elas voltaram vermelhas. Depois de abrir o pull
//! request, e na retomada da spec com ele aberto, a mesma leitura
//! ([`provider_checks`]) é relatada: a palavra do provedor e a frase que diz o
//! que ela pede. Relatar não decide nada: o merge faz a leitura dele na hora
//! de juntar.

use std::path::Path;

use mustard_core::platform::i18n::{Locale, translate};
use serde::Serialize;

use crate::commands::review::pr_door::provider_checks;
use crate::commands::review::pr_publish::{SpecPr, spec_pr};
use crate::shared::pr_provider::PrChecks;

/// O que a esteira respondeu sobre um pull request.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct ChecksSaid {
    /// A palavra do provedor (`passed`, `running`, `failed`, `absent`) ou o
    /// motivo de não dar para ler.
    pub state: String,
    /// A frase para quem lê, no idioma do projeto.
    pub text: String,
}

/// A palavra que uma resposta traz das verificações: a do vocabulário do
/// provedor, ou o motivo de não dar para perguntar, como veio.
pub(crate) fn checks_word(checks: &Result<PrChecks, String>) -> String {
    match checks {
        Ok(state) => state.word().to_string(),
        Err(reason) => reason.clone(),
    }
}

/// A leitura `checks` do pull request `number`, como as portas a relatam. Com
/// a spec `spec`, o vermelho traz a porta de conserto dela.
pub(crate) fn said(checks: &Result<PrChecks, String>, number: u64, spec: Option<&str>, lang: Locale) -> ChecksSaid {
    let fill = |key: &str| translate(key, lang).replace("{pr}", &number.to_string());
    let state = checks_word(checks);
    let text = match (checks, spec) {
        (Ok(PrChecks::Passed), _) => fill("pr.checks.passed"),
        (Ok(PrChecks::Running), _) => fill("pr.checks.running"),
        (Ok(PrChecks::Absent), _) => fill("pr.checks.absent"),
        (Ok(PrChecks::Failed), None) => fill("pr.checks.failed"),
        (Ok(PrChecks::Failed), Some(spec)) => {
            format!("{} {}", fill("pr.checks.failed"), translate("pr.checks.fix", lang).replace("{spec}", spec))
        }
        (Ok(PrChecks::Unknown(_)) | Err(_), _) => fill("pr.checks.unreadable").replace("{reason}", &state),
    };
    ChecksSaid { state, text }
}

/// A esteira do pull request `number`, lida agora pelo provedor do
/// repositório `root`.
pub(crate) fn read(root: &Path, number: u64, spec: Option<&str>, lang: Locale) -> ChecksSaid {
    said(&provider_checks(root, number), number, spec, lang)
}

/// A esteira do pull request que a spec `spec` gravou ao abri-lo. `None`
/// quando ela não gravou número nenhum: sem ele, não há a quem perguntar.
pub(crate) fn of_spec(root: &Path, spec: &str, lang: Locale) -> Option<ChecksSaid> {
    match spec_pr(root, spec)? {
        SpecPr::Number(number) => Some(read(root, number, Some(spec), lang)),
        SpecPr::Head(_) => None,
    }
}
