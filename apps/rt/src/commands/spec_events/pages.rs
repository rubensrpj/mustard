//! A página e o `.md` de uma spec, refeitos do `spec.ndjson` pelo motor de
//! página.
//!
//! Só o binário escreve os dois, e nunca a cada evento gravado: refazê-los
//! custa segundos na spec real, e gravar um evento tem de custar o tempo de
//! escrever uma linha. Quem os refaz, todos por [`refresh`]:
//!
//! - os passos do fluxo, no fim de cada um — hoje o `open`, o `grill` e o
//!   `plan`; o `round` e o `close` chamam a mesma porta quando existirem;
//! - o fim de cada onda, que é o `entregou` dela, por [`rebuild`], dentro da
//!   própria gravação;
//! - o `page --spec`, quando alguém pede.
//!
//! Os dois saem da mesma árvore (`view::document`), então dizem sempre a mesma
//! coisa, e a mesma lista de eventos dá sempre os mesmos bytes.
//!
//! Os dois são refeitos sempre com a trava do arquivo de eventos presa: o
//! `entregou` dentro da própria gravação, o [`refresh`] pedindo a trava. Assim
//! uma gravação nunca entra entre a leitura e a escrita da página, e a página
//! nunca fica atrás do arquivo.
//!
//! O [`refresh`] refaz também a página do projeto (`.claude/spec/project.html`),
//! montada só do índice das specs; o `index` a refaz quando refaz o índice.
//!
//! O `.html` é o que se publica, e sai conferido para isso:
//!
//! - todo trecho com cara de segredo (chave, token, senha) fica fora dele, com
//!   um aviso no lugar, até o item ser expurgado; o `.md`, que fica na máquina,
//!   continua inteiro;
//! - a página que passaria de [`PAGE_MAX_BYTES`] perde os registros mais
//!   antigos da conversa, e diz quantos ficaram só no `.md`.

mod secret;

use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::{Refusal, SpecLog};
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{translate, Locale};
use mustard_core::view::document::{
    conversation_len, cut_oldest_conversation, project_document, spec_page, Document, RtkDay, SpecInputs,
    WavePrompts,
};
use mustard_core::ClaudePaths;

use crate::report::Render;

/// O tamanho máximo, em bytes, de uma página que vai ser publicada: o
/// claude.ai aceita até 16 MB.
pub(crate) const PAGE_MAX_BYTES: usize = 16_000_000;

/// O nome da página do projeto, ao lado do índice das specs.
const PROJECT_PAGE: &str = "project.html";

/// Onde os arquivos foram gravados, relativos ao projeto, e o que a
/// conferência antes de publicar fez.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SpecPages {
    pub md: String,
    pub html: String,
    /// A página do projeto, quando foi refeita junto.
    pub project: Option<String>,
    /// O código de cada item que ficou fora da página por ter texto com cara
    /// de segredo.
    pub withheld: Vec<String>,
    /// Quantos registros da conversa ficaram só no `.md`.
    pub trimmed: usize,
    /// O que não impediu a página, mas precisa ser dito.
    pub warnings: Vec<String>,
}

/// A página pronta para publicar e o que a conferência tirou dela.
struct Publishable {
    html: String,
    withheld: Vec<String>,
    loose: usize,
    trimmed: usize,
    too_big: bool,
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
    // O rtk roda antes da trava: ninguém espera por ele para gravar.
    let rtk = rtk_days(root);
    let mut pages = store::with_locked_log(&files.events, |log| {
        let mut pages = write_pages(root, spec, &files, log, &rtk, lang)?;
        // A página do projeto sai do índice: a linha desta spec fica igual ao
        // arquivo que acabou de dar a página, mesmo que ele tenha sido
        // editado à mão.
        if let Some((index, name)) = mustard_core::io::spec_index::index_for(&files.events)
            && let Err(refusal) = mustard_core::io::spec_index::refresh_line(&index, &name, log)
        {
            pages.warnings.push(refusal.message(lang));
        }
        Ok(pages)
    })?
    .unwrap_or_else(|| Err(Refusal::NoSpecFile { spec: spec.trim().to_string() }))?;
    match refresh_project(root, lang) {
        Ok((path, warnings)) => {
            pages.project = Some(path);
            pages.warnings.extend(warnings);
        }
        Err(refusal) => pages.warnings.push(refusal.message(lang)),
    }
    Ok(pages)
}

