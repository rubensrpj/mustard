//! O que a rodada diz a quem conduz sobre os comandos que o projeto declara:
//! ela junta e comita sem rodá-los, e o fechamento compila e roda o lint, a
//! suíte e os critérios.

use std::path::Path;

use mustard_core::platform::i18n::{Locale, translate};

/// O que fazer quando os agentes voltarem. No projeto que declara o lint ou a
/// suíte, a frase diz também que eles ficam para o fechamento — quem conduz
/// não os roda por conta própria.
pub(super) fn report_back(root: &Path, lang: Locale) -> String {
    let declared = mustard_core::ProjectConfig::load(root).commands();
    let back = translate("round.report", lang);
    if declared.test.is_none() && declared.lint.is_none() {
        return back.to_string();
    }
    format!("{back} {}", translate("round.report.checks", lang))
}
