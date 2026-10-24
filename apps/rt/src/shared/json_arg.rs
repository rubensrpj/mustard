//! O valor de um argumento JSON dos comandos `run` (`--json`, `--facts`,
//! `--args-json`): o texto como veio ou, quando começa com `@`, o conteúdo
//! inteiro do arquivo UTF-8 cujo caminho segue a arroba. O texto longo, com
//! acento, aspas, crase e dois-pontos, chega ao programa por um arquivo
//! gravado pela ferramenta de escrita, sem script ao lado que monte a linha
//! de comando. Nenhum JSON começa com `@`, então o valor direto segue como
//! sempre. O caminho relativo parte da pasta em que o comando roda.

use std::path::Path;

/// O texto do argumento `raw`: o do arquivo, com `@caminho`, ou o próprio
/// valor. O arquivo que falta ou não se lê como UTF-8 dá o motivo com o
/// caminho, e quem chama recusa antes de gravar qualquer coisa.
pub(crate) fn resolve(raw: String) -> Result<String, String> {
    resolve_in(raw, Path::new(""))
}

/// [`resolve`] com o caminho relativo partindo de `folder`, a pasta em que o
/// comando rodou: é como o gancho relê o argumento de um comando que já
/// rodou. O caminho absoluto ignora `folder`.
pub(crate) fn resolve_in(raw: String, folder: &Path) -> Result<String, String> {
    let Some(path) = raw.strip_prefix('@') else { return Ok(raw) };
    std::fs::read_to_string(folder.join(path)).map_err(|error| format!("@{path}: {error}"))
}

/// [`resolve`] do argumento opcional `raw`.
pub(crate) fn resolve_opt(raw: Option<String>) -> Result<Option<String>, String> {
    raw.map(resolve).transpose()
}
