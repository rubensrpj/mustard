//! A cópia da spec para o banco de dados da página publicada dela.
//!
//! O claude.ai não enxerga o disco da máquina: a página publicada de uma spec
//! é um template do Mustard que lê um banco de dados guardado junto dela
//! (`mustard_core::platform::page_templates`). O binário não monta página
//! nenhuma; nos marcos (a aprovação, o fim de cada rodada e o fechamento) e
//! logo depois de um pedido que muda o plano, ele prepara em arquivos o que vai
//! para o banco, e a conversa copia esses arquivos com a ferramenta do banco
//! (`ArtifactData`), sem ler os itens.
//!
//! ## O que a cópia leva
//!
//! - cada item com número maior que o do último copiado. O último copiado é o
//!   maior `last` das cópias da página da spec gravadas depois da última
//!   publicação dela que deu certo; sem cópia depois dela, a página acabou de
//!   nascer, e a cópia leva a spec inteira. O número decide, não o horário:
//!   vários itens caem no mesmo segundo;
//! - sem os registros internos ([`LEFT_OUT`]): o texto que um gancho colocou
//!   na conversa, a chamada de um comando e o aviso de um gancho;
//! - sem o item que guarda um trecho com cara de segredo: ele fica fora até
//!   ser expurgado, e o marco diz o código dele;
//! - o item anterior que um expurgo gravado depois da última cópia tocou: a
//!   versão limpa substitui a do banco, e a que ainda guarda um trecho com
//!   cara de segredo sai do banco;
//! - o documento das coisas calculadas, trocado a cada cópia: o estado de cada
//!   onda, o pedido de cada onda que ainda não saiu e a economia do rtk.
//!
//! Cada documento vai num arquivo JSON próprio, dentro de `copy/` na pasta da
//! spec, e cada lote (`spec-<n>.json`) é a lista `writes` de uma chamada da
//! ferramenta do banco, com até [`BATCH_MAX`] escritas. A cópia feita vira um
//! registro `copy` na spec, gravado pela conversa, com o número até onde a
//! cópia foi: é por ele que a cópia seguinte começa.
//!
//! ## A linha da spec na página do projeto
//!
//! Nos marcos, a linha da spec vai para o banco da página do projeto quando a
//! fase dela mudou desde a última cópia dela (o `phase` do último `copy` da
//! página do projeto). Com a página do projeto ainda sem endereço, ou
//! publicada de novo nesta spec depois da última cópia, vão todas as linhas do
//! índice. Os lotes dela são `project-<n>.json`.
//!
//! A preparação inteira — ler o arquivo de eventos, apagar a cópia anterior e
//! gravar os arquivos novos — acontece com a trava do arquivo de eventos
//! presa: duas rodadas ao mesmo tempo nunca misturam os arquivos de uma com os
//! da outra, e nenhuma gravação entra no meio.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::{Hidden, Refusal, SpecEvent, SpecLog, PURGED_MARK};
use mustard_core::domain::spec_index::{project_url, published_to, ProjectRow, PROJECT_PAGE, SPEC_PAGE};
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{translate, Locale};
use mustard_core::platform::page_templates::{
    project_page_template, spec_page_template, COMPUTED, ITEMS, PROJECT_CAPABILITIES, SPECS, SPEC_CAPABILITIES,
};
use mustard_core::view::document::{RtkDay, WaveState};
use mustard_core::ClaudePaths;
use serde_json::{json, Map, Value};

use super::relative;

/// A pasta da cópia, dentro da pasta da spec.
pub(crate) const FOLDER: &str = "copy";

/// Quantas escritas cabem numa chamada da ferramenta do banco.
pub(crate) const BATCH_MAX: usize = 50;

/// Os registros internos, que a cópia não leva: o texto que um gancho colocou
/// na conversa, a chamada de um comando e o aviso de um gancho.
pub(crate) const LEFT_OUT: &[&str] = &["injection", "hook", "call"];

/// O template da página da spec, como a instalação o deixa no projeto.
pub(crate) const SPEC_TEMPLATE: &str = ".claude/mustard/pages/spec.html";

/// O template da página do projeto, como a instalação o deixa no projeto.
pub(crate) const PROJECT_TEMPLATE: &str = ".claude/mustard/pages/project.html";

/// Quando a cópia é preparada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Moment {
    /// Um marco: a aprovação, o fim de uma rodada ou o fechamento. A página
    /// ainda não publicada é publicada antes, e a linha da spec na página do
    /// projeto vai junto quando a fase mudou.
    Milestone,
    /// Logo depois de um pedido que muda o plano: só a página da spec, e só
    /// quando ela já foi publicada.
    Request,
}

/// A cópia preparada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Prepared {
    /// A pasta da cópia, relativa ao projeto.
    pub folder: String,
    pub spec: Target,
    /// A página do projeto, quando alguma linha vai para o banco dela.
    pub project: Option<Target>,
    /// O código de cada item que guarda um trecho com cara de segredo e por
    /// isso fica fora da cópia, na ordem do arquivo.
    pub withheld: Vec<String>,
}

