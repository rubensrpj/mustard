//! O que para sem travar a rodada: o limite de consertos de cada onda, com a
//! pergunta ao usuário; a volta recusada por uma conferência dela, ou
//! reprovada por quem conduz a obra, que segura só a própria onda; e o plano
//! que muda — a onda replanejada depois do
//! pedido e a mudança de plano que um agente propõe. Toda mudança do plano
//! aprovado espera o clique do usuário e segura só a onda dela. As tarefas
//! que a onda não fez voltam ao backlog quando a rodada assume a volta, menos
//! a que outra onda já levou, que fica com ela. Uma tarefa devolvida por
//! impedimento permanece bloqueada até ser revisada. A mudança de plano trava
//! só a tarefa que ela cita, e a rodada seguinte ao aceite a solta. A mudança
//! recusada — pelo clique em "Recusar" ou pela linha de quem conduz — não
//! segura nada: a onda entra no commit e as tarefas voltam soltas.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mustard_core::domain::spec_events::{Block, BlockQuery, SpecEvent, SpecLog};
use mustard_core::domain::spec_state::PhaseWriter;
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Map, Value, json};

use super::answer::RoundRefusal;
use super::commit::git_lock;
use super::jev_decision::{JevOn, Judged, Question, Waiting, decided};
use super::report::{PlanChange, PlanRefusal, WaveReport, backlog_return};
use super::sent_tasks::sent_tasks;
use crate::commands::spec_events::write::record;

/// Quantas rodadas de conserto uma onda tem. A reprovação que vem depois da
/// última delas para a onda e as que dependem dela: o problema é de desenho,
/// e vai ao usuário. O mesmo teto vale para a conferência depois da onda,
/// para a compilação, o lint e a suíte que caem antes do commit e para a
/// verificação de critério que não passa, numa conta só, por
/// [`fix_rounds_done`]: passadas todas, a recusa seguinte, venha de qual
/// delas vier, vira a mesma pergunta ao usuário.
pub(super) const MAX_FIX_ROUNDS: usize = 2;

/// Por quantas rodadas de conserto a onda `wave` de `waves` já passou: cada
/// volta que ela grava de novo desde o envio que a despachou conta uma; a
/// primeira entrega não conta. A onda fora de `waves` não passou por nenhuma.
pub(super) fn fix_rounds_done(waves: &[WaveReport], wave: u64) -> usize {
    waves.iter().find(|w| w.wave == wave).map_or(0, |w| w.returns.len().saturating_sub(1))
}

/// A etapa com que a rodada grava o falso positivo de quem conduz a obra,
/// seguida de `:` e do número da onda.
const FALSE_POSITIVE_STAGE: &str = "after-wave-false-positive";

/// A volta da onda `wave` que quem conduz a obra aceitou como falso positivo
/// da conferência depois da onda, pelo número dela; a mais nova, quando
/// houve mais de uma.
pub(super) fn false_positive_of(log: &SpecLog, wave: u64) -> Option<u64> {
    let stage = format!("{FALSE_POSITIVE_STAGE}:{wave}");
    log.events
        .iter()
        .filter(|e| e.event_type == "stage_run" && e.str_field("author") == Some("binary") && e.str_field("stage") == Some(stage.as_str()))
        .filter_map(|e| e.fields.get("attempt").and_then(Value::as_u64))
        .max()
}

/// Grava cada falso positivo que o relatório trouxe (`lines`, a onda e o
/// motivo), preso à volta pendente da onda: a conferência depois da onda
/// passa a só avisar o que achou nessa volta, e a onda entra no commit. A
/// volta nova da onda volta a ser conferida como qualquer outra. A onda sem
/// volta pendente não tem o que aceitar, e a linha dela não grava nada; o
/// mesmo falso positivo de novo também não. Devolve se gravou algum.
pub(super) fn record_false_positives(start: &Path, spec: &str, log: &SpecLog, lines: &[(u64, String)]) -> Result<bool, RoundRefusal> {
    let mut wrote = false;
    for (wave, reason) in lines {
        let Some(back) = super::rejection::pending_return(log, *wave) else { continue };
        if false_positive_of(log, *wave) == Some(back) {
            continue;
        }
        let fields = Map::from_iter([
            ("author".into(), json!("binary")),
            ("phase".into(), json!("delivery")),
            ("stage".into(), json!(format!("{FALSE_POSITIVE_STAGE}:{wave}"))),
            ("result".into(), json!("pass")),
            ("ms".into(), json!(0)),
            ("attempt".into(), json!(back)),
            ("output".into(), json!(crate::shared::secret::without_secrets(reason))),
        ]);
        record(start, spec, "stage_run", fields, PhaseWriter::Binary).map_err(RoundRefusal::Refused)?;
        wrote = true;
    }
    Ok(wrote)
}

/// As ondas paradas pelo limite de consertos, na resposta da rodada: cada uma
/// com a pergunta ao usuário e os vereditos que a pararam, por inteiro — é com
/// eles que o usuário decide —, e o texto que manda fazer cada pergunta.
pub(super) fn stopped_waves(stuck: &BTreeMap<u64, Vec<&SpecEvent>>, codes: &BTreeMap<u64, String>, lang: Locale) -> (Vec<Value>, Vec<String>) {
    let max = MAX_FIX_ROUNDS.to_string();
    stuck
        .iter()
        .map(|(wave, verdicts)| {
            let listed: Vec<(String, &str)> =
                verdicts.iter().map(|v| (codes.get(&v.id).cloned().unwrap_or_else(|| v.id.to_string()), v.str_field("text").unwrap_or_default())).collect();
            let names: Vec<&str> = listed.iter().map(|(code, _)| code.as_str()).collect();
            let asked = translate("round.fix_limit", lang)
                .replace("{wave}", &wave.to_string())
                .replace("{count}", &listed.len().to_string())
                .replace("{max}", &max)
                .replace("{verdicts}", &names.join(", "));
            let question = translate("round.fix_limit.question", lang).replace("{wave}", &wave.to_string()).replace("{max}", &max);
            let verdicts: Vec<Value> = listed.iter().map(|(code, text)| json!({ "code": code, "text": text })).collect();
            (json!({ "wave": wave, "question": question, "verdicts": verdicts }), asked)
        })
        .unzip()
}

/// O código da mudança proposta, que vai na pergunta que a decide: a onda e
/// uma chave do texto da mudança, para que um "sim" nunca sirva para outra.
pub(crate) fn replan_code(wave: u64, change: &str) -> String {
    let key = crate::commands::agent::render::prompt_ref::fnv1a64(&[change.trim()]) & 0x00ff_ffff;
    format!("onda-{wave}-{key:06x}")
}

/// A volta `event` diz que a mudança de plano dela troca uma decisão do
/// usuário: só essa mudança espera o clique.
pub(crate) fn swaps_decision(event: &SpecEvent) -> bool {
    event.str_field("changes_decision").is_some_and(|decision| !decision.trim().is_empty())
}

/// O código de mudança que `text` traz, quando traz um: a palavra com a forma
/// que [`replan_code`] escreve. É assim que a testemunha lê o código no
/// cabeçalho da pergunta, sem depender de nada do enunciado.
pub(crate) fn change_code_of(text: &str) -> Option<String> {
    text.split_whitespace().find(|word| is_change_code(word)).map(str::to_string)
}

/// `word` tem a forma de um código de mudança: `onda-<número>-<seis dígitos
/// hexadecimais minúsculos>`, como [`replan_code`] o escreve.
fn is_change_code(word: &str) -> bool {
    let Some((wave, key)) = word.strip_prefix("onda-").and_then(|rest| rest.split_once('-')) else {
        return false;
    };
    !wave.is_empty() && wave.bytes().all(|b| b.is_ascii_digit()) && key.len() == 6 && key.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A mudança de código `code`, proposta pela onda `wave`, foi aceita: o
/// clique mais novo do usuário nela, gravado pela testemunha depois do último
/// pedido da onda, é o "Aceitar". Um clique em "Recusar" depois dele desfaz o
/// "sim"; um clique de antes do pedido não vale para ele.
///
/// Quem diz qual mudança o clique decide é o código que a testemunha guardou
/// ao lado da resposta, nunca a frase mostrada ao usuário: a pergunta escrita
/// com as palavras dele vale igual, e o "sim" de uma mudança nunca serve para
/// outra.
///
/// Só conta a mensagem de autor `user` com a testemunha. O `run write` recusa
/// toda mensagem com a testemunha, de qualquer autor, e recusa rever ou tirar
/// uma delas: só a testemunha grava o clique.
pub(crate) fn change_accepted(log: &SpecLog, wave: u64, code: &str) -> bool {
    let sent = log.last_dispatch_by_wave().get(&wave).copied().unwrap_or(0);
    accepted_after(log, code, sent)
}

/// O clique mais novo do usuário na mudança de código `code`, gravado pela
/// testemunha depois do evento `after`, é o "Aceitar".
fn accepted_after(log: &SpecLog, code: &str, after: u64) -> bool {
    last_click(log, code, after).is_some_and(|(_, answer)| is_option("change.accept", answer))
}

/// O clique mais novo do usuário na mudança de código `code`, gravado pela
/// testemunha depois do evento `after`: o número da mensagem e a opção.
fn last_click<'a>(log: &'a SpecLog, code: &str, after: u64) -> Option<(u64, &'a str)> {
    log.block(BlockQuery::Block(Block::Conversation))
        .into_iter()
        .filter(|e| e.event_type == "message" && e.id > after && e.str_field("author") == Some("user"))
        .filter_map(|e| Some((e.id, e.fields.get("witness")?)))
        .filter(|(_, w)| w.get("change").and_then(Value::as_str).is_some_and(|clicked| clicked.trim() == code))
        .filter_map(|(id, w)| Some((id, w.get("answer").and_then(Value::as_str)?)))
        .next_back()
}

/// `answer` é a opção `key` do catálogo, por inteiro, num dos idiomas.
fn is_option(key: &str, answer: &str) -> bool {
    [Locale::PtBr, Locale::EnUs].iter().any(|lang| translate(key, *lang) == answer.trim())
}

/// A recusa da mudança de plano `change` da onda `wave`, lida em `log`: o
/// clique mais novo do usuário nela, depois do último pedido da onda, em
/// "Recusar", com o número da mensagem; sem clique nenhum, a linha
/// `PLAN_REFUSED` de quem conduz (`lines`), com o motivo dela e a última
/// mensagem do usuário. O clique em "Aceitar" vence a linha: só o usuário
/// muda o plano, e recusar pela linha nunca desfaz o "sim" dele.
fn refusal_of(log: &SpecLog, wave: u64, change: &str, lines: &[(u64, String)], lang: Locale) -> Option<PlanRefusal> {
    let sent = log.last_dispatch_by_wave().get(&wave).copied().unwrap_or(0);
    match last_click(log, &replan_code(wave, change), sent) {
        Some((id, answer)) if is_option("change.decline", answer) => {
            Some(PlanRefusal { reason: translate("round.plan_refused.click", lang).to_string(), origin: Some(id) })
        }
        Some(_) => None,
        None => {
            let (_, reason) = lines.iter().rfind(|(n, _)| *n == wave)?;
            let said = log.block(BlockQuery::Block(Block::Conversation)).into_iter().rfind(|e| e.event_type == "message" && e.str_field("author") == Some("user"));
            Some(PlanRefusal { reason: reason.trim().to_string(), origin: said.map(|e| e.id) })
        }
    }
}

/// Cada linha `PLAN_REFUSED` (`lines`) aponta uma onda cuja volta, entre as
/// que esperam a rodada (`waves`), propõe mudar o plano. A que aponta outra
/// onda é recusada pelo relatório, antes de qualquer gravação.
pub(super) fn plan_refusals_point_at_changes(waves: &[WaveReport], lines: &[(u64, String)]) -> Result<(), RoundRefusal> {
    match lines.iter().find(|(n, _)| !waves.iter().any(|w| w.wave == *n && w.replan.is_some())) {
        Some((wave, _)) => Err(RoundRefusal::BadReport { detail: format!("PLAN_REFUSED: onda {wave} sem mudança de plano à espera") }),
        None => Ok(()),
    }
}

/// A volta de uma onda que a rodada deixa fora do commit, com a recusa que a
/// segura: a que troca uma decisão do usuário e ainda espera o clique dele, ou a que
/// uma conferência da própria volta recusou — sem o título do commit, com
/// tarefa de outra onda em `undone`, com a mudança de plano sem `undone`, com
/// arquivo que não existe —, ou a que quem conduz a obra reprovou, à espera
/// de um agente novo. A recusa segura só a onda dela: a volta fica na
/// spec sem ninguém a assumir, a cópia fica como está, e o envio aberto
/// segura a vaga e os arquivos dela — o despacho não oferece onda que divida
/// arquivo com ela nem a que dependa dela. O resto da rodada segue. A rodada
/// seguinte a assume como qualquer outra, depois do clique ou da volta
/// gravada de novo.
pub(crate) struct HeldReturn {
    pub wave: u64,
    pub refusal: RoundRefusal,
}

