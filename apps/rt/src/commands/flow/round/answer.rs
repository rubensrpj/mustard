//! A resposta da rodada e o próximo passo: a recusa, com a mensagem no idioma
//! do projeto, e o caminho de uma chamada — conferir a fase, fechar o que
//! voltou, entregar ao orquestrador os candidatos da onda que ainda não tem
//! escolha, despachar as ondas prontas e dizer o que fazer em seguida. A
//! rodada não pede a revisão de onda nenhuma: quem confere o trabalho é o
//! agente de teste dedicado que o fechamento pede, uma vez por obra.
//!
//! A obra de até 3 pontos ([`crate::commands::flow::plan::is_solo_work`]), com
//! nota em cada tarefa, não vai para um agente: a rodada não cria cópia
//! separada, e o próximo passo manda o orquestrador fazer a onda na própria
//! janela, no checkout principal, com o mesmo pedido montado. A entrega dele
//! volta pela mesma linha `<DELIVERED>` de um agente, e a rodada grava e
//! comita como a de qualquer onda.

use std::collections::BTreeMap;
use std::path::Path;

use mustard_core::domain::spec_events::{Block, BlockQuery, Refusal, SpecLog, DELIVERED_MAX_CHARS};
use mustard_core::domain::spec_state::{PhaseWriter, SpecState, State};
use mustard_core::io::spec_events as store;
use mustard_core::io::wave_prompt::{prompts, recorded_copy, Flight};
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Map, Value};

use super::commit::git_lock;
use super::queue::{
    analyse, analysis_lines, first_unfinished, max_parallel, next_waves, only_analysis, open_copies, open_sends,
    orphaned_waves, sent_items, silent_minutes, touches_a_submodule, waves_in_progress, Analysed,
};
use super::report::Taken;
use super::stops::{change_question, stopped_waves, waves_stuck};
use super::{can_run, RoundOpts, DONE_STEP};
use crate::commands::spec_events::{read::checkout, write::record};
use crate::shared::spec_state::DiskSpecState;

/// Por que a rodada não correu.
pub(crate) enum RoundRefusal {
    /// Uma recusa do arquivo de eventos.
    Refused(Refusal),
    /// Uma linha do relatório não se entende.
    BadReport { detail: String },
    /// O relatório não traz nenhuma linha de entrega nem de veredito.
    LineMissing,
    /// Uma linha do relatório não traz um campo obrigatório.
    LineField { line: &'static str, field: &'static str },
    /// A entrega de uma onda conflita com o repositório principal: os
    /// trechos, a cópia em que se resolve e o commit atual.
    MergeConflict { wave: u64, copy: String, conflicts: Vec<String>, head: String },
    /// Um arquivo entregue não está no disco nem é conhecido do git.
    FileUnknown { file: String, wave: u64 },
    /// A spec ainda não foi aprovada.
    NotApproved { phase: String },
    /// O que uma onda entregou passa do teto de caracteres.
    DeliveredTooLong { wave: u64, chars: usize },
    /// A mensagem do commit não cabe no modelo.
    CommitTooLong { part: String, chars: usize, max: usize },
    /// A mensagem do commit traz o que ela nunca leva.
    CommitForbidden { found: String },
    /// Um agente disse que o plano da onda não funciona: a rodada para e
    /// mostra a mudança proposta, com a pergunta que decide.
    Replan { wave: u64, change: String, code: String },
    /// O git recusou o commit.
    Git { detail: String },
}

impl RoundRefusal {
    pub(crate) fn reason(&self) -> String {
        match self {
            Self::Refused(refusal) => refusal.reason().to_string(),
            Self::BadReport { .. } => "round-bad-report".into(),
            Self::LineMissing => "round-line-missing".into(),
            Self::LineField { .. } => "round-line-field-missing".into(),
            Self::MergeConflict { .. } => "round-merge-conflict".into(),
            Self::FileUnknown { .. } => "round-file-unknown".into(),
            Self::NotApproved { .. } => "round-not-approved".into(),
            Self::DeliveredTooLong { .. } => "delivered-too-long".into(),
            Self::CommitTooLong { .. } => "commit-too-long".into(),
            Self::CommitForbidden { .. } => "commit-forbidden-text".into(),
            Self::Replan { .. } => "wave-plan-does-not-work".into(),
            Self::Git { .. } => "git-refused".into(),
        }
    }

    pub(crate) fn message(&self, lang: Locale) -> String {
        let fill = |key: &str, slots: &[(&str, String)]| {
            slots.iter().fold(translate(key, lang).to_string(), |text, (slot, value)| text.replace(slot, value))
        };
        match self {
            Self::Refused(refusal) => refusal.message(lang),
            Self::BadReport { detail } => fill("round.bad_report", &[("{detail}", detail.clone())]),
            Self::LineMissing => fill("round.line_missing", &[]),
            Self::LineField { line, field } => {
                fill("round.line_field", &[("{line}", (*line).to_string()), ("{field}", (*field).to_string())])
            }
            Self::MergeConflict { wave, copy, conflicts, head } => fill(
                "round.merge_conflict",
                &[
                    ("{wave}", wave.to_string()),
                    ("{conflicts}", conflicts.join(", ")),
                    ("{copy}", copy.clone()),
                    ("{head}", head.clone()),
                ],
            ),
            Self::FileUnknown { file, wave } => {
                fill("round.file_unknown", &[("{file}", file.clone()), ("{wave}", wave.to_string())])
            }
            Self::NotApproved { phase } => fill("round.not_approved", &[("{phase}", phase.clone())]),
            Self::DeliveredTooLong { wave, chars } => fill(
                "round.delivered_too_long",
                &[
                    ("{wave}", wave.to_string()),
                    ("{chars}", chars.to_string()),
                    ("{max}", DELIVERED_MAX_CHARS.to_string()),
                ],
            ),
            Self::CommitTooLong { part, chars, max } => fill(
                "round.commit_too_long",
                &[("{part}", part.clone()), ("{chars}", chars.to_string()), ("{max}", max.to_string())],
            ),
            Self::CommitForbidden { found } => {
                fill("round.commit_forbidden", &[("{found}", found.clone())])
            }
            Self::Replan { wave, change, code } => fill(
                "round.replan",
                &[
                    ("{wave}", wave.to_string()),
                    ("{change}", change.clone()),
                    ("{question}", change_question(code, lang)),
                    ("{yes}", translate("change.accept", lang).to_string()),
                    ("{no}", translate("change.decline", lang).to_string()),
                ],
            ),
            Self::Git { detail } => fill("round.git_refused", &[("{detail}", detail.clone())]),
        }
    }