/// O que vai para o banco de uma página.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Target {
    /// O endereço da página; sem ele, a página ainda não foi publicada.
    pub url: Option<String>,
    /// Os lotes, relativos ao projeto, na ordem em que vão.
    pub batches: Vec<String>,
    /// O `--json` do `run write copy` que grava a cópia feita.
    pub record: Value,
}

/// Prepara a cópia da spec `spec` do projeto `root` para o banco da página
/// dela e, num marco, a da linha dela para o banco da página do projeto.
/// Depois de um pedido, numa spec cuja página ainda não foi publicada, nada é
/// preparado e a resposta é `Ok(None)`.
///
/// # Errors
///
/// A recusa do nome da spec, a da spec sem arquivo de eventos e a falha de
/// gravação dos arquivos.
pub(crate) fn prepare(root: &Path, spec: &str, moment: Moment, lang: Locale) -> Result<Option<Prepared>, Refusal> {
    let paths = ClaudePaths::for_project(root).map_err(|e| Refusal::Io { detail: e.to_string() })?;
    let spec_paths = paths.for_spec(spec.trim()).map_err(|_| Refusal::BadSpecName { spec: spec.to_string() })?;
    // O rtk roda antes da trava: ninguém espera por ele para gravar.
    let rtk = super::rtk_days(root);
    let folder = spec_paths.dir().join(FOLDER);
    let place = Place { root, spec: spec.trim(), folder, index: paths.spec_index_path() };
    store::with_locked_log(&spec_paths.spec_ndjson_path(), |log| build(&place, log, &rtk, moment, lang))?
        .unwrap_or_else(|| Err(Refusal::NoSpecFile { spec: spec.trim().to_string() }))
}

/// A cópia de um marco da spec `spec`, que sempre sai: [`prepare`] no
/// [`Moment::Milestone`].
///
/// # Errors
///
/// As recusas de [`prepare`].
pub(crate) fn prepare_milestone(root: &Path, spec: &str, lang: Locale) -> Result<Prepared, Refusal> {
    prepare(root, spec, Moment::Milestone, lang)?.ok_or_else(|| Refusal::NoSpecFile { spec: spec.trim().to_string() })
}

/// Onde a cópia de uma spec é preparada.
struct Place<'a> {
    root: &'a Path,
    spec: &'a str,
    folder: PathBuf,
    index: PathBuf,
}

/// Monta e grava a cópia a partir de `log`, lido com a trava presa.
fn build(
    place: &Place,
    log: &SpecLog,
    rtk: &[RtkDay],
    moment: Moment,
    lang: Locale,
) -> Result<Option<Prepared>, Refusal> {
    let (url, since) = spec_page(log);
    if moment == Moment::Request && url.is_none() {
        return Ok(None);
    }
    clear(&place.folder)?;
    let mut writes: Vec<Value> = Vec::new();
    for (id, body) in items_after(log, since) {
        writes.push(set(place, ITEMS, &id.to_string(), &body)?);
    }
    for (id, body) in purged_since(log, since) {
        match body {
            Some(body) => writes.push(set(place, ITEMS, &id.to_string(), &body)?),
            None => writes.push(json!({ "op": "delete", "collection": ITEMS, "doc_id": id.to_string() })),
        }
    }
    let (collection, doc) = COMPUTED.split_once('/').unwrap_or((COMPUTED, "current"));
    writes.push(set(place, collection, doc, &computed(place, log, rtk, lang))?);
    if url.is_none() {
        ensure_template(place.root, SPEC_TEMPLATE, || spec_page_template(lang))?;
    }
    let spec = Target {
        url,
        batches: batches(place, "spec", &writes)?,
        record: json!({ "page": SPEC_PAGE, "last": log.max_id() }),
    };
    let project = match moment {
        Moment::Milestone => project_rows(place, log, lang)?,
        Moment::Request => None,
    };
    Ok(Some(Prepared { folder: relative(place.root, &place.folder), spec, project, withheld: withheld(log) }))
}

/// O endereço da página da spec, da última publicação dela que deu certo, e
/// o número do último item copiado para o banco dela: o maior `last` das
/// cópias gravadas depois dessa publicação, ou zero.
fn spec_page(log: &SpecLog) -> (Option<String>, u64) {
    let visible = log.visible();
    let mut publications = visible.iter().filter_map(|e| published_to(e, SPEC_PAGE).map(|url| (e.id, url)));
    let Some((published, url)) = publications.next_back() else {
        return (None, 0);
    };
    let since = visible
        .iter()
        .filter(|e| e.id > published && copy_of(e) == Some(SPEC_PAGE))
        .filter_map(|e| e.int("last"))
        .max()
        .unwrap_or(0);
    (Some(url.to_string()), since)
}

/// A página de um registro de cópia.
fn copy_of(event: &SpecEvent) -> Option<&str> {
    (event.event_type == "copy").then(|| event.str_field("page")).flatten()
}

/// O item como vai para o banco: a linha do arquivo, sem o campo de busca,
/// que a página não usa. `None` quando ele guarda um trecho com cara de
/// segredo.
fn body_of(event: &SpecEvent) -> Option<Value> {
    let mut fields = event.fields.clone();
    fields.remove("search");
    let body = Value::Object(fields);
    (!holds_secret(&body)).then_some(body)
}

