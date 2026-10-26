//! `mustard-rt run resume [--spec <nome>]` — a retomada de uma spec.
//!
//! Lê o arquivo de eventos da spec e responde a fase em que ela está, a
//! branch e a base, o próximo passo em palavras e como comando, a aprovação
//! do objetivo e o que aconteceu desde a última mensagem do usuário: as ondas
//! aceitas, os commits, os despachos e o que está à espera. Com o pull request
//! aberto, ela traz também a esteira dele, lida na hora. É essa resposta
//! que conduz a conversa de volta ao ponto em que ela parou. Nenhum endereço
//! de página entra na resposta: o link mora na barra de status.
//!
//! É também o que o `/mustard:continue` chama — o botão de reserva, porque a
//! retomada já acontece sozinha no início da sessão: a linha de retomada
//! ([`resume_line`]) — a spec, a fase, o último passo e o próximo item — sai
//! igual nos dois lugares.
//!
//! O bloco de retomada ([`resume_block`]), mais largo que a linha, também
//! mora aqui, montado num lugar só para os dois momentos da compactação: o
//! aviso antes dela e o início da sessão depois do resumo.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::{Block, BlockQuery, Refusal, SpecEvent, SpecLog};
use mustard_core::domain::spec_state::{SpecState, State};
use mustard_core::domain::survey;
use mustard_core::io::spec_events as store;
use mustard_core::io::spec_index::later;
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Value, json};

use crate::commands::maint::work_copies;
use crate::commands::spec_events;
use crate::commands::spec_events::conversation::observed;
use crate::shared::spec_state::{DiskSpecState, checkout, session_from_env};

/// As opções de `mustard-rt run resume`.
pub struct ResumeOpts {
    /// Qualquer pasta dentro do repositório.
    pub root: PathBuf,
    /// A spec retomada; sem ela, a spec atual.
    pub spec: Option<String>,
}

/// O núcleo testável de [`run_cmd`]. A sessão vem do ambiente, e a conversa
/// dela fica ligada à spec retomada: depois do `/clear`, a sessão nova volta
/// a gravar na spec por aqui. Nunca entra em pânico.
pub(crate) fn resume_at(opts: &ResumeOpts) -> Value {
    let session = session_from_env();
    let report = resume_for(opts, session.as_deref());
    if let Some(spec) = report["spec"].as_str().filter(|_| report["ok"] == json!(true)) {
        spec_events::session_spec::bind(&opts.root, session.as_deref(), spec);
    }
    report
}

/// [`resume_at`] com a sessão recebida, que é como um teste a escolhe.
pub(crate) fn resume_for(opts: &ResumeOpts, session: Option<&str>) -> Value {
    let project = spec_events::project(&opts.root);
    let lang = project.lang;
    let refuse = |refusal: &Refusal| spec_events::refused(refusal, lang);

    let spec = match opts.spec.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(spec) => spec.to_string(),
        // Sem spec nenhuma, a retomada não é erro: o mapa da sessão manda
        // rodá-la sempre, e o próximo passo do projeto que ainda não começou
        // obra é abrir a spec. Sem fase, não há comando de fase.
        None => match DiskSpecState::new(&checkout(&opts.root)).active(session) {
            Some(spec) => spec,
            None => return json!({ "ok": true, "next": translate("resume.no_spec", lang) }),
        },
    };
    if let Err(refusal) = mustard_core::io::legacy_specs::refuse_legacy(&project.root, &spec) {
        return refuse(&refusal);
    }
    let path = match store::spec_file(&project.root, &spec) {
        Ok(path) => path,
        Err(refusal) => return refuse(&refusal),
    };
    let log: SpecLog = match store::read(&path) {
        Ok(Some(log)) => log,
        Ok(None) => return refuse(&Refusal::NoSpecFile { spec }),
        Err(refusal) => return refuse(&refusal),
    };

    let state = State::from_log(&log);
    let phase = state.phase.unwrap_or("survey");
    let observed = observed(&project.root, &spec, &log);
    let mut out = json!({
        "ok": true,
        "spec": spec,
        "phase": phase,
        "line": resume_line(&spec, &log, &observed, lang),
        // A dica do plano manda fazer a pergunta de aprovação com o texto
        // exato do catálogo, o único que a testemunha da aprovação reconhece.
        "next": translate(next_key(phase), lang)
            .replace("{question}", translate("approval.question", lang))
            .replace("{option}", translate("approval.option", lang)),
        "command": next_command_now(&project.root, phase, &spec, &state, &log),
    });
    if let Some(branch) = state.branch.as_deref() {
        out["branch"] = json!(branch);
    }
    if let Some(base) = state.base.as_deref() {
        out["base"] = json!(base);
    }
    // A retomada é onde o usuário volta ao projeto, e é aqui que ele precisa
    // saber de duas coisas que nenhum passo do fluxo diz: a obra que está em
    // execução sobre uma branch que já não existe, e as cópias de obra que
    // sobraram no disco. A primeira não tem como ser retomada; as segundas são
    // peso parado, e a limpeza tem o que tirar.
    // O fechamento que caiu, sem mais nada a rodar: a retomada diz o motivo e
    // as saídas. Sem isto ela mandava rodar outra rodada, que sairia sem nada
    // para enviar, e o motivo do fechamento não chegava a ninguém.
    if let Some(said) = refused_close(&project.root, &spec, &log, lang) {
        out["close_refused"] = json!(said);
        out["next"] = json!(said);
    }
    // Com o pull request aberto, o passo que falta depende da esteira dele: a
    // retomada a lê agora, pela mesma leitura do merge, e diz o que ela pede.
    if phase == "pr_open"
        && let Some(checks) = crate::commands::review::pr_checks::of_spec(&project.root, &spec, lang)
    {
        out["checks"] = json!(checks);
    }
    if let Some(branch) = work_copies::lost_branch(&project.root, &state) {
        out["branch_missing"] = json!(translate("copies.branch_missing", lang).replace("{branch}", &branch));
    }
    let orphans = work_copies::orphans(&project.root);
    if let Some(hint) = work_copies::orphans_hint(&orphans, lang) {
        out["copies_left"] = json!({"hint": hint, "copies": orphans});
    }
    out["goal_approval"] = super::goal::status(&log, lang);
    out["since_last_message"] = since_last_message(&project.root, &spec, &log);
    out
}

/// O motivo do último fechamento que caiu, e as saídas, quando o próximo
/// passo desta spec já é fechar: a retomada o diz em vez de mandar rodar
/// outra rodada. `None` quando o último fechamento não caiu, quando nenhum
/// foi tentado, ou quando ainda há onda ou tarefa a rodar.
///
/// O motivo vem da marca que o próprio fechamento grava quando recusa
/// ([`super::close::REFUSED_STAGE`]). A retomada não repete a mensagem
/// inteira: ela manda rodar o fechamento de novo, que a diz com a saída do
/// comando que caiu.
fn refused_close(root: &Path, spec: &str, log: &SpecLog, lang: Locale) -> Option<String> {
    let state = State::from_log(log);
    let phase = state.phase.unwrap_or("survey");
    let (step, _) = step_of(root, phase, spec, log, lang);
    if step != "close" {
        return None;
    }
    let last = log
        .events
        .iter()
        .rfind(|event| event.event_type == "stage_run" && event.str_field("stage") == Some(super::close::REFUSED_STAGE))?;
    // A marca velha não vale: um fechamento depois dela, que pediu a revisão
    // ou fechou, apaga o que a de antes dizia.
    let newer = log.events.iter().any(|event| {
        event.id > last.id
            && event.event_type == "stage_run"
            && event.str_field("phase") == Some("final")
            && event.str_field("stage") == Some("final-validation")
    });
    if newer {
        return None;
    }
    let reason = last.str_field("output")?.to_string();
    Some(translate("resume.close_refused", lang).replace("{reason}", &reason))
}