    pub(crate) fn to_value(&self, lang: Locale) -> Value {
        let mut out = json!({ "ok": false, "reason": self.reason(), "hint": self.message(lang) });
        // A pergunta da mudança vai pronta, com as opções, como a revisão de
        // um bloco do levantamento: é ela, e só ela, que a testemunha lê.
        if let Self::Replan { code, .. } = self {
            out["question"] = json!(change_question(code, lang));
            out["options"] = json!([translate("change.accept", lang), translate("change.decline", lang)]);
        }
        out
    }
}

/// As ondas a reenviar nesta rodada, cada uma com o número do pedido
/// anterior: as órfãs, de um Claude Code que fechou, e as pausadas por este
/// relatório — a onda pausada sai de novo na mesma rodada.
fn resend_targets(log: &SpecLog, paused: &[u64]) -> BTreeMap<u64, u64> {
    let last_sends = log.last_by_wave("send");
    let mut out = orphaned_waves(log);
    for wave in paused {
        if let Some(sent) = last_sends.get(wave) {
            out.entry(*wave).or_insert(*sent);
        }
    }
    out
}

/// O código ou o número de um campo que aponta outro evento (`item`, num
/// passo): o código quando o mapa o conhece, senão o próprio número.
fn ref_shown(value: Option<&Value>, codes: &BTreeMap<u64, String>) -> String {
    match value {
        Some(Value::String(code)) => code.clone(),
        Some(v) => v.as_u64().map_or_else(String::new, |n| codes.get(&n).cloned().unwrap_or_else(|| n.to_string())),
        None => String::new(),
    }
}

/// Os passos que a onda `wave` já gravou, uma linha por passo, pelo código do
/// item: o agente de onda os grava ao terminar uma tarefa e ao provar um
/// critério, e o reenvio os lista para o agente novo não repetir o já feito.
fn wave_steps(log: &SpecLog, wave: u64, codes: &BTreeMap<u64, String>) -> Vec<String> {
    log.block(BlockQuery::Block(Block::Waves))
        .into_iter()
        .filter(|e| e.event_type == "step" && e.wave() == Some(wave))
        .map(|e| format!("- {}: {}", ref_shown(e.fields.get("item"), codes), e.str_field("text").unwrap_or_default()))
        .collect()
}

/// O texto do reenvio: o pedido gravado no envio anterior, palavra por
/// palavra, mais os passos já gravados desta onda e o aviso de começar vendo
/// o que mudou na cópia. Sem passo nenhum, só o aviso.
fn resend_text(previous: &str, steps: &[String], lang: Locale) -> String {
    let mut parts = vec![previous.to_string()];
    if !steps.is_empty() {
        parts.push(translate("round.resume.steps", lang).to_string());
        parts.push(steps.join("\n"));
    }
    parts.push(translate("round.resume.notice", lang).to_string());
    parts.join("\n\n")
}

pub(super) fn run_round(
    opts: &RoundOpts,
    root: &Path,
    lang: Locale,
    session: Option<&str>,
) -> Result<Value, RoundRefusal> {
    run_round_with_mine(opts, root, lang, session, &|root, out| {
        mustard_core::Scan::locate().scan(root, out)
    })
}