/// A linha que a cópia nunca leva: o registro interno e a linha que o
/// formato antigo do expurgo esvaziou, que saiu da leitura.
fn never_copied(event: &SpecEvent, log_hidden: &std::collections::BTreeMap<u64, Hidden>) -> bool {
    LEFT_OUT.contains(&event.event_type.as_str()) || matches!(log_hidden.get(&event.id), Some(Hidden::Purged { .. }))
}

/// Os itens com número maior que `since` que vão para o banco, em ordem de
/// número, cada um com o corpo dele. O item com cara de segredo fica fora.
fn items_after(log: &SpecLog, since: u64) -> Vec<(u64, Value)> {
    let hidden = log.hidden();
    log.events
        .iter()
        .filter(|e| e.id > since && !never_copied(e, &hidden))
        .filter_map(|e| body_of(e).map(|body| (e.id, body)))
        .collect()
}

/// Os itens de número até `since` que um expurgo gravado depois de `since`
/// tocou: todas as versões de cada alvo e, num ponto do levantamento, o outro
/// lado do par. Cada um vai com a versão limpa, que substitui a do banco, ou
/// sem corpo, quando ainda guarda um trecho com cara de segredo e sai do banco.
fn purged_since(log: &SpecLog, since: u64) -> Vec<(u64, Option<Value>)> {
    let codes = log.codes();
    let mut touched: BTreeSet<u64> = BTreeSet::new();
    for purge in log.events.iter().filter(|e| e.id > since && e.event_type == "purge") {
        for target in purge.fields.get("targets").and_then(Value::as_array).into_iter().flatten() {
            let found: Vec<u64> = match (target.as_u64(), target.as_str()) {
                (Some(id), _) => vec![id],
                (None, Some(code)) => {
                    codes.iter().filter(|(_, c)| c.as_str() == code.trim()).map(|(id, _)| *id).collect()
                }
                _ => Vec::new(),
            };
            for id in found {
                // Todas as versões do item têm o mesmo código.
                let code = codes.get(&id);
                touched.extend(codes.iter().filter(|(_, c)| Some(*c) == code).map(|(other, _)| *other));
                touched.insert(id);
            }
        }
    }
    // O fechamento de um ponto copia a lacuna do original: o expurgo toca os
    // dois lados do par.
    let pairs: Vec<u64> = log
        .events
        .iter()
        .filter(|e| e.event_type == "point")
        .filter_map(|e| e.int("closes").map(|closes| (e.id, closes)))
        .filter(|(closing, closes)| touched.contains(closing) || touched.contains(closes))
        .flat_map(|(closing, closes)| [closing, closes])
        .collect();
    touched.extend(pairs);
    let hidden = log.hidden();
    touched
        .into_iter()
        .filter(|id| *id <= since)
        .filter_map(|id| log.get(id))
        .filter(|e| !never_copied(e, &hidden))
        .map(|e| (e.id, body_of(e)))
        .collect()
}

/// O código de cada item que guarda um trecho com cara de segredo e por isso
/// fica fora da cópia, uma vez só, na ordem do arquivo.
pub(crate) fn withheld(log: &SpecLog) -> Vec<String> {
    let codes = log.codes();
    let hidden = log.hidden();
    let mut out: Vec<String> = Vec::new();
    for event in log.events.iter().filter(|e| !never_copied(e, &hidden)) {
        let mut fields = event.fields.clone();
        fields.remove("search");
        if holds_secret(&Value::Object(fields)) {
            let code = codes.get(&event.id).cloned().unwrap_or_else(|| event.id.to_string());
            if !out.contains(&code) {
                out.push(code);
            }
        }
    }
    out
}

/// Algum texto de `value`, em qualquer profundidade, tem um trecho com cara
/// de segredo.
fn holds_secret(value: &Value) -> bool {
    match value {
        Value::String(text) => !super::secret::secret_excerpts(text).is_empty(),
        Value::Array(items) => items.iter().any(holds_secret),
        Value::Object(map) => map.values().any(holds_secret),
        _ => false,
    }
}

/// `value` com cada trecho com cara de segredo trocado por "…", em qualquer
/// profundidade: o que o binário calcula não é item da spec e não tem como
/// ser expurgado, então sai limpo.
fn redacted(value: Value) -> Value {
    match value {
        Value::String(mut text) => {
            for excerpt in super::secret::secret_excerpts(&text) {
                text = text.replace(&excerpt, PURGED_MARK);
            }
            Value::String(text)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(redacted).collect()),
        Value::Object(map) => Value::Object(map.into_iter().map(|(k, v)| (k, redacted(v))).collect()),
        other => other,
    }
}

