//! `mustard-rt run reopen --reason <motivo> [--fix] [--spec <nome>]` — leva a
//! spec de volta para receber um pedido novo, ou abre a porta de conserto do
//! pull request que o servidor reprovou.
//!
//! São duas portas separadas, e quem escolhe é quem chama, pelo que o usuário
//! disse: ajuste novo vai à reabertura, sem opção; servidor reprovou vai ao
//! `--fix`. O pull request vermelho não escolhe sozinho: um ajuste novo pedido
//! com ele vermelho continua sendo reabertura, e não vira onda de conserto
//! presa aos critérios antigos.
//!
//! ## A reabertura, sem opção
//!
//! A volta segue a fase em que a spec está. Nada do que está gravado é
//! apagado — a volta é mais um evento no arquivo, um `state` com a fase nova,
//! e ele guarda quem pediu, quando e por quê.
//!
//! - **Antes do fechamento** (em plano, aprovada ou em execução), a spec volta
//!   ao levantamento, e daí o `grill` roda de novo: os pontos novos convivem
//!   com o que já foi decidido. O levantamento seguinte traz os itens que o
//!   motivo toca, para o usuário dizer se cada um fica, muda ou sai; o que o
//!   motivo não toca fica como está.
//! - **Fechada ou com o pull request aberto**, a spec volta à execução, já
//!   aprovada, na mesma branch e na mesma base: nada do que foi decidido e
//!   aprovado é perguntado de novo. O pedido novo entra pelo `write request`,
//!   como tarefas novas, e as ondas delas saem pela rodada, cada uma na sua
//!   cópia. Depois vem o fechamento de novo, com o revisor final chamado de
//!   novo, e o pull request continua o mesmo.
//!
//!   Com o pull request aberto, o provedor é perguntado antes de gravar se ele
//!   já entrou na base. Entrou: a spec é entregue pelo mesmo caminho do merge
//!   percebido no início da sessão, e a volta é recusada como a de qualquer
//!   spec entregue. Sem resposta, a volta acontece e `warnings` avisa que o
//!   merge não foi conferido. Depois de gravar a volta, o pull request vai
//!   para rascunho, para ninguém juntar pelo botão a versão sem o ajuste; a
//!   resposta traz `pr` e `draft`, e o rascunho recusado vira aviso, sem
//!   desfazer a volta. O `pr-open` do fechamento seguinte tira o rascunho.
//! - **Entregue na base, descartada ou sem fase gravada**, não volta por
//!   caminho nenhum: pedido novo sobre ela é obra nova, pelo `open`.
//!
//! A spec que já está em levantamento não grava nada e responde o mesmo passo.
//!
//! O motivo é obrigatório: é ele que explica, daqui a um mês, por que a spec
//! voltou — e, no levantamento, é ele que vira a consulta. Um `--reason` em
//! branco é recusado.
//!
//! ```text
//! {"ok": true, "spec": "x", "phase": "survey", "from": "running", "id": 42,
//!  "reason": "O pedido mudou de alvo.", "next": "A spec x voltou ao levantamento…"}
//! {"ok": true, "spec": "x", "phase": "running", "from": "pr_open", "id": 57,
//!  "reason": "Ajustar o pedido.", "pr": 12, "draft": true,
//!  "next": "A spec x voltou à execução…"}
//! ```
//!
//! Recusa sai com exit 1 e `ok: false`, com a razão curta em `reason` e a
//! mensagem no idioma do projeto em `hint`.
//!
//! ## A porta de conserto, com `--fix`
//!
//! Uma obra fechada abriu o pull request, e o servidor de integração rodou os
//! testes de novo e reprovou. A obra não é reaberta — o que ela decidiu está
//! certo, o que está errado é o que o servidor achou — e uma spec nova para
//! um conserto de minutos é mentira.
//!
//! A porta só abre quando pedida pelo nome. Numa spec com o pull request
//! aberto, ela pergunta ao provedor as verificações daquele pull request
//! ([`crate::commands::review::pr_door::red_reported`], a mesma leitura do
//! portão do merge). Só o vermelho abre a porta: verde, em andamento, ausente,
//! ilegível ou a spec fora do pull request aberto recusam com `fix-not-red`,
//! e nada é gravado.
//!
//! Com o vermelho, a porta anda em dois momentos, e a fase nunca muda — a obra
//! não é reaberta e nenhuma spec nova nasce:
//!
//! 1. **Abre a onda de conserto.** Grava, na spec fechada, a onda de conserto
//!    e a tarefa dela, com o vermelho relatado. O próximo passo é a rodada de
//!    sempre (`round`), que despacha a onda e comita o conserto na mesma
//!    branch. Chamado de novo enquanto a onda não entregou, não grava nada e
//!    repete o passo.
//! 2. **Empurra.** Com a entrega da onda de conserto gravada, empurra a branch
//!    da spec para o servidor, para ele rodar os testes de novo, e grava na
//!    spec o que foi consertado.
//!
//! A onda de conserto traz as chaves do conserto, as mesmas da nota do
//! empurrão, e só conta numa spec fechada ou com o pull request aberto: a onda
//! nova de uma spec reaberta, em execução, é onda comum.
//!
//! ```text
//! {"ok": true, "spec": "x", "action": "fix", "phase": "pr_open", "pr": 12,
//!  "checks": "failed", "wave": 7, "recorded": true, "next": "…"}
//! ```

use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::{Block, BlockQuery, Refusal, SpecLog};
use mustard_core::domain::spec_state::{reopenable, returns_to_running, PhaseWriter, SpecState, State};
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Map, Value};

use crate::commands::git_settle::settle_unit_at;
use crate::commands::review::pr_door::{merged_elsewhere_with, project_root, provider_checks, red_reported, MergedElsewhere};
use crate::commands::review::pr_publish::{spec_pr, SpecPr};
use crate::commands::spec_events::{self, read::checkout, write::record};
use crate::shared::pr_provider::{provider_for, PrChecks, PrProvider, PrRef};
use crate::shared::spec_state::{session_from_env, DiskSpecState};

/// Como a porta pergunta ao provedor as verificações de um pull request.
/// Injetada para o teste exercitar o vermelho e o verde sem provedor, sem
/// rede e sem repositório, como o portão do merge já faz.
type Checks<'a> = &'a dyn Fn(&Path, u64) -> Result<PrChecks, String>;

/// Como a porta empurra a branch da spec para o servidor. Injetada pelo mesmo
/// motivo, e para o teste provar em que branch o conserto foi empurrado.
type Push<'a> = &'a dyn Fn(&Path, &str) -> Result<(), String>;

/// Como a reabertura chega ao provedor do pull request da spec, a partir da
/// raiz do repositório: para perguntar se ele já entrou na base e para pô-lo
/// em rascunho. Injetado pelo mesmo motivo, para o teste rodar sem rede.
type Provider<'a> = &'a dyn Fn(&Path) -> Box<dyn PrProvider>;

/// A arrumação da branch depois de um merge feito fora, a mesma do início da
/// sessão. Injetada para o teste não mexer em repositório nenhum.
type Settle<'a> = &'a dyn Fn(&Path, &str) -> Value;

/// As chaves de busca da onda de conserto e da nota do empurrão. Fixas nos
/// dois idiomas: é por elas que a porta reconhece a onda de conserto e sabe,
/// na chamada seguinte, que aquele conserto já foi empurrado.
pub(crate) const FIX_KEYS: &[&str] = &["conserto", "pull-request", "servidor"];

