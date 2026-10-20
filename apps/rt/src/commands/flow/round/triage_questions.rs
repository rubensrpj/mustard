//! A triagem das atividades que a rodada grava — o alvo da mudança de plano
//! que não cita tarefa, o item combinado que a onda ou a revisão final não
//! cumpriu e a sobra da volta — decidida antes do ensaio, sem chamar o Jev
//! com trava nenhuma presa. A pergunta de cada volta sai da spec como está
//! gravada e da própria volta ([`decide`]): com a trava do passo do git
//! presa, a rodada só lê a decisão já tomada — a resposta que uma rodada
//! anterior gravou, ou o recibo da reserva —, e a volta, ou o veredito
//! final, sem decisão fica fora desta rodada, e só ela. A pergunta sai
//! depois de soltas a trava do passo do git e a das cópias
//! ([`ask_after_locks`]), refeita sobre a spec como a rodada a deixou, com o
//! prazo de toda chamada; a que falha vai à reserva do Jev, e a volta espera
//! o recibo. Com o Jev desligado, nada espera: vale a regra de sempre.

use std::collections::BTreeSet;
use std::path::Path;

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::domain::spec_state::PhaseWriter;
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::Locale;
use serde_json::Value;

use super::answer::RoundRefusal;
use super::jev_decision::{Decisions, FINAL_REVIEW, JevOn, Judged, Question, Waiting, decided};
use super::report::{Report, VerdictReport, WaveReport, rehearse_verdicts, returned_verdict, returned_waves};
use super::stops::{HeldReturn, hold_waiting_changes};
use crate::commands::spec_events::write::RecordCheck;

/// A spec como está gravada, de onde sai a pergunta de cada volta, as ondas
/// cuja volta ainda não foi assumida, e as decisões já tomadas.
pub(super) struct Triage<'a> {
    pub(super) log: &'a SpecLog,
    pub(super) returning: BTreeSet<u64>,
    pub(super) decisions: Decisions,
}

impl<'a> Triage<'a> {
    /// A triagem lida em `log`, ainda sem decisão.
    fn of(log: &'a SpecLog) -> Self {
        let (waves, held) = returned_waves(log);
        let returning = waves.iter().flat_map(|wave| std::iter::once(wave.wave).chain(wave.fixes.iter().copied())).chain(held.iter().map(|one| one.wave)).collect();
        Self { log, returning, decisions: Decisions::default() }
    }

    /// A decisão da pergunta `question` da volta `wave`, feita pelo pedido
    /// `payload`, com o Jev ligado em `on`: a já tomada fica guardada; sem
    /// ela e com o Jev ligado, volta a espera.
    fn judge(&mut self, root: &Path, (wave, question): (u64, Question), payload: String, on: JevOn) -> Option<Waiting> {
        let payloads = vec![payload];
        match decided(root, self.log, question, &payloads, on) {
            Judged::Answered(docs) => {
                if let Some(doc) = docs.into_iter().next() {
                    self.decisions.keep(&payloads[0], doc);
                }
                None
            }
            Judged::Rule => None,
            Judged::Waiting => Some(Waiting { wave, question, payloads }),
        }
    }
}

/// As perguntas da triagem da volta `wave`, lidas em `triage`, cada uma com
/// o pedido: o alvo da mudança de plano aceita, os itens não cumpridos e as
/// sobras.
fn wave_questions(root: &Path, triage: &Triage<'_>, wave: &WaveReport) -> Vec<(Question, String)> {
    let mut out = Vec::new();
    if let Some(plan) = wave.replan.as_ref().filter(|plan| plan.refused.is_none())
        && let Some(question) = super::stops::change_target_question(triage.log, &plan.change, &wave.undone)
    {
        out.push((Question::ChangeTarget, question.payload));
    }
    if let Some((question, _)) = super::agreed::unmet_question(triage.log, &triage.returning, wave) {
        out.push((Question::UnmetItems, question.payload));
    }
    if let Some((question, _)) = super::leftovers::leftover_question(root, triage.log, &triage.returning, wave) {
        out.push((Question::Leftovers, question.payload));
    }
    out
}

/// O pedido da triagem dos itens que a revisão final de `verdicts` não
/// cumpriu, sobre a spec `spec` de `start` com o veredito já ensaiado, como
/// a rodada o ensaia; `None` sem veredito ou sem item.
fn review_question(start: &Path, spec: &str, verdicts: &[VerdictReport]) -> Option<String> {
    if verdicts.is_empty() {
        return None;
    }
    let mut check = RecordCheck::open(start, spec, PhaseWriter::Binary).ok()?;
    let (_, unmet) = rehearse_verdicts(&mut check, verdicts).ok()?;
    super::agreed::review_question(check.log(), &unmet).map(|question| question.payload)
}