impl HeldReturn {
    /// O aviso da resposta da rodada: o motivo, a onda e a mensagem da recusa,
    /// no idioma `lang`, com o cabeçalho e as opções quando é a mudança de
    /// plano, como a recusa os mostra.
    pub(super) fn warning(&self, lang: Locale) -> Value {
        let mut warning = self.refusal.to_value(lang);
        if let Some(fields) = warning.as_object_mut() {
            fields.remove("ok");
            if self.wave != super::jev_decision::FINAL_REVIEW {
                fields.insert("wave".into(), json!(self.wave));
            }
        }
        warning
    }

    /// A frase do próximo passo, no idioma `lang`: a mudança de plano já diz
    /// a onda e o que perguntar; a volta recusada diz que só ela ficou fora
    /// da rodada, com o que falta para ela entrar; o veredito final que
    /// espera a decisão do Jev já diz o que falta.
    pub(super) fn next_line(&self, lang: Locale) -> String {
        let hint = self.refusal.message(lang);
        match self.refusal {
            RoundRefusal::Replan { .. } => hint,
            _ if self.wave == super::jev_decision::FINAL_REVIEW => hint,
            _ => translate("round.held_return", lang).replace("{wave}", &self.wave.to_string()).replace("{hint}", &hint),
        }
    }
}

/// Segura a mudança do plano aprovado até o aceite do usuário. Sem o campo
/// da decisão, a mudança espera do mesmo jeito. A pergunta lista só as
/// tarefas que a mudança vai travar ([`held_by_change`]). A mudança recusada
/// ([`refusal_of`], pelo clique ou pela linha `PLAN_REFUSED` de `lines`) não
/// segura a onda: ela segue para o commit com a recusa, e a volta que mudou a
/// cópia sem o resumo do commit ganha um, no idioma `lang`, para o que a onda
/// fez entrar pelo caminho de sempre.
pub(super) fn hold_waiting_changes(log: &SpecLog, waves: &mut Vec<WaveReport>, lines: &[(u64, String)], lang: Locale) -> Vec<HeldReturn> {
    for wave in waves.iter_mut() {
        let Some(plan) = wave.replan.as_mut() else {
            continue;
        };
        plan.refused = refusal_of(log, wave.wave, &plan.change, lines, lang);
        if plan.refused.is_some() && wave.commit.is_none() {
            wave.commit = Some(translate("round.plan_refused.commit", lang).to_string());
        }
    }
    hold_refused(waves, |wave| {
        let Some(PlanChange { change, refused, .. }) = &wave.replan else {
            return Ok(());
        };
        if refused.is_some() {
            return Ok(());
        }
        let code = replan_code(wave.wave, change);
        if change_accepted(log, wave.wave, &code) {
            return Ok(());
        }
        let tasks = held_by_change(change, &wave.undone).into_iter().map(str::to_string).collect();
        Err(RoundRefusal::Replan { wave: wave.wave, code, tasks })
    })
}

/// Os códigos das tarefas de `undone` que a mudança de plano `change` trava:
/// as que ela cita pelo código; sem nenhuma citada, todas.
fn held_by_change<'a>(change: &str, undone: &'a [(u64, String)]) -> Vec<&'a str> {
    let all = undone.iter().map(|(_, code)| code.as_str());
    let cited: Vec<&str> = all.clone().filter(|code| cites(change, code)).collect();
    if cited.is_empty() { all.collect() } else { cited }
}

/// Os códigos das tarefas de `undone`, lidas em `log`, que recebem a mudança
/// de plano `change`: as que ela cita pelo código; sem citação, a que o
/// classificador das atividades escolhe entre elas, por `ask`; sem escolha
/// confiante, a primeira da lista, a que o agente parou. Nunca a onda
/// inteira: a mudança que não cita tarefa não vai a quem ela não trata.
fn change_targets<'a>(log: &SpecLog, change: &str, undone: &'a [(u64, String)], ask: &dyn Fn(&str) -> Option<Value>) -> Vec<&'a str> {
    use crate::shared::task_triage::Destination;
    let cited: Vec<&str> = undone.iter().map(|(_, code)| code.as_str()).filter(|code| cites(change, code)).collect();
    let Some(first) = undone.first().filter(|_| cited.is_empty()) else {
        return cited;
    };
    let question = change_target_question(log, change, undone);
    let read = question.and_then(|question| ask(&question.payload).map(|doc| question.read(&doc)));
    let chosen = read.into_iter().flatten().next().flatten().and_then(|destination| match destination {
        Destination::Covered { task, .. } | Destination::Join { task, .. } => Some(task),
        Destination::New { near } => near,
    });
    let target = chosen.and_then(|id| undone.iter().find(|(own, _)| *own == id)).unwrap_or(first);
    vec![target.1.as_str()]
}

/// A pergunta ao classificador das atividades sobre qual das tarefas de
/// `undone`, lidas em `log` pelo número, recebe a mudança de plano `change`
/// que não cita nenhuma; `None` quando ela cita alguma, ou sem tarefa. Só
/// depende das tarefas gravadas e da volta: a rodada que deixa a volta
/// esperando e a seguinte fazem o mesmo pedido.
pub(super) fn change_target_question(log: &SpecLog, change: &str, undone: &[(u64, String)]) -> Option<crate::shared::task_triage::Classifying> {
    use crate::shared::task_triage::{Candidate, Item, classifying};
    if undone.is_empty() || undone.iter().any(|(_, code)| cites(change, code)) {
        return None;
    }
    let candidates: Vec<Candidate> = undone
        .iter()
        .filter_map(|(id, _)| log.get(*id))
        .map(|task| Candidate {
            id: task.id,
            title: task.str_field("title").unwrap_or_default(),
            text: task.str_field("text").unwrap_or_default(),
            files: super::queue::task_files(task),
            joinable: true,
        })
        .collect();
    let item = Item { title: String::new(), text: change.trim().to_string(), files: BTreeSet::new() };
    classifying(&candidates, &[item], &[])
}

/// `text` traz `code` como palavra inteira: sem letra, algarismo, hífen ou
/// sublinhado colado antes ou depois dele.
fn cites(text: &str, code: &str) -> bool {
    let glued = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_');
    !code.is_empty() && text.match_indices(code).any(|(at, _)| !glued(text[..at].chars().next_back()) && !glued(text[at + code.len()..].chars().next()))
}

/// Solta, no começo da rodada, cada tarefa travada por uma mudança de plano
/// que o usuário já aceitou ([`accepted_holds`]): grava a versão nova dela,
/// sem a trava, com a trava do passo do git presa e a spec `spec`, em
/// `path`, relida sob ela. A trava que a rodada grava ao assumir a volta
/// aceita só sai na rodada seguinte: quem conduz tem esse intervalo para
/// reescrever a tarefa. Devolve a leitura da spec depois da soltura — a de
/// entrada (`log`), quando nada foi solto — e o que foi gravado.
pub(super) fn release_accepted_holds(root: &Path, spec: &str, path: &Path, log: SpecLog) -> Result<(SpecLog, Vec<Value>), RoundRefusal> {
    if accepted_holds(&log).is_empty() {
        return Ok((log, Vec::new()));
    }
    let _lock = git_lock(root)?;
    let read = || store::read(path).map_err(RoundRefusal::Refused).map(|fresh| fresh.unwrap_or_else(|| log.clone()));
    let fresh = read()?;
    let mut recorded = Vec::new();
    for task in accepted_holds(&fresh) {
        let mut draft = backlog_return(task);
        draft.remove("blocked_by");
        if let Some(wave) = task.fields.get("wave") {
            draft.insert("wave".into(), wave.clone());
        }
        let written = record(root, spec, "task", draft, PhaseWriter::Binary).map_err(RoundRefusal::Refused)?;
        recorded.push(json!({ "type": "task", "id": written.written.id, "released": task.id }));
    }
    if recorded.is_empty() {
        return Ok((log, recorded));
    }
    Ok((read()?, recorded))
}

/// As tarefas por entregar travadas por uma mudança de plano aceita: a trava
/// (`blocked_by`) é o texto da mudança (`replan`) de uma entrega da onda, e o
/// clique mais novo do usuário no código dela ([`replan_code`]) é o
/// "Aceitar". Vale também para a tarefa travada antes de a trava pegar só a
/// tarefa citada. A trava por impedimento, ou pela tarefa devolvida de novo
/// sem mudança, não é texto de mudança nenhuma e fica.
fn accepted_holds(log: &SpecLog) -> Vec<&SpecEvent> {
    let visible = log.visible();
    let done = log.delivered_waves();
    let mut held: Vec<&SpecEvent> = visible
        .iter()
        .copied()
        .filter(|e| e.event_type == "task" && e.str_field("blocked_by").is_some() && e.wave().is_none_or(|n| !done.contains(&n)))
        .collect();
    if held.is_empty() {
        return held;
    }
    let accepted: BTreeSet<&str> = visible
        .iter()
        .filter(|e| e.event_type == "delivered")
        .filter_map(|e| Some((e.wave()?, e.str_field("replan")?)))
        .filter(|(wave, change)| accepted_after(log, &replan_code(*wave, change), 0))
        .map(|(_, change)| change.trim())
        .collect();
    held.retain(|task| task.str_field("blocked_by").is_some_and(|hold| accepted.contains(hold.trim())));
    held
}

/// Tira das voltas `waves` cada uma que a conferência `check` recusa, e
/// devolve cada uma com a recusa dela ([`HeldReturn`]): a recusa de uma volta
/// segura só a onda dela, e as outras seguem.
pub(super) fn hold_refused(waves: &mut Vec<WaveReport>, mut check: impl FnMut(&WaveReport) -> Result<(), RoundRefusal>) -> Vec<HeldReturn> {
    let mut held = Vec::new();
    waves.retain(|wave| match check(wave) {
        Ok(()) => true,
        Err(refusal) => {
            held.push(HeldReturn { wave: wave.wave, refusal });
            false
        }
    });
    held
}

/// As ondas paradas pelo limite de consertos, cada uma com as reprovações
/// seguidas que a pararam: depois da primeira reprovação vêm no máximo
/// [`MAX_FIX_ROUNDS`] rodadas de conserto, e a reprovação seguinte para. A
/// conta começa na versão mais nova do plano da onda que o usuário pediu
/// depois da última reprovação: a onda que ele replanejou volta à fila com a
/// conta zerada, e o que o orquestrador acrescenta ao plano não zera nada.
pub(crate) fn waves_stuck(log: &SpecLog) -> BTreeMap<u64, Vec<&SpecEvent>> {
    let verdicts = log.verdicts_by_wave();
    let reset = last_reset_by_user(log, &verdicts);
    let mut out = BTreeMap::new();
    for (n, verdicts) in verdicts {
        let since = reset.get(&n).copied().unwrap_or(0);
        let mut rejected: Vec<&SpecEvent> = verdicts.iter().rev().take_while(|v| v.id > since && v.str_field("result") == Some("rejected")).copied().collect();
        if rejected.len() > MAX_FIX_ROUNDS {
            rejected.reverse();
            out.insert(n, rejected);
        }
    }
    out
}

/// Cada versão do plano, com as ondas que ela mexe: a versão da onda ou de
/// uma tarefa dela. A tarefa que muda de onda conta para as duas — a de onde
/// saiu, pela versão que ela substitui, e a para onde foi.
fn plan_versions(log: &SpecLog) -> Vec<(u64, &SpecEvent)> {
    let mut out = Vec::new();
    for event in log.block(BlockQuery::Block(Block::Waves)) {
        if !matches!(event.event_type.as_str(), "wave" | "task") {
            continue;
        }
        let before = event.int("replaces").and_then(|old| log.get(old)).and_then(SpecEvent::wave);
        let mut waves: Vec<u64> = event.wave().into_iter().chain(before).collect();
        waves.dedup();
        out.extend(waves.into_iter().map(|n| (n, event)));
    }
    out
}

/// O número do evento mais novo do plano de cada onda.
fn last_planned(log: &SpecLog) -> BTreeMap<u64, u64> {
    let mut planned: BTreeMap<u64, u64> = BTreeMap::new();
    for (n, event) in plan_versions(log) {
        let newest = planned.entry(n).or_insert(0);
        *newest = (*newest).max(event.id);
    }
    planned
}

/// O número da versão mais nova do plano de cada onda que zera a conta de
/// consertos: a que nasce (`origin`) de uma mensagem ou de uma decisão do
/// usuário gravada depois da última reprovação da onda anterior à versão.
fn last_reset_by_user(log: &SpecLog, verdicts: &BTreeMap<u64, Vec<&SpecEvent>>) -> BTreeMap<u64, u64> {
    let from_user = |origin: u64| log.get(origin).filter(|o| matches!(o.event_type.as_str(), "message" | "decision") && o.str_field("author") == Some("user"));
    let mut reset: BTreeMap<u64, u64> = BTreeMap::new();
    for (n, event) in plan_versions(log) {
        let Some(asked) = event.int("origin").and_then(from_user) else {
            continue;
        };
        let last_rejection =
            verdicts.get(&n).into_iter().flatten().filter(|v| v.id < event.id && v.str_field("result") == Some("rejected")).map(|v| v.id).max().unwrap_or(0);
        if asked.id > last_rejection {
            let newest = reset.entry(n).or_insert(0);
            *newest = (*newest).max(event.id);
        }
    }
    reset
}