/// Quantos caracteres do sha cada commit mostra na retomada: o tamanho curto
/// que o git usa por padrão.
const SHORT_SHA: usize = 7;

/// O que aconteceu desde a última mensagem do usuário, para quem volta depois
/// de limpar a conversa saber o que a rodada fez sem precisar perguntar: as
/// ondas que ela aceitou, os commits pelo sha curto e pelo título, e as ondas
/// que ela despachou, cada uma com as tarefas que levou. Junto vem o que está à
/// espera agora, venha de quando vier: as voltas gravadas que a rodada ainda
/// não assumiu, cada uma com os minutos de espera, e as ondas no ar, cada uma
/// dizendo se tem agente vivo e se perdeu o processo que a enviou. Sem
/// mensagem do usuário, conta desde o início da spec.
fn since_last_message(root: &Path, spec: &str, log: &SpecLog) -> Value {
    use crate::commands::flow::round::{minutes_since, queue, waves_in_progress};

    let codes = log.codes();
    let code_of = |id: u64| codes.get(&id).cloned().unwrap_or_else(|| id.to_string());
    let message =
        log.visible().into_iter().filter(|e| e.event_type == "message" && e.str_field("author") == Some("user")).map(|e| e.id).max();
    let since = message.unwrap_or(0);

    let accepted: BTreeSet<u64> = log
        .block(BlockQuery::Block(Block::Waves))
        .into_iter()
        .filter(|e| e.event_type == "delivered" && e.id > since)
        .filter_map(SpecEvent::wave)
        .collect();
    let commits: Vec<Value> = log
        .block(BlockQuery::Block(Block::Progress))
        .into_iter()
        .filter(|e| e.event_type == "commit" && e.id > since)
        .map(|e| {
            let sha = e.str_field("sha").unwrap_or_default();
            json!({ "sha": sha.get(..SHORT_SHA).unwrap_or(sha), "title": e.str_field("title").unwrap_or_default() })
        })
        .collect();
    // A versão do envio que só soma o consumo some da leitura e não despacha
    // nada de novo: conta o envio que é a própria posição do despacho.
    let dispatched: Vec<Value> = log
        .events
        .iter()
        .filter(|e| e.event_type == "send" && e.id > since && e.str_field("role") != Some("review"))
        .filter(|e| log.dispatch_position(e.id) == e.id)
        .filter_map(|e| {
            let tasks: Vec<String> = e
                .ints("items")
                .into_iter()
                .filter(|id| log.get(*id).is_some_and(|item| item.event_type == "task"))
                .map(code_of)
                .collect();
            Some(json!({ "wave": e.wave()?, "tasks": tasks }))
        })
        .collect();

    // As voltas à espera são as que a rodada assume, pela mesma leitura.
    let returns = log.returns_awaiting_round();
    let returned: BTreeSet<u64> = returns.iter().filter_map(|e| e.wave()).collect();
    let awaiting: Vec<Value> = returns
        .iter()
        .filter_map(|e| {
            let mut entry = json!({ "wave": e.wave()? });
            if let Some(minutes) = minutes_since(log, e.id) {
                entry["minutes"] = json!(minutes);
            }
            Some(entry)
        })
        .collect();

    let alive = queue::waves_with_live_agent(root, spec, log);
    let mut in_flight: BTreeMap<u64, Value> = BTreeMap::new();
    for wave in waves_in_progress(log).into_keys().filter(|n| !returned.contains(n)) {
        in_flight.insert(wave, json!({ "wave": wave, "agent_alive": alive.contains_key(&wave), "orphaned": false }));
    }
    for wave in queue::orphaned_waves(log).into_keys().filter(|n| !returned.contains(n)) {
        in_flight.insert(wave, json!({ "wave": wave, "agent_alive": false, "orphaned": true }));
    }

    json!({
        "after": message.map(code_of),
        "accepted": accepted,
        "commits": commits,
        "dispatched": dispatched,
        "awaiting_review": awaiting,
        "in_flight": in_flight.into_values().collect::<Vec<_>>(),
    })
}

/// A linha de retomada da spec `spec`: a spec, a fase, o último passo do
/// fluxo, tirado do que o programa observou ([`observed`]), e o próximo item.
/// É o que o início da sessão coloca depois de `/clear`, e o que o `resume`
/// devolve em `line`.
pub(crate) fn resume_line(spec: &str, log: &SpecLog, observed: &[SpecEvent], lang: Locale) -> String {
    let phase = State::from_log(log).phase.unwrap_or("survey");
    let none = translate("resume.none", lang);
    translate("resume.line", lang)
        .replace("{spec}", spec)
        .replace("{phase}", phase)
        .replace("{last}", &last_step(observed).unwrap_or_else(|| none.to_string()))
        .replace("{next}", &next_item(spec, phase, log, lang).unwrap_or_else(|| none.to_string()))
}

/// A linha de retomada da spec atual da sessão `session`, vista de `root`.
/// `None` sem spec atual, sem arquivo de eventos e na spec que já terminou,
/// que não tem para onde voltar.
pub(crate) fn current_line(root: &Path, session: Option<&str>) -> Option<String> {
    let (spec, log, lang, observed) = current_log(root, session)?;
    Some(resume_line(&spec, &log, &observed, lang))
}

/// A linha de retomada da spec atual, como [`current_line`], e as ondas com
/// agente vivo ([`waves_with_live_agent`](crate::commands::flow::round::queue::waves_with_live_agent)),
/// numa leitura só da spec: o aviso de tamanho de quem conduz manda esperar a
/// volta delas antes de limpar a conversa.
pub(crate) fn current_line_and_live(root: &Path, session: Option<&str>) -> Option<(String, Vec<u64>)> {
    let (spec, log, lang, observed) = current_log(root, session)?;
    let live = crate::commands::flow::round::queue::waves_with_live_agent(&spec_events::project(root).root, &spec, &log);
    Some((resume_line(&spec, &log, &observed, lang), live.into_keys().collect()))
}

/// O que a retomada depois de `/clear` diz das ondas cujo agente a limpeza
/// parou: as que tinham agente vivo quando a conversa foi limpa, e o que
/// fazer com cada uma, mandar a linha `PAUSED` dela na próxima rodada, que a
/// reenvia na mesma cópia. `None` sem spec atual ou sem onda nesse estado.
pub(crate) fn lost_by_clear(root: &Path, session: Option<&str>) -> Option<String> {
    let (spec, log, lang, _) = current_log(root, session)?;
    let live = crate::commands::flow::round::queue::waves_with_live_agent(&spec_events::project(root).root, &spec, &log);
    if live.is_empty() {
        return None;
    }
    let waves: Vec<String> = live.keys().map(u64::to_string).collect();
    let paused: Vec<String> = live.keys().map(|wave| format!("`<PAUSED>{{\"wave\":{wave}}}</PAUSED>`")).collect();
    Some(translate("resume.lost_by_clear", lang).replace("{waves}", &waves.join(", ")).replace("{paused}", &paused.join(" ")))
}

