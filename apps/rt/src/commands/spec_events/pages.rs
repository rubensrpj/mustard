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
use mustard_core::view::document::{spec_document, WavePrompts};
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

/// A pasta é de uma spec do formato antigo, cujo `spec.md` é o documento e não
/// a página refeita do arquivo de eventos. Dois sinais, e a diferença entre
/// eles importa:
///
/// - o `spec.md` com a seção de critérios de aceitação, de onde o fluxo antigo
///   lia os critérios, marca a spec como antiga sempre;
/// - o `meta.json` marca só quando não há `spec.ndjson`: uma spec aberta pelo
///   `open` pode ganhar um `meta.json` de uma porta antiga e continua nova.
///
/// Numa pasta assim o binário não grava nada: nem o evento, nem a página, que
/// refeita do arquivo de eventos apagaria o texto da spec. A única conferência
/// disso.
pub(crate) fn old_format_spec(root: &Path, spec: &str) -> bool {
    let Ok(paths) = ClaudePaths::for_project(root).and_then(|paths| paths.for_spec(spec.trim())) else {
        return false;
    };
    let criteria_section = std::fs::read_to_string(paths.spec_md_path())
        .ok()
        .and_then(|md| crate::commands::review::qa_run::extract_ac_section(&md))
        .is_some();
    criteria_section || (paths.meta_json_path().is_file() && !paths.spec_ndjson_path().is_file())
}

/// Refaz o `spec.md` e o `spec.html` da spec `spec` do projeto `root`, com os
/// rótulos no idioma `lang`, lendo o arquivo de eventos com a trava presa.
/// Recusa um nome que não é de spec, uma spec sem arquivo de eventos e uma
/// spec do formato antigo.
pub(crate) fn refresh(root: &Path, spec: &str, lang: Locale) -> Result<SpecPages, Refusal> {
    if old_format_spec(root, spec) {
        return Err(Refusal::OldFormatSpec { spec: spec.trim().to_string() });
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
    // O pedido de cada onda é montado aqui, com o disco, e vai pronto para a
    // página: quem aprova lê exatamente o que o agente da onda vai ler.
    let prompts: WavePrompts = mustard_core::io::wave_prompt::prompts(root, spec.trim(), log, lang)
        .into_iter()
        .map(|built| (built.wave, built.text))
        .collect();
    let doc = spec_document(spec.trim(), log, &prompts, lang);
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

    /// Numa spec do formato antigo, o `page --spec` recusa e não toca no
    /// `spec.md` dela, nem cria a página. Os dois sinais valem: a pasta só com
    /// o `meta.json`, e o `spec.md` com a seção de critérios de aceitação,
    /// mesmo com arquivo de eventos ao lado.
    #[test]
    fn the_page_is_never_rebuilt_over_an_old_format_spec() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let text = "# Rascunho\n\nO texto da spec.\n";
        let with_criteria = "# Rascunho\n\n## Acceptance Criteria\n\n- [ ] AC-1: passa — Command: `cd .`\n";
        for (spec, md, with_events) in
            [("so-meta", text, false), ("com-criterios", with_criteria, true)]
        {
            let spec_dir = root.join(".claude").join("spec").join(spec);
            std::fs::create_dir_all(&spec_dir).unwrap();
            std::fs::write(spec_dir.join("meta.json"), r#"{"scope":"light","stage":"Plan"}"#).unwrap();
            std::fs::write(spec_dir.join("spec.md"), md).unwrap();
            if with_events {
                let plan = serde_json::json!({ "phase": "plan" });
                store::write(&spec_dir.join("spec.ndjson"), "state", plan.as_object().cloned().unwrap(), &[])
                    .unwrap();
            }

            let refused = refresh(root, spec, Locale::PtBr).unwrap_err();
            assert_eq!(refused.reason(), "old-format-spec", "{spec}");
            assert_eq!(std::fs::read_to_string(spec_dir.join("spec.md")).unwrap(), md, "{spec}");
            assert!(!spec_dir.join("spec.html").exists(), "{spec}: no page over an old spec");
        }
    }

    /// Uma spec com arquivo de eventos nunca é do formato antigo por um
    /// `meta.json` que apareceu ao lado: a página dela continua sendo refeita.
    #[test]
    fn a_spec_with_an_event_file_is_never_old_format_even_with_a_meta_json() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let spec_dir = root.join(".claude").join("spec").join("nova");
        std::fs::create_dir_all(&spec_dir).unwrap();
        let survey = serde_json::json!({ "phase": "survey" });
        store::write(&spec_dir.join("spec.ndjson"), "state", survey.as_object().cloned().unwrap(), &[]).unwrap();
        std::fs::write(spec_dir.join("meta.json"), r#"{"scope":"light","stage":"Plan"}"#).unwrap();

        assert!(!old_format_spec(root, "nova"));
        let pages = refresh(root, "nova", Locale::PtBr).expect("the page is rebuilt");
        assert!(pages.html.ends_with("spec.html"), "{pages:?}");
        assert!(spec_dir.join("spec.html").is_file());
    }
}
