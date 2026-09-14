//! `mustard-rt run pending` — a lista de PENDÊNCIAS que mora fora de qualquer
//! unidade de trabalho.
//!
//! Uma pendência é um trabalho que o operador e o Mustard combinaram fazer e que
//! ainda não fechou. Ela nasce na conversa, pode existir antes de qualquer
//! unidade e pode atravessar várias — e é justamente por isso que nem o canal de
//! material (`material-add`) nem o caderno (`notebook`) servem: os dois vivem no
//! diretório da unidade, e o primeiro recusa gravar quando nenhuma existe.
//!
//! Medido em 09/09/2026: três trabalhos combinados na ordem 2 → 3 → 1. A ordem
//! valia para três unidades e não pertencia a nenhuma; os dois primeiros viraram
//! pull requests, e o resumo de fechamento do dia omitiu o terceiro. O operador
//! só descobriu no dia seguinte, perguntando.
//!
//! ## Onde mora
//!
//! `.claude/pending/ledger.json`, resolvido no checkout PRINCIPAL (o diretório
//! comum do git). Um arquivo versionado mudaria com o branch em uso, e uma
//! unidade cortada antes do item não o veria; um worktree lê o mesmo arquivo que
//! o checkout principal. Por isso o `.claude/.gitignore` semeado cobre
//! `pending/` — o oposto deliberado do caderno, que viaja com o branch.
//!
//! ## Legível por máquina
//!
//! JSON, não prosa: a trava de fim de turno e o resumo da interação leem o mesmo
//! arquivo. Cada item carrega id `P-{n}`, título, detalhe e estado (`open`,
//! `closed` ou `dropped`); fechar ou descartar exige motivo, e um motivo em
//! branco é recusado com o arquivo intacto — um item que some sem dizer por quê
//! é a perda que esta lista existe para impedir. Um item entra uma vez só: o
//! mesmo título, sem ligar para maiúscula nem acento, é recusado apontando o
//! item que já está aberto.
//!
//! ## Datas e notas
//!
//! Cada item guarda o dia em que entrou (`created`, `AAAA-MM-DD` em UTC). Um
//! item gravado antes de a data existir ganha a data do dia na primeira
//! gravação da lista: assim, trinta dias depois, os antigos não voltam todos de
//! uma vez. Dois campos vêm depois: `swept`, o dia em que a faxina mostrou o
//! item, e `became`, a nota "virou a spec X".
//!
//! O início da sessão mostra só uma linha, com a contagem ([`count_line`]); a
//! lista inteira sai da listagem.
//!
//! Recusa sai com exit 1 e o JSON `ok: false`, como o `material-add`.

use std::path::{Path, PathBuf};

use mustard_core::domain::search::{query_terms, SearchIndex};
use mustard_core::domain::spec_events::{search_field, Block, BlockQuery, SpecLog};
use mustard_core::domain::spec_state::SpecState;
use mustard_core::domain::text;
use mustard_core::platform::i18n::Locale;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use mustard_core::io::fs::lock::{read_shared, LockedFile};

use crate::commands::agent::render::prompt_ref::fnv1a64;
use crate::commands::git_settle::main_checkout_root;
use crate::shared::spec_state::DiskSpecState;

/// Options for `mustard-rt run pending`.
#[derive(Debug, Clone, Default)]
pub struct PendingOpts {
    /// Qualquer diretório dentro do repositório; o ledger é resolvido no
    /// checkout principal a partir dele.
    pub root: PathBuf,
    pub add: bool,
    pub title: Option<String>,
    pub detail: Option<String>,
    pub close: Option<String>,
    pub drop: Option<String>,
    pub reason: Option<String>,
    /// O dia de hoje, `AAAA-MM-DD`. `None` lê o relógio; os testes passam um
    /// dia fixo para darem sempre a mesma resposta.
    pub now: Option<String>,
    /// Tira pendências da lista, com um seletor (`id`, `term` ou `before`) e
    /// sempre com motivo. Sem `confirm`, só mostra o que sairia e devolve o
    /// código da confirmação.
    pub remove: bool,
    /// Seletor da remoção: os números, separados por vírgula (`P-2,P-5`).
    pub id: Option<String>,
    /// Seletor da remoção: as palavras, pela busca sobre título e detalhe.
    pub term: Option<String>,
    /// Seletor da remoção: as que entraram antes deste dia, `AAAA-MM-DD`.
    pub before: Option<String>,
    /// O código que a prévia devolveu: a remoção tira exatamente aquele
    /// conjunto.
    pub confirm: Option<String>,
    /// Volta a aberta uma pendência descartada.
    pub reopen: Option<String>,
    /// A faxina: mostra, uma vez só, as abertas paradas há 30 dias ou mais.
    pub stale: bool,
    /// Tira como vencidas as paradas que a última faxina mostrou, menos as de
    /// `keep`.
    pub expire: bool,
    /// As paradas que ficam, na resposta à pergunta da faxina: `P-2,P-5`.
    pub keep: Option<String>,
}

/// O estado de um item. Fechado (`closed`) e descartado (`dropped`) ficam
/// separados de propósito: "entregue" e "desistimos" são respostas diferentes
/// para quem relê a lista.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Status {
    Open,
    Closed,
    Dropped,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::Dropped => "dropped",
        }
    }
}

/// Um item da lista.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PendingItem {
    id: String,
    title: String,
    detail: String,
    status: Status,
    /// Por que o item saiu da lista — presente só depois de fechado ou descartado.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    /// O dia em que o item entrou, `AAAA-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    created: Option<String>,
    /// O dia em que a faxina mostrou o item parado: ele volta uma vez só.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    swept: Option<String>,
    /// A nota "virou a spec X": o nome da spec que nasceu deste item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    became: Option<String>,
}

/// O documento inteiro. `deny_unknown_fields` pelo mesmo motivo do
/// `material-add`: sem ele, uma chave escrita à mão seria aceita aqui e
/// removida em silêncio na próxima gravação.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    #[serde(default)]
    items: Vec<PendingItem>,
    /// O lote da faxina que o usuário ainda não respondeu: os números que o
    /// `--stale` mostrou. O `--expire` o consome.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    sweep: Vec<String>,
    /// Quantas gravações a lista já teve. Toda gravação conta uma, e o código
    /// de uma remoção muda com ela: se a lista mudou, nada sai.
    #[serde(default, skip_serializing_if = "is_zero")]
    revision: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// A recusa de uma pendência repetida: o mesmo título, sem ligar para
/// maiúscula nem acento, de uma que já está aberta.
fn duplicate(open: &PendingItem, lang: Locale) -> Value {
    let hint = mustard_core::translate("pending.duplicate", lang)
        .replace("{id}", &open.id)
        .replace("{title}", &open.title);
    json!({ "ok": false, "reason": "duplicate", "id": open.id, "hint": hint })
}

/// O que a chamada pede — exatamente uma ação.
enum Action {
    List,
    Add { title: String, detail: String },
    Close { id: String, reason: String },
    Remove { selector: Selector, reason: String, confirm: Option<String> },
    Reopen { id: String },
    Stale,
    Expire { keep: Vec<String> },
}

/// O que uma remoção tira: números, palavras ou uma data.
enum Selector {
    Ids(Vec<String>),
    Term(String),
    Before(String),
}

impl Selector {
    /// O seletor como a chamada o escreveu, para a recusa.
    fn spelled(&self) -> String {
        match self {
            Self::Ids(ids) => format!("--id {}", ids.join(",")),
            Self::Term(term) => format!("--term \"{term}\""),
            Self::Before(day) => format!("--before {day}"),
        }
    }
}

/// Há quantos dias, no mínimo, uma pendência aberta está parada para a faxina
/// mostrá-la.
const STALE_DAYS: i64 = 30;

/// Um texto como o arquivo guarda: uma linha, espaços colapsados. Um detalhe
/// colado com quebras de linha não pode virar vários itens nem nenhum.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn refused(reason: &str, hint: &str) -> Value {
    json!({ "ok": false, "reason": reason, "hint": hint })
}

/// O dia de hoje, `AAAA-MM-DD` em UTC: o de `now` quando vem, senão o do
/// relógio.
fn today(now: Option<&str>) -> String {
    now.map(str::trim).filter(|day| !day.is_empty()).map_or_else(
        || mustard_core::time::now_iso8601().get(..10).unwrap_or_default().to_string(),
        str::to_string,
    )
}

