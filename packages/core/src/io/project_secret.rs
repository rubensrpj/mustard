//! `project_secret` — de onde vem um segredo do projeto, numa regra só para
//! todos: a chave do Jev e o token da publicação.
//!
//! O ambiente vence. Sem ele, vale a variável no `env` das configurações
//! locais privadas do Claude e, por último, o campo do `mustard.json`. Os dois
//! arquivos só valem quando o git não os vê: dentro de um repositório, o git
//! precisa ignorá-los sem guardá-los. O arquivo que o git guarda, ou que ele
//! não ignora, vai no próximo commit com o segredo dentro, e o segredo dele
//! não se usa. Fora de um repositório não há commit, e o arquivo vale.
//!
//! Nenhum erro, aviso ou texto de depuração leva o segredo.

use std::fmt;
use std::path::Path;

use crate::domain::config::{JEV_KEY_FIELD, PUBLICATION_TOKEN_ENV, PUBLICATION_TOKEN_FIELD};
use crate::io::jev_gate::KEY_ENV;
use crate::platform::git;

/// Os nomes de campo cujo valor é segredo, no `mustard.json` e no `env` das
/// configurações locais: o que toda leitura do arquivo esconde.
pub const SECRET_NAMES: [&str; 4] = [JEV_KEY_FIELD, KEY_ENV, PUBLICATION_TOKEN_FIELD, PUBLICATION_TOKEN_ENV];

const LOCAL_SETTINGS: &str = ".claude/settings.local.json";
const CONFIG_FILE: &str = "mustard.json";

/// Por que não há segredo: nenhum lugar o traz, ou só o `mustard.json` o
/// traz e o git vê o arquivo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretError {
    Missing,
    InGit,
}

/// O segredo achado e, quando o `mustard.json` também traz um e o git vê o
/// arquivo, o sinal para avisar, mesmo quando o do ambiente vale. Não se
/// imprime: o `Debug` escreve reticências, e não há `Display`.
#[derive(Clone, PartialEq, Eq)]
pub struct FoundSecret {
    value: String,
    in_git: bool,
}

impl FoundSecret {
    /// O texto do segredo, sem espaço em volta.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    /// O `mustard.json` também traz o segredo, e o git o guarda ou não o
    /// ignora.
    #[must_use]
    pub fn in_git(&self) -> bool {
        self.in_git
    }
}

impl fmt::Debug for FoundSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FoundSecret").field("value", &"…").field("in_git", &self.in_git).finish()
    }
}

/// O segredo do projeto em `root`: `env` (o valor da variável `env_name`,
/// que quem chama lê) no ambiente; sem ela, a mesma variável no `env` das
/// configurações locais privadas, do projeto ou da raiz do repositório acima
/// dele; por último `file`, o valor do `mustard.json` de `root`.
///
/// # Errors
/// [`SecretError::Missing`] sem segredo; [`SecretError::InGit`] quando só o
/// `mustard.json` o traz e o git vê o arquivo.
pub fn find(root: &Path, env_name: &str, env: Option<String>, file: Option<&str>) -> Result<FoundSecret, SecretError> {
    let env = env.filter(|value| !value.trim().is_empty()).or_else(|| local_environment(root, env_name)).or_else(|| {
        let top = git::run(root, &["rev-parse", "--show-toplevel"]).out()?;
        let top = Path::new(top.trim());
        if top == root || !root.canonicalize().ok()?.starts_with(top.canonicalize().ok()?) {
            return None;
        }
        local_environment(top, env_name)
    });
    choose(env, file, || !kept_out_of_git(root, CONFIG_FILE))
}

/// A escolha do segredo, sobre o valor do ambiente e o do `mustard.json`; o
/// valor em branco vale como ausente. `exposed` diz se o git vê o arquivo, e
/// só se pergunta quando ele traz um segredo: o segredo que o git vê não se
/// usa, e o aviso sai mesmo quando o do ambiente vale.
///
/// # Errors
/// Os de [`find`].
pub fn choose(env: Option<String>, file: Option<&str>, exposed: impl FnOnce() -> bool) -> Result<FoundSecret, SecretError> {
    let file = file.map(str::trim).filter(|value| !value.is_empty());
    let in_git = file.is_some() && exposed();
    if let Some(value) = env.map(|value| value.trim().to_string()).filter(|value| !value.is_empty()) {
        return Ok(FoundSecret { value, in_git });
    }
    if in_git {
        return Err(SecretError::InGit);
    }
    let value = file.ok_or(SecretError::Missing)?;
    Ok(FoundSecret { value: value.to_string(), in_git: false })
}