/// Options for `mustard-rt run reopen`.
pub struct ReopenOpts {
    /// Qualquer pasta dentro do repositório.
    pub root: PathBuf,
    /// A spec que volta; sem ela, a spec atual.
    pub spec: Option<String>,
    /// Por que a spec volta, palavra por palavra de quem pediu.
    pub reason: String,
    /// A porta de conserto do pull request reprovado, pedida pelo nome. Sem
    /// ela, o passo é a reabertura.
    pub fix: bool,
}

/// Por que a volta não aconteceu.
enum ReopenRefusal {
    /// O `--reason` veio em branco.
    ReasonMissing,
    /// A spec foi entregue na base, descartada ou não tem fase gravada: não
    /// volta por caminho nenhum.
    Settled { spec: String, phase: String },
    /// O `--fix` veio sem o vermelho do servidor relatado, ou fora do pull
    /// request aberto.
    FixNotRed { spec: String, phase: String },
    /// O git recusou empurrar o conserto para o servidor.
    PushFailed { branch: String, error: String },
    /// Uma recusa da leitura ou da gravação do arquivo de eventos.
    Spec(Refusal),
}

impl ReopenRefusal {
    /// A razão curta, estável, para quem lê a saída por máquina.
    fn reason(&self) -> &'static str {
        match self {
            Self::ReasonMissing => "reason-missing",
            Self::Settled { .. } => "spec-settled",
            Self::FixNotRed { .. } => "fix-not-red",
            Self::PushFailed { .. } => "push-failed",
            Self::Spec(refusal) => refusal.reason(),
        }
    }

    /// A mensagem exata, no idioma pedido.
    fn message(&self, lang: Locale) -> String {
        let fill = |key: &str, slots: &[(&str, &str)]| {
            slots.iter().fold(translate(key, lang).to_string(), |text, (slot, value)| text.replace(slot, value))
        };
        match self {
            Self::ReasonMissing => fill("reopen.reason_missing", &[]),
            Self::Settled { spec, phase } => fill("reopen.settled", &[("{spec}", spec), ("{phase}", phase)]),
            Self::FixNotRed { spec, phase } => fill("reopen.fix_not_red", &[("{spec}", spec), ("{phase}", phase)]),
            Self::PushFailed { branch, error } => {
                fill("reopen.fix_push_failed", &[("{branch}", branch), ("{error}", error)])
            }
            Self::Spec(refusal) => refusal.message(lang),
        }
    }

    fn report(&self, lang: Locale) -> Value {
        json!({ "ok": false, "reason": self.reason(), "hint": self.message(lang) })
    }
}

/// O núcleo testável de [`run`]. A sessão vem do ambiente. Nunca entra em
/// pânico.
pub(crate) fn reopen_at(opts: &ReopenOpts) -> Value {
    reopen_for(opts, session_from_env().as_deref())
}

/// [`reopen_at`] com a sessão recebida, e com os dois efeitos de fora ligados
/// no provedor e no git de verdade.
pub(crate) fn reopen_for(opts: &ReopenOpts, session: Option<&str>) -> Value {
    reopen_with(opts, session, &provider_checks, &push_to_server)
}

/// Empurra a branch da unidade para o servidor, que é o que faz o provedor
/// rodar os testes do pull request de novo. O erro do git volta com as
/// palavras dele.
fn push_to_server(root: &Path, branch: &str) -> Result<(), String> {
    mustard_core::platform::git::run(root, &["push", "-q", "origin", branch])
        .result()
        .map(|_| ())
        .map_err(|error| if error.trim().is_empty() { "push-failed".to_string() } else { error })
}

/// [`reopen_for`] com as verificações e o envio recebidos; o provedor do pull
/// request e a arrumação são os de verdade.
pub(crate) fn reopen_with(opts: &ReopenOpts, session: Option<&str>, checks: Checks, push: Push) -> Value {
    reopen_with_provider(opts, session, checks, push, &provider_for, &settle_unit_at)
}

/// [`reopen_with`] com o provedor do pull request e a arrumação recebidos
/// também, que é como um teste escolhe todos os efeitos de fora.
pub(crate) fn reopen_with_provider(
    opts: &ReopenOpts,
    session: Option<&str>,
    checks: Checks,
    push: Push,
    provider: Provider,
    settle: Settle,
) -> Value {
    let project = spec_events::project(&opts.root);
    let lang = project.lang;
    let refuse = |refusal: ReopenRefusal| refusal.report(lang);

    let reason = opts.reason.trim();
    if reason.is_empty() {
        return refuse(ReopenRefusal::ReasonMissing);
    }
    let spec = match opts.spec.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(spec) => spec.to_string(),
        None => match DiskSpecState::new(&checkout(&opts.root)).active(session) {
            Some(spec) => spec,
            None => return refuse(ReopenRefusal::Spec(Refusal::NoCurrentSpec)),
        },
    };
    let path = match store::spec_file(&project.root, &spec) {
        Ok(path) => path,
        Err(refusal) => return refuse(ReopenRefusal::Spec(refusal)),
    };
    let log = match store::read(&path) {
        Ok(Some(log)) => log,
        Ok(None) => return refuse(ReopenRefusal::Spec(Refusal::NoSpecFile { spec })),
        Err(refusal) => return refuse(ReopenRefusal::Spec(refusal)),
    };
    let from = State::from_log(&log).phase.unwrap_or("-");
    // A porta de conserto só abre pedida pelo nome, e só com o vermelho que
    // o provedor relata para o pull request aberto: sem ele, nada é gravado.
    if opts.fix {
        return match (from == "pr_open").then(|| red_reported(&checkout(&opts.root), &spec, checks)) {
            Some(Ok(pr)) => fix_door(opts, &spec, &log, FixRed { pr, reason }, push, lang),
            _ => refuse(ReopenRefusal::FixNotRed { spec, phase: from.to_string() }),
        };
    }
    if from == "survey" {
        return json!({
            "ok": true, "spec": spec, "phase": "survey", "from": from, "recorded": false,
            "next": say("reopen.already", lang, &spec),
        });
    }
    // A volta segue a fase: a spec que ainda não fechou volta ao
    // levantamento; a fechada e a com o pull request aberto voltam à
    // execução, já aprovadas, na mesma branch; o resto não volta.
    let (to, next) = if returns_to_running(from) {
        ("running", "reopen.reopened")
    } else if reopenable(from) {
        ("survey", "reopen.next")
    } else {
        return refuse(ReopenRefusal::Settled { spec, phase: from.to_string() });
    };

    // Com o pull request aberto, o provedor é perguntado antes de gravar: um
    // colega pode ter feito o merge sem o Mustard perceber, e o ajuste novo
    // iria para uma branch já juntada. Juntado, a spec é entregue pelo mesmo
    // caminho do merge percebido no início da sessão, e não volta. Sem
    // resposta, ela volta, e a resposta avisa.
    let mut warnings: Vec<String> = Vec::new();
    if from == "pr_open" {
        match merged_elsewhere_with(&opts.root, &spec, session, provider, settle) {
            Some(MergedElsewhere::Landed { .. }) => {
                let now = store::read(&path)
                    .ok()
                    .flatten()
                    .and_then(|log| State::from_log(&log).phase)
                    .unwrap_or("delivered");
                return refuse(ReopenRefusal::Settled { spec, phase: now.to_string() });
            }
            Some(MergedElsewhere::Unanswered { reason }) => {
                warnings.push(fill("reopen.merge_unchecked", lang, &[("{spec}", &spec), ("{reason}", &reason)]));
            }
            Some(MergedElsewhere::Submodules(_)) | None => {}
        }
    }

    // Só a fase e o motivo: a branch e a base ficam as que a spec já tinha,
    // herdadas pela dobra dos estados.
    let mut draft = Map::new();
    draft.insert("phase".to_string(), json!(to));
    draft.insert("author".to_string(), json!("binary"));
    draft.insert("reason".to_string(), json!(reason));
    let recorded = match record(&opts.root, &spec, "state", draft, PhaseWriter::Binary) {
        Ok(recorded) => recorded,
        Err(refusal) => return refuse(ReopenRefusal::Spec(refusal)),
    };
    let mut answer = json!({
        "ok": true, "spec": spec, "phase": to, "from": from, "recorded": true,
        "id": recorded.written.id, "reason": reason,
        "next": say(next, lang, &spec),
    });
    // Já em execução, o pull request vai para rascunho, e ninguém o junta
    // pelo botão enquanto a spec não fecha de novo. O rascunho recusado não
    // desfaz a volta: a resposta avisa que o pull request ficou liberado.
    if from == "pr_open" {
        let (pr, drafted) = put_in_draft(&opts.root, &spec, provider);
        if let Some(number) = pr {
            answer["pr"] = json!(number);
        }
        answer["draft"] = json!(drafted.is_ok());
        if let Err(reason) = drafted {
            warnings.push(fill("reopen.draft_failed", lang, &[("{spec}", &spec), ("{reason}", &reason)]));
        }
    }
    if !warnings.is_empty() {
        answer["warnings"] = json!(warnings);
    }
    answer
}