/// Uma recusa com o texto do catálogo, no idioma do projeto.
fn refused_in(reason: &str, key: &str, lang: Locale, slots: &[(&str, &str)]) -> Value {
    let hint = slots
        .iter()
        .fold(mustard_core::translate(key, lang).to_string(), |text, (slot, value)| text.replace(slot, value));
    refused(reason, &hint)
}

/// Os números de uma lista `P-2,p-5`, sem espaço, em maiúsculas e sem
/// repetição.
fn parse_ids(text: &str) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for id in text.split(',').map(|id| id.trim().to_ascii_uppercase()).filter(|id| !id.is_empty()) {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

/// O número do dia `AAAA-MM-DD` contado desde 1970; `None` para um texto
/// que não é uma data nesse formato.
fn day_number(day: &str) -> Option<i64> {
    let day = day.trim();
    let shaped = day.len() == 10
        && day.bytes().enumerate().all(|(at, b)| if at == 4 || at == 7 { b == b'-' } else { b.is_ascii_digit() });
    if !shaped {
        return None;
    }
    let month: u32 = day.get(5..7)?.parse().ok()?;
    let date: u32 = day.get(8..10)?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&date) {
        return None;
    }
    mustard_core::time::parse_iso_millis(&format!("{day}T00:00:00Z")).map(|ms| ms.div_euclid(86_400_000))
}

/// A pendência está aberta, entrou há [`STALE_DAYS`] dias ou mais de `today`
/// e a faxina ainda não a mostrou.
fn is_stale(item: &PendingItem, today: Option<i64>) -> bool {
    let entered = item.created.as_deref().and_then(day_number);
    item.status == Status::Open
        && item.swept.is_none()
        && matches!((today, entered), (Some(today), Some(entered)) if today - entered >= STALE_DAYS)
}

/// O seletor da remoção: exatamente um de `--id`, `--term` e `--before`.
fn selector_of(opts: &PendingOpts, lang: Locale) -> Result<Selector, Value> {
    let text = |value: &Option<String>| value.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let mut given = [
        text(&opts.id).map(|ids| Selector::Ids(parse_ids(&ids))),
        text(&opts.term).map(Selector::Term),
        text(&opts.before).map(Selector::Before),
    ]
    .into_iter()
    .flatten();
    let required = || refused_in("selector-required", "pending.selector_required", lang, &[]);
    let (Some(selector), None) = (given.next(), given.next()) else {
        return Err(required());
    };
    match &selector {
        Selector::Ids(ids) if ids.is_empty() => Err(required()),
        Selector::Before(day) if day_number(day).is_none() => {
            Err(refused_in("bad-date", "pending.bad_date", lang, &[("{date}", day)]))
        }
        _ => Ok(selector),
    }
}

/// Traduz as flags em UMA ação, recusando o que não fecha. Toda validação de
/// argumento acontece aqui, ANTES de ler o arquivo — uma recusa nunca toca nele.
fn resolve_action(opts: &PendingOpts, lang: Locale) -> Result<Action, Value> {
    let removing = opts.remove || opts.drop.is_some();
    let chosen = [opts.add, opts.close.is_some(), removing, opts.reopen.is_some(), opts.stale, opts.expire]
        .iter()
        .filter(|on| **on)
        .count();
    if chosen > 1 || (opts.remove && opts.drop.is_some()) {
        return Err(refused(
            "conflicting-actions",
            "pass ONE of `--add`, `--close <id>`, `--drop <id>`, `--remove`, `--reopen <id>`, \
             `--stale` or `--expire` per call",
        ));
    }
    if !opts.add && (opts.title.is_some() || opts.detail.is_some()) {
        return Err(refused(
            "stray-flag",
            "`--title` and `--detail` describe a NEW item — pass them with `--add`",
        ));
    }
    if opts.close.is_none() && !removing && opts.reason.is_some() {
        return Err(refused(
            "stray-flag",
            "`--reason` explains why an item left the list — pass it with `--close <id>`, \
             `--drop <id>` or `--remove`",
        ));
    }

    if opts.add {
        let title = opts.title.as_deref().map(one_line).unwrap_or_default();
        let detail = opts.detail.as_deref().map(one_line).unwrap_or_default();
        if title.is_empty() || detail.is_empty() {
            return Err(refused(
                "missing-field",
                "an item needs both halves: `--title \"<what was agreed>\"` and `--detail \"<scope / why>\"`",
            ));
        }
        return Ok(Action::Add { title, detail });
    }

    // Motivo em branco é motivo nenhum: `--reason ""` e `--reason "   "` recusam
    // igual à flag ausente.
    let reason = opts.reason.as_deref().map(one_line).unwrap_or_default();
    if let Some(id) = &opts.close {
        let id = id.trim().to_ascii_uppercase();
        if id.is_empty() {
            return Err(refused("missing-id", "name the item to settle, e.g. `--close P-1`"));
        }
        if reason.is_empty() {
            return Err(refused(
                "reason-required",
                "closing or dropping an item always carries `--reason \"<what delivered it / why it no longer stands>\"` — nothing was written",
            ));
        }
        return Ok(Action::Close { id, reason });
    }
    if removing {
        if opts.drop.as_deref().is_some_and(|id| id.trim().is_empty()) {
            return Err(refused("missing-id", "name the item to settle, e.g. `--drop P-1`"));
        }
        if reason.is_empty() {
            return Err(refused_in("reason-required", "pending.reason_required", lang, &[]));
        }
        // `--drop P-12` é a remoção pelo número, com a mesma prévia.
        let selector = match &opts.drop {
            Some(id) => Selector::Ids(vec![id.trim().to_ascii_uppercase()]),
            None => selector_of(opts, lang)?,
        };
        let confirm = opts.confirm.as_deref().map(str::trim).filter(|c| !c.is_empty()).map(str::to_string);
        return Ok(Action::Remove { selector, reason, confirm });
    }
    if let Some(id) = &opts.reopen {
        return Ok(Action::Reopen { id: id.trim().to_ascii_uppercase() });
    }
    if opts.stale {
        return Ok(Action::Stale);
    }
    if opts.expire {
        return Ok(Action::Expire { keep: opts.keep.as_deref().map(parse_ids).unwrap_or_default() });
    }
    Ok(Action::List)
}

/// A recusa de um número que não está na lista.
fn unknown_id(id: &str) -> Value {
    refused("unknown-id", &format!("no pending item `{id}` — list the ledger with `mustard-rt run pending`"))
}

/// A recusa de um número que já saiu da lista.
fn already_settled(id: &str, status: Status) -> Value {
    refused("already-settled", &format!("`{id}` is already {} — nothing was written", status.as_str()))
}

/// O número `n` de `P-n`.
fn number(id: &str) -> Option<u64> {
    id.strip_prefix("P-").and_then(|n| n.parse().ok())
}

/// As pendências abertas que o seletor pega, na ordem da lista. Recusa, com o
/// arquivo intacto, quando nenhuma casa, e quando um número pedido não é de
/// uma pendência aberta.
fn select(ledger: &Ledger, selector: &Selector, lang: Locale) -> Result<Vec<String>, Value> {
    let open = || ledger.items.iter().filter(|i| i.status == Status::Open);
    let chosen: Vec<String> = match selector {
        Selector::Ids(ids) => {
            for id in ids {
                let Some(item) = ledger.items.iter().find(|i| &i.id == id) else {
                    return Err(unknown_id(id));
                };
                if item.status != Status::Open {
                    return Err(already_settled(id, item.status));
                }
            }
            open().filter(|i| ids.contains(&i.id)).map(|i| i.id.clone()).collect()
        }
        Selector::Term(term) => {
            let docs: Vec<(u64, String)> = open()
                .filter_map(|i| Some((number(&i.id)?, search_field(Some(&format!("{} {}", i.title, i.detail)), &[]))))
                .collect();
            let hits = SearchIndex::build(docs.iter().map(|(n, search)| (*n, search.as_str())))
                .top(&query_terms(term), docs.len());
            let found: Vec<String> = hits.iter().map(|hit| format!("P-{}", hit.id)).collect();
            open().filter(|i| found.contains(&i.id)).map(|i| i.id.clone()).collect()
        }
        Selector::Before(day) => open()
            .filter(|i| i.created.as_deref().is_some_and(|entered| entered < day.as_str()))
            .map(|i| i.id.clone())
            .collect(),
    };
    if chosen.is_empty() {
        return Err(refused_in("nothing-matches", "pending.nothing_matches", lang, &[("{selector}", &selector.spelled())]));
    }
    Ok(chosen)
}