/// Com a trava do passo do git presa: a decisão de cada pergunta da triagem
/// das voltas de `report` e do veredito final dele, lida na spec gravada
/// `log` de `root`, a da spec `spec` vista de `start`, com o Jev ligado em
/// `on`. Nenhuma chamada sai daqui. A volta com alguma pergunta sem decisão
/// sai de `report`, e o veredito final também, cada um com a recusa que diz
/// o que ele espera. Devolve a triagem com as decisões, para a rodada ler, a
/// recusa de cada volta que ficou fora e as perguntas a fazer depois das
/// travas.
pub(super) fn decide<'a>(start: &Path, root: &Path, spec: &str, log: &'a SpecLog, report: &mut Report, on: JevOn) -> (Triage<'a>, Vec<HeldReturn>, Vec<Waiting>) {
    let mut triage = Triage::of(log);
    let mut waiting = Vec::new();
    for wave in &report.waves {
        for (question, payload) in wave_questions(root, &triage, wave) {
            waiting.extend(triage.judge(root, (wave.wave, question), payload, on));
        }
    }
    if let Some(payload) = review_question(start, spec, &report.verdicts) {
        waiting.extend(triage.judge(root, (FINAL_REVIEW, Question::ReviewUnmet), payload, on));
    }
    let mut out: BTreeSet<u64> = BTreeSet::new();
    let mut held = Vec::new();
    for one in &waiting {
        if out.insert(one.wave) {
            held.push(HeldReturn { wave: one.wave, refusal: RoundRefusal::JevPending { wave: one.wave, question: one.question } });
        }
    }
    report.waves.retain(|wave| !out.contains(&wave.wave));
    if out.contains(&FINAL_REVIEW) {
        report.verdicts.clear();
    }
    (triage, held, waiting)
}

/// Depois de soltas a trava do passo do git e a das cópias: as perguntas de
/// `waiting` vão ao Jev ([`super::jev_decision::ask_outside`]), e a resposta
/// fica gravada para a rodada seguinte. As da triagem são refeitas antes
/// sobre a spec `spec` de `root` como a rodada a deixou — o que ela gravou
/// das outras voltas muda as tarefas abertas —, e só as que ainda não têm
/// decisão saem. Devolve os pedidos de reserva das que o Jev ligado não
/// respondeu.
pub(crate) fn ask_after_locks(start: &Path, root: &Path, spec: &str, waiting: &[Waiting], lang: Locale) -> Vec<Value> {
    if waiting.is_empty() {
        return Vec::new();
    }
    let waiting = refreshed(start, root, spec, waiting, lang);
    super::jev_decision::ask_outside(start, root, spec, &waiting)
}

/// As perguntas de `waiting`, com as da triagem refeitas sobre a spec
/// `spec` de `root` como está gravada agora, para as voltas e o veredito que
/// ficaram esperando: só as que ainda não têm decisão. Sem a spec, ficam
/// como vieram.
fn refreshed(start: &Path, root: &Path, spec: &str, waiting: &[Waiting], lang: Locale) -> Vec<Waiting> {
    let (triaged, mut out): (Vec<&Waiting>, Vec<&Waiting>) = waiting.iter().partition(|one| one.question.is_triage());
    let mut out: Vec<Waiting> = out.drain(..).cloned().collect();
    if triaged.is_empty() {
        return out;
    }
    let Some(log) = store::spec_file(root, spec).ok().and_then(|path| store::read(&path).ok().flatten()) else {
        out.extend(triaged.into_iter().cloned());
        return out;
    };
    let on = JevOn::read(root, spec);
    let mut triage = Triage::of(&log);
    let held: BTreeSet<u64> = triaged.iter().map(|one| one.wave).collect();
    let mut waves = returned_waves(&log).0;
    // A mudança de plano recusada pelo clique não pergunta o alvo, como na
    // rodada.
    let _ = hold_waiting_changes(&log, &mut waves, &[], lang);
    for wave in waves.iter().filter(|wave| held.contains(&wave.wave)) {
        for (question, payload) in wave_questions(root, &triage, wave) {
            out.extend(triage.judge(root, (wave.wave, question), payload, on));
        }
    }
    if held.contains(&FINAL_REVIEW)
        && let Some(payload) = review_question(start, spec, &returned_verdict(&log))
    {
        out.extend(triage.judge(root, (FINAL_REVIEW, Question::ReviewUnmet), payload, on));
    }
    out
}
