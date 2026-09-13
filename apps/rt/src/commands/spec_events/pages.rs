//! A página e o `.md` de uma spec, refeitos do `spec.ndjson` pelo motor de
//! página.
//!
//! Só o binário escreve os dois. O `write` os refaz a cada evento gravado, e
//! o `page --spec` quando alguém pede. Os dois saem da mesma árvore
//! (`view::document`), então dizem sempre a mesma coisa, e a mesma lista de
//! eventos dá sempre os mesmos bytes.
//!
//! Os dois são refeitos sempre com a trava do arquivo de eventos presa: o
//! `write` dentro da própria gravação, o `page --spec` pedindo a trava. Assim
//! uma gravação nunca entra entre a leitura e a escrita da página, e a página
//! nunca fica atrás do arquivo.

use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::{Refusal, SpecLog};
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::Locale;
use mustard_core::view::document::spec_document;
use mustard_core::ClaudePaths;

use crate::report::Render;

/// Onde os dois arquivos foram gravados, relativos ao projeto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SpecPages {
    pub md: String,
    pub html: String,
}

/// Os três arquivos de uma spec.
struct SpecFiles {
    events: PathBuf,
    md: PathBuf,
    html: PathBuf,
}

/// O `spec.md` da spec é um documento, e não a página refeita do arquivo de
/// eventos: o `meta.json` do `spec-draft` está na pasta, ou o `spec.md` traz a
/// seção de critérios de aceitação, de onde o QA lê os ACs. Ali a página e o
/// `.md` nunca são refeitos do arquivo de eventos, que apagaria o texto da
/// spec: o `write` e as pontes do binário só gravam o evento, e o
/// `page --spec` recusa. A única conferência disso.
pub(crate) fn drafted_by_spec_draft(root: &Path, spec: &str) -> bool {
    let Ok(paths) = ClaudePaths::for_project(root).and_then(|paths| paths.for_spec(spec.trim())) else {
        return false;
    };
    paths.meta_json_path().is_file()
        || std::fs::read_to_string(paths.spec_md_path())
            .ok()
            .and_then(|md| crate::commands::review::qa_run::extract_ac_section(&md))
            .is_some()
}

/// Refaz o `spec.md` e o `spec.html` da spec `spec` do projeto `root`, com os
/// rótulos no idioma `lang`, lendo o arquivo de eventos com a trava presa.
/// Recusa um nome que não é de spec, uma spec sem arquivo de eventos e uma
/// spec do `spec-draft`.
pub(crate) fn refresh(root: &Path, spec: &str, lang: Locale) -> Result<SpecPages, Refusal> {
    if drafted_by_spec_draft(root, spec) {
        return Err(Refusal::DraftedSpec { spec: spec.trim().to_string() });
    }
    let files = spec_files(root, spec)?;
    store::with_locked_log(&files.events, |log| write_pages(root, spec, &files, log, lang))?
        .unwrap_or_else(|| Err(Refusal::NoSpecFile { spec: spec.trim().to_string() }))
}

/// Refaz os dois a partir de `log`, o arquivo de eventos que acabou de ser
/// gravado. Quem chama segura a trava do arquivo de eventos.
pub(crate) fn rebuild(root: &Path, spec: &str, log: &SpecLog, lang: Locale) -> Result<SpecPages, Refusal> {
    write_pages(root, spec, &spec_files(root, spec)?, log, lang)
}

fn spec_files(root: &Path, spec: &str) -> Result<SpecFiles, Refusal> {
    let paths = ClaudePaths::for_project(root)
        .map_err(|e| Refusal::Io { detail: e.to_string() })?
        .for_spec(spec.trim())
        .map_err(|_| Refusal::BadSpecName { spec: spec.to_string() })?;
    Ok(SpecFiles { events: paths.spec_ndjson_path(), md: paths.spec_md_path(), html: paths.spec_html_path() })
}

fn write_pages(
    root: &Path,
    spec: &str,
    files: &SpecFiles,
    log: &SpecLog,
    lang: Locale,
) -> Result<SpecPages, Refusal> {
    let doc = spec_document(spec.trim(), log, lang);
    for (path, render) in [(&files.md, Render::Md), (&files.html, Render::Html)] {
        mustard_core::io::fs::write_atomic(path, render.render(&doc).as_bytes())
            .map_err(|e| Refusal::Io { detail: e.to_string() })?;
    }
    Ok(SpecPages { md: relative(root, &files.md), html: relative(root, &files.html) })
}

/// O caminho relativo ao projeto, com barras normais: a saída não traz o
/// caminho da máquina.
pub(crate) fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Numa spec do `spec-draft`, o `page --spec` recusa e não toca no
    /// `spec.md` dele, nem cria a página.
    #[test]
    fn the_page_is_never_rebuilt_over_a_drafted_spec() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let spec_dir = root.join(".claude").join("spec").join("rascunho");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("meta.json"), r#"{"scope":"light","stage":"Plan"}"#).unwrap();
        std::fs::write(spec_dir.join("spec.md"), "# Rascunho\n\nO texto da spec.\n").unwrap();
        let plan = serde_json::json!({ "phase": "plan" });
        store::write(&spec_dir.join("spec.ndjson"), "state", plan.as_object().cloned().unwrap(), &[])
            .unwrap();

        let refused = refresh(root, "rascunho", Locale::PtBr).unwrap_err();
        assert_eq!(refused.reason(), "drafted-spec");
        assert_eq!(std::fs::read_to_string(spec_dir.join("spec.md")).unwrap(), "# Rascunho\n\nO texto da spec.\n");
        assert!(!spec_dir.join("spec.html").exists(), "no page over a draft");
    }
}