/// O bloco de retomada da spec atual da sessão `session`, vista de `root`,
/// com os mesmos `None` de [`current_line`]: cabe no teto do início da sessão
/// sozinho.
pub(crate) fn current_block(root: &Path, session: Option<&str>) -> Option<String> {
    current_block_within(root, session, crate::hooks::session::session_start_inject::MAX_BYTES)
}

/// [`current_block`] encolhido para caber em `cap` bytes, quando algo mais
/// precisa do mesmo teto: as listas cedem na ordem de [`fit`]. Se nem o bloco
/// com todas as listas vazias cabe, ele volta nesse tamanho mínimo.
pub(crate) fn current_block_within(root: &Path, session: Option<&str>, cap: usize) -> Option<String> {
    let (spec, log, lang, observed) = current_log(root, session)?;
    Some(resume_block(&spec, &log, &observed, lang, cap))
}

/// A spec atual da sessão `session`, o arquivo de eventos dela, o idioma do
/// projeto e o que o programa observou dela ([`observed`]). `None` sem spec
/// atual, na spec de antes desta versão do Mustard, sem arquivo de eventos e
/// na spec que já terminou.
fn current_log(root: &Path, session: Option<&str>) -> Option<(String, SpecLog, Locale, Vec<SpecEvent>)> {
    let project = spec_events::project(root);
    let spec = DiskSpecState::new(&checkout(root)).active(session).filter(|spec| !mustard_core::io::legacy_specs::is_legacy(&project.root, spec))?;
    let log = store::read(&store::spec_file(&project.root, &spec).ok()?).ok()??;
    let phase = State::from_log(&log).phase;
    if matches!(phase, Some("delivered" | "discarded")) {
        return None;
    }
    let observed = observed(&project.root, &spec, &log);
    Some((spec, log, project.lang, observed))
}

/// Os tipos cujo código o bloco de retomada lista quando foram gravados depois
/// da última rodada que deu certo: o que a conversa decidiu e a rodada ainda
/// não levou às ondas.
const RECORDED_KINDS: &[&str] = &["decision", "rule", "limit", "request", "criterion", "task"];

/// O bloco de retomada da spec `spec`: a spec e a fase; quantas ondas foram
/// entregues, só a conta, porque o número de cada uma não muda o passo
/// seguinte e cresce com a obra; cada onda em andamento com a pasta da cópia dela; as ondas cuja volta está
/// gravada e espera a rodada, dizendo qual troca uma decisão do usuário ainda
/// sem o clique dele; as paradas no limite de consertos; as que faltam; o
/// código de cada item gravado depois da última rodada; e o próximo comando.
/// Cabe em `cap` bytes: quando passa, as listas encolhem na ordem de [`fit`] e
/// cada uma diz quantos ficaram de fora.
pub(crate) fn resume_block(spec: &str, log: &SpecLog, observed: &[SpecEvent], lang: Locale, cap: usize) -> String {
    use crate::commands::flow::round::{
        change_accepted, replan_code, swaps_decision, waves_in_progress, waves_stuck,
    };

    let state = State::from_log(log);
    let phase = state.phase.unwrap_or("survey");
    let none = translate("resume.none", lang);

    // As mesmas contas da barra de status e do painel.
    let states = crate::commands::panel::progress::wave_states(log);
    let delivered = states.values().filter(|state| state.finished()).count();
    let returns = log.returns_awaiting_round();
    let returned: BTreeSet<u64> = returns.iter().filter_map(|e| e.wave()).collect();
    let running: BTreeSet<u64> =
        waves_in_progress(log).into_keys().filter(|n| !returned.contains(n)).collect();
    let stuck: Vec<String> = waves_stuck(log).into_keys().map(|n| n.to_string()).collect();
    let missing: Vec<String> = crate::commands::panel::progress::missing_waves(&states, &running, &returned)
        .into_iter()
        .map(|n| n.to_string())
        .collect();
    let in_flight: Vec<String> = running
        .iter()
        .map(|n| match mustard_core::io::wave_prompt::recorded_copy(log, *n) {
            Some(copy) => translate("conversation_size.copy", lang)
                .replace("{wave}", &n.to_string())
                .replace("{copy}", &copy.path),
            None => n.to_string(),
        })
        .collect();
    let waiting: Vec<String> = returns
        .iter()
        .filter_map(|e| {
            let wave = e.wave()?;
            let asks = e.str_field("replan").is_some_and(|change| swaps_decision(e) && !change_accepted(log, wave, &replan_code(wave, change)));
            Some(if asks {
                translate("conversation_size.replan", lang).replace("{wave}", &wave.to_string())
            } else {
                wave.to_string()
            })
        })
        .collect();
    let command = next_command(phase, spec, &state).as_str().map_or_else(|| none.to_string(), str::to_string);
    let next = translate(next_key(phase), lang)
        .replace("{question}", translate("approval.question", lang))
        .replace("{option}", translate("approval.option", lang));
    let text = translate("conversation_size.block", lang)
        .replace("{spec}", spec)
        .replace("{phase}", phase)
        .replace("{command}", &command)
        .replace("{delivered}", &delivered.to_string())
        .replace("{next}", &next);
    let lists = [
        ("{recorded}", recorded_since_round(log, observed)),
        ("{missing}", missing),
        ("{stuck}", stuck),
        ("{returned}", waiting),
        ("{running}", in_flight),
    ];
    fit(&text, &lists, lang, cap)
}

/// O código de cada decisão, regra, limite, pedido, critério e tarefa
/// gravado depois da última chamada da rodada que deu certo, na ordem do
/// arquivo. A chamada é das que o programa observou (`observed`), e mora no
/// arquivo de operações ou, na spec antiga, na própria spec: o que veio
/// depois dela se diz pela hora.
fn recorded_since_round(log: &SpecLog, observed: &[SpecEvent]) -> Vec<String> {
    let since = observed
        .iter()
        .rfind(|e| e.event_type == "call" && e.str_field("command") == Some("round") && e.str_field("result") == Some("ok"))
        .map(SpecEvent::at);
    let codes = log.codes();
    log.visible()
        .into_iter()
        .filter(|e| since.is_none_or(|round| later(e.at(), round)) && RECORDED_KINDS.contains(&e.event_type.as_str()))
        .map(|e| codes.get(&e.id).cloned().unwrap_or_else(|| e.id.to_string()))
        .collect()
}