/// Refaz os dois a partir de `log`, o arquivo de eventos que acabou de ser
/// gravado. Quem chama segura a trava do arquivo de eventos.
pub(crate) fn rebuild(root: &Path, spec: &str, log: &SpecLog, lang: Locale) -> Result<SpecPages, Refusal> {
    let rtk = rtk_days(root);
    write_pages(root, spec, &spec_files(root, spec)?, log, &rtk, lang)
}

/// A economia do rtk no projeto, só dos dias já fechados tanto na hora local
/// quanto na universal: o rtk pode contar o dia por qualquer uma delas.
fn rtk_days(root: &Path) -> Vec<RtkDay> {
    let local = mustard_core::io::spec_index::today();
    let universal = mustard_core::time::now_iso8601();
    let before = universal.get(..10).map_or(local.as_str(), |utc| utc.min(local.as_str()));
    crate::shared::rtk_gain::project_days(root, before)
}

/// Refaz a página do projeto a partir do índice das specs e devolve onde ela
/// foi gravada, relativo ao projeto, com os avisos da conferência.
pub(crate) fn refresh_project(root: &Path, lang: Locale) -> Result<(String, Vec<String>), Refusal> {
    let paths = ClaudePaths::for_project(root).map_err(|e| Refusal::Io { detail: e.to_string() })?;
    let index = paths.spec_index_path();
    let page = paths.spec_dir().join(PROJECT_PAGE);
    let lines = mustard_core::io::spec_index::read_rows(root);
    let name = root.file_name().map_or_else(|| "?".to_string(), |n| n.to_string_lossy().to_string());
    let today = mustard_core::io::spec_index::today();
    let doc = project_document(&name, &lines, &relative(root, &index), &today, lang);
    let checked = publishable(doc, lang, PAGE_MAX_BYTES);
    write(&page, &checked.html)?;
    Ok((relative(root, &page), warnings(&checked, lang)))
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
    rtk: &[RtkDay],
    lang: Locale,
) -> Result<SpecPages, Refusal> {
    // O pedido de cada onda é montado aqui, com o disco, e vai pronto para a
    // página: quem aprova lê exatamente o que o agente da onda vai ler.
    let prompts: WavePrompts = mustard_core::io::wave_prompt::prompts(root, spec.trim(), log, lang)
        .into_iter()
        .map(|built| (built.wave, built.text))
        .collect();
    let doc = spec_page(spec.trim(), log, SpecInputs { prompts: &prompts, rtk }, lang);
    write(&files.md, &Render::Md.render(&doc))?;
    let checked = publishable(doc, lang, PAGE_MAX_BYTES);
    write(&files.html, &checked.html)?;
    Ok(SpecPages {
        md: relative(root, &files.md),
        html: relative(root, &files.html),
        project: None,
        warnings: warnings(&checked, lang),
        withheld: checked.withheld,
        trimmed: checked.trimmed,
    })
}

fn write(path: &Path, text: &str) -> Result<(), Refusal> {
    mustard_core::io::fs::write_atomic(path, text.as_bytes()).map_err(|e| Refusal::Io { detail: e.to_string() })
}

/// A página `doc` pronta para publicar: sem trecho com cara de segredo e, se
/// passaria de `max` bytes, sem os registros mais antigos da conversa que
/// forem precisos para caber.
fn publishable(mut doc: Document, lang: Locale, max: usize) -> Publishable {
    let (withheld, loose) = doc.withhold(&secret::looks_like_secret, translate("page.withheld", lang));
    let html = Render::Html.render(&doc);
    if html.len() <= max {
        return Publishable { html, withheld, loose, trimmed: 0, too_big: false };
    }
    // Cortar um registro a mais nunca deixa a página maior: a menor quantidade
    // que cabe é achada pela metade do intervalo a cada volta.
    let cut = |count: usize| {
        let mut shorter = doc.clone();
        let trimmed = cut_oldest_conversation(&mut shorter, count, lang);
        (Render::Html.render(&shorter), trimmed)
    };
    let (mut low, mut high) = (1, conversation_len(&doc));
    let (mut best, mut trimmed) = cut(high);
    if high == 0 || best.len() > max {
        let too_big = best.len() > max;
        return Publishable { html: best, withheld, loose, trimmed, too_big };
    }
    while low < high {
        let mid = low + (high - low) / 2;
        let (html, count) = cut(mid);
        if html.len() <= max {
            (best, trimmed, high) = (html, count, mid);
        } else {
            low = mid + 1;
        }
    }
    Publishable { html: best, withheld, loose, trimmed, too_big: false }
}