/// [`run_round`] com quem relê o mapa depois do commit da rodada (`mine`),
/// que um teste escolhe sem instalar a ferramenta do scan de verdade.
pub(super) fn run_round_with_mine(
    opts: &RoundOpts,
    root: &Path,
    lang: Locale,
    session: Option<&str>,
    mine: &dyn Fn(&Path, &Path) -> mustard_core::platform::error::Result<mustard_core::domain::scan::ScanReport>,
) -> Result<Value, RoundRefusal> {
    let refuse = RoundRefusal::Refused;
    let spec = match opts.spec.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(spec) => spec.to_string(),
        None => DiskSpecState::new(&checkout(&opts.root))
            .active(session)
            .ok_or_else(|| refuse(Refusal::NoCurrentSpec))?,
    };
    let path = store::spec_file(root, &spec).map_err(RoundRefusal::Refused)?;
    let log = store::read(&path)
        .map_err(RoundRefusal::Refused)?
        .ok_or_else(|| RoundRefusal::Refused(Refusal::NoSpecFile { spec: spec.clone() }))?;

    // Só uma spec aprovada roda. Antes disso a rodada não tem o que despachar.
    let phase = State::from_log(&log).phase.unwrap_or_default().to_string();
    if !can_run(&phase) {
        return Err(RoundRefusal::NotApproved { phase });
    }

    // O relatório que só traz a escolha antes do envio não tem o que juntar:
    // a rodada vai direto ao despacho, com a escolha de cada onda.
    let raw = opts.report.as_deref().map(str::trim).filter(|r| !r.is_empty());
    let Taken { mut recorded, formatted, mut warnings, commit, paused } = match raw {
        Some(raw) if !only_analysis(raw) => {
            super::report::take_report_with_mine(&opts.root, root, &spec, raw, &log, lang, mine)?
        }
        _ => Taken { recorded: Vec::new(), formatted: Vec::new(), warnings: Vec::new(), commit: None, paused: Vec::new() },
    };
    let (given, unread) = analysis_lines(raw, lang);
    warnings.extend(unread);

    // Nada fica preso: todo processo que um agente deixou rodando — um laço
    // de espera, ou um comando na cópia de uma onda que o commit anterior já
    // apagou — é encerrado a cada rodada, e a resposta diz qual.
    let stuck_ended = crate::commands::flow::stuck::end_stuck_processes(root);
    if let Some(hint) = crate::commands::flow::stuck::report_line(&stuck_ended, lang) {
        warnings.push(json!({ "reason": "stuck-ended", "hint": hint }));
    }

    // O despacho — a entrada na execução, a leitura da spec, a escolha das
    // ondas, a criação das cópias e a gravação dos envios — roda inteiro com a
    // trava do passo do git presa: a rodada que chega ao mesmo tempo só lê a
    // spec depois dos envios desta, e nunca solta a mesma onda de novo.
    let held_lock = git_lock(root)?;
    // A primeira rodada leva a spec para a execução.
    let entering = phase == "approved"
        && crate::commands::spec_events::write::record_phase(&opts.root, &spec, "running", session);

    let log = store::read(&path)
        .map_err(RoundRefusal::Refused)?
        .ok_or_else(|| RoundRefusal::Refused(Refusal::NoSpecFile { spec: spec.clone() }))?;
    let codes = log.codes();

    // O despacho da rodada seguinte: as ondas prontas, no máximo o que o
    // projeto deixa compilar ao mesmo tempo contando as que já estão em
    // andamento, e nenhuma que a onda parada pelo limite de consertos segura.
    // Cada uma sai com a sua cópia e a sua pasta de compilação.
    let running = waves_in_progress(&log);
    // A órfã segue ocupando a cópia e a pasta de compilação dela até o
    // reenvio, mais abaixo: quem conta vaga livre e cópia livre soma as duas,
    // vivas e órfãs, e só a viva entra no "esperando" da resposta.
    let occupied = open_sends(&log);
    let stuck = waves_stuck(&log);
    let ready = next_waves(&log, max_parallel(root), &occupied, &stuck);
    // A escolha antes do envio, antes da cópia: a onda com item do projeto
    // todo, item sem dono ou lição a julgar só sai com a escolha do
    // orquestrador; sem ela, a resposta traz os candidatos dela, e a onda fica
    // para a rodada que trouxer a escolha.
    let Analysed { go, choices, asked, warnings: ignored } = analyse(root, &log, &ready, &given, lang);
    warnings.extend(ignored);
    // A obra de até 3 pontos, com nota em cada tarefa, fica com o
    // orquestrador: ele faz a onda na própria janela, no checkout principal,
    // sem cópia separada nem agente — a mesma soma de `plan::who_executes`,
    // pela mesma leitura, para as duas nunca discordarem de quem executa. A
    // que toca submódulo continua com a cópia, mesmo pequena: é ela quem põe
    // o submódulo na branch certa antes de editar.
    let solo = crate::commands::flow::plan::is_solo_work(&log) && !touches_a_submodule(root, &log, &go);
    let (copies, not_copied) = open_copies(root, &spec, &log, &held_lock, &go, &occupied, solo, lang);
    warnings.extend(not_copied);
    let next: Vec<u64> = if solo { go } else { go.into_iter().filter(|wave| copies.contains_key(wave)).collect() };
    // O pedido de cada onda lista as outras em andamento, contando as que
    // saem junto com ela nesta rodada, e traz a cópia dela e a escolha do
    // orquestrador.
    let flight = Flight { running: occupied.keys().chain(&next).copied().collect(), copies, choices };
    let built = prompts(root, &spec, &log, lang, &flight);
    let mut dispatched: Vec<Value> = Vec::new();
    let mut in_flight: BTreeMap<u64, String> = running
        .iter()
        .map(|(wave, sent)| (*wave, codes.get(sent).cloned().unwrap_or_else(|| sent.to_string())))
        .collect();
    // O processo do Claude Code por trás desta rodada: gravado em todo envio,
    // novo ou reenviado, para a rodada seguinte saber se este ainda está
    // aberto. Sem achar um (fora do Linux, ou sem pai de nome `claude`), o
    // envio sai sem o par, e conta como fechado só onde isso faz sentido.
    let claude = crate::commands::flow::stuck::claude_ancestor();
    for wave in &next {
        let Some(prompt) = built.iter().find(|p| p.wave == *wave) else { continue };
        let mut draft = Map::new();
        draft.insert("wave".into(), json!(wave));
        draft.insert("role".into(), json!("wave"));
        draft.insert("text".into(), json!(prompt.text));
        draft.insert("lines".into(), json!(prompt.lines));
        draft.insert("chars".into(), json!(prompt.text.chars().count()));
        // Os itens que ficaram, e à parte a escolha do orquestrador: o que
        // saiu e o que entrou, cada um com o motivo.
        draft.insert("items".into(), json!(sent_items(&log, *wave, flight.choices.get(wave))));
        if let Some(choice) = flight.choices.get(wave) {
            draft.insert("analysis".into(), choice.to_value());
        }
        draft.insert("mustard".into(), json!(env!("CARGO_PKG_VERSION")));
        if let Some(copy) = flight.copies.get(wave) {
            draft.insert("copy".into(), json!(copy.path));
            if let Some(dir) = &copy.build_dir {
                draft.insert("build_dir".into(), json!(dir));
            }
        }
        if let Some((pid, started)) = claude {
            draft.insert("claude_pid".into(), json!(pid));
            draft.insert("claude_started".into(), json!(started));
        }
        draft.insert("author".into(), json!("binary"));
        let written = record(&opts.root, &spec, "send", draft, PhaseWriter::Binary)
            .map_err(RoundRefusal::Refused)?;
        recorded.push(json!({ "wave": wave, "type": "send", "id": written.written.id }));
        dispatched.push(json!({ "wave": wave, "lines": prompt.lines, "prompt": prompt.text }));
        let code = written.written.code.clone().unwrap_or_else(|| written.written.id.to_string());
        in_flight.insert(*wave, code);
    }
    // O reenvio: a onda pausada por este relatório, ou a órfã de um Claude
    // Code que fechou, sai de novo com o pedido gravado no envio anterior,
    // palavra por palavra, na mesma cópia e na mesma pasta de compilação — sem
    // montar o pedido de novo —, mais os passos já gravados e o aviso de
    // começar vendo o que mudou na cópia. O envio novo aponta o anterior.
    for (wave, previous) in resend_targets(&log, &paused) {
        let Some(prior) = log.get(previous) else { continue };
        let Some(copy) = recorded_copy(&log, wave) else { continue };
        let steps = wave_steps(&log, wave, &codes);
        let mut draft = Map::new();
        draft.insert("wave".into(), json!(wave));
        draft.insert("role".into(), json!("wave"));
        let text = resend_text(prior.str_field("text").unwrap_or_default(), &steps, lang);
        draft.insert("chars".into(), json!(text.chars().count()));
        draft.insert("lines".into(), json!(text.lines().count()));
        draft.insert("text".into(), json!(text));
        draft.insert("items".into(), prior.fields.get("items").cloned().unwrap_or_else(|| json!([])));
        draft.insert("mustard".into(), json!(env!("CARGO_PKG_VERSION")));
        draft.insert("copy".into(), json!(copy.path));
        if let Some(dir) = &copy.build_dir {
            draft.insert("build_dir".into(), json!(dir));
        }
        draft.insert("resends".into(), json!(previous));
        if let Some((pid, started)) = claude {
            draft.insert("claude_pid".into(), json!(pid));
            draft.insert("claude_started".into(), json!(started));
        }
        draft.insert("author".into(), json!("binary"));
        let written = record(&opts.root, &spec, "send", draft, PhaseWriter::Binary)
            .map_err(RoundRefusal::Refused)?;
        recorded.push(json!({ "wave": wave, "type": "send", "id": written.written.id }));
        dispatched.push(json!({ "wave": wave, "lines": text.lines().count(), "prompt": text }));
        let code = written.written.code.clone().unwrap_or_else(|| written.written.id.to_string());
        in_flight.insert(wave, code);
    }
    drop(held_lock);

    // O sinal de vida: a onda em andamento, viva, sem nenhuma ação gravada
    // (pelo observador da cópia, ou, sem ela, a hora do próprio envio) há mais
    // de 40 minutos, sai como aviso — a pausada agora não conta, porque acabou
    // de dar sinal.
    for (wave, sent) in running.iter().filter(|(wave, _)| !paused.contains(wave)) {
        let Some(minutes) = silent_minutes(root, &spec, *wave, &log, *sent) else {
            continue;
        };
        if minutes >= 40 {
            warnings.push(json!({
                "reason": "wave-silent",
                "wave": wave,
                "hint": translate("round.resume.silent", lang).replace("{wave}", &wave.to_string()),
            }));
        }
    }

    // O próximo passo: despachar o que saiu agora; esperar as que estão em
    // andamento; fechar, com tudo entregue e aprovado; ou dizer qual onda
    // falta, quando nada se move. A rodada não pede revisão de onda nenhuma:
    // quem confere o trabalho é o agente de teste dedicado que o fechamento
    // pede, uma vez por obra.
    let report_back = translate("round.report", lang);
    let mut command: Option<String> = None;
    let then = if !dispatched.is_empty() {
        let next_key = if solo { "round.next.solo" } else { "round.next" };
        format!("{} {report_back}", translate(next_key, lang))
    } else if !asked.is_empty() {
        String::new()
    } else if !running.is_empty() {
        let waves: Vec<String> = running.keys().map(u64::to_string).collect();
        format!("{} {report_back}", translate("round.waiting", lang).replace("{waves}", &waves.join(", ")))
    } else if !stuck.is_empty() {
        String::new()
    } else if let Some(wave) = first_unfinished(&log, &running) {
        translate("round.missing", lang).replace("{wave}", &wave.to_string())
    } else {
        let state = State::from_log(&log);
        let close = crate::commands::flow::resume::step_command(DONE_STEP, &spec, &state);
        let text = translate("round.close", lang).replace("{command}", close.as_deref().unwrap_or_default());
        command = close;
        text
    };
    // A pergunta da onda parada vem antes do resto, que segue sem ela; o
    // pedido da escolha vem logo depois.
    let (stopped, question) = stopped_waves(&stuck, &codes, lang);
    let waiting: Vec<String> = asked.iter().filter_map(|a| a["wave"].as_u64()).map(|n| n.to_string()).collect();
    let analysis = (!asked.is_empty()).then(|| translate("round.analysis", lang).replace("{waves}", &waiting.join(", ")));
    let then = question
        .into_iter()
        .chain(analysis)
        .chain(Some(then).filter(|t| !t.is_empty()))
        .collect::<Vec<_>>()
        .join(" ");

    // A cópia para o banco da página sai no fim do passo, uma vez, e a rodada
    // manda copiá-la, menos quando ela não pôde ser preparada.
    let prepared = crate::commands::spec_events::pages::copy::prepare_milestone(root, &spec, lang);

    // Com o pull request aberto, o corpo dele é refeito aqui: ele é montado do
    // mesmo arquivo de eventos que acabou de mudar, e um corpo que descreve a
    // rodada anterior é pior do que nenhum — foi por isso que existiu um portão
    // só para reparar que ele tinha envelhecido.
    let rewritten = rewrite_open_pr(root, &spec);

    let running: Vec<Value> =
        in_flight.iter().map(|(wave, send)| json!({ "wave": wave, "send": send })).collect();
    let mut out = json!({
        "ok": true,
        "spec": spec,
        "recorded": recorded,
        "formatted": formatted,
        "dispatch": dispatched,
        "running": running,
    });
    if entering {
        out["phase"] = json!("running");
    }
    if !stopped.is_empty() {
        out["stopped"] = json!(stopped);
    }
    if !asked.is_empty() {
        out["analysis"] = json!(asked);
    }
    if let Some(commit) = commit {
        out["commit"] = commit;
    }
    if !warnings.is_empty() {
        out["warnings"] = json!(warnings);
    }
    crate::commands::spec_events::pages::end_milestone(&mut out, prepared.as_ref(), &spec, "round", &then, lang);
    if let Some(command) = command {
        out["command"] = json!(command);
    }
    if let Some(number) = rewritten {
        out["pr"] = json!({ "number": number, "body": "rewritten" });
    }
    Ok(out)
}

