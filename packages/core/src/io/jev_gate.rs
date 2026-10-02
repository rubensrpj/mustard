//! `jev_gate` — a regra que liga o filtro do Jev na busca, num lugar só.
//!
//! A busca chama o Jev quando duas coisas valem: o `mustard.json` não
//! desliga o filtro (`search.filter` diferente de `none` e de um valor que
//! não existe) e o projeto tem uma chave válida. A chave vem de [`KEY_ENV`] no
//! ambiente ou, sem ela, de `jev.key` no `mustard.json`; o git não pode
//! guardar esse arquivo: guardado, a chave dele não se usa. A busca, que monta
//! o filtro com a chave, e a prova da medida, que diz se o filtro estava
//! ligado ([`crate::io::search_pieces`]), chamam as mesmas funções daqui, e por
//! isso o número da medida não diz "desligado" com o filtro valendo.
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

/// Se a busca no projeto em `root` chama o Jev: a configuração deixa e há
/// chave válida em `env` ou em `jev.key`, com a mesma recusa da chave num
/// `mustard.json` que o git acompanha.
#[must_use]
pub fn filter_on(root: &Path, config: &ProjectConfig, env: Option<String>) -> bool {
    setting_allows(config.search_filter()) && find_key(root, config, env).is_ok()
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
    use tempfile::{TempDir, tempdir};

    /// Um projeto numa pasta nova com o `mustard.json` que o teste escreve;
    /// com `git`, a pasta é um repositório, ainda sem o arquivo.
    fn project(config: &serde_json::Value, git: bool) -> TempDir {
        let dir = tempdir().unwrap();
        if git {
            assert!(crate::platform::git::run(dir.path(), &["init", "-q"]).ok);
        }
        std::fs::write(dir.path().join("mustard.json"), config.to_string()).unwrap();
        dir
    }

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

    /// A chave só em `jev.key` liga o filtro; `search.filter` igual a `none`
    /// o desliga mesmo com a chave; sem chave em lugar nenhum, desligado.
    #[test]
    fn the_filter_is_on_with_the_key_of_the_project_file_unless_the_setting_turns_it_off() {
        let keyed = project(&json!({"jev": {"key": "from-file"}}), false);
        let config = ProjectConfig::load(keyed.path());
        assert!(filter_on(keyed.path(), &config, None), "the key of the file alone turns it on");
        assert!(filter_on(keyed.path(), &config, Some("from-env".to_string())));

        let off = project(&json!({"search": {"filter": "none"}, "jev": {"key": "from-file"}}), false);
        let config = ProjectConfig::load(off.path());
        assert!(!filter_on(off.path(), &config, None), "none turns it off with the key of the file");
        assert!(!filter_on(off.path(), &config, Some("from-env".to_string())), "none turns it off with the key of the environment");

        let bare = project(&json!({}), false);
        let config = ProjectConfig::load(bare.path());
        assert!(!filter_on(bare.path(), &config, None));
        assert!(filter_on(bare.path(), &config, Some("from-env".to_string())), "the key of the environment alone turns it on");
    }

    /// O `mustard.json` que o git guarda não entrega a chave: sem a do
    /// ambiente o filtro fica desligado; com ela, vale a do ambiente, com o
    /// aviso.
    #[test]
    fn a_key_in_a_file_that_git_tracks_does_not_turn_the_filter_on() {
        let tracked = project(&json!({"jev": {"key": "from-file"}}), true);
        let config = ProjectConfig::load(tracked.path());
        assert!(filter_on(tracked.path(), &config, None), "out of git, the key counts");

        assert!(crate::platform::git::run(tracked.path(), &["add", "mustard.json"]).ok);
        assert!(!filter_on(tracked.path(), &config, None), "git tracks the file: its key is not used");
        assert_eq!(find_key(tracked.path(), &config, None).unwrap_err(), FilterError::KeyInGit);
        assert!(filter_on(tracked.path(), &config, Some("from-env".to_string())), "the key of the environment still counts");
        let found = find_key(tracked.path(), &config, Some("from-env".to_string())).unwrap();
        assert_eq!((found.value(), found.warning()), ("from-env", Some(&FilterError::KeyInGit)));
    }

    #[test]
    fn the_debug_text_of_a_key_never_carries_it() {
        let found = key_from(Some("sk-secret-value".to_string()), None, || false).unwrap();
        assert!(!format!("{found:?}").contains("sk-secret-value"), "{found:?}");
    }
}