/// O que a conferência antes de publicar precisa dizer.
fn warnings(checked: &Publishable, lang: Locale) -> Vec<String> {
    let mut out = Vec::new();
    let count = checked.withheld.len() + checked.loose;
    if count > 0 {
        let mut places = checked.withheld.clone();
        if checked.loose > 0 {
            places.push(format!("+{}", checked.loose));
        }
        out.push(
            translate("page.withheld_found", lang)
                .replace("{count}", &count.to_string())
                .replace("{codes}", &places.join(", ")),
        );
    }
    if checked.trimmed > 0 {
        out.push(translate("page.conversation.cut", lang).replace("{count}", &checked.trimmed.to_string()));
    }
    if checked.too_big {
        out.push(
            translate("page.too_big", lang)
                .replace("{bytes}", &checked.html.len().to_string())
                .replace("{max}", &PAGE_MAX_BYTES.to_string()),
        );
    }
    out
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

    fn put(root: &Path, spec: &str, event_type: &str, draft: serde_json::Value) -> u64 {
        let path = root.join(".claude").join("spec").join(spec).join("spec.ndjson");
        store::write(&path, event_type, draft.as_object().cloned().unwrap(), &[]).unwrap().id
    }

    fn read(root: &Path, relative: &str) -> String {
        std::fs::read_to_string(root.join(relative)).unwrap()
    }

    /// A página que passaria de 16 MB perde as mensagens mais antigas da
    /// conversa, só as precisas para caber, e diz quantas ficaram só no
    /// `.md`, que continua com todas.
    #[test]
    fn a_page_over_sixteen_megabytes_drops_the_oldest_messages_and_says_how_many() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        put(root, "grande", "state", serde_json::json!({"phase": "survey"}));
        let megabyte = |n: usize| format!("mensagem {n:02} {}", "palavra ".repeat(125_000));
        for n in 1..=18 {
            put(root, "grande", "message", serde_json::json!({"author": "user", "text": megabyte(n)}));
        }

        let pages = refresh(root, "grande", Locale::PtBr).expect("the page is built");
        let html = read(root, &pages.html);
        let md = read(root, &pages.md);
        assert!(html.len() <= PAGE_MAX_BYTES, "the page has {} bytes", html.len());
        assert!(pages.trimmed > 0, "{pages:?}");
        let cut = format!("Os {} registros mais antigos da conversa ficaram só no", pages.trimmed);
        assert!(html.contains(&format!("<p>{cut} <code>spec.md</code>")), "the page says how many stayed behind");
        assert!(pages.warnings.iter().any(|w| w.starts_with(&cut)), "{:?}", pages.warnings);
        for n in 1..=pages.trimmed {
            assert!(!html.contains(&format!("mensagem {n:02} ")), "message {n} is still on the page");
        }
        let kept = pages.trimmed + 1;
        assert!(html.contains(&format!("mensagem {kept:02} ")), "message {kept} was cut without need");
        for n in 1..=18 {
            assert!(md.contains(&format!("mensagem {n:02} ")), "message {n} left the .md");
        }
        // Com uma mensagem a menos, a página cabia: só as precisas saíram.
        let mut fewer = spec_page(
            "grande",
            &store::read(&root.join(".claude/spec/grande/spec.ndjson")).unwrap().unwrap(),
            SpecInputs { prompts: &WavePrompts::new(), rtk: &[] },
            Locale::PtBr,
        );
        cut_oldest_conversation(&mut fewer, pages.trimmed - 1, Locale::PtBr);
        assert!(Render::Html.render(&fewer).len() > PAGE_MAX_BYTES, "one entry fewer would not fit");
    }

    /// A página de uma spec pequena sai inteira, sem aviso.
    #[test]
    fn a_small_page_keeps_the_whole_conversation() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        put(root, "pequena", "state", serde_json::json!({"phase": "survey"}));
        put(root, "pequena", "message", serde_json::json!({"author": "user", "text": "oi"}));
        let pages = refresh(root, "pequena", Locale::PtBr).unwrap();
        assert_eq!((pages.trimmed, pages.withheld.len(), pages.warnings.len()), (0, 0, 0), "{pages:?}");
        assert!(!read(root, &pages.html).contains("ficaram só no"));
    }

    /// Com três specs, uma delas descartada, a página do projeto lista as
    /// três com o estado e o link da página de cada uma, e mostra no rodapé o
    /// caminho do índice de onde saiu.
    #[test]
    fn the_project_page_lists_every_spec_with_its_state_link_and_the_index_path() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let published = |spec: &str| {
            put(root, spec, "publish", serde_json::json!({"page": "spec", "milestone": "approval", "ok": true,
                "url": format!("https://claude.ai/code/artifact/{spec}")}));
        };
        put(root, "busca", "state", serde_json::json!({"phase": "running", "branch": "feature/busca"}));
        published("busca");
        put(root, "trava", "state", serde_json::json!({"phase": "plan"}));
        published("trava");
        put(root, "velha", "state", serde_json::json!({"phase": "survey"}));
        published("velha");
        put(root, "velha", "state", serde_json::json!({"phase": "discarded", "reason": "não serve mais"}));

        let pages = refresh(root, "trava", Locale::PtBr).unwrap();
        assert_eq!(pages.project.as_deref(), Some(".claude/spec/project.html"));
        let html = read(root, ".claude/spec/project.html");
        for (spec, state) in [("busca", "em execução"), ("trava", "plano"), ("velha", "descartada")] {
            let row = format!(
                "<tr><td><a href=\"https://claude.ai/code/artifact/{spec}\">{spec}</a></td><td>{state}</td>"
            );
            assert!(html.contains(&row), "{spec} is not listed with its state and link:\n{html}");
        }
        assert!(
            html.contains("<footer>Índice das specs: <code>.claude/spec/index.ndjson</code></footer>"),
            "the index path is in the footer:\n{html}"
        );
        assert!(html.contains("Por fase: 1 plano, 1 em execução, 1 descartada."), "{html}");
        assert!(html.contains("<li>specs <b>3</b></li>"), "{html}");
        crate::report::assert_only_the_fonts_are_external_but(&html, "https://claude.ai/code/artifact/");
    }

    /// Quando a página publicada foi apagada, a publicação falha, a página é
    /// refeita do arquivo de eventos e publicada num endereço novo: a página
    /// do projeto passa a apontar para ele.
    #[test]
    fn a_page_published_again_at_a_new_address_is_the_one_the_project_page_links() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let publish = |ok: bool, url: &str| {
            let draft = if ok {
                serde_json::json!({"page": "spec", "milestone": "round", "ok": true, "url": url})
            } else {
                serde_json::json!({"page": "spec", "milestone": "round", "ok": false, "reason": "a página foi apagada"})
            };
            put(root, "busca", "publish", draft);
        };
        put(root, "busca", "state", serde_json::json!({"phase": "running"}));
        publish(true, "https://claude.ai/code/artifact/antiga");
        publish(false, "");
        let pages = refresh(root, "busca", Locale::PtBr).expect("the page is rebuilt from the events");
        assert!(root.join(&pages.html).is_file());
        assert!(read(root, ".claude/spec/project.html").contains("artifact/antiga"), "the failure keeps the old address");

        publish(true, "https://claude.ai/code/artifact/nova");
        refresh(root, "busca", Locale::PtBr).unwrap();
        let html = read(root, ".claude/spec/project.html");
        assert!(html.contains("href=\"https://claude.ai/code/artifact/nova\""), "{html}");
        assert!(!html.contains("artifact/antiga"), "{html}");
    }

    /// Um trecho com cara de segredo não vai para a página: o item fica só
    /// com o código e o aviso de expurgar, a resposta diz qual é, e o `.md`
    /// local continua inteiro. Expurgado o item, a página volta a sair sem
    /// aviso.
    #[test]
    fn a_secret_never_reaches_the_page_until_the_item_is_purged() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        put(root, "s", "state", serde_json::json!({"phase": "survey"}));
        let said = put(root, "s", "message", serde_json::json!({"author": "user", "text": "a senha é hunter2-segredo"}));
        put(root, "s", "message", serde_json::json!({"author": "user", "text": "texto comum"}));

        let pages = refresh(root, "s", Locale::PtBr).unwrap();
        let html = read(root, &pages.html);
        assert!(!html.contains("hunter2"), "the secret reached the page");
        assert!(html.contains("<dd><p>Retido: este trecho tem texto com cara de segredo"), "{html}");
        assert!(html.contains("texto comum"));
        assert_eq!(pages.withheld, ["MSTD-MSG-0001"]);
        assert!(pages.warnings.iter().any(|w| w.contains("MSTD-MSG-0001") && w.contains("purge")), "{:?}", pages.warnings);
        assert!(read(root, &pages.md).contains("hunter2"), "the local .md keeps everything");

        put(root, "s", "purge", serde_json::json!({"targets": [said], "reason": "secret", "origin": said}));
        let pages = refresh(root, "s", Locale::PtBr).unwrap();
        assert!(pages.withheld.is_empty() && pages.warnings.is_empty(), "{pages:?}");
        assert!(!read(root, &pages.html).contains("Retido"));
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