/// Refaz o corpo do pull request desta spec, quando há um aberto. Devolve o
/// número do pull request reescrito, `None` quando não há nenhum ou quando o
/// provedor não respondeu — a rodada nunca para por causa disso.
///
/// O pull request é o da branch DESTA spec, não o da branch em que o checkout
/// está. Fora da branch da spec não há o que refazer aqui, e perguntar pelo
/// checkout reescreveria o corpo do pull request de outra unidade.
fn rewrite_open_pr(root: &Path, spec: &str) -> Option<u64> {
    let branch = crate::commands::spec_events::write::branch_of_spec(root, spec)?;
    let (_, body) = crate::commands::review::pr_publish::message_of(root, spec).ok()?;
    let provider = crate::shared::pr_provider::provider_for(root);
    crate::commands::review::pr_publish::rewrite_body(provider.as_ref(), &branch, &body)
}

#[cfg(test)]
mod tests {
    use mustard_core::domain::spec_events::SpecEvent;
    use tempfile::tempdir;

    use crate::commands::spec_events::write::record_open;

    use super::*;
    use crate::commands::flow::round::tests::*;

    /// A spec que ainda não foi aprovada não roda onda nenhuma.
    #[test]
    fn a_spec_that_is_not_approved_yet_dispatches_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        assert_eq!(record_open(root, "x", "feature/x", "dev"), Ok(true));
        let refused = round(root, "x", None);
        assert_eq!(refused["reason"], json!("round-not-approved"), "{refused}");
    }

    /// A primeira rodada leva a spec para a execução, grava o envio de cada
    /// onda com o pedido exato e devolve o pedido pronto para injetar.
    #[test]
    fn the_first_round_records_what_it_injected_and_marks_the_spec_running() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);

        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["phase"], json!("running"), "{out}");
        let dispatched = out["dispatch"].as_array().cloned().unwrap_or_default();
        assert_eq!(dispatched.len(), 1, "{out}");
        let prompt = dispatched[0]["prompt"].as_str().unwrap_or_default().to_string();
        // A lista só com os códigos, e o comando de leitura uma vez só, no
        // exemplo, com o caminho do repositório principal: a onda trabalha na
        // cópia que a rodada criou.
        assert!(prompt.lines().any(|l| l.starts_with("- `waves`: ") && l.contains("MSTD-TASK-0001")), "{prompt}");
        let example = translate("prompt.read", Locale::PtBr)
            .replace("{root}", &format!("--root {} ", mustard_core::io::wave_prompt::shown(root)))
            .replace("{spec}", "x");
        assert!(prompt.contains(&example), "{prompt}");
        assert_eq!(prompt.matches("mustard-rt run read").count(), 1, "{prompt}");

        // O envio gravado guarda o pedido exato, letra por letra.
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let sent: Vec<&SpecEvent> =
            log.visible().into_iter().filter(|e| e.event_type == "send").collect();
        assert_eq!(sent.len(), 1, "um envio por onda despachada");
        assert_eq!(sent[0].str_field("text"), Some(prompt.as_str()));
        assert_eq!(sent[0].wave(), Some(1));
    }

    /// Os campos de um envio já gravado, prontos para virar a base de um novo
    /// (mesmo texto, mesmos itens, mesma cópia): quem chama troca só o que
    /// precisa.
    fn resend_draft(sent: &SpecEvent) -> Value {
        json!({
            "wave": sent.wave().unwrap(),
            "role": "wave",
            "text": sent.str_field("text").unwrap_or_default(),
            "lines": sent.int("lines").unwrap_or(1),
            "chars": sent.int("chars").unwrap_or(1),
            "items": sent.fields.get("items").cloned().unwrap_or_else(|| json!([])),
            "mustard": "0",
            "copy": sent.str_field("copy").unwrap_or_default(),
        })
    }

    /// Grava um envio à mão, com a hora `at`: supera o envio mais novo da
    /// mesma onda, porque a leitura pega sempre o de maior número.
    fn seed_send_at(root: &Path, draft: Value, at: &str) {
        let path = store::spec_file(root, "x").unwrap();
        store::write_at(&path, "send", draft.as_object().cloned().unwrap(), &[], at).unwrap();
    }

    /// O passo que o agente grava (`MSTD-TASK-0041`); a onda pausada e a
    /// órfã, de um Claude Code que fechou, reenviam o pedido de antes,
    /// palavra por palavra, com os passos e o aviso, e o envio novo aponta o
    /// anterior; a onda de um Claude Code ainda aberto — mesmo depois de um
    /// `/clear`, que não muda o processo do sistema — não é reenviada; e a
    /// onda viva sem sinal por 40 minutos sai como aviso, enquanto a de 39
    /// minutos não sai (`MSTD-TASK-0042`, `MSTD-CRIT-0031`).
    #[test]
    fn the_wave_resumes_from_its_steps_in_a_new_agent() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(
            root,
            "x",
            &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[]), (3, &["src/c.rs"], &[]), (4, &["src/d.rs"], &[])],
        );
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":10}"#).unwrap();

        let first = round(root, "x", None);
        let mut first_out = waves_in(&first, "dispatch");
        first_out.sort_unstable();
        assert_eq!(first_out, vec![1, 2, 3, 4], "{first}");
        let first_prompt = first["dispatch"][0]["prompt"].as_str().unwrap_or_default().to_string();

        let path = store::spec_file(root, "x").unwrap();
        let claude = crate::commands::flow::stuck::claude_ancestor();
        let (draft2, draft3, draft4) = {
            let log = store::read(&path).unwrap().unwrap();
            let sent_of = |wave: u64| -> Value {
                resend_draft(log.visible().into_iter().find(|e| e.wave() == Some(wave) && e.event_type == "send").unwrap())
            };
            (sent_of(2), sent_of(3), sent_of(4))
        };

        // O agente grava um passo ao terminar a tarefa da onda 1.
        write(root, "x", "step", json!({"wave": 1, "item": "MSTD-TASK-0001", "text": "A tarefa 1 ficou pronta."}));

        // A onda 2 é órfã: o Claude Code dela fechou — um processo nascido e
        // já colhido nunca mais aparece com a mesma hora de início.
        let mut dead = std::process::Command::new("true").spawn().expect("spawn the fixture process");
        let dead_pid = dead.id();
        dead.wait().expect("reap the fixture process");
        let mut draft2 = draft2;
        draft2["claude_pid"] = json!(dead_pid);
        draft2["claude_started"] = json!(1);
        seed_send_at(root, draft2, &chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string());

        // As ondas 3 e 4 seguem com o mesmo Claude Code, vivo de verdade, mas
        // sem sinal de vida há 39 e 40 minutos.
        for (mut draft, minutes_ago) in [(draft3, 39), (draft4, 40)] {
            if let Some((pid, started)) = claude {
                draft["claude_pid"] = json!(pid);
                draft["claude_started"] = json!(started);
            }
            let at = (chrono::Local::now() - chrono::Duration::minutes(minutes_ago)).to_rfc3339();
            seed_send_at(root, draft, &at);
        }

        // A pausa e a órfã saem de novo; a onda 1 (viva) e a 3 (39 minutos)
        // não geram aviso; a onda 4 (40 minutos) gera.
        let paused = line("PAUSED", json!({"wave": 1}));
        let out = round(root, "x", Some(&paused));
        assert_eq!(out["ok"], json!(true), "{out}");
        let mut resent = waves_in(&out, "dispatch");
        resent.sort_unstable();
        assert_eq!(resent, vec![1, 2], "só a pausada e a órfã saem de novo: {out}");

        let prompt_of = |wave: u64| -> String {
            out["dispatch"]
                .as_array()
                .unwrap()
                .iter()
                .find(|d| d["wave"].as_u64() == Some(wave))
                .and_then(|d| d["prompt"].as_str())
                .unwrap_or_default()
                .to_string()
        };
        let notice = translate("round.resume.notice", Locale::PtBr);
        let wave1_prompt = prompt_of(1);
        assert!(wave1_prompt.starts_with(&first_prompt), "o pedido de antes volta palavra por palavra: {wave1_prompt}");
        assert!(
            wave1_prompt.contains("MSTD-TASK-0001") && wave1_prompt.contains("A tarefa 1 ficou pronta."),
            "{wave1_prompt}"
        );
        assert!(wave1_prompt.contains(notice), "{wave1_prompt}");
        let wave2_prompt = prompt_of(2);
        assert!(wave2_prompt.contains(notice) && !wave2_prompt.contains("Passos já gravados"), "{wave2_prompt}");

        // O envio novo da onda 1 aponta o anterior.
        let log = store::read(&path).unwrap().unwrap();
        let sends_of_1: Vec<&SpecEvent> =
            log.visible().into_iter().filter(|e| e.event_type == "send" && e.wave() == Some(1)).collect();
        let (previous, resent_send) = (sends_of_1[sends_of_1.len() - 2], sends_of_1[sends_of_1.len() - 1]);
        assert_eq!(resent_send.int("resends"), Some(previous.id), "{out}");

        // Só a onda 4 (40 minutos) sai como aviso.
        let warnings = out["warnings"].as_array().cloned().unwrap_or_default();
        let stale: Vec<u64> =
            warnings.iter().filter(|w| w["reason"] == json!("wave-silent")).filter_map(|w| w["wave"].as_u64()).collect();
        assert_eq!(stale, vec![4], "{warnings:?}");
        let hint = warnings.iter().find(|w| w["wave"].as_u64() == Some(4)).unwrap()["hint"].as_str().unwrap_or_default();
        assert!(hint.contains('4') && hint.contains("PAUSED"), "{hint}");

        // A onda 3, com o mesmo Claude Code ainda vivo — mesmo depois de um
        // `/clear` simulado, que não muda o processo do sistema —, segue em
        // andamento, e a rodada seguinte não a reenvia nem a 1, recém-saída.
        let waiting = round(root, "x", None);
        assert!(waves_in(&waiting, "dispatch").is_empty(), "{waiting}");
    }

    /// A onda órfã segue ocupando a vaga e a cópia dela até o reenvio: com o
    /// teto de compilação em 1, uma onda fresca não sai por cima da órfã na
    /// mesma rodada em que ela é reenviada.
    #[test]
    fn an_orphaned_wave_keeps_holding_its_slot_until_it_is_resent() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":1}"#).unwrap();

        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1], "só uma vaga, só a onda 1 sai: {first}");

        let draft1 = {
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            let sent = log.visible().into_iter().find(|e| e.wave() == Some(1) && e.event_type == "send").unwrap();
            resend_draft(sent)
        };
        let mut dead = std::process::Command::new("true").spawn().expect("spawn the fixture process");
        let dead_pid = dead.id();
        dead.wait().expect("reap the fixture process");
        let mut draft1 = draft1;
        draft1["claude_pid"] = json!(dead_pid);
        draft1["claude_started"] = json!(1);
        seed_send_at(root, draft1, &chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string());

        let second = round(root, "x", None);
        assert_eq!(waves_in(&second, "dispatch"), vec![1], "a onda 2 não usa a vaga da órfã: {second}");
    }

    /// A tarefa da onda `wave` da spec `x` ganha (ou troca) a nota `points`:
    /// uma versão nova, com a mesma origem, que substitui a mais nova dela.
    fn rate_task(root: &Path, wave: u64, points: u64) {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let task = log
            .visible()
            .into_iter()
            .find(|e| e.event_type == "task" && e.wave() == Some(wave))
            .unwrap_or_else(|| panic!("sem tarefa da onda {wave}"));
        let mut fields = task.fields.clone();
        for key in ["v", "id", "code", "at", "search", "type", "author"] {
            fields.remove(key);
        }
        fields.insert("points".into(), json!(points));
        fields.insert("replaces".into(), json!(task.id));
        write(root, "x", "task", Value::Object(fields));
    }

    /// A obra de até 3 pontos, numa onda só, com nota na tarefa dela, fica com
    /// o orquestrador: a rodada não cria cópia nenhuma, o próximo passo manda
    /// fazer a onda na própria janela, no checkout principal — sem falar do
    /// agente `mustard-wave` —, e o envio gravado não leva cópia. A entrega
    /// dele, pela mesma linha `<DELIVERED>` de um agente, é gravada e
    /// comitada como a de qualquer onda, e a obra termina mandando fechar. Na
    /// divisa: com 4 pontos, um a mais que o teto, a rodada volta a criar a
    /// cópia e a pedir o agente, como antes; e com os mesmos 3 pontos (1 + 2),
    /// mas já em duas ondas, cada uma também volta a ganhar cópia e agente —
    /// o orquestrador só fica com a obra de uma onda só.
    #[test]
    fn the_orchestrator_does_a_work_of_up_to_three_points() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        rate_task(root, 1, 3);

        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        let dispatched = out["dispatch"].as_array().cloned().unwrap_or_default();
        assert_eq!(dispatched.len(), 1, "{out}");
        assert!(dispatched[0]["prompt"].as_str().is_some_and(|p| !p.is_empty()), "{out}");
        assert!(
            !mustard_core::io::wave_prompt::copy_path(root, "x", 1, false).exists(),
            "a obra pequena não ganha cópia separada"
        );
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let sent = log.visible().into_iter().find(|e| e.event_type == "send").unwrap();
        assert!(sent.fields.get("copy").is_none(), "{:?}", sent.fields);

        let solo = translate("round.next.solo", Locale::PtBr);
        let report_back = translate("round.report", Locale::PtBr);
        assert!(out["next"].as_str().unwrap_or_default().ends_with(&format!("{solo} {report_back}")), "{out}");
        assert!(!out["next"].as_str().unwrap_or_default().contains("mustard-wave"), "{out}");

        // A entrega, sem cópia nenhuma para juntar, vira commit e a obra
        // termina mandando fechar, como qualquer onda.
        let done = round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        assert_eq!(done["ok"], json!(true), "{done}");
        assert_eq!(done["command"], json!("mustard-rt run close --spec x"), "{done}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert_eq!(log.visible().iter().filter(|e| e.event_type == "delivered").count(), 1, "{done}");

        // Na divisa: 4 pontos, um a mais que o teto do orquestrador, voltam a
        // pedir a cópia separada e o agente da onda.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        rate_task(root, 1, 4);

        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        assert!(mustard_core::io::wave_prompt::copy_path(root, "x", 1, false).join(".git").is_file(), "{out}");
        let normal = translate("round.next", Locale::PtBr);
        assert!(out["next"].as_str().unwrap_or_default().ends_with(&format!("{normal} {report_back}")), "{out}");
        assert!(out["next"].as_str().unwrap_or_default().contains("mustard-wave"), "{out}");

        // Na outra divisa: os mesmos 3 pontos, mas em duas ondas (1 + 2). O
        // orquestrador só faz a obra de uma onda só; com duas, cada uma volta
        // a sair numa cópia separada, para o agente da onda.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        rate_task(root, 1, 1);
        rate_task(root, 2, 2);

        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1, 2], "{out}");
        for wave in [1, 2] {
            assert!(
                mustard_core::io::wave_prompt::copy_path(root, "x", wave, false).join(".git").is_file(),
                "a onda {wave} ganha cópia: {out}"
            );
        }
        assert!(out["next"].as_str().unwrap_or_default().ends_with(&format!("{normal} {report_back}")), "{out}");
        assert!(out["next"].as_str().unwrap_or_default().contains("mustard-wave"), "{out}");
    }

    /// O pedido da onda nova traz os comandos do projeto e a outra onda que
    /// sai junto, com o arquivo dela. O do conserto traz também o veredito,
    /// a entrega anterior e a decisão gravada depois do envio. Entregue o
    /// conserto, a rodada não pede revisão nenhuma dele: a resposta não traz
    /// o campo `reviews`, e a onda 1 sai da fila sem veredito novo.
    #[test]
    fn the_round_assembles_the_new_request_the_fix_request_and_its_review() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"buildCommand":"make","testCommand":"make test"}"#).unwrap();
        let text = |out: &Value, field: &str, wave: u64| -> String {
            let found = out[field].as_array().into_iter().flatten().find(|d| d["wave"] == json!(wave));
            found.and_then(|d| d["prompt"].as_str()).unwrap_or_default().to_string()
        };
        let first = text(&round(root, "x", None), "dispatch", 1);
        for line in ["- Compile com `make`.", "- Teste com `make test`.", "  - Onda 2: `src/b.rs`"] {
            assert!(first.contains(line), "{line}: {first}");
        }
        assert!(!first.contains("## Conserto"), "{first}");

        round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        write(root, "x", "decision", json!({"author": "user", "text": "A soma aceita negativos.", "keys": ["soma"],
            "why": "o usuário pediu", "waves": [1]}));
        let fix = text(&round(root, "x", Some(&verdict(1, "rejected", "faltou o teste"))), "dispatch", 1);
        let heading = format!("## Conserto\n\n{}", translate("prompt.fix.wave", Locale::PtBr));
        assert!(fix.contains(&heading), "{fix}");
        let fix_lines = |text: &str| -> Vec<String> {
            let part = text.split("\n## ").nth(1).unwrap_or_default();
            part.lines().filter(|l| l.starts_with("- ")).map(str::to_string).collect()
        };
        let lines = fix_lines(&fix);
        assert_eq!(lines[..2], ["- `review`: MSTD-VERD-0001", "- `waves`: MSTD-DELIV-0001"], "{fix}");
        assert!(lines[2].starts_with("- `agreed`: ") && lines[2].contains("MSTD-DEC-0001"), "{fix}");

        let back = round(root, "x", Some(&delivered(root, 1, "Teste acrescentado.", &["src/a.rs"])));
        assert!(back.get("reviews").is_none(), "a rodada não pede revisão do conserto: {back}");
        assert_eq!(waves_in(&back, "dispatch"), Vec::<u64>::new(), "{back}");
    }

    /// A rodada diz o próximo passo de cada situação: despachar o que saiu;
    /// esperar as ondas em andamento; dizer qual onda falta quando nada se
    /// move; e, com todas as ondas entregues, fechar — com a linha do
    /// fechamento pronta, que o binário aceita. A rodada não pede revisão de
    /// onda nenhuma: a entrega já basta, sem esperar veredito.
    #[test]
    fn the_round_answers_close_when_every_wave_is_approved_and_names_the_missing_one() {
        let report_back = translate("round.report", Locale::PtBr);

        // A cópia da única onda não pode ser criada: nada sai, o aviso diz por
        // quê, e a resposta diz que falta a onda 1. Desfeito o impedimento, a
        // onda sai na rodada seguinte.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let blocked = mustard_core::io::wave_prompt::copy_path(root, "x", 1, false);
        std::fs::create_dir_all(blocked.parent().unwrap()).unwrap();
        std::fs::write(&blocked, b"no caminho da copia").unwrap();
        let held = round(root, "x", None);
        assert_eq!(held["ok"], json!(true), "{held}");
        assert_eq!(waves_in(&held, "dispatch"), Vec::<u64>::new(), "{held}");
        assert_eq!(held["running"], json!([]), "{held}");
        let warnings = held["warnings"].as_array().cloned().unwrap_or_default();
        assert_eq!(warnings.len(), 1, "{held}");
        assert_eq!((&warnings[0]["reason"], &warnings[0]["wave"]), (&json!("copy-not-created"), &json!(1)), "{held}");
        let failed = translate("round.copy_failed", Locale::PtBr).replace("{wave}", "1");
        let (before, after) = failed.split_once("{detail}").unwrap();
        let hint = warnings[0]["hint"].as_str().unwrap_or_default();
        assert!(hint.starts_with(before) && hint.ends_with(after) && hint.len() > failed.len(), "{hint}");
        let missing = translate("round.missing", Locale::PtBr).replace("{wave}", "1");
        assert!(held["next"].as_str().unwrap_or_default().ends_with(&missing), "{held}");
        assert!(held.get("command").is_none(), "{held}");
        std::fs::remove_file(&blocked).unwrap();
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1]);

        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);

        let first = round(root, "x", None);
        let next = first["next"].as_str().unwrap_or_default();
        assert!(next.ends_with(&format!("{} {report_back}", translate("round.next", Locale::PtBr))), "{first}");
        assert!(first.get("command").is_none(), "{first}");

        let waiting = round(root, "x", None);
        let next = waiting["next"].as_str().unwrap_or_default();
        let expected = translate("round.waiting", Locale::PtBr).replace("{waves}", "1, 2");
        assert!(next.ends_with(&format!("{expected} {report_back}")), "{waiting}");
        assert!(waiting.get("command").is_none(), "{waiting}");

        // As duas entregam: a rodada não pede revisão nenhuma, e a entrega já
        // basta para dizer que a obra terminou.
        let both = format!("{}\n{}", delivered(root, 1, "Saiu.", &["src/a.rs"]), delivered(root, 2, "Saiu.", &["src/b.rs"]));
        let done = round(root, "x", Some(&both));
        assert!(done.get("reviews").is_none(), "{done}");
        assert_eq!(done["ok"], json!(true), "{done}");
        let command = done["command"].as_str().unwrap_or_default();
        assert_eq!(command, "mustard-rt run close --spec x", "{done}");
        crate::commands::flow::resume::assert_parses(command);
        let close = translate("round.close", Locale::PtBr).replace("{command}", command);
        assert!(done["next"].as_str().unwrap_or_default().ends_with(&close), "{done}");
    }

    /// Duas rodadas ao mesmo tempo, sem relatório, com uma onda pronta. As
    /// duas chegam ao despacho enquanto outro passo do git segura a trava;
    /// solta a trava, uma despacha a onda, e a outra lê a spec depois do envio
    /// dela: vê a onda em andamento e não a solta de novo. A onda tem um envio
    /// só, as duas respostas a mostram em andamento com esse envio, e a spec
    /// entra na execução uma vez.
    #[test]
    fn two_rounds_at_the_same_time_dispatch_a_wave_only_once() {
        use std::time::Duration;
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);

        let outs: Vec<Value> = std::thread::scope(|scope| {
            let Ok(lock) = super::git_lock(root) else { panic!("the git lock") };
            let rounds = [scope.spawn(|| round(root, "x", None)), scope.spawn(|| round(root, "x", None))];
            // O tempo de as duas lerem a spec e chegarem à trava.
            std::thread::sleep(Duration::from_millis(1000));
            drop(lock);
            rounds.into_iter().map(|r| r.join().unwrap()).collect()
        });
        for out in &outs {
            assert_eq!(out["ok"], json!(true), "{outs:?}");
        }
        let dispatched: Vec<u64> = outs.iter().flat_map(|out| waves_in(out, "dispatch")).collect();
        assert_eq!(dispatched, vec![1], "only one round sends the wave out: {outs:?}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let visible = log.visible();
        let sends: Vec<String> = visible.iter().filter(|e| e.event_type == "send").map(|e| codes[&e.id].clone()).collect();
        assert_eq!(sends.len(), 1, "the wave has one send: {outs:?}");
        for out in &outs {
            assert_eq!(out["running"], json!([{"wave": 1, "send": sends[0]}]), "{outs:?}");
        }
        let entered = visible.iter().filter(|e| e.event_type == "state" && e.str_field("phase") == Some("running"));
        assert_eq!(entered.count(), 1, "the spec enters the run once: {outs:?}");
    }

    /// Cada rodada encerra o que um agente deixou preso: um comando cuja
    /// cópia de onda já foi apagada é achado e citado nos avisos.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_round_ends_a_stuck_process_and_reports_it_in_warnings() {
        use std::process::{Command, Stdio};

        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let copy = mustard_core::ClaudePaths::compose_unchecked(root).claude_dir().join("worktrees").join("mustard-x-99");
        std::fs::create_dir_all(&copy).unwrap();
        let mut orphaned = Command::new("sleep")
            .arg("30")
            .current_dir(&copy)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn a process in the wave's copy");
        std::fs::remove_dir_all(&copy).unwrap();

        let out = round(root, "x", None);
        let warned = out["warnings"].as_array().cloned().unwrap_or_default();
        let pid = orphaned.id().to_string();
        assert!(warned.iter().any(|w| w["reason"] == json!("stuck-ended") && w["hint"].as_str().unwrap_or_default().contains(&pid)), "{out}");
        let _ = orphaned.wait();
    }
}
