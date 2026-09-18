//! A página e o `.md` de uma spec, refeitos do `spec.ndjson` pelo motor de
//! página.
//!
//! Só o binário escreve os dois, e nunca a cada evento gravado: refazê-los
//! custa segundos na spec real, e gravar um evento tem de custar o tempo de
//! escrever uma linha. Quem os refaz, todos por [`refresh`]:
//!
//! - os passos do fluxo, no fim de cada um: o `open`, o `grill`, o `plan`, o
//!   `round` e o `close`;
//! - o fim de cada onda, que é o `entregou` dela, por [`rebuild`], dentro da
//!   própria gravação;
//! - o `page --spec`, quando alguém pede.
//!
//! Os dois saem da mesma árvore (`view::document`), então dizem sempre a mesma
//! coisa, e a mesma lista de eventos dá sempre os mesmos bytes. O estado de
//! cada onda vai pronto para a árvore, lido pela rodada.
//!
//! Os dois são refeitos sempre com a trava do arquivo de eventos presa: o
//! `entregou` dentro da própria gravação, o [`refresh`] pedindo a trava. Assim
//! uma gravação nunca entra entre a leitura e a escrita da página, e a página
//! nunca fica atrás do arquivo.
//!
//! O [`refresh`] refaz também a página do projeto (`.claude/spec/project.html`),
//! montada só do índice das specs; o `index` a refaz quando refaz o índice.
//!
//! A lista dos itens sem dono (`owners.html`, ao lado da página da spec) sai
//! só quando alguém pede, pelo `page --spec <nome> --owners`, por
//! [`write_owners`].
//!
//! O `.html` é o que se publica, e sai conferido para isso:
//!
//! - todo trecho com cara de segredo (chave, token, senha) sai dele como "…",
//!   e o resto do item fica; o aviso diz o código de cada item que ainda guarda
//!   o trecho no arquivo, para ser expurgado; o `.md`, que fica na máquina,
//!   continua inteiro;
//! - a página que passaria de [`PAGE_MAX_BYTES`] perde os registros mais
//!   antigos da conversa, e diz quantos ficaram só no `.md`.
//!
//! Só os marcos mandam publicar — a aprovação, o fim de uma rodada e o
//! fechamento —, todos pela mesma porta, [`end_milestone`]: o item que ainda
//! guarda um trecho com cara de segredo não segura a publicação, e o marco diz
//! o código dele para ser expurgado; com a página da spec ou a do projeto que
//! não pôde ser refeita, diz qual falhou e por quê, e não manda publicar.
//!
//! Para a página que já foi publicada, a ordem de publicar diz o que entrou na
//! spec depois da última publicação dela ([`published_pages`]), pela data e
//! hora gravadas no arquivo de eventos, lidas com o fuso: a conversa confere
//! só essa lista e publica, sem ler o `.md` nem o `.html`. A lista leva só os
//! itens que têm texto para ler ([`READABLE_TYPES`]); os registros internos,
//! como a marca de um gancho, de uma chamada de comando ou de um texto
//! injetado, ficam fora. E manda ler antes o endereço gravado quando a
//! conversa ainda não publicou a página, porque a ferramenta de publicar
//! exige.

pub(crate) mod secret;

use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::{type_spec, Refusal, SpecEvent, SpecLog};
use mustard_core::domain::spec_index::{published_to, title_of, PROJECT_PAGE as PROJECT_KEY, SPEC_PAGE as SPEC_KEY};
use mustard_core::domain::wave_prompt::OwnerLine;
use mustard_core::io::spec_events as store;
use mustard_core::io::spec_index::later;
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

/// Onde os arquivos foram gravados, relativos ao projeto, e o que a
/// conferência antes de publicar fez.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SpecPages {
    pub md: String,
    pub html: String,
    /// A página do projeto, quando foi refeita junto.
    pub project: Option<String>,
    /// Por que a página do projeto não pôde ser refeita junto. Com ela
    /// faltando, nenhum marco manda publicar.
    pub project_failed: Option<Refusal>,
    /// O código de cada item que ainda guarda no arquivo um trecho com cara
    /// de segredo; na página, o trecho saiu como "…".
    pub withheld: Vec<String>,
    /// Quantos registros da conversa ficaram só no `.md`.
    pub trimmed: usize,
    /// O que não impediu a página, mas precisa ser dito.
    pub warnings: Vec<String>,
    /// Cada página que já foi publicada, com o que entrou na spec depois da
    /// última publicação dela; a que nunca foi publicada fica de fora.
    pub published: Vec<Published>,
}