/// As ondas replanejadas depois do último pedido: a onda, ou uma tarefa dela,
/// ganhou versão nova depois do envio.
pub(super) fn waves_replanned(log: &SpecLog) -> BTreeSet<u64> {
    let last_send = log.last_dispatch_by_wave();
    last_planned(log).into_iter().filter(|(n, planned)| last_send.get(n).is_some_and(|sent| sent < planned)).map(|(n, _)| n).collect()
}

/// O que a volta da onda `wave` diz não ter feito (`undone`): as tarefas que
/// voltam ao backlog, cada uma pelo número da versão vigente e pelo código, e
/// se alguma das citadas já foi para outra onda. Cada código citado aponta
/// uma tarefa com que a onda saiu ([`sent_tasks`]), regravada ou não depois
/// do envio; o que não aponta recusa a volta, com essas tarefas. Volta ao
/// backlog só a tarefa cuja versão vigente leva a própria onda ou nenhuma. A
/// que leva outra onda conta para assumir a volta, mas fica com a onda que a
/// levou, na mesma versão: devolvê-la à fila faria um agente refazer o que
/// essa onda entrega ou já entregou. Com a mudança de plano (`replan`), a lista
/// é obrigatória, vazia quando a onda fez todas: sem ela, a rodada daria por
/// feitas as tarefas que o agente não fez. Sem a mudança, a ausência quer
/// dizer que a onda fez todas.
pub(super) fn undone_of(log: &SpecLog, wave: u64, fields: &Map<String, Value>, replan: bool) -> Result<(Vec<(u64, String)>, bool), RoundRefusal> {
    let codes = log.codes();
    let code_of = |task: &SpecEvent| codes.get(&task.id).cloned().unwrap_or_else(|| task.id.to_string());
    let own = sent_tasks(log, wave);
    let tasks = || own.iter().map(|task| code_of(task)).collect::<Vec<_>>();
    let Some(cited) = fields.get("undone").and_then(Value::as_array) else {
        return if replan { Err(RoundRefusal::ReplanNeedsUndone { wave, tasks: tasks() }) } else { Ok((Vec::new(), false)) };
    };
    let mut undone: Vec<(u64, String)> = Vec::new();
    let mut taken_elsewhere = false;
    for value in cited {
        let said = value.as_str().map_or_else(|| value.to_string(), |code| code.trim().to_string());
        let Some(task) = own.iter().find(|task| code_of(task) == said) else {
            return Err(RoundRefusal::UndoneNotInWave { wave, code: said, tasks: tasks() });
        };
        if task.wave().is_some_and(|other| other != wave) {
            taken_elsewhere = true;
        } else if !undone.iter().any(|(id, _)| *id == task.id) {
            undone.push((task.id, said));
        }
    }
    Ok((undone, taken_elsewhere))
}

/// A versão nova da tarefa `task`, que a onda `wave` não fez: a mesma volta
/// ao backlog de [`backlog_return`]. Com a mudança de plano aceita
/// (`change`), a parte do agente ganha uma linha com ela, no idioma `lang`;
/// o título e a parte do usuário ficam como estão.
fn undone_return(task: &SpecEvent, wave: u64, change: Option<&str>, lang: Locale) -> Map<String, Value> {
    let mut draft = backlog_return(task);
    if let Some(change) = change {
        // Outra onda não resolve o mesmo plano só tentando de novo: a tarefa
        // fica no backlog, travada, até a versão revista ou, na mudança de
        // plano aceita, até a rodada seguinte ([`release_accepted_holds`]).
        draft.insert("blocked_by".into(), json!(change));
        let line = translate("round.returned_change", lang).replace("{wave}", &wave.to_string()).replace("{change}", change.trim());
        append_agent_line(&mut draft, &line);
    }
    draft
}

/// Acrescenta a linha `line` ao fim da parte do agente da versão `draft` de
/// uma tarefa: como item da lista, quando a parte termina numa; como
/// parágrafo novo, quando termina em texto corrido; sozinha, quando a parte
/// está vazia ou falta.
pub(crate) fn append_agent_line(draft: &mut Map<String, Value>, line: &str) {
    let before = draft.get("agent").and_then(Value::as_str).map(str::trim_end).filter(|text| !text.is_empty());
    let agent = match before {
        Some(text) if text.lines().last().is_some_and(|last| last.trim_start().starts_with("- ")) => {
            format!("{text}\n- {line}")
        }
        Some(text) => format!("{text}\n\n{line}"),
        None => line.to_string(),
    };
    draft.insert("agent".into(), json!(agent));
}

/// A versão nova de cada tarefa que as voltas `waves` não fizeram, com a
/// onda que a devolveu, lida em `log`: a volta ao backlog de
/// [`undone_return`]. O impedimento da volta trava todas; a mudança de plano,
/// só as que a recebem ([`change_targets`]), pela decisão do classificador
/// das atividades que `ask` dá ao pedido de [`change_target_question`] — a
/// já tomada, que nunca chama o Jev. A mudança recusada não trava nem anota
/// nada: a tarefa volta solta, sem o texto dela.
pub(super) fn undone_returns(log: &SpecLog, waves: &[WaveReport], lang: Locale, ask: &dyn Fn(&str) -> Option<Value>) -> Vec<(u64, Map<String, Value>)> {
    let mut out = Vec::new();
    for wave in waves {
        let impediment = wave.returns.last().and_then(|id| log.get(*id)).and_then(|event| event.str_field("blocked_by"));
        let refused = wave.replan.as_ref().is_some_and(|plan| plan.refused.is_some());
        let change = wave.replan.as_ref().filter(|_| !refused).map(|plan| plan.change.as_str());
        let held = change.map(|change| change_targets(log, change, &wave.undone, ask)).unwrap_or_default();
        for (id, code) in &wave.undone {
            let Some(task) = log.get(*id) else {
                continue;
            };
            let hold = impediment.or_else(|| change.filter(|_| held.contains(&code.as_str())));
            out.push((wave.wave, guarded_return(task, wave.wave, !wave.files.is_empty(), hold, refused, lang)));
        }
    }
    out
}

/// A volta ao backlog da tarefa `task` que a onda `wave` não fez, com a
/// trava `change` quando há uma. A tarefa devolvida sem mudança na cópia
/// guarda a impressão do que pede, e a segunda volta igual a trava até ser
/// revista. Na mudança de plano recusada (`refused`), sem impedimento, a
/// tarefa volta solta — sem a trava herdada e sem a da segunda volta igual —,
/// mas a impressão fica: a próxima volta igual, sem recusa, a trava.
fn guarded_return(task:&SpecEvent,wave:u64,has_changes:bool,change:Option<&str>,refused:bool,lang:Locale)->Map<String,Value> {
    let mut draft=undone_return(task,wave,change,lang);
    if refused && change.is_none() {
        draft.remove("blocked_by");
    }
    if !has_changes && change.is_none() {
        let context=json!([task.fields.get("title"),task.fields.get("user"),task.fields.get("agent")]).to_string();
        let mut hash=mustard_core::io::sha256::Sha256::new(); hash.update(context.as_bytes());
        let fingerprint=hash.hex_digest();
        if !refused && task.fields.get("retry_guard").and_then(|v|v["fingerprint"].as_str())==Some(fingerprint.as_str()) {
            let hold=if lang==Locale::PtBr {"Tarefa devolvida sem mudança novamente. Confira os insumos atuais e revise ou divida a tarefa antes de tentar outra vez."}
        else {"Task returned without changes again. Verify current inputs and revise or split the task before retrying."};
            draft=undone_return(task,wave,Some(hold),lang);
        }
        draft.insert("retry_guard".into(),json!({"fingerprint":fingerprint,"wave":wave}));
    } else {draft.remove("retry_guard");}
    draft
}

/// A triagem de cada volta sem mudança de `waves`, ambígua — sem impedimento,
/// sem mudança de plano nem tarefa devolvida, e com relato —, pela decisão
/// do Jev já tomada ([`super::jev_decision::decided`]), que nunca o chama: a
/// trava do passo do git está presa, e `jev_on` foi lido antes dela. Volta o aviso de cada volta classificada,
/// em `lang`; sem decisão do Jev, com ele desligado, o aviso pede a
/// conferência de quem conduz, como sempre. A volta cuja decisão ainda não
/// existe, com o Jev ligado, sai de `waves` e fica fora desta rodada, com a
/// pergunta dela, que sai depois de a trava soltar.
pub(super) fn triage_returns(root: &Path, log: &SpecLog, waves: &mut Vec<WaveReport>, lang: Locale, jev_on: JevOn) -> (Vec<Value>, Vec<HeldReturn>, Vec<Waiting>) {
    use crate::shared::return_triage::{Evidence, payload, verdict};
    let (mut warnings, mut waiting) = (Vec::new(), Vec::new());
    let held = hold_refused(waves, |wave| {
        let evidence = Evidence {
            wave: wave.wave,
            assignment: mustard_core::domain::wave_prompt::tasks_text(log, wave.wave),
            summary: wave.delivered.clone(),
            has_changes: !wave.files.is_empty(),
            explicit_replan: wave.replan.is_some(),
            explicit_undone: !wave.undone.is_empty(),
            explicit_blocker: wave.returns.last().and_then(|id| log.get(*id)).and_then(|e| e.str_field("blocked_by")).is_some(),
        };
        let Some(asked) = payload(&evidence) else {
            return Ok(());
        };
        let payloads = vec![asked];
        let doc = match decided(root, log, Question::NoChange, &payloads, jev_on) {
            Judged::Waiting => {
                waiting.push(Waiting::by_payloads(wave.wave, Question::NoChange, payloads));
                return Err(RoundRefusal::JevPending { wave: wave.wave, question: Question::NoChange });
            }
            Judged::Answered(docs) => docs.into_iter().next(),
            Judged::Rule => None,
        };
        let mut warning = verdict(&evidence, doc.as_ref());
        warning["reason"] = json!("no-change-return-needs-verification");
        if lang == Locale::PtBr {
            warning["hint"] = json!(
                "Retorno sem mudança: confira as condições externas atuais e os recibos de teste. Divida ou bloqueie o trabalho não concluído antes de outro despacho. A classificação do relato não comprova suas afirmações."
            );
        }
        warnings.push(warning);
        Ok(())
    });
    (warnings, held, waiting)
}

