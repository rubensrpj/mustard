//! `jev_gate` — a regra que liga o filtro do Jev na busca, num lugar só.
//!
//! A busca usa regras nativas por padrão. O Jev exige `ai.fallback: true`,
//! um filtro `jev` explicitamente escolhido para a finalidade e uma chave
//! válida. A chave vem de [`KEY_ENV`] no ambiente ou, sem ela, de `jev.key`
//! no `mustard.json`, pela regra dos segredos do projeto
//! ([`project_secret`]): o arquivo que o git guarda ou não ignora não entrega
//! a chave. A busca monta o filtro com a chave chamando as funções daqui, e
//! nenhuma outra regra liga o filtro.
//!
//! Nenhum erro, aviso ou texto de depuração leva a chave.

use std::fmt;
use std::path::Path;

use crate::domain::config::{FilterSetting, ProjectConfig};
use crate::domain::map_filter::FilterError;
use crate::io::project_secret::{self, SecretError};

/// A variável de ambiente da chave do Jev; vence o `mustard.json`.
pub const KEY_ENV: &str = "TYPESAFE_API_KEY";

/// A chave achada e, quando o git guarda ou não ignora o `mustard.json` que
/// também traz uma chave, o aviso para tirá-lo do git. A chave do ambiente vale do mesmo
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

    /// O aviso da chave do `mustard.json` que o git vê, mesmo quando a do
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

/// A provider must be named explicitly. Missing settings use native rules.
#[must_use]
pub fn setting_allows(setting: FilterSetting) -> bool {
    matches!(setting, FilterSetting::Jev)
}

/// A chave do projeto em `root`, pela regra dos segredos do projeto
/// ([`project_secret::find`]): `env` (o valor de [`KEY_ENV`], que quem chama
/// lê) no ambiente; sem ela, o env local privado e então `jev.key` do
/// `mustard.json` que `config` leu. Sem chave, [`FilterError::MissingKey`];
/// com a do arquivo que o git guarda ou não ignora, [`FilterError::KeyInGit`].
///
/// # Errors
/// A chave que falta ou que o git vê, como acima.
pub fn find_key(root: &Path, config: &ProjectConfig, env: Option<String>) -> Result<FoundKey, FilterError> {
    found(project_secret::find(root, KEY_ENV, env, config.jev_key()))
}

/// A escolha da chave pela regra dos segredos ([`project_secret::choose`]),
/// com os erros e o aviso do filtro.
#[cfg(test)]
fn key_from(env: Option<String>, project: Option<&str>, exposed: impl FnOnce() -> bool) -> Result<FoundKey, FilterError> {
    found(project_secret::choose(env, project, exposed))
}

/// O segredo achado como chave do filtro: o arquivo que o git vê vira o aviso
/// ou a recusa [`FilterError::KeyInGit`].
fn found(secret: Result<project_secret::FoundSecret, SecretError>) -> Result<FoundKey, FilterError> {
    match secret {
        Ok(secret) => Ok(FoundKey { value: secret.value().to_string(), warning: secret.in_git().then_some(FilterError::KeyInGit) }),
        Err(SecretError::Missing) => Err(FilterError::MissingKey),
        Err(SecretError::InGit) => Err(FilterError::KeyInGit),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn native_cli_uses_the_same_private_local_environment_key_as_the_host() {
        let dir=tempfile::tempdir().unwrap(); let root=dir.path();
        assert!(crate::platform::git::run(root,&["init","-q"]).ok);
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(root.join(".gitignore"),".claude/settings.local.json\n").unwrap();
        std::fs::write(root.join(".claude/settings.local.json"),json!({"env":{KEY_ENV:"private-test-key"}}).to_string()).unwrap();
        let config=ProjectConfig::load(root);
        assert_eq!(find_key(root,&config,None).unwrap().value(),"private-test-key");
        assert_eq!(find_key(root,&config,Some("explicit-env".into())).unwrap().value(),"explicit-env");
        let nested=root.join("apps/admin");std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(find_key(&nested,&ProjectConfig::load(&nested),None).unwrap().value(),"private-test-key");
        assert!(crate::platform::git::run(root,&["add","-f","--",".claude/settings.local.json"]).ok);
        assert_eq!(find_key(root,&config,None).unwrap_err(),FilterError::MissingKey);
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

    /// An API key alone must never activate the provider.
    #[test]
    fn only_an_explicit_jev_filter_allows_the_provider() {
        let allows = |value: serde_json::Value| setting_allows(FilterSetting::of(Some(&value)));
        assert!(!setting_allows(FilterSetting::of(None)));
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