/// O bloco `text` com cada lista de `lists` no lugar da vaga dela: inteiras,
/// quando cabem em `cap` bytes. Senão, as listas cedem na ordem
/// em que vêm — os códigos gravados primeiro, as ondas em andamento por
/// último —, cada uma mostrando só os primeiros itens e quantos ficaram de
/// fora, até o bloco caber; a lista seguinte só encolhe quando a anterior já
/// não mostra item nenhum.
fn fit(text: &str, lists: &[(&str, Vec<String>)], lang: Locale, cap: usize) -> String {
    let render = |kept: &[usize]| {
        lists.iter().zip(kept).fold(text.to_string(), |block, ((slot, items), &kept)| {
            let mut shown: Vec<String> = items[..kept].to_vec();
            if kept < items.len() {
                shown.push(translate("conversation_size.more", lang).replace("{count}", &(items.len() - kept).to_string()));
            }
            let list = if shown.is_empty() { translate("resume.none", lang).to_string() } else { shown.join(", ") };
            block.replace(slot, &list)
        })
    };
    let mut kept: Vec<usize> = lists.iter().map(|(_, items)| items.len()).collect();
    for at in 0..lists.len() {
        while kept[at] > 0 && render(&kept).len() > cap {
            kept[at] -= 1;
        }
    }
    render(&kept)
}

/// O último passo do fluxo: o comando da chamada mais nova que deu certo
/// entre os passos do fluxo ([`STEPS`](super::STEPS)), menos a retomada, que
/// só lê. A procura, a leitura, as buscas e o mapa nunca são passo.
fn last_step(observed: &[SpecEvent]) -> Option<String> {
    observed
        .iter()
        .filter(|e| e.event_type == "call" && e.str_field("result") == Some("ok"))
        .filter_map(|e| e.str_field("command"))
        .map(str::trim)
        .rfind(|command| *command != "resume" && super::STEPS.contains(command))
        .map(str::to_string)
}

/// O próximo item, por fase: no levantamento, o próximo ponto em aberto; na
/// execução, a onda em andamento ou a próxima; nas outras fases, o comando do
/// passo seguinte.
fn next_item(spec: &str, phase: &str, log: &SpecLog, lang: Locale) -> Option<String> {
    let wave = |n: u64| translate("resume.wave", lang).replace("{n}", &n.to_string());
    let state = State::from_log(log);
    let command = || next_command(phase, spec, &state).as_str().map(str::to_string);
    match phase {
        "survey" => {
            let codes = log.codes();
            survey::open_points(log)
                .first()
                .map(|point| codes.get(&point.id).cloned().unwrap_or_else(|| point.id.to_string()))
                .or_else(command)
        }
        "approved" | "running" => {
            // Em andamento é o que a rodada diz que está: o pedido anterior ao
            // replanejamento da onda não conta.
            let running = crate::commands::flow::round::waves_in_progress(log);
            let delivered = log.delivered_waves();
            let planned = log.planned_waves();
            let mut pending = planned.iter().copied().filter(|n| !delivered.contains(n));
            running.keys().next().copied().or_else(|| pending.next()).map(wave).or_else(command)
        }
        _ => command(),
    }
}

/// O que dizer a quem retoma, por fase.
fn next_key(phase: &str) -> &'static str {
    match phase {
        "plan" => "resume.next.plan",
        "approved" | "running" => "resume.next.running",
        "closed" => "resume.next.closed",
        "pr_open" => "resume.next.pr_open",
        "delivered" => "resume.next.delivered",
        "discarded" => "resume.next.discarded",
        _ => "resume.next.survey",
    }
}

/// O comando que cada fase manda rodar em seguida — a tabela única do próximo
/// passo.
///
/// É daqui que sai o campo `command` de toda resposta de retomada, e é ela que
/// dá chamador a cada comando do fluxo: nenhum texto precisa dizer a ordem dos
/// passos, porque cada passo responde qual é o seguinte. A fase que não aparece
/// aqui não tem próximo passo no binário.
pub const NEXT_BY_PHASE: &[(&str, &str)] = &[
    ("survey", "grill"),
    ("plan", "plan"),
    ("approved", "round"),
    ("running", "round"),
    ("closed", "pr-open"),
];

/// O comando do próximo passo, pronto para rodar, pela [`NEXT_BY_PHASE`]. A
/// fase que não tem próximo passo no binário não devolve comando nenhum, e a
/// que tem também não, quando o estado não traz o que o passo exige.
///
/// É público porque a catraca da prosa confere esta instrução como confere a
/// de qualquer arquivo do produto: ela não mora em arquivo nenhum, é montada
/// aqui na hora, e um teste que copiasse o formato conferiria a cópia.
pub fn next_command(phase: &str, spec: &str, state: &State) -> Value {
    NEXT_BY_PHASE
        .iter()
        .find(|(candidate_phase, _)| *candidate_phase == phase)
        .and_then(|(_, command_name)| step_command(command_name, spec, state))
        .map_or(Value::Null, Value::from)
}

/// O próximo passo da spec para quem lê o painel: o comando de
/// [`next_command`] e a frase, no idioma `lang`, que diz em palavras do dia a
/// dia o que esse comando faz pela spec agora. `Null` na fase sem comando.
pub(crate) fn next_step(root: &Path, phase: &str, spec: &str, state: &State, log: &SpecLog, lang: Locale) -> Value {
    if next_command(phase, spec, state).is_null() {
        return Value::Null;
    }
    let (step, text) = step_of(root, phase, spec, log, lang);
    let command = step_command(step, spec, state).map_or(Value::Null, Value::from);
    json!({ "command": command, "text": text })
}

/// O comando do próximo passo da spec, lido do que o passo é e não só da fase
/// em que ela está. Na execução sem onda nem tarefa a rodar, o passo é
/// fechar, e este comando é o do fechamento; a tabela por fase devolveria o
/// da rodada, que sairia sem nada para enviar.
///
/// `Null` na fase que não tem próximo passo no binário, pela mesma tabela de
/// [`next_command`]: fora do fluxo, o passo não é do programa.
pub(crate) fn next_command_now(root: &Path, phase: &str, spec: &str, state: &State, log: &SpecLog) -> Value {
    if next_command(phase, spec, state).is_null() {
        return Value::Null;
    }
    let (step, _) = step_of(root, phase, spec, log, Locale::EnUs);
    step_command(step, spec, state).map_or(Value::Null, Value::from)
}