/// O código da confirmação: a impressão da revisão da lista, do motivo, de
/// cada pendência que sairia (número, estado e título) e da lista aberta
/// inteira. Outro motivo, qualquer gravação no meio (um `--reopen`, uma
/// pendência nova) ou qualquer mudança na lista aberta dão outro código.
fn token(ledger: &Ledger, chosen: &[String], reason: &str) -> String {
    let mut parts = vec![ledger.revision.to_string(), reason.to_string()];
    parts.extend(
        ledger
            .items
            .iter()
            .filter(|i| chosen.contains(&i.id))
            .map(|i| format!("{} {} {}", i.id, i.status.as_str(), i.title)),
    );
    parts.push("--".to_string());
    parts.extend(
        ledger.items.iter().filter(|i| i.status == Status::Open).map(|i| format!("{} {}", i.id, i.title)),
    );
    let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
    format!("{:08x}", fnv1a64(&refs) >> 32)
}

/// As pendências `ids`, como os leitores de fora as veem, na ordem da lista.
fn open_items(ledger: &Ledger, ids: &[String]) -> Vec<OpenPending> {
    ledger
        .items
        .iter()
        .filter(|i| ids.contains(&i.id))
        .map(|i| OpenPending { id: i.id.clone(), title: i.title.clone() })
        .collect()
}

/// O checkout principal, visto de `root`.
///
/// Primeiro a âncora do workspace (`mustard.json`): de dentro de um submódulo o
/// `.` é o submódulo, e o ledger iria parar no repositório errado — o mesmo
/// defeito medido no caderno. Depois o checkout principal da âncora, para que um
/// worktree grave e leia o mesmo arquivo que o principal. Fora de um repositório
/// git, a âncora fica.
fn ledger_root(root: &Path) -> PathBuf {
    let anchor = if root.join("mustard.json").is_file() {
        root.to_path_buf()
    } else {
        PathBuf::from(crate::shared::context::project_dir())
    };
    main_checkout_root(&anchor).unwrap_or(anchor)
}

/// Lê o ledger. Arquivo AUSENTE começa vazio (nada se perdeu); arquivo que
/// existe e não parseia RECUSA — gravar por cima descartaria tudo o que ele
/// guarda, que é exatamente a perda que a lista existe para impedir.
fn load(path: &Path) -> Result<Ledger, Value> {
    if !path.is_file() {
        return Ok(Ledger::default());
    }
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Err(refused(
            "ledger-unreadable",
            "the pending ledger exists and could not be read — fix its permissions; nothing was written",
        ));
    };
    // Vazio não é corrompido: uma criação interrompida deixa zero bytes.
    if raw.trim().is_empty() {
        return Ok(Ledger::default());
    }
    serde_json::from_str::<Ledger>(&raw).map_err(|e| {
        refused(
            "ledger-corrupt",
            &format!(
                "the pending ledger does not parse ({e}) — writing would DISCARD every item it \
                 holds, so nothing was written. Repair the JSON, or move the file aside"
            ),
        )
    })
}

/// O próximo id: um acima do maior `P-{n}` já emitido. Ids nunca se repetem,
/// porque itens fechados continuam no arquivo.
fn next_id(ledger: &Ledger) -> String {
    let max = ledger
        .items
        .iter()
        .filter_map(|i| i.id.strip_prefix("P-").and_then(|n| n.parse::<u64>().ok()))
        .max()
        .unwrap_or(0);
    format!("P-{}", max.saturating_add(1))
}

fn item_json(item: &PendingItem) -> Value {
    let mut out = Map::new();
    out.insert("id".into(), json!(item.id));
    out.insert("title".into(), json!(item.title));
    out.insert("detail".into(), json!(item.detail));
    out.insert("status".into(), json!(item.status.as_str()));
    for (key, value) in [
        ("reason", &item.reason),
        ("created", &item.created),
        ("swept", &item.swept),
        ("became", &item.became),
    ] {
        if let Some(value) = value {
            out.insert(key.into(), json!(value));
        }
    }
    Value::Object(out)
}

/// Grava a lista. Antes, todo item sem data ganha a data do dia (é a
/// primeira gravação dele desde que a data existe), e a revisão conta mais
/// uma.
fn write(path: &Path, ledger: &mut Ledger, today: &str) -> Result<(), Value> {
    for item in ledger.items.iter_mut().filter(|i| i.created.is_none()) {
        item.created = Some(today.to_string());
    }
    ledger.revision = ledger.revision.saturating_add(1);
    let mut body = serde_json::to_string_pretty(ledger)
        .map_err(|e| refused("write-failed", &e.to_string()))?;
    body.push('\n');
    mustard_core::io::fs::write_atomic(path, body.as_bytes())
        .map_err(|e| refused("write-failed", &e.to_string()))
}

