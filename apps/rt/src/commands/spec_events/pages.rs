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

/// Refaz o `spec.md` e o `spec.html` da spec `spec` do projeto `root`, com os
/// rótulos no idioma `lang`, lendo o arquivo de eventos com a trava presa.
/// Recusa um nome que não é de spec e uma spec sem arquivo de eventos.
pub(crate) fn refresh(root: &Path, spec: &str, lang: Locale) -> Result<SpecPages, Refusal> {
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

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}