/// A frase de [`next_step`]. Na execução, a rodada é lida na ordem em que ela
/// mesma responde: a onda parada no limite de consertos, as que saem agora —
/// as prontas do plano, as reprovadas e as que ficaram sem agente —, as
/// tarefas prontas do backlog com vaga livre, as que estão no ar, a onda que
/// não tem como sair, o backlog preso, a revisão final aberta que ainda espera
/// veredito e, sem nada disso, o fechamento, que refaz a revisão final quando
/// a reabertura anulou a aprovação.
fn step_of(root: &Path, phase: &str, spec: &str, log: &SpecLog, lang: Locale) -> (&'static str, String) {
    use crate::commands::flow::round::queue::{backlog_ready, next_waves};
    use crate::commands::flow::round::{awaits_verdict, backlog_left, waves_stuck};
    use mustard_core::view::document::WaveState;

    let said = |key: &str| ("close", translate(key, lang).to_string());
    let round = |key: &str| ("round", translate(key, lang).to_string());
    let waves = |one: &str, many: &str, list: &[u64]| {
        let shown = list.iter().map(u64::to_string).collect::<Vec<_>>().join(", ");
        ("round", translate(if list.len() == 1 { one } else { many }, lang).replace("{waves}", &shown))
    };
    match phase {
        "survey" => return ("grill", translate("resume.step.survey", lang).to_string()),
        "plan" => return ("plan", translate("resume.step.plan", lang).to_string()),
        "closed" => return ("pr-open", translate("resume.step.closed", lang).to_string()),
        _ => {}
    }
    // A spec sem onda planejada não tem o que fechar: o passo dela é a
    // rodada, que forma as ondas das tarefas prontas do backlog. Sem esta
    // linha, a spec recém-aprovada caía nas voltas do fechamento, lá embaixo.
    if log.planned_waves().is_empty() {
        return round("resume.step.backlog");
    }
    let states = crate::commands::panel::progress::wave_states(log);
    let in_state = |wanted: WaveState| states.iter().filter(|(_, state)| **state == wanted).map(|(n, _)| *n).collect::<Vec<u64>>();
    let stuck = waves_stuck(log);
    if !stuck.is_empty() {
        return waves("resume.step.stuck_one", "resume.step.stuck_many", &stuck.keys().copied().collect::<Vec<_>>());
    }
    // As vagas pela conta única da rodada: a onda que voltou guarda a cópia
    // sem ocupar vaga.
    let vacancies = crate::commands::flow::round::vacancy::vacancies(root, spec, log);
    let mut going = match next_waves(log, &vacancies, &stuck) {
        Ok(going) => going,
        Err(cycle) => return waves("resume.step.cycle", "resume.step.cycle", &cycle),
    };
    going.extend(in_state(WaveState::NoAgent));
    going.sort_unstable();
    going.dedup();
    if !going.is_empty() {
        return waves("resume.step.dispatch_one", "resume.step.dispatch_many", &going);
    }
    if !backlog_ready(log).is_empty() && vacancies.free() > 0 {
        return round("resume.step.backlog");
    }
    let running = in_state(WaveState::Running);
    if !running.is_empty() {
        return waves("resume.step.wait_one", "resume.step.wait_many", &running);
    }
    if let Some(wave) = states.iter().find(|(_, state)| !state.finished()).map(|(n, _)| *n) {
        return ("round", translate("resume.step.missing", lang).replace("{wave}", &wave.to_string()));
    }
    if !backlog_left(log).is_empty() {
        return round("resume.step.backlog_held");
    }
    // O pedido aberto mais velho que a última mudança não espera veredito,
    // nem o que ficou para trás de um item gravado depois dele, pela mesma
    // conferência da rodada e do fechamento: o passo é o fechamento, que
    // grava o pedido novo.
    if awaits_verdict(root, spec, log, lang) {
        return ("close", translate("resume.step.review_open", lang).to_string());
    }
    let review = crate::commands::panel::progress::review(log);
    if review["valid"] == true {
        said("resume.step.close_approved")
    } else if review["reopened_at"].is_null() {
        said("resume.step.close")
    } else {
        said("resume.step.close_again")
    }
}

/// A linha inteira do passo `name` da spec `spec`, com as opções que o passo
/// exige tiradas do estado: o pull request sai da branch da spec para a base
/// dela. `None` quando o estado não traz o que o passo exige — uma linha sem
/// uma opção obrigatória é recusada pelo binário antes de fazer qualquer coisa.
pub fn step_command(name: &str, spec: &str, state: &State) -> Option<String> {
    match name {
        "round" => Some(format!("mustard-rt run round --dispatch --spec {spec}")),
        "pr-open" => {
            let base = state.base.as_deref().map(str::trim).filter(|b| !b.is_empty())?;
            let head = state.branch.as_deref().map(str::trim).filter(|b| !b.is_empty())?;
            Some(format!("mustard-rt run pr-open --base {base} --head {head} --spec {spec}"))
        }
        _ => Some(format!("mustard-rt run {name} --spec {spec}")),
    }
}

