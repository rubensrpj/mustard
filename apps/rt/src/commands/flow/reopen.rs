//! `mustard-rt run reopen --reason <motivo> [--spec <nome>]` — leva a spec de
//! volta ao levantamento.
//!
//! O caminho de volta. Uma spec já em plano, aprovada ou em execução volta à
//! fase de levantamento por este comando, e daí o `grill` roda de novo: os
//! pontos novos convivem com o que já foi decidido. Nada do que está gravado é
//! apagado — a volta é mais um evento no arquivo, um `state` com a fase
//! `survey`, e ele guarda quem pediu, quando e por quê.
//!
//! O motivo é obrigatório: é ele que explica, daqui a um mês, por que o
//! levantamento recomeçou — e é ele que vira a consulta. O levantamento
//! seguinte traz os itens que o motivo toca, para o usuário dizer se cada um
//! fica, muda ou sai; o que o motivo não toca fica como está, e nada é
//! perguntado de novo. Um `--reason` em branco é recusado.
//!
//! A spec fechada, com o pull request aberto, entregue ou descartada não
//! volta: o que ela decidiu já saiu, e o caminho é uma spec nova pelo `open`.
//! A spec que já está em levantamento não grava nada e responde o mesmo passo.
//!
//! ```text
//! {"ok": true, "spec": "x", "phase": "survey", "from": "running", "id": 42,
//!  "reason": "O pedido mudou de alvo.", "next": "A spec x voltou ao levantamento…"}
//! ```
//!
//! Recusa sai com exit 1 e `ok: false`, com a razão curta em `reason` e a
//! mensagem no idioma do projeto em `hint`.
//!
//! ## A porta de conserto do pull request reprovado
//!
//! Uma obra fechada abriu o pull request, e o servidor de integração rodou os
//! testes de novo e reprovou. A obra não volta ao levantamento — o que ela
//! decidiu está certo, o que está errado é o que o servidor achou — e uma spec
//! nova para um conserto de minutos é mentira. Sem caminho de volta, quem usa
//! editava arquivo na branch por fora, e o conserto não existia em lugar
//! nenhum.
//!
//! Este mesmo comando é a porta. Numa spec com o pull request aberto, ele
//! pergunta ao provedor as verificações daquele pull request
//! ([`crate::commands::review::pr_door::red_reported`], a mesma leitura do
//! portão do merge). Só o vermelho abre a porta: verde, em andamento, ausente
//! ou ilegível caem na recusa de sempre, porque sem reprovação não há o que
//! consertar.
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
//! A onda de conserto é reconhecida pelo lugar dela no arquivo: é onda gravada
//! depois do fechamento. Nenhuma marca nova, nenhuma segunda lista.
//!
//! ```text
//! {"ok": true, "spec": "x", "action": "fix", "phase": "pr_open", "pr": 12,
//!  "checks": "failed", "wave": 7, "recorded": true, "next": "…"}
//! ```

use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::{Block, BlockQuery, Refusal, SpecLog};
use mustard_core::domain::spec_state::{reopenable, PhaseWriter, SpecState, State};
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Map, Value};

use crate::commands::review::pr_door::{provider_checks, red_reported};
use crate::commands::spec_events::{self, read::checkout, write::record};
use crate::shared::pr_provider::PrChecks;
use crate::shared::spec_state::{session_from_env, DiskSpecState};

/// Como a porta pergunta ao provedor as verificações de um pull request.
/// Injetada para o teste exercitar o vermelho e o verde sem provedor, sem
/// rede e sem repositório, como o portão do merge já faz.
type Checks<'a> = &'a dyn Fn(&Path, u64) -> Result<PrChecks, String>;

/// Como a porta empurra a branch da spec para o servidor. Injetada pelo mesmo
/// motivo, e para o teste provar em que branch o conserto foi empurrado.
type Push<'a> = &'a dyn Fn(&Path, &str) -> Result<(), String>;

/// As chaves de busca da nota do conserto. Fixas nos dois idiomas: é por elas
/// que a porta sabe, na chamada seguinte, que aquele conserto já foi
/// empurrado.
const FIX_KEYS: &[&str] = &["conserto", "pull-request", "servidor"];

/// Options for `mustard-rt run reopen`.
pub struct ReopenOpts {
    /// Qualquer pasta dentro do repositório.
    pub root: PathBuf,
    /// A spec que volta ao levantamento; sem ela, a spec atual.
    pub spec: Option<String>,
    /// Por que a spec volta, palavra por palavra de quem pediu.
    pub reason: String,
}

