//! As páginas de uma spec: a cópia dela para o banco de dados da página
//! publicada e o motor antigo, que refaz o `.md` e o `.html` a pedido.
//!
//! A página publicada de uma spec, e a do projeto, são templates do Mustard
//! que leem o banco de dados guardado junto delas no claude.ai. O binário não
//! escreve mais o `spec.md`, o `spec.html` nem o `project.html` em passo
//! nenhum do fluxo: nos marcos (a aprovação, o fim de uma rodada e o
//! fechamento) e logo depois de um pedido que muda o plano, ele prepara a
//! cópia para o banco ([`copy`]), e o marco manda copiá-la, pela mesma porta,
//! [`end_milestone`]. A primeira vez de cada página, o marco manda antes
//! publicar o template dela e gravar o endereço.
//!
//! O item que guarda um trecho com cara de segredo não vai para o banco, e
//! nem segura o marco: o marco diz o código dele para ser expurgado. A cópia
//! que não pôde ser preparada vai para os avisos, e o marco não manda copiar
//! nada: a cópia seguinte leva os mesmos itens.
//!
//! O motor que monta a página e o `.md` (`view::document`) continua no
//! código, e só o comando `page --spec` o chama, por [`refresh`]: ele refaz os
//! dois a partir do `spec.ndjson`, com a trava do arquivo de eventos presa, e
//! a página do projeto a partir do índice. O `.html` sai conferido como
//! antes: todo trecho com cara de segredo sai dele como "…", e a página que
//! passaria de [`PAGE_MAX_BYTES`] perde os registros mais antigos da conversa.
//!
//! A lista dos itens sem dono (`owners.html`, ao lado da página da spec) sai
//! só quando alguém pede, pelo `page --spec <nome> --owners`, por
//! [`write_owners`].

pub(crate) mod copy;
pub(crate) mod secret;

use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::{Refusal, SpecLog};
use mustard_core::domain::wave_prompt::OwnerLine;
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{translate, Locale};
use mustard_core::view::document::{
    conversation_len, cut_oldest_conversation, owners_page, project_document, spec_page, Document, RtkDay,
    SpecInputs, WavePrompts,
};
use mustard_core::ClaudePaths;

use serde_json::{json, Value};

use crate::report::Render;

/// O tamanho máximo, em bytes, de uma página que vai ser publicada: o
/// claude.ai aceita até 16 MB.
pub(crate) const PAGE_MAX_BYTES: usize = 16_000_000;

/// O nome da página do projeto, ao lado do índice das specs.
const PROJECT_PAGE: &str = "project.html";

/// O nome da lista dos itens sem dono, ao lado da página da spec.
const OWNERS_PAGE: &str = "owners.html";

/// Onde o comando de página gravou os arquivos, relativos ao projeto, e o
/// que a conferência antes de publicar fez.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SpecPages {
    pub md: String,
    pub html: String,
    /// A página do projeto, quando foi refeita junto.
    pub project: Option<String>,
    /// O código de cada item que ainda guarda no arquivo um trecho com cara
    /// de segredo; na página, o trecho saiu como "…".
    pub withheld: Vec<String>,
    /// Quantos registros da conversa ficaram só no `.md`.
    pub trimmed: usize,
    /// O que não impediu a página, mas precisa ser dito.
    pub warnings: Vec<String>,
}

/// A página pronta para publicar e o que a conferência trocou nela.
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

/// O comando de página: refaz o `spec.md` e o `spec.html` da spec `spec` do
/// projeto `root`, com os rótulos no idioma `lang`, lendo o arquivo de eventos
/// com a trava presa, e a página do projeto a partir do índice. Nenhum passo
/// do fluxo chama esta função. Recusa um nome que não é de spec, uma spec sem
/// arquivo de eventos e uma spec do formato antigo.
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
fn refresh_project(root: &Path, lang: Locale) -> Result<(String, Vec<String>), Refusal> {
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
    let running = crate::commands::flow::round::waves_in_progress(log).into_keys().collect();
    let flight = mustard_core::io::wave_prompt::Flight { running, ..Default::default() };
    let prompts: WavePrompts = mustard_core::io::wave_prompt::prompts(root, spec.trim(), log, lang, &flight)
        .into_iter()
        .map(|built| (built.wave, built.text))
        .collect();
    // O estado de cada onda sai da mesma leitura que decide o que a rodada
    // despacha: a página não tem regra própria para ele.
    let waves = crate::commands::flow::round::wave_states(log);
    let doc = spec_page(spec.trim(), log, SpecInputs { prompts: &prompts, rtk, waves: &waves }, lang);
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

/// A lista dos itens sem dono gravada, e o que a conferência antes de
/// publicar fez nela.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnersPage {
    /// Onde ela foi gravada, relativo ao projeto: ao lado da página da spec.
    pub html: String,
    pub withheld: Vec<String>,
    pub warnings: Vec<String>,
}