/// O documento das coisas calculadas: o nome da spec, o estado de cada onda,
/// o pedido de cada onda que ainda não saiu e a economia do rtk.
fn computed(place: &Place, log: &SpecLog, rtk: &[RtkDay], lang: Locale) -> Value {
    let running = crate::commands::flow::round::waves_in_progress(log).into_keys().collect();
    let flight = mustard_core::io::wave_prompt::Flight { running, ..Default::default() };
    let sent: BTreeSet<u64> =
        log.visible().into_iter().filter(|e| e.event_type == "send").filter_map(SpecEvent::wave).collect();
    let prompts: Map<String, Value> = mustard_core::io::wave_prompt::prompts(place.root, place.spec, log, lang, &flight)
        .into_iter()
        .filter(|built| !sent.contains(&built.wave))
        .map(|built| (built.wave.to_string(), Value::String(built.text)))
        .collect();
    let waves: Map<String, Value> = crate::commands::flow::round::wave_states(log)
        .into_iter()
        .map(|(n, state)| (n.to_string(), json!(state_name(state))))
        .collect();
    let rtk: Vec<Value> = rtk
        .iter()
        .map(|day| json!({ "date": day.date, "commands": day.commands, "input": day.input, "saved": day.saved }))
        .collect();
    redacted(json!({ "spec": place.spec, "waves": waves, "prompts": prompts, "rtk": rtk }))
}

/// O nome do estado de uma onda no documento das coisas calculadas.
fn state_name(state: WaveState) -> &'static str {
    match state {
        WaveState::Todo => "todo",
        WaveState::Running => "running",
        WaveState::Delivered => "delivered",
        WaveState::Approved => "approved",
        WaveState::Rejected => "rejected",
    }
}

/// As linhas do índice que vão para o banco da página do projeto: todas,
/// quando ela ainda não tem endereço ou foi publicada de novo nesta spec
/// depois da última cópia; senão, a linha desta spec, quando a fase dela
/// mudou desde a última cópia; senão, nenhuma.
fn project_rows(place: &Place, log: &SpecLog, lang: Locale) -> Result<Option<Target>, Refusal> {
    let url = mustard_core::io::fs::lock::read_shared(&place.index).ok().and_then(|content| project_url(&content));
    let rows = mustard_core::io::spec_index::read_rows(place.root);
    let Some(own) = rows.iter().find(|row| row.name == place.spec) else {
        return Ok(None);
    };
    let visible = log.visible();
    let published = visible.iter().filter(|e| published_to(e, PROJECT_PAGE).is_some()).map(|e| e.id).next_back();
    let copied = visible.iter().rfind(|e| copy_of(e) == Some(PROJECT_PAGE));
    let copied_phase = copied.and_then(|e| e.str_field("phase")).map(str::to_string);
    let fresh = match (published, copied) {
        (Some(published), Some(copied)) => published > copied.id,
        (Some(_), None) => true,
        _ => false,
    };
    let chosen: Vec<&ProjectRow> = if url.is_none() || fresh {
        rows.iter().collect()
    } else if copied.is_none() || own.phase != copied_phase {
        vec![own]
    } else {
        return Ok(None);
    };
    if url.is_none() {
        ensure_template(place.root, PROJECT_TEMPLATE, || project_page_template(lang))?;
    }
    let mut writes = Vec::new();
    for row in chosen {
        writes.push(set(place, SPECS, &row.name, &row_body(row))?);
    }
    let mut record = json!({ "page": PROJECT_PAGE });
    if let Some(phase) = &own.phase {
        record["phase"] = json!(phase);
    }
    Ok(Some(Target { url, batches: batches(place, "project", &writes)?, record }))
}

/// A linha de uma spec como vai para o banco da página do projeto.
fn row_body(row: &ProjectRow) -> Value {
    redacted(json!({
        "name": row.name, "goal": row.goal, "phase": row.phase, "branch": row.branch,
        "created": row.created, "updated": row.updated, "url": row.url,
    }))
}

/// Grava o documento `doc_id` da coleção `collection` num arquivo próprio
/// e devolve a escrita que o manda para o banco pelo arquivo.
fn set(place: &Place, collection: &str, doc_id: &str, body: &Value) -> Result<Value, Refusal> {
    let path = place.folder.join(collection).join(format!("{doc_id}.json"));
    write(&path, &body.to_string())?;
    Ok(json!({ "op": "set", "collection": collection, "doc_id": doc_id, "file_path": relative(place.root, &path) }))
}

/// Grava as escritas em lotes de até [`BATCH_MAX`], `<nome>-1.json`,
/// `<nome>-2.json`…, uma escrita por linha, e devolve os caminhos.
fn batches(place: &Place, name: &str, writes: &[Value]) -> Result<Vec<String>, Refusal> {
    let mut out = Vec::new();
    for (n, chunk) in writes.chunks(BATCH_MAX).enumerate() {
        let path = place.folder.join(format!("{name}-{}.json", n + 1));
        let lines: Vec<String> = chunk.iter().map(Value::to_string).collect();
        write(&path, &format!("[\n{}\n]\n", lines.join(",\n")))?;
        out.push(relative(place.root, &path));
    }
    Ok(out)
}

/// Apaga a cópia anterior: os lotes dela não valem mais.
fn clear(folder: &Path) -> Result<(), Refusal> {
    if !folder.exists() {
        return Ok(());
    }
    mustard_core::io::fs::remove_dir_all(folder).map_err(|e| Refusal::Io { detail: e.to_string() })
}

