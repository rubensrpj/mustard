//! A página e o `.md` de uma spec, refeitos do `spec.ndjson` pelo motor de
//! página.
//!
//! Só o binário escreve os dois. O `write` os refaz a cada evento gravado, e
//! o `page --spec` quando alguém pede. Os dois saem da mesma árvore
//! (`view::document`), então dizem sempre a mesma coisa, e a mesma lista de
//! eventos dá sempre os mesmos bytes.

use std::path::Path;

use mustard_core::domain::spec_events::Refusal;
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

/// Refaz o `spec.md` e o `spec.html` da spec `spec` do projeto `root`, com os
/// rótulos no idioma `lang`. Recusa um nome que não é de spec e uma spec sem
/// arquivo de eventos.
pub(crate) fn refresh(root: &Path, spec: &str, lang: Locale) -> Result<SpecPages, Refusal> {
    let name = spec.trim();
    let paths = ClaudePaths::for_project(root)
        .map_err(|e| Refusal::Io { detail: e.to_string() })?
        .for_spec(name)
        .map_err(|_| Refusal::BadSpecName { spec: spec.to_string() })?;
    let Some(log) = store::read(&paths.spec_ndjson_path())? else {
        return Err(Refusal::NoSpecFile { spec: name.to_string() });
    };
    let doc = spec_document(name, &log, lang);
    let md = paths.spec_md_path();
    let html = paths.spec_html_path();
    for (path, render) in [(&md, Render::Md), (&html, Render::Html)] {
        mustard_core::io::fs::write_atomic(path, render.render(&doc).as_bytes())
            .map_err(|e| Refusal::Io { detail: e.to_string() })?;
    }
    Ok(SpecPages { md: relative(root, &md), html: relative(root, &html) })
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}