/// O passe do ledger — o núcleo testável de [`run`]. Nunca entra em pânico.
#[must_use]
pub(crate) fn pending_at(opts: &PendingOpts) -> Value {
    let project = ledger_root(&opts.root);
    let lang = mustard_core::ProjectConfig::load(&project).language().text_or_default();
    let action = match resolve_action(opts, lang) {
        Ok(a) => a,
        Err(refusal) => return refusal,
    };
    let paths = match mustard_core::ClaudePaths::for_project(&project) {
        Ok(p) => p,
        Err(e) => return refused("bad-root", &e.to_string()),
    };
    let path = paths.pending_ledger_path();
    let mut ledger = match load(&path) {
        Ok(l) => l,
        Err(refusal) => return refusal,
    };
    let today = today(opts.now.as_deref());

    let mut extra = Map::new();
    match action {
        Action::List => {}
        Action::Add { title, detail } => {
            // Uma pendência entra uma vez só: o título é comparado sem
            // maiúscula nem acento ("Humanize" e "humanize" são a mesma), e a
            // repetição é recusada apontando a que já está aberta.
            let key = text::fold(&title);
            if let Some(open) =
                ledger.items.iter().find(|i| i.status == Status::Open && text::fold(&i.title) == key)
            {
                return duplicate(open, lang);
            }
            let id = next_id(&ledger);
            ledger.items.push(PendingItem {
                id: id.clone(),
                title,
                detail,
                status: Status::Open,
                reason: None,
                created: Some(today.clone()),
                swept: None,
                became: None,
            });
            if let Err(refusal) = write(&path, &mut ledger, &today) {
                return refusal;
            }
            extra.insert("id".into(), json!(id));
            extra.insert("added".into(), json!(true));
        }
        Action::Close { id, reason } => {
            let Some(item) = ledger.items.iter_mut().find(|i| i.id == id) else {
                return unknown_id(&id);
            };
            if item.status != Status::Open {
                return already_settled(&id, item.status);
            }
            item.status = Status::Closed;
            item.reason = Some(reason);
            if let Err(refusal) = write(&path, &mut ledger, &today) {
                return refusal;
            }
            extra.insert("id".into(), json!(id));
            extra.insert("status".into(), json!(Status::Closed.as_str()));
        }
        Action::Remove { selector, reason, confirm } => {
            // Duas chamadas: a primeira mostra o que sairia e devolve o código;
            // a segunda, com o código e depois do sim do usuário, tira
            // exatamente aquele conjunto. Se a lista mudou, nada sai.
            let chosen = match select(&ledger, &selector, lang) {
                Ok(ids) => ids,
                Err(refusal) => return refusal,
            };
            let code = token(&ledger, &chosen, &reason);
            match confirm {
                None => {
                    let items = open_items(&ledger, &chosen);
                    let hint = mustard_core::translate("pending.remove.preview", lang)
                        .replace("{count}", &items.len().to_string())
                        .replace("{items}", &format_pending_items(&items, items.len()))
                        .replace("{reason}", &reason)
                        .replace("{token}", &code);
                    extra.insert("preview".into(), json!(true));
                    extra.insert("remove".into(), json!(items));
                    extra.insert("token".into(), json!(code));
                    extra.insert("hint".into(), json!(hint));
                }
                Some(given) if given == code => {
                    for item in ledger.items.iter_mut().filter(|i| chosen.contains(&i.id)) {
                        item.status = Status::Dropped;
                        item.reason = Some(reason.clone());
                    }
                    if let Err(refusal) = write(&path, &mut ledger, &today) {
                        return refusal;
                    }
                    extra.insert("removed".into(), json!(chosen));
                }
                Some(_) => return refused_in("confirm-mismatch", "pending.confirm_mismatch", lang, &[]),
            }
        }
        Action::Reopen { id } => {
            let Some(item) = ledger.items.iter().find(|i| i.id == id) else {
                return unknown_id(&id);
            };
            if item.status != Status::Dropped {
                return refused_in("not-dropped", "pending.not_dropped", lang, &[("{id}", &id)]);
            }
            // Reabrir não cria uma repetida: com o mesmo título já aberto, a
            // reabertura é recusada apontando a que está aberta.
            let key = text::fold(&item.title);
            if let Some(open) =
                ledger.items.iter().find(|i| i.status == Status::Open && text::fold(&i.title) == key)
            {
                return duplicate(open, lang);
            }
            // A reaberta volta a poder aparecer na faxina.
            ledger.sweep.retain(|swept| swept != &id);
            if let Some(item) = ledger.items.iter_mut().find(|i| i.id == id) {
                item.status = Status::Open;
                item.reason = None;
                item.swept = None;
            }
            if let Err(refusal) = write(&path, &mut ledger, &today) {
                return refusal;
            }
            extra.insert("id".into(), json!(id));
            extra.insert("reopened".into(), json!(true));
        }
        Action::Stale => {
            // A faxina mostra cada parada uma vez só: o dia fica gravado nela.
            let now = day_number(&today);
            let stale: Vec<String> =
                ledger.items.iter().filter(|i| is_stale(i, now)).map(|i| i.id.clone()).collect();
            if !stale.is_empty() {
                for item in ledger.items.iter_mut().filter(|i| stale.contains(&i.id)) {
                    item.swept = Some(today.clone());
                }
                for id in &stale {
                    if !ledger.sweep.contains(id) {
                        ledger.sweep.push(id.clone());
                    }
                }
                if let Err(refusal) = write(&path, &mut ledger, &today) {
                    return refusal;
                }
                extra.insert("question".into(), json!(mustard_core::translate("pending.stale.question", lang)));
            }
            let shown: Vec<Value> =
                ledger.items.iter().filter(|i| stale.contains(&i.id)).map(item_json).collect();
            extra.insert("stale".into(), Value::Array(shown));
        }
        Action::Expire { keep } => {
            // O lote é o da faxina que o usuário ainda não respondeu. As que
            // ele não marcou saem como vencidas, e o lote se consome: um
            // segundo `--expire` não tira as que ficaram.
            let batch: Vec<String> = ledger
                .items
                .iter()
                .filter(|i| i.status == Status::Open && ledger.sweep.contains(&i.id))
                .map(|i| i.id.clone())
                .collect();
            if batch.is_empty() {
                return refused_in("nothing-matches", "pending.nothing_matches", lang, &[("{selector}", "--expire")]);
            }
            if let Some(stray) = keep.iter().find(|id| !batch.contains(id)) {
                let selector = format!("--keep {stray}");
                return refused_in("nothing-matches", "pending.nothing_matches", lang, &[("{selector}", &selector)]);
            }
            let expired: Vec<String> = batch.iter().filter(|id| !keep.contains(id)).cloned().collect();
            let reason = mustard_core::translate("pending.expired_reason", lang);
            for item in ledger.items.iter_mut().filter(|i| expired.contains(&i.id)) {
                item.status = Status::Dropped;
                item.reason = Some(reason.to_string());
            }
            ledger.sweep.clear();
            if let Err(refusal) = write(&path, &mut ledger, &today) {
                return refusal;
            }
            extra.insert("expired".into(), json!(expired));
            extra.insert("kept".into(), json!(keep));
        }
    }

    let (open, closed): (Vec<&PendingItem>, Vec<&PendingItem>) =
        ledger.items.iter().partition(|i| i.status == Status::Open);
    let mut report = Map::new();
    report.insert("ok".into(), json!(true));
    // Relativo ao checkout, barras normais: o relatório não carrega caminho de
    // máquina e lê igual em toda plataforma.
    report.insert(
        "path".into(),
        json!(path
            .strip_prefix(&project)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/")),
    );
    report.insert("open".into(), Value::Array(open.into_iter().map(item_json).collect()));
    report.insert("closed".into(), Value::Array(closed.into_iter().map(item_json).collect()));
    report.insert("count_line".into(), json!(count_line_of(&ledger, &today, lang)));
    report.extend(extra);
    Value::Object(report)
}

/// A linha de contagem das pendências abertas, com o complemento das paradas
/// há 30 dias ou mais que a faxina ainda não mostrou; `None` quando nada está
/// aberto.
fn count_line_of(ledger: &Ledger, today: &str, lang: Locale) -> Option<String> {
    let mut line = match ledger.items.iter().filter(|i| i.status == Status::Open).count() {
        0 => return None,
        1 => mustard_core::translate("pending.count.one", lang).to_string(),
        count => mustard_core::translate("pending.count.many", lang).replace("{count}", &count.to_string()),
    };
    let now = day_number(today);
    let stale = ledger.items.iter().filter(|i| is_stale(i, now)).count();
    if stale > 0 {
        line.push_str(&mustard_core::translate("pending.count.stale", lang).replace("{stale}", &stale.to_string()));
    }
    Some(line)
}

/// A linha que o início da sessão mostra: quantas pendências estão abertas e
/// onde está a lista inteira. `None` quando nada está aberto, ou quando a
/// lista não se lê: quem recusa e explica o conserto é `run pending`.
#[must_use]
pub(crate) fn count_line(root: &Path, lang: Locale) -> Option<String> {
    let project = ledger_root(root);
    let paths = mustard_core::ClaudePaths::for_project(&project).ok()?;
    let ledger = load(&paths.pending_ledger_path()).ok()?;
    count_line_of(&ledger, &today(None), lang)
}

/// A chave, no payload do evento `pipeline.kind`, que liga a unidade a uma
/// pendência. Um só nome para quem grava (`emit-pipeline --pending`) e para
/// quem lê (`pr-merge`), para que os dois nunca discordem da grafia.
pub(crate) const UNIT_PENDING_KEY: &str = "pending";

/// Uma pendência aberta, como a enxergam os leitores de fora do ledger — o
/// início de sessão, a cobrança de fim de turno, a abertura e o merge da
/// unidade. Só id e título: é o que cada um deles exibe ou confere.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct OpenPending {
    pub id: String,
    pub title: String,
}

/// As pendências abertas, na ordem do ledger (a ordem em que foram combinadas).
///
/// Lê o MESMO arquivo que `run pending`, resolvido pelo mesmo
/// [`ledger_root`], para que nenhum leitor veja uma lista diferente da que o
/// operador grava. Arquivo ausente, ilegível ou corrompido devolve a lista
/// vazia: quem só EXIBE não tem o que fazer com um ledger quebrado, e
/// `run pending` é quem recusa e diz como consertar.
#[must_use]
pub(crate) fn open_pending(root: &Path) -> Vec<OpenPending> {
    let project = ledger_root(root);
    mustard_core::ClaudePaths::for_project(&project)
        .ok()
        .and_then(|paths| load(&paths.pending_ledger_path()).ok())
        .map(|ledger| {
            ledger
                .items
                .into_iter()
                .filter(|i| i.status == Status::Open)
                .map(|i| OpenPending { id: i.id, title: i.title })
                .collect()
        })
        .unwrap_or_default()
}

/// O número de uma pendência escrito à mão, como `P-12`, `p-12` ou `12`, com
/// ou sem espaço nas pontas: devolve a grafia da lista, `P-12`. Zero e texto
/// sem número dão `None`. É a leitura única do número, a do pedido adiado e a
/// do `open --pending`.
#[must_use]
pub(crate) fn pending_id(text: &str) -> Option<String> {
    let text = text.trim();
    let digits = text.strip_prefix("P-").or_else(|| text.strip_prefix("p-")).unwrap_or(text);
    digits.trim().parse::<u64>().ok().filter(|n| *n > 0).map(|n| format!("P-{n}"))
}

/// A pendência `id` na lista, lida como [`open_pending`] lê: `Some(true)`
/// aberta, `Some(false)` fechada ou descartada, `None` quando a lista não a
/// tem. Uma lista ausente, ilegível ou corrompida não tem pendência nenhuma: é
/// o `run pending` quem diz como consertá-la. O pedido adiado de uma spec só
/// aponta uma pendência aberta daqui.
#[must_use]
pub(crate) fn pending_is_open(root: &Path, id: &str) -> Option<bool> {
    let project = ledger_root(root);
    let ledger = mustard_core::ClaudePaths::for_project(&project)
        .ok()
        .and_then(|paths| load(&paths.pending_ledger_path()).ok())?;
    ledger.items.iter().find(|item| item.id == id).map(|item| item.status == Status::Open)
}