/// Põe em rascunho o pull request da spec `spec`: o número gravado no "pull
/// request aberto" ou, sem ele, o que o provedor acha pela branch da spec.
/// Devolve o número, quando se soube qual é, e o que o provedor respondeu.
fn put_in_draft(root: &Path, spec: &str, provider: Provider) -> (Option<u64>, Result<(), String>) {
    let repo = project_root(root);
    let provider = provider(&repo);
    let number = match spec_pr(&repo, spec) {
        Some(SpecPr::Number(number)) => Ok(number),
        Some(SpecPr::Head(branch)) => provider.view(PrRef::Head(&branch)).map(|view| view.number),
        None => Err("pr-unknown".to_string()),
    };
    match number {
        Ok(number) => (Some(number), provider.mark_draft(number)),
        Err(reason) => (None, Err(reason)),
    }
}

/// Um texto do catálogo com o nome da spec preenchido.
fn say(key: &str, lang: Locale, spec: &str) -> String {
    translate(key, lang).replace("{spec}", spec)
}

/// Um texto do catálogo com as vagas preenchidas.
fn fill(key: &str, lang: Locale, slots: &[(&str, &str)]) -> String {
    slots.iter().fold(translate(key, lang).to_string(), |text, (slot, value)| text.replace(slot, value))
}

/// O vermelho relatado que abriu a porta de conserto: o pull request que o
/// servidor reprovou e a frase de quem pediu o conserto.
struct FixRed<'a> {
    /// O número do pull request reprovado.
    pr: u64,
    /// O `--reason`: o que o servidor reprovou, em palavras.
    reason: &'a str,
}

/// A onda de conserto de uma spec fechada.
struct FixWave {
    /// O número da onda.
    n: u64,
    /// O número do evento da entrega dela; `None` enquanto ela não entregou.
    delivered: Option<u64>,
}

/// A onda de conserto mais nova da spec: a última onda gravada depois do
/// fechamento que traz as chaves do conserto, numa spec fechada ou com o pull
/// request aberto.
///
/// O lugar no arquivo não basta: a spec reaberta volta à execução, e a rodada
/// grava nela ondas novas depois do fechamento, que são ondas comuns. As
/// chaves são as que esta porta grava na onda que ela abre, as mesmas da nota
/// do empurrão.
fn fix_wave(log: &SpecLog) -> Option<FixWave> {
    let state = State::from_log(log);
    if !state.phase.is_some_and(returns_to_running) {
        return None;
    }
    let closed = state.last_closing?;
    let n = log
        .block(BlockQuery::Block(Block::Waves))
        .into_iter()
        .filter(|event| event.event_type == "wave" && event.id > closed && carries_fix_keys(event))
        .max_by_key(|event| event.id)?
        .wave()?;
    Some(FixWave { n, delivered: log.last_by_wave("delivered").get(&n).copied() })
}

/// O evento traz as chaves do conserto: a onda que a porta abriu, ou a nota
/// que ela grava ao empurrar.
fn carries_fix_keys(event: &mustard_core::domain::spec_events::SpecEvent) -> bool {
    let keys = event.fields.get("keys").and_then(Value::as_array);
    FIX_KEYS.iter().all(|fix| keys.is_some_and(|keys| keys.iter().any(|key| key.as_str() == Some(fix))))
}

/// Os critérios vigentes da obra, pelo número de cada um.
fn criteria_of(log: &SpecLog) -> Vec<u64> {
    log.block(BlockQuery::Block(Block::Criteria))
        .into_iter()
        .filter(|event| event.event_type == "criterion")
        .map(|event| event.id)
        .collect()
}

/// A onda de conserto aberta de uma spec fechada: a que esta porta gravou e
/// que ainda não entregou. `None` quando não há nenhuma.
///
/// É por ela que a rodada sabe que tem o que despachar numa spec fechada — a
/// única fresta do portão da rodada, e a mesma leitura que esta porta usa,
/// para as duas nunca discordarem sobre o que é onda de conserto.
pub(crate) fn open_fix_wave_of(log: &SpecLog) -> Option<u64> {
    match fix_wave(log) {
        Some(FixWave { n, delivered: None }) => Some(n),
        _ => None,
    }
}

/// O conserto entregue em `delivered` já foi empurrado: existe, depois dele, a
/// nota que esta porta grava ao empurrar.
fn fix_pushed_after(log: &SpecLog, delivered: u64) -> bool {
    log.block(BlockQuery::Block(Block::Notes))
        .into_iter()
        .any(|event| event.event_type == "note" && event.id > delivered && carries_fix_keys(event))
}

/// A porta de conserto do pull request que o servidor reprovou, numa spec que
/// continua fechada: abre a onda de conserto e, com ela entregue, empurra o
/// commit dela para o servidor.
///
/// A fase não é tocada em momento nenhum — a obra não volta ao levantamento e
/// nenhuma spec nova nasce; o que muda é o que a spec fechada passa a contar
/// sobre o conserto.
fn fix_door(opts: &ReopenOpts, spec: &str, log: &SpecLog, red: FixRed, push: Push, lang: Locale) -> Value {
    let refuse = |refusal: ReopenRefusal| refusal.report(lang);
    let pr = red.pr.to_string();
    // Sem branch gravada não há para onde empurrar o conserto, e a porta de
    // conserto não abre.
    let Some(branch) = State::from_log(log).branch.map(|b| b.trim().to_string()).filter(|b| !b.is_empty()) else {
        return refuse(ReopenRefusal::FixNotRed { spec: spec.to_string(), phase: "pr_open".to_string() });
    };

    match fix_wave(log) {
        // A onda de conserto já está aberta e ainda não entregou: nada é
        // gravado de novo, e o passo é o mesmo.
        Some(FixWave { n, delivered: None }) => json!({
            "ok": true, "spec": spec, "action": "fix", "phase": "pr_open", "pr": red.pr,
            "checks": "failed", "wave": n, "recorded": false,
            "next": fill("reopen.fix_waiting", lang, &[("{spec}", spec), ("{wave}", &n.to_string())]),
        }),
        // A onda de conserto entregou e a rodada já comitou na mesma branch:
        // o que falta é o servidor ver o conserto.
        Some(FixWave { n, delivered: Some(id) }) if !fix_pushed_after(log, id) => {
            if let Err(error) = push(&opts.root, &branch) {
                return refuse(ReopenRefusal::PushFailed { branch, error });
            }
            let wave = n.to_string();
            let mut note = Map::new();
            note.insert("author".to_string(), json!("binary"));
            note.insert("keys".to_string(), json!(FIX_KEYS));
            note.insert(
                "text".to_string(),
                fill("reopen.fix_note", lang, &[("{wave}", &wave), ("{pr}", &pr), ("{branch}", &branch)]).into(),
            );
            match record(&opts.root, spec, "note", note, PhaseWriter::Binary) {
                Ok(recorded) => json!({
                    "ok": true, "spec": spec, "action": "fix-pushed", "phase": "pr_open", "pr": red.pr,
                    "checks": "failed", "wave": n, "branch": branch, "recorded": true,
                    "id": recorded.written.id,
                    "next": fill(
                        "reopen.fix_pushed",
                        lang,
                        &[("{spec}", spec), ("{wave}", &wave), ("{pr}", &pr), ("{branch}", &branch)],
                    ),
                }),
                Err(refusal) => refuse(ReopenRefusal::Spec(refusal)),
            }
        }
        // Nenhuma onda de conserto aberta, ou a última já foi empurrada e o
        // servidor reprovou de novo: nasce a onda de conserto, com a tarefa
        // que leva o vermelho relatado a quem vai consertar.
        _ => open_fix_wave(opts, spec, log, &red, lang),
    }
}