/// O aviso da resposta da rodada de que as tarefas que a volta `wave` não
/// fez voltaram ao backlog, com os códigos delas, no idioma `lang`: a
/// mudança aceita pode pedir uma decisão nova ou a tarefa reescrita antes da
/// rodada seguinte. `None` quando a onda fez todas.
pub(super) fn tasks_returned(wave: &WaveReport, lang: Locale) -> Option<Value> {
    let codes: Vec<&str> = wave.undone.iter().map(|(_, code)| code.as_str()).collect();
    (!codes.is_empty()).then(|| {
        let hint = translate("round.tasks_returned", lang).replace("{wave}", &wave.wave.to_string()).replace("{tasks}", &codes.join(", "));
        json!({ "reason": "tasks-returned", "wave": wave.wave, "tasks": codes, "hint": hint })
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use mustard_core::io::spec_events as store;
    use tempfile::tempdir;

    use crate::commands::spec_events::write::WriteOpts;

    use super::*;
    use crate::commands::flow::round::tests::*;

    /// A onda que ganha versão nova depois do pedido volta para a fila, uma
    /// vez só: o pedido gravado descrevia o plano antigo. A tarefa que muda de
    /// onda replaneja as duas. A onda que já entregou não volta por
    /// replanejamento — só a reprovação a devolve.
    #[test]
    fn a_wave_replanned_after_its_send_goes_out_again_and_only_once() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // A onda 2 é um lote do programa com duas tarefas, em arquivos
        // diferentes: a que muda de onda não a esvazia, porque um lote vazio
        // nunca sai, e não a prende à onda 1 por um arquivo dividido.
        approved_with(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])], |said| {
            write(
                root,
                "x",
                "task",
                json!({"wave": 2, "text": "Outra tarefa da onda 2.",
                "files": [{"path": "src/c.rs"}], "depends_on": [], "origin": said}),
            );
        });
        let first = round(root, "x", None);
        assert_eq!(first["dispatch"].as_array().map(Vec::len), Some(2), "{first}");

        let path = store::spec_file(root, "x").unwrap();
        let current = |kind: &str, n: u64| -> Value {
            let log = store::read(&path).unwrap().unwrap();
            let event = log.visible().into_iter().find(|e| e.event_type == kind && e.wave() == Some(n)).unwrap_or_else(|| panic!("sem {kind} da onda {n}"));
            let mut fields = event.fields.clone();
            for key in ["v", "id", "code", "at", "search", "type", "author"] {
                fields.remove(key);
            }
            let id = event.id;
            let mut body = Value::Object(fields);
            body["replaces"] = json!(id);
            body
        };

        // A onda 1 ganha outra versão; a tarefa da onda 2 muda para a 1.
        let mut wave = current("wave", 1);
        wave["done_when"] = json!("A suíte passa e o teste novo também.");
        id_of(&write(root, "x", "wave", wave));
        let mut task = current("task", 2);
        task["wave"] = json!(1);
        id_of(&write(root, "x", "task", task));

        let again = round(root, "x", None);
        let waves: Vec<u64> = again["dispatch"].as_array().cloned().unwrap_or_default().iter().filter_map(|d| d["wave"].as_u64()).collect();
        assert_eq!(waves, vec![1, 2], "as duas foram replanejadas depois do pedido: {again}");

        let quiet = round(root, "x", None);
        assert_eq!(quiet["dispatch"], json!([]), "o pedido novo já descreve o plano atual: {quiet}");

        // Entregue, a onda não volta por uma versão nova.
        round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        let mut wave = current("wave", 1);
        wave["text"] = json!("Onda 1, texto revisto.");
        id_of(&write(root, "x", "wave", wave));
        let after = round(root, "x", None);
        assert_eq!(after["dispatch"], json!([]), "a onda 1 já entregou: {after}");
    }

    /// O agente que diz que o plano da onda não funciona segura a onda dele
    /// até o "sim" do usuário, e o "sim" é o clique em "Aceitar" na pergunta
    /// da mudança, gravado pela testemunha. A rodada não recusa: o aviso dela
    /// manda fazer a pergunta, com o cabeçalho e as opções; uma mensagem
    /// escrita à mão pelo modelo, com a mesma pergunta e a mesma resposta, não
    /// destrava nada; o "sim" de outra mudança também não; o clique em
    /// "Aceitar" destrava, e a rodada grava o que a onda entregou.
    #[test]
    fn a_plan_change_that_swaps_a_user_decision_waits_for_the_users_click() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let session = "s-replan";
        crate::shared::context::session::bind_session_spec(&root.to_string_lossy(), session, "x");

        let change = "A onda 1 precisa da 2 antes.";
        let code = replan_code(1, change);
        let back = json!({"wave": 1, "text": "Parei.", "files": ["src/a.rs"], "replan": change,
            "changes_decision": DECISION, "undone": []});
        assert_eq!(returned(root, back)["ok"], json!(true));
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "a mudança segura só a onda dela: {out}");
        let stopped = change_asked(&out);
        assert_eq!(stopped["wave"], json!(1), "{out}");
        let question = QUESTION;
        assert_eq!(stopped["header"], json!(code), "{stopped}");
        assert_eq!(stopped["options"], json!(["Aceitar", "Recusar"]), "{stopped}");
        let hint = stopped["hint"].as_str().unwrap_or_default();
        assert!(hint.contains(&code), "o cabeçalho que a pergunta leva vai no aviso: {hint}");
        assert!(out["next"].as_str().unwrap_or_default().contains(hint), "o próximo passo manda perguntar: {out}");
        assert_eq!(delivered_count(root), 0);

        // O modelo não escreve o "sim": o `run write` recusa a mensagem com a
        // testemunha, seja do usuário, seja do próprio modelo.
        let by_hand = |body: Value| {
            crate::commands::spec_events::write::write_at(&WriteOpts {
                root: root.to_path_buf(),
                spec: Some("x".to_string()),
                event_type: "message".into(),
                json: body.to_string(),
            })
        };
        let witness = json!({ "question": question, "answer": "Aceitar", "change": code });
        let forged = by_hand(json!({ "author": "user", "text": format!("{question}\nAceitar"), "witness": witness }));
        assert_eq!(forged["reason"], json!("user-message-by-hook"), "{forged}");
        let own = by_hand(json!({ "text": format!("{question}\nAceitar"), "witness": witness }));
        assert_eq!(own["reason"], json!("user-message-by-hook"), "{own}");
        let still = round(root, "x", None);
        assert_eq!(change_asked(&still)["wave"], json!(1), "a forged yes accepts nothing: {still}");

        // O "sim" de uma mudança nunca serve para outra.
        click(root, session, question, &replan_code(1, "Outra mudança."), "Aceitar");
        let other = round(root, "x", None);
        assert_eq!(change_asked(&other)["wave"], json!(1), "{other}");
        assert_eq!(delivered_count(root), 0);

        click(root, session, question, &code, "Aceitar");
        let went = round(root, "x", None);
        assert_eq!(went["ok"], json!(true), "{went}");
        assert!(change_asked(&went).is_null(), "{went}");
        assert_eq!(delivered_count(root), 1, "the round records what the wave delivered");
    }

    /// A rodada manda fazer a pergunta e não a escreve: o aviso traz o
    /// cabeçalho e as opções. O código vai no cabeçalho da pergunta, e é ele,
    /// guardado ao lado da resposta, que reconhece o "sim" — a pergunta
    /// escrita com as palavras do usuário vale igual, o código de outra
    /// mudança não vale, e a pergunta sem código nenhum não destrava nada.
    #[test]
    fn question_goes_in_words_and_the_yes_is_recognised_by_the_code() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let session = "s-pergunta-em-palavras";
        crate::shared::context::session::bind_session_spec(&root.to_string_lossy(), session, "x");

        let change = "A onda 1 precisa da onda 2 antes dela.";
        let code = replan_code(1, change);
        let back = json!({"wave": 1, "text": "Parei: o plano não fecha.", "replan": change,
            "changes_decision": DECISION, "undone": []});
        assert_eq!(returned(root, back)["ok"], json!(true));
        let stopped = change_asked(&round(root, "x", None));
        assert_eq!(stopped["wave"], json!(1), "{stopped}");

        // O enunciado é de quem conduz: o aviso não traz pergunta pronta, e o
        // código vai no cabeçalho.
        assert!(stopped.get("question").is_none(), "o aviso não escreve a pergunta: {stopped}");
        assert_eq!(stopped["header"], json!(code), "o código vai no cabeçalho: {stopped}");
        assert_eq!(stopped["options"], json!(["Aceitar", "Recusar"]), "{stopped}");

        // A pergunta que quem despacha reescreve com as palavras do usuário,
        // sem o código em lugar nenhum do texto.
        let mine = "A onda 1 travou e quer a onda 2 antes dela. Posso seguir assim?";

        // O código de outra mudança no cabeçalho não aceita esta.
        click(root, session, mine, &replan_code(1, "Outra mudança."), "Aceitar");
        let other = round(root, "x", None);
        assert_eq!(change_asked(&other)["wave"], json!(1), "o sim de outra mudança não vale: {other}");

        // Sem código nenhum no cabeçalho, nada diz qual mudança o clique
        // decide, e a rodada segue parada.
        click(root, session, mine, "Mudança", "Aceitar");
        let blind = round(root, "x", None);
        assert_eq!(change_asked(&blind)["wave"], json!(1), "sem código não destrava: {blind}");

        // Com o código no cabeçalho, o "sim" vale, seja qual for a frase.
        click(root, session, mine, &code, "Aceitar");
        let went = round(root, "x", None);
        assert_eq!(went["ok"], json!(true), "{went}");
        assert_eq!(delivered_count(root), 1, "a rodada gravou o que a onda entregou: {went}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let clicked = log.visible().into_iter().filter_map(|e| e.fields.get("witness").cloned()).next_back().expect("o clique gravado");
        assert_eq!(clicked["change"], json!(code), "o código fica ao lado da resposta: {clicked}");
        assert_eq!(clicked["question"], json!(mine), "a frase gravada é a que o usuário leu: {clicked}");
    }

    /// Omitting changes_decision cannot authorize a change to an approved plan.
    #[test]
    fn a_plan_change_without_a_decision_field_still_waits_before_commit() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let session = "s-plan-without-decision";
        crate::shared::context::session::bind_session_spec(&root.to_string_lossy(), session, "x");
        std::fs::write(copy_of(root, 1).join("src/a.rs"), "fn one() {}\n// a entrega 1 mudou\n").unwrap();
        let change = "Dividir a soma em duas funções.";
        let back = json!({"wave": 1, "text": "Parei.", "files": ["src/a.rs"], "commit": "a entrega 1 mudou",
            "replan": change, "undone": []});
        assert_eq!(returned(root, back)["ok"], json!(true));

        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        let asked = change_asked(&out);
        assert_eq!(asked["wave"], 1, "{out}");
        assert!(official_deliveries(root).is_empty(), "{out}");
        let again = round(root, "x", None);
        assert_eq!(change_asked(&again)["header"], asked["header"]);
        assert!(official_deliveries(root).is_empty());
        click(root, session, QUESTION, asked["header"].as_str().unwrap(), "Aceitar");
        let accepted = round(root, "x", None);
        assert_eq!(accepted["ok"], true, "{accepted}");
        assert_eq!(official_deliveries(root), BTreeSet::from([1]));
        assert_eq!(last_commit_files(root), "src/a.rs");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let recorded = log.visible().into_iter().find(|e| e.event_type == "delivered").expect("a entrega");
        assert_eq!(recorded.str_field("replan"), Some(change), "a mudança fica gravada na entrega");
        assert!(!swaps_decision(recorded), "{:?}", recorded.fields);
    }

    /// O aviso que manda perguntar não leva texto nenhum do agente — nem a
    /// mudança, nem a decisão que ela troca, nem o que a volta disse: quem
    /// conduz lê a volta e escreve a pergunta com as palavras do usuário, pelo
    /// estilo de resposta.
    #[test]
    fn the_stop_for_a_swapped_decision_carries_none_of_the_agents_text() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let (change, decision, said) =
            ("Mover fold_totals para src/zz_marca.rs e ajustar 37 chamadas", "Manter tudo em um arquivo só", "Parei no laboratório 1177");
        let back = json!({"wave": 1, "text": said, "replan": change, "changes_decision": decision, "undone": []});
        assert_eq!(returned(root, back)["ok"], json!(true));

        let out = round(root, "x", None);
        let stopped = change_asked(&out);
        assert_eq!(stopped["wave"], json!(1), "{out}");
        let shown = format!("{stopped} {}", out["next"]);
        for agent_text in [change, decision, said, "fold_totals", "zz_marca"] {
            assert!(!shown.contains(agent_text), "o aviso copiou o texto do agente ({agent_text}): {shown}");
        }
        assert!(!shown.contains("Pergunta pronta") && stopped.get("question").is_none(), "{shown}");
        let hint = stopped["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("Aceitar") && hint.contains("Recusar") && hint.contains("efeito da mudança"), "a regra da pergunta: {hint}");
    }

    /// A mudança de plano que cita uma das três tarefas que a onda não fez
    /// trava só ela, e a pergunta lista só ela; as outras duas voltam ao
    /// backlog soltas. A tarefa já travada pelo texto da mudança, como antes
    /// de a trava pegar só a citada, segue travada enquanto o usuário não
    /// aceita. A rodada seguinte à que assumiu a volta aceita solta as duas.
    #[test]
    fn a_plan_change_holds_only_the_task_it_cites_and_the_round_after_the_yes_releases_it() {
        use crate::commands::flow::round::queue::task_now;

        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved_with(root, "x", &[(1, &["src/a.rs"], &[])], |said| {
            for text in ["Segunda tarefa da onda 1.", "Terceira tarefa da onda 1."] {
                write(root, "x", "task", json!({"wave": 1, "text": text, "files": [{"path": "src/a.rs"}], "depends_on": [], "origin": said}));
            }
        });
        round(root, "x", None);
        let session = "s-trava-so-a-citada";
        crate::shared::context::session::bind_session_spec(&root.to_string_lossy(), session, "x");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let tasks: Vec<String> = sent_tasks(&log, 1).iter().map(|task| codes[&task.id].clone()).collect();
        assert_eq!(tasks.len(), 3, "{tasks:?}");
        let change = format!("A {} precisa de outra forma antes.", tasks[1]);
        let crit = log.visible().into_iter().find(|e| e.event_type == "criterion").map(|e| e.id).expect("o critério");
        let old = id_of(&write(root, "x", "task", json!({"text": "Tarefa travada antes do conserto.", "files": [{"path": "src/b.rs"}],
            "depends_on": [], "covers": [crit], "blocked_by": change})));
        let old = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap().codes()[&old].clone();
        let back = json!({"wave": 1, "text": "Parei.", "replan": change, "undone": tasks});
        assert_eq!(returned(root, back)["ok"], json!(true));

        let waiting = round(root, "x", None);
        let hint = change_asked(&waiting)["hint"].as_str().unwrap_or_default().to_string();
        assert!(hint.contains(&tasks[1]) && !hint.contains(&tasks[0]) && !hint.contains(&tasks[2]), "a pergunta lista só a citada: {hint}");
        assert!(task_now(root, &old).str_field("blocked_by").is_some(), "sem o aceite, a trava fica: {waiting}");

        click(root, session, QUESTION, &replan_code(1, &change), "Aceitar");
        let accepted = round(root, "x", None);
        assert_eq!(accepted["ok"], json!(true), "{accepted}");
        let held: Vec<bool> = tasks.iter().map(|code| task_now(root, code).str_field("blocked_by").is_some()).collect();
        assert_eq!(held, vec![false, true, false], "só a citada fica travada: {accepted}");

        let blocked = task_now(root, &tasks[1]).id;
        let next = round(root, "x", None);
        let released = task_now(root, &tasks[1]);
        assert_eq!(released.str_field("blocked_by"), None, "a rodada seguinte solta a citada: {next}");
        assert_eq!(released.int("replaces"), Some(blocked), "{:?}", released.fields);
        assert_eq!(task_now(root, &old).str_field("blocked_by"), None, "a trava gravada antes também sai: {next}");
    }

    /// A onda 1 da spec `x`, com três tarefas em TypeScript, sai, muda `src/a.ts` na cópia e
    /// volta pedindo mudança de plano, sem o resumo do commit e com as três em
    /// `undone`; a rodada seguinte a segura à espera do clique. Devolve os
    /// códigos das três e o texto da mudança.
    fn refused_wave_setup(root: &Path) -> (Vec<String>, &'static str) {
        approved_with(root, "x", &[(1, &["src/a.ts"], &[])], |said| {
            for text in ["Segunda tarefa da onda 1.", "Terceira tarefa da onda 1."] {
                write(root, "x", "task", json!({"wave": 1, "text": text, "files": [{"path": "src/a.ts"}], "depends_on": [], "origin": said}));
            }
        });
        round(root, "x", None);
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let tasks: Vec<String> = sent_tasks(&log, 1).iter().map(|task| codes[&task.id].clone()).collect();
        assert_eq!(tasks.len(), 3, "{tasks:?}");
        std::fs::write(copy_of(root, 1).join("src/a.ts"), "export function one() {}\n// a entrega 1 mudou\n").unwrap();
        let change = "Dividir a onda em três ondas menores.";
        let back = json!({"wave": 1, "text": "Parei.", "files": ["src/a.ts"], "replan": change, "undone": tasks});
        assert_eq!(returned(root, back)["ok"], json!(true));
        let waiting = round(root, "x", None);
        assert_eq!(change_asked(&waiting)["wave"], json!(1), "sem recusa, a onda espera o clique: {waiting}");
        assert!(official_deliveries(root).is_empty(), "{waiting}");
        (tasks, change)
    }

    /// A recusa assumida pela rodada `out`: a entrega oficial da onda 1 é
    /// gravada com o motivo `reason` e a mensagem `origin`, o que a onda mudou
    /// entra no commit, a onda sai do ar e as três tarefas `tasks` voltam ao
    /// backlog numa versão sem onda, sem trava e sem o texto da mudança.
    fn assert_refusal_taken(root: &Path, out: &Value, tasks: &[String], change: &str, reason: &str, origin: u64) {
        use crate::commands::flow::round::queue::open_sends;

        assert_eq!(out["ok"], json!(true), "{out}");
        assert!(change_asked(out).is_null(), "a onda recusada não espera mais: {out}");
        assert_eq!(official_deliveries(root), BTreeSet::from([1]), "{out}");
        assert_eq!(last_commit_files(root), "src/a.ts", "o que a onda mudou entra no commit: {out}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert!(!open_sends(&log).contains_key(&1), "a onda não ocupa vaga: {out}");
        let delivered = log.visible().into_iter().find(|e| e.event_type == "delivered" && e.wave() == Some(1)).expect("a entrega oficial");
        assert_eq!(delivered.str_field("replan_refused"), Some(reason), "{:?}", delivered.fields);
        assert_eq!(delivered.int("origin"), Some(origin), "{:?}", delivered.fields);
        let codes = log.codes();
        let returned: Vec<&SpecEvent> = out["recorded"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|entry| entry["type"] == json!("task") && entry["wave"] == json!(1))
            .filter_map(|entry| log.get(entry["id"].as_u64()?))
            .collect();
        let mut back: Vec<String> = returned.iter().map(|task| codes[&task.id].clone()).collect();
        back.sort();
        let mut expected = tasks.to_vec();
        expected.sort();
        assert_eq!(back, expected, "as três voltam ao backlog: {out}");
        for task in returned {
            assert_eq!(task.wave(), None, "{:?}", task.fields);
            assert_eq!(task.str_field("blocked_by"), None, "sem trava: {:?}", task.fields);
            assert!(!task.str_field("agent").unwrap_or_default().contains(change), "sem o texto da mudança: {:?}", task.fields);
        }
    }

    /// A linha `REVIEWED` de quem conduz para a volta da onda 1 da spec `x`,
    /// depois de ler o diff dela pela revisão de onda, como na rodada de
    /// verdade: a onda que mudou código só entra no commit revisada.
    fn reviewed_line(root: &Path) -> String {
        let view = crate::commands::flow::round::review::view_at(root, Some("x"), 1, false);
        let fingerprint = view["fingerprint"].as_str().unwrap_or_else(|| panic!("a revisão da onda 1: {view}"));
        format!(r#"<REVIEWED>{{"wave":1,"fingerprint":"{fingerprint}","note":"Diff lido."}}</REVIEWED>"#)
    }

    /// O clique em "Recusar" na pergunta da mudança solta a onda na rodada
    /// seguinte: a recusa fica gravada com o clique, o que a onda mudou entra
    /// no commit, e as três tarefas voltam ao backlog soltas.
    #[test]
    fn plan_refused_by_the_click_returns_the_tasks_unheld_and_frees_the_slot() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (tasks, change) = refused_wave_setup(root);
        let session = "s-recusa-no-clique";
        crate::shared::context::session::bind_session_spec(&root.to_string_lossy(), session, "x");
        click(root, session, QUESTION, &replan_code(1, change), "Recusar");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let clicked = log.visible().into_iter().rfind(|e| e.fields.contains_key("witness")).map(|e| e.id).expect("o clique gravado");

        let out = round(root, "x", Some(&reviewed_line(root)));
        let reason = translate("round.plan_refused.click", Locale::PtBr);
        assert_refusal_taken(root, &out, &tasks, change, reason, clicked);
    }

    /// Quem conduz recusa a mudança sem clique, pela linha `PLAN_REFUSED` do
    /// relatório, com o mesmo efeito do clique: a recusa guarda o motivo da
    /// linha e, em `origin`, a última mensagem do usuário.
    #[test]
    fn plan_refused_by_the_report_line_has_the_effect_of_the_click() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (tasks, change) = refused_wave_setup(root);
        let said = id_of(&write(root, "x", "message", json!({"author": "user", "text": "Pode recusar a onda 1."})));

        let reason = "O usuário mandou manter o plano aprovado.";
        let refusal = format!(r#"<PLAN_REFUSED>{{"wave":1,"reason":"{reason}"}}</PLAN_REFUSED>"#);
        let out = round(root, "x", Some(&format!("{}\n{refusal}", reviewed_line(root))));
        assert_refusal_taken(root, &out, &tasks, change, reason, said);
    }

    /// Aceitar continua só pelo clique: nenhuma linha do relatório aceita a
    /// mudança, e a linha de recusa que aponta onda sem mudança à espera é
    /// recusada sem gravar nada. A onda segue esperando o clique.
    #[test]
    fn plan_refused_line_never_accepts_and_points_only_at_a_waiting_change() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        refused_wave_setup(root);
        let accept = round(root, "x", Some(r#"<PLAN_ACCEPTED>{"wave":1}</PLAN_ACCEPTED>"#));
        assert_eq!(accept["ok"], json!(false), "não há linha que aceite: {accept}");
        let elsewhere = round(root, "x", Some(r#"<PLAN_REFUSED>{"wave":2,"reason":"Outra onda."}</PLAN_REFUSED>"#));
        assert_eq!(elsewhere["ok"], json!(false), "{elsewhere}");
        assert!(elsewhere["hint"].as_str().unwrap_or_default().contains("PLAN_REFUSED"), "{elsewhere}");
        let blank = round(root, "x", Some(r#"<PLAN_REFUSED>{"wave":1,"reason":" "}</PLAN_REFUSED>"#));
        assert_eq!(blank["ok"], json!(false), "a recusa pede o motivo: {blank}");
        assert!(official_deliveries(root).is_empty());
        let still = round(root, "x", Some(r#"<USAGE>{"wave":1}</USAGE>"#));
        assert_eq!(change_asked(&still)["wave"], json!(1), "sem clique, a mudança segue à espera: {still}");
        assert!(official_deliveries(root).is_empty(), "{still}");
    }

    /// A tarefa da mudança recusada volta solta mesmo devolvida de novo sem
    /// mudança na cópia, mas a impressão dela fica: a próxima volta igual,
    /// sem recusa, a trava até ser revista.
    #[test]
    fn plan_refused_return_frees_the_task_but_keeps_the_retry_count() {
        let task = SpecEvent {
            id: 1,
            event_type: "task".into(),
            line: 1,
            fields: json!({"title": "Export", "agent": "Implement export", "files": [], "depends_on": [], "blocked_by": "Dividir a onda."}).as_object().unwrap().clone(),
        };
        let first = guarded_return(&task, 1, false, None, false, Locale::EnUs);
        let mut again = task.clone();
        again.fields = first;
        let refused = guarded_return(&again, 2, false, None, true, Locale::EnUs);
        assert!(!refused.contains_key("blocked_by"), "a recusa solta a tarefa: {refused:?}");
        assert!(refused.contains_key("retry_guard"), "{refused:?}");
        let mut next = task.clone();
        next.fields = refused;
        assert!(guarded_return(&next, 3, false, None, false, Locale::EnUs).contains_key("blocked_by"), "a volta igual seguinte trava");
    }

    /// A cópia gravada no último envio da onda `n` da spec `x`.
    fn copy_of(root: &Path, n: u64) -> std::path::PathBuf {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let copy = mustard_core::io::wave_prompt::recorded_copy(&log, n).unwrap_or_else(|| panic!("a entrega {n} sem cópia"));
        std::path::PathBuf::from(copy.path)
    }

    /// Os arquivos que o último commit de `root` mudou.
    fn last_commit_files(root: &Path) -> String {
        git_text(root, &["show", "--name-only", "--format=", "HEAD"])
    }

    /// As ondas com a entrega oficial gravada na spec `x`.
    fn official_deliveries(root: &Path) -> BTreeSet<u64> {
        store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap().delivered_waves()
    }

    #[test]
    fn an_external_impediment_without_replanning_is_not_dispatched_again() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let task = log.visible().into_iter().find(|e| e.event_type == "task").unwrap();
        let code = log.codes().get(&task.id).unwrap().clone();
        let out = returned(root, json!({"wave":1,"text":"Acesso ainda não verificado.","blocked_by":"Aguardar acesso autorizado à fonte","undone":[code]}));
        assert_eq!(out["ok"], true, "{out}");
        let accepted = round(root, "x", None);
        assert_eq!(accepted["ok"], true, "{accepted}");
        for _ in 0..3 {
            let again = round(root, "x", None);
            assert!(again["dispatch"].as_array().unwrap().is_empty(), "{again}");
        }
        let log = store::read(&path).unwrap().unwrap();
        let current = log.current(task.id).unwrap();
        assert_eq!(current.str_field("blocked_by"), Some("Aguardar acesso autorizado à fonte"));
        assert!(super::super::queue::backlog_left(&log).contains(&current.id));
    }

    #[test]
    fn repeated_empty_undone_work_requires_revision_instead_of_another_agent() {
        let task=SpecEvent{id:1,event_type:"task".into(),line:1,fields:json!({"title":"Export","user":"Preserve input","agent":"Implement export","files":[],"depends_on":[]}).as_object().unwrap().clone()};
        let first=guarded_return(&task,1,false,None,false,Locale::EnUs);
        assert!(!first.contains_key("blocked_by"));
        let mut retried=task.clone();retried.fields=first;
        let second=guarded_return(&retried,2,false,None,false,Locale::EnUs);
        assert!(second.contains_key("blocked_by"));
        retried.fields.insert("agent".into(),json!("Use newly supplied local input"));
        assert!(!guarded_return(&retried,3,false,None,false,Locale::EnUs).contains_key("blocked_by"));
        assert!(!guarded_return(&task,1,true,None,false,Locale::EnUs).contains_key("retry_guard"));
    }

    /// Duas ondas voltam na mesma rodada, e uma pede novo plano: a outra é
    /// conferida, comitada e gravada, e o despacho segue com a onda que só
    /// dependia dela. A que pede novo plano fica fora do commit, com a cópia
    /// como está e o aviso que manda perguntar; a onda que depende dela não
    /// sai, e a que divide arquivo com ela já tinha saído ao lado dela. Depois
    /// do clique em "Aceitar", a rodada seguinte a comita, e a que ela
    /// segurava sai.
    #[test]
    fn a_wave_asking_for_a_new_plan_holds_only_itself_and_the_next_round_commits_it_after_the_click() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[]), (3, &["src/a.rs"], &[]), (4, &["src/c.rs"], &[2]), (5, &["src/d.rs"], &[1])]);
        let mut first = waves_in(&round(root, "x", None), "dispatch");
        first.sort_unstable();
        assert_eq!(first, vec![1, 2, 3], "a entrega 3 sai ao lado da 1, mesmo dividindo o arquivo dela");
        let (one, two) = (copy_of(root, 1), copy_of(root, 2));
        std::fs::write(one.join("src/a.rs"), "fn one() {}\n// a entrega 1 mudou\n").unwrap();
        std::fs::write(two.join("src/b.rs"), "fn one() {}\n// a entrega 2 mudou\n").unwrap();
        let change = "A onda 1 precisa de outra tarefa antes.";
        let code = replan_code(1, change);
        let asks = json!({"wave": 1, "text": "Parei.", "files": ["src/a.rs"], "commit": "a entrega 1 mudou",
            "replan": change, "changes_decision": DECISION, "undone": []});
        assert_eq!(returned(root, asks)["ok"], json!(true));
        let done = json!({"wave": 2, "text": "Saiu.", "files": ["src/b.rs"], "commit": "a entrega 2 saiu"});
        assert_eq!(returned(root, done)["ok"], json!(true));

        let held = round(root, "x", None);
        assert_eq!(held["ok"], json!(true), "a mudança não segura a outra volta: {held}");
        assert_eq!(last_commit_files(root), "src/b.rs", "só a onda 2 entra no commit: {held}");
        assert_eq!(official_deliveries(root), BTreeSet::from([2]), "{held}");
        assert_eq!(std::fs::read_to_string(root.join("src/a.rs")).unwrap(), "fn one() {}\n", "{held}");
        assert_eq!(std::fs::read_to_string(one.join("src/a.rs")).unwrap(), "fn one() {}\n// a entrega 1 mudou\n");
        let asked = change_asked(&held);
        assert_eq!(asked["wave"], json!(1), "{held}");
        assert_eq!(asked["header"], json!(code), "{asked}");
        let question = QUESTION;
        let hint = asked["hint"].as_str().unwrap_or_default();
        assert!(hint.contains(&code), "o cabeçalho da pergunta vai no aviso: {hint}");
        assert!(held["next"].as_str().unwrap_or_default().contains(hint), "{held}");
        assert_eq!(waves_in(&held, "dispatch"), vec![4], "a 5 depende da 1: {held}");
        assert_eq!(waves_in(&held, "running"), vec![3, 4], "a onda que espera o clique não está em andamento: {held}");

        let session = "s-segura-so-ela";
        crate::shared::context::session::bind_session_spec(&root.to_string_lossy(), session, "x");
        click(root, session, question, &code, "Aceitar");
        let went = round(root, "x", None);
        assert_eq!(went["ok"], json!(true), "{went}");
        assert!(change_asked(&went).is_null(), "{went}");
        assert_eq!(last_commit_files(root), "src/a.rs", "a rodada seguinte comita a onda 1: {went}");
        assert_eq!(official_deliveries(root), BTreeSet::from([1, 2]), "{went}");
        assert_eq!(std::fs::read_to_string(root.join("src/a.rs")).unwrap(), "fn one() {}\n// a entrega 1 mudou\n");
        let mut sent = waves_in(&went, "dispatch");
        sent.sort_unstable();
        assert_eq!(sent, vec![5], "a onda que ela segurava sai: {went}");
    }

    /// A onda que pede novo plano já voltou e não é órfã, mesmo com o Claude
    /// Code que a mandou fechado: a cópia dela fica com o que ela entregou, a
    /// rodada não a manda de novo, e ela segue segurando a onda que depende
    /// dela. Com o clique, a rodada comita o que ficou na cópia.
    #[test]
    fn a_wave_waiting_for_the_click_keeps_its_copy_when_its_sender_is_gone() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/a.rs"], &[1])]);
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1]);

        // O Claude Code que mandou a onda 1 fechou.
        orphan_the_send(root, 1);

        let one = copy_of(root, 1);
        std::fs::write(one.join("src/a.rs"), "fn one() {}\n// a entrega 1 mudou\n").unwrap();
        let change = "A onda 1 precisa de outra tarefa antes.";
        let asks = json!({"wave": 1, "text": "Parei.", "files": ["src/a.rs"], "commit": "a entrega 1 mudou",
            "replan": change, "changes_decision": DECISION, "undone": []});
        assert_eq!(returned(root, asks)["ok"], json!(true));

        let held = round(root, "x", None);
        assert_eq!(held["ok"], json!(true), "{held}");
        assert_eq!(change_asked(&held)["wave"], json!(1), "{held}");
        assert_eq!(waves_in(&held, "dispatch"), Vec::<u64>::new(), "nem reenvio, nem a onda que depende dela: {held}");
        assert_eq!(std::fs::read_to_string(one.join("src/a.rs")).unwrap(), "fn one() {}\n// a entrega 1 mudou\n");

        let session = "s-copia-fica";
        crate::shared::context::session::bind_session_spec(&root.to_string_lossy(), session, "x");
        click(root, session, QUESTION, &replan_code(1, change), "Aceitar");
        let went = round(root, "x", None);
        assert_eq!(went["ok"], json!(true), "{went}");
        assert_eq!(last_commit_files(root), "src/a.rs", "{went}");
        assert_eq!(std::fs::read_to_string(root.join("src/a.rs")).unwrap(), "fn one() {}\n// a entrega 1 mudou\n");
    }

    /// O fechamento depende de todas as ondas: a que pede novo plano sem o
    /// clique o segura, com a pergunta que a decide, depois de assumir e
    /// comitar a outra volta.
    #[test]
    fn the_close_takes_the_other_return_and_is_held_by_the_wave_waiting_for_the_click() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        round(root, "x", None);
        std::fs::write(copy_of(root, 2).join("src/b.rs"), "fn one() {}\n// a entrega 2 mudou\n").unwrap();
        let change = "A onda 1 precisa de outra tarefa antes.";
        let asks = json!({"wave": 1, "text": "Parei.", "replan": change, "changes_decision": DECISION, "undone": []});
        assert_eq!(returned(root, asks)["ok"], json!(true));
        let done = json!({"wave": 2, "text": "Saiu.", "files": ["src/b.rs"], "commit": "a entrega 2 saiu"});
        assert_eq!(returned(root, done)["ok"], json!(true));

        let opts = crate::commands::flow::close::CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() };
        super::super::review::test_conductor_review(root, "x");
        let closed = crate::commands::flow::close::close_for(&opts, None);
        assert_eq!(closed["reason"], json!("wave-plan-does-not-work"), "{closed}");
        assert_eq!(closed["header"], json!(replan_code(1, change)), "{closed}");
        assert!(closed.get("question").is_none(), "{closed}");
        assert_eq!(official_deliveries(root), BTreeSet::from([2]), "{closed}");
        assert_eq!(last_commit_files(root), "src/b.rs", "{closed}");
    }

    /// Cada onda tem no máximo duas rodadas de conserto. Depois da terceira
    /// reprovação seguida a rodada não a manda de novo, e a resposta traz a
    /// pergunta ao usuário com os três vereditos e as duas saídas. A parada
    /// segue até o usuário replanejar a onda; a versão nova do plano que não
    /// nasce dele não a destrava, e a que nasce de uma decisão dele a devolve
    /// à fila.
    #[test]
    fn a_wave_rejected_after_its_second_fix_round_is_not_sent_again_and_the_user_is_asked() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":1}"#).unwrap();
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1]);

        // Cada tentativa volta, e a revisão dela reprova.
        let rejected = |n: usize| {
            let out = round(root, "x", Some(&delivered(root, 1, &format!("Tentativa {n}."), &["src/a.rs"])));
            assert_eq!(out["ok"], json!(true), "{out}");
            round(root, "x", Some(&verdict(root, 1, "rejected", &format!("reprovação {n}"))))
        };
        for n in 1..=2 {
            let fix = rejected(n);
            assert_eq!(waves_in(&fix, "dispatch"), vec![1], "rodada de conserto {n}: {fix}");
        }
        // Os envios de onda: o pedido de revisão que cada veredito pede não
        // conta.
        let sends = |root: &Path| {
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            log.visible().iter().filter(|e| e.event_type == "send" && e.wave().is_some()).count()
        };
        assert_eq!(sends(root), 3);
        // O conserto que saiu está em andamento e ocupa a única vaga.
        let busy = round(root, "x", None);
        assert_eq!(waves_in(&busy, "dispatch"), Vec::<u64>::new(), "{busy}");
        assert_eq!(waves_in(&busy, "running"), vec![1], "{busy}");

        let stopped = rejected(3);
        assert_eq!(stopped["ok"], json!(true), "a parada não recusa a rodada: {stopped}");
        assert_eq!(waves_in(&stopped, "dispatch"), Vec::<u64>::new(), "{stopped}");
        assert_eq!(sends(root), 3, "nada saiu depois da terceira reprovação");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let verdicts: Vec<Value> = log
            .visible()
            .into_iter()
            .filter(|e| e.event_type == "verdict")
            .map(|e| json!({"code": codes[&e.id], "text": e.str_field("text").unwrap()}))
            .collect();
        assert_eq!(verdicts.len(), 3, "a terceira reprovação foi gravada");
        let question = translate("round.fix_limit.question", Locale::PtBr).replace("{wave}", "1").replace("{max}", &MAX_FIX_ROUNDS.to_string());
        assert_eq!(stopped["stopped"], json!([{"wave": 1, "question": question, "verdicts": verdicts}]), "{stopped}");
        // Sem mais nada a fazer, o próximo passo é a pergunta, com os vereditos.
        let codes: Vec<&str> = verdicts.iter().filter_map(|v| v["code"].as_str()).collect();
        let asked = translate("round.fix_limit", Locale::PtBr)
            .replace("{wave}", "1")
            .replace("{count}", "3")
            .replace("{max}", &MAX_FIX_ROUNDS.to_string())
            .replace("{verdicts}", &codes.join(", "));
        assert!(stopped["next"].as_str().unwrap_or_default().ends_with(&asked), "{stopped}");
        assert!(stopped.get("command").is_none(), "{stopped}");

        // Parada continua parada.
        let still = round(root, "x", None);
        assert_eq!(waves_in(&still, "stopped"), vec![1], "{still}");
        assert_eq!(sends(root), 3);

        // A versão nova da tarefa que o orquestrador grava, com a origem de
        // antes das reprovações, não destrava a onda.
        replan(root, 1);
        let held = round(root, "x", None);
        assert_eq!(waves_in(&held, "stopped"), vec![1], "{held}");
        assert_eq!(waves_in(&held, "dispatch"), Vec::<u64>::new(), "{held}");

        // O plano que o usuário revê devolve a onda à fila.
        let decided = user_decides(root, "Refazer a tarefa da onda 1.");
        replan_from(root, 1, Some(decided));
        let back = round(root, "x", None);
        assert_eq!(back["ok"], json!(true), "{back}");
        assert_eq!(waves_in(&back, "dispatch"), vec![1], "{back}");
        assert!(back.get("stopped").is_none(), "{back}");
    }

    /// A decisão do usuário de rever o plano, nascida de uma fala dele gravada
    /// agora pelo gancho da entrada. Devolve o número da decisão.
    fn user_decides(root: &Path, text: &str) -> u64 {
        let said = id_of(&write(root, "x", "message", json!({"author": "user", "text": text})));
        id_of(&write(
            root,
            "x",
            "decision",
            json!({"author": "user", "text": text,
            "why": "o usuário reviu o plano da onda", "keys": ["plano"], "waves": [1], "origin": said}),
        ))
    }

    /// A conta de consertos só zera com o usuário. A tarefa que o orquestrador
    /// acrescenta à onda depois de uma reprovação, nascida de uma decisão dele,
    /// sai de novo para a onda, mas não zera a conta: a terceira reprovação
    /// para a onda. A fala do usuário gravada antes da última reprovação também
    /// não zera; a decisão dele gravada depois, sim.
    #[test]
    fn only_a_users_decision_resets_the_fix_count_of_a_wave() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":1}"#).unwrap();
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1]);
        // O veredito final reprovado, com o combinado vigente atendido: sem
        // a lista `agreed` cobrindo o item que a decisão do orquestrador cria
        // mais adiante, a terceira reprovação seria recusada por faltar item,
        // antes de a conta de consertos entrar em jogo.
        let rejected = |n: usize, agreed: &[&str]| {
            let out = round(root, "x", Some(&delivered(root, 1, &format!("Tentativa {n}."), &["src/a.rs"])));
            assert_eq!(out["ok"], json!(true), "{out}");
            let agreed: Vec<Value> = agreed.iter().map(|item| json!({"item": item, "met": true})).collect();
            seed_review(root);
            let wrote = judged(
                root,
                json!({"wave": 1, "result": "rejected", "final": true,
                "text": format!("reprovação {n}"), "criteria": [{"criterion": "MSTD-CRIT-0001"}],
                "agreed": agreed}),
            );
            assert_eq!(wrote["ok"], json!(true), "{wrote}");
            round(root, "x", None)
        };
        assert_eq!(waves_in(&rejected(1, &[]), "dispatch"), vec![1]);
        // Uma fala do usuário antes da segunda reprovação: a tarefa que nasce
        // dela depois dessa reprovação não zera a conta.
        let early = id_of(&write(root, "x", "message", json!({"author": "user", "text": "Veja o teste."})));
        assert_eq!(waves_in(&rejected(2, &[]), "dispatch"), vec![1]);

        // O orquestrador acrescenta uma tarefa à onda, por decisão dele.
        let own = id_of(&write(
            root,
            "x",
            "decision",
            json!({"text": "Falta uma tarefa na onda 1.",
            "why": "a revisão apontou", "keys": ["tarefa"], "waves": [1], "origin": early}),
        ));
        for origin in [own, early] {
            write(
                root,
                "x",
                "task",
                json!({"wave": 1, "text": format!("Tarefa acrescentada ({origin})."),
                "files": [{"path": "src/a.rs"}], "depends_on": [], "origin": origin}),
            );
        }
        let again = round(root, "x", None);
        assert_eq!(waves_in(&again, "dispatch"), vec![1], "o plano mudou depois do pedido: {again}");

        let stopped = rejected(3, &["MSTD-DEC-0001"]);
        assert_eq!(waves_in(&stopped, "stopped"), vec![1], "a tarefa do orquestrador não zera a conta: {stopped}");
        assert_eq!(waves_in(&stopped, "dispatch"), Vec::<u64>::new(), "{stopped}");
        assert_eq!(stopped["stopped"][0]["verdicts"].as_array().map(Vec::len), Some(3), "{stopped}");

        let decided = user_decides(root, "Refazer a onda 1 com a tarefa nova.");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let wave = log.visible().into_iter().find(|e| e.event_type == "wave" && e.wave() == Some(1)).unwrap();
        let mut body = wave.fields.clone();
        for key in ["v", "id", "code", "at", "search", "type", "author"] {
            body.remove(key);
        }
        body.insert("replaces".into(), json!(wave.id));
        body.insert("origin".into(), json!(decided));
        write(root, "x", "wave", Value::Object(body));
        let back = round(root, "x", None);
        assert!(back.get("stopped").is_none(), "a decisão do usuário zera a conta: {back}");
        assert_eq!(waves_in(&back, "dispatch"), vec![1], "{back}");
    }

    /// A história da onda `n` parada pelo limite de consertos: cada tentativa
    /// é um pedido, a entrega e a reprovação dela.
    fn stuck(root: &Path, n: u64, files: &[&str]) {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let crit = log.visible().into_iter().find(|e| e.event_type == "criterion").map(|e| e.id).unwrap();
        for attempt in 0..=MAX_FIX_ROUNDS {
            seed_send(root, n);
            write(root, "x", "delivered", json!({"wave": n, "text": format!("Tentativa {attempt}."), "files": files}));
            crate::shared::spec_state::seed_verdict(root, "x", n, "rejected", crit);
        }
    }

    /// A onda parada pelo limite de consertos segura só ela e as que dependem
    /// dela, direta ou por outra onda: a onda independente sai, e a resposta
    /// traz a pergunta com os vereditos da onda parada antes do resto. Tirada
    /// do plano, a onda parada deixa de contar: não segura mais nada; e a
    /// onda em andamento tirada do plano não ocupa vaga.
    #[test]
    fn a_stuck_wave_holds_only_itself_and_its_dependents_and_stops_counting_out_of_the_plan() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(
            root,
            "x",
            &[
                (1, &["src/a.rs"], &[]),
                (2, &["src/b.rs"], &[1]),
                (3, &["src/c.rs"], &[2]),
                (4, &["src/d.rs"], &[]),
                (5, &["src/e.rs"], &[]),
                (6, &["src/f.rs"], &[1]),
            ],
        );
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":3}"#).unwrap();
        stuck(root, 1, &["src/a.rs"]);
        // A 2 entregou antes de a rodada existir: a 3 só espera por ela através da 1.
        write(root, "x", "delivered", json!({"wave": 2, "text": "Saiu antes da rodada.", "files": ["src/b.rs"]}));
        seed_send(root, 4);
        write(root, "x", "delivered", json!({"wave": 4, "text": "Saiu.", "files": ["src/d.rs"]}));

        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(waves_in(&out, "dispatch"), vec![5], "a 6 depende da 1, e a 3 depende dela pela 2: {out}");
        assert!(out.get("reviews").is_none(), "{out}");
        assert_eq!(waves_in(&out, "stopped"), vec![1], "{out}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let judged: Vec<&SpecEvent> = log.visible().into_iter().filter(|e| e.event_type == "verdict").collect();
        let verdicts: Vec<Value> = judged.iter().map(|e| json!({"code": codes[&e.id], "text": e.str_field("text").unwrap()})).collect();
        assert_eq!(out["stopped"][0]["verdicts"], json!(verdicts), "{out}");
        let names: Vec<&str> = judged.iter().map(|e| codes[&e.id].as_str()).collect();
        let asked = translate("round.fix_limit", Locale::PtBr)
            .replace("{wave}", "1")
            .replace("{count}", "3")
            .replace("{max}", &MAX_FIX_ROUNDS.to_string())
            .replace("{verdicts}", &names.join(", "));
        let rest = format!("{} {}", translate("round.next", Locale::PtBr), translate("round.report", Locale::PtBr));
        assert!(out["next"].as_str().unwrap_or_default().ends_with(&format!("{asked} {rest}")), "{out}");

        // O usuário tira do plano a onda 1 e a 5, que estava em andamento.
        let targets: Vec<u64> =
            log.visible().into_iter().filter(|e| matches!(e.event_type.as_str(), "wave" | "task") && matches!(e.wave(), Some(1 | 5))).map(|e| e.id).collect();
        write(root, "x", "remove", json!({"targets": targets, "reason": "o usuário tirou as ondas do plano"}));
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert!(out.get("stopped").is_none(), "{out}");
        let mut sent = waves_in(&out, "dispatch");
        sent.sort_unstable();
        assert_eq!(sent, vec![3, 6], "{out}");
        assert_eq!(waves_in(&out, "running"), vec![3, 6], "a onda fora do plano não ocupa vaga: {out}");
    }

    /// O aviso da resposta `out` com o motivo `reason`, ou nulo.
    fn held_warning(out: &Value, reason: &str) -> Value {
        let mut warnings = out["warnings"].as_array().into_iter().flatten();
        warnings.find(|w| w["reason"] == json!(reason)).cloned().unwrap_or(Value::Null)
    }

    /// A frase do próximo passo da volta da onda `wave` que ficou de fora,
    /// com a mensagem `hint` da recusa dela.
    fn held_line(wave: u64, hint: &Value) -> String {
        translate("round.held_return", Locale::PtBr).replace("{wave}", &wave.to_string()).replace("{hint}", hint.as_str().unwrap_or_default())
    }

    /// Duas ondas voltam na mesma rodada, e a cópia de uma mudou arquivo sem
    /// o título do commit na volta: a outra é conferida, comitada e gravada, e
    /// o despacho segue com a onda que só dependia dela. A recusada fica fora
    /// do commit, com a cópia como está e o aviso e o próximo passo pedindo a
    /// volta de novo; ela não está em andamento, e a onda que depende dela não
    /// sai. Gravada de novo com o título, a rodada seguinte a comita, e a onda
    /// que dependia dela sai.
    #[test]
    fn a_return_without_the_commit_title_holds_only_its_wave_and_goes_in_once_recorded_again() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[]), (3, &["src/c.rs"], &[2]), (4, &["src/d.rs"], &[1])]);
        let mut first = waves_in(&round(root, "x", None), "dispatch");
        first.sort_unstable();
        assert_eq!(first, vec![1, 2], "a 3 e a 4 esperam as dependências");
        let (one, two) = (copy_of(root, 1), copy_of(root, 2));
        let changed = "fn one() {}\n// a entrega 1 mudou\n";
        std::fs::write(one.join("src/a.rs"), changed).unwrap();
        std::fs::write(two.join("src/b.rs"), "fn one() {}\n// a entrega 2 mudou\n").unwrap();
        assert_eq!(returned(root, json!({"wave": 1, "text": "Mexi e não contei."}))["ok"], json!(true));
        let done = json!({"wave": 2, "text": "Saiu.", "files": ["src/b.rs"], "commit": "a entrega 2 saiu"});
        assert_eq!(returned(root, done)["ok"], json!(true));

        let held = round(root, "x", None);
        assert_eq!(held["ok"], json!(true), "a volta recusada não segura a outra: {held}");
        assert_eq!(last_commit_files(root), "src/b.rs", "só a onda 2 entra no commit: {held}");
        assert_eq!(official_deliveries(root), BTreeSet::from([2]), "{held}");
        assert_eq!(std::fs::read_to_string(root.join("src/a.rs")).unwrap(), "fn one() {}\n", "{held}");
        assert_eq!(std::fs::read_to_string(one.join("src/a.rs")).unwrap(), changed, "a cópia fica como está");
        let warning = held_warning(&held, "round-return-needs-commit");
        assert_eq!(warning["wave"], json!(1), "{held}");
        let line = held_line(1, &warning["hint"]);
        assert!(held["next"].as_str().unwrap_or_default().contains(&line), "{held}");
        assert_eq!(waves_in(&held, "dispatch"), vec![3], "a 4 depende da 1: {held}");
        assert_eq!(waves_in(&held, "running"), vec![3], "a onda recusada não está em andamento: {held}");

        let again = json!({"wave": 1, "text": "Saiu.", "files": ["src/a.rs"], "commit": "a entrega 1 saiu"});
        assert_eq!(returned(root, again)["ok"], json!(true));
        let went = round(root, "x", None);
        assert_eq!(went["ok"], json!(true), "{went}");
        assert!(held_warning(&went, "round-return-needs-commit").is_null(), "{went}");
        assert_eq!(last_commit_files(root), "src/a.rs", "a volta gravada de novo entra: {went}");
        assert_eq!(official_deliveries(root), BTreeSet::from([1, 2]), "{went}");
        assert_eq!(std::fs::read_to_string(root.join("src/a.rs")).unwrap(), changed, "{went}");
        assert_eq!(waves_in(&went, "dispatch"), vec![4], "a onda que dependia dela sai: {went}");
    }

    /// A volta que a gravação não conferiu — de um binário antigo, ou de um
    /// plano que mudou depois dela — e que a rodada recusa ao lê-la segura só
    /// a própria onda, mesmo sozinha: a mudança de plano sem `undone`, a
    /// tarefa de outra onda em `undone` e o arquivo que não está no disco nem
    /// no git. A rodada responde sem recusar, com o aviso e o próximo passo da
    /// volta recusada; nada dela é gravado nem comitado, e a outra onda segue
    /// em andamento.
    #[test]
    fn a_return_the_round_refuses_on_reading_holds_only_its_wave_even_alone() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        for case in ["replan-needs-undone", "undone-not-in-wave", "round-file-unknown"] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
            assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 2], "{case}");
            let path = store::spec_file(root, "x").unwrap();
            let log = store::read(&path).unwrap().unwrap();
            let other = log.visible().into_iter().find(|e| e.event_type == "task" && e.wave() == Some(2)).unwrap().id;
            let other = log.codes()[&other].clone();
            let mut body = match case {
                "replan-needs-undone" => {
                    json!({"wave": 1, "text": "Parei.", "replan": "Dividir a onda."})
                }
                "undone-not-in-wave" => json!({"wave": 1, "text": "Parei.", "undone": [other]}),
                _ => {
                    json!({"wave": 1, "text": "Saiu.", "files": ["src/nao_existe.rs"], "commit": "a entrega 1 saiu"})
                }
            };
            body["returned"] = json!(true);
            body["author"] = json!("wave");
            store::write(&path, "delivered", body.as_object().cloned().unwrap(), &[]).unwrap();
            let head = git_text(root, &["rev-parse", "HEAD"]);

            let out = round(root, "x", None);
            assert_eq!(out["ok"], json!(true), "{case}: a recusa não vira a resposta: {out}");
            let warning = held_warning(&out, case);
            assert_eq!(warning["wave"], json!(1), "{case}: {out}");
            let line = held_line(1, &warning["hint"]);
            assert!(out["next"].as_str().unwrap_or_default().contains(&line), "{case}: {out}");
            assert_eq!(official_deliveries(root), BTreeSet::new(), "{case}: nada dela foi gravado: {out}");
            assert_eq!(git_text(root, &["rev-parse", "HEAD"]), head, "{case}: nada foi comitado: {out}");
            assert_eq!(waves_in(&out, "running"), vec![2], "{case}: só a onda 2 segue em andamento: {out}");
        }
    }

    /// O despacho de `prompt` ao agente `agent`, como o Claude Code o manda,
    /// pelo gancho do despacho.
    fn hook_dispatch(root: &Path, agent: &str, prompt: &str) -> mustard_core::domain::model::contract::Verdict {
        use crate::hooks::task::subagent_inject::SubagentInject;
        use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger};
        let input = HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            tool_name: Some("Agent".to_string()),
            tool_input: json!({ "prompt": prompt, "subagent_type": agent, "description": "onda" }),
            ..HookInput::default()
        };
        let mut ctx = Ctx::for_test(root.to_string_lossy().into_owned(), Some(Trigger::PreToolUse));
        ctx.config = mustard_core::ProjectConfig::load(root);
        SubagentInject.evaluate(&input, &ctx).expect("never errors")
    }

    /// O despacho da onda em andamento que traz, além do título e do comando
    /// de leitura que a rodada devolveu, um aviso do condutor sai só com o
    /// título e o comando, com o recado ao condutor de que o texto a mais
    /// saiu; o título segue na primeira linha. O despacho que já é esse texto
    /// passa como veio. O título da onda que ainda espera a outra é barrado.
    #[test]
    fn a_wave_dispatch_goes_out_with_only_the_title_and_the_read_command() {
        use mustard_core::domain::model::contract::Verdict;
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[1])]);
        // O despacho vem antes de o agente nascer: a rodada sem marcar o
        // lançamento, como o condutor a recebe.
        let opts = crate::commands::flow::round::RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None };
        let out = crate::commands::flow::round::round_for(&opts, None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        let read = out["dispatch"][0]["read"].as_str().unwrap_or_default().to_string();
        let title = mustard_core::domain::wave_prompt::wave_title("x", 1, Locale::PtBr);
        // O condutor escreve as duas linhas; o despacho leva, entre elas, a
        // ordem de rodar o comando como está.
        let two = format!("{title}\n\n{read}");
        let exact = crate::commands::flow::round::wave_dispatch(root, "x", 1, Locale::PtBr);
        // O lançamento sai sempre com o modelo e o esforço do `mustard.json`;
        // o que este teste guarda é o texto do pedido.
        let prompt_out = |verdict: Verdict, sent: &str| match verdict {
            Verdict::Allow => Some(sent.to_string()),
            Verdict::Rewrite { tool_input, note: None } => tool_input["prompt"].as_str().map(str::to_string),
            _ => None,
        };
        match hook_dispatch(root, "mustard-wave", &two) {
            Verdict::Rewrite { tool_input, note } => {
                assert_eq!(tool_input["prompt"], json!(exact), "the two lines lose nothing: the dispatch takes the whole text");
                assert_eq!(tool_input["subagent_type"], json!("mustard-wave"));
                assert_eq!(tool_input["description"], json!("onda"));
                assert_eq!(note, None, "no note for the two lines");
            }
            other => panic!("the dispatch is rewritten, got {other:?}"),
        }

        let extra = format!("{two}\n\nAviso do condutor: rode a suíte inteira antes de entregar.");
        match hook_dispatch(root, "mustard-wave", &extra) {
            Verdict::Rewrite { tool_input, note } => {
                assert_eq!(tool_input["prompt"], json!(exact), "só o título e o comando");
                assert_eq!(tool_input["subagent_type"], json!("mustard-wave"));
                assert_eq!(tool_input["description"], json!("onda"));
                assert!(note.is_some_and(|text| text.contains("instruções adicionais foram gravadas")));
                let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
                let sent = log.get(*log.last_by_wave("send").get(&1).unwrap()).unwrap();
                assert!(sent.str_field("text").unwrap().contains("rode a suíte inteira antes de entregar"));
            }
            other => panic!("the dispatch is rewritten, got {other:?}"),
        }
        assert_eq!(prompt_out(hook_dispatch(root, "mustard-wave", &exact), &exact), Some(exact.clone()));
        let trailing = format!("{exact}\n");
        assert_eq!(prompt_out(hook_dispatch(root, "mustard-wave", &trailing), &trailing), Some(trailing.clone()));

        let waiting = mustard_core::domain::wave_prompt::wave_title("x", 2, Locale::PtBr);
        let refused = translate("subagent.wave_not_running", Locale::PtBr).replace("{spec}", "x").replace("{wave}", "2");
        assert_eq!(hook_dispatch(root, "mustard-wave", &format!("{waiting}\n\n{read}")), Verdict::Deny { reason: refused });
    }

    /// A onda que pede novo plano e espera o clique já voltou: nem a página
    /// nem o pedido das outras ondas a mostram em andamento. Na página, ela
    /// não está em andamento, e a onda que saiu junto está; o pedido que a
    /// rodada gravou para a onda que saiu não a lista entre as ondas em
    /// andamento.
    #[test]
    fn the_page_and_the_other_requests_do_not_show_the_wave_waiting_for_the_click_in_progress() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[]), (3, &["src/c.rs"], &[2])]);
        round(root, "x", None);
        std::fs::write(copy_of(root, 2).join("src/b.rs"), "fn one() {}\n// a entrega 2 mudou\n").unwrap();
        let asks = json!({"wave": 1, "text": "Parei.", "replan": "A onda 1 precisa de outra tarefa antes.",
            "changes_decision": DECISION, "undone": []});
        assert_eq!(returned(root, asks)["ok"], json!(true));
        let done = json!({"wave": 2, "text": "Saiu.", "files": ["src/b.rs"], "commit": "a entrega 2 saiu"});
        assert_eq!(returned(root, done)["ok"], json!(true));

        let held = round(root, "x", None);
        assert_eq!(change_asked(&held)["wave"], json!(1), "{held}");
        assert_eq!(waves_in(&held, "dispatch"), vec![3], "{held}");

        let panel = crate::commands::panel::snapshot(root, Some("x"));
        let waves = panel["specs"][0]["waves"].as_array().unwrap();
        assert_ne!(waves.iter().find(|w| w["wave"] == 1).unwrap()["status"], "running", "waiting for authorization is not running");
        assert_eq!(waves.iter().find(|w| w["wave"] == 3).unwrap()["status"], "running");
        let recorded = request_of(&held, 3);
        let label = translate("prompt.execution.running", Locale::PtBr);
        assert!(!recorded.contains(label), "nenhuma outra onda em andamento: {recorded}");
    }

    /// A tarefa que a rodada devolve ao backlog duas vezes, sem mudança de
    /// plano, volta com a parte do agente que tinha: nenhuma linha se acumula
    /// a cada volta, nem a do resumo da onda que parou.
    #[test]
    fn a_task_returned_twice_without_a_plan_change_keeps_its_agent_part() {
        use crate::commands::flow::round::queue::{backlog_project, spec_now};

        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let agent = "Mexer em `src/a.rs`.\n- ler a função antes";
        let mut current = id_of(&write(
            root,
            "x",
            "task",
            json!({"text": "Mexer no código de um.", "agent": agent, "files": [{"path": "src/a.rs"}],
                "depends_on": [], "covers": [crit], "origin": said}),
        ));
        let path = store::spec_file(root, "x").unwrap();
        for wave in [1, 2] {
            let log = spec_now(root);
            let draft = undone_return(log.current(current).expect("the task"), wave, None, Locale::PtBr);
            assert_eq!(draft.get("agent"), Some(&json!(agent)), "return {wave}: {draft:?}");
            current = store::write(&path, "task", draft, &[]).unwrap().id;
        }
    }

    /// Numa onda que deixou A, B e C por fazer, a mudança de plano aceita que
    /// cita só A vai só a A, sem perguntar ao Jev; a que não cita tarefa vai
    /// só à que o classificador escolhe, e, sem escolha confiante, só à
    /// primeira: nunca às três.
    #[test]
    fn task_triage_sends_a_plan_change_only_to_the_tasks_it_concerns() {
        let log = mustard_core::domain::spec_events::parse_log(
            &(1..=3)
                .map(|id| json!({"v":1,"id":id,"type":"task","title":format!("T{id}"),"text":"t","files":[{"path":"src/a.rs"}],"depends_on":[]}).to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let undone: Vec<(u64, String)> = (1..=3).map(|id| (id, log.codes()[&id].clone())).collect();
        let (a, b) = (undone[0].1.as_str(), undone[1].1.as_str());
        let cited = format!("A {a} precisa de outra forma.");
        assert_eq!(change_targets(&log, &cited, &undone, &|_| panic!("a citação basta")), [a]);
        let chosen = |_: &str| Some(json!({"answers": {"i0": {"choice": "join:t2", "confidence": 0.9, "probabilities": {"join:t2": 0.9, "new": 0.05}}}}));
        assert_eq!(change_targets(&log, "O plano precisa de outra ordem.", &undone, &chosen), [b]);
        assert_eq!(change_targets(&log, "O plano precisa de outra ordem.", &undone, &|_| None), [a]);
    }

    /// A classificação de cada pergunta de escolha `choice`, com a certeza
    /// alta, como o Jev a dá.
    fn classified(payload: &str, choice: &str) -> Value {
        let body: Value = serde_json::from_str(payload).unwrap();
        let answers: Map<String, Value> = body["questions"]
            .as_object()
            .unwrap()
            .keys()
            .map(|key| (key.clone(), json!({"type": "choice", "choice": choice, "confidence": 0.95, "probabilities": {choice: 0.95, "insufficient": 0.05}})))
            .collect();
        json!({ "answers": answers })
    }

    /// As voltas da spec `x` de `root` que nenhuma rodada assumiu ainda.
    fn pending_returns(root: &Path) -> Vec<u64> {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        super::super::report::returned_waves(&log).0.iter().map(|wave| wave.wave).collect()
    }

    /// O aviso da volta sem mudança na resposta `out` da rodada.
    fn no_change_warning(out: &Value) -> Option<Value> {
        out["warnings"].as_array()?.iter().find(|w| w["reason"] == json!("no-change-return-needs-verification")).cloned()
    }

    /// A volta sem mudança nunca vai ao Jev com a trava do passo do git
    /// presa: ela fica fora desta rodada, a pergunta sai depois de a trava
    /// soltar, e a rodada seguinte a assume com a classificação gravada, sem
    /// perguntar de novo. O Jev que falha manda a pergunta à reserva, e a
    /// volta espera o recibo. Com o Jev desligado, a volta é assumida na
    /// mesma rodada, como sempre, sem classificação.
    #[test]
    fn a_no_change_return_waits_for_the_jev_outside_the_git_lock_and_the_next_round_applies_it() {
        for mode in ["jev", "reserve", "off"] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            approved(root, "x", &[(1, &["src/a.rs"], &[])]);
            round(root, "x", None);
            let back = json!({"wave": 1, "text": "Nada a mudar: o conserto já estava no projeto e a prova do critério passou."});
            assert_eq!(returned(root, back)["ok"], json!(true));
            let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::<bool>::new()));
            if mode != "off" {
                let (watched, answers) = (root.to_path_buf(), mode == "jev");
                let seen = std::rc::Rc::clone(&asked);
                crate::shared::jev::yes_no::ask_in_test(Some(std::rc::Rc::new(move |payloads: &[String]| {
                    if payloads.iter().any(|payload| payload.contains("return-triage-v1")) {
                        let free = mustard_core::io::fs::lock::LockedFile::exclusive_if_free(&watched.join(".claude/spec/round-git.lock")).unwrap().is_some();
                        seen.borrow_mut().push(free);
                    }
                    answers.then(|| payloads.iter().map(|payload| classified(payload, "validation-only")).collect())
                })));
            }
            let first = round(root, "x", None);
            if mode == "off" {
                assert!(pending_returns(root).is_empty(), "taken in the same round: {first}");
                assert_eq!(no_change_warning(&first).unwrap_or_else(|| panic!("{first}"))["reported_category"], json!("insufficient"));
                assert!(first.get("reserves").is_none(), "{first}");
                continue;
            }
            let waiting = first["warnings"].as_array().into_iter().flatten().find(|w| w["reason"] == json!("round-jev-decision-pending")).cloned();
            assert_eq!(waiting.map(|w| w["wave"].clone()), Some(json!(1)), "{first}");
            assert_eq!(pending_returns(root), vec![1], "the return waits for the decision: {first}");
            assert!(no_change_warning(&first).is_none(), "{first}");
            assert_eq!(asked.take(), vec![true], "one question, asked with the git-step lock free");
            if mode == "reserve" {
                let request = first["reserves"][0].clone();
                let prompt = request["delegation"]["request"]["prompt"].as_str().unwrap_or_else(|| panic!("{first}"));
                assert!(prompt.contains("What does the agent's summary in w1 report as the reason for no changes?"), "{prompt}");
                crate::shared::jev::reserve::answer_in_test(root, &request, |_| "validation-only".into(), "sure");
            }
            let second = round(root, "x", None);
            crate::shared::jev::yes_no::ask_in_test(None);
            assert!(asked.take().is_empty(), "the recorded decision answers in place of the Jev");
            assert!(pending_returns(root).is_empty(), "{second}");
            let warning = no_change_warning(&second).unwrap_or_else(|| panic!("{second}"));
            assert_eq!(warning["reported_category"], json!("validation-only"), "{mode}: {second}");
            assert_eq!(warning["verified_fact"], json!(false));
        }
    }
}