/// Uma página já publicada: a última publicação dela que deu certo, lida do
/// arquivo de eventos, e o que entrou na spec depois.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Published {
    pub page: Page,
    /// A data e a hora da publicação, como gravadas.
    pub at: String,
    /// O endereço gravado na publicação.
    pub url: String,
    /// Os itens com texto para ler gravados depois dela, na ordem do arquivo.
    pub changed: Vec<Changed>,
}

/// Um item que entrou na spec depois da última publicação de uma página.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Changed {
    pub code: String,
    /// O texto curto: o rótulo do item; sem ele, o título ou a primeira frase
    /// do texto; sem texto, o nome do tipo.
    pub text: String,
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
        Err(refusal) => {
            pages.warnings.push(not_rebuilt(Page::Project, &refusal, lang));
            pages.project_failed = Some(refusal);
        }
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
        project_failed: None,
        warnings: warnings(&checked, lang),
        withheld: checked.withheld,
        trimmed: checked.trimmed,
        published: published_pages(log, lang),
    })
}

/// Os tipos que têm texto para ler, os únicos que a lista da ordem de
/// publicar leva: a conversa, o combinado, a especificação, os critérios, as
/// ondas com as entregas, a revisão e as anotações. Os outros são registros
/// internos, que o binário ou um gancho grava sozinho, e ficam fora: a marca
/// de um gancho (`hook`), o texto que ele colocou (`injection`), a chamada
/// de um comando (`call`), o pedido montado para um agente (`send`), a troca
/// de fase (`state`), a publicação (`publish`), a rodada de uma prova
/// (`criterion_run`) e o commit (`commit`).
const READABLE_TYPES: &[&str] = &[
    // A conversa.
    "message",
    "response",
    "remove",
    "purge",
    // O combinado.
    "work_type",
    "point",
    "rule",
    "limit",
    "contract",
    "error",
    "edge_case",
    "out_of_scope",
    "decision",
    // A especificação e os critérios.
    "context",
    "concern",
    "criterion",
    // As ondas, a revisão e o andamento.
    "wave",
    "task",
    "skill",
    "delivered",
    "verdict",
    "pr_summary",
    // As anotações.
    "request",
    "deferred",
    "note",
];

/// Cada página que já foi publicada, a da spec e a do projeto, nessa ordem:
/// a última publicação dela que deu certo e os itens com texto para ler
/// gravados depois dela. As publicações ficam fora da lista, como os outros
/// registros internos: elas dizem onde a página está, não o que mudou nela.
fn published_pages(log: &SpecLog, lang: Locale) -> Vec<Published> {
    let visible = log.visible();
    let codes = log.codes();
    [Page::Spec, Page::Project]
        .into_iter()
        .filter_map(|page| {
            let (place, last, url) = visible
                .iter()
                .enumerate()
                .filter_map(|(i, e)| published_to(e, page.key()).map(|url| (i, *e, url)))
                .next_back()?;
            let changed = visible
                .iter()
                .enumerate()
                .filter(|(i, e)| {
                    READABLE_TYPES.contains(&e.event_type.as_str()) && recorded_after(e, *i, last, place)
                })
                .map(|(_, e)| Changed {
                    code: codes.get(&e.id).cloned().unwrap_or_else(|| e.id.to_string()),
                    text: short_text(e, lang),
                })
                .collect();
            Some(Published { page, at: last.at().to_string(), url: url.to_string(), changed })
        })
        .collect()
}

/// O item `event`, na posição `place` do arquivo, foi gravado depois da
/// publicação `publication`, na posição `published_place`: pela data e hora
/// de cada um, lidas com o fuso, como o índice das specs as compara. No
/// mesmo instante, vale a ordem do arquivo.
fn recorded_after(event: &SpecEvent, place: usize, publication: &SpecEvent, published_place: usize) -> bool {
    if later(event.at(), publication.at()) {
        return true;
    }
    !later(publication.at(), event.at()) && place > published_place
}