/// Os números das pendências que nasceram na spec: as que um evento
/// `deferred` visível dela cita no campo `pending`.
#[must_use]
pub(crate) fn born_in(log: &SpecLog) -> Vec<String> {
    log.block(BlockQuery::Block(Block::Notes))
        .into_iter()
        .filter(|event| event.event_type == "deferred")
        .filter_map(|event| event.int("pending"))
        .map(|n| format!("P-{n}"))
        .collect()
}

/// As pendências abertas, na ordem da lista, que nasceram na spec do
/// arquivo de eventos `log`.
#[must_use]
pub(crate) fn open_born_in(root: &Path, log: &SpecLog) -> Vec<OpenPending> {
    let born = born_in(log);
    open_pending(root).into_iter().filter(|item| born.contains(&item.id)).collect()
}

/// As pendências abertas que nasceram na spec `spec` do projeto em `root`: é
/// só delas que a entrega pergunta. Vazio quando a spec não tem arquivo de
/// eventos.
#[must_use]
pub(crate) fn open_pending_born_in(root: &Path, spec: &str) -> Vec<OpenPending> {
    DiskSpecState::new(root).log(spec).map(|log| open_born_in(root, &log)).unwrap_or_default()
}

/// Grava na pendência aberta `id` a nota "virou a spec `spec`": a unidade que
/// nasceu dela se chama `spec`, e o merge dessa spec fecha a pendência.
/// `true` quando a nota está gravada.
pub(crate) fn mark_became(root: &Path, id: &str, spec: &str) -> bool {
    let project = ledger_root(root);
    let Ok(paths) = mustard_core::ClaudePaths::for_project(&project) else {
        return false;
    };
    let path = paths.pending_ledger_path();
    let Ok(mut ledger) = load(&path) else {
        return false;
    };
    let Some(item) = ledger.items.iter_mut().find(|i| i.id == id && i.status == Status::Open) else {
        return false;
    };
    if item.became.as_deref() == Some(spec) {
        return true;
    }
    item.became = Some(spec.to_string());
    write(&path, &mut ledger, &today(None)).is_ok()
}

/// O arquivo dos fechamentos armados, ao lado da lista.
const CHARGES_FILE: &str = "charges.json";

/// Um fechamento ou um merge que a ponte armou para a cobrança do fim da
/// resposta: a spec, o número do `state` que fechou e quantas vezes a cobrança
/// já bloqueou por ele.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Charge {
    pub(crate) spec: String,
    pub(crate) closure: u64,
    pub(crate) blocks: u32,
    /// A sessão que armou, quando ela era conhecida: só essa sessão é cobrada.
    /// Sem sessão conhecida, qualquer sessão principal é.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) session: Option<String>,
}

/// O arquivo dos fechamentos armados.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Charges {
    #[serde(default)]
    armed: Vec<Charge>,
}

/// `<checkout principal>/.claude/pending/charges.json`: ao lado da lista, fora
/// da pasta da sessão e fora do git. O checkout principal é o mesmo em que a
/// spec mora, então quem fecha de dentro de um worktree arma o mesmo arquivo
/// que o fim da resposta lê no checkout principal, e vice-versa.
fn charges_path(root: &Path) -> Option<PathBuf> {
    let main = mustard_core::io::spec_events::spec_root(root);
    let ledger = mustard_core::ClaudePaths::for_project(&main).ok()?.pending_ledger_path();
    Some(ledger.parent()?.join(CHARGES_FILE))
}

/// Os fechamentos armados, na ordem em que foram armados. Arquivo ausente ou
/// ilegível dá lista vazia: a cobrança nunca barra por erro próprio.
#[must_use]
pub(crate) fn armed_charges(root: &Path) -> Vec<Charge> {
    charges_path(root)
        .and_then(|path| read_shared(&path).ok())
        .map(|body| parse_charges(&body))
        .unwrap_or_default()
}

/// Os fechamentos de um arquivo; vazio ou ilegível dá lista vazia.
fn parse_charges(body: &str) -> Vec<Charge> {
    serde_json::from_str::<Charges>(body).map(|charges| charges.armed).unwrap_or_default()
}

/// Lê, muda com `change` e grava os fechamentos armados, com a trava
/// exclusiva do arquivo presa do começo ao fim: um fechamento armado por
/// outro processo no meio não some. Sem nenhum fechamento, o arquivo fica
/// vazio. `true` quando gravou.
pub(crate) fn update_charges(root: &Path, change: impl FnOnce(Vec<Charge>) -> Vec<Charge>) -> bool {
    let Some(path) = charges_path(root) else {
        return false;
    };
    let Ok(mut file) = LockedFile::exclusive(&path) else {
        return false;
    };
    let armed = file.read_to_string().map(|body| parse_charges(&body)).unwrap_or_default();
    let next = change(armed);
    let body = if next.is_empty() {
        Ok(Vec::new())
    } else {
        serde_json::to_vec_pretty(&Charges { armed: next })
    };
    body.is_ok_and(|body| file.replace(&body).is_ok())
}

/// Arma a cobrança do fechamento `closure` da spec `spec`, no lugar de um
/// fechamento anterior da mesma spec, guardando a sessão de quem fechou,
/// quando ela é conhecida. `true` quando gravou.
pub(crate) fn arm_charge(root: &Path, spec: &str, closure: u64, session: Option<&str>) -> bool {
    let session = session.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    update_charges(root, |mut armed| {
        armed.retain(|charge| charge.spec != spec);
        armed.push(Charge { spec: spec.to_string(), closure, blocks: 0, session });
        armed
    })
}

/// A pendência aberta que virou a spec `spec`, pela nota da lista.
#[must_use]
pub(crate) fn became_of(root: &Path, spec: &str) -> Option<String> {
    let project = ledger_root(root);
    let paths = mustard_core::ClaudePaths::for_project(&project).ok()?;
    load(&paths.pending_ledger_path())
        .ok()?
        .items
        .into_iter()
        .find(|i| i.status == Status::Open && i.became.as_deref() == Some(spec))
        .map(|i| i.id)
}

/// Fecha `id` como ENTREGUE com `reason`, pelo mesmo passe de `run pending`
/// (motivo obrigatório, item já resolvido recusado). `true` só quando o
/// arquivo foi de fato gravado.
pub(crate) fn close_pending(root: &Path, id: &str, reason: &str) -> bool {
    pending_at(&PendingOpts {
        root: root.to_path_buf(),
        close: Some(id.to_string()),
        reason: Some(reason.to_string()),
        ..PendingOpts::default()
    })["ok"]
        == json!(true)
}

/// Uma linha com os itens, `P-1 "título"; P-2 "título"`, cortada em `cap` com
/// `(+N)` para o resto. A ÚNICA grafia de uma pendência em texto corrido: o
/// aviso de início de sessão, a cobrança de fim de turno e a abertura da
/// unidade escrevem o item do mesmo jeito.
#[must_use]
pub(crate) fn format_pending_items(items: &[OpenPending], cap: usize) -> String {
    let named: Vec<String> =
        items.iter().take(cap).map(|i| format!("{} \"{}\"", i.id, i.title)).collect();
    let rest = items.len().saturating_sub(named.len());
    if rest > 0 {
        format!("{} (+{rest})", named.join("; "))
    } else {
        named.join("; ")
    }
}