/// Grava a lista dos itens sem dono da spec `spec` (`owners.html`, ao lado da
/// página da spec), conferida para publicar como as outras páginas. O arquivo
/// de eventos não muda.
pub(crate) fn write_owners(
    root: &Path,
    spec: &str,
    log: &SpecLog,
    lines: &[OwnerLine],
    lang: Locale,
) -> Result<OwnersPage, Refusal> {
    let path = spec_files(root, spec)?.html.with_file_name(OWNERS_PAGE);
    let checked = publishable(owners_page(spec.trim(), log, lines, lang), lang, PAGE_MAX_BYTES);
    write(&path, &checked.html)?;
    Ok(OwnersPage { html: relative(root, &path), warnings: warnings(&checked, lang), withheld: checked.withheld })
}

fn write(path: &Path, text: &str) -> Result<(), Refusal> {
    mustard_core::io::fs::write_atomic(path, text.as_bytes()).map_err(|e| Refusal::Io { detail: e.to_string() })
}

/// A página `doc` pronta para publicar: com cada trecho com cara de segredo
/// trocado por "…" e, se passaria de `max` bytes, sem os registros mais
/// antigos da conversa que forem precisos para caber.
fn publishable(mut doc: Document, lang: Locale, max: usize) -> Publishable {
    let (withheld, loose) = doc.redact(&secret::secret_excerpts, mustard_core::domain::spec_events::PURGED_MARK);
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

/// Um aviso a mais na lista `warnings` da resposta de um passo, que nasce
/// quando falta.
pub(crate) fn push_warning(report: &mut Value, reason: &str, hint: &str) {
    let warning = json!({ "reason": reason, "hint": hint });
    match report.get_mut("warnings").and_then(Value::as_array_mut) {
        Some(list) => list.push(warning),
        None => report["warnings"] = json!([warning]),
    }
}

/// O item que guarda um trecho com cara de segredo, na resposta de um passo
/// da spec `spec`: o código de cada um em `withheld` e, em `warnings`, a
/// ordem de expurgá-lo. Sem item assim, a resposta não muda.
pub(crate) fn note_withheld(report: &mut Value, spec: &str, withheld: &[String], lang: Locale) {
    if withheld.is_empty() {
        return;
    }
    report["withheld"] = json!(withheld);
    push_warning(report, "page-check", &purge_pending(spec, withheld, lang));
}

/// A ordem de expurgar os itens `withheld` da spec `spec`.
fn purge_pending(spec: &str, withheld: &[String], lang: Locale) -> String {
    translate("page.purge_pending", lang).replace("{codes}", &withheld.join(", ")).replace("{spec}", spec.trim())
}

/// O fim de um passo que é um marco (`approval`, `round` ou `close`) da spec
/// `spec`, com a cópia preparada: a resposta diz em `copy` os lotes de cada
/// página, em `publish` as páginas que ainda precisam da primeira publicação,
/// e manda, em `next`, publicar cada uma delas, copiar os lotes, gravar cada
/// cópia feita e expurgar o item que guarda um trecho com cara de segredo, e
/// segue com `then`. Quando a cópia não pôde ser preparada, o motivo vai
/// para os avisos e a resposta não manda copiar nada.
pub(crate) fn end_milestone(
    report: &mut Value,
    prepared: Result<&copy::Prepared, &Refusal>,
    spec: &str,
    milestone: &str,
    then: &str,
    lang: Locale,
) {
    let prepared = match prepared {
        Ok(prepared) => prepared,
        Err(refusal) => {
            push_warning(report, refusal.reason(), &refusal.message(lang));
            report["next"] = json!(format!("{} {then}", translate("page.copy.failed", lang)));
            return;
        }
    };
    note_withheld(report, spec, &prepared.withheld, lang);
    let publish = prepared.to_publish();
    if !publish.is_empty() {
        report["publish"] = json!(publish);
    }
    report["copy"] = prepared.to_value();
    let mut next = prepared.order(spec.trim(), Some(milestone), lang);
    if !prepared.withheld.is_empty() {
        next.push(purge_pending(spec, &prepared.withheld, lang));
    }
    next.push(then.to_string());
    report["next"] = json!(next.join(" "));
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
            SpecInputs { prompts: &WavePrompts::new(), rtk: &[], waves: &Default::default() },
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

    /// Com três specs, uma delas descartada pelo comando de descartar, a
    /// página do projeto que o comando de página refaz lista as três com o
    /// estado e o link da página de cada uma, e mostra no rodapé o caminho do
    /// índice de onde saiu. Ela continua listando a descartada quando o índice
    /// é refeito do zero.
    #[test]
    fn the_project_page_lists_every_spec_with_its_state_link_and_the_index_path() {
        use crate::commands::flow::discard::{discard_for, DiscardOpts};

        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
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

        let discard = |confirm: Option<String>| {
            discard_for(
                &DiscardOpts { root: root.to_path_buf(), spec: Some("velha".into()), remote: false, delete: false, confirm },
                None,
            )
        };
        let code = discard(None)["token"].as_str().map(str::to_string);
        let done = discard(code);
        assert_eq!(done["ok"], serde_json::json!(true), "{done}");
        assert!(!root.join(".claude/spec/velha").exists(), "the discarded spec was archived");

        let pages = refresh(root, "trava", Locale::PtBr).unwrap();
        assert_eq!(pages.project.as_deref(), Some(".claude/spec/project.html"));
        let html = read(root, ".claude/spec/project.html");
        for (spec, state) in [("busca", "tag run\">em execução"), ("trava", "tag\">plano"), ("velha", "tag\">descartada")] {
            let row = format!("<code class=\"c\">{spec}</code><span class=\"t\"></span><span class=\"tail\"><span class=\"{state}</span>");
            assert!(html.contains(&row), "{spec} is not listed with its state:\n{html}");
            let link = format!("<dd><a href=\"https://claude.ai/code/artifact/{spec}\">{spec}</a></dd>");
            assert!(html.contains(&link), "{spec} is not listed with its link:\n{html}");
        }
        assert!(
            html.contains("<footer>Índice das specs: <code>.claude/spec/index.ndjson</code></footer>"),
            "the index path is in the footer:\n{html}"
        );
        assert!(html.contains("Por fase: 1 plano, 1 em execução, 1 descartada."), "{html}");
        assert!(html.contains("<li>specs <b>3</b></li>"), "{html}");
        crate::report::assert_only_the_fonts_are_external_but(&html, "https://claude.ai/code/artifact/");

        std::fs::remove_file(root.join(".claude/spec/index.ndjson")).unwrap();
        std::fs::remove_file(root.join(".claude/spec/project.html")).unwrap();
        crate::commands::spec_events::index::index_at(&crate::commands::spec_events::index::IndexOpts {
            root: root.to_path_buf(),
        });
        refresh(root, "trava", Locale::PtBr).unwrap();
        assert_eq!(read(root, ".claude/spec/project.html"), html, "the rebuilt index keeps the discarded spec");
    }

    /// A publicação da página do projeto, gravada pelo `run write` na spec em
    /// que o passo corre, leva o endereço para a linha do projeto do índice,
    /// de onde a barra de status o lê; o link da página da spec não muda.
    #[test]
    fn the_project_page_address_is_recorded_on_the_project_line() {
        use crate::commands::spec_events::write::{write_at, WriteOpts};

        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        put(root, "busca", "state", serde_json::json!({"phase": "plan"}));
        let publish = |page: &str, url: &str| {
            write_at(&WriteOpts {
                root: root.to_path_buf(),
                spec: Some("busca".into()),
                event_type: "publish".into(),
                json: serde_json::json!({"page": page, "milestone": "approval", "ok": true, "url": url}).to_string(),
            })
        };
        let spec = publish("spec", "https://claude.ai/code/artifact/busca");
        assert_eq!(spec["ok"], serde_json::json!(true), "{spec}");
        let project = publish("project", "https://claude.ai/code/artifact/projeto");
        assert_eq!(project["ok"], serde_json::json!(true), "{project}");

        let index = read(root, ".claude/spec/index.ndjson");
        assert_eq!(
            mustard_core::domain::spec_index::project_url(&index).as_deref(),
            Some("https://claude.ai/code/artifact/projeto"),
            "{index}"
        );
        let rows = mustard_core::io::spec_index::read_rows(root);
        assert_eq!(rows[0].url.as_deref(), Some("https://claude.ai/code/artifact/busca"), "{rows:?}");
        refresh(root, "busca", Locale::PtBr).unwrap();
        let html = read(root, ".claude/spec/project.html");
        assert!(html.contains("href=\"https://claude.ai/code/artifact/busca\""), "{html}");
        assert!(!html.contains("artifact/projeto"), "the project page does not link itself as a spec: {html}");
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

    /// Um trecho com cara de segredo não vai para a página: ele sai como "…",
    /// o resto do item fica, a resposta diz o código de cada item que ainda o
    /// guarda no arquivo, e o `.md` local continua inteiro. Vale para uma
    /// mensagem, um ponto e um veredito. Expurgados os itens pela gravação, os
    /// três continuam na página, com o trecho oculto, e o aviso some.
    #[test]
    fn a_secret_never_reaches_the_page_and_the_purged_items_stay_with_the_excerpt_hidden() {
        use crate::commands::spec_events::write::{write_at, WriteOpts};

        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        put(root, "s", "state", serde_json::json!({"phase": "survey"}));
        let token = ["ghp_", &"b2".repeat(18)].concat();
        put(root, "s", "message", serde_json::json!({"author": "user", "text": "a senha é hunter2-segredo"}));
        put(root, "s", "message", serde_json::json!({"author": "user", "text": "texto comum"}));
        put(root, "s", "point", serde_json::json!({"block": "limits", "gap": "o acesso ao banco", "from": "gap",
            "status": "open", "origin": 2,
            "facts": [{"text": "o banco usa DB_PASSWORD=S3nh4F0rte2024", "source": "mensagem 2"}]}));
        put(root, "s", "verdict", serde_json::json!({"author": "review", "wave": 1, "result": "rejected",
            "text": format!("O log mostra o token {token}."), "criteria": [{"criterion": 1, "tests_rule": true}]}));

        let pages = refresh(root, "s", Locale::PtBr).unwrap();
        let html = read(root, &pages.html);
        for secret in ["hunter2", "S3nh4F0rte2024", token.as_str()] {
            assert!(!html.contains(secret), "{secret} reached the page");
        }
        for kept in ["a senha é …", "texto comum", "o banco usa DB_PASSWORD=…", "O log mostra o token …."] {
            assert!(html.contains(kept), "{kept} is not on the page:\n{html}");
        }
        assert_eq!(pages.withheld, ["MSTD-POINT-0001", "MSTD-VERD-0001", "MSTD-MSG-0001"], "page order: {pages:?}");
        assert!(pages.warnings.iter().any(|w| w.contains("MSTD-VERD-0001") && w.contains("purge")), "{:?}", pages.warnings);
        assert!(read(root, &pages.md).contains("hunter2"), "the local .md keeps everything");

        for code in ["MSTD-MSG-0001", "MSTD-POINT-0001", "MSTD-VERD-0001"] {
            let purged = write_at(&WriteOpts {
                root: root.to_path_buf(),
                spec: Some("s".into()),
                event_type: "purge".into(),
                json: serde_json::json!({"targets": [code], "reason": "secret"}).to_string(),
            });
            assert_eq!(purged["ok"], serde_json::json!(true), "{code}: {purged}");
        }
        let pages = refresh(root, "s", Locale::PtBr).unwrap();
        assert!(pages.withheld.is_empty() && pages.warnings.is_empty(), "{pages:?}");
        let html = read(root, &pages.html);
        for kept in ["a senha é …", "o banco usa DB_PASSWORD=…", "O log mostra o token …."] {
            assert!(html.contains(kept), "{kept} left the page after the purge");
        }
        let md = read(root, &pages.md);
        assert!(!md.contains("hunter2") && !md.contains(&token), "the purge took the excerpt out of the file");
    }

    /// O item revisto com o segredo nas duas versões sai uma vez só na lista
    /// dos que ainda guardam o trecho, e o segredo num pedido já enviado diz o
    /// código do envio, não só que um trecho saiu.
    #[test]
    fn each_withheld_item_is_named_once_and_a_sent_request_by_its_code() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        put(root, "s", "state", serde_json::json!({"phase": "running"}));
        let said = put(root, "s", "message", serde_json::json!({"author": "user", "text": "combine"}));
        let old = put(root, "s", "decision", serde_json::json!({"text": "A senha do banco: S3nh4F0rte", "why": "w",
            "keys": ["banco"], "origin": said}));
        put(root, "s", "decision", serde_json::json!({"text": "A senha do banco: 0utr4S3nh4", "why": "w",
            "keys": ["banco"], "origin": said, "replaces": old}));
        put(root, "s", "send", serde_json::json!({"wave": 1, "role": "wave", "lines": 2, "chars": 40, "items": [said],
            "mustard": "0.0.0", "text": "# Pedido\nDB_PASSWORD=S3nh4F0rte2024", "author": "binary"}));

        let pages = refresh(root, "s", Locale::PtBr).unwrap();
        assert_eq!(pages.withheld, ["MSTD-DEC-0001", "MSTD-SEND-0001"], "{pages:?}");
        let html = read(root, &pages.html);
        assert!(!html.contains("S3nh4F0rte") && !html.contains("0utr4S3nh4"), "a secret reached the page");
        let warning = pages.warnings.iter().find(|w| w.contains("purge")).cloned().unwrap_or_default();
        assert!(warning.contains("MSTD-DEC-0001, MSTD-SEND-0001") && !warning.contains('+'), "{warning}");
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