/// `true` quando o git não vê o arquivo `relative` de `root`: ele o ignora e
/// não o guarda, ou `root` não está num repositório. O `check-ignore` responde
/// as duas coisas de uma vez, porque um arquivo guardado nunca conta como
/// ignorado.
#[must_use]
pub fn kept_out_of_git(root: &Path, relative: &str) -> bool {
    git::run(root, &["check-ignore", "-q", "--", relative]).ok || !git::run(root, &["rev-parse", "--is-inside-work-tree"]).ok
}

/// O valor de `name` no `env` das configurações locais privadas de `root`,
/// quando o git não vê o arquivo. Claude passa esse `env` aos processos que
/// abre; um programa nativo ou o Codex podem não passar, e todos leem o mesmo
/// segredo. Um arquivo ligado para fora do projeto não conta.
fn local_environment(root: &Path, name: &str) -> Option<String> {
    let path = root.join(LOCAL_SETTINGS);
    if !path.canonicalize().ok()?.starts_with(root.canonicalize().ok()?) || path.metadata().ok()?.len() > 1_048_576 {
        return None;
    }
    if !kept_out_of_git(root, LOCAL_SETTINGS) {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    value["env"][name].as_str().map(str::trim).filter(|value| !value.is_empty()).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Um repositório em `root` que só ignora o que o teste manda: o arquivo
    /// global de ignorar da máquina fica de fora.
    fn repository(root: &Path) {
        assert!(git::run(root, &["init", "-q"]).ok);
        assert!(git::run(root, &["config", "core.excludesFile", ""]).ok);
    }

    /// O `mustard.json` que o git não ignora entrega o segredo só depois de
    /// ignorado, e o perde de novo quando o git passa a guardá-lo. Fora de um
    /// repositório, vale.
    #[test]
    fn a_file_secret_counts_only_while_git_does_not_see_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert_eq!(find(root, PUBLICATION_TOKEN_ENV, None, Some("from-file")).unwrap().value(), "from-file", "no repository, no commit");
        repository(root);
        std::fs::write(root.join(CONFIG_FILE), "{}").unwrap();
        assert_eq!(find(root, PUBLICATION_TOKEN_ENV, None, Some("from-file")).unwrap_err(), SecretError::InGit);
        let with_env = find(root, PUBLICATION_TOKEN_ENV, Some(" from-env ".into()), Some("from-file")).unwrap();
        assert_eq!(with_env.value(), "from-env");
        assert!(with_env.in_git());
        std::fs::write(root.join(".git/info/exclude"), "/mustard.json\n").unwrap();
        assert_eq!(find(root, PUBLICATION_TOKEN_ENV, None, Some(" from-file ")).unwrap().value(), "from-file");
        assert!(git::run(root, &["add", "-f", "--", CONFIG_FILE]).ok);
        assert_eq!(find(root, PUBLICATION_TOKEN_ENV, None, Some("from-file")).unwrap_err(), SecretError::InGit);
        assert_eq!(find(root, PUBLICATION_TOKEN_ENV, None, None).unwrap_err(), SecretError::Missing);
    }

    /// A variável nas configurações locais vale pelo nome de cada segredo,
    /// e só quando o git não vê o arquivo.
    #[test]
    fn the_local_settings_answer_by_the_secret_name() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        repository(root);
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(root.join(LOCAL_SETTINGS), json!({"env": {PUBLICATION_TOKEN_ENV: "local-token"}}).to_string()).unwrap();
        assert_eq!(find(root, PUBLICATION_TOKEN_ENV, None, None).unwrap_err(), SecretError::Missing, "a file git does not ignore");
        std::fs::write(root.join(".git/info/exclude"), "/.claude/\n").unwrap();
        assert_eq!(find(root, PUBLICATION_TOKEN_ENV, None, None).unwrap().value(), "local-token");
        assert_eq!(find(root, KEY_ENV, None, None).unwrap_err(), SecretError::Missing, "another secret's name");
    }

    #[test]
    fn the_debug_text_of_a_secret_never_carries_it() {
        let found = choose(Some("sk-secret-value".to_string()), None, || false).unwrap();
        assert!(!format!("{found:?}").contains("sk-secret-value"), "{found:?}");
    }
}