/// Run `pending` and print the JSON report; exit 1 on a refusal.
pub fn run(opts: &PendingOpts) {
    let report = pending_at(opts);
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into()));
    if report["ok"] != json!(true) {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::tempdir;

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git").args(args).current_dir(dir).output().expect("spawn git");
        assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// O número de uma pendência se lê como `P-12`, `p-12` ou `12`, com
    /// espaço nas pontas, e sai na grafia da lista; zero, texto sem número e
    /// o vazio não são pendência.
    #[test]
    fn a_pending_number_is_read_as_p_n_or_n() {
        let table = [
            ("P-12", Some("P-12")),
            ("p-12", Some("P-12")),
            ("12", Some("P-12")),
            (" P-12 ", Some("P-12")),
            ("+12", Some("P-12")),
            ("P-0", None),
            ("0", None),
            ("P-x", None),
            ("", None),
        ];
        for (text, id) in table {
            assert_eq!(pending_id(text).as_deref(), id, "{text:?}");
        }
    }

    /// Um repositório parado na base de integração `dev` — nenhuma unidade aberta.
    fn repo() -> tempfile::TempDir {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        git(root, &["init", "."]);
        git(root, &["config", "user.email", "t@t"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["checkout", "-b", "dev"]);
        std::fs::write(root.join("mustard.json"), r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#)
            .expect("cfg");
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "seed"]);
        dir
    }

    fn opts(root: &Path) -> PendingOpts {
        PendingOpts { root: root.to_path_buf(), ..PendingOpts::default() }
    }

    fn add(root: &Path, title: &str, detail: &str) -> Value {
        pending_at(&PendingOpts {
            add: true,
            title: Some(title.into()),
            detail: Some(detail.into()),
            ..opts(root)
        })
    }

    fn ids(list: &Value) -> Vec<String> {
        list.as_array()
            .map(|a| a.iter().filter_map(|i| i["id"].as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    }

    /// Uma pendência gravada sem nenhuma unidade aberta aparece na
    /// listagem lida de OUTRO branch do mesmo checkout, e também de um worktree.
    #[test]
    fn pending_item_added_without_unit_is_listed() {
        let dir = repo();
        let root = dir.path();

        // Na base, sem unidade — o caso que o `material-add` recusa.
        let wrote = add(root, "Humanize", "terceiro trabalho combinado em 10/09");
        assert_eq!(wrote["ok"], json!(true), "report: {wrote}");
        assert_eq!(wrote["id"], json!("P-1"));
        assert_eq!(wrote["added"], json!(true));
        assert_eq!(wrote["path"], json!(".claude/pending/ledger.json"));

        // Outro branch do mesmo checkout lê o mesmo item.
        git(root, &["checkout", "-b", "feature/outra-coisa"]);
        let read = pending_at(&opts(root));
        assert_eq!(read["ok"], json!(true), "report: {read}");
        assert_eq!(ids(&read["open"]), vec!["P-1"], "the item survives the branch switch: {read}");
        assert_eq!(read["open"][0]["title"], json!("Humanize"));
        assert_eq!(read["closed"], json!([]));

        // Um worktree resolve o checkout principal e lê o MESMO arquivo — nada
        // é gravado dentro dele.
        let wt_parent = tempdir().expect("tempdir");
        let wt = wt_parent.path().join("wt");
        git(root, &["worktree", "add", &wt.to_string_lossy(), "-b", "fix/no-worktree"]);
        let from_wt = pending_at(&opts(&wt));
        assert_eq!(ids(&from_wt["open"]), vec!["P-1"], "a worktree reads the main ledger: {from_wt}");
        assert!(
            !wt.join(".claude/pending/ledger.json").exists(),
            "the ledger lives in the main checkout, never in the worktree",
        );
    }

    /// Fechar ou descartar sem motivo (ausente ou em branco) recusa, e o
    /// arquivo fica byte a byte intacto.
    #[test]
    fn pending_close_without_reason_is_refused() {
        let dir = repo();
        let root = dir.path();
        assert_eq!(add(root, "trava de pendencias", "primeira unidade")["id"], json!("P-1"));
        let ledger = root.join(".claude/pending/ledger.json");
        let before = std::fs::read(&ledger).expect("ledger written");

        for reason in [None, Some(""), Some("   "), Some("\n\t")] {
            for closing in [true, false] {
                let id = Some("P-1".to_string());
                let out = pending_at(&PendingOpts {
                    close: if closing { id.clone() } else { None },
                    drop: if closing { None } else { id },
                    reason: reason.map(str::to_string),
                    ..opts(root)
                });
                assert_eq!(out["ok"], json!(false), "{reason:?} closing={closing}: {out}");
                assert_eq!(out["reason"], json!("reason-required"), "{out}");
                assert_eq!(std::fs::read(&ledger).expect("ledger"), before, "the file stays intact");
            }
        }
        assert_eq!(ids(&pending_at(&opts(root))["open"]), vec!["P-1"], "still open");

        // Com motivo, fecha — e o motivo fica no item.
        let closed = pending_at(&PendingOpts {
            close: Some("p-1".into()),
            reason: Some("PR 271 mergeado".into()),
            ..opts(root)
        });
        assert_eq!(closed["ok"], json!(true), "{closed}");
        assert_eq!(closed["status"], json!("closed"));
        assert_eq!(closed["open"], json!([]));
        assert_eq!(closed["closed"][0]["reason"], json!("PR 271 mergeado"));

        // Um item já resolvido não é resolvido de novo.
        let again = pending_at(&PendingOpts {
            drop: Some("P-1".into()),
            reason: Some("mudou de ideia".into()),
            ..opts(root)
        });
        assert_eq!(again["reason"], json!("already-settled"), "{again}");
    }

    /// O `material.md` injetado manda gravar com `run pending` todo
    /// trabalho combinado além da unidade aberta, e continua cabendo no teto do
    /// injetável.
    #[test]
    fn material_injectable_names_the_pending_door() {
        // O mesmo teto e a mesma medida de `apps/cli/tests/template_budget.rs`
        // (`INJECTABLE_CHAR_CAP`, `payload_size`): o maior entre caracteres e
        // bytes, porque o harness não documenta qual dos dois conta.
        const INJECTABLE_CHAR_CAP: usize = 8_000;
        let material = mustard_core::MATERIAL_MD;
        assert!(
            material.contains("mustard-rt run pending --add"),
            "the material part never tells the reader to record agreed work as a pending item",
        );
        assert!(
            material.contains("--close") && material.contains("--drop") && material.contains("--reason"),
            "the material part never says an item leaves the list only with a reason",
        );
        assert!(
            material.contains("BEFORE the gate call"),
            "the material part never says WHEN to record — before the unit opens",
        );
        let size = material.chars().count().max(material.len());
        assert!(size <= INJECTABLE_CHAR_CAP, "material.md is {size}, over the {INJECTABLE_CHAR_CAP} cap");
    }

    /// Arquivo corrompido falha fechado: nada é gravado por cima.
    #[test]
    fn a_corrupt_ledger_is_refused_rather_than_discarded() {
        let dir = repo();
        let root = dir.path();
        let ledger = root.join(".claude/pending/ledger.json");
        std::fs::create_dir_all(ledger.parent().expect("parent")).expect("mkdir");
        std::fs::write(&ledger, r#"{"items":[{"id":"P-1""#).expect("write");

        let out = add(root, "novo", "item");
        assert_eq!(out["ok"], json!(false), "{out}");
        assert_eq!(out["reason"], json!("ledger-corrupt"));
        assert_eq!(std::fs::read_to_string(&ledger).expect("read"), r#"{"items":[{"id":"P-1""#);
    }

    /// Ids sequenciais que nunca se repetem, repetição recusada, e as recusas
    /// de argumento que não fazem sentido juntas.
    #[test]
    fn ids_are_sequential_a_repeat_is_refused_and_stray_flags_refuse() {
        let dir = repo();
        let root = dir.path();
        assert_eq!(add(root, "um", "a")["id"], json!("P-1"));
        assert_eq!(add(root, "dois", "b")["id"], json!("P-2"));
        let again = add(root, "um", "outro detalhe");
        assert_eq!(again["reason"], json!("duplicate"), "{again}");
        assert_eq!(again["id"], json!("P-1"), "the refusal points at the open item");

        let preview = pending_at(&PendingOpts { drop: Some("P-2".into()), reason: Some("x".into()), ..opts(root) });
        let token = preview["token"].as_str().map(str::to_string);
        let dropped = pending_at(&PendingOpts { drop: Some("P-2".into()), reason: Some("x".into()), confirm: token, ..opts(root) });
        assert_eq!(dropped["removed"], json!(["P-2"]), "{dropped}");
        assert_eq!(add(root, "tres", "c")["id"], json!("P-3"), "a settled id is never reused");

        let unknown = pending_at(&PendingOpts { close: Some("P-9".into()), reason: Some("x".into()), ..opts(root) });
        assert_eq!(unknown["reason"], json!("unknown-id"));
        let missing = pending_at(&PendingOpts { add: true, title: Some("so titulo".into()), ..opts(root) });
        assert_eq!(missing["reason"], json!("missing-field"));
        let stray = pending_at(&PendingOpts { reason: Some("sem acao".into()), ..opts(root) });
        assert_eq!(stray["reason"], json!("stray-flag"));
    }

    /// "Humanize" com uma "humanize" já aberta é a mesma pendência: a segunda
    /// é recusada apontando a primeira, e o arquivo fica intacto. Fechada a
    /// primeira, o mesmo título volta a ser uma pendência nova.
    #[test]
    fn a_title_differing_only_in_case_or_accent_is_a_duplicate() {
        let dir = repo();
        let root = dir.path();
        assert_eq!(add(root, "humanize", "a")["id"], json!("P-1"));
        let ledger = root.join(".claude/pending/ledger.json");
        let before = std::fs::read_to_string(&ledger).expect("read");
        for repeat in ["Humanize", "HUMANIZE", "humanizé", "  humanize  "] {
            let refused = add(root, repeat, "b");
            assert_eq!(refused["ok"], json!(false), "{repeat}: {refused}");
            assert_eq!(refused["reason"], json!("duplicate"));
            assert_eq!(refused["id"], json!("P-1"));
            assert!(refused["hint"].as_str().unwrap_or_default().contains("P-1 \"humanize\""));
        }
        assert_eq!(std::fs::read_to_string(&ledger).expect("read"), before, "nothing was written");

        let closed = pending_at(&PendingOpts {
            close: Some("P-1".into()),
            reason: Some("feito".into()),
            ..opts(root)
        });
        assert_eq!(closed["ok"], json!(true), "{closed}");
        assert_eq!(add(root, "Humanize", "c")["id"], json!("P-2"));
    }

    /// O `--add` grava o dia em que o item entrou, e um item gravado antes de
    /// a data existir ganha a data do dia na primeira gravação da lista.
    #[test]
    fn an_added_item_carries_its_day_and_an_undated_one_gets_today_on_the_next_write() {
        let dir = repo();
        let root = dir.path();
        let ledger = root.join(".claude/pending/ledger.json");
        std::fs::create_dir_all(ledger.parent().expect("parent")).expect("mkdir");
        std::fs::write(
            &ledger,
            r#"{"items":[{"id":"P-1","title":"antigo","detail":"sem data","status":"open"}]}"#,
        )
        .expect("seed");

        let listed = pending_at(&opts(root));
        assert_eq!(listed["open"][0].get("created"), None, "listing writes nothing: {listed}");

        let added = pending_at(&PendingOpts {
            add: true,
            title: Some("novo".into()),
            detail: Some("com data".into()),
            now: Some("2026-09-13".into()),
            ..opts(root)
        });
        assert_eq!(added["ok"], json!(true), "{added}");
        assert_eq!(added["open"][1]["created"], json!("2026-09-13"), "{added}");
        assert_eq!(added["open"][0]["created"], json!("2026-09-13"), "the undated item got today: {added}");
    }

    /// A listagem traz a linha de contagem das abertas, no idioma do projeto,
    /// e nenhuma linha quando nada está aberto.
    #[test]
    fn the_listing_carries_the_count_line() {
        let dir = repo();
        let root = dir.path();
        assert_eq!(pending_at(&opts(root))["count_line"], Value::Null, "nothing open");
        add(root, "um", "a");
        assert_eq!(
            pending_at(&opts(root))["count_line"],
            json!(mustard_core::translate("pending.count.one", Locale::default()))
        );
        add(root, "dois", "b");
        let line = pending_at(&opts(root))["count_line"].as_str().unwrap_or_default().to_string();
        assert!(line.starts_with("[Mustard] 2 "), "{line}");
        assert_eq!(count_line(root, Locale::default()).as_deref(), Some(line.as_str()));
    }

    /// Hoje, nos testes da faxina e da remoção.
    const TODAY: &str = "2026-09-13";

    /// Uma pendência gravada no dia `day`, pelo mesmo passe do `--add`.
    fn add_on(root: &Path, title: &str, day: &str) -> Value {
        let out = pending_at(&PendingOpts {
            add: true,
            title: Some(title.into()),
            detail: Some("combinado".into()),
            now: Some(day.into()),
            ..opts(root)
        });
        assert_eq!(out["ok"], json!(true), "seed: {out}");
        out
    }

    fn ledger_bytes(root: &Path) -> Vec<u8> {
        std::fs::read(root.join(".claude/pending/ledger.json")).expect("ledger")
    }

    /// As paradas há 30 dias ou mais aparecem na linha de contagem e voltam
    /// numa faxina só, uma vez; as que o usuário não marcou saem como
    /// vencidas, e as marcadas ficam.
    #[test]
    fn stale_items_come_back_once_and_the_unmarked_ones_expire() {
        let dir = repo();
        let root = dir.path();
        for title in ["velha um", "velha dois", "velha tres"] {
            add_on(root, title, "2026-08-01");
        }
        for title in ["nova um", "nova dois"] {
            add_on(root, title, "2026-09-10");
        }
        let listed = pending_at(&PendingOpts { now: Some(TODAY.into()), ..opts(root) });
        let line = listed["count_line"].as_str().unwrap_or_default();
        assert!(line.contains(" 3 ") && line.contains("--stale"), "the three idle ones are counted: {line}");

        let swept = pending_at(&PendingOpts { stale: true, now: Some(TODAY.into()), ..opts(root) });
        assert_eq!(ids(&swept["stale"]), vec!["P-1", "P-2", "P-3"], "{swept}");
        assert_eq!(swept["question"], json!(mustard_core::translate("pending.stale.question", Locale::default())));
        let again = pending_at(&PendingOpts { stale: true, now: Some(TODAY.into()), ..opts(root) });
        assert_eq!(again["stale"], json!([]), "they come back once: {again}");
        assert!(!again["count_line"].as_str().unwrap_or_default().contains("--stale"), "{again}");

        let before = ledger_bytes(root);
        let stray = pending_at(&PendingOpts { expire: true, keep: Some("P-4".into()), ..opts(root) });
        assert_eq!(stray["reason"], json!("nothing-matches"), "a kept item outside the batch: {stray}");
        assert_eq!(ledger_bytes(root), before, "nothing was removed");

        let expired = pending_at(&PendingOpts { expire: true, keep: Some("p-2".into()), ..opts(root) });
        assert_eq!(expired["expired"], json!(["P-1", "P-3"]), "{expired}");
        assert_eq!(ids(&expired["open"]), vec!["P-2", "P-4", "P-5"], "the marked one stays: {expired}");
        let reason = mustard_core::translate("pending.expired_reason", Locale::default());
        for item in expired["closed"].as_array().expect("closed list") {
            assert_eq!(item["status"], json!("dropped"), "{item}");
            assert_eq!(item["reason"], json!(reason), "{item}");
        }

        // O lote se consome: um segundo `--expire` sem faxina nova não tira a
        // que o usuário marcou para ficar.
        let before = ledger_bytes(root);
        let again = pending_at(&PendingOpts { expire: true, ..opts(root) });
        assert_eq!(again["reason"], json!("nothing-matches"), "{again}");
        assert_eq!(ledger_bytes(root), before, "the kept one stays");
        assert_eq!(ids(&pending_at(&opts(root))["open"]), vec!["P-2", "P-4", "P-5"]);
    }

    /// O código da remoção muda com o motivo, com qualquer gravação no meio
    /// (uma reabertura, sem novo sim) e com qualquer mudança na lista aberta,
    /// mesmo fora do que sairia.
    #[test]
    fn the_removal_code_changes_with_the_reason_a_reopen_or_any_change_in_the_list() {
        let dir = repo();
        let root = dir.path();
        add(root, "Humanize", "a");
        add(root, "Painel", "b");
        let remove = |reason: &str, confirm: Option<String>| {
            pending_at(&PendingOpts {
                remove: true,
                id: Some("P-1".into()),
                reason: Some(reason.into()),
                confirm,
                ..opts(root)
            })
        };
        let token = |report: &Value| report["token"].as_str().map(str::to_string);

        // Outro motivo na confirmação.
        let shown = remove("mudou o plano", None);
        let other = remove("outro motivo", token(&shown));
        assert_eq!(other["reason"], json!("confirm-mismatch"), "{other}");

        // Depois de tirar e reabrir, o código antigo não vale de novo.
        let gone = remove("mudou o plano", token(&shown));
        assert_eq!(gone["removed"], json!(["P-1"]), "{gone}");
        assert_eq!(pending_at(&PendingOpts { reopen: Some("P-1".into()), ..opts(root) })["reopened"], json!(true));
        let replayed = remove("mudou o plano", token(&shown));
        assert_eq!(replayed["reason"], json!("confirm-mismatch"), "a reopen asks for a new yes: {replayed}");

        // Uma pendência nova, fora do que sairia, também muda a lista.
        let shown = remove("mudou o plano", None);
        add(root, "Relatorio", "c");
        let changed = remove("mudou o plano", token(&shown));
        assert_eq!(changed["reason"], json!("confirm-mismatch"), "{changed}");
        assert_eq!(ids(&pending_at(&opts(root))["open"]), vec!["P-1", "P-2", "P-3"], "nothing left");
    }

    /// Reabrir uma pendência com o título de uma já aberta é recusado
    /// apontando a aberta; a reaberta volta a poder aparecer na faxina.
    #[test]
    fn a_reopen_refuses_a_duplicate_and_the_reopened_item_can_be_swept_again() {
        let dir = repo();
        let root = dir.path();
        add_on(root, "Humanize", "2020-01-01");
        let swept = pending_at(&PendingOpts { stale: true, now: Some(TODAY.into()), ..opts(root) });
        assert_eq!(ids(&swept["stale"]), vec!["P-1"], "{swept}");
        let expired = pending_at(&PendingOpts { expire: true, ..opts(root) });
        assert_eq!(expired["expired"], json!(["P-1"]), "{expired}");

        add_on(root, "humanize", TODAY);
        let refused = pending_at(&PendingOpts { reopen: Some("P-1".into()), ..opts(root) });
        assert_eq!(refused["reason"], json!("duplicate"), "{refused}");
        assert_eq!(refused["id"], json!("P-2"), "the refusal points at the open one");

        let closed = pending_at(&PendingOpts { close: Some("P-2".into()), reason: Some("feito".into()), ..opts(root) });
        assert_eq!(closed["ok"], json!(true), "{closed}");
        let back = pending_at(&PendingOpts { reopen: Some("P-1".into()), ..opts(root) });
        assert_eq!(back["reopened"], json!(true), "{back}");
        assert_eq!(back["open"][0].get("swept"), None, "the sweep day is cleared: {back}");
        let again = pending_at(&PendingOpts { stale: true, now: Some(TODAY.into()), ..opts(root) });
        assert_eq!(ids(&again["stale"]), vec!["P-1"], "the reopened item comes back to the sweep: {again}");
    }

    /// A remoção pelo número, por palavra e por data mostra primeiro o que
    /// sairia e devolve um código, sem tocar na lista; com o código, tira
    /// exatamente aquelas; se a lista mudou entre as duas chamadas, nada sai.
    #[test]
    fn removing_by_number_word_or_date_shows_first_and_removes_only_what_was_confirmed() {
        let dir = repo();
        let root = dir.path();
        add_on(root, "Humanize o texto", TODAY);
        add_on(root, "Humanize a pagina", TODAY);
        add_on(root, "Painel novo", TODAY);
        add_on(root, "Relatorio antigo", "2026-07-20");
        let remove = |selector: PendingOpts, confirm: Option<String>| {
            pending_at(&PendingOpts { remove: true, reason: Some("o usuario pediu".into()), confirm, ..selector })
        };
        let by_id = || PendingOpts { id: Some("p-3".into()), ..opts(root) };
        let by_term = || PendingOpts { term: Some("humanize".into()), ..opts(root) };
        let by_day = || PendingOpts { before: Some("2026-08-01".into()), ..opts(root) };
        let token = |report: &Value| report["token"].as_str().map(str::to_string);

        // Pelo número.
        let before = ledger_bytes(root);
        let shown = remove(by_id(), None);
        assert_eq!(shown["preview"], json!(true), "{shown}");
        assert_eq!(shown["remove"], json!([{ "id": "P-3", "title": "Painel novo" }]));
        assert!(shown["hint"].as_str().unwrap_or_default().contains(&token(&shown).unwrap_or_default()));
        assert_eq!(ledger_bytes(root), before, "the preview writes nothing");
        assert_eq!(remove(by_id(), token(&shown))["removed"], json!(["P-3"]));

        // Por palavra: a lista muda entre as duas chamadas, e nada sai.
        let shown = remove(by_term(), None);
        assert_eq!(ids(&shown["remove"]), vec!["P-1", "P-2"], "{shown}");
        add_on(root, "Humanize de novo", TODAY);
        let before = ledger_bytes(root);
        let refused = remove(by_term(), token(&shown));
        assert_eq!(refused["reason"], json!("confirm-mismatch"), "{refused}");
        assert_eq!(ledger_bytes(root), before, "nothing was removed");
        let shown = remove(by_term(), None);
        assert_eq!(ids(&shown["remove"]), vec!["P-1", "P-2", "P-5"], "{shown}");
        assert_eq!(remove(by_term(), token(&shown))["removed"], json!(["P-1", "P-2", "P-5"]));

        // Pela data.
        let shown = remove(by_day(), None);
        assert_eq!(ids(&shown["remove"]), vec!["P-4"], "{shown}");
        let gone = remove(by_day(), token(&shown));
        assert_eq!(gone["removed"], json!(["P-4"]), "{gone}");
        assert_eq!(gone["open"], json!([]), "only what was confirmed left, and all of it did");
    }

    /// Uma remoção sem motivo, ou sem dizer o que remover, é recusada com o
    /// texto do catálogo e a lista fica intacta; um seletor que não pega nada
    /// e uma data que não se lê também.
    #[test]
    fn a_removal_without_a_reason_is_refused_and_nothing_is_written() {
        let dir = repo();
        let root = dir.path();
        add(root, "Humanize", "a");
        let before = ledger_bytes(root);
        let lang = Locale::default();
        let removal = |id: Option<&str>, term: Option<&str>, day: Option<&str>, reason: Option<&str>| {
            pending_at(&PendingOpts {
                remove: true,
                id: id.map(str::to_string),
                term: term.map(str::to_string),
                before: day.map(str::to_string),
                reason: reason.map(str::to_string),
                ..opts(root)
            })
        };
        for (out, code, key) in [
            (removal(Some("P-1"), None, None, None), "reason-required", "pending.reason_required"),
            (removal(Some("P-1"), None, None, Some("   ")), "reason-required", "pending.reason_required"),
            (removal(None, None, None, Some("x")), "selector-required", "pending.selector_required"),
            (removal(None, Some("zzz"), Some("2026-08-01"), Some("x")), "selector-required", "pending.selector_required"),
        ] {
            assert_eq!(out["ok"], json!(false), "{out}");
            assert_eq!(out["reason"], json!(code), "{out}");
            assert_eq!(out["hint"], json!(mustard_core::translate(key, lang)), "{out}");
            assert_eq!(ledger_bytes(root), before, "nothing was written");
        }
        let nothing = removal(None, Some("zzz"), None, Some("x"));
        assert_eq!(nothing["reason"], json!("nothing-matches"), "{nothing}");
        assert!(nothing["hint"].as_str().unwrap_or_default().contains("--term \"zzz\""), "{nothing}");
        let bad = removal(None, None, Some("2026-13-40"), Some("x"));
        assert_eq!(bad["reason"], json!("bad-date"), "{bad}");
        assert_eq!(ledger_bytes(root), before, "nothing was written");
    }

    /// A pendência tirada fica na lista como descartada, com o motivo, e volta
    /// a aberta pelo `--reopen`; só uma descartada volta.
    #[test]
    fn a_removed_item_stays_dropped_with_its_reason_and_can_be_reopened() {
        let dir = repo();
        let root = dir.path();
        add(root, "Humanize", "a");
        add(root, "Painel", "b");
        let drop = |confirm: Option<String>| {
            pending_at(&PendingOpts {
                drop: Some("P-1".into()),
                reason: Some("mudou o plano".into()),
                confirm,
                ..opts(root)
            })
        };
        let gone = drop(drop(None)["token"].as_str().map(str::to_string));
        let dropped = &gone["closed"][0];
        assert_eq!(dropped["id"], json!("P-1"), "{gone}");
        assert_eq!(dropped["status"], json!("dropped"));
        assert_eq!(dropped["reason"], json!("mudou o plano"), "the reason stays on the item");

        let back = pending_at(&PendingOpts { reopen: Some("p-1".into()), ..opts(root) });
        assert_eq!(back["reopened"], json!(true), "{back}");
        assert_eq!(ids(&back["open"]), vec!["P-1", "P-2"]);
        assert_eq!(back["open"][0].get("reason"), None, "an open item carries no reason");

        let not = pending_at(&PendingOpts { reopen: Some("P-2".into()), ..opts(root) });
        assert_eq!(not["reason"], json!("not-dropped"), "{not}");
        assert_eq!(
            not["hint"],
            json!(mustard_core::translate("pending.not_dropped", Locale::default()).replace("{id}", "P-2"))
        );
    }
}
