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
//! Recusa sai com exit 1 e o JSON `ok: false`, como o `material-add`.

use std::path::{Path, PathBuf};

use mustard_core::domain::text;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::commands::git_settle::main_checkout_root;

/// Options for `mustard-rt run pending`.
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
}

/// O documento inteiro. `deny_unknown_fields` pelo mesmo motivo do
/// `material-add`: sem ele, uma chave escrita à mão seria aceita aqui e
/// removida em silêncio na próxima gravação.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    #[serde(default)]
    items: Vec<PendingItem>,
}

/// O que a chamada pede — exatamente uma ação.
enum Action {
    List,
    Add { title: String, detail: String },
    Settle { id: String, status: Status, reason: String },
}

/// Um texto como o arquivo guarda: uma linha, espaços colapsados. Um detalhe
/// colado com quebras de linha não pode virar vários itens nem nenhum.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn refused(reason: &str, hint: &str) -> Value {
    json!({ "ok": false, "reason": reason, "hint": hint })
}

/// Traduz as flags em UMA ação, recusando o que não fecha. Toda validação de
/// argumento acontece aqui, ANTES de ler o arquivo — uma recusa nunca toca nele.
fn resolve_action(opts: &PendingOpts) -> Result<Action, Value> {
    let chosen = [opts.add, opts.close.is_some(), opts.drop.is_some()]
        .iter()
        .filter(|on| **on)
        .count();
    if chosen > 1 {
        return Err(refused(
            "conflicting-actions",
            "pass ONE of `--add`, `--close <id>` or `--drop <id>` per call",
        ));
    }
    if !opts.add && (opts.title.is_some() || opts.detail.is_some()) {
        return Err(refused(
            "stray-flag",
            "`--title` and `--detail` describe a NEW item — pass them with `--add`",
        ));
    }
    if opts.close.is_none() && opts.drop.is_none() && opts.reason.is_some() {
        return Err(refused(
            "stray-flag",
            "`--reason` explains why an item left the list — pass it with `--close <id>` or `--drop <id>`",
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

    let settle = opts
        .close
        .as_deref()
        .map(|id| (id, Status::Closed))
        .or_else(|| opts.drop.as_deref().map(|id| (id, Status::Dropped)));
    let Some((id, status)) = settle else {
        return Ok(Action::List);
    };
    let id = id.trim().to_ascii_uppercase();
    if id.is_empty() {
        return Err(refused("missing-id", "name the item to settle, e.g. `--close P-1`"));
    }
    // Motivo em branco é motivo nenhum: `--reason ""` e `--reason "   "` recusam
    // igual à flag ausente.
    let reason = opts.reason.as_deref().map(one_line).unwrap_or_default();
    if reason.is_empty() {
        return Err(refused(
            "reason-required",
            "closing or dropping an item always carries `--reason \"<what delivered it / why it no longer stands>\"` — nothing was written",
        ));
    }
    Ok(Action::Settle { id, status, reason })
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
    let mut out = json!({
        "id": item.id,
        "title": item.title,
        "detail": item.detail,
        "status": item.status.as_str(),
    });
    if let (Some(reason), Some(map)) = (&item.reason, out.as_object_mut()) {
        map.insert("reason".into(), json!(reason));
    }
    out
}

fn write(path: &Path, ledger: &Ledger) -> Result<(), Value> {
    let mut body = serde_json::to_string_pretty(ledger)
        .map_err(|e| refused("write-failed", &e.to_string()))?;
    body.push('\n');
    mustard_core::io::fs::write_atomic(path, body.as_bytes())
        .map_err(|e| refused("write-failed", &e.to_string()))
}

/// O passe do ledger — o núcleo testável de [`run`]. Nunca entra em pânico.
#[must_use]
pub(crate) fn pending_at(opts: &PendingOpts) -> Value {
    let action = match resolve_action(opts) {
        Ok(a) => a,
        Err(refusal) => return refusal,
    };
    let project = ledger_root(&opts.root);
    let paths = match mustard_core::ClaudePaths::for_project(&project) {
        Ok(p) => p,
        Err(e) => return refused("bad-root", &e.to_string()),
    };
    let path = paths.pending_ledger_path();
    let mut ledger = match load(&path) {
        Ok(l) => l,
        Err(refusal) => return refusal,
    };

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
                let lang = mustard_core::ProjectConfig::load(&project).language().text_or_default();
                let hint = mustard_core::translate("pending.duplicate", lang)
                    .replace("{id}", &open.id)
                    .replace("{title}", &open.title);
                return json!({ "ok": false, "reason": "duplicate", "id": open.id, "hint": hint });
            }
            let id = next_id(&ledger);
            ledger.items.push(PendingItem {
                id: id.clone(),
                title,
                detail,
                status: Status::Open,
                reason: None,
            });
            if let Err(refusal) = write(&path, &ledger) {
                return refusal;
            }
            extra.insert("id".into(), json!(id));
            extra.insert("added".into(), json!(true));
        }
        Action::Settle { id, status, reason } => {
            let Some(item) = ledger.items.iter_mut().find(|i| i.id == id) else {
                return refused(
                    "unknown-id",
                    &format!("no pending item `{id}` — list the ledger with `mustard-rt run pending`"),
                );
            };
            if item.status != Status::Open {
                return refused(
                    "already-settled",
                    &format!("`{id}` is already {} — nothing was written", item.status.as_str()),
                );
            }
            item.status = status;
            item.reason = Some(reason);
            if let Err(refusal) = write(&path, &ledger) {
                return refusal;
            }
            extra.insert("id".into(), json!(id));
            extra.insert("status".into(), json!(status.as_str()));
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
    report.extend(extra);
    Value::Object(report)
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

/// Fecha `id` como ENTREGUE com `reason`, pelo mesmo passe de `run pending`
/// (motivo obrigatório, item já resolvido recusado). `true` só quando o
/// arquivo foi de fato gravado.
pub(crate) fn close_pending(root: &Path, id: &str, reason: &str) -> bool {
    pending_at(&PendingOpts {
        root: root.to_path_buf(),
        add: false,
        title: None,
        detail: None,
        close: Some(id.to_string()),
        drop: None,
        reason: Some(reason.to_string()),
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
        PendingOpts {
            root: root.to_path_buf(),
            add: false,
            title: None,
            detail: None,
            close: None,
            drop: None,
            reason: None,
        }
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

        let _ = pending_at(&PendingOpts { drop: Some("P-2".into()), reason: Some("x".into()), ..opts(root) });
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
}