/// Deixa no projeto o template que a ordem manda publicar, quando ele falta.
fn ensure_template(root: &Path, template: &str, body: impl FnOnce() -> String) -> Result<(), Refusal> {
    let path = root.join(template);
    if path.is_file() {
        return Ok(());
    }
    write(&path, &body())
}

fn write(path: &Path, text: &str) -> Result<(), Refusal> {
    mustard_core::io::fs::write_atomic(path, text.as_bytes()).map_err(|e| Refusal::Io { detail: e.to_string() })
}

impl Prepared {
    /// A cópia como a resposta de um passo a mostra.
    pub(crate) fn to_value(&self) -> Value {
        let target = |t: &Target| json!({ "published": t.url.is_some(), "batches": t.batches, "record": t.record });
        let mut out = json!({ "folder": self.folder, "spec": target(&self.spec) });
        if let Some(project) = &self.project {
            out["project"] = target(project);
        }
        out
    }

    /// As páginas que ainda precisam da primeira publicação, na ordem em que
    /// são publicadas.
    pub(crate) fn to_publish(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.spec.url.is_none() {
            out.push(SPEC_PAGE);
        }
        if self.project.as_ref().is_some_and(|p| p.url.is_none()) {
            out.push(PROJECT_PAGE);
        }
        out
    }

    /// A ordem da cópia, uma frase por passo: publicar a página que ainda não
    /// tem endereço, no marco `milestone`, copiar os lotes de cada página e
    /// gravar cada cópia feita; no fim, não levar os endereços para a
    /// resposta. Sem marco, a página sem endereço fica para o próximo.
    pub(crate) fn order(&self, spec: &str, milestone: Option<&str>, lang: Locale) -> Vec<String> {
        let mut out = Vec::new();
        for (target, key, name, template, capabilities) in self.targets() {
            let page = translate(name, lang);
            if target.url.is_none() {
                let Some(milestone) = milestone else { continue };
                out.push(
                    translate("page.copy.publish", lang)
                        .replace("{page}", page)
                        .replace("{template}", template)
                        .replace("{capabilities}", capabilities)
                        .replace("{spec}", spec)
                        .replace("{key}", key)
                        .replace("{milestone}", milestone),
                );
            }
            let url = target.url.clone().unwrap_or_else(|| translate("page.copy.new_address", lang).to_string());
            let files: Vec<String> = target.batches.iter().map(|b| format!("`{b}`")).collect();
            out.push(
                translate("page.copy.batches", lang)
                    .replace("{page}", page)
                    .replace("{url}", &url)
                    .replace("{files}", &files.join(", "))
                    .replace("{spec}", spec)
                    .replace("{record}", &target.record.to_string()),
            );
        }
        if !out.is_empty() {
            out.push(translate("page.copy.no_links", lang).to_string());
        }
        out
    }

    /// Cada página com o que vai para o banco dela: a chave dela, o nome no
    /// catálogo, o template e o que ela declara ao ser publicada.
    fn targets(&self) -> Vec<(&Target, &'static str, &'static str, &'static str, &'static str)> {
        let mut out = vec![(&self.spec, SPEC_PAGE, "page.name.spec", SPEC_TEMPLATE, SPEC_CAPABILITIES)];
        if let Some(project) = &self.project {
            out.push((project, PROJECT_PAGE, "page.name.project", PROJECT_TEMPLATE, PROJECT_CAPABILITIES));
        }
        out
    }
}