/// O texto curto de um item: o rótulo; sem ele, o título ou a primeira frase
/// do texto; sem texto, o nome do tipo, como a página o diz (o tipo que este
/// binário não conhece sai com o nome gravado).
fn short_text(event: &SpecEvent, lang: Locale) -> String {
    let label = event.str_field("label").map(str::trim).filter(|l| !l.is_empty()).map(str::to_string);
    let text = label.or_else(|| title_of(event)).unwrap_or_else(|| match type_spec(&event.event_type) {
        Some(_) => translate(&format!("page.type.{}", event.event_type), lang).to_string(),
        None => event.event_type.clone(),
    });
    // O ponto final da frase sai: os itens vão separados por ponto e vírgula.
    text.trim_end_matches('.').to_string()
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

/// O que a conferência da página achou, na resposta do passo: o código de
/// cada item que ainda guarda um trecho a expurgar em `withheld` e cada aviso
/// em `warnings`.
pub(crate) fn note_checked(report: &mut Value, pages: &SpecPages) {
    if !pages.withheld.is_empty() {
        report["withheld"] = json!(pages.withheld);
    }
    for warning in &pages.warnings {
        push_warning(report, "page-check", warning);
    }
}

/// O fim de um passo que é um marco (`approval`, `round` ou `close`), com o
/// resultado de refazer a página: a resposta manda publicar a página da spec e
/// a do projeto, diz como gravar cada publicação, diz o código de cada item
/// que ainda guarda um trecho a expurgar, e segue com `then`. Quando uma das
/// duas páginas não pôde ser refeita, o motivo vai para os avisos e a resposta
/// não manda publicar: manda refazer a página, publicar e então `then`.
pub(crate) fn end_milestone(
    report: &mut Value,
    pages: Result<&SpecPages, &Refusal>,
    milestone: &str,
    then: &str,
    lang: Locale,
) {
    let (pages, failed) = match pages {
        Ok(pages) => {
            note_checked(report, pages);
            (Some(pages), pages.project_failed.as_ref().map(|_| Page::Project))
        }
        Err(refusal) => {
            push_warning(report, refusal.reason(), &not_rebuilt(Page::Spec, refusal, lang));
            (None, Some(Page::Spec))
        }
    };
    // A falta de qualquer uma das duas páginas é página não refeita: a do
    // disco não passou pela conferência deste marco.
    if let Some(page) = failed {
        let said = translate("page.not_rebuilt", lang).replace("{page}", page.name(lang));
        let rebuild = translate("page.after_rebuild", lang).replace("{milestone}", milestone);
        report["next"] = json!(format!("{said} {rebuild} {then}"));
        return;
    }
    let Some(pages) = pages else {
        return;
    };
    report["publish"] = json!(["spec", "project"]);
    let mut next = translate("page.publish", lang).replace("{milestone}", milestone);
    if !pages.withheld.is_empty() {
        next.push(' ');
        next.push_str(&translate("page.purge_pending", lang).replace("{codes}", &pages.withheld.join(", ")));
    }
    for sentence in since_published(&pages.published, lang) {
        next.push(' ');
        next.push_str(&sentence);
    }
    report["next"] = json!(format!("{next} {then}"));
}

/// O que a ordem de publicar diz de cada página já publicada: o que entrou
/// na spec depois da última publicação dela, e que é preciso ler antes o
/// endereço gravado quando a conversa ainda não a publicou; no fim, que a
/// conversa confere só essa lista. A página do projeto com a mesma lista da
/// página da spec não a repete. Sem página publicada, não diz nada.
fn since_published(published: &[Published], lang: Locale) -> Vec<String> {
    let mut out = Vec::new();
    for (n, page) in published.iter().enumerate() {
        let name = page.page.name(lang);
        let at = page.at.get(..16).unwrap_or(&page.at).replace('T', " ");
        let repeated = published[..n].iter().any(|before| before.changed == page.changed);
        let list = if repeated {
            translate("page.publish.same_list", lang).replace("{page}", name)
        } else if page.changed.is_empty() {
            translate("page.publish.nothing_since", lang).replace("{page}", name).replace("{at}", &at)
        } else {
            let items: Vec<String> = page.changed.iter().map(|item| format!("{} — {}", item.code, item.text)).collect();
            translate("page.publish.since", lang)
                .replace("{page}", name)
                .replace("{at}", &at)
                .replace("{items}", &items.join("; "))
        };
        out.push(list);
        out.push(translate("page.publish.read_first", lang).replace("{page}", name).replace("{url}", &page.url));
    }
    if !out.is_empty() {
        out.push(translate("page.publish.check_only", lang).to_string());
    }
    out
}

/// As duas páginas que um marco publica.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Page {
    Spec,
    Project,
}