/// Grava a onda de conserto e a tarefa dela na spec fechada, e devolve o
/// passo: a rodada de sempre, que despacha a onda e comita o conserto na
/// mesma branch.
fn open_fix_wave(opts: &ReopenOpts, spec: &str, log: &SpecLog, red: &FixRed, lang: Locale) -> Value {
    let refuse = |refusal: ReopenRefusal| refusal.report(lang);
    let pr = red.pr.to_string();
    let n = log.planned_waves().iter().copied().max().unwrap_or(0) + 1;
    let what = fill("reopen.fix_wave_text", lang, &[("{pr}", &pr), ("{reason}", red.reason)]);

    let mut wave = Map::new();
    wave.insert("author".to_string(), json!("binary"));
    wave.insert("n".to_string(), json!(n));
    wave.insert("text".to_string(), json!(what));
    // As chaves do conserto são a marca da onda: é por elas que esta porta e
    // a rodada a reconhecem.
    wave.insert("keys".to_string(), json!(FIX_KEYS));
    // Os critérios da onda de conserto são os da obra: o servidor reprovando
    // diz que o que a obra prometeu não está provado lá, então é por eles que
    // o conserto responde. Critério novo não nasce aqui — a obra já decidiu o
    // que tinha a decidir.
    wave.insert("criteria".to_string(), json!(criteria_of(log)));
    wave.insert("done_when".to_string(), json!(fill("reopen.fix_wave_done", lang, &[("{pr}", &pr)])));
    let recorded = match record(&opts.root, spec, "wave", wave, PhaseWriter::Binary) {
        Ok(recorded) => recorded,
        Err(refusal) => return refuse(ReopenRefusal::Spec(refusal)),
    };

    // A tarefa é o que a rodada despacha: onda sem tarefa nenhuma é lote
    // esvaziado para a fila, e nunca sairia. Os arquivos ficam por dizer —
    // quem descobre quais são é quem lê o que o servidor reprovou.
    let mut task = Map::new();
    task.insert("author".to_string(), json!("binary"));
    task.insert("wave".to_string(), json!(n));
    task.insert("text".to_string(), json!(what));
    task.insert("files".to_string(), json!([]));
    task.insert("depends_on".to_string(), json!([]));
    if let Err(refusal) = record(&opts.root, spec, "task", task, PhaseWriter::Binary) {
        return refuse(ReopenRefusal::Spec(refusal));
    }

    json!({
        "ok": true, "spec": spec, "action": "fix", "phase": "pr_open", "pr": red.pr,
        "checks": "failed", "wave": n, "recorded": true, "id": recorded.written.id,
        "reason": red.reason,
        "next": fill("reopen.fix_opened", lang, &[("{spec}", spec), ("{wave}", &n.to_string()), ("{pr}", &pr)]),
    })
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::spec_events::write::{seed_at, WriteOpts};
    use crate::shared::branch_state::PrStatus;
    use crate::shared::pr_provider::{PrOpened, PrToOpen, PrView};
    use std::cell::RefCell;
    use std::path::Path;
    use std::rc::Rc;
    use tempfile::tempdir;

    /// Uma spec aberta e levada até a fase `phase`, um `state` por fase, pela
    /// gravação do arquivo: o caminho inteiro do fluxo passaria por portas que
    /// não são o assunto deste comando.
    fn spec_in(root: &Path, spec: &str, phase: &str) {
        const STEPS: &[&str] = &["survey", "plan", "approved", "running", "closed", "pr_open", "delivered"];
        let path = store::spec_file(root, spec).expect("spec file");
        std::fs::create_dir_all(path.parent().expect("spec folder")).expect("spec folder");
        for step in STEPS {
            let mut fields = json!({"phase": step, "author": "binary"});
            if *step == "survey" {
                fields["branch"] = json!(format!("feature/{spec}"));
                fields["base"] = json!("dev");
            }
            if *step == "approved" {
                fields["witness"] = json!({"question": "Aprovar?", "answer": "Aprovar"});
            }
            if *step == "pr_open" {
                fields["pr"] = json!({"number": 1, "url": "https://exemplo/1"});
            }
            store::write(&path, "state", fields.as_object().cloned().expect("an object"), &[]).expect("state");
            if *step == phase {
                return;
            }
        }
        panic!("{phase} is not a phase of the flow");
    }

    /// Um provedor de mentira para a reabertura: responde a consulta do pull
    /// request e o pedido de rascunho como o teste manda, e grava cada pedido
    /// com a fase em que a spec estava naquela hora. O que a reabertura nunca
    /// pede grita.
    struct FakeProvider {
        root: PathBuf,
        view: Result<PrStatus, String>,
        draft: Result<(), String>,
        seen: Rc<RefCell<Vec<String>>>,
    }

    impl PrProvider for FakeProvider {
        fn provider(&self) -> &'static str {
            "github"
        }

        fn open(&self, _pr: &PrToOpen) -> Result<PrOpened, String> {
            panic!("the reopen never opens a pull request")
        }

        fn edit_body(&self, _number: u64, _body: &str) -> Result<(), String> {
            panic!("the reopen never rewrites the body")
        }

        fn ready(&self, _number: u64) -> Result<(), String> {
            panic!("the reopen never takes the draft off")
        }

        fn view(&self, which: PrRef<'_>) -> Result<PrView, String> {
            self.seen.borrow_mut().push(format!("view {which:?}"));
            let status = self.view.clone()?;
            Ok(PrView {
                number: 1,
                title: "a obra".into(),
                head: "feature/epico".into(),
                base: "dev".into(),
                status,
                merge_status: None,
                draft: false,
                url: "https://exemplo/1".into(),
            })
        }

        fn checks(&self, _number: u64) -> Result<PrChecks, String> {
            panic!("the reopen without --fix never asks the checks")
        }

        fn branch_protection(&self, _branch: &str) -> Result<bool, String> {
            panic!("the reopen never asks what the server protects")
        }

        fn mark_draft(&self, number: u64) -> Result<(), String> {
            let phase = phase_of(&self.root, "epico").unwrap_or("-");
            self.seen.borrow_mut().push(format!("draft {number} phase={phase}"));
            self.draft.clone()
        }
    }

    /// A reabertura, sem opção, com o provedor do pull request respondendo
    /// `view` à consulta e `draft` ao rascunho. Devolve a resposta, os pedidos
    /// que o provedor recebeu e as branches que a arrumação recebeu.
    fn reopen_through(
        root: &Path,
        view: Result<PrStatus, String>,
        draft: Result<(), String>,
    ) -> (Value, Vec<String>, Vec<String>) {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let settled = RefCell::new(Vec::new());
        let provider = |repo: &Path| -> Box<dyn PrProvider> {
            Box::new(FakeProvider {
                root: repo.to_path_buf(),
                view: view.clone(),
                draft: draft.clone(),
                seen: Rc::clone(&seen),
            })
        };
        let settle = |_: &Path, branch: &str| {
            settled.borrow_mut().push(branch.to_string());
            json!({ "ok": true })
        };
        let out = reopen_with_provider(
            &ReopenOpts {
                root: root.to_path_buf(),
                spec: Some("epico".into()),
                reason: "Ajustar o pedido ao otimizador.".into(),
                fix: false,
            },
            None,
            &|_, _| panic!("the reopen without --fix never asks the checks"),
            &|_, branch| panic!("nada empurra a branch {branch} neste teste"),
            &provider,
            &settle,
        );
        let seen = seen.borrow().clone();
        (out, seen, settled.into_inner())
    }

    /// A reabertura, sem opção, com um provedor que não responde e um git
    /// que não deixa empurrar nada sem alguém pedir.
    fn reopen(root: &Path, spec: &str, reason: &str) -> Value {
        reopen_checked(root, spec, reason, false, &|_, _| Err("no-provider".to_string()))
    }

    /// O reopen com a opção `--fix` quando `fix`, e com as verificações do
    /// provedor escolhidas: é o vermelho delas que abre a porta de conserto.
    /// O provedor do pull request não responde nada, e a arrumação grita.
    fn reopen_checked(root: &Path, spec: &str, reason: &str, fix: bool, checks: Checks) -> Value {
        let silent = |repo: &Path| -> Box<dyn PrProvider> {
            Box::new(FakeProvider {
                root: repo.to_path_buf(),
                view: Err("no-provider".into()),
                draft: Err("no-provider".into()),
                seen: Rc::new(RefCell::new(Vec::new())),
            })
        };
        reopen_with_provider(
            &ReopenOpts { root: root.to_path_buf(), spec: Some(spec.to_string()), reason: reason.to_string(), fix },
            None,
            checks,
            &|_, branch| panic!("nada empurra a branch {branch} neste teste"),
            &silent,
            &|_, branch| panic!("nada arruma a branch {branch} neste teste"),
        )
    }

    /// A spec com o pull request aberto volta à execução, e o pull request
    /// vai para rascunho depois de a volta estar gravada: com o provedor
    /// aceitando, a resposta traz o número e `draft: true`, sem aviso. Com o
    /// provedor recusando, a spec volta à execução do mesmo jeito, com
    /// `draft: false` e o aviso de que o pull request ficou liberado.
    #[test]
    fn reopening_a_pr_open_spec_puts_its_pull_request_in_draft() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        spec_in(root, "epico", "pr_open");
        let (out, seen, settled) = reopen_through(root, Ok(PrStatus::Open), Ok(()));
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!((out["from"].clone(), out["phase"].clone()), (json!("pr_open"), json!("running")), "{out}");
        assert_eq!(out["pr"], json!(1), "{out}");
        assert_eq!(out["draft"], json!(true), "{out}");
        assert!(out["warnings"].is_null(), "{out}");
        assert_eq!(seen, ["view Number(1)", "draft 1 phase=running"], "the draft comes after the return is recorded");
        assert!(settled.is_empty(), "an open pull request is not settled: {settled:?}");
        assert_eq!(phase_of(root, "epico"), Some("running"));

        let dir = tempdir().unwrap();
        let root = dir.path();
        spec_in(root, "epico", "pr_open");
        let (out, seen, _) = reopen_through(root, Ok(PrStatus::Open), Err("gh-failed".into()));
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["phase"], json!("running"), "{out}");
        assert_eq!(out["pr"], json!(1), "{out}");
        assert_eq!(out["draft"], json!(false), "{out}");
        let warnings = out["warnings"].as_array().cloned().unwrap_or_default();
        assert_eq!(warnings.len(), 1, "{out}");
        let warning = warnings[0].as_str().unwrap_or_default();
        assert!(
            warning.contains("epico") && warning.contains("gh-failed") && warning.contains("liberado"),
            "{warning}",
        );
        assert_eq!(seen, ["view Number(1)", "draft 1 phase=running"]);
        assert_eq!(phase_of(root, "epico"), Some("running"), "the refused draft does not undo the return");
    }

    /// O pull request que um colega já juntou pelo provedor, sem o Mustard
    /// perceber: a reabertura grava a spec como entregue, pelo mesmo caminho
    /// do merge percebido no início da sessão, e é recusada como a de spec
    /// entregue, sem `state` de execução nem rascunho. Com o provedor sem
    /// resposta, a spec volta à execução, e a resposta avisa que o merge não
    /// foi conferido — e, com o rascunho também sem resposta, que o pull
    /// request ficou liberado.
    #[test]
    fn a_pull_request_merged_outside_is_delivered_and_the_reopen_refused() {
        let states = |root: &Path| -> Vec<String> {
            DiskSpecState::new(root)
                .log("epico")
                .expect("the event file")
                .events
                .iter()
                .filter(|e| e.event_type == "state")
                .filter_map(|e| e.str_field("phase").map(str::to_string))
                .collect()
        };

        let dir = tempdir().unwrap();
        let root = dir.path();
        spec_in(root, "epico", "pr_open");
        let before = states(root);
        let (out, seen, settled) = reopen_through(root, Ok(PrStatus::Merged), Ok(()));
        assert_eq!(out["ok"], json!(false), "{out}");
        assert_eq!(out["reason"], json!("spec-settled"), "{out}");
        let hint = out["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("epico") && hint.contains("delivered"), "{hint}");
        assert_eq!(phase_of(root, "epico"), Some("delivered"));
        let after = states(root);
        assert_eq!(after.len(), before.len() + 1, "one state more: {after:?}");
        assert_eq!(after.last().map(String::as_str), Some("delivered"), "no running after the merge: {after:?}");
        assert_eq!(seen, ["view Number(1)"], "no draft on a merged pull request");
        assert_eq!(settled, ["feature/epico"], "the same settling as the merge seen at the session start");

        let dir = tempdir().unwrap();
        let root = dir.path();
        spec_in(root, "epico", "pr_open");
        let (out, _, settled) = reopen_through(root, Err("gh-not-found".into()), Err("gh-not-found".into()));
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["phase"], json!("running"), "{out}");
        assert_eq!(out["draft"], json!(false), "{out}");
        let warnings: Vec<String> = out["warnings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|w| w.as_str().map(str::to_string))
            .collect();
        assert_eq!(warnings.len(), 2, "{out}");
        assert!(warnings[0].contains("conferir") && warnings[0].contains("gh-not-found"), "{warnings:?}");
        assert!(warnings[1].contains("liberado"), "{warnings:?}");
        assert!(settled.is_empty());
        assert_eq!(phase_of(root, "epico"), Some("running"));
    }

    /// Os bytes do arquivo de eventos da spec, para provar que a recusa não
    /// gravou nada.
    fn bytes_of(root: &Path, spec: &str) -> Vec<u8> {
        std::fs::read(store::spec_file(root, spec).expect("spec file")).expect("the event file")
    }

    fn phase_of(root: &Path, spec: &str) -> Option<&'static str> {
        State::from_log(&DiskSpecState::new(root).log(spec).expect("the event file")).phase
    }

    /// Uma spec em execução volta ao levantamento: a fase é a de levantamento
    /// de novo, o motivo fica gravado no evento da volta, e nada do que estava
    /// no arquivo sai.
    #[test]
    fn a_running_spec_goes_back_to_the_survey_with_the_reason_on_the_record() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        spec_in(root, "epico", "running");
        let before = DiskSpecState::new(root).log("epico").expect("the event file").events.len();

        let out = reopen(root, "epico", "  O pedido mudou de alvo.  ");
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["from"], json!("running"), "{out}");
        assert_eq!(out["phase"], json!("survey"), "{out}");
        assert_eq!(out["reason"], json!("O pedido mudou de alvo."), "the reason is trimmed: {out}");
        assert!(out["next"].as_str().unwrap().contains("grill"), "{out}");
        assert_eq!(phase_of(root, "epico"), Some("survey"));

        let log = DiskSpecState::new(root).log("epico").expect("the event file");
        assert_eq!(log.events.len(), before + 1, "one event more, and not one less");
        let back = log.get(out["id"].as_u64().unwrap()).expect("the event of the return");
        assert_eq!(back.str_field("reason").map(str::trim), Some("O pedido mudou de alvo."));
        assert_eq!(back.str_field("phase"), Some("survey"));
    }

    /// Depois da volta, a branch e a base que a spec tinha continuam no
    /// estado: a volta não desfaz o que estava gravado.
    #[test]
    fn the_branch_and_the_base_survive_the_return() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        spec_in(root, "epico", "approved");
        assert_eq!(reopen(root, "epico", "Faltou levantar o limite.")["ok"], json!(true));
        let state = State::from_log(&DiskSpecState::new(root).log("epico").expect("the event file"));
        assert_eq!(state.phase, Some("survey"));
        assert_eq!(state.branch.as_deref(), Some("feature/epico"));
        assert_eq!(state.base.as_deref(), Some("dev"));
        assert!(!state.approved, "the spec is under survey again");
    }

    /// O `--fix` sem o vermelho relatado não reabre nada: a spec fechada, a
    /// com o pull request aberto e a entregue, com o provedor sem resposta,
    /// são recusadas dizendo a fase em que estão, e nada é gravado.
    #[test]
    fn a_settled_spec_is_refused_and_nothing_is_written() {
        for phase in ["closed", "pr_open", "delivered"] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            spec_in(root, "epico", phase);
            let before = bytes_of(root, "epico");

            let out = reopen_checked(root, "epico", "Quero consertar.", true, &|_, _| Err("no-provider".into()));
            assert_eq!(out["reason"], json!("fix-not-red"), "{phase}: {out}");
            let hint = out["hint"].as_str().unwrap();
            assert!(hint.contains("epico") && hint.contains(phase), "{hint}");
            assert_eq!(bytes_of(root, "epico"), before);
            assert_eq!(phase_of(root, "epico"), Some(phase));
        }
    }

    /// A spec fechada e a com o pull request aberto voltam à execução pelo
    /// reopen sem opção, com o provedor sem resposta: um `state` com a fase
    /// de execução e o motivo, já aprovada, na mesma branch e na mesma base,
    /// e a resposta manda gravar o pedido novo pelo `write request`. A spec
    /// em execução continua voltando ao levantamento.
    #[test]
    fn a_closed_or_pr_open_spec_reopens_to_running_on_the_same_branch() {
        for phase in ["closed", "pr_open"] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            spec_in(root, "epico", phase);
            let before = DiskSpecState::new(root).log("epico").expect("the event file").events.len();

            let out = reopen(root, "epico", "  Ajustar o pedido ao otimizador.  ");
            assert_eq!(out["ok"], json!(true), "{phase}: {out}");
            assert_eq!(out["phase"], json!("running"), "{phase}: {out}");
            assert_eq!(out["from"], json!(phase), "{out}");
            assert_eq!(out["recorded"], json!(true), "{out}");
            assert_eq!(out["reason"], json!("Ajustar o pedido ao otimizador."), "{out}");
            assert!(out["action"].is_null(), "no fix door without the option: {out}");
            let next = out["next"].as_str().unwrap_or_default();
            assert!(next.contains("mustard-rt run write request --spec epico"), "{next}");

            let log = DiskSpecState::new(root).log("epico").expect("the event file");
            assert_eq!(log.events.len(), before + 1, "{phase}: one state more, and nothing else");
            let back = log.get(out["id"].as_u64().unwrap_or_else(|| panic!("{out}"))).expect("the return");
            assert_eq!(back.event_type, "state");
            assert_eq!(back.str_field("phase"), Some("running"));
            assert_eq!(back.str_field("reason").map(str::trim), Some("Ajustar o pedido ao otimizador."));
            let state = State::from_log(&log);
            assert_eq!(state.phase, Some("running"), "{phase}");
            assert!(state.approved, "{phase}: back to running already approved");
            assert!(mustard_core::domain::spec_state::approval_event(&log).is_some(), "{phase}: the approval holds");
            assert_eq!(state.branch.as_deref(), Some("feature/epico"), "{phase}: the same branch");
            assert_eq!(state.base.as_deref(), Some("dev"), "{phase}: the same base");
        }

        // Antes do fechamento, a volta continua sendo ao levantamento.
        let dir = tempdir().unwrap();
        let root = dir.path();
        spec_in(root, "epico", "running");
        let out = reopen(root, "epico", "O pedido mudou de alvo.");
        assert_eq!((out["from"].clone(), out["phase"].clone()), (json!("running"), json!("survey")), "{out}");
        assert_eq!(phase_of(root, "epico"), Some("survey"));
    }

    /// A spec entregue na base, a descartada e a sem fase gravada não voltam
    /// por caminho nenhum: o reopen recusa e nada é gravado. A frase da
    /// recusa fala só da entregue e da descartada, e aponta uma spec nova,
    /// nos dois idiomas.
    #[test]
    fn delivered_discarded_or_phaseless_specs_still_refuse_the_reopen() {
        let languages = [
            ("pt-BR", ["entregue", "descartada", "mustard-rt run open"], ["fechada", "pull request", "levantamento"]),
            ("en-US", ["delivered", "discarded", "mustard-rt run open"], ["closed", "pull request", "survey"]),
        ];
        for (language, says, never) in languages {
            for phase in ["delivered", "discarded", "-"] {
                let dir = tempdir().unwrap();
                let root = dir.path();
                std::fs::write(root.join("mustard.json"), format!(r#"{{"language":{{"text":"{language}"}}}}"#))
                    .unwrap();
                let path = store::spec_file(root, "epico").expect("spec file");
                match phase {
                    "-" => {
                        std::fs::create_dir_all(path.parent().expect("spec folder")).expect("spec folder");
                        let said = json!({"author": "user", "text": "Travar o merge."});
                        store::write(&path, "message", said.as_object().cloned().expect("an object"), &[])
                            .expect("a message");
                    }
                    "discarded" => {
                        spec_in(root, "epico", "running");
                        let gone = json!({"phase": "discarded", "author": "binary", "reason": "Descartada."});
                        store::write(&path, "state", gone.as_object().cloned().expect("an object"), &[])
                            .expect("the discard");
                    }
                    settled => spec_in(root, "epico", settled),
                }
                let before = bytes_of(root, "epico");

                let out = reopen(root, "epico", "Pedido novo sobre ela.");
                assert_eq!(out["ok"], json!(false), "{language} {phase}: {out}");
                assert_eq!(out["reason"], json!("spec-settled"), "{language} {phase}: {out}");
                assert_eq!(bytes_of(root, "epico"), before, "{language} {phase}: nothing was written");
                let hint = out["hint"].as_str().unwrap_or_default();
                for word in says {
                    assert!(hint.contains(word), "{language} {phase}: no {word:?} in {hint}");
                }
                for word in never {
                    assert!(!hint.contains(word), "{language} {phase}: {word:?} in {hint}");
                }
            }
        }
    }

    /// O pull request vermelho não escolhe a porta: sem `--fix`, a spec volta
    /// à execução sem perguntar as verificações, e nenhuma onda de conserto
    /// nasce. Com `--fix` e o vermelho, a onda de conserto nasce, com as
    /// chaves do conserto, e a fase continua no pull request aberto. Com
    /// `--fix` sem o vermelho — verde, em andamento, sem verificação, sem
    /// resposta ou fora do pull request aberto —, recusa, e nada é gravado.
    #[test]
    fn a_red_pull_request_opens_the_fix_wave_only_with_fix() {
        let red: Checks = &|_, _| Ok(PrChecks::Failed);
        let waves = |root: &Path| {
            DiskSpecState::new(root)
                .log("epico")
                .expect("the event file")
                .events
                .iter()
                .filter(|e| e.event_type == "wave")
                .count()
        };

        // Sem `--fix`: a reabertura, e o provedor nem é perguntado.
        let dir = tempdir().unwrap();
        let root = dir.path();
        spec_in(root, "epico", "pr_open");
        criterion_of_the_work(root, "epico");
        let out = reopen_checked(root, "epico", "Ajuste novo no pedido.", false, &|_, _| {
            panic!("the reopen without --fix never asks the provider")
        });
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["phase"], json!("running"), "{out}");
        assert!(out["action"].is_null(), "{out}");
        assert_eq!(phase_of(root, "epico"), Some("running"));
        assert_eq!(waves(root), 0, "no fix wave was born");

        // Com `--fix` e o vermelho: a onda de conserto, e a fase não muda.
        let dir = tempdir().unwrap();
        let root = dir.path();
        spec_in(root, "epico", "pr_open");
        criterion_of_the_work(root, "epico");
        let opened = reopen_checked(root, "epico", "O servidor reprovou dois testes.", true, red);
        assert_eq!(opened["action"], json!("fix"), "{opened}");
        assert_eq!(opened["phase"], json!("pr_open"), "{opened}");
        let wave = opened["wave"].as_u64().unwrap_or_else(|| panic!("{opened}"));
        assert_eq!(phase_of(root, "epico"), Some("pr_open"));
        assert_eq!(waves(root), 1, "the fix wave was born");
        let log = DiskSpecState::new(root).log("epico").expect("the event file");
        let recorded = log.get(opened["id"].as_u64().unwrap_or_else(|| panic!("{opened}"))).expect("the fix wave");
        let keys: Vec<&str> =
            recorded.fields.get("keys").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
        assert_eq!(keys, FIX_KEYS, "the fix wave carries the fix keys");
        assert_eq!(open_fix_wave_of(&log), Some(wave));

        // Com `--fix` sem o vermelho: recusa, e nada é gravado.
        let not_red: [(&str, Checks); 4] = [
            ("passed", &|_, _| Ok(PrChecks::Passed)),
            ("running", &|_, _| Ok(PrChecks::Running)),
            ("absent", &|_, _| Ok(PrChecks::Absent)),
            ("no answer", &|_, _| Err("no-provider".to_string())),
        ];
        for (said, checks) in not_red {
            let dir = tempdir().unwrap();
            let root = dir.path();
            spec_in(root, "epico", "pr_open");
            criterion_of_the_work(root, "epico");
            let before = bytes_of(root, "epico");
            let out = reopen_checked(root, "epico", "Quero consertar.", true, checks);
            assert_eq!(out["ok"], json!(false), "{said}: {out}");
            assert_eq!(out["reason"], json!("fix-not-red"), "{said}: {out}");
            assert_eq!(bytes_of(root, "epico"), before, "{said}: nothing was written");
        }
        // Fora do pull request aberto, nem o vermelho abre a porta.
        let dir = tempdir().unwrap();
        let root = dir.path();
        spec_in(root, "epico", "closed");
        let before = bytes_of(root, "epico");
        let out = reopen_checked(root, "epico", "Quero consertar.", true, red);
        assert_eq!(out["reason"], json!("fix-not-red"), "{out}");
        assert_eq!(bytes_of(root, "epico"), before, "closed: nothing was written");
    }

    /// O motivo é obrigatório: em branco, a volta é recusada e nada é
    /// gravado.
    #[test]
    fn a_blank_reason_is_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        spec_in(root, "epico", "running");
        let before = std::fs::read(store::spec_file(root, "epico").unwrap()).unwrap();

        let out = reopen(root, "epico", "   ");
        assert_eq!(out["reason"], json!("reason-missing"), "{out}");
        assert!(out["hint"].as_str().unwrap().contains("--reason"), "{out}");
        assert_eq!(std::fs::read(store::spec_file(root, "epico").unwrap()).unwrap(), before);
        assert_eq!(phase_of(root, "epico"), Some("running"));
    }

    /// Uma spec que já está em levantamento não grava nada e responde o mesmo
    /// passo; uma spec sem arquivo de eventos é recusada pelo nome.
    #[test]
    fn a_spec_already_under_survey_writes_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        spec_in(root, "epico", "survey");
        let before = std::fs::read(store::spec_file(root, "epico").unwrap()).unwrap();

        let out = reopen(root, "epico", "Levantar de novo.");
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["recorded"], json!(false), "{out}");
        assert!(out["next"].as_str().unwrap().contains("grill"), "{out}");
        assert_eq!(std::fs::read(store::spec_file(root, "epico").unwrap()).unwrap(), before);

        let missing = reopen(root, "nunca-aberta", "Levantar de novo.");
        assert_eq!(missing["reason"], json!("no-spec-file"), "{missing}");
    }

    /// Um critério da obra, que é o que a onda de conserto responde: sem
    /// critério nenhum não há obra a consertar.
    fn criterion_of_the_work(root: &Path, spec: &str) -> u64 {
        let said = seed_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: Some(spec.to_string()),
            event_type: "message".into(),
            json: json!({"author": "user", "text": "Travar o merge."}).to_string(),
        });
        let said = said["id"].as_u64().unwrap_or_else(|| panic!("{said}"));
        let criterion = seed_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: Some(spec.to_string()),
            event_type: "criterion".into(),
            json: json!({"when": "a suíte roda", "then": "ela passa", "proof": "cargo test",
                "form": "event_driven", "origin": said})
            .to_string(),
        });
        criterion["id"].as_u64().unwrap_or_else(|| panic!("{criterion}"))
    }

    /// O que a rodada deixa quando a onda de conserto entrega: a entrega e o
    /// commit dela, gravados pela mesma porta do binário que a rodada usa.
    fn round_delivers(root: &Path, spec: &str, wave: u64) {
        let delivered = json!({"author": "binary", "wave": wave, "text": "O teste do servidor voltou verde.",
            "files": ["apps/rt/src/lib.rs"]});
        record(root, spec, "delivered", delivered.as_object().cloned().expect("an object"), PhaseWriter::Binary)
            .expect("the delivery of the fix wave");
        let commit = json!({"author": "binary", "sha": "abc1234", "title": "conserto do que o servidor reprovou",
            "waves": [wave], "files": ["apps/rt/src/lib.rs"], "repo": "."});
        record(root, spec, "commit", commit.as_object().cloned().expect("an object"), PhaseWriter::Binary)
            .expect("the commit of the fix wave");
    }

    /// O pull request de uma obra já fechada volta reprovado pelo servidor, e
    /// há porta de conserto pelo `--fix`: o vermelho relatado é aceito, a onda
    /// de conserto abre dentro da spec fechada, a rodada a comita na mesma
    /// branch e este mesmo passo empurra o commit — sem reabrir a obra e sem
    /// spec nova.
    ///
    /// Sem o vermelho não há porta: com as verificações verdes o `--fix` é
    /// recusado, e nada é gravado nem empurrado.
    #[test]
    fn o_pull_request_reprovado_tem_porta_de_conserto() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        spec_in(root, "epico", "pr_open");
        let criterion = criterion_of_the_work(root, "epico");
        let spec_file = store::spec_file(root, "epico").expect("spec file");
        let folder = spec_file.parent().and_then(Path::parent).expect("the specs folder").to_path_buf();
        let specs = || {
            std::fs::read_dir(&folder)
                .expect("the specs folder")
                .flatten()
                .filter(|entry| entry.path().join("spec.ndjson").is_file())
                .count()
        };
        let states = |root: &Path| {
            DiskSpecState::new(root)
                .log("epico")
                .expect("the event file")
                .events
                .iter()
                .filter(|e| e.event_type == "state")
                .count()
        };
        let before_states = states(root);

        // Sem vermelho não há porta: o verde do servidor deixa a spec fechada
        // como ela estava, e nada é empurrado (o `push` deste ajudante grita).
        let green = reopen_checked(root, "epico", "Quero consertar.", true, &|_, _| Ok(PrChecks::Passed));
        assert_eq!(green["reason"], json!("fix-not-red"), "{green}");
        assert_eq!(states(root), before_states, "nothing was written: {green}");

        // O vermelho relatado abre a onda de conserto dentro da spec fechada.
        let red: Checks = &|_, number| {
            assert_eq!(number, 1, "the pull request the spec recorded");
            Ok(PrChecks::Failed)
        };
        let opened = reopen_checked(root, "epico", "O servidor reprovou dois testes.", true, red);
        assert_eq!(opened["ok"], json!(true), "{opened}");
        assert_eq!(opened["action"], json!("fix"), "{opened}");
        assert_eq!(opened["phase"], json!("pr_open"), "the work was not reopened: {opened}");
        assert_eq!(opened["pr"], json!(1), "{opened}");
        assert_eq!(opened["checks"], json!("failed"), "{opened}");
        assert_eq!(opened["recorded"], json!(true), "{opened}");
        let wave = opened["wave"].as_u64().unwrap_or_else(|| panic!("{opened}"));
        assert!(opened["next"].as_str().unwrap().contains("round --spec epico"), "{opened}");
        assert_eq!(phase_of(root, "epico"), Some("pr_open"), "the phase never moved");
        assert_eq!(states(root), before_states, "no state event: the work was not reopened");
        assert_eq!(specs(), 1, "no new spec was born");

        // A onda de conserto e a tarefa dela estão na spec, com o vermelho
        // relatado: é o que a rodada despacha.
        let log = DiskSpecState::new(root).log("epico").expect("the event file");
        assert!(log.planned_waves().contains(&wave), "the fix wave is in the plan");
        let recorded = log.get(opened["id"].as_u64().unwrap_or_else(|| panic!("{opened}"))).expect("the fix wave");
        assert_eq!(recorded.event_type, "wave");
        assert_eq!(recorded.ints("criteria"), vec![criterion], "the fix wave answers for the work's criteria");
        let task = log
            .visible()
            .into_iter()
            .find(|e| e.event_type == "task" && e.wave() == Some(wave))
            .unwrap_or_else(|| panic!("the fix wave has no task, and the round never sends one"));
        assert!(task.str_field("text").unwrap().contains("O servidor reprovou dois testes."), "{:?}", task.fields);

        // Chamada de novo antes da entrega: nada é gravado e o passo é o mesmo.
        let before = std::fs::read(&spec_file).unwrap();
        let again = reopen_checked(root, "epico", "O servidor reprovou dois testes.", true, red);
        assert_eq!(again["recorded"], json!(false), "{again}");
        assert_eq!(again["wave"], json!(wave), "{again}");
        assert_eq!(std::fs::read(&spec_file).unwrap(), before, "nothing was written twice");

        // A rodada entrega e comita a onda de conserto na mesma branch; a
        // porta então empurra essa branch, e só ela.
        round_delivers(root, "epico", wave);
        let pushed: std::cell::RefCell<Vec<String>> = std::cell::RefCell::new(Vec::new());
        let out = reopen_with(
            &ReopenOpts {
                root: root.to_path_buf(),
                spec: Some("epico".into()),
                reason: "O servidor reprovou dois testes.".into(),
                fix: true,
            },
            None,
            red,
            &|_, branch| {
                pushed.borrow_mut().push(branch.to_string());
                Ok(())
            },
        );
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["action"], json!("fix-pushed"), "{out}");
        assert_eq!(out["branch"], json!("feature/epico"), "{out}");
        assert_eq!(pushed.borrow().as_slice(), ["feature/epico"], "the same branch the spec lives on, once");
        assert_eq!(phase_of(root, "epico"), Some("pr_open"), "the work was not reopened");
        assert_eq!(states(root), before_states, "no state event in the whole repair");
        assert_eq!(specs(), 1, "no new spec was born");

        // O que foi consertado ficou gravado na spec.
        let note = DiskSpecState::new(root)
            .log("epico")
            .expect("the event file")
            .get(out["id"].as_u64().unwrap_or_else(|| panic!("{out}")))
            .expect("the record of the repair")
            .clone();
        assert_eq!(note.event_type, "note");
        let text = note.str_field("text").unwrap_or_default();
        assert!(text.contains(&wave.to_string()) && text.contains("feature/epico"), "{text}");

        // Um vermelho novo depois do empurrão abre outra onda de conserto: a
        // porta não fica presa na anterior.
        let next = reopen_checked(root, "epico", "O servidor reprovou de novo.", true, red);
        assert_eq!(next["recorded"], json!(true), "{next}");
        assert_eq!(next["wave"], json!(wave + 1), "{next}");
    }

    /// Depois da volta, o `grill` roda de novo na mesma spec: é a recusa que
    /// ele dava fora do levantamento que a volta desfaz.
    #[test]
    fn after_the_return_the_survey_runs_again() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        spec_in(root, "epico", "running");
        let said = seed_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: Some("epico".into()),
            event_type: "message".into(),
            json: json!({"author": "user", "text": "Travar o merge."}).to_string(),
        });
        let said = said["id"].as_u64().unwrap_or_else(|| panic!("{said}"));

        let refused = crate::commands::flow::grill::grill_for(
            &crate::commands::flow::grill::GrillOpts {
                root: root.to_path_buf(),
                spec: Some("epico".into()),
                kinds: Some("fix".into()),
                condensed: false,
            },
            None,
        );
        assert_eq!(refused["reason"], json!("not-in-survey"), "{refused}");

        assert_eq!(reopen(root, "epico", "Faltou levantar.")["ok"], json!(true));
        let goal = seed_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: Some("epico".into()),
            event_type: "context".into(),
            json: json!({"text": "Travar o merge.", "origin": said}).to_string(),
        });
        assert_eq!(goal["ok"], json!(true), "{goal}");
        let after = crate::commands::flow::grill::grill_for(
            &crate::commands::flow::grill::GrillOpts {
                root: root.to_path_buf(),
                spec: Some("epico".into()),
                kinds: Some("fix".into()),
                condensed: false,
            },
            None,
        );
        assert_eq!(after["ok"], json!(true), "the survey runs again: {after}");
    }
}
