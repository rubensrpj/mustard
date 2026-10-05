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
//! Uma terceira coisa vale antes de toda chamada: o gasto do Jev no mês tem
//! um teto, `jev.monthly_budget_usd` no `mustard.json`, com
//! [`DEFAULT_MONTHLY_BUDGET_USD`] quando ele não diz. O que sobra do teto sai
//! de [`left_in_month`]; a chamada que custaria mais que isso não sai, e quem
//! chamou segue como seguiria sem chave.
//!
//! Nenhum erro, aviso ou texto de depuração leva a chave.

use std::fmt;
use std::path::Path;

use crate::domain::config::{FilterSetting, ProjectConfig};
use crate::domain::map_filter::FilterError;
use crate::io::spend;

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

/// O teto de gasto do Jev por mês, em dólares, quando o `mustard.json` não
/// diz outro: o que vale a pena gastar com ele, sem passar do plano.
pub const DEFAULT_MONTHLY_BUDGET_USD: f64 = 10.0;

/// O teto do mês em milionésimos de dólar: `jev.monthly_budget_usd` do
/// `mustard.json`; ausente ou inválido, [`DEFAULT_MONTHLY_BUDGET_USD`].
#[must_use]
pub fn monthly_budget_micro_usd(config: &ProjectConfig) -> u64 {
    let usd = config.jev_monthly_budget_usd().unwrap_or(DEFAULT_MONTHLY_BUDGET_USD);
    (usd * 1_000_000.0).round() as u64
}

/// O que sobra do teto no mês `month` (`AAAA-MM`), em milionésimos de dólar:
/// o teto de `config` menos o gasto do mês nas specs do projeto de `root` e
/// no arquivo do gasto da máquina em `ledger_dir` ([`spend::jev_month_micro_usd`]).
/// Zero quando o gasto já passou dele.
#[must_use]
pub fn left_in_month(root: &Path, config: &ProjectConfig, ledger_dir: Option<&Path>, month: &str) -> u64 {
    monthly_budget_micro_usd(config).saturating_sub(spend::jev_month_micro_usd(root, ledger_dir, month))
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

    /// O teto do mês é o do `mustard.json` em dólares, com fração, e o padrão
    /// de 10 dólares quando ele falta ou é texto, negativo ou nulo; o que sobra
    /// dele soma o gasto do mês pedido e nunca passa de zero para baixo.
    #[test]
    fn the_monthly_budget_is_the_file_value_or_ten_dollars_and_never_goes_below_zero() {
        let dir = tempfile::tempdir().unwrap();
        let budget = |text: &str| {
            std::fs::write(dir.path().join("mustard.json"), text).unwrap();
            ProjectConfig::load(dir.path())
        };
        assert_eq!(monthly_budget_micro_usd(&budget("{}")), 10_000_000);
        assert_eq!(monthly_budget_micro_usd(&budget(r#"{"jev": {"monthly_budget_usd": 7.5}}"#)), 7_500_000);
        assert_eq!(monthly_budget_micro_usd(&budget(r#"{"jev": {"monthly_budget_usd": 0}}"#)), 0);
        for invalid in [r#""x""#, "-3", "null"] {
            let text = format!(r#"{{"jev": {{"monthly_budget_usd": {invalid}}}}}"#);
            assert_eq!(monthly_budget_micro_usd(&budget(&text)), 10_000_000, "{invalid} falls back to the default");
        }
        assert_eq!(monthly_budget_micro_usd(&budget(r#"{"jev": {"key": "k"}}"#)), 10_000_000);

        let spec = dir.path().join(".claude/spec/uma");
        std::fs::create_dir_all(&spec).unwrap();
        let call = r#"{"v":1,"id":1,"code":"X-CALL-0001","at":"2026-10-02T10:00:00-03:00","type":"call","command":"map search","tokens":9,"cost_micro_usd":4000000}"#;
        std::fs::write(spec.join("spec.ndjson"), format!("{call}\n")).unwrap();
        let ten = budget("{}");
        assert_eq!(left_in_month(dir.path(), &ten, None, "2026-10"), 6_000_000);
        assert_eq!(left_in_month(dir.path(), &ten, None, "2026-09"), 10_000_000, "the other month has none of it");
        let two = budget(r#"{"jev": {"monthly_budget_usd": 2}}"#);
        assert_eq!(left_in_month(dir.path(), &two, None, "2026-10"), 0);
    }
}