impl Page {
    /// O nome da página, como o aviso a diz.
    fn name(self, lang: Locale) -> &'static str {
        match self {
            Self::Spec => translate("page.name.spec", lang),
            Self::Project => translate("page.name.project", lang),
        }
    }

    /// O valor do campo `page` da publicação desta página.
    fn key(self) -> &'static str {
        match self {
            Self::Spec => SPEC_KEY,
            Self::Project => PROJECT_KEY,
        }
    }
}

/// O aviso da página `page` que não pôde ser refeita: a falha de gravação diz
/// qual página falhou; as outras recusas já dizem o motivo por inteiro.
fn not_rebuilt(page: Page, refusal: &Refusal, lang: Locale) -> String {
    match refusal {
        Refusal::Io { detail } => translate("page.rebuild_failed", lang)
            .replace("{page}", page.name(lang))
            .replace("{detail}", detail),
        other => other.message(lang),
    }
}


/// O caminho relativo ao projeto, com barras normais: a saída não traz o
/// caminho da máquina.
pub(crate) fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::spec_state::DiskSpecState;
    use mustard_core::domain::spec_state::SpecState;
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
    /// página do projeto lista as três com o estado e o link da página de
    /// cada uma, e mostra no rodapé o caminho do índice de onde saiu. Ela
    /// continua listando a descartada quando outra spec a refaz e quando o
    /// índice é refeito do zero.
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
        let discarded = read(root, ".claude/spec/project.html");

        let pages = refresh(root, "trava", Locale::PtBr).unwrap();
        assert_eq!(pages.project.as_deref(), Some(".claude/spec/project.html"));
        let html = read(root, ".claude/spec/project.html");
        assert_eq!(html, discarded, "the discard left the project page as the next step makes it");
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
        crate::commands::spec_events::index::index_at(&crate::commands::spec_events::index::IndexOpts {
            root: root.to_path_buf(),
        });
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

    /// Grava pelo gravador de verdade, com a data e a hora `hms` de 18/09, e
    /// devolve o número e o código do item.
    fn put_at(root: &Path, spec: &str, hms: &str, event_type: &str, draft: serde_json::Value) -> (u64, String) {
        put_when(root, spec, &format!("2026-09-18T{hms}-03:00"), event_type, draft)
    }

    /// Grava pelo gravador de verdade, com a data, a hora e o fuso `at`, e
    /// devolve o número e o código do item.
    fn put_when(root: &Path, spec: &str, at: &str, event_type: &str, draft: serde_json::Value) -> (u64, String) {
        let path = root.join(".claude").join("spec").join(spec).join("spec.ndjson");
        let written = store::write_at(&path, event_type, draft.as_object().cloned().unwrap(), &[], at)
            .unwrap_or_else(|r| panic!("{event_type} was refused: {r:?}"));
        (written.id, written.code.unwrap_or_default())
    }

    /// O `next` de um marco, pela mesma porta que o plano, a rodada e o
    /// fechamento usam: a página refeita e o fim do marco.
    fn milestone_next(root: &Path, spec: &str, milestone: &str) -> String {
        let pages = refresh(root, spec, Locale::PtBr);
        let mut report = json!({ "ok": true });
        end_milestone(&mut report, pages.as_ref(), milestone, "Depois, siga.", Locale::PtBr);
        assert_eq!(report["publish"], json!(["spec", "project"]), "{report}");
        report["next"].as_str().unwrap_or_default().to_string()
    }

    /// Uma spec já publicada chega a um marco: a ordem de publicar lista, pela
    /// data e hora, o que entrou na spec depois da última publicação que deu
    /// certo de cada página, com o código e o texto curto de cada item, diz
    /// para conferir só isso e manda ler antes o endereço gravado quando a
    /// conversa ainda não publicou a página. Na divisa: o item de um segundo
    /// antes e o do mesmo segundo gravado antes da publicação não aparecem; o
    /// do mesmo segundo gravado depois e o de um segundo depois aparecem. A
    /// publicação que falhou não muda o começo da lista, e as publicações não
    /// entram nela.
    #[test]
    fn the_publish_order_lists_what_entered_the_spec_since_the_last_publication() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        let spec_url = "https://claude.ai/code/artifact/busca";
        let project_url = "https://claude.ai/code/artifact/projeto";
        put_at(root, "busca", "09:00:00", "state", json!({"phase": "survey"}));
        let (origin, said) =
            put_at(root, "busca", "09:00:10", "message", json!({"author": "user", "text": "Publique sempre."}));
        let (_, old) = put_at(root, "busca", "09:00:59", "decision",
            json!({"text": "Uma decisão antiga, já na página.", "why": "w", "keys": ["antiga"], "origin": origin}));
        let (_, same_before) = put_at(root, "busca", "09:01:00", "message", json!({"author": "user", "text": "Antes de publicar."}));
        put_at(root, "busca", "09:01:00", "publish",
            json!({"page": "spec", "milestone": "approval", "ok": true, "url": spec_url}));
        put_at(root, "busca", "09:01:00", "publish",
            json!({"page": "project", "milestone": "approval", "ok": true, "url": project_url}));
        let (_, same_after) = put_at(root, "busca", "09:01:00", "message", json!({"author": "user", "text": "Mais uma coisa."}));
        let (_, decision) = put_at(root, "busca", "09:01:01", "decision",
            json!({"text": "**A ordem diz o que mudou.** O resto já foi publicado.", "why": "w", "keys": ["ordem"],
                "origin": origin}));
        let (_, criterion) = put_at(root, "busca", "09:05:00", "criterion",
            json!({"when": "a spec chega a um marco", "then": "a ordem diz o que mudou", "proof": "cargo test",
                "label": "a ordem de publicar diz o que mudou", "origin": origin}));
        put_at(root, "busca", "09:06:00", "publish",
            json!({"page": "spec", "milestone": "round", "ok": false, "reason": "a ferramenta caiu"}));

        let next = milestone_next(root, "busca", "round");
        let spec_name = translate("page.name.spec", Locale::PtBr);
        let project_name = translate("page.name.project", Locale::PtBr);
        let items = format!(
            "{same_after} — Mais uma coisa; {decision} — A ordem diz o que mudou; \
             {criterion} — a ordem de publicar diz o que mudou"
        );
        let since = translate("page.publish.since", Locale::PtBr)
            .replace("{page}", spec_name)
            .replace("{at}", "2026-09-18 09:01")
            .replace("{items}", &items);
        assert!(next.contains(&since), "the spec page list is missing:\n{since}\n{next}");
        let read_first = |name: &str, url: &str| {
            translate("page.publish.read_first", Locale::PtBr).replace("{page}", name).replace("{url}", url)
        };
        assert!(next.contains(&read_first(spec_name, spec_url)), "{next}");
        assert!(next.contains(&read_first(project_name, project_url)), "{next}");
        assert!(
            next.contains(&translate("page.publish.same_list", Locale::PtBr).replace("{page}", project_name)),
            "the project page holds the same list, said once: {next}"
        );
        assert!(next.contains(translate("page.publish.check_only", Locale::PtBr)), "{next}");
        assert!(next.contains("write publish") && next.ends_with("Depois, siga."), "{next}");
        for before in [&said, &old, &same_before] {
            assert!(!next.contains(before.as_str()), "{before} came before the publication: {next}");
        }
        assert!(!next.contains("MSTD-PUB-"), "the publications are not in the list: {next}");
        assert_eq!(next.matches(&items).count(), 1, "the list is said once: {next}");

        // Publicada de novo, a lista recomeça dali: o que já foi publicado
        // sai dela.
        put_at(root, "busca", "09:10:00", "publish",
            json!({"page": "spec", "milestone": "round", "ok": true, "url": spec_url}));
        let (_, later) = put_at(root, "busca", "09:10:01", "message", json!({"author": "user", "text": "Depois da rodada."}));
        let next = milestone_next(root, "busca", "round");
        let since = translate("page.publish.since", Locale::PtBr)
            .replace("{page}", spec_name)
            .replace("{at}", "2026-09-18 09:10")
            .replace("{items}", &format!("{later} — Depois da rodada"));
        // A frase termina logo depois da lista: o que já foi publicado na
        // página da spec saiu dela.
        assert!(next.contains(&since), "{since}\n{next}");
        // A página do projeto, publicada às 09:01, segue com a lista dela.
        let project_since = translate("page.publish.since", Locale::PtBr)
            .replace("{page}", project_name)
            .replace("{at}", "2026-09-18 09:01")
            .replace("{items}", &format!("{items}; {later} — Depois da rodada"));
        assert!(next.contains(&project_since), "{project_since}\n{next}");
    }

    /// A página que nunca foi publicada não tem lista nem endereço: a ordem de
    /// publicar é só a de publicar, e nenhum endereço entra nela.
    #[test]
    fn a_page_never_published_gets_no_list_and_no_address() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        put_at(root, "nova", "09:00:00", "state", json!({"phase": "survey"}));
        put_at(root, "nova", "09:00:10", "message", json!({"author": "user", "text": "Comece."}));
        put_at(root, "nova", "09:00:20", "publish",
            json!({"page": "spec", "milestone": "approval", "ok": false, "reason": "a ferramenta caiu"}));

        let next = milestone_next(root, "nova", "approval");
        let publish = translate("page.publish", Locale::PtBr).replace("{milestone}", "approval");
        assert_eq!(next, format!("{publish} Depois, siga."));
        assert!(!next.contains("http"), "{next}");
    }

    /// Um projeto com a spec `spec` aberta, o checkout na branch dela e as
    /// duas páginas já publicadas pelo `run write`, como a conversa faz.
    fn published_project(spec: &str) -> tempfile::TempDir {
        use crate::commands::spec_events::write::record_open;
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        crate::shared::spec_state::stand_on_spec_branch(root, spec);
        record_open(root, spec, &format!("feature/{spec}"), "dev").unwrap();
        for page in ["spec", "project"] {
            run_write(root, spec, "publish", json!({"page": page, "milestone": "approval", "ok": true,
                "url": format!("https://claude.ai/code/artifact/{page}")}));
        }
        dir
    }

    /// Grava pelo `run write`, o comando que a conversa usa, e devolve o
    /// número do item.
    fn run_write(root: &Path, spec: &str, event_type: &str, draft: Value) -> u64 {
        use crate::commands::spec_events::write::{write_at, WriteOpts};
        let out = write_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: Some(spec.into()),
            event_type: event_type.into(),
            json: draft.to_string(),
        });
        assert_eq!(out["ok"], json!(true), "{event_type}: {out}");
        out["id"].as_u64().unwrap_or_default()
    }

    /// Um evento do Claude Code, pelo mesmo despachante que o `mustard-rt on`
    /// usa, na sessão `s1` do projeto em `root`.
    fn hook_event(root: &Path, event: &str, raw: Value) -> mustard_core::domain::model::contract::Outcome {
        use mustard_core::domain::model::contract::{HookInput, Trigger};
        let input = HookInput {
            hook_event_name: Some(event.to_string()),
            session_id: Some("s1".to_string()),
            cwd: Some(root.to_string_lossy().into_owned()),
            raw,
            ..HookInput::default()
        };
        crate::dispatch::run_event(Trigger::from_event_name(event), &input)
    }

    /// Um passo do fluxo, rodado pelo mesmo despacho que o `mustard-rt run`
    /// usa: grava a chamada na spec.
    fn flow_step(root: &Path, spec: &str) {
        crate::commands::flow::cli::dispatch(crate::commands::flow::cli::FlowCmd::Resume {
            spec: Some(spec.to_string()),
            root: root.to_path_buf(),
        });
    }

    /// Os tipos gravados depois das duas publicações, na ordem do arquivo, e
    /// a hora da publicação da página da spec como a ordem de publicar a diz.
    fn after_publication(root: &Path, spec: &str) -> (Vec<String>, String) {
        let log = DiskSpecState::new(root).log(spec).unwrap();
        let visible = log.visible();
        let spec_page = visible.iter().rposition(|e| published_to(e, SPEC_KEY).is_some()).unwrap();
        let at = visible[spec_page].at().get(..16).unwrap_or_default().replace('T', " ");
        let last = visible.iter().rposition(|e| e.event_type == "publish").unwrap();
        (visible[last + 1..].iter().map(|e| e.event_type.clone()).collect(), at)
    }

    /// Entre a última publicação e o marco, a conversa segue pelos ganchos e
    /// pelos comandos de verdade: a mensagem do usuário, o texto que os
    /// ganchos colocam na conversa, a escrita que o portão recusa antes da
    /// aprovação, a resposta, a volta que um bloqueio do fim da resposta pede,
    /// a decisão gravada pelo `run write` e um passo do fluxo rodado pelo
    /// despacho do `mustard-rt run`. A lista da ordem de publicar traz só a
    /// mensagem, as duas respostas e a decisão, na ordem do arquivo; a marca
    /// do gancho que recusou, o texto colocado e a chamada do passo ficam
    /// fora, embora gravados depois da publicação.
    #[test]
    fn the_publish_list_carries_only_the_items_with_text() {
        use mustard_core::domain::model::contract::{HookInput, Trigger};
        let dir = published_project("lista");
        let root = dir.path();
        assert!(!hook_event(root, "UserPromptSubmit", json!({ "prompt": "Grave a decisão da lista." })).is_blocking());
        let log = DiskSpecState::new(root).log("lista").unwrap();
        let said = log.visible().into_iter().filter(|e| e.event_type == "message").map(|e| e.id).next_back().unwrap();
        let write = HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            tool_name: Some("Write".to_string()),
            tool_input: json!({ "file_path": root.join("src/a.rs"), "content": "x" }),
            session_id: Some("s1".to_string()),
            cwd: Some(root.to_string_lossy().into_owned()),
            ..HookInput::default()
        };
        assert!(crate::dispatch::run_event(Some(Trigger::PreToolUse), &write).is_blocking(), "not approved yet");
        let reply = "Gravei a decisão. Depois de ler todos os arquivos do projeto e conferir cada teste que \
                     ainda falhava na máquina do usuário, eu ajustei a leitura do idioma e a contagem das \
                     linhas para que a resposta final saia bem curta e clara.";
        assert!(!hook_event(root, "Stop", json!({ "last_assistant_message": reply })).is_blocking());
        let retry = json!({ "last_assistant_message": "Resumo: gravei a decisão.", "stop_hook_active": true });
        assert!(!hook_event(root, "Stop", retry).is_blocking());
        run_write(root, "lista", "decision", json!({"text": "**A lista leva só o que tem texto.** O resto fica fora.",
            "why": "w", "keys": ["lista"], "origin": said}));
        flow_step(root, "lista");

        let (types, at) = after_publication(root, "lista");
        assert_eq!(
            types,
            ["message", "injection", "hook", "response", "response", "decision", "call"],
            "the hooks and the step recorded their marks after the publication"
        );
        let next = milestone_next(root, "lista", "round");
        let items = "MSTD-MSG-0001 — Grave a decisão da lista; MSTD-RESP-0001 — Gravei a decisão; \
                     MSTD-RESP-0002 — Resumo: gravei a decisão; MSTD-DEC-0001 — A lista leva só o que tem texto";
        let since = translate("page.publish.since", Locale::PtBr)
            .replace("{page}", translate("page.name.spec", Locale::PtBr))
            .replace("{at}", &at)
            .replace("{items}", items);
        assert!(next.contains(&since), "only the items with text, in file order:\n{since}\n{next}");
        for internal in ["MSTD-INJ-", "MSTD-HOOK-", "MSTD-CALL-", "MSTD-STATE-", "MSTD-PUB-"] {
            assert!(!next.contains(internal), "{internal} is an internal record: {next}");
        }
    }

    /// Depois da última publicação só entraram registros internos: o texto
    /// que os ganchos do início da sessão colocam e a chamada de um passo do
    /// fluxo. Para a ordem de publicar, nada entrou na spec, e ela diz isso
    /// com a hora da publicação.
    #[test]
    fn only_internal_records_since_the_publication_say_nothing_entered() {
        let dir = published_project("nada");
        let root = dir.path();
        hook_event(root, "SessionStart", json!({ "source": "startup" }));
        flow_step(root, "nada");

        let (types, at) = after_publication(root, "nada");
        assert!(types.iter().any(|t| t == "call"), "the step recorded its call: {types:?}");
        assert!(types.iter().all(|t| ["injection", "hook", "call"].contains(&t.as_str())), "{types:?}");
        let next = milestone_next(root, "nada", "round");
        let nothing = translate("page.publish.nothing_since", Locale::PtBr)
            .replace("{page}", translate("page.name.spec", Locale::PtBr))
            .replace("{at}", &at);
        assert!(next.contains(&nothing), "{nothing}\n{next}");
        assert!(!next.contains("MSTD-"), "no item is listed: {next}");
    }

    /// Só os tipos que têm texto para ler entram na lista; os outros são os
    /// registros internos, e um tipo novo não entra na lista sem alguém
    /// decidir de que lado ele fica.
    #[test]
    fn every_type_is_either_readable_or_an_internal_record() {
        use mustard_core::domain::spec_events::TYPES;
        for name in READABLE_TYPES {
            assert!(type_spec(name).is_some(), "{name} is not a type");
        }
        let internal: Vec<&str> = TYPES.iter().map(|t| t.name).filter(|n| !READABLE_TYPES.contains(n)).collect();
        assert_eq!(internal, ["injection", "hook", "call", "state", "publish", "criterion_run", "send", "commit"]);
    }

    /// A data e a hora são lidas com o fuso, como o índice das specs as
    /// compara, e não como texto: a publicação às 12:00 de Brasília é 15:00
    /// em UTC. Na divisa, o item de 14:59:59 UTC não aparece, embora o texto
    /// "14:59" passe de "12:00"; o do mesmo instante, 15:00:00 UTC, aparece só
    /// quando foi gravado depois da publicação no arquivo; o de 15:00:01 UTC
    /// aparece. E o de 11:30 a cinco horas de UTC, que é 16:30 UTC, aparece,
    /// embora o texto "11:30" fique antes de "12:00".
    #[test]
    fn the_publish_list_reads_the_time_with_its_offset() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        let put = |at: &str, event_type: &str, draft: Value| put_when(root, "fuso", at, event_type, draft).1;
        put("2026-09-18T11:00:00-03:00", "state", json!({"phase": "survey"}));
        let same_before = put("2026-09-18T15:00:00Z", "message", json!({"author": "user", "text": "Antes, no mesmo instante."}));
        for page in ["spec", "project"] {
            put("2026-09-18T12:00:00-03:00", "publish",
                json!({"page": page, "milestone": "approval", "ok": true, "url": format!("https://claude.ai/code/artifact/{page}")}));
        }
        let second_before = put("2026-09-18T14:59:59Z", "message", json!({"author": "user", "text": "Um segundo antes."}));
        let same_after = put("2026-09-18T15:00:00+00:00", "message", json!({"author": "user", "text": "No mesmo instante."}));
        let second_after = put("2026-09-18T15:00:01Z", "message", json!({"author": "user", "text": "Um segundo depois."}));
        let west = put("2026-09-18T11:30:00-05:00", "message", json!({"author": "user", "text": "Em outro fuso, depois."}));

        let next = milestone_next(root, "fuso", "round");
        let items = format!(
            "{same_after} — No mesmo instante; {second_after} — Um segundo depois; {west} — Em outro fuso, depois"
        );
        let since = translate("page.publish.since", Locale::PtBr)
            .replace("{page}", translate("page.name.spec", Locale::PtBr))
            .replace("{at}", "2026-09-18 12:00")
            .replace("{items}", &items);
        assert!(next.contains(&since), "{since}\n{next}");
        for before in [&same_before, &second_before] {
            assert!(!next.contains(before.as_str()), "{before} came before the publication: {next}");
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