/// Por que a volta não aconteceu.
enum ReopenRefusal {
    /// O `--reason` veio em branco.
    ReasonMissing,
    /// A spec já fechou, foi para o pull request, foi entregue ou descartada.
    Settled { spec: String, phase: String },
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

/// [`reopen_for`] com os efeitos de fora recebidos, que é como um teste os
/// escolhe.
pub(crate) fn reopen_with(opts: &ReopenOpts, session: Option<&str>, checks: Checks, push: Push) -> Value {
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
    if from == "survey" {
        return json!({
            "ok": true, "spec": spec, "phase": "survey", "from": from, "recorded": false,
            "next": say("reopen.already", lang, &spec),
        });
    }
    // A porta de conserto: a obra fechada cujo pull request o servidor
    // reprovou. Só o vermelho relatado a abre; qualquer outra resposta do
    // provedor cai na recusa de sempre, logo abaixo.
    if from == "pr_open"
        && let Ok(pr) = red_reported(&checkout(&opts.root), &spec, checks)
    {
        return fix_door(opts, &spec, &log, FixRed { pr, reason }, push, lang);
    }
    if !reopenable(from) {
        return refuse(ReopenRefusal::Settled { spec, phase: from.to_string() });
    }

    let mut draft = Map::new();
    draft.insert("phase".to_string(), json!("survey"));
    draft.insert("author".to_string(), json!("binary"));
    draft.insert("reason".to_string(), json!(reason));
    match record(&opts.root, &spec, "state", draft, PhaseWriter::Binary) {
        Ok(recorded) => json!({
            "ok": true, "spec": spec, "phase": "survey", "from": from, "recorded": true,
            "id": recorded.written.id, "reason": reason,
            "next": say("reopen.next", lang, &spec),
        }),
        Err(refusal) => refuse(ReopenRefusal::Spec(refusal)),
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
/// fechamento.
///
/// O lugar no arquivo é a marca, e é marca suficiente: numa spec fechada,
/// onda gravada depois do fechamento só nasce por esta porta. Uma marca nova
/// no evento seria uma segunda lista para o resto do binário aprender a ler.
fn fix_wave(log: &SpecLog) -> Option<FixWave> {
    let closed = State::from_log(log).last_closing?;
    let n = log
        .block(BlockQuery::Block(Block::Waves))
        .into_iter()
        .filter(|event| event.event_type == "wave" && event.id > closed)
        .max_by_key(|event| event.id)?
        .wave()?;
    Some(FixWave { n, delivered: log.last_by_wave("delivered").get(&n).copied() })
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
    log.block(BlockQuery::Block(Block::Notes)).into_iter().any(|event| {
        event.event_type == "note"
            && event.id > delivered
            && event
                .fields
                .get("keys")
                .and_then(Value::as_array)
                .is_some_and(|keys| keys.iter().any(|key| key.as_str() == Some(FIX_KEYS[0])))
    })
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
    // conserto não tem o que fazer: fica a recusa de sempre.
    let Some(branch) = State::from_log(log).branch.map(|b| b.trim().to_string()).filter(|b| !b.is_empty()) else {
        return refuse(ReopenRefusal::Settled { spec: spec.to_string(), phase: "pr_open".to_string() });
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
    use std::path::Path;
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

    /// A volta ao levantamento com um provedor que não responde — nenhum
    /// vermelho relatado, então a porta de conserto não abre — e um git que
    /// não deixa empurrar nada sem alguém pedir.
    fn reopen(root: &Path, spec: &str, reason: &str) -> Value {
        reopen_checked(root, spec, reason, &|_, _| Err("no-provider".to_string()))
    }

    /// A volta ao levantamento com as verificações do provedor escolhidas: é
    /// o vermelho delas que abre a porta de conserto.
    fn reopen_checked(root: &Path, spec: &str, reason: &str, checks: Checks) -> Value {
        reopen_with(
            &ReopenOpts { root: root.to_path_buf(), spec: Some(spec.to_string()), reason: reason.to_string() },
            None,
            checks,
            &|_, branch| panic!("nada empurra a branch {branch} neste teste"),
        )
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

    /// Uma spec fechada, com o pull request aberto ou entregue não volta, e a
    /// recusa diz a fase em que ela está; nada é gravado.
    #[test]
    fn a_settled_spec_is_refused_and_nothing_is_written() {
        for phase in ["closed", "pr_open", "delivered"] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            spec_in(root, "epico", phase);
            let before = std::fs::read(store::spec_file(root, "epico").unwrap()).unwrap();

            let out = reopen(root, "epico", "Quero levantar de novo.");
            assert_eq!(out["reason"], json!("spec-settled"), "{phase}: {out}");
            let hint = out["hint"].as_str().unwrap();
            assert!(hint.contains("epico") && hint.contains(phase), "{hint}");
            assert_eq!(std::fs::read(store::spec_file(root, "epico").unwrap()).unwrap(), before);
            assert_eq!(phase_of(root, "epico"), Some(phase));
        }
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
    /// há porta de conserto: o vermelho relatado é aceito, a onda de conserto
    /// abre dentro da spec fechada, a rodada a comita na mesma branch e este
    /// mesmo passo empurra o commit — sem reabrir a obra e sem spec nova.
    ///
    /// Sem o vermelho não há porta: com as verificações verdes a spec fechada
    /// é recusada como sempre foi, e nada é gravado nem empurrado.
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
        let green = reopen_checked(root, "epico", "Quero consertar.", &|_, _| Ok(PrChecks::Passed));
        assert_eq!(green["reason"], json!("spec-settled"), "{green}");
        assert_eq!(states(root), before_states, "nothing was written: {green}");

        // O vermelho relatado abre a onda de conserto dentro da spec fechada.
        let red: Checks = &|_, number| {
            assert_eq!(number, 1, "the pull request the spec recorded");
            Ok(PrChecks::Failed)
        };
        let opened = reopen_checked(root, "epico", "O servidor reprovou dois testes.", red);
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
        let again = reopen_checked(root, "epico", "O servidor reprovou dois testes.", red);
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
        let next = reopen_checked(root, "epico", "O servidor reprovou de novo.", red);
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