/// A linha inteira que uma resposta manda rodar passa pelo parser do binário,
/// com todas as opções que o comando exige.
#[cfg(test)]
pub(crate) fn assert_parses(line: &str) {
    use clap::Subcommand;
    let mut tree = crate::commands::RunCmd::augment_subcommands(clap::Command::new("run"));
    let argv: Vec<&str> = line.split_whitespace().collect();
    assert_eq!(argv.first(), Some(&"mustard-rt"), "{line}");
    if let Err(error) = tree.try_get_matches_from_mut(&argv[1..]) {
        panic!("o binário recusa `{line}`: {error}");
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::spec_events::write::record_open;
    use mustard_core::platform::i18n::Locale;
    use std::path::Path;
    use tempfile::tempdir;

    fn resume(root: &Path, spec: &str) -> Value {
        resume_for(&ResumeOpts { root: root.to_path_buf(), spec: Some(spec.to_string()) }, None)
    }

    /// A spec da sessão que é de antes desta versão do Mustard não se retoma:
    /// a retomada recusa com o código próprio, pelo nome e pela branch, e o
    /// início da sessão não traz a linha nem o bloco dela.
    #[test]
    fn resuming_a_legacy_spec_is_refused() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        assert_eq!(record_open(root, "x", "feature/x", "dev"), Ok(true));
        crate::shared::spec_state::stand_on_spec_branch(root, "x");
        mustard_core::io::legacy_specs::mark_once(root, "0.2.24").unwrap();

        let named = resume(root, "x");
        assert_eq!(named["reason"], json!("legacy-spec"), "{named}");
        let current = resume_for(&ResumeOpts { root: root.to_path_buf(), spec: None }, None);
        assert_eq!(current["reason"], json!("legacy-spec"), "{current}");
        assert!(current_line(root, None).is_none());
        assert!(current_block(root, None).is_none());
    }

    /// A obra em execução cuja branch saiu do repositório não tem mais como
    /// ser retomada, e a retomada diz isso nomeando a branch que faltou.
    /// Junto vai a lista das cópias de obra que a limpeza tiraria, cada uma
    /// com o tamanho que ocupa — era esse estado que segurava gigabytes de
    /// cópia no disco sem ninguém saber.
    #[test]
    fn resuming_a_work_without_its_branch_says_so_and_lists_the_leftover_copies() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
                .args(args)
                .current_dir(root)
                .output()
                .expect("git");
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        git(&["init", "-q", "."]);
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "semente"]);
        crate::commands::flow::round::copies_leave_with_the_test(root);
        assert_eq!(record_open(root, "x", "feature/x", "dev"), Ok(true));
        git(&["branch", "feature/x"]);
        crate::shared::spec_state::seed_event(root, "x", "state", json!({"phase": "running", "author": "binary"}));
        let slot = mustard_core::io::wave_prompt::copies_dir(root).join("x").join("0");
        std::fs::create_dir_all(slot.join("target")).unwrap();
        std::fs::write(slot.join("target").join("compilado"), "x".repeat(4096)).unwrap();

        // Com a branch no lugar, a obra é uma obra em execução como qualquer
        // outra: nada se diz dela, e a cópia dela é trabalho de alguém.
        let working = resume(root, "x");
        assert_eq!(working["phase"], json!("running"), "{working}");
        assert!(working.get("branch_missing").is_none(), "{working}");
        assert!(working.get("copies_left").is_none(), "{working}");

        git(&["branch", "-D", "feature/x"]);
        let out = resume(root, "x");
        let missing = out["branch_missing"].as_str().unwrap_or_default();
        assert!(missing.contains("feature/x"), "a retomada nomeia a branch que faltou: {out}");
        let shown = mustard_core::io::wave_prompt::shown(&mustard_core::io::wave_prompt::copies_dir(root).join("x"));
        let hint = out["copies_left"]["hint"].as_str().unwrap_or_default();
        assert!(hint.contains(&shown), "o aviso nomeia a cópia: {out}");
        assert_eq!(out["copies_left"]["copies"][0]["path"], json!(shown), "{out}");
        assert!(out["copies_left"]["copies"][0]["bytes"].as_u64().unwrap_or_default() >= 4096, "a cópia vai medida: {out}");
    }

    /// A retomada lê só o estado e devolve, por fase, o próximo passo em
    /// palavras e o comando que o faz — e nunca o endereço da página. A linha
    /// de retomada que ela devolve é a mesma que o início da sessão traz
    /// depois de `/clear`.
    #[test]
    fn resuming_reads_the_state_and_answers_the_next_step_of_that_phase() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        assert_eq!(record_open(root, "x", "feature/x", "dev"), Ok(true));

        let out = resume(root, "x");
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["phase"], json!("survey"), "{out}");
        assert_eq!(out["branch"], json!("feature/x"), "{out}");
        assert_eq!(out["base"], json!("dev"), "{out}");
        assert_eq!(out["command"], json!("mustard-rt run grill --spec x"), "{out}");
        assert_eq!(out["next"], json!(translate("resume.next.survey", Locale::PtBr)), "{out}");
        assert!(!out.to_string().contains("http"), "nenhum endereço entra na resposta: {out}");
        // A linha roda como veio: com o objetivo gravado, o `grill` sem o tipo
        // usa o da branch e não é recusado.
        let seed = |event_type: &str, body: Value| crate::shared::spec_state::seed_event(root, "x", event_type, body);
        let said = seed("message", json!({"author": "user", "text": "Travar o merge."}));
        seed("context", json!({"text": "Travar o merge.", "origin": said}));
        let grilled = crate::commands::flow::grill::run_grill_line(root, out["command"].as_str().unwrap_or_default());
        assert_eq!(grilled["ok"], json!(true), "{grilled}");
        assert_eq!(grilled["kinds"], json!(["feature"]), "{grilled}");

        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join("x"));
        let out = resume(root, "x");
        assert_eq!(out["phase"], json!("approved"), "{out}");
        assert_eq!(out["command"], json!("mustard-rt run round --dispatch --spec x"), "{out}");
        resume_line_after_clear();
    }

    /// A linha de retomada, no início da sessão depois de `/clear` e no
    /// `resume`, numa branch com spec em execução.
    fn resume_line_after_clear() {
        // Depois de `/clear`, numa branch com spec em execução, o início da
        // sessão traz a linha de retomada — a spec, a fase, o último passo e o
        // próximo item — e o `resume` devolve a mesma linha.
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        assert_eq!(record_open(root, "x", "feature/x", "dev"), Ok(true));
        crate::shared::spec_state::stand_on_spec_branch(root, "x");
        let seed = |event_type: &str, body: Value| crate::shared::spec_state::seed_event(root, "x", event_type, body);
        let said = seed("message", json!({"author": "user", "text": "o plano"}));
        let crit = seed("criterion", json!({"when": "a", "then": "b", "proof": "p", "form": "ubiquitous", "origin": said}));
        for n in 1..=4 {
            seed("wave", json!({"n": n, "text": format!("Onda {n}."), "criteria": [crit], "done_when": "x", "origin": said}));
        }
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join("x"));
        seed("state", json!({"phase": "running", "author": "binary"}));
        // A onda 1 saiu e entregou; a onda 2 saiu e está em andamento.
        let send = |n: u64| {
            seed("send", json!({"wave": n, "role": "wave", "text": "pedido", "lines": 1, "chars": 6,
                "items": [crit], "mustard": "0", "author": "binary"}))
        };
        send(1);
        seed("delivered", json!({"wave": 1, "text": "Pronta.", "files": ["a.rs"], "author": "wave"}));
        send(2);
        seed("call", json!({"command": "round", "ms": 3, "result": "ok", "author": "binary"}));
        seed("call", json!({"command": "close", "ms": 3, "result": "refused", "author": "binary"}));

        let line = translate("resume.line", Locale::PtBr)
            .replace("{spec}", "x")
            .replace("{phase}", "running")
            .replace("{last}", "round")
            .replace("{next}", &translate("resume.wave", Locale::PtBr).replace("{n}", "2"));
        let out = resume_for(&ResumeOpts { root: root.to_path_buf(), spec: None }, Some("s-clear"));
        assert_eq!(out["line"], json!(line), "{out}");

        let started = crate::hooks::session::session_start_inject::started_after_clear(root, "s-clear");
        assert!(started.lines().any(|l| l == line), "the session start carries the resume line: {started}");
    }

    /// O último passo é o passo do fluxo mais novo que deu certo, venha ele
    /// da própria spec, como as specs antigas gravaram, ou do arquivo de
    /// operações: a procura, a leitura, a busca, o mapa e a própria retomada
    /// não são passo.
    #[test]
    fn the_last_step_is_the_newest_flow_step() {
        use crate::commands::spec_events::conversation::record_call_as;
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        assert_eq!(record_open(root, "x", "feature/x", "dev"), Ok(true));
        crate::shared::spec_state::seed_event(root, "x", "call", json!({"command": "round", "ms": 3, "result": "ok", "author": "binary"}));
        let ok = json!({"ok": true, "spec": "x"});
        for command in ["close", "pr-open", "find", "read", "search", "map search", "resume"] {
            record_call_as(root, None, command, Some("x"), std::time::Instant::now(), &ok);
        }

        let template = translate("resume.line", Locale::PtBr);
        let label = template[..template.find("{last}").unwrap()].rsplit(';').next().unwrap().to_string();
        let line = resume(root, "x")["line"].as_str().unwrap_or_default().to_string();
        assert!(line.contains(&format!("{label}pr-open;")), "{line}");
    }

    /// A rodada que deu certo marca o que veio depois dela também quando a
    /// chamada dela mora no arquivo de operações: o bloco de retomada lista
    /// o critério gravado depois da rodada e deixa de fora o de antes.
    #[test]
    fn the_round_recorded_in_operations_still_marks_what_came_after() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        assert_eq!(record_open(root, "x", "feature/x", "dev"), Ok(true));
        crate::shared::spec_state::stand_on_spec_branch(root, "x");
        let seed = |event_type: &str, body: Value| crate::shared::spec_state::seed_event(root, "x", event_type, body);
        let said = seed("message", json!({"author": "user", "text": "o plano"}));
        let criterion = |when: &str| seed("criterion", json!({"when": when, "then": "b", "proof": "p", "form": "ubiquitous", "origin": said}));
        let early = criterion("antes");
        crate::commands::spec_events::conversation::record_call_as(root, None, "round", Some("x"), std::time::Instant::now(), &json!({"ok": true, "spec": "x"}));
        let late = criterion("depois");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert!(log.visible().iter().all(|e| e.event_type != "call"), "the round call lives in the operations file");
        let codes = log.codes();
        let block = current_block(root, None).expect("the block");
        assert!(block.contains(&codes[&late]) && !block.contains(&codes[&early]), "{block}");
    }

    /// Na linha de retomada, a onda em andamento é a que a rodada diz que
    /// está: o pedido anterior ao replanejamento da onda não conta, e o
    /// próximo item é a primeira onda que falta.
    #[test]
    fn the_resume_line_does_not_count_a_send_older_than_the_replan() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        assert_eq!(record_open(root, "x", "feature/x", "dev"), Ok(true));
        let seed = |event_type: &str, body: Value| crate::shared::spec_state::seed_event(root, "x", event_type, body);
        let said = seed("message", json!({"author": "user", "text": "o plano"}));
        let crit = seed("criterion", json!({"when": "a", "then": "b", "proof": "p", "form": "ubiquitous", "origin": said}));
        let wave = |n: u64| json!({"n": n, "text": format!("Onda {n}."), "criteria": [crit], "done_when": "x", "origin": said});
        seed("wave", wave(1));
        let second = seed("wave", wave(2));
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join("x"));
        seed("state", json!({"phase": "running", "author": "binary"}));
        seed("send", json!({"wave": 2, "role": "wave", "text": "pedido", "lines": 1, "chars": 6,
            "items": [crit], "mustard": "0", "author": "binary"}));
        let mut revised = wave(2);
        revised["replaces"] = json!(second);
        seed("wave", revised);

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let line = translate("resume.line", Locale::PtBr)
            .replace("{spec}", "x")
            .replace("{phase}", "running")
            .replace("{last}", translate("resume.none", Locale::PtBr))
            .replace("{next}", &translate("resume.wave", Locale::PtBr).replace("{n}", "1"));
        assert_eq!(resume_line("x", &log, &observed(root, "x", &log), Locale::PtBr), line);
    }

    /// Depois da última mensagem do usuário, a rodada aceitou a onda 2,
    /// comitou, despachou as ondas 3 e 5, e a onda 4 voltou sem revisão. A
    /// retomada conta só o que veio depois da mensagem: a entrega, o commit e
    /// os envios de antes ficam de fora. As voltas à espera e as ondas no ar
    /// saem pelo estado de agora: a onda 3 com agente vivo, e a 5 sem ele,
    /// porque o processo que a enviou fechou.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_resume_lists_what_the_round_did_since_the_last_user_message() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        assert_eq!(record_open(root, "x", "feature/x", "dev"), Ok(true));
        let seed = |event_type: &str, body: Value| crate::shared::spec_state::seed_event(root, "x", event_type, body);
        let said = seed("message", json!({"author": "user", "text": "o plano"}));
        let crit = seed("criterion", json!({"when": "a", "then": "b", "proof": "p", "form": "ubiquitous", "origin": said}));
        let tasks: Vec<u64> = (1..=5u64)
            .map(|n| {
                seed("wave", json!({"n": n, "text": format!("Onda {n}."), "criteria": [crit], "done_when": "x", "origin": said}));
                seed("task", json!({"wave": n, "title": format!("Tarefa {n}"), "text": "Fazer.", "agent": "Fazer.",
                    "covers": [crit], "files": [{"path": format!("src/w{n}.ts")}], "origin": said}))
            })
            .collect();
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join("x"));
        seed("state", json!({"phase": "running", "author": "binary"}));
        let send = |n: u64, extra: Value| {
            let mut body = json!({"wave": n, "role": "wave", "text": "pedido", "lines": 1, "chars": 6,
                "items": [crit, tasks[usize::try_from(n).unwrap() - 1]], "mustard": "0", "author": "binary"});
            body.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap_or_default());
            seed("send", body)
        };
        let commit = |sha: &str, title: &str, wave: u64| {
            seed("commit", json!({"sha": sha, "title": title, "waves": [wave], "files": [format!("src/w{wave}.ts")], "repo": "x", "author": "binary"}))
        };
        send(1, json!({}));
        seed("delivered", json!({"wave": 1, "text": "Pronta.", "files": ["src/w1.ts"], "author": "wave"}));
        commit("1111111aaaaaaaa", "Onda 1 entra", 1);
        send(2, json!({}));
        send(4, json!({}));
        let last = seed("message", json!({"author": "user", "text": "e agora?"}));
        seed("delivered", json!({"wave": 2, "text": "Pronta.", "files": ["src/w2.ts"], "author": "wave"}));
        commit("2222222bbbbbbbb", "Onda 2 entra", 2);
        send(3, json!({"agent_launched": true}));
        seed("delivered", json!({"wave": 4, "text": "Pronta.", "files": ["src/w4.ts"], "returned": true, "author": "wave"}));
        let (dead_pid, dead_started) = crate::commands::flow::round::closed_process();
        send(5, json!({"claude_pid": dead_pid, "claude_started": dead_started}));

        let codes = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap().codes();
        let code = |id: u64| codes.get(&id).cloned().unwrap_or_else(|| id.to_string());
        let out = resume(root, "x");
        assert_eq!(
            out["since_last_message"],
            json!({
                "after": code(last),
                "accepted": [2],
                "commits": [{"sha": "2222222", "title": "Onda 2 entra"}],
                "dispatched": [{"wave": 3, "tasks": [code(tasks[2])]}, {"wave": 5, "tasks": [code(tasks[4])]}],
                "awaiting_review": [{"wave": 4, "minutes": 0}],
                "in_flight": [
                    {"wave": 3, "agent_alive": true, "orphaned": false},
                    {"wave": 5, "agent_alive": false, "orphaned": true},
                ],
            }),
            "{out}"
        );
    }

    #[test]
    fn resuming_skips_an_empty_binary_wave_as_the_round_does() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        assert_eq!(record_open(root, "x", "feature/x", "dev"), Ok(true));
        let seed = |kind: &str, fields: Value| crate::shared::spec_state::seed_event(root, "x", kind, fields);
        let said = seed("message", json!({"author":"user","text":"Concluir a tarefa."}));
        let criterion = seed("criterion", json!({"when":"a","then":"b","proof":"cargo test","form":"ubiquitous","origin":said}));
        for (n, text) in [(9, "Onda esvaziada."), (18, "Onda vigente.")] {
            seed("wave", json!({"n":n,"author":"binary","text":text,"criteria":[criterion],"done_when":"Concluir a tarefa.","origin":said}));
        }
        seed("task", json!({"wave":18,"title":"Tarefa vigente","text":"Concluir a tarefa.","agent":"Concluir a tarefa.","covers":[criterion],"files":[{"path":"src/a.rs"}],"origin":said}));
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let expected = translate("resume.wave", Locale::PtBr).replace("{n}", "18");
        assert_eq!(next_item("x", "running", &log, Locale::PtBr), Some(expected));
    }

    /// A retomada da spec fechada devolve a linha inteira do pull request,
    /// com a base e a branch tiradas do estado, e o binário a aceita. A linha
    /// de retomada traz a mesma. Sem a base no estado, não há linha: uma linha
    /// sem opção obrigatória seria recusada antes de fazer qualquer coisa.
    #[test]
    fn a_closed_spec_resumes_with_the_whole_pull_request_line() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        assert_eq!(record_open(root, "x", "feature/x", "dev"), Ok(true));
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join("x"));
        crate::shared::spec_state::seed_event(root, "x", "state", json!({"phase": "closed", "author": "binary"}));

        let out = resume(root, "x");
        let line = "mustard-rt run pr-open --base dev --head feature/x --spec x";
        assert_eq!(out["command"], json!(line), "{out}");
        assert_parses(line);
        assert!(out["line"].as_str().unwrap_or_default().contains(line), "{out}");

        let without_base = State { branch: Some("feature/x".into()), ..State::default() };
        assert_eq!(next_command("closed", "x", &without_base), Value::Null);
    }

    /// A retomada da spec no plano, a que vale depois de `/clear`, manda fazer
    /// a pergunta de aprovação com o texto exato que a testemunha da aprovação
    /// reconhece, "Aprovar esta spec?", e nenhuma vaga fica por preencher.
    #[test]
    fn resuming_in_the_plan_phase_asks_the_exact_approval_question() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), br#"{"language":{"text":"pt-BR"}}"#).unwrap();
        assert_eq!(record_open(root, "x", "feature/x", "dev"), Ok(true));
        crate::shared::spec_state::seed_event(root, "x", "state", json!({"phase": "plan", "author": "binary"}));

        let out = resume(root, "x");
        assert_eq!(out["phase"], json!("plan"), "{out}");
        let next = out["next"].as_str().unwrap_or_default();
        assert!(next.contains("com o texto exato \"Aprovar esta spec?\""), "{next}");
        assert!(next.contains("na ordem de explicar do estilo de resposta"), "{next}");
        assert!(!next.contains("{question}"), "{next}");
    }

    /// A dica de aprovar da retomada lê a opção de aprovar do catálogo, a
    /// mesma que a testemunha da aprovação reconhece, nos dois idiomas.
    #[test]
    fn the_approval_hint_reads_the_option_from_the_catalog() {
        for (lang, config) in [
            (Locale::PtBr, "{}".to_string()),
            (Locale::EnUs, r#"{"language":{"text":"en-US"}}"#.to_string()),
        ] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            std::fs::write(root.join("mustard.json"), config.as_bytes()).unwrap();
            assert_eq!(record_open(root, "x", "feature/x", "dev"), Ok(true));
            crate::shared::spec_state::seed_event(root, "x", "state", json!({"phase": "plan", "author": "binary"}));

            let out = resume(root, "x");
            let next = out["next"].as_str().unwrap_or_default();
            let option = translate("approval.option", lang);
            assert!(next.contains(&format!("\"{option}\"")), "{lang:?}: {next}");
            assert!(!next.contains("{option}"), "{lang:?}: {next}");
        }
    }

    /// A spec pedida pelo nome e que não existe é recusada, dizendo que o
    /// arquivo dela falta. Sem spec pedida nem atual, a retomada não recusa:
    /// diz para abrir uma.
    #[test]
    fn resuming_without_a_current_spec_is_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        let refused = resume(root, "nao-existe");
        assert_eq!(refused["ok"], json!(false), "{refused}");
        assert_eq!(refused["reason"], json!("no-spec-file"), "{refused}");
    }

    /// O próximo passo que o painel mostra da spec `x` gravada em `root`, na
    /// fase de execução.
    fn panel_step(root: &Path) -> Value {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        next_step(root, "running", "x", &State::from_log(&log), &log, Locale::PtBr)
    }

    /// Com a única onda entregue e comitada e a revisão final pedida depois
    /// dela, o painel manda esperar o veredito. O critério gravado depois do
    /// pedido deixa o pedido para trás: o veredito dele já não aprovaria a
    /// obra, e o painel manda fechar, que grava o pedido novo.
    #[test]
    fn a_criterion_recorded_after_the_review_request_turns_the_panel_wait_into_close() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        let folder = root.join(".claude/spec/x");
        std::fs::create_dir_all(&folder).unwrap();
        let rows = [
            json!({"v":1,"id":1,"at":"2026-10-01T10:00:00Z","type":"state","author":"binary","phase":"survey","branch":"fix/x","base":"dev"}),
            json!({"v":1,"id":2,"at":"2026-10-01T10:01:00Z","type":"message","text":"Pedido do usuário."}),
            json!({"v":1,"id":3,"at":"2026-10-01T10:02:00Z","type":"criterion","when":"Ao abrir o painel","then":"Os números batem","proof":"echo ok"}),
            json!({"v":1,"id":4,"at":"2026-10-01T10:03:00Z","type":"wave","n":1,"text":"Primeira onda.","criteria":[3],"done_when":"Conferido.","origin":2}),
            json!({"v":1,"id":5,"at":"2026-10-01T10:04:00Z","type":"task","wave":1,"title":"Primeira","text":"Fazer a primeira.","files":[{"path":"src/alpha.rs"}],"covers":[3]}),
            json!({"v":1,"id":6,"at":"2026-10-01T10:05:00Z","type":"state","author":"user","phase":"approved"}),
            json!({"v":1,"id":7,"at":"2026-10-01T10:06:00Z","type":"state","author":"binary","phase":"running"}),
            json!({"v":1,"id":8,"at":"2026-10-02T09:00:00Z","type":"send","wave":1,"role":"wave","text":"Pedido da onda 1.","agent_launched":true}),
            json!({"v":1,"id":9,"at":"2026-10-02T11:00:00Z","type":"delivered","wave":1,"text":"Feito.","files":["src/alpha.rs"],"agreed":[{"item":3,"met":true}]}),
            json!({"v":1,"id":10,"at":"2026-10-02T11:05:00Z","type":"commit","sha":"abc123","title":"Primeira","waves":[1],"files":["src/alpha.rs"],"repo":"."}),
            json!({"v":1,"id":11,"at":"2026-10-02T11:06:00Z","type":"criterion_run","criterion":3,"result":"pass","command":"echo ok"}),
            json!({"v":1,"id":12,"at":"2026-10-02T12:00:00Z","type":"send","role":"review","text":"Revise a obra."}),
        ];
        let text = rows.iter().map(Value::to_string).collect::<Vec<_>>().join("\n") + "\n";
        std::fs::write(folder.join("spec.ndjson"), &text).unwrap();

        let waiting = panel_step(root);
        assert_eq!(waiting["text"], json!(translate("resume.step.review_open", Locale::PtBr)), "o pedido de agora espera o veredito: {waiting}");

        let criterion = json!({"v":1,"id":13,"at":"2026-10-02T13:00:00Z","type":"criterion","when":"A busca roda",
            "then":"A citação sai com a pasta","proof":"git --version","origin":2});
        std::fs::write(folder.join("spec.ndjson"), format!("{text}{criterion}\n")).unwrap();
        let past = panel_step(root);
        assert_eq!(past["command"], json!("mustard-rt run close --spec x"), "o painel manda fechar: {past}");
        assert_eq!(past["text"], json!(translate("resume.step.close", Locale::PtBr)), "o painel não espera o veredito do pedido para trás: {past}");
    }
}