/// Para os testes: as escritas dos lotes da página `page` (`spec` ou
/// `project`) que a resposta `report` de um passo mandou copiar, na ordem,
/// como a ferramenta do banco as recebe, cada uma com o corpo do documento
/// lido do `file_path` dela em `body`.
#[cfg(test)]
pub(crate) fn sent(root: &Path, report: &Value, page: &str) -> Vec<Value> {
    let batches = report["copy"][page]["batches"].as_array().cloned().unwrap_or_default();
    let mut out = Vec::new();
    for batch in batches {
        let path = root.join(batch.as_str().unwrap_or_default());
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let writes: Vec<Value> = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(writes.len() <= BATCH_MAX, "{} has {} writes", path.display(), writes.len());
        for mut write in writes {
            if let Some(file) = write["file_path"].as_str() {
                let body = std::fs::read_to_string(root.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
                write["body"] = serde_json::from_str(&body).unwrap_or_else(|e| panic!("{file}: {e}"));
            }
            out.push(write);
        }
    }
    out
}

/// Para os testes: os números dos itens que os lotes da página da spec, na
/// resposta `report`, mandam gravar (`set`) no banco, na ordem.
#[cfg(test)]
pub(crate) fn sent_items(root: &Path, report: &Value) -> Vec<u64> {
    sent(root, report, SPEC_PAGE)
        .iter()
        .filter(|w| w["collection"] == json!(ITEMS) && w["op"] == json!("set"))
        .filter_map(|w| w["doc_id"].as_str().and_then(|id| id.parse().ok()))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use mustard_core::domain::model::contract::{HookInput, Outcome, Trigger};
    use mustard_core::domain::spec_state::SpecState as _;
    use serde_json::{json, Value};
    use tempfile::tempdir;

    use super::*;
    use crate::commands::flow::round::{round_for, RoundOpts};
    use crate::commands::spec_events::write::{record_open, seed_at, WriteOpts};
    use crate::shared::spec_state::DiskSpecState;

    const SPEC_URL: &str = "https://claude.ai/code/artifact/spec-x";
    const PROJECT_URL: &str = "https://claude.ai/code/artifact/projeto";

    fn git(root: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(root)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// Grava pelo `run write`, o comando que a conversa usa; a fala do
    /// usuário, que só um gancho grava, vai pela mesma gravação dos ganchos.
    fn write(root: &Path, event_type: &str, draft: Value) -> Value {
        let out = seed_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: Some("x".into()),
            event_type: event_type.into(),
            json: draft.to_string(),
        });
        assert_eq!(out["ok"], json!(true), "{event_type}: {out}");
        out
    }

    fn id_of(report: &Value) -> u64 {
        report["id"].as_u64().unwrap_or_else(|| panic!("não gravou: {report}"))
    }

    /// Um evento do Claude Code, pelo mesmo despachante que o `mustard-rt on`
    /// usa, na sessão `s1` do projeto em `root`.
    fn hook_event(root: &Path, event: &str, tool: Option<&str>, tool_input: Value, raw: Value) -> Outcome {
        let input = HookInput {
            hook_event_name: Some(event.to_string()),
            tool_name: tool.map(str::to_string),
            tool_input,
            session_id: Some("s1".to_string()),
            cwd: Some(root.to_string_lossy().into_owned()),
            raw,
            ..HookInput::default()
        };
        crate::dispatch::run_event(Trigger::from_event_name(event), &input)
    }

    /// Um passo do fluxo pelo despacho do `mustard-rt run`: grava a chamada.
    fn flow_step(root: &Path) {
        crate::commands::flow::cli::dispatch(crate::commands::flow::cli::FlowCmd::Resume {
            spec: Some("x".to_string()),
            root: root.to_path_buf(),
        });
    }

    /// A rodada, o marco que mais se repete, pela porta do comando.
    fn round(root: &Path) -> Value {
        let out = round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None }, None);
        assert_eq!(out["ok"], json!(true), "{out}");
        out
    }

    fn log(root: &Path) -> SpecLog {
        DiskSpecState::new(root).log("x").expect("the spec")
    }

    /// Um projeto no git, na branch da spec `x`, com a spec aprovada: uma
    /// onda com uma tarefa num arquivo que o git conhece.
    fn approved_project() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        std::fs::write(root.join("src/a.rs"), "fn um() {}\n").unwrap();
        git(root, &["init", "-q"]);
        git(root, &["config", "core.autocrlf", "false"]);
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "semente"]);
        for (key, value) in [("user.email", "t@t"), ("user.name", "t"), ("commit.gpgsign", "false")] {
            git(root, &["config", key, value]);
        }
        git(root, &["checkout", "-q", "-b", "feature/x"]);
        assert_eq!(record_open(root, "x", "feature/x", "dev"), Ok(true));
        let said = id_of(&write(root, "message", json!({"author": "user", "text": "o objetivo"})));
        let crit = id_of(&write(root, "criterion",
            json!({"when": "a onda roda", "then": "a suíte passa", "proof": "cargo test", "origin": said})));
        write(root, "wave", json!({"n": 1, "text": "Onda 1.", "criteria": [crit], "done_when": "A suíte passa.",
            "origin": said}));
        write(root, "task", json!({"wave": 1, "text": "Tarefa da onda 1.", "files": [{"path": "src/a.rs"}],
            "origin": said}));
        crate::shared::spec_state::approve_in(&root.join(".claude/spec/x"));
        dir
    }

    /// O que a conversa faz com a ordem de um marco: publica cada página que
    /// ainda não tem endereço e grava o endereço, copia os lotes e grava cada
    /// cópia feita com o `--json` que a ordem traz.
    fn follow(root: &Path, report: &Value) {
        let milestone = "round";
        for page in report["publish"].as_array().cloned().unwrap_or_default() {
            let url = if page == json!("spec") { SPEC_URL } else { PROJECT_URL };
            write(root, "publish", json!({"page": page, "milestone": milestone, "ok": true, "url": url}));
        }
        for page in ["spec", "project"] {
            if !report["copy"][page].is_null() {
                write(root, "copy", report["copy"][page]["record"].clone());
            }
        }
    }

    /// Um marco chega depois de itens novos no `spec.ndjson`, gravados pelos
    /// ganchos e pelos comandos de verdade: a fala do usuário e o texto que
    /// os ganchos colocam, um comando que a trava barra, a resposta, uma
    /// decisão, uma anotação com uma senha e a chamada de um passo do fluxo. A
    /// cópia leva só os itens com número maior que o do último copiado: na
    /// divisa, o último copiado fica fora e o seguinte entra. Ficam fora os
    /// registros internos e a anotação com a senha, que o marco manda
    /// expurgar. A cópia grava o número até onde foi, e a cópia seguinte
    /// começa dele. Nenhum marco escreve `spec.md`, `spec.html` nem
    /// `project.html`.
    #[test]
    fn the_copy_carries_only_the_items_after_the_last_copied_number() {
        let dir = approved_project();
        let root = dir.path();

        // O primeiro marco: as páginas ainda não têm endereço, e a cópia leva
        // a spec inteira.
        let first = round(root);
        assert_eq!(first["publish"], json!(["spec", "project"]), "{first}");
        let last = first["copy"]["spec"]["record"]["last"].as_u64().expect("the number copied");
        assert_eq!(last, log(root).max_id(), "the copy goes up to the last item");
        assert_eq!(sent_items(root, &first).first(), Some(&1), "the whole spec: {first}");
        follow(root, &first);

        // Itens novos, pelos ganchos e pelos comandos.
        let prompt = json!({"prompt": "Anote a senha do banco e siga."});
        assert!(!hook_event(root, "UserPromptSubmit", None, Value::Null, prompt).is_blocking());
        let barred = json!({"command": "rm -rf /"});
        assert!(hook_event(root, "PreToolUse", Some("Bash"), barred, Value::Null).is_blocking());
        let reply = json!({"last_assistant_message": "Anotei a decisão e deixei a senha fora."});
        assert!(!hook_event(root, "Stop", None, Value::Null, reply).is_blocking());
        let said = log(root).visible().into_iter().filter(|e| e.event_type == "message").map(|e| e.id).next_back();
        let decision = id_of(&write(root, "decision", json!({"text": "A cópia leva só o que é novo.", "why": "w",
            "keys": ["cópia"], "applies_to": {"files": ["**"]}, "origin": said})));
        let secret = write(root, "note", json!({"text": "A senha do banco: S3nh4F0rte2024", "keys": ["banco"],
            "origin": said}));
        flow_step(root);

        let second = round(root);
        let after = log(root);
        let new: Vec<&SpecEvent> = after.events.iter().filter(|e| e.id > last).collect();
        for internal in LEFT_OUT {
            assert!(new.iter().any(|e| e.event_type == *internal), "{internal} was recorded after the copy");
        }
        let expected: Vec<u64> = new
            .iter()
            .filter(|e| !LEFT_OUT.contains(&e.event_type.as_str()) && e.id != id_of(&secret))
            .map(|e| e.id)
            .collect();
        let items = sent_items(root, &second);
        assert_eq!(items, expected, "only the items after {last}, without the internal records and the secret");
        assert!(!items.contains(&last) && items.first() == Some(&(last + 1)), "the boundary: {items:?}");
        assert!(items.contains(&decision));
        let bodies = sent(root, &second, "spec");
        assert!(bodies.iter().all(|w| !w.to_string().contains("S3nh4F0rte2024")), "the secret never goes");
        let computed = bodies.iter().find(|w| w["collection"] == json!("computed")).expect("the computed item");
        assert_eq!(computed["body"]["waves"], json!({"1": "running"}), "{computed}");
        assert_eq!(second["withheld"], json!([secret["code"]]), "{second}");
        let next = second["next"].as_str().unwrap_or_default();
        assert!(next.contains("write purge") && next.contains(SPEC_URL), "{next}");
        assert!(second.get("publish").is_none(), "both pages have their address: {second}");
        assert!(second["copy"].get("project").is_none(), "the phase did not change: {second}");

        // A cópia feita grava o número, e a seguinte começa dele.
        let recorded = id_of(&write(root, "copy", second["copy"]["spec"]["record"].clone()));
        flow_step(root);
        let third = round(root);
        assert_eq!(sent_items(root, &third), [recorded], "only what came after the recorded number");

        for page in [".claude/spec/x/spec.md", ".claude/spec/x/spec.html", ".claude/spec/project.html"] {
            assert!(!root.join(page).exists(), "{page} is no longer written");
        }
    }

    /// Um pedido que muda o plano, gravado pelo `run write`, faz a cópia sair
    /// logo depois dele, numa spec cuja página já foi publicada: a resposta
    /// traz os lotes, com o pedido, e a ordem de copiá-los. Numa spec ainda
    /// sem página publicada, nada é preparado e o passo segue como antes.
    #[test]
    fn a_request_that_changes_the_plan_is_copied_right_after_it() {
        let dir = approved_project();
        let root = dir.path();
        let said = log(root).visible().into_iter().find(|e| e.event_type == "message").map(|e| e.id);
        let request = json!({"text": "Incluir o Windows.", "keys": ["windows"], "effect": "new_waves", "origin": said});

        let unpublished = write(root, "request", request.clone());
        assert!(unpublished.get("copy").is_none(), "{unpublished}");
        assert!(!unpublished["next"].as_str().unwrap_or_default().contains("write copy"), "{unpublished}");
        assert!(!root.join(".claude/spec/x/copy").exists(), "nothing is prepared before the page exists");

        let first = round(root);
        follow(root, &first);
        let asked = write(root, "request", request);
        let next = asked["next"].as_str().unwrap_or_default();
        assert!(next.starts_with(translate("request.new_waves", Locale::PtBr)), "{next}");
        assert!(next.contains("write copy") && next.contains(SPEC_URL), "{next}");
        assert!(!next.contains("write publish"), "the page already has its address: {next}");
        let last = first["copy"]["spec"]["record"]["last"].as_u64().unwrap_or_default();
        let items = sent_items(root, &asked);
        assert_eq!(items.last(), Some(&id_of(&asked)), "the request goes right after it: {items:?}");
        assert!(items.iter().all(|id| *id > last), "only what came after the last copy: {items:?}");
        assert!(asked["copy"].get("project").is_none(), "{asked}");
    }

    /// Um expurgo gravado depois da última cópia manda de novo o item
    /// anterior que ele tocou: a versão limpa substitui a do banco. O item que
    /// ainda guarda um trecho com cara de segredo depois do expurgo sai do
    /// banco.
    #[test]
    fn a_purge_after_the_copy_sends_the_clean_item_again_or_takes_it_out() {
        let dir = approved_project();
        let root = dir.path();
        let said = log(root).visible().into_iter().find(|e| e.event_type == "message").map(|e| e.id);
        let clean = id_of(&write(root, "note",
            json!({"text": "O código do cofre é azul-marinho-42.", "keys": ["cofre"], "origin": said})));
        let held = id_of(&write(root, "note",
            json!({"text": "Cofre azul-marinho-42, e a senha: S3nh4F0rte2024.", "keys": ["cofre"], "origin": said})));

        let first = round(root);
        let copied = sent_items(root, &first);
        assert!(copied.contains(&clean) && !copied.contains(&held), "{copied:?}");
        follow(root, &first);
        let purge =
            json!({"targets": [clean, held], "reason": "client_data", "excerpt": "azul-marinho-42", "origin": said});
        write(root, "purge", purge);

        let second = round(root);
        let writes = sent(root, &second, "spec");
        let of = |id: u64| writes.iter().find(|w| w["doc_id"] == json!(id.to_string())).cloned();
        let again = of(clean).expect("the purged item goes again");
        assert_eq!((&again["op"], &again["body"]["text"]), (&json!("set"), &json!("O código do cofre é ….")));
        let out = of(held).expect("the item with a secret left leaves the database");
        assert_eq!(out["op"], json!("delete"), "{out}");
        assert!(!writes.iter().any(|w| w.to_string().contains("azul-marinho")), "{writes:?}");
    }

    /// A linha da spec vai para o banco da página do projeto só quando a fase
    /// dela muda: a primeira rodada leva a spec de aprovada para em execução,
    /// e a linha vai; a rodada seguinte não muda a fase, e a linha não vai.
    #[test]
    fn the_project_row_is_copied_only_when_the_phase_changes() {
        let dir = approved_project();
        let root = dir.path();
        // O marco da aprovação copiou a linha na fase de então.
        write(root, "publish", json!({"page": "project", "milestone": "approval", "ok": true, "url": PROJECT_URL}));
        write(root, "copy", json!({"page": "project", "phase": "approved"}));

        let first = round(root);
        assert_eq!(first["publish"], json!(["spec"]), "the project page has its address: {first}");
        let rows = sent(root, &first, "project");
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!((&rows[0]["doc_id"], &rows[0]["body"]["phase"]), (&json!("x"), &json!("running")));
        assert_eq!(first["copy"]["project"]["record"], json!({"page": "project", "phase": "running"}));
        let next = first["next"].as_str().unwrap_or_default();
        assert!(next.contains(PROJECT_URL) && next.contains(r#"'{"page":"project","phase":"running"}'"#), "{next}");
        follow(root, &first);

        let second = round(root);
        assert!(second["copy"].get("project").is_none(), "the phase did not change: {second}");
    }

    /// A cópia de uma spec longa vai em lotes de até 50 escritas, na ordem
    /// dos itens, e o documento das coisas calculadas vai no último.
    #[test]
    fn a_long_copy_goes_in_batches_of_fifty() {
        let dir = approved_project();
        let root = dir.path();
        let said = log(root).visible().into_iter().find(|e| e.event_type == "message").map(|e| e.id);
        for n in 0..60 {
            write(root, "note", json!({"text": format!("Nota {n}."), "keys": ["nota"], "origin": said}));
        }
        let first = round(root);
        let batches = first["copy"]["spec"]["batches"].as_array().cloned().unwrap_or_default();
        assert_eq!(batches, [json!(".claude/spec/x/copy/spec-1.json"), json!(".claude/spec/x/copy/spec-2.json")]);
        let writes = sent(root, &first, "spec");
        let items: Vec<u64> = sent_items(root, &first);
        assert_eq!(items, (1..=log(root).max_id()).filter(|id| items.contains(id)).collect::<Vec<_>>(), "in order");
        assert_eq!(writes.len(), items.len() + 1, "the items and the computed document");
        assert_eq!(writes.last().map(|w| w["collection"].clone()), Some(json!("computed")));
    }
}
