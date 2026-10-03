//! `jev_gate` — a regra que liga o filtro do Jev na busca, num lugar só.
//!
//! A busca chama o Jev quando duas coisas valem: o `mustard.json` não
//! desliga o filtro (`search.filter` diferente de `none` e de um valor que
//! não existe) e o projeto tem uma chave válida. A chave vem de [`KEY_ENV`] no
//! ambiente ou, sem ela, de `jev.key` no `mustard.json`; o git não pode
//! guardar esse arquivo: guardado, a chave dele não se usa. A busca monta o
//! filtro com a chave chamando as funções daqui, e nenhuma outra regra liga o
//! filtro.
//!
//! Nenhum erro, aviso ou texto de depuração leva a chave.

use std::fmt;
use std::path::Path;

use crate::domain::config::{FilterSetting, ProjectConfig};
use crate::domain::map_filter::FilterError;

/// A variável de ambiente da chave do Jev; vence o `mustard.json`.
pub const KEY_ENV: &str = "TYPESAFE_API_KEY";

/// A chave achada e, quando o git guarda o `mustard.json` que também traz
/// uma chave, o aviso para tirá-lo do git. A chave do ambiente vale do mesmo
/// jeito. Não se imprime: o `Debug` escreve reticências, e não há `Display`.
#[derive(Clone, PartialEq, Eq)]
pub struct FoundKey {
    value: String,
    warning: Option<FilterError>,
}

impl FoundKey {
    /// O texto da chave, sem espaço em volta.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    /// O aviso da chave do `mustard.json` que o git guarda, mesmo quando a do
    /// ambiente vale.
    #[must_use]
    pub fn warning(&self) -> Option<&FilterError> {
        self.warning.as_ref()
    }
}

impl fmt::Debug for FoundKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FoundKey").field("value", &"…").field("warning", &self.warning).finish()
    }
}

/// Se o `mustard.json` deixa a busca chamar o Jev: ausente ou `jev`, sim;
/// `none` ou um valor que não existe, não.
#[must_use]
pub fn setting_allows(setting: FilterSetting) -> bool {
    matches!(setting, FilterSetting::Absent | FilterSetting::Jev)
}

/// A chave do projeto em `root`: `env` (o valor de [`KEY_ENV`], que quem
/// chama lê) no ambiente; sem ela, `jev.key` do `mustard.json` que `config`
/// leu. Sem nenhuma das duas, [`FilterError::MissingKey`]; com a do arquivo
/// que o git guarda, [`FilterError::KeyInGit`].
///
/// # Errors
/// A chave que falta ou que o git guarda, como acima.
pub fn find_key(root: &Path, config: &ProjectConfig, env: Option<String>) -> Result<FoundKey, FilterError> {
    key_from(env, config.jev_key(), || tracked_by_git(root))
}

/// A escolha da chave, sobre o valor do ambiente e o do `mustard.json`; o
/// valor em branco vale como ausente. `tracked` diz se o git guarda o
/// arquivo, e só se pergunta quando ele traz uma chave: a chave guardada no
/// git não se usa, e o aviso sai mesmo quando a do ambiente vale.
fn key_from(env: Option<String>, project: Option<&str>, tracked: impl FnOnce() -> bool) -> Result<FoundKey, FilterError> {
    let project = project.map(str::trim).filter(|key| !key.is_empty());
    let in_git = project.is_some() && tracked();
    let warning = in_git.then_some(FilterError::KeyInGit);
    if let Some(key) = env.map(|value| value.trim().to_string()).filter(|value| !value.is_empty()) {
        return Ok(FoundKey { value: key, warning });
    }
    if in_git {
        return Err(FilterError::KeyInGit);
    }
    let key = project.ok_or(FilterError::MissingKey)?;
    Ok(FoundKey { value: key.to_string(), warning: None })
}

/// O git guarda o `mustard.json` de `root`, no índice ou num commit. Sem git
/// ou fora de um repositório, não guarda.
fn tracked_by_git(root: &Path) -> bool {
    crate::platform::git::run(root, &["ls-files", "--error-unmatch", "--", "mustard.json"]).ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_environment_key_comes_before_the_project_file() {
        let found = key_from(Some(" from-env ".to_string()), Some("from-file"), || false).unwrap();
        assert_eq!(found.value(), "from-env");
        let found = key_from(Some("  ".to_string()), Some(" from-file "), || false).unwrap();
        assert_eq!(found.value(), "from-file");
        let found = key_from(None, Some("from-file"), || false).unwrap();
        assert_eq!(found.value(), "from-file");
        assert!(found.warning().is_none());
    }

    #[test]
    fn without_a_key_anywhere_the_filter_has_no_key() {
        let never = || panic!("without a key in the file, git is not asked");
        assert_eq!(key_from(None, None, never).unwrap_err(), FilterError::MissingKey);
        assert_eq!(key_from(Some(" ".to_string()), Some("  "), never).unwrap_err(), FilterError::MissingKey);
        let found = key_from(Some("from-env".to_string()), None, never).unwrap();
        assert!(found.warning().is_none());
    }

    /// Só `none` e o valor que não existe desligam o filtro.
    #[test]
    fn only_none_and_an_unknown_value_turn_the_filter_off() {
        let allows = |value: serde_json::Value| setting_allows(FilterSetting::of(Some(&value)));
        assert!(setting_allows(FilterSetting::of(None)));
        assert!(allows(json!("jev")));
        assert!(!allows(json!("none")));
        assert!(!allows(json!("another")));
        assert!(!allows(json!(3)));
    }

    #[test]
    fn the_debug_text_of_a_key_never_carries_it() {
        let found = key_from(Some("sk-secret-value".to_string()), None, || false).unwrap();
        assert!(!format!("{found:?}").contains("sk-secret-value"), "{found:?}");
    }
}
