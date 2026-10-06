//! A resposta da rodada e o próximo passo: a recusa, com a mensagem no idioma
//! do projeto, e o caminho de uma chamada — conferir a fase, fechar o que
//! voltou, entregar ao orquestrador os candidatos da onda que ainda não tem
//! escolha, despachar as ondas prontas e dizer o que fazer em seguida. A
//! rodada não pede a revisão de onda nenhuma: quem confere o trabalho é o
//! agente de teste dedicado que o fechamento pede, uma vez por obra.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::{Block, BlockQuery, Refusal, SpecLog};
use mustard_core::domain::spec_state::{not_closed_yet, returns_to_running, PhaseWriter, SpecState, State};
use mustard_core::domain::wave_prompt::{estimate_tokens, summary_of, token_cap_message, wave_files, wave_title, WaveCopy};
use mustard_core::io::spec_events as store;
use mustard_core::io::wave_prompt::{prompts, recorded_copy, Flight};
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Map, Value};

use super::backlog::{dispatch_backlog, Judge};
use super::commit::git_lock;
use super::item_choice::choose_items;
use super::queue::{
    backlog_left, backlog_ready, backlog_uncovered, first_unfinished, max_parallel, next_waves, open_review, open_sends, orphaned_waves,
    sent_items, silent_minutes, waves_awaiting_new_agent, waves_in_progress, waves_returned,
};
use super::rejection::rejected_message;
use super::report::Taken;
use super::slots::{open_copies, sharing_copy, without_live_copy};
use super::stops::{stopped_waves, waves_stuck};
use super::usage::Caller;
use super::{can_run, RoundOpts, DONE_STEP};
use crate::commands::spec_events::write::record;
use crate::shared::jev::Board;
use crate::shared::search_door::first_warning;
use crate::shared::spec_state::{checkout, DiskSpecState};

/// Por que a rodada não correu.
pub(crate) enum RoundRefusal {
    /// Uma recusa do arquivo de eventos.
    Refused(Refusal),
    /// Uma linha do relatório não se entende.
    BadReport { detail: String },
    /// Uma linha do relatório não traz um campo obrigatório.
    LineField { line: &'static str, field: &'static str },
    /// Ondas que dependem umas das outras em círculo: nenhuma pôde ser
    /// escolhida para sair. A aprovação do plano já recusa isso antes de a
    /// rodada rodar — chegar aqui é a mesma leitura do grafo pegando um
    /// defeito que passou por outra porta, não uma segunda conta à parte.
    /// O ciclo aparece depois de a rodada já ter juntado, comitado e gravado
    /// o relatório: `answered` é a resposta do que ela já fez — o que foi
    /// gravado, o commit e as instruções de cópia da página —, e a recusa sai
    /// junto dela, em vez de trocá-la.
    WaveLoop { cycle: Vec<u64>, answered: Value },
    /// A entrega de uma onda conflita com o repositório principal: os
    /// trechos, a cópia em que se resolve e o commit atual.
    MergeConflict { wave: u64, copy: String, conflicts: Vec<String>, head: String },
    /// Um arquivo entregue não está no disco nem é conhecido do git.
    FileUnknown { file: String, wave: u64 },
    /// A spec ainda não foi aprovada.
    NotApproved { phase: String },
    /// A spec fechou, ou está com o pull request aberto, e não tem onda de
    /// conserto aberta: o pedido novo nela passa antes pela reabertura.
    SpecClosed { spec: String, phase: String },
    /// A spec já foi entregue na base ou descartada: ela não volta, e o
    /// pedido novo sobre ela abre uma spec nova.
    SpecFinished { spec: String, phase: String },
    /// O relatório trouxe a linha da entrega colada: a entrega mora na spec,
    /// e o agente a grava.
    ReturnLine,
    /// A linha de consumo chegou para uma onda com envio aberto, sem volta
    /// gravada, e com o Claude Code dela ainda aberto: o agente terminou sem
    /// gravar a entrega.
    ReturnMissing { wave: u64 },
    /// A cópia da onda mudou arquivo, e a volta gravada não traz o resumo do
    /// commit: o agente grava a entrega de novo.
    ReturnNeedsCommit { wave: u64 },
    /// O revisor gravou o veredito sem pedido de revisão aberto.
    NoOpenReview,
    /// O pedido de revisão segue aberto e o revisor ainda não gravou o
    /// veredito: o fechamento não pede outra revisão.
    VerdictMissing,
    /// A mensagem do commit não cabe no modelo.
    CommitTooLong { part: String, chars: usize, max: usize },
    /// A mensagem do commit traz o que ela nunca leva.
    CommitForbidden { found: String },
    /// O campo `commit` do relatório de entrega chegou com cara de código de
    /// commit, e não com o título em palavras que ele pede.
    CommitLooksLikeSha { found: String },
    /// Um agente disse que a mudança de plano da onda troca uma decisão do
    /// usuário: a onda espera o clique dele fora do commit. Manda quem
    /// conduz escrever a pergunta com as palavras do usuário, com o código no
    /// cabeçalho, e diz as tarefas que o agente não fez, que voltam à fila com
    /// o aceite. O texto do agente não vai junto. Na rodada vira aviso, e o
    /// resto segue; no fechamento, que depende de todas as ondas, é a
    /// resposta.
    Replan { wave: u64, code: String, tasks: Vec<String> },
    /// A entrega muda o plano e não diz quais tarefas da onda ficaram por
    /// fazer: sem a lista, a rodada daria todas por feitas. Leva as tarefas
    /// da onda, para o agente escolher.
    ReplanNeedsUndone { wave: u64, tasks: Vec<String> },
    /// A entrega cita como não feita uma tarefa que não é da onda dela: o
    /// código citado e as tarefas da onda.
    UndoneNotInWave { wave: u64, code: String, tasks: Vec<String> },
    /// A entrega devolve tarefas como não feitas (`tasks`), sem mudar o
    /// plano, e a cópia da onda mudou depois do último passo de término — ou
    /// mudou sem passo de término nenhum: há trabalho começado. O agente
    /// conclui a tarefa, grava o passo e entrega de novo.
    StartedWorkUndone { wave: u64, tasks: Vec<String> },
    /// A entrega dá como feitas as tarefas `tasks` sem o passo de término de
    /// cada uma: sem o passo, o agente não leu a medida da conversa.
    DoneWithoutStep { wave: u64, tasks: Vec<String> },
    /// Quem conduz a obra reprovou a volta da onda, com o motivo dele: a
    /// volta fica fora do commit, a cópia fica como está, e a onda espera um
    /// agente novo, que recebe o motivo. Com `title`, o agente novo ainda não
    /// saiu, e a resposta manda despachá-lo por esse título; sem ele, o agente
    /// novo já saiu e trabalha na cópia.
    Rejected { wave: u64, reason: String, title: Option<String> },
    /// A linha `REJECTED` aponta uma onda sem volta gravada à espera da
    /// rodada: não há entrega a reprovar.
    RejectedWithoutReturn { wave: u64 },
    /// A linha `REJECTED` aponta uma onda cuja volta já entrou no commit
    /// `sha`: a rodada não desfaz commit.
    RejectedCommitted { wave: u64, sha: String },
    /// O git recusou o commit.
    Git { detail: String },
    /// Um comando que o projeto declara — a compilação, o lint ou a suíte
    /// inteira ([`super::checks::Check`]) — caiu no repositório principal
    /// antes do commit da rodada, com o comando e o fim da saída: nada foi
    /// comitado.
    CheckFailed { check: super::checks::Check, command: String, output: String },
    /// A prova de um critério que as ondas deste relatório cobrem não
    /// executou ou não passou: nada foi comitado.
    CriterionProofFailed { code: String, command: String, output: String },
    /// A prova de um critério que as ondas deste relatório cobrem saiu verde
    /// sem rodar teste nenhum, com o número que a saída disse: nada foi
    /// comitado.
    CriterionRanNoTest { code: String, command: String, tests: u64 },
    /// A prova de um critério que as ondas deste relatório cobrem saiu verde
    /// citando um teste que não existe no projeto, com o nome que faltou:
    /// nada foi comitado.
    CriterionMissingTest { code: String, name: String },
    /// O pedido de uma onda passa do teto de tokens: a rodada recusa antes de
    /// gravar o envio, com o tamanho medido e o teto.
    TokenCap { wave: u64, tokens: u64 },
    /// A conferência depois da onda achou o que consertar: nada foi comitado.
    /// Com `question`, uma onda já passou por todas as rodadas de conserto, e
    /// a pergunta vai ao usuário. `fixes` leva, de cada onda recusada, o
    /// trecho de `text` que é dela: é o que volta ao agente da onda.
    AfterWave { text: String, question: Option<String>, fixes: Vec<(u64, String)> },
}

impl RoundRefusal {
    pub(crate) fn reason(&self) -> String {
        match self {
            Self::Refused(refusal) => refusal.reason().to_string(),
            Self::WaveLoop { .. } => "waves-loop".into(),
            Self::BadReport { .. } => "round-bad-report".into(),
            Self::LineField { .. } => "round-line-field-missing".into(),
            Self::MergeConflict { .. } => "round-merge-conflict".into(),
            Self::FileUnknown { .. } => "round-file-unknown".into(),
            Self::NotApproved { .. } => "round-not-approved".into(),
            Self::SpecClosed { .. } => "round-spec-closed".into(),
            Self::SpecFinished { .. } => "round-spec-finished".into(),
            Self::ReturnLine => "round-return-line".into(),
            Self::ReturnMissing { .. } => "round-return-missing".into(),
            Self::ReturnNeedsCommit { .. } => "round-return-needs-commit".into(),
            Self::NoOpenReview => "no-open-review".into(),
            Self::VerdictMissing => "review-verdict-missing".into(),
            Self::CommitTooLong { .. } => "commit-too-long".into(),
            Self::CommitForbidden { .. } => "commit-forbidden-text".into(),
            Self::CommitLooksLikeSha { .. } => "commit-looks-like-sha".into(),
            Self::Replan { .. } => "wave-plan-does-not-work".into(),
            Self::ReplanNeedsUndone { .. } => "replan-needs-undone".into(),
            Self::UndoneNotInWave { .. } => "undone-not-in-wave".into(),
            Self::StartedWorkUndone { .. } => "delivery-started-work-undone".into(),
            Self::DoneWithoutStep { .. } => "delivery-done-without-step".into(),
            Self::Rejected { .. } => "round-wave-rejected".into(),
            Self::RejectedWithoutReturn { .. } => "round-rejected-without-return".into(),
            Self::RejectedCommitted { .. } => "round-rejected-committed".into(),
            Self::Git { .. } => "git-refused".into(),
            Self::CheckFailed { check, .. } => check.reason().into(),
            Self::CriterionProofFailed { .. } => "round-criterion-proof-failed".into(),
            Self::CriterionRanNoTest { .. } => "round-criterion-ran-no-test".into(),
            Self::CriterionMissingTest { .. } => "round-criterion-missing-test".into(),
            Self::TokenCap { .. } => "wave-token-cap".into(),
            Self::AfterWave { question, .. } => {
                if question.is_some() { "round-after-wave-limit".into() } else { "round-after-wave".into() }
            }
        }
    }

    pub(crate) fn message(&self, lang: Locale) -> String {
        let fill = |key: &str, slots: &[(&str, String)]| {
            slots.iter().fold(translate(key, lang).to_string(), |text, (slot, value)| text.replace(slot, value))
        };
        match self {
            Self::Refused(refusal) => refusal.message(lang),
            // A mesma chave da recusa que já trava a aprovação do plano
            // ([`crate::commands::flow::plan::PlanFinding::WaveLoop`]): uma
            // recusa só, com a mesma leitura do ciclo e a mesma mensagem, não
            // duas contas que pudessem discordar entre si.
            Self::WaveLoop { cycle, .. } => wave_loop_message(cycle, lang),
            Self::BadReport { detail } => fill("round.bad_report", &[("{detail}", detail.clone())]),
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
            Self::SpecClosed { spec, phase } => {
                fill("round.closed", &[("{spec}", spec.clone()), ("{phase}", phase.clone())])
            }
            Self::SpecFinished { spec, phase } => {
                fill("round.finished", &[("{spec}", spec.clone()), ("{phase}", phase.clone())])
            }
            Self::ReturnLine => fill("spec_events.report_carries_return_line", &[]),
            Self::ReturnMissing { wave } => fill("spec_events.return_missing", &[("{wave}", wave.to_string())]),
            Self::ReturnNeedsCommit { wave } => {
                fill("spec_events.return_needs_commit", &[("{wave}", wave.to_string())])
            }
            Self::NoOpenReview => fill("spec_events.no_open_review", &[]),
            Self::VerdictMissing => fill("spec_events.verdict_missing", &[]),
            Self::CommitTooLong { part, chars, max } => fill(
                "round.commit_too_long",
                &[("{part}", part.clone()), ("{chars}", chars.to_string()), ("{max}", max.to_string())],
            ),
            Self::CommitForbidden { found } => {
                fill("round.commit_forbidden", &[("{found}", found.clone())])
            }
            Self::CommitLooksLikeSha { found } => {
                fill("round.commit_looks_like_sha", &[("{found}", found.clone())])
            }
            Self::Replan { wave, code, tasks } => fill(
                "round.replan",
                &[
                    ("{wave}", wave.to_string()),
                    ("{code}", code.clone()),
                    ("{yes}", translate("change.accept", lang).to_string()),
                    ("{no}", translate("change.decline", lang).to_string()),
                    ("{tasks}", task_list(tasks, lang)),
                ],
            ),
            Self::ReplanNeedsUndone { wave, tasks } => fill(
                "round.replan_needs_undone",
                &[("{wave}", wave.to_string()), ("{tasks}", task_list(tasks, lang))],
            ),
            Self::UndoneNotInWave { wave, code, tasks } => fill(
                "round.undone_not_in_wave",
                &[("{wave}", wave.to_string()), ("{code}", code.clone()), ("{tasks}", task_list(tasks, lang))],
            ),
            Self::StartedWorkUndone { wave, tasks } => fill(
                "round.started_work_undone",
                &[("{wave}", wave.to_string()), ("{tasks}", task_list(tasks, lang))],
            ),
            Self::DoneWithoutStep { wave, tasks } => fill(
                "round.done_without_step",
                &[("{wave}", wave.to_string()), ("{tasks}", task_list(tasks, lang))],
            ),
            Self::Rejected { wave, reason, title } => rejected_message(*wave, reason, title.as_deref(), lang),
            Self::RejectedWithoutReturn { wave } => fill("round.rejected_without_return", &[("{wave}", wave.to_string())]),
            Self::RejectedCommitted { wave, sha } => {
                fill("round.rejected_committed", &[("{wave}", wave.to_string()), ("{sha}", sha.clone())])
            }
            Self::Git { detail } => fill("round.git_refused", &[("{detail}", detail.clone())]),
            Self::CheckFailed { check, command, output } => {
                fill(check.message_key(), &[("{command}", command.clone()), ("{output}", output.clone())])
            }
            Self::CriterionProofFailed { code, command, output } => fill(
                "round.criterion_proof_failed",
                &[("{code}", code.clone()), ("{command}", command.clone()), ("{output}", output.clone())],
            ),
            Self::CriterionRanNoTest { code, command, tests } => fill(
                "round.criterion_ran_no_test",
                &[("{code}", code.clone()), ("{command}", command.clone()), ("{count}", tests.to_string())],
            ),
            Self::CriterionMissingTest { code, name } => {
                fill("round.criterion_missing_test", &[("{code}", code.clone()), ("{name}", name.clone())])
            }
            Self::TokenCap { wave, tokens } => {
                token_cap_message(*wave, *tokens, lang).unwrap_or_default()
            }
            Self::AfterWave { text, .. } => text.clone(),
        }
    }

    pub(crate) fn to_value(&self, lang: Locale) -> Value {
        // A recusa do ciclo leva junto o que a rodada já gravou: ela chega
        // depois do commit, e trocar a resposta inteira pela recusa deixaria
        // a página para trás, sem ninguém mandado copiá-la.
        let mut out = match self {
            Self::WaveLoop { answered, .. } if answered.is_object() => answered.clone(),
            _ => json!({}),
        };
        out["ok"] = json!(false);
        out["reason"] = json!(self.reason());
        out["hint"] = json!(self.message(lang));
        // O cabeçalho e as opções da pergunta da mudança vão prontos; o
        // enunciado quem conduz escreve com as palavras do usuário, e é o
        // cabeçalho, não a frase, que diz à testemunha qual mudança o clique
        // decide.
        if let Self::Replan { code, .. } = self {
            out["header"] = json!(code);
            out["options"] = json!([translate("change.accept", lang), translate("change.decline", lang)]);
        }
        if let Self::AfterWave { question: Some(question), .. } = self {
            out["question"] = json!(question);
        }
        out
    }
}

/// A mudança `change` sem o ponto final: a frase do catálogo já fecha a
/// mudança com o ponto dela, e a que o agente mandou terminada em ponto
/// sairia com dois.
pub(super) fn without_final_period(change: &str) -> String {
    change.trim().trim_end_matches('.').trim_end().to_string()
}

/// Os códigos de tarefa `tasks` numa lista só, para a frase do catálogo; a
/// palavra de nenhuma, no idioma `lang`, quando não há tarefa.
fn task_list(tasks: &[String], lang: Locale) -> String {
    if tasks.is_empty() { translate("round.no_tasks", lang).to_string() } else { tasks.join(", ") }
}

/// A recusa das ondas `cycle`, que dependem umas das outras em círculo.
fn wave_loop_message(cycle: &[u64], lang: Locale) -> String {
    let waves: Vec<String> = cycle.iter().map(u64::to_string).collect();
    translate("plan.wave_loop", lang).replace("{waves}", &waves.join(", "))
}

/// As ondas a reenviar nesta rodada, cada uma com o número do pedido
/// anterior: as órfãs, de um Claude Code que fechou, e as pausadas por este
/// relatório — a onda pausada sai de novo na mesma rodada. Só a onda do plano
/// entra aqui: a de lote que perdeu todas as tarefas para o backlog — o que o
/// corte de uma onda de lote, no mesmo relatório, acabou de fazer — saiu dele
/// ([`SpecLog::planned_waves`]), e reenviá-la seria despachar uma onda vazia.
/// A pausada tem de estar com o pedido aberto e sem volta: a onda que já
/// voltou, ou já entregou, não tem o que refazer, e reenviá-la a poria por
/// cima da cópia que guarda o que ela entregou.
fn resend_targets(log: &SpecLog, paused: &[u64]) -> BTreeMap<u64, u64> {
    let last_sends = log.last_by_wave("send");
    let planned = log.planned_waves();
    let open = open_sends(log);
    let returned = waves_returned(log);
    let mut out = orphaned_waves(log);
    for wave in paused.iter().filter(|wave| planned.contains(wave) && open.contains_key(wave) && !returned.contains(wave)) {
        if let Some(sent) = last_sends.get(wave) {
            out.entry(*wave).or_insert(*sent);
        }
    }
    out
}

/// O código ou o número de um campo que aponta outro evento (`item`, num
/// passo): o código quando o mapa o conhece, senão o próprio número.
pub(super) fn ref_shown(value: Option<&Value>, codes: &BTreeMap<u64, String>) -> String {
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

/// O último passo gravado depois do envio `sent` da onda `wave`: o código do
/// item, pelo mapa `codes`, e o texto. `None` sem passo depois desse envio —
/// um passo de um envio anterior, já respondido, não conta.
fn last_step(log: &SpecLog, wave: u64, sent: u64, codes: &BTreeMap<u64, String>) -> Option<(String, String)> {
    log.block(BlockQuery::Block(Block::Waves))
        .into_iter()
        .rfind(|e| e.event_type == "step" && e.wave() == Some(wave) && e.id > sent)
        .map(|e| (ref_shown(e.fields.get("item"), codes), e.str_field("text").unwrap_or_default().to_string()))
}

/// Os minutos desde o evento `at`. `None` sem hora legível.
fn minutes_since(log: &SpecLog, at: u64) -> Option<i64> {
    let raw = log.get(at)?.at().to_string();
    let at = chrono::DateTime::parse_from_rfc3339(raw.trim()).ok()?;
    let now = chrono::Local::now().with_timezone(at.offset());
    Some((now - at).num_minutes())
}

/// Os arquivos mudados na cópia da onda `wave`, pelo `git status` curto dela.
/// `None` sem cópia gravada, ou sem git.
///
/// Cada linha do `git status --porcelain` traz dois caracteres de estado, um
/// espaço separador e o caminho — sempre na posição 3. A saída inteira chega
/// trimada (`GitRun::out`), o que só afeta a primeira linha: quando o
/// primeiro caractere de estado dela é espaço (ex.: `" M arquivo"`), o trim
/// da string inteira apaga só esse espaço, e o separador cai uma posição
/// antes. Por isso só a linha de índice 0 é conferida: as outras sempre
/// mantêm a posição 3. O arquivo renomeado (`"velho -> novo"`) dá o caminho
/// novo.
fn copy_files_changed(log: &SpecLog, wave: u64) -> Option<Vec<String>> {
    let copy = recorded_copy(log, wave)?;
    let out = mustard_core::platform::git::run(Path::new(&copy.path), &["status", "--porcelain", "--untracked-files=all"])
        .out()?;
    Some(
        out.lines()
            .enumerate()
            .filter_map(|(i, line)| {
                let offset = if i == 0 && line.as_bytes().get(2) != Some(&b' ') { 2 } else { 3 };
                line.get(offset..)
            })
            .map(|path| path.rsplit(" -> ").next().unwrap_or(path).trim())
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .collect(),
    )
}

/// O estado da onda `wave` em andamento, para o orquestrador saber como ela
/// vai sem conferir a cópia à mão: o código do envio, os minutos desde ele,
/// os arquivos que a cópia já tem mudados, o último passo gravado depois do
/// envio e os minutos desde o último sinal de vida — a mesma leitura do
/// aviso dos 40 minutos. O que falta fica fora da entrada, sem erro nenhum:
/// nada disto trava a rodada.
fn running_state(root: &Path, spec: &str, log: &SpecLog, codes: &BTreeMap<u64, String>, wave: u64, send: &str, sent: u64) -> Value {
    let mut out = json!({ "wave": wave, "send": send });
    if let Some(minutes) = minutes_since(log, sent) {
        out["minutes"] = json!(minutes);
    }
    if let Some(files) = copy_files_changed(log, wave) {
        out["files"] = json!(files);
    }
    if let Some((item, text)) = last_step(log, wave, sent, codes) {
        out["step"] = json!({ "item": item, "text": text });
    }
    if let Some(silent) = silent_minutes(root, spec, wave, log, sent) {
        out["silent_minutes"] = json!(silent);
    }
    out
}

/// O texto do reenvio: o pedido gravado no envio anterior, com a parte das
/// ondas em andamento recalculada para as `running` de agora — a gravada
/// fica velha assim que outra onda entrega, entra ou sai de cena —, mais os
/// passos já gravados desta onda e o aviso de começar vendo o que mudou na
/// cópia. Sem passo nenhum, só o aviso.
fn resend_text(previous: &str, running: &[(u64, Vec<String>)], steps: &[String], lang: Locale) -> String {
    let mut parts = vec![refresh_running(previous, running, lang)];
    if !steps.is_empty() {
        parts.push(translate("round.resume.steps", lang).to_string());
        parts.push(steps.join("\n"));
    }
    parts.push(translate("round.resume.notice", lang).to_string());
    parts.join("\n\n")
}

/// A parte das ondas em andamento do pedido anterior (`## Como trabalhar`), trocada
/// pela de agora: a marca é a linha do rótulo
/// (`prompt.execution.running`) e as linhas indentadas logo depois dela, uma
/// por onda, no mesmo formato que o primeiro envio escreve. Sem onda em
/// andamento nenhuma agora, a linha do rótulo e as dela somem; sem elas no
/// texto anterior e com onda em andamento agora, elas nascem no fim da
/// seção. Sem a seção `## Como trabalhar` no texto anterior, nada muda.
fn refresh_running(previous: &str, running: &[(u64, Vec<String>)], lang: Locale) -> String {
    let header = format!("## {}", translate("prompt.part.work", lang));
    let label = format!("- {}", translate("prompt.execution.running", lang));
    let lines: Vec<&str> = previous.lines().collect();
    let Some(head) = lines.iter().position(|line| *line == header) else { return previous.to_string() };
    // A linha em branco logo depois do título abre a seção; o fim dela é a
    // próxima linha em branco, ou o fim do texto, quando é a última seção.
    let content_at = head + 2;
    let stop = lines[content_at..].iter().position(|line| line.is_empty()).map_or(lines.len(), |n| content_at + n);
    let old_at = lines[content_at..stop].iter().position(|line| *line == label).map(|n| content_at + n);
    let old_end = old_at.map_or(stop, |at| {
        lines[at + 1..stop].iter().position(|line| !line.starts_with("  - ")).map_or(stop, |n| at + 1 + n)
    });
    let fresh: Vec<String> = running
        .iter()
        .map(|(wave, files)| {
            let name = translate("prompt.execution.wave", lang).replace("{n}", &wave.to_string());
            let files: Vec<String> = files.iter().map(|file| format!("`{file}`")).collect();
            if files.is_empty() { format!("  - {name}") } else { format!("  - {name}: {}", files.join(", ")) }
        })
        .collect();
    let cut_at = old_at.unwrap_or(stop);
    let mut out: Vec<String> = lines[..cut_at].iter().map(|l| (*l).to_string()).collect();
    if !fresh.is_empty() {
        out.push(label);
        out.extend(fresh);
    }
    out.extend(lines[old_end..].iter().map(|l| (*l).to_string()));
    out.join("\n")
}

pub(super) fn run_round(
    opts: &RoundOpts,
    root: &Path,
    lang: Locale,
    caller: Caller<'_>,
) -> Result<Value, RoundRefusal> {
    run_round_with_mine(opts, root, lang, caller, &scan_mine)
}

/// Quem relê o mapa depois do commit da rodada: a ferramenta do scan
/// instalada. Só o mapa do próprio projeto começa, em segundo plano, a leitura
/// da história dos arquivos dele: a cópia do mapa que a conferência depois da
/// onda relê e joga fora é só passada, e a história dela seria lida à toa. Os
/// testes da biblioteca nunca começam a leitura com um scan de verdade, que
/// escreveria no mapa por conta própria enquanto o teste ainda o usa; o
/// programa inteiro, com o scan ao lado, é provado pelo fluxo de ponta a ponta.
pub(super) fn scan_mine(root: &Path, out: &Path) -> mustard_core::platform::error::Result<mustard_core::domain::scan::ScanReport> {
    let scan = mustard_core::Scan::locate();
    if !cfg!(test) && out == mustard_core::io::project_map::model_path(root) {
        scan.scan_then_read_history(root, out)
    } else {
        scan.scan(root, out)
    }
}

/// [`run_round`] com quem relê o mapa depois do commit da rodada (`mine`),
/// que um teste escolhe sem instalar a ferramenta do scan de verdade.
pub(super) fn run_round_with_mine(
    opts: &RoundOpts,
    root: &Path,
    lang: Locale,
    caller: Caller<'_>,
    mine: &dyn Fn(&Path, &Path) -> mustard_core::platform::error::Result<mustard_core::domain::scan::ScanReport>,
) -> Result<Value, RoundRefusal> {
    let entry = enter_round(opts, root, caller.session)?;
    // Os agentes do projeto voltam ao texto deste programa no começo da
    // rodada, e não na hora do despacho: o Claude Code leva alguns segundos
    // para reler o arquivo, e o agente aberto logo depois da troca ainda
    // roda o texto antigo.
    let refreshed = agents_refreshed(root, lang);
    let mut out = run_entered_round(opts, root, lang, caller, mine, entry)?;
    if let Some((reason, hint)) = refreshed {
        crate::commands::spec_events::pages::push_warning(&mut out, reason, &hint);
    }
    Ok(out)
}

/// Regrava os agentes do Mustard no projeto em `root` que diferem do texto
/// deste programa, e devolve o aviso da resposta: o motivo e a frase em
/// `lang`, que nomeia os arquivos regravados, ou o erro que impediu a
/// regravação. Sem nada regravado, nada a dizer. É a mesma conferência na
/// rodada e no fechamento, antes de cada um despachar um agente.
pub(crate) fn agents_refreshed(root: &Path, lang: Locale) -> Option<(&'static str, String)> {
    match mustard_core::refresh_agent_texts(root) {
        Ok(files) if files.is_empty() => None,
        Ok(files) => {
            Some(("agents-refreshed", translate("round.agents_refreshed", lang).replace("{files}", &files.join(", "))))
        }
        Err(error) => Some((
            "agents-not-refreshed",
            translate("round.agents_not_refreshed", lang).replace("{detail}", &error.to_string()),
        )),
    }
}

/// O que a rodada leu ao entrar, antes de assumir qualquer volta e de pegar
/// a trava do passo do git: o nome da spec, o arquivo dela, a leitura de
/// entrada e a fase gravada nela.
pub(super) struct Entry {
    spec: String,
    path: PathBuf,
    log: SpecLog,
    phase: String,
}

/// A entrada da rodada: acha a spec, lê o arquivo dela, recusa a spec que
/// não pode rodar. A leitura que
/// sai daqui é a de entrada, que o resto da rodada ([`run_entered_round`])
/// compara com a feita já com a trava presa. A sessão (`session`) acha a spec
/// atual quando o pedido não diz qual.
pub(super) fn enter_round(opts: &RoundOpts, root: &Path, session: Option<&str>) -> Result<Entry, RoundRefusal> {
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
    //
    // A exceção é a onda de conserto que a porta do pull request reprovado
    // abriu numa spec já fechada
    // ([`crate::commands::flow::reopen::open_fix_wave_of`]): é a rodada de
    // sempre que a despacha e comita o conserto na mesma branch, e sem esta
    // fresta a porta abriria uma onda que nunca sairia.
    //
    // A spec fechada, ou com o pull request aberto, sem onda de conserto tem
    // recusa própria: ela já foi aprovada, e o pedido novo nela entra pela
    // reabertura, que a leva de volta à execução.
    //
    // A spec entregue na base ou descartada também já passou da aprovação, e
    // não volta por caminho nenhum: a recusa aponta uma spec nova, a mesma
    // saída de quem grava pedido ou tarefa nela. A frase de spec não
    // aprovada fica só para as fases antes da aprovação.
    let recorded = State::from_log(&log).phase;
    let phase = recorded.unwrap_or_default().to_string();
    if !can_run(&phase) && crate::commands::flow::reopen::open_fix_wave_of(&log).is_none() {
        return Err(if returns_to_running(&phase) {
            RoundRefusal::SpecClosed { spec, phase }
        } else if recorded.is_some_and(|phase| !not_closed_yet(phase)) {
            RoundRefusal::SpecFinished { spec, phase }
        } else {
            RoundRefusal::NotApproved { phase }
        });
    }

    Ok(Entry { spec, path, log, phase })
}

/// O resto da rodada, depois da leitura de entrada (`entry`, de
/// [`enter_round`]): assume as voltas, forma os lotes, despacha as ondas
/// prontas e monta a resposta.
pub(super) fn run_entered_round(
    opts: &RoundOpts,
    root: &Path,
    lang: Locale,
    caller: Caller<'_>,
    mine: &dyn Fn(&Path, &Path) -> mustard_core::platform::error::Result<mustard_core::domain::scan::ScanReport>,
    entry: Entry,
) -> Result<Value, RoundRefusal> {
    let session = caller.session;
    let Entry { spec, path, log, phase } = entry;

    // O backlog é lido também como estava ao entrar na rodada, antes de o
    // relatório dela mexer em onda ou tarefa: a tarefa que o corte de uma
    // onda de lote devolve solta, agora mesmo, fica solta até a rodada
    // seguinte — só a que já estava pronta antes desta rodada começar, e
    // continua pronta com a trava presa, é empacotada aqui.
    let log_on_entry = log.clone();

    // A rodada assume, antes de despachar, a volta que cada onda gravou na
    // spec, com ou sem relatório: o relatório traz só as linhas de quem
    // despacha. Sem volta e sem linha a assumir, nada é juntado, e a rodada
    // vai direto ao despacho, com a escolha de cada onda.
    let raw = opts.report.as_deref().map(str::trim).filter(|r| !r.is_empty());
    let Taken { mut recorded, formatted, mut warnings, commit, paused, waiting: held } =
        super::report::take_report_with_mine(&opts.root, root, &spec, raw, &log, lang, caller, mine)
            .map_err(|refused| new_agents_named(refused, root, &spec, &log, lang))?;

    // O despacho — a entrada na execução, a leitura da spec, a escolha das
    // ondas, a criação das cópias e a gravação dos envios — roda inteiro com a
    // trava do passo do git presa: a rodada que chega ao mesmo tempo só lê a
    // spec depois dos envios desta, e nunca solta a mesma onda de novo.
    let held_lock = git_lock(root)?;

    // Nada fica preso: todo processo que um agente deixou rodando — um laço
    // de espera, ou um comando na cópia de uma onda que o commit anterior já
    // apagou — é encerrado a cada rodada, e a resposta diz qual. A busca vem
    // depois da trava: a rodada que chegou antes já gravou o envio da vaga
    // que preparou, e o git dela, lá dentro, nunca é tomado por esquecido.
    let stuck_ended = crate::commands::flow::stuck::end_stuck_processes(root, &held_lock);
    if let Some(hint) = crate::commands::flow::stuck::report_line(&stuck_ended, lang) {
        warnings.push(json!({ "reason": "stuck-ended", "hint": hint }));
    }

    // O mapa volta ao commit atual antes de montar os pedidos: um commit à
    // mão ou um pull podem ter mudado o código fora da rodada, e sem isto a
    // sugestão da onda seguinte apontaria linhas velhas.
    super::commit::refresh_map_if_stale(root, mine);

    // O backlog monta as ondas que saem agora, uma por vaga livre e um assunto
    // em cada, com as tarefas prontas nas duas leituras: a de entrada
    // (`log_on_entry`) e a feita agora, já com a trava presa. A onda leva
    // também, depois delas, a tarefa do backlog nas duas leituras que espera
    // só por tarefas dela e divide arquivo com ela. A onda de lote que ficou
    // montada e sem sair se desfaz. O binário grava a onda e as tarefas
    // dela, com autor próprio, e só depois a rodada lê as ondas que existem —
    // as novas e as já entregues. A tarefa que o corte de uma onda de lote
    // acabou de devolver solta, no relatório desta mesma chamada, não estava
    // pronta na entrada e fica solta até a rodada seguinte. A que outra
    // rodada, chegada ao mesmo tempo, empacotou enquanto esta esperava a
    // trava já tem onda na leitura de agora, e não sai de novo; o número da
    // onda nova também sai dela.
    let locked = store::read(&path)
        .map_err(RoundRefusal::Refused)?
        .ok_or_else(|| RoundRefusal::Refused(Refusal::NoSpecFile { spec: spec.clone() }))?;
    // Com a trava presa e a spec relida, sai da pasta de despacho o trecho de
    // conserto da onda que a rodada comitou ou que voltou de novo.
    sweep_fixes(root, &spec, Some(&locked));
    // O commit que uma rodada anterior fez e não chegou a anotar, porque caiu
    // logo depois dele, é anotado agora, antes de qualquer despacho: sem ele
    // o fechamento recusa a obra.
    recorded.extend(super::lost_commit::record_lost_commits(&opts.root, root, &spec, &locked, lang, &held_lock)?);
    let waves_jev = crate::shared::jev::for_waves(root);
    if waves_jev.key_in_git && first_warning(root, session, "round.key_in_git") {
        warnings.push(json!({ "reason": "key-in-git", "hint": translate("map.round.key_in_git", lang) }));
    }
    let jev = waves_jev.filter.as_ref();
    let judge = jev.map(|jev| move |board: &Board| jev.judge_backlog(board));
    dispatch_backlog(
        &opts.root,
        &spec,
        &log_on_entry,
        &locked,
        max_parallel(root),
        judge.as_ref().map(|judge| judge as &Judge<'_>),
    )
    .map_err(RoundRefusal::Refused)?;
    // A tarefa do backlog que não cobre item nenhum não forma onda: a rodada
    // a nomeia, porque ela fica no backlog e segura o fechamento.
    let uncovered = backlog_uncovered(&locked);
    if !uncovered.is_empty() {
        let codes = locked.codes();
        let tasks: Vec<String> =
            uncovered.iter().map(|id| codes.get(id).cloned().unwrap_or_else(|| id.to_string())).collect();
        let hint = translate("round.task_without_covers", lang).replace("{tasks}", &tasks.join(", "));
        warnings.push(json!({ "reason": "task-without-covers", "hint": hint }));
    }
    // A primeira rodada leva a spec para a execução só depois de o lote
    // passar, ainda com a trava do git presa: o lote recusado deixa a spec
    // aprovada, como estava, e a rodada seguinte entra de novo por aqui. A
    // leitura logo abaixo já vê a fase nova.
    let entering = phase == "approved"
        && crate::commands::spec_events::write::record_phase(&opts.root, &spec, "running", session);
    let log = store::read(&path)
        .map_err(RoundRefusal::Refused)?
        .ok_or_else(|| RoundRefusal::Refused(Refusal::NoSpecFile { spec: spec.clone() }))?;
    let codes = log.codes();

    // O despacho da rodada seguinte: as ondas prontas, no máximo o que o
    // projeto deixa compilar ao mesmo tempo contando as que já estão em
    // andamento, e nenhuma que a onda parada pelo limite de consertos segura.
    // Cada uma sai com a sua vaga, a cópia fixa em que ela compila.
    // A onda cuja volta ficou de fora — a que pede novo plano, ou a recusada
    // por uma conferência dela — já voltou: o agente dela terminou, e ela
    // espera o clique do usuário ou a volta regravada, não o agente. O envio
    // aberto segue segurando a vaga e os arquivos dela (`occupied`), mas ela
    // não aparece em andamento nem manda esperar por ela
    // ([`waves_in_progress`]).
    let running: BTreeMap<u64, u64> = waves_in_progress(&log);
    // A órfã segue ocupando a vaga dela até o reenvio, mais abaixo: quem
    // conta vaga livre soma as vivas e as órfãs, e só a viva entra no
    // "esperando" da resposta.
    let occupied = open_sends(&log);
    let stuck = waves_stuck(&log);
    // O ciclo entre as ondas seguintes só aparece aqui, depois de o relatório
    // já ter sido juntado, comitado e gravado: a recusa sai com o que a
    // rodada fez até agora, e a página, que o relatório mudou, é copiada do
    // mesmo jeito que numa rodada que despacha.
    let ready = match next_waves(&log, max_parallel(root), &occupied, &stuck) {
        Ok(ready) => ready,
        Err(cycle) => {
            drop(held_lock);
            let mut answered = json!({ "spec": spec, "recorded": recorded, "formatted": formatted });
            if entering {
                answered["phase"] = json!("running");
            }
            if let Some(commit) = commit {
                answered["commit"] = commit;
            }
            if !warnings.is_empty() {
                answered["warnings"] = json!(warnings);
            }
            // O "depois" da página é a própria recusa: o que falta fazer é
            // cortar uma das dependências do ciclo.
            let then = wave_loop_message(&cycle, lang);
            end_answer(root, &spec, &mut answered, &then, lang);
            return Err(RoundRefusal::WaveLoop { cycle, answered });
        }
    };
    // A escolha dos itens do pedido, antes da cópia: o Jev julga, uma chamada
    // por onda, o item do projeto todo, o dos arquivos dela e o sem ligação
    // com ela, e a onda sai nesta mesma rodada, sem esperar quem conduz. Sem
    // Jev, ou com a chamada falhando, o pedido leva o padrão.
    let choices = choose_items(&opts.root, &spec, &log, &ready, jev);
    // Depois da última chamada ao Jev: o teto de gasto do mês que o segurou,
    // já gasto na montagem ou recusando uma chamada, sai num aviso, uma vez
    // por sessão, como na busca.
    if waves_jev.held_by_budget() && first_warning(root, session, "round.jev_over_budget") {
        warnings.push(json!({ "reason": "jev-over-budget", "hint": translate("round.jev_over_budget", lang) }));
    }
    let go = ready;
    // A onda a reenviar cuja cópia gravada outra onda também segura não volta
    // a ela: sai numa vaga livre, como a onda nova, e antes dela, porque já
    // estava em andamento. A cuja cópia gravada deixou de ser uma cópia viva —
    // a pasta foi apagada entre o envio e o reenvio — também pede uma cópia
    // preparada agora, na mesma vaga quando ela serve: o reenvio nunca grava
    // uma pasta que o agente não acharia.
    let resends = resend_targets(&log, &paused);
    let moved = sharing_copy(&log, resends.keys().copied());
    let gone = without_live_copy(&log, resends.keys().copied());
    let renewed: BTreeSet<u64> = moved.union(&gone).copied().collect();
    let wanted: Vec<u64> = renewed.iter().chain(&go).copied().collect();
    let (mut copies, not_copied) = open_copies(root, &spec, &log, &held_lock, &wanted, lang);
    // O código que a limpeza de uma cópia guardou vai também no que vem
    // depois: a resposta diz de que onda era e como trazê-lo de volta.
    let kept_lines: Vec<String> = not_copied
        .iter()
        .filter(|warning| warning["reason"] == json!("code-kept"))
        .filter_map(|warning| warning["hint"].as_str().map(str::to_string))
        .collect();
    warnings.extend(not_copied);
    let mut renewed_copies: BTreeMap<u64, WaveCopy> =
        renewed.iter().filter_map(|w| copies.remove(w).map(|c| (*w, c))).collect();
    let next: Vec<u64> = go.into_iter().filter(|wave| copies.contains_key(wave)).collect();
    // O pedido de cada onda lista as outras em andamento, contando as órfãs,
    // que saem de novo nesta rodada, e as que saem junto com ela, e traz a
    // cópia dela e a escolha do orquestrador. A onda cuja volta ficou de fora
    // não está em andamento, como no gancho que monta o mesmo pedido.
    let orphans = orphaned_waves(&log);
    let away = running.keys().chain(orphans.keys()).chain(&next).copied().collect();
    let flight = Flight { running: away, copies, choices };
    let built = prompts(root, &spec, &log, lang, &flight);
    let mut dispatched: Vec<Value> = Vec::new();
    // O código e o número do envio de cada onda em andamento — o número é o
    // que a resposta usa para achar o último passo dela e a hora do envio.
    let mut in_flight: BTreeMap<u64, (String, u64)> = running
        .iter()
        .map(|(wave, sent)| (*wave, (codes.get(sent).cloned().unwrap_or_else(|| sent.to_string()), *sent)))
        .collect();
    // O processo por trás desta rodada — o Claude Code que a chamou ou, sem
    // um por cima, ela mesma: gravado em todo envio, novo ou reenviado, para
    // a rodada seguinte saber se aquele ainda está aberto.
    let (claude_pid, claude_started) = crate::commands::flow::stuck::sender_process();
    for wave in &next {
        let Some(prompt) = built.iter().find(|p| p.wave == *wave) else { continue };
        // O pedido acima do teto de tokens recusa a rodada antes de gravar o
        // envio: sem isso o agente recebia um pedido grande demais sem
        // ninguém ter decidido dividir o lote.
        let tokens = estimate_tokens(&prompt.text);
        if token_cap_message(*wave, tokens, lang).is_some() {
            return Err(RoundRefusal::TokenCap { wave: *wave, tokens });
        }
        let mut draft = Map::new();
        draft.insert("wave".into(), json!(wave));
        draft.insert("role".into(), json!("wave"));
        // O nome do agente, `wave` em toda onda, de uma tarefa ou de várias,
        // e não o molde dele, que mora no projeto: a resposta desta rodada o
        // repete, para quem despacha saber qual chamar. O pedido e o modelo
        // pedido vão junto — nada disso é remontado na leitura, é o que foi
        // enviado.
        let agent = prompt.agent.clone();
        draft.insert("agent".into(), json!(agent));
        draft.insert("text".into(), json!(prompt.text));
        draft.insert("model".into(), json!(prompt.model));
        draft.insert("effort".into(), json!(prompt.effort));
        draft.insert("lines".into(), json!(prompt.lines));
        draft.insert("chars".into(), json!(prompt.text.chars().count()));
        // Os itens que ficaram, e à parte a escolha do orquestrador: o que
        // saiu e o que entrou, cada um com o motivo.
        draft.insert("items".into(), json!(sent_items(&log, *wave, flight.choices.get(wave))));
        // O que o pedido manda ler, item a item: a mesma lista que imprimiu
        // as linhas dele, e contra ela a entrega confere o que foi lido.
        draft.insert("read_items".into(), json!(prompt.listed));
        // O resumo que a onda continua, que o pedido abre mandando ler: o
        // envio o cita, e é por ele que o resumo passa a estar em uso.
        if let Some(summary) = summary_of(&log, *wave) {
            draft.insert("summary".into(), json!(summary.id));
        }
        if let Some(choice) = flight.choices.get(wave) {
            draft.insert("analysis".into(), choice.to_value());
        }
        draft.insert("mustard".into(), json!(env!("CARGO_PKG_VERSION")));
        if let Some(copy) = flight.copies.get(wave) {
            draft.insert("copy".into(), json!(copy.path));
        }
        draft.insert("claude_pid".into(), json!(claude_pid));
        draft.insert("claude_started".into(), json!(claude_started));
        draft.insert("author".into(), json!("binary"));
        let written = record(&opts.root, &spec, "send", draft, PhaseWriter::Binary)
            .map_err(RoundRefusal::Refused)?;
        recorded.push(json!({ "wave": wave, "type": "send", "id": written.written.id }));
        dispatched.push(json!({ "wave": wave, "lines": prompt.lines, "read": request_command(root, &spec, *wave), "agent": agent }));
        let code = written.written.code.clone().unwrap_or_else(|| written.written.id.to_string());
        in_flight.insert(*wave, (code, written.written.id));
    }
    // O reenvio: a onda pausada por este relatório, ou a órfã de um Claude
    // Code que fechou, sai de novo com o pedido gravado no envio anterior,
    // palavra por palavra, na mesma cópia — sem
    // montar o pedido de novo —, mais os passos já gravados e o aviso de
    // começar vendo o que mudou na cópia. O envio novo aponta o anterior.
    for (wave, previous) in resends {
        let Some(prior) = log.get(previous) else { continue };
        let Some(own) = recorded_copy(&log, wave) else { continue };
        let copy = if renewed.contains(&wave) {
            // A cópia que outra onda também segura tem a frase dela; a que
            // sumiu, a sua. Quando nenhuma cópia nova saiu, o aviso da
            // criação (`copy-not-created`) já disse por quê.
            let (reason, kept, no_copy) = if moved.contains(&wave) {
                ("resend-copy-moved", "round.resend_moved", "round.resend_no_copy")
            } else {
                ("resend-copy-gone", "round.resend_gone", "round.resend_gone_no_copy")
            };
            let said = |key: &str| translate(key, lang).replace("{wave}", &wave.to_string()).replace("{copy}", &own.path);
            let Some(fresh) = renewed_copies.remove(&wave) else {
                warnings.push(json!({ "reason": "resend-no-copy", "wave": wave, "hint": said(no_copy) }));
                continue;
            };
            warnings.push(json!({ "reason": reason, "wave": wave, "hint": said(kept) }));
            fresh
        } else {
            own
        };
        let steps = wave_steps(&log, wave, &codes);
        let mut draft = Map::new();
        draft.insert("wave".into(), json!(wave));
        draft.insert("role".into(), json!("wave"));
        // As ondas em andamento de agora, e não as do envio anterior: a
        // rodada que reenvia já sabe quem está em curso ([`flight`]).
        let running: Vec<(u64, Vec<String>)> =
            flight.running.iter().filter(|n| **n != wave).map(|n| (*n, wave_files(&log, *n))).collect();
        let text = resend_text(prior.str_field("text").unwrap_or_default(), &running, &steps, lang);
        draft.insert("chars".into(), json!(text.chars().count()));
        draft.insert("lines".into(), json!(text.lines().count()));
        draft.insert("text".into(), json!(text));
        // O modelo e o esforço pedidos são os do envio original: um reenvio
        // não remonta o input, só acrescenta o aviso do que mudou na cópia. O agente é o
        // `wave`, o de toda onda, mesmo quando o envio antigo chamou o agente
        // de tarefa única, que foi juntado a ele e não existe mais no projeto.
        let agent = "wave";
        draft.insert("agent".into(), json!(agent));
        if let Some(model) = prior.str_field("model") {
            draft.insert("model".into(), json!(model));
        }
        if let Some(effort) = prior.str_field("effort") {
            draft.insert("effort".into(), json!(effort));
        }
        draft.insert("items".into(), prior.fields.get("items").cloned().unwrap_or_else(|| json!([])));
        // O pedido do reenvio é o do envio anterior: a lista de leitura dele
        // também, e o envio anterior sem ela segue sem.
        if let Some(read_items) = prior.fields.get("read_items") {
            draft.insert("read_items".into(), read_items.clone());
        }
        draft.insert("mustard".into(), json!(env!("CARGO_PKG_VERSION")));
        draft.insert("copy".into(), json!(copy.path));
        draft.insert("resends".into(), json!(previous));
        draft.insert("claude_pid".into(), json!(claude_pid));
        draft.insert("claude_started".into(), json!(claude_started));
        draft.insert("author".into(), json!("binary"));
        let written = record(&opts.root, &spec, "send", draft, PhaseWriter::Binary)
            .map_err(RoundRefusal::Refused)?;
        recorded.push(json!({ "wave": wave, "type": "send", "id": written.written.id }));
        dispatched.push(json!({ "wave": wave, "lines": text.lines().count(), "read": request_command(root, &spec, wave), "agent": agent }));
        let code = written.written.code.clone().unwrap_or_else(|| written.written.id.to_string());
        in_flight.insert(wave, (code, written.written.id));
    }
    drop(held_lock);

    // O número do `mustard.json` que o pedido de uma onda que saiu agora leu
    // e não vale: a montagem do pedido não tem sessão, e o aviso sai aqui,
    // na volta, uma vez por sessão, como o das perguntas do mapa — as duas
    // guardam a marca pela mesma chave.
    let mut bad_seen: BTreeSet<&str> = BTreeSet::new();
    for setting in next.iter().filter_map(|wave| built.iter().find(|p| p.wave == *wave)).flat_map(|p| &p.bad_settings) {
        if bad_seen.insert(setting.key) && crate::shared::search_door::first_warning(root, session, setting.key) {
            warnings.push(json!({ "reason": "bad-setting", "key": setting.key, "hint": setting.message }));
        }
    }

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
    // andamento; dizer qual onda falta, quando nada se move; rodar de novo,
    // ou nomear as tarefas presas, quando o backlog ainda tem tarefa;
    // esperar o veredito, quando a revisão final segue aberta; ou fechar,
    // com tudo entregue e aprovado e o backlog vazio. A rodada não pede revisão de onda nenhuma:
    // quem confere o trabalho é o agente de teste dedicado que o fechamento
    // pede, uma vez por obra.
    let report_back = super::checks::report_back(root, lang);
    let mut command: Option<String> = None;
    let then = if !dispatched.is_empty() {
        format!("{} {report_back}", translate("round.next", lang))
    } else if !running.is_empty() {
        let waves: Vec<String> = running.keys().map(u64::to_string).collect();
        format!("{} {report_back}", translate("round.waiting", lang).replace("{waves}", &waves.join(", ")))
    } else if !stuck.is_empty() || !held.is_empty() {
        String::new()
    } else if let Some(wave) = first_unfinished(&log, &running) {
        translate("round.missing", lang).replace("{wave}", &wave.to_string())
    } else if !backlog_left(&log).is_empty() {
        // Toda onda planejada terminou, mas o backlog ainda tem tarefa: a obra
        // não fecha. A tarefa que ficou pronta pela entrega desta mesma
        // rodada só vira lote na rodada seguinte, porque o backlog foi formado
        // pela leitura de entrada; sem tarefa pronta e sem nada em andamento,
        // as que sobraram estão presas, e a resposta as nomeia.
        let shown = |ids: &mut dyn Iterator<Item = u64>| -> String {
            ids.map(|id| codes.get(&id).cloned().unwrap_or_else(|| id.to_string())).collect::<Vec<_>>().join(", ")
        };
        let ready = backlog_ready(&log);
        if ready.is_empty() {
            translate("round.backlog_stuck", lang).replace("{tasks}", &shown(&mut backlog_left(&log).into_iter()))
        } else {
            let state = State::from_log(&log);
            let step = crate::commands::flow::resume::step_command("round", &spec, &state);
            let text = translate("round.backlog_left", lang)
                .replace("{tasks}", &shown(&mut ready.into_iter()))
                .replace("{command}", step.as_deref().unwrap_or_default());
            command = step;
            text
        }
    } else if open_review(&log).is_some() {
        // Toda onda está entregue, mas a revisão final segue aberta, sem o
        // veredito do revisor: a obra ainda não está aprovada, e o fechamento
        // recusaria. A resposta manda esperar o veredito, sem o comando de
        // fechar.
        translate("round.review_open", lang).to_string()
    } else {
        let state = State::from_log(&log);
        // A obra já fechada não fecha de novo: a rodada acabou de receber o
        // conserto do pull request reprovado, e o passo é empurrá-lo pela
        // mesma porta que o abriu.
        let (step, key) = if state.phase == Some("pr_open") {
            let reason = translate("round.fix_reason", lang);
            (Some(format!("mustard-rt run reopen --fix --spec {spec} --reason \"{reason}\"")), "round.fix_push")
        } else {
            (crate::commands::flow::resume::step_command(DONE_STEP, &spec, &state), "round.close")
        };
        let text = translate(key, lang).replace("{command}", step.as_deref().unwrap_or_default());
        command = step;
        text
    };
    // A pergunta da onda parada vem antes do resto, que segue sem ela; a da
    // mudança de plano de cada onda que espera o clique vem em seguida.
    let (stopped, question) = stopped_waves(&stuck, &codes, lang);
    let then = question
        .into_iter()
        .chain(held.iter().map(|one| one.next_line(lang)))
        .chain(kept_lines)
        .chain(Some(then).filter(|t| !t.is_empty()))
        .collect::<Vec<_>>()
        .join(" ");

    let running: Vec<Value> = in_flight
        .iter()
        .map(|(wave, (send, sent))| running_state(root, &spec, &log, &codes, *wave, send, *sent))
        .collect();
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
    if let Some(commit) = commit {
        out["commit"] = commit;
    }
    if !warnings.is_empty() {
        out["warnings"] = json!(warnings);
    }
    end_answer(root, &spec, &mut out, &then, lang);
    if let Some(command) = command {
        out["command"] = json!(command);
    }
    Ok(out)
}

/// O comando que lê o pedido gravado no envio da onda `wave`. A resposta da
/// rodada traz este comando no lugar do pedido inteiro, e o agente lê o
/// próprio pedido por ele, de dentro da cópia dele: por isso o comando leva o
/// caminho do repositório principal, onde a spec mora.
fn request_command(root: &Path, spec: &str, wave: u64) -> String {
    read_command(root, spec, &format!("request-{wave}"))
}

/// O texto com que o condutor despacha a onda `wave`: o título do pedido
/// dela, na primeira linha, e o comando que o lê, o mesmo de
/// `dispatch[].read`. O gancho do despacho troca por ele o texto que o
/// condutor escreveu.
pub(crate) fn wave_dispatch(root: &Path, spec: &str, wave: u64, lang: Locale) -> String {
    format!("{}\n\n{}", wave_title(spec, wave, lang), request_command(root, spec, wave))
}

/// O arquivo com o trecho que a conferência depois da onda devolveu para a
/// onda `wave`, gravado pela volta que ela recusou, ou com o motivo de quem
/// conduz a obra, pela volta que ele reprovou: a última entrega da onda
/// que nenhuma rodada assumiu. A entrega nova muda o arquivo, e o trecho de
/// uma volta velha nunca chega ao agente. Nada sem volta pendente da onda.
pub(crate) fn fix_file(root: &Path, spec: &str, log: &SpecLog, wave: u64) -> Option<PathBuf> {
    let back = log.unassumed_returns().into_iter().filter(|e| e.event_type == "delivered" && e.wave() == Some(wave)).map(|e| e.id).max()?;
    Some(fixes_dir(root, spec)?.join(format!("fix-{wave}-{back}.md")))
}

/// A recusa da conferência depois da onda com, para cada onda recusada que
/// espera um agente novo ([`waves_awaiting_new_agent`]), a frase que manda o
/// condutor despachar um pelo título dela; a onda que não está ali segue com
/// o conserto mandado ao agente que a fez. Outra recusa sai como veio.
fn new_agents_named(refused: RoundRefusal, root: &Path, spec: &str, log: &SpecLog, lang: Locale) -> RoundRefusal {
    let RoundRefusal::AfterWave { mut text, question, fixes } = refused else { return refused };
    let awaiting = waves_awaiting_new_agent(root, spec, log);
    for wave in fixes.iter().map(|(wave, _)| *wave).filter(|wave| awaiting.contains_key(wave)) {
        let line = translate("round.after_wave.new_agent", lang)
            .replace("{wave}", &wave.to_string())
            .replace("{title}", &wave_title(spec, wave, lang));
        text = format!("{text}\n\n{line}");
    }
    RoundRefusal::AfterWave { text, question, fixes }
}

/// A pasta de despacho da spec, onde moram os trechos de conserto.
fn fixes_dir(root: &Path, spec: &str) -> Option<PathBuf> {
    Some(store::spec_file(root, spec).ok()?.parent()?.join(".dispatch"))
}

/// A onda do trecho de conserto `path`, pelo nome que [`fix_file`] dá a ele;
/// nada para outro arquivo da pasta.
fn fix_wave(path: &Path) -> Option<u64> {
    let name = path.file_name()?.to_str()?.strip_prefix("fix-")?.strip_suffix(".md")?;
    let (wave, back) = name.split_once('-')?;
    back.parse::<u64>().ok().and(wave.parse().ok())
}

/// Apaga da pasta de despacho da spec cada trecho de conserto que não é mais
/// o da volta pendente da onda dele ([`fix_file`], pela leitura `log`): o da
/// volta velha, que a entrega nova trocou, e o da onda que a rodada já
/// comitou. Sem leitura, como no fechamento, todos saem. A pasta vazia sai
/// junto; o arquivo que não sai fica para a próxima varredura. Quem chama
/// segura a trava do passo do git e leu `log` sob ela: a rodada ao mesmo
/// tempo nunca grava um trecho mais novo que esta leitura.
pub(crate) fn sweep_fixes(root: &Path, spec: &str, log: Option<&SpecLog>) {
    let Some(dir) = fixes_dir(root, spec) else { return };
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    for path in entries.flatten().map(|entry| entry.path()) {
        let Some(wave) = fix_wave(&path) else { continue };
        if log.and_then(|log| fix_file(root, spec, log, wave)).as_ref() != Some(&path) {
            let _ = std::fs::remove_file(&path);
        }
    }
    let _ = std::fs::remove_dir(&dir);
}

/// Grava, para cada onda que a conferência depois da onda recusou, o trecho
/// dela no arquivo da volta recusada ([`fix_file`]), onde o gancho da
/// mensagem ao agente da onda o lê. Antes, toda recusa varre os trechos que
/// ficaram velhos ([`sweep_fixes`]); outra recusa não grava nada, e a falha
/// de gravação deixa a mensagem do condutor passar como veio.
pub(super) fn keep_fixes(root: &Path, spec: &str, log: &SpecLog, refused: &RoundRefusal) {
    sweep_fixes(root, spec, Some(log));
    let RoundRefusal::AfterWave { fixes, .. } = refused else { return };
    for (wave, section) in fixes {
        let Some(file) = fix_file(root, spec, log, *wave) else { continue };
        let _ = file.parent().map(std::fs::create_dir_all);
        let _ = std::fs::write(&file, section);
    }
}

/// O comando `read` de uma leitura da spec (`block`), com o caminho do
/// repositório principal, onde a spec mora, e não o da cópia de quem o roda.
/// É o comando que a resposta entrega no lugar do texto, da onda ou da
/// revisão final.
pub(crate) fn read_command(root: &Path, spec: &str, block: &str) -> String {
    let main = mustard_core::io::wave_prompt::shown(root);
    format!("mustard-rt run read {block} --root {main} --spec {spec}")
}

/// O fim de toda resposta da rodada, a que despacha e a que recusa um ciclo
/// depois de gravar: a cópia para o banco da página sai uma vez, e a rodada
/// manda copiá-la, menos quando ela não pôde ser preparada; e, com o pull
/// request aberto, o corpo dele é refeito do mesmo arquivo de eventos que
/// acabou de mudar — um corpo que descreve a rodada anterior é pior do que
/// nenhum, e foi por isso que existiu um portão só para reparar que ele tinha
/// envelhecido.
///
/// A ordem por extenso vai para a pasta da cópia antes de soltar a trava em
/// que a cópia foi preparada: a rodada que roda ao mesmo tempo começa a
/// preparação dela apagando essa pasta, e acharia ali o arquivo sendo
/// gravado.
fn end_answer(root: &Path, spec: &str, out: &mut Value, then: &str, lang: Locale) {
    use crate::commands::spec_events::pages::{copy, end_milestone};
    let prepared = copy::prepare_then(root, spec, lang, |prepared| {
        end_milestone(out, Ok(prepared), spec, "round", then, lang);
        shorten_publish_order(root, spec, out, then, lang);
    });
    if let Err(refusal) = prepared {
        end_milestone(out, Err(&refusal), spec, "round", then, lang);
    }
    if let Some(number) = rewrite_open_pr(root, spec) {
        out["pr"] = json!({ "number": number, "body": "rewritten" });
    }
}

/// A instrução de publicar e copiar a página, que [`crate::commands::spec_events::pages::end_milestone`]
/// monta por extenso em `out["next"]` — numerada quando há mais de uma
/// ordem —, sai dali quando há página a publicar (`out["publish"]`): o texto
/// vai para `.claude/spec/<spec>/copy/next.md`, sob a pasta da spec, e
/// `out["next"]` fica só com uma linha curta que manda ler o arquivo, seguida
/// do `then`, que já era curto. O que o orquestrador copia não muda, ele
/// mesmo e sem agente: só onde a ordem mora. Sem página a publicar, a ordem
/// é só a da cópia, curta, e fica em `next`: ler um arquivo custaria uma
/// resposta a mais à cópia. Sem instrução de página — `next` já é só o
/// `then` —, nada muda; falha de disco também deixa `next` como estava.
fn shorten_publish_order(root: &Path, spec: &str, out: &mut Value, then: &str, lang: Locale) {
    if out.get("publish").is_none() {
        return;
    }
    let Some(next) = out.get("next").and_then(Value::as_str).map(str::to_string) else { return };
    if next == then {
        return;
    }
    let order = next.strip_suffix(then).map(str::trim_end).unwrap_or(next.as_str());
    if order.is_empty() {
        return;
    }
    let Ok(paths) = mustard_core::ClaudePaths::for_project(root) else { return };
    let Ok(spec_paths) = paths.for_spec(spec) else { return };
    // A mesma pasta dos lotes que a cópia já grava (`pages::copy::FOLDER`):
    // um arquivo a mais ali não muda o que a pasta da spec mostra por fora.
    let path = spec_paths.dir().join(crate::commands::spec_events::pages::copy::FOLDER).join("next.md");
    if mustard_core::io::fs::write_atomic(&path, order.as_bytes()).is_err() {
        return;
    }
    let shown = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
    let short = translate("round.next.copy_file", lang).replace("{path}", &shown);
    out["next"] = json!([short, then.to_string()].into_iter().filter(|s: &String| !s.is_empty()).collect::<Vec<_>>().join(" "));
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
    use crate::commands::flow::round::slots::live_copy;
    use crate::commands::flow::round::tests::*;

    /// A rodada é quem despacha a onda de conserto que a porta do pull
    /// request reprovado abre numa obra já fechada: é por ela que o conserto
    /// chega ao commit, na mesma branch, com a spec parada no pull request
    /// aberto. Sem onda de conserto aberta, a spec fechada continua sem
    /// rodada — a fresta é só essa.
    #[test]
    fn rejected_pull_request_has_a_fix_door_dispatched_by_the_round() {
        use crate::commands::flow::reopen::{reopen_with, ReopenOpts};
        use crate::commands::spec_events::write::{record, record_phase, record_pr_open};
        use mustard_core::domain::spec_state::PhaseWriter;

        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let delivered = json!({"author": "binary", "wave": 1, "text": "A onda 1 entregou.",
            "files": ["src/a.rs"]});
        record(root, "x", "delivered", delivered.as_object().cloned().expect("an object"), PhaseWriter::Binary)
            .expect("the delivery of wave 1");
        assert!(record_phase(root, "x", "closed", None), "the work closes");
        assert!(record_pr_open(root, "x", 9, None), "the pull request opens");

        // Sem onda de conserto, a obra fechada não roda rodada nenhuma.
        let refused = round(root, "x", None);
        assert_eq!(refused["reason"], json!("round-spec-closed"), "{refused}");

        // O vermelho do servidor abre a onda de conserto pela porta.
        let opened = reopen_with(
            &ReopenOpts {
                root: root.to_path_buf(),
                spec: Some("x".into()),
                reason: "o teste do servidor caiu".into(),
                fix: true,
            },
            None,
            &|_, number| {
                assert_eq!(number, 9);
                Ok(crate::shared::pr_provider::PrChecks::Failed)
            },
            &|_, branch| panic!("nada empurra a branch {branch} antes do conserto"),
        );
        assert_eq!(opened["action"], json!("fix"), "{opened}");
        let wave = opened["wave"].as_u64().unwrap_or_else(|| panic!("{opened}"));

        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        let dispatched = out["dispatch"].as_array().cloned().unwrap_or_default();
        assert_eq!(dispatched.len(), 1, "só a onda de conserto sai: {out}");
        assert_eq!(dispatched[0]["wave"], json!(wave), "{out}");
        assert_eq!(
            State::from_log(&store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap()).phase,
            Some("pr_open"),
            "a obra não foi reaberta"
        );
    }

    /// A pasta dos agentes do Mustard no projeto em `root`, já criada, com o
    /// `mustard.json` que escolhe o modelo e o esforço deles.
    fn agents_folder(root: &Path) -> std::path::PathBuf {
        std::fs::write(root.join("mustard.json"), br#"{"agents":{"model":"sonnet","effort":"low"}}"#).unwrap();
        let folder = root.join(".claude/agents/mustard");
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    /// Os avisos da resposta `out` com o motivo `reason`.
    fn warnings_of(out: &Value, reason: &str) -> Vec<Value> {
        let all = out["warnings"].as_array().cloned().unwrap_or_default();
        all.into_iter().filter(|w| w["reason"] == json!(reason)).collect()
    }

    /// A rodada confere os agentes do projeto antes de despachar: o arquivo
    /// com o texto de outra versão volta ao que a instalação escreveria, com
    /// o modelo e o esforço do `mustard.json`, e a resposta o nomeia. O
    /// arquivo já igual não é regravado, nem citado no aviso.
    #[test]
    fn the_round_rewrites_the_stale_agent_file_and_names_it() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let folder = agents_folder(root);
        std::fs::write(folder.join("wave.md"), "---\nname: mustard-wave\nmodel: opus\n---\nO texto antigo.\n").unwrap();
        std::fs::write(folder.join("review.md"), shipped_agent(root, "review")).unwrap();
        let long_ago = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
        std::fs::File::options().write(true).open(folder.join("review.md")).unwrap().set_modified(long_ago).unwrap();

        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["dispatch"].as_array().map(Vec::len), Some(1), "{out}");

        let wave = std::fs::read_to_string(folder.join("wave.md")).unwrap();
        assert_eq!(wave, shipped_agent(root, "wave"));
        assert!(wave.contains("\nmodel: sonnet\n") && wave.contains("\neffort: low\n"), "{wave}");
        let review = std::fs::metadata(folder.join("review.md")).unwrap().modified().unwrap();
        assert_eq!(review, long_ago, "o agente igual foi regravado");
        let said = warnings_of(&out, "agents-refreshed");
        assert_eq!(said.len(), 1, "{out}");
        let hint = said[0]["hint"].as_str().unwrap_or_default();
        assert!(hint.contains(".claude/agents/mustard/wave.md") && !hint.contains("review.md"), "{hint}");
    }

    /// O projeto sem a pasta dos agentes do Mustard não a ganha da rodada, e
    /// a resposta não fala de agente nenhum.
    #[test]
    fn the_round_creates_no_agents_folder_in_a_project_without_it() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);

        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert!(!root.join(".claude/agents").exists(), "a rodada criou a pasta dos agentes");
        assert!(warnings_of(&out, "agents-refreshed").is_empty(), "{out}");
    }

    /// O agente que não pode ser regravado não segura a rodada: ela despacha
    /// do mesmo jeito, e a resposta diz que o agente pode estar com o texto
    /// de outra versão, com o erro.
    #[test]
    fn an_agent_that_cannot_be_rewritten_is_reported_and_the_round_goes_on() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let folder = agents_folder(root);
        std::fs::create_dir_all(folder.join("wave.md")).unwrap();
        std::fs::write(folder.join("wave.md/dentro"), "uma pasta no lugar do agente").unwrap();

        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["dispatch"].as_array().map(Vec::len), Some(1), "{out}");
        assert_eq!(warnings_of(&out, "agents-not-refreshed").len(), 1, "{out}");
    }

    /// A obra aprovada `x`, com a onda 1 entregue e fechada; com `pr`, também
    /// com o pull request 9 aberto.
    fn closed_work(root: &Path, pr: bool) {
        use crate::commands::spec_events::write::{record_phase, record_pr_open};
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let delivered = json!({"author": "binary", "wave": 1, "text": "A onda 1 entregou.", "files": ["src/a.rs"]});
        record(root, "x", "delivered", delivered.as_object().cloned().expect("an object"), PhaseWriter::Binary)
            .expect("the delivery of wave 1");
        assert!(record_phase(root, "x", "closed", None), "the work closes");
        if pr {
            assert!(record_pr_open(root, "x", 9, None), "the pull request opens");
        }
    }

    /// O arquivo de eventos da spec `x`, lido de novo.
    fn log_of(root: &Path) -> SpecLog {
        store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap()
    }

    /// A rodada numa spec fechada, ou com o pull request aberto, sem onda de
    /// conserto aberta recusa com razão própria, e nada é gravado: a frase
    /// diz que a spec fechou e aponta a reabertura, nos dois idiomas. A
    /// frase de spec não aprovada fica para a spec antes da aprovação.
    #[test]
    fn a_round_on_a_closed_spec_points_to_the_reopen() {
        for (language, lang, closed_word) in [("pt-BR", Locale::PtBr, "fechou"), ("en-US", Locale::EnUs, "closed")] {
            for phase in ["closed", "pr_open"] {
                let dir = tempdir().unwrap();
                let root = dir.path();
                closed_work(root, phase == "pr_open");
                std::fs::write(root.join("mustard.json"), format!(r#"{{"language":{{"text":"{language}"}}}}"#))
                    .unwrap();
                let before = std::fs::read(store::spec_file(root, "x").unwrap()).unwrap();

                let refused = round(root, "x", None);
                assert_eq!(refused["ok"], json!(false), "{language} {phase}: {refused}");
                assert_eq!(refused["reason"], json!("round-spec-closed"), "{language} {phase}: {refused}");
                let hint = refused["hint"].as_str().unwrap_or_default();
                assert!(hint.contains("mustard-rt run reopen --spec x"), "{language} {phase}: {hint}");
                assert!(hint.contains(closed_word) && hint.contains(phase), "{language} {phase}: {hint}");
                assert_ne!(hint, translate("round.not_approved", lang).replace("{phase}", phase), "{language}");
                assert!(refused["dispatch"].is_null(), "{refused}");
                assert_eq!(std::fs::read(store::spec_file(root, "x").unwrap()).unwrap(), before, "nothing written");
            }

            // Antes da aprovação, a frase de spec não aprovada, com a fase.
            let dir = tempdir().unwrap();
            let root = dir.path();
            std::fs::write(root.join("mustard.json"), format!(r#"{{"language":{{"text":"{language}"}}}}"#)).unwrap();
            assert_eq!(record_open(root, "x", "feature/x", "dev"), Ok(true));
            let early = round(root, "x", None);
            assert_eq!(early["reason"], json!("round-not-approved"), "{early}");
            assert_eq!(early["hint"], json!(translate("round.not_approved", lang).replace("{phase}", "survey")));
        }
    }

    /// A rodada numa spec entregue na base ou descartada recusa com razão
    /// própria, e nada é gravado: a frase diz que a spec foi entregue ou
    /// descartada e manda abrir uma spec nova, nos dois idiomas. Nunca a
    /// frase de spec não aprovada, nem a reabertura, que ela não aceita.
    #[test]
    fn a_round_on_a_delivered_or_discarded_spec_points_to_a_new_spec() {
        use crate::commands::spec_events::write::record_phase;
        let languages = [
            ("pt-BR", Locale::PtBr, ["entregue", "descartada"], ["não foi aprovada", "reopen"]),
            ("en-US", Locale::EnUs, ["delivered", "discarded"], ["not approved", "reopen"]),
        ];
        for (language, lang, says, never) in languages {
            for phase in ["delivered", "discarded"] {
                let dir = tempdir().unwrap();
                let root = dir.path();
                if phase == "delivered" {
                    closed_work(root, true);
                    assert!(record_phase(root, "x", "delivered", None), "the merge is recorded");
                } else {
                    approved(root, "x", &[(1, &["src/a.rs"], &[])]);
                    let gone = json!({"phase": "discarded", "author": "binary", "reason": "Descartada."});
                    store::write(
                        &store::spec_file(root, "x").unwrap(),
                        "state",
                        gone.as_object().cloned().expect("an object"),
                        &[],
                    )
                    .expect("the discard");
                }
                assert_eq!(State::from_log(&log_of(root)).phase, Some(phase), "{language} {phase}");
                std::fs::write(root.join("mustard.json"), format!(r#"{{"language":{{"text":"{language}"}}}}"#))
                    .unwrap();
                let before = std::fs::read(store::spec_file(root, "x").unwrap()).unwrap();

                let refused = round(root, "x", None);
                assert_eq!(refused["ok"], json!(false), "{language} {phase}: {refused}");
                assert_eq!(refused["reason"], json!("round-spec-finished"), "{language} {phase}: {refused}");
                let hint = refused["hint"].as_str().unwrap_or_default();
                assert!(hint.contains("mustard-rt run open"), "{language} {phase}: {hint}");
                assert!(hint.contains(phase), "{language} {phase}: {hint}");
                for word in says {
                    assert!(hint.contains(word), "{language} {phase}: no {word:?} in {hint}");
                }
                for word in never {
                    assert!(!hint.contains(word), "{language} {phase}: {word:?} in {hint}");
                }
                assert_ne!(hint, translate("round.not_approved", lang).replace("{phase}", phase), "{language}");
                assert!(refused["dispatch"].is_null(), "{refused}");
                assert_eq!(std::fs::read(store::spec_file(root, "x").unwrap()).unwrap(), before, "nothing written");
            }
        }
    }

    /// Depois da reabertura de uma spec fechada, a onda nova que a rodada
    /// grava do backlog, depois do último fechamento, é onda comum: a rodada
    /// a despacha como numa spec em execução, e a leitura da onda de conserto
    /// não a conta. Onda de conserto é só a que traz as chaves do conserto,
    /// numa spec fechada ou com o pull request aberto.
    #[test]
    fn only_a_fix_keyed_wave_of_a_closed_spec_is_a_fix_wave() {
        use crate::commands::flow::reopen::{open_fix_wave_of, reopen_with, ReopenOpts, FIX_KEYS};
        let reopened = |root: &Path| {
            reopen_with(
                &ReopenOpts {
                    root: root.to_path_buf(),
                    spec: Some("x".into()),
                    reason: "Ajustar o pedido.".into(),
                    fix: false,
                },
                None,
                &|_, _| panic!("the reopen without --fix never asks the provider"),
                &|_, branch| panic!("nothing pushes {branch}"),
            )
        };
        // Uma onda gravada pelo binário na spec, depois de tudo o que já está
        // lá, com ou sem as chaves do conserto.
        let wave = |root: &Path, n: u64, keyed: bool| {
            let crit = log_of(root).visible().into_iter().find(|e| e.event_type == "criterion").map(|e| e.id);
            let mut fields = json!({"author": "binary", "n": n, "text": format!("Onda {n}."), "criteria": [crit],
                "done_when": "A suíte passa."});
            if keyed {
                fields["keys"] = json!(FIX_KEYS);
            }
            record(root, "x", "wave", fields.as_object().cloned().expect("an object"), PhaseWriter::Binary)
                .expect("the wave")
                .written
                .id
        };

        // A spec reaberta recebe um pedido novo, e a rodada despacha a onda
        // que ela grava do backlog, depois do fechamento.
        let dir = tempdir().unwrap();
        let root = dir.path();
        closed_work(root, false);
        let closing = State::from_log(&log_of(root)).last_closing.expect("the closing");
        let back = reopened(root);
        assert_eq!(back["phase"], json!("running"), "{back}");
        std::fs::write(root.join("src/b.rs"), "fn dois() {}\n").unwrap();
        git_at(root, &["add", "-A"]);
        git_at(root, &["commit", "-q", "-m", "b"]);
        let said = id_of(&write(root, "x", "message", json!({"author": "user", "text": "Ajustar o pedido."})));
        let crit = log_of(root).visible().into_iter().find(|e| e.event_type == "criterion").map(|e| e.id).unwrap();
        write(root, "x", "task", json!({"text": "Ajustar o pedido.", "files": [{"path": "src/b.rs"}],
            "depends_on": [], "covers": [crit], "origin": said}));

        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        let dispatched = out["dispatch"].as_array().cloned().unwrap_or_default();
        assert_eq!(dispatched.len(), 1, "{out}");
        let n = dispatched[0]["wave"].as_u64().unwrap_or_else(|| panic!("{out}"));
        let log = log_of(root);
        let born = log.visible().into_iter().find(|e| e.event_type == "wave" && e.wave() == Some(n)).expect("the wave");
        assert!(born.id > closing, "the new wave came after the closing");
        assert_eq!(State::from_log(&log).phase, Some("running"));
        assert_eq!(open_fix_wave_of(&log), None, "the new wave of a reopened spec is not a fix wave");

        // Nem com as chaves do conserto a onda de uma spec em execução é onda
        // de conserto.
        wave(root, n + 1, true);
        assert_eq!(open_fix_wave_of(&log_of(root)), None, "a keyed wave of a running spec");

        // Numa spec fechada ou com o pull request aberto, a onda depois do
        // fechamento só é de conserto com as chaves.
        for pr in [false, true] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            closed_work(root, pr);
            wave(root, 2, false);
            assert_eq!(open_fix_wave_of(&log_of(root)), None, "pr {pr}: a wave without the fix keys");
            wave(root, 3, true);
            assert_eq!(open_fix_wave_of(&log_of(root)), Some(3), "pr {pr}: the fix-keyed wave");
        }
    }

    /// A primeira rodada leva a spec para a execução e grava o envio de cada
    /// onda com o pedido exato. A resposta não traz o pedido: traz o comando
    /// que o lê, com o caminho do repositório principal, e as linhas dele; o
    /// comando devolve o pedido gravado, letra por letra.
    #[test]
    fn the_first_round_records_the_request_and_answers_the_command_that_reads_it() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);

        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["phase"], json!("running"), "{out}");
        let dispatched = out["dispatch"].as_array().cloned().unwrap_or_default();
        assert_eq!(dispatched.len(), 1, "{out}");
        assert!(dispatched[0].get("prompt").is_none(), "the answer carries no request: {out}");
        let main = mustard_core::io::wave_prompt::shown(root);
        assert_eq!(dispatched[0]["read"], json!(format!("mustard-rt run read request-1 --root {main} --spec x")), "{out}");
        let prompt = request_at(&out, 0);
        // A tarefa ganha um passo próprio, pelo código, com o arquivo embaixo,
        // e a leitura aparece uma vez só, na seção de como ler — o comando de
        // um item pelo código e o de uma lição —, com o caminho do repositório
        // principal: a onda trabalha na cópia que a rodada criou.
        assert!(prompt.lines().any(|l| l.starts_with("1. Faça a tarefa MSTD-TASK-0001")), "{prompt}");
        assert!(prompt.lines().any(|l| l == "   - Arquivo: `src/a.rs`"), "{prompt}");
        let example = translate("prompt.read.wave", Locale::PtBr)
            .replace("{root}", &format!("--root {} ", mustard_core::io::wave_prompt::shown(root)))
            .replace("{spec}", "x");
        assert!(prompt.contains(&example), "{prompt}");
        assert_eq!(prompt.matches("mustard-rt run read").count(), 2, "{prompt}");

        // O envio gravado guarda o pedido exato, letra por letra.
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let sent: Vec<&SpecEvent> =
            log.visible().into_iter().filter(|e| e.event_type == "send").collect();
        assert_eq!(sent.len(), 1, "um envio por onda despachada");
        assert_eq!(sent[0].str_field("text"), Some(prompt.as_str()));
        assert_eq!(sent[0].wave(), Some(1));
        assert_eq!(dispatched[0]["lines"], json!(sent[0].int("lines")), "the answer keeps the request's size: {out}");
    }

    /// A obra `x` com a onda 1 em `src/a.rs`, cujo último commit ficou fora
    /// da janela cheia do mapa — o pedido lê `map.historyMoves` para montar a
    /// receita do git —, com o `mustard.json` `config`.
    fn work_reading_the_history_moves(root: &Path, config: &str) {
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), config).unwrap();
        let commits: Vec<Value> = (0..mustard_core::domain::project_map::MAX_COMMITS)
            .map(|n| json!({"id": format!("n{n:05}"), "at": 1_789_000_000 + n, "changed": [0]}))
            .collect();
        let model = json!({
            "modules": [{"path": "src/a.rs", "language": "rust", "loc": 30}],
            "history": {"base": "main", "paths": ["src/busy.rs"], "commits": commits},
        });
        mustard_core::io::project_map::write_text(root, &model.to_string()).unwrap();
    }

    /// Uma rodada da obra `x` na sessão `session`, com o scan de mentira que
    /// deixa o mapa como está.
    fn round_in_session(root: &Path, session: Option<&str>) -> Value {
        let opts = RoundOpts { root: root.to_path_buf(), spec: Some("x".to_string()), report: None };
        let project = crate::commands::spec_events::project(root);
        let mine = |_: &Path, _: &Path| Ok(mustard_core::domain::scan::ScanReport::default());
        match run_round_with_mine(&opts, &project.root, project.lang, Caller { session, config_dir: None }, &mine) {
            Ok(out) => out,
            Err(refusal) => refusal.to_value(project.lang),
        }
    }

    /// Os avisos de número inválido da resposta da rodada.
    fn bad_setting_warnings(out: &Value) -> Vec<Value> {
        let all = out["warnings"].as_array().cloned().unwrap_or_default();
        all.into_iter().filter(|w| w["reason"] == json!("bad-setting")).collect()
    }

    /// O pedido da onda que sai lê `map.historyMoves`; o valor inválido — zero,
    /// negativo ou texto — cai no padrão, e a volta da rodada traz o aviso com
    /// a chave e o padrão, uma vez só na sessão: a marca é a mesma da pergunta
    /// do mapa, e a rodada sem sessão avisa sempre. Com o valor certo, ou sem
    /// a chave, nenhum aviso.
    #[test]
    fn the_round_warns_of_an_invalid_history_moves_once_per_session() {
        let default = mustard_core::domain::project_map::MOVES_FOLLOWED.to_string();
        for bad in ["0", "-2", "\"dez\""] {
            let config = format!(r#"{{"map": {{"historyMoves": {bad}}}}}"#);

            // A primeira rodada da sessão avisa e deixa a marca.
            let dir = tempdir().unwrap();
            let root = dir.path();
            work_reading_the_history_moves(root, &config);
            let out = round_in_session(root, Some("sessao-1"));
            assert_eq!(out["dispatch"].as_array().map(Vec::len), Some(1), "{bad}: {out}");
            let warned = bad_setting_warnings(&out);
            assert_eq!(warned.len(), 1, "{bad}: {out}");
            assert_eq!(warned[0]["key"], json!("historyMoves"), "{bad}: {out}");
            let hint = warned[0]["hint"].as_str().unwrap_or_default();
            assert!(hint.contains("map.historyMoves") && hint.contains(&default), "{bad}: {hint}");
            let marker = root.join(".claude/.session/sessao-1/warned-map-historyMoves");
            assert!(marker.is_file(), "{bad}: the round leaves the session's mark");

            // A sessão em que a pergunta do mapa já avisou não recebe o aviso
            // de novo na rodada.
            let dir = tempdir().unwrap();
            let root = dir.path();
            work_reading_the_history_moves(root, &config);
            assert!(crate::shared::search_door::first_warning(root, Some("sessao-2"), "historyMoves"));
            let out = round_in_session(root, Some("sessao-2"));
            assert_eq!(out["dispatch"].as_array().map(Vec::len), Some(1), "{bad}: {out}");
            assert!(bad_setting_warnings(&out).is_empty(), "{bad}: one warning per session: {out}");

            // Sem sessão conhecida, avisa: calar o valor que não vale é pior.
            let dir = tempdir().unwrap();
            let root = dir.path();
            work_reading_the_history_moves(root, &config);
            let out = round_in_session(root, None);
            assert_eq!(bad_setting_warnings(&out).len(), 1, "{bad}: {out}");
        }

        for config in [r#"{"map": {"historyMoves": 2}}"#, "{}"] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            work_reading_the_history_moves(root, config);
            let out = round_in_session(root, Some("sessao-3"));
            assert_eq!(out["dispatch"].as_array().map(Vec::len), Some(1), "{config}: {out}");
            assert!(bad_setting_warnings(&out).is_empty(), "{config}: {out}");
        }
    }

    /// Os avisos da chave do `mustard.json` que o git guarda, na resposta da
    /// rodada.
    fn key_in_git_warnings(out: &Value) -> Vec<Value> {
        let all = out["warnings"].as_array().cloned().unwrap_or_default();
        all.into_iter().filter(|w| w["reason"] == json!("key-in-git")).collect()
    }

    /// Uma obra aprovada em que o `mustard.json` traz `config`, com o git
    /// guardando o arquivo, como o repositório de teste o guarda.
    fn work_with_the_project_file(root: &Path, config: &str) {
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), config).unwrap();
    }

    /// A única chave do Jev está no `mustard.json` que o git guarda: ela não
    /// vale, a rodada monta as ondas pelos arquivos e avisa, uma vez só na
    /// sessão, com o texto no idioma do projeto e sem a chave. Sem sessão
    /// conhecida, avisa sempre. Sem chave, com a chave fora do git ou com o
    /// Jev desligado em `search.filter`, nenhum aviso.
    #[test]
    fn the_round_warns_once_per_session_when_the_only_jev_key_is_in_the_file_git_tracks() {
        const SECRET: &str = "sk-secret-value";
        for (language, lang) in [("pt-BR", Locale::PtBr), ("en-US", Locale::EnUs)] {
            let config = format!(r#"{{"language": {{"text": "{language}"}}, "jev": {{"key": "{SECRET}"}}}}"#);

            // A primeira rodada da sessão avisa e deixa a marca; a segunda, na
            // mesma sessão, cala.
            let dir = tempdir().unwrap();
            let root = dir.path();
            work_with_the_project_file(root, &config);
            let out = round_in_session(root, Some("sessao-1"));
            assert_eq!(out["ok"], json!(true), "{language}: {out}");
            let warned = key_in_git_warnings(&out);
            assert_eq!(warned.len(), 1, "{language}: {out}");
            let hint = warned[0]["hint"].as_str().unwrap_or_default();
            assert_eq!(hint, translate("map.round.key_in_git", lang), "{language}");
            assert!(!out.to_string().contains(SECRET), "{language}: the key never reaches the answer");
            assert!(root.join(".claude/.session/sessao-1/warned-map-round.key_in_git").is_file(), "{language}: the mark");
            let again = round_in_session(root, Some("sessao-1"));
            assert!(key_in_git_warnings(&again).is_empty(), "{language}: one warning per session: {again}");
            let other = round_in_session(root, Some("sessao-2"));
            assert_eq!(key_in_git_warnings(&other).len(), 1, "{language}: another session warns: {other}");

            // Sem sessão conhecida, avisa: calar a chave que não vale é pior.
            let dir = tempdir().unwrap();
            let root = dir.path();
            work_with_the_project_file(root, &config);
            assert_eq!(key_in_git_warnings(&round_in_session(root, None)).len(), 1, "{language}");
        }

        // Nada a avisar: sem chave, com a chave num arquivo que o git não
        // guarda e com o Jev desligado.
        let tracked = format!(r#"{{"jev": {{"key": "{SECRET}"}}}}"#);
        let off = format!(r#"{{"search": {{"filter": "none"}}, "jev": {{"key": "{SECRET}"}}}}"#);
        for (name, config, untracked) in [("no key", "{}", false), ("key outside git", &*tracked, true), ("filter off", &*off, false)] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            work_with_the_project_file(root, config);
            if untracked {
                git_at(root, &["rm", "--cached", "-q", "mustard.json"]);
            }
            let out = round_in_session(root, Some("sessao-3"));
            assert_eq!(out["ok"], json!(true), "{name}: {out}");
            assert!(key_in_git_warnings(&out).is_empty(), "{name}: {out}");
        }
    }

    /// O teto de gasto do mês já gasto segura o Jev: a rodada monta as ondas
    /// sem ele e avisa, uma vez só na sessão, no idioma do projeto; outra
    /// sessão ouve de novo. Abaixo do teto, nenhum aviso.
    #[test]
    fn the_round_warns_once_per_session_when_the_month_budget_holds_the_jev() {
        let budget_warnings = |out: &Value| -> Vec<Value> {
            let all = out["warnings"].as_array().cloned().unwrap_or_default();
            all.into_iter().filter(|w| w["reason"] == json!("jev-over-budget")).collect()
        };
        // A chave mora num `mustard.json` que o git não guarda: com ela, só o
        // teto segura o Jev.
        let work = |root: &Path, config: &str| {
            work_with_the_project_file(root, config);
            git_at(root, &["rm", "--cached", "-q", "mustard.json"]);
        };
        for (language, lang) in [("pt-BR", Locale::PtBr), ("en-US", Locale::EnUs)] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            work(root, &format!(r#"{{"language": {{"text": "{language}"}}, "jev": {{"key": "k", "monthly_budget_usd": 0}}}}"#));
            let out = round_in_session(root, Some("sessao-1"));
            assert_eq!(out["dispatch"].as_array().map(Vec::len), Some(1), "{language}: the round goes on: {out}");
            let hints: Vec<Value> = budget_warnings(&out).into_iter().map(|w| w["hint"].clone()).collect();
            assert_eq!(hints, vec![json!(translate("round.jev_over_budget", lang))], "{language}: {out}");
            let again = round_in_session(root, Some("sessao-1"));
            assert!(budget_warnings(&again).is_empty(), "{language}: one warning per session: {again}");
            let other = round_in_session(root, Some("sessao-2"));
            assert_eq!(budget_warnings(&other).len(), 1, "{language}: another session warns: {other}");
        }

        let dir = tempdir().unwrap();
        let root = dir.path();
        work(root, r#"{"jev": {"key": "k", "monthly_budget_usd": 5}}"#);
        let out = round_in_session(root, Some("sessao-1"));
        assert_eq!(out["dispatch"].as_array().map(Vec::len), Some(1), "{out}");
        assert!(budget_warnings(&out).is_empty(), "below the budget: {out}");
    }

    /// Uma spec aprovada sem onda nenhuma, com `src/a.rs` e `src/b.rs` no
    /// git e duas tarefas soltas no backlog, uma em cada arquivo, que a
    /// primeira rodada monta em duas ondas. Devolve o número da segunda.
    fn approved_backlog(root: &Path) -> u64 {
        std::fs::create_dir_all(root.join("src")).unwrap();
        for name in ["a.rs", "b.rs"] {
            std::fs::write(root.join("src").join(name), "fn one() {}\n").unwrap();
        }
        approved(root, "x", &[]);
        let log = log_of(root);
        let first = |kind: &str| log.visible().into_iter().find(|e| e.event_type == kind).map(|e| e.id).unwrap();
        let (said, crit) = (first("message"), first("criterion"));
        let task = |text: &str, file: &str| {
            id_of(&write(root, "x", "task", json!({"text": text, "files": [{"path": file}],
                "depends_on": [], "covers": [crit], "origin": said})))
        };
        task("Mexer no código de um.", "src/a.rs");
        task("Mexer no código de dois.", "src/b.rs")
    }

    /// A fase gravada da spec `x`, em `root`.
    fn phase_of(root: &Path) -> String {
        State::from_log(&log_of(root)).phase.unwrap_or_default().to_string()
    }

    /// Na primeira rodada de uma spec aprovada, o lote que a gravação recusa
    /// não leva a spec para a execução: ela fica aprovada, e nada do lote
    /// fica gravado — nem a onda, nem a versão da primeira tarefa. A spec
    /// fica igual byte a byte, e a rodada seguinte entra de novo pela
    /// aprovação. A recusa é forçada na segunda tarefa, com um campo que esta
    /// versão não conhece, e a prova atravessa `round`, o comando de verdade.
    #[test]
    fn first_round_with_the_batch_refused_leaves_the_spec_approved() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let bad = approved_backlog(root);
        assert_eq!(phase_of(root), "approved");

        let path = store::spec_file(root, "x").unwrap();
        let mark = format!("\"id\":{bad},");
        let original = std::fs::read_to_string(&path).unwrap();
        let edited: String = original
            .lines()
            .map(|line| if line.contains(&mark) { line.replacen(",\"text\":", ",\"futuro\":1,\"text\":", 1) } else { line.to_string() })
            .map(|line| line + "\n")
            .collect();
        assert_ne!(edited, original, "a linha da segunda tarefa mudou");
        std::fs::write(&path, &edited).unwrap();

        let out = round(root, "x", None);

        assert_eq!(out["ok"], json!(false), "{out}");
        assert_eq!(out["reason"], json!("unknown-field"), "{out}");
        assert_eq!(phase_of(root), "approved", "a spec segue aprovada: {out}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), edited, "a spec fica igual byte a byte");
        assert!(log_of(root).visible().iter().all(|e| e.event_type != "wave"), "nenhuma onda gravada");
    }

    /// Sem recusa, a primeira rodada de uma spec aprovada forma uma onda por
    /// assunto das tarefas soltas — cada uma com a sua, porque não dividem
    /// arquivo —, grava as duas ondas e deixa a spec na execução, na resposta
    /// e no arquivo, como sempre.
    #[test]
    fn first_round_forms_one_wave_per_subject_and_takes_the_spec_to_execution() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved_backlog(root);

        let out = round(root, "x", None);

        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["phase"], json!("running"), "{out}");
        assert_eq!(phase_of(root), "running", "{out}");
        let log = log_of(root);
        let waves: Vec<&SpecEvent> = log.visible().into_iter().filter(|e| e.event_type == "wave").collect();
        assert_eq!(waves.len(), 2, "uma onda por assunto: {out}");
        assert!(waves.iter().all(|wave| wave.str_field("author") == Some("binary")), "{out}");
        for wave in &waves {
            let n = wave.fields.get("n").cloned();
            let in_wave =
                log.visible().into_iter().filter(|e| e.event_type == "task" && e.fields.get("wave").cloned() == n).count();
            assert_eq!(in_wave, 1, "cada onda leva a tarefa do seu assunto: {out}");
        }
        let dispatched = out["dispatch"].as_array().cloned().unwrap_or_default();
        assert_eq!(dispatched.len(), 2, "as duas ondas saem despachadas: {out}");
    }

    /// Toda onda vai ao agente `wave`. A de uma tarefa só grava o `wave` no
    /// envio, e a resposta da rodada o nomeia no despacho e no próximo passo,
    /// que manda ao `mustard-wave` e nunca ao agente de tarefa única, juntado
    /// a ele. O reenvio de um envio antigo gravado com o agente de tarefa
    /// única — pelo nome ou só pelo molde — também sai ao `wave`, sem
    /// remontar o pedido. A onda de várias tarefas vai ao mesmo agente.
    #[test]
    fn a_single_task_wave_is_sent_to_the_wave_agent() {
        let agent_of = |out: &Value| -> String {
            out["dispatch"]
                .as_array()
                .unwrap()
                .iter()
                .find(|d| d["wave"].as_u64() == Some(1))
                .and_then(|d| d["agent"].as_str())
                .unwrap_or_default()
                .to_string()
        };
        let last_send = |root: &Path| -> SpecEvent {
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            log.last_by_wave("send").get(&1).and_then(|id| log.get(*id)).cloned().expect("the send")
        };

        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let first = round(root, "x", None);
        assert_eq!(agent_of(&first), "wave", "{first}");
        let sent = last_send(root);
        assert_eq!(sent.str_field("agent"), Some("wave"), "{sent:?}");
        let next = first["next"].as_str().unwrap_or_default();
        assert!(next.contains("`mustard-wave`") && !next.contains("wave-solo"), "{first}");

        // O envio antigo gravado com o nome do agente de tarefa única, e o
        // mais antigo ainda, que só guardou o molde dele: cada um é reenviado
        // ao `wave`, e o envio novo não guarda o molde.
        for (key, old_value) in [("agent", "wave-solo"), ("template", "---\nname: mustard-wave-solo\n---\n\nO molde.")] {
            let current = last_send(root);
            let mut old = current.fields.clone();
            for gone in ["v", "id", "code", "at", "search", "type", "agent", "template", "resends", "replaces"] {
                old.remove(gone);
            }
            old.insert(key.into(), json!(old_value));
            old.insert("replaces".into(), json!(current.id));
            crate::shared::spec_state::seed_event(root, "x", "send", Value::Object(old));

            let resent = round(root, "x", Some(&line("PAUSED", json!({"wave": 1}))));
            assert_eq!(agent_of(&resent), "wave", "{key}: {resent}");
            let last = last_send(root);
            assert!(last.fields.contains_key("resends"), "{key}: {last:?}");
            assert_eq!(last.str_field("agent"), Some("wave"), "{key}: {last:?}");
            assert_eq!(last.str_field("template"), None, "{key}: {last:?}");
        }

        let multi_dir = tempdir().unwrap();
        let multi_root = multi_dir.path();
        approved_with(multi_root, "x", &[(1, &["src/a.rs"], &[])], |said| {
            write(
                multi_root,
                "x",
                "task",
                json!({"wave": 1, "text": "Tarefa 2 da onda 1.",
                "files": [{"path": "src/a.rs"}], "depends_on": [], "origin": said}),
            );
        });
        let multi_out = round(multi_root, "x", None);
        assert_eq!(agent_of(&multi_out), "wave", "{multi_out}");
    }

    /// O envio grava o nome do agente e não o molde dele; a lista das ondas
    /// e o painel mostram o envio sem o pedido, e só a leitura da onda o
    /// traz inteiro.
    #[test]
    fn reading_the_waves_repeats_neither_the_template_nor_the_request() {
        use crate::commands::spec_events::read::{read_for, ReadOpts};

        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        let prompt = request_at(&out, 0);
        assert!(!prompt.is_empty(), "{out}");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let sent = log.visible().into_iter().find(|e| e.event_type == "send").cloned().expect("the send");
        assert_eq!(sent.str_field("agent"), Some("wave"), "{sent:?}");
        assert_eq!(sent.str_field("template"), None, "{sent:?}");

        let read = |block: &str| -> Value {
            let opts = ReadOpts { root: root.to_path_buf(), spec: Some("x".into()), block: block.into(), term: None };
            serde_json::from_str(&read_for(&opts, None, root).expect("the block reads")).expect("the output is JSON")
        };
        let send_of = |shown: &Value| -> Value {
            shown["events"].as_array().unwrap().iter().find(|e| e["type"] == json!("send")).cloned().expect("send")
        };
        for block in ["waves", "metrics"] {
            let shown = send_of(&read(block));
            assert!(shown.get("text").is_none(), "{block} shows the request: {shown}");
            assert_eq!(shown["agent"], json!("wave"), "{block}: {shown}");
        }
        assert_eq!(send_of(&read("wave-1"))["text"], json!(prompt), "the wave shows the whole request");
    }

    /// A instrução de publicar e copiar a página, por extenso, fica só no
    /// arquivo sob a pasta de lotes da spec: a resposta da rodada leva uma
    /// linha curta que manda lê-lo, sem o texto que cita a ferramenta
    /// `ArtifactData` nem a lista dos lotes. A resposta inteira fica bem
    /// menor do que o texto que foi para o arquivo.
    #[test]
    fn the_round_response_moves_the_publish_order_to_a_file() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);

        let out = round(root, "x", None);
        let next = out["next"].as_str().unwrap_or_default().to_string();
        assert!(next.contains("Leia `.claude/spec/x/copy/next.md`"), "{next}");
        assert!(!next.contains("ArtifactData"), "o texto por extenso não fica na resposta: {next}");

        let file_path = root.join(".claude").join("spec").join("x").join("copy").join("next.md");
        let file_text = std::fs::read_to_string(&file_path).expect("o arquivo com a ordem por extenso");
        assert!(file_text.contains("ArtifactData"), "{file_text}");

        // O que `next` seria sem o desvio para o arquivo (a ordem por
        // extenso mais o "depois" que já era curto) contra o que ele é
        // agora: a resposta da rodada fica bem menor.
        let before = format!("{file_text} …");
        assert!(
            next.len() < before.len(),
            "antes: {} bytes; depois: {} bytes — a resposta devia ficar menor",
            before.len(),
            next.len()
        );
    }

    /// Sem página a publicar, a ordem da cópia é curta e fica na própria
    /// resposta da rodada: nenhum arquivo a ler antes de mandar os lotes, e
    /// o `copy/next.md` nem nasce.
    #[test]
    fn without_a_page_to_publish_the_copy_order_stays_in_the_response() {
        use mustard_core::platform::page_templates::{project_page_template, spec_page_template, template_stamp};
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        for page in ["spec", "project"] {
            let template =
                if page == "spec" { spec_page_template(Locale::PtBr) } else { project_page_template(Locale::PtBr) };
            let stamp = template_stamp(&template).expect("the stamp");
            let url = format!("https://claude.ai/code/artifact/{page}");
            let published = write(root, "x", "publish",
                json!({"page": page, "milestone": "approval", "ok": true, "template": true, "stamp": stamp, "url": url}));
            assert_eq!(published["ok"], json!(true), "{published}");
        }

        let out = round(root, "x", None);
        assert!(out.get("publish").is_none(), "both pages are published: {out}");
        let next = out["next"].as_str().unwrap_or_default();
        assert!(next.contains("ArtifactData") && next.contains("`copy.spec.writes`"), "the order stays: {next}");
        assert!(!next.contains("copy/next.md"), "{next}");
        assert!(!root.join(".claude/spec/x/copy/next.md").exists(), "no file to read");
    }

    /// Os campos de um envio já gravado, prontos para virar a base de um novo
    /// (mesmo texto, mesmos itens, mesma cópia): quem chama troca só o que
    /// precisa. Só os dois testes de onda órfã usam, e eles só valem no
    /// Linux.
    #[cfg(target_os = "linux")]
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
    /// mesma onda, porque a leitura pega sempre o de maior número. Só os
    /// dois testes de onda órfã usam, e eles só valem no Linux.
    #[cfg(target_os = "linux")]
    fn seed_send_at(root: &Path, draft: Value, at: &str) {
        let path = store::spec_file(root, "x").unwrap();
        store::write_at(&path, "send", draft.as_object().cloned().unwrap(), &[], at).unwrap();
    }

    /// O processo e a hora de início de um Claude Code que já fechou: um
    /// processo nascido e já colhido nunca mais aparece com a mesma hora de
    /// início. Só os testes de onda órfã usam, e eles só valem no Linux.
    #[cfg(target_os = "linux")]
    fn closed_sender() -> (u32, u64) {
        let mut dead = std::process::Command::new("true").spawn().expect("spawn the fixture process");
        let pid = dead.id();
        dead.wait().expect("reap the fixture process");
        (pid, 1)
    }

    /// O passo que o agente grava pelo `run write step`; a onda pausada e a
    /// órfã, de um Claude Code que fechou, reenviam o pedido de antes,
    /// palavra por palavra, com os passos e o aviso, e o envio novo aponta o
    /// anterior; a onda de um Claude Code ainda aberto — mesmo depois de um
    /// `/clear`, que não muda o processo do sistema — não é reenviada; e a
    /// onda viva sem sinal por 40 minutos sai como aviso, enquanto a de 39
    /// minutos não sai.
    /// Só roda no Linux: fora dele nenhum processo é dado como morto, então
    /// onda órfã não existe para ser provada.
    #[test]
    #[cfg(target_os = "linux")]
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
        let first_prompt = request_at(&first, 0);

        let path = store::spec_file(root, "x").unwrap();
        let (claude_pid, claude_started) = crate::commands::flow::stuck::sender_process();
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
        let (dead_pid, dead_started) = closed_sender();
        // O envio dela é de antes da vaga fixa: ainda grava a pasta de
        // compilação, o campo antigo que o envio novo não grava mais.
        let mut draft2 = draft2;
        let copy2 = draft2["copy"].clone();
        draft2["claude_pid"] = json!(dead_pid);
        draft2["claude_started"] = json!(dead_started);
        draft2["build_dir"] = json!("/antiga/target/copias/b");
        seed_send_at(root, draft2, &chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string());

        // As ondas 3 e 4 seguem com o mesmo Claude Code, vivo de verdade, mas
        // sem sinal de vida há 39 e 40 minutos.
        for (mut draft, minutes_ago) in [(draft3, 39), (draft4, 40)] {
            draft["claude_pid"] = json!(claude_pid);
            draft["claude_started"] = json!(claude_started);
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

        let prompt_of = |wave: u64| -> String { request_of(&out, wave) };
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
        // O reenvio leva o modelo e o esforço do envio original, sem remontar.
        assert_eq!(previous.str_field("effort"), Some("xhigh"), "{previous:?}");
        assert_eq!(resent_send.str_field("model"), previous.str_field("model"), "{resent_send:?}");
        assert_eq!(resent_send.str_field("effort"), Some("xhigh"), "{resent_send:?}");
        // E a lista de leitura dele, que é a do pedido que volta palavra por palavra.
        let listed = previous.fields.get("read_items").and_then(Value::as_array).cloned().unwrap_or_default();
        assert!(listed.iter().any(|item| item == "MSTD-TASK-0001"), "{previous:?}");
        assert_eq!(resent_send.fields.get("read_items"), previous.fields.get("read_items"), "{resent_send:?}");
        // O envio antigo da onda 2, com a pasta de compilação, é lido e
        // reenviado na mesma cópia; o envio novo não grava a pasta.
        let resent2 = log.visible().into_iter().rfind(|e| e.event_type == "send" && e.wave() == Some(2)).unwrap();
        assert!(resent2.int("resends").is_some(), "{out}");
        assert_eq!(resent2.fields.get("copy"), Some(&copy2), "{out}");
        assert!(resent2.fields.get("build_dir").is_none(), "{out}");
        assert!(resent2.fields.get("read_items").is_none(), "o envio sem lista segue sem ela: {out}");

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

    /// O reenvio de uma onda pausada traz as ondas em andamento de agora, não
    /// as do primeiro envio: a onda 2, em andamento no primeiro envio da onda
    /// 1, entrega no mesmo relatório que pausa a 1, e a vaga dela libera a
    /// onda 4; o pedido reenviado à onda 1 mostra a 3 e a 4 em andamento, e
    /// não mais a 2.
    #[test]
    fn a_resend_shows_the_waves_in_flight_now_not_the_ones_from_the_first_send() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(
            root,
            "x",
            &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[]), (3, &["src/c.rs"], &[]), (4, &["src/d.rs"], &[])],
        );
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":3}"#).unwrap();

        let first = round(root, "x", None);
        let mut started = waves_in(&first, "dispatch");
        started.sort_unstable();
        assert_eq!(started, vec![1, 2, 3], "só 3 vagas: a onda 4 espera: {first}");
        let first_prompt = request_of(&first, 1);
        assert!(first_prompt.contains("Onda 2") && first_prompt.contains("Onda 3"), "{first_prompt}");

        // A onda 2 entrega — libera a vaga dela, que a 4 assume — no mesmo
        // relatório que pausa a onda 1.
        let report = format!("{}\n{}", delivered(root, 2, "Saiu.", &["src/b.rs"]), line("PAUSED", json!({"wave": 1})));
        let out = round(root, "x", Some(&report));
        assert_eq!(out["ok"], json!(true), "{out}");
        let mut sent_now = waves_in(&out, "dispatch");
        sent_now.sort_unstable();
        assert_eq!(sent_now, vec![1, 4], "a 4 assume a vaga da 2, e a 1 reenvia: {out}");

        let resent = request_of(&out, 1);
        assert!(resent.contains("Onda 3") && resent.contains("Onda 4"), "a 3 segue e a 4 entrou: {resent}");
        assert!(!resent.contains("Onda 2"), "a 2 já entregou: a lista velha não segue no reenvio: {resent}");

        // O resto do pedido, antes de "Como trabalhar", não mudou.
        let header = format!("## {}", translate("prompt.part.work", Locale::PtBr));
        let before = |text: &str| text.split(&header).next().unwrap_or_default().to_string();
        assert_eq!(before(&resent), before(&first_prompt), "{resent}");
    }

    /// Duas ondas em andamento, a 1 com o último envio gravando a cópia da 2 —
    /// o estado de uma vaga que passou de uma onda para a outra: o envio da
    /// onda 1 sai vivo (`alive`) ou de um Claude Code que fechou, e a cópia
    /// da onda 2 guarda o trabalho dela sem commit. Devolve essa cópia.
    #[cfg(target_os = "linux")]
    fn two_waves_on_one_copy(root: &Path, alive: bool) -> PathBuf {
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":2}"#).unwrap();
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch").len(), 2, "{first}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let sent_of = |wave: u64| log.visible().into_iter().find(|e| e.wave() == Some(wave) && e.event_type == "send").unwrap();
        let shared = recorded_copy(&log, 2).map(|copy| copy.path).expect("a cópia da onda 2");
        let mut draft = resend_draft(sent_of(1));
        draft["copy"] = json!(shared);
        let (claude_pid, claude_started) =
            if alive { crate::commands::flow::stuck::sender_process() } else { closed_sender() };
        draft["claude_pid"] = json!(claude_pid);
        draft["claude_started"] = json!(claude_started);
        seed_send_at(root, draft, &chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string());
        let copy = PathBuf::from(shared);
        std::fs::write(copy.join("src/b.rs"), "fn one() {}\n// o trabalho da onda 2\n").unwrap();
        copy
    }

    /// O reenvio da pausa não volta à cópia que outra onda viva também segura:
    /// escolhe uma vaga livre, como o envio de uma onda nova, avisa a troca, e
    /// a cópia da outra onda segue com o trabalho dela.
    #[test]
    #[cfg(target_os = "linux")]
    fn the_resend_of_a_paused_wave_never_lands_on_the_copy_another_live_wave_holds() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let shared = two_waves_on_one_copy(root, true);

        let out = round(root, "x", Some(&line("PAUSED", json!({"wave": 1}))));
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "a onda pausada é reenviada: {out}");
        assert_eq!(warning_of(&out, "resend-copy-moved")["wave"], json!(1), "{out}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let resent = recorded_copy(&log, 1).map(|copy| copy.path).expect("a cópia do reenvio");
        assert_ne!(PathBuf::from(&resent), shared, "o reenvio sai em outra vaga: {out}");
        assert_eq!(
            std::fs::read_to_string(shared.join("src/b.rs")).unwrap(),
            "fn one() {}\n// o trabalho da onda 2\n",
            "a cópia da onda 2 fica como estava: {out}"
        );
    }

    /// A onda órfã cuja cópia outra onda viva segura não zera essa cópia: o
    /// trabalho da onda viva fica, e o reenvio da órfã sai em outra vaga.
    #[test]
    #[cfg(target_os = "linux")]
    fn an_orphan_wave_never_cleans_the_copy_another_live_wave_holds() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let shared = two_waves_on_one_copy(root, false);

        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(
            std::fs::read_to_string(shared.join("src/b.rs")).unwrap(),
            "fn one() {}\n// o trabalho da onda 2\n",
            "a limpeza da órfã não apaga o trabalho da onda viva: {out}"
        );
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "a órfã é reenviada: {out}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let resent = recorded_copy(&log, 1).map(|copy| copy.path).expect("a cópia do reenvio");
        assert_ne!(PathBuf::from(&resent), shared, "o reenvio sai em outra vaga: {out}");
    }

    /// A onda 1 enviada, com a pasta da cópia dela apagada do disco antes de a
    /// rodada reenviá-la — o que aconteceu quando uma limpeza de pastas passou
    /// entre o envio e o reenvio. Devolve a cópia que o envio gravou.
    fn wave_one_without_its_copy(root: &Path) -> PathBuf {
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1]);
        let copy = PathBuf::from(recorded_copy(&log_of(root), 1).map(|copy| copy.path).expect("a cópia da onda 1"));
        std::fs::remove_dir_all(&copy).unwrap();
        assert!(!live_copy(&copy), "a cópia saiu do disco");
        copy
    }

    /// Cada envio que a spec `x` tem, de qualquer onda.
    fn sends_of(root: &Path) -> Vec<SpecEvent> {
        log_of(root).visible().into_iter().filter(|e| e.event_type == "send").cloned().collect()
    }

    /// O reenvio de uma onda pausada cuja cópia foi apagada do disco prepara
    /// outra antes de gravar o envio: ela nasce na mesma vaga, no commit atual,
    /// o aviso diz que a cópia é nova, e o envio novo grava uma pasta que o
    /// agente acha pronta.
    #[test]
    fn the_resend_of_a_paused_wave_whose_copy_was_deleted_prepares_a_new_copy_first() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let copy = wave_one_without_its_copy(root);

        let out = round(root, "x", Some(&line("PAUSED", json!({"wave": 1}))));
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "a onda pausada é reenviada: {out}");

        let resent = recorded_copy(&log_of(root), 1).map(|copy| copy.path).expect("a cópia do reenvio");
        assert_eq!(PathBuf::from(&resent), copy, "o reenvio volta à mesma vaga: {out}");
        assert!(live_copy(&copy) && copy.join("src/a.rs").is_file(), "a pasta do reenvio é uma cópia viva: {out}");
        let renewed = warning_of(&out, "resend-copy-gone");
        let said = translate("round.resend_gone", Locale::PtBr).replace("{wave}", "1").replace("{copy}", &resent);
        assert_eq!((renewed["wave"].clone(), renewed["hint"].as_str().unwrap_or_default()), (json!(1), said.as_str()), "{out}");
        assert!(out["warnings"].as_array().into_iter().flatten().all(|w| w["reason"] != json!("resend-copy-moved")), "{out}");
    }

    /// O reenvio da onda órfã cuja cópia foi apagada faz o mesmo: o Claude Code
    /// que a mandou fechou, e a cópia que ela gravou já não está no disco.
    /// Só roda no Linux: fora dele nenhum processo é dado como morto.
    #[test]
    #[cfg(target_os = "linux")]
    fn the_resend_of_an_orphan_wave_whose_copy_was_deleted_prepares_a_new_copy_first() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let copy = wave_one_without_its_copy(root);
        let mut draft = resend_draft(&sends_of(root)[0]);
        let (pid, started) = closed_sender();
        draft["claude_pid"] = json!(pid);
        draft["claude_started"] = json!(started);
        seed_send_at(root, draft, &chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string());

        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "a órfã é reenviada: {out}");
        let resent = recorded_copy(&log_of(root), 1).map(|copy| copy.path).expect("a cópia do reenvio");
        assert_eq!(PathBuf::from(&resent), copy, "{out}");
        assert!(live_copy(&copy), "o reenvio grava uma cópia viva: {out}");
        assert_eq!(warning_of(&out, "resend-copy-gone")["wave"], json!(1), "{out}");
    }

    /// A cópia apagada que a rodada não consegue preparar de novo segura o
    /// reenvio: nenhum envio é gravado, e dois avisos dizem por quê — o da
    /// criação, com o motivo do git, e o da onda que não saiu, com a cópia
    /// que sumiu. Desfeito o impedimento, a rodada seguinte reenvia numa cópia
    /// viva.
    #[test]
    fn a_resend_whose_copy_was_deleted_and_cannot_be_made_again_does_not_go_out_and_says_why() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let copy = wave_one_without_its_copy(root);
        // Um arquivo no lugar da pasta: o git não cria a cópia por cima dele.
        std::fs::write(&copy, b"no caminho da copia").unwrap();
        let before = sends_of(root).len();

        let out = round(root, "x", Some(&line("PAUSED", json!({"wave": 1}))));
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(waves_in(&out, "dispatch"), Vec::<u64>::new(), "a onda não sai: {out}");
        assert_eq!(sends_of(root).len(), before, "nenhum envio foi gravado: {out}");
        assert_eq!(warning_of(&out, "copy-not-created")["wave"], json!(1), "{out}");
        let held = warning_of(&out, "resend-no-copy");
        let said = translate("round.resend_gone_no_copy", Locale::PtBr)
            .replace("{wave}", "1")
            .replace("{copy}", &mustard_core::io::wave_prompt::shown(&copy));
        assert_eq!(held["hint"].as_str().unwrap_or_default(), said, "{out}");
        assert!(held["hint"].as_str().unwrap_or_default().contains("não é mais uma cópia"), "{out}");

        std::fs::remove_file(&copy).unwrap();
        let again = round(root, "x", Some(&line("PAUSED", json!({"wave": 1}))));
        assert_eq!(waves_in(&again, "dispatch"), vec![1], "{again}");
        assert!(live_copy(&copy), "{again}");
        assert_eq!(sends_of(root).len(), before + 1, "{again}");
    }

    /// A pausa de uma onda que já voltou não a reenvia: a volta espera a
    /// rodada, e o reenvio poria a onda por cima da cópia que guarda o que ela
    /// entregou.
    #[test]
    fn a_pause_of_a_wave_that_already_returned_does_not_resend_it() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1]);
        let copy = PathBuf::from(recorded_copy(&log_of(root), 1).map(|copy| copy.path).expect("a cópia"));
        std::fs::write(copy.join("src/a.rs"), "fn one() {}\n// a onda 1 mudou\n").unwrap();
        let asks = json!({"wave": 1, "text": "Parei.", "files": ["src/a.rs"], "commit": "a onda 1 mudou",
            "replan": "A onda 1 precisa de outra tarefa antes.", "changes_decision": DECISION, "undone": []});
        assert_eq!(returned(root, asks)["ok"], json!(true));
        let sends = |root: &Path| log_of(root).events.iter().filter(|e| e.event_type == "send").count();
        let before = sends(root);

        let out = round(root, "x", Some(&line("PAUSED", json!({"wave": 1}))));
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(waves_in(&out, "dispatch"), Vec::<u64>::new(), "{out}");
        assert_eq!(sends(root), before, "nenhum envio novo: {out}");
        assert_eq!(change_asked(&out)["wave"], json!(1), "a volta segue esperando o clique: {out}");
        assert_eq!(std::fs::read_to_string(copy.join("src/a.rs")).unwrap(), "fn one() {}\n// a onda 1 mudou\n");
    }

    /// A onda órfã segue ocupando a vaga dela até o reenvio: com o teto de
    /// compilação em 1, uma onda fresca não sai por cima da órfã na mesma
    /// rodada em que ela é reenviada — a vaga é gravada pela cópia no envio,
    /// e o reenvio a reocupa mesmo antes de outra onda tentar.
    /// Só roda no Linux: fora dele nenhum processo é dado como morto, então
    /// onda órfã não existe para ser provada.
    #[test]
    #[cfg(target_os = "linux")]
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
        let (dead_pid, dead_started) = closed_sender();
        let mut draft1 = draft1;
        draft1["claude_pid"] = json!(dead_pid);
        draft1["claude_started"] = json!(dead_started);
        seed_send_at(root, draft1, &chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string());

        let second = round(root, "x", None);
        assert_eq!(waves_in(&second, "dispatch"), vec![1], "a onda 2 não usa a vaga da órfã: {second}");
    }

    /// O pedido da onda nova traz o comando de compilar do projeto — o de
    /// testar não, porque a suíte é da rodada — e a outra onda que sai junto,
    /// com o arquivo dela. O do conserto traz também o veredito,
    /// a entrega anterior e a decisão gravada depois do envio. Entregue o
    /// conserto, a rodada não pede revisão nenhuma dele: a resposta não traz
    /// o campo `reviews`, e a onda 1 sai da fila sem veredito novo.
    #[test]
    fn the_round_assembles_the_new_request_the_fix_request_and_its_review() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"buildCommand":"make","testCommand":"make test"}"#).unwrap();
        // `make` e `make test` rodam de verdade: a rodada compila e roda a
        // suíte antes de comitar, e sem um Makefile de verdade o teste
        // pegaria a recusa de uma delas em vez do fluxo que ele testa.
        std::fs::write(root.join("Makefile"), "default:\n\t@true\ntest:\n\t@true\n").unwrap();
        let first = request_of(&round(root, "x", None), 1);
        for line in ["- Compile com `make`.", "  - Onda 2: `src/b.rs`"] {
            assert!(first.contains(line), "{line}: {first}");
        }
        assert!(!first.contains("make test"), "{first}");
        assert!(!first.contains(translate("prompt.fix.wave", Locale::PtBr)), "{first}");

        round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        write(root, "x", "decision", json!({"author": "user", "title": "A soma aceita negativos", "text": "A soma aceita negativos.", "keys": ["soma"],
            "why": "o usuário pediu", "waves": [1]}));
        // O veredito final, com o item combinado vigente atendido: sem a
        // lista `agreed`, a revisão final seria recusada por faltar item,
        // antes de a rodada montar o pedido do conserto que este teste prova.
        seed_review(root);
        let rejected_with_agreed = judged(root, json!({"wave": 1, "result": "rejected", "final": true,
            "text": "faltou o teste", "criteria": [{"criterion": "MSTD-CRIT-0001", "tests_rule": true}],
            "agreed": [{"item": "MSTD-DEC-0001", "met": true}]}));
        assert_eq!(rejected_with_agreed["ok"], json!(true), "{rejected_with_agreed}");
        let fix = request_of(&round(root, "x", None), 1);
        let heading =
            format!("## {}\n\n{}", translate("prompt.part.do", Locale::PtBr), translate("prompt.fix.wave", Locale::PtBr));
        assert!(fix.contains(&heading), "{fix}");
        let fix_lines = |text: &str| -> Vec<String> {
            let part = text.split("\n## ").find(|part| part.starts_with(translate("prompt.part.do", Locale::PtBr))).unwrap_or_default();
            part.lines().filter(|l| l.starts_with("- ")).map(str::to_string).collect()
        };
        let lines = fix_lines(&fix);
        assert_eq!(
            lines,
            [
                "- Veredito MSTD-VERD-0001 — faltou o teste",
                "- Entrega MSTD-DELIV-0001 — A soma saiu.",
                "- Decisão MSTD-DEC-0001 — A soma aceita negativos",
            ],
            "{fix}"
        );

        let back = round(root, "x", Some(&delivered(root, 1, "Teste acrescentado.", &["src/a.rs"])));
        assert!(back.get("reviews").is_none(), "a rodada não pede revisão do conserto: {back}");
        assert_eq!(waves_in(&back, "dispatch"), Vec::<u64>::new(), "{back}");
    }

    /// O texto do veredito que o revisor grava traz o veredito e cada achado,
    /// um por linha: o pedido de conserto aponta o código dela pelo mesmo
    /// `fix_lines` de sempre, e ler por esse código devolve os quatro
    /// achados inteiros, sem cortar nenhum.
    #[test]
    fn the_fix_request_carries_every_finding() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));

        let findings = "aprovação recusada\n\
            src/a.rs:12 crítico: falta o teste do sinal negativo\n\
            src/a.rs:20 maior: repete a soma que já existe em src/util.rs\n\
            src/a.rs:5 menor: nome da variável confuso";
        assert_eq!(findings.lines().count(), 4, "quatro achados, um por linha");

        let fix = request_of(&round(root, "x", Some(&verdict(root, 1, "rejected", findings))), 1);
        assert!(fix.contains("MSTD-VERD-0001"), "o pedido de conserto aponta o veredito: {fix}");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let recorded = log.visible().into_iter().find(|e| e.event_type == "verdict").expect("o veredito gravado");
        assert_eq!(
            recorded.str_field("text"),
            Some(findings),
            "os quatro achados chegam inteiros, sem cortar nenhum: {fix}"
        );
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
        let blocked = mustard_core::io::wave_prompt::slot_path(root, "x", 0);
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

    /// Com a revisão final aberta e ainda sem o veredito do revisor, a
    /// rodada que comita a última onda não manda fechar: a resposta manda
    /// esperar o veredito, sem o comando de fechar. Só depois de o veredito
    /// aprovado ser gravado e assumido a rodada manda fechar.
    #[test]
    fn round_waits_for_the_verdict_while_the_final_review_is_open() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1]);
        seed_review(root);

        let done = round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        assert_eq!(done["ok"], json!(true), "{done}");
        assert!(done["commit"]["sha"].is_string(), "a última onda foi comitada: {done}");
        assert!(done.get("command").is_none(), "sem o comando de fechar: {done}");
        let next = done["next"].as_str().unwrap_or_default();
        assert!(next.ends_with(translate("round.review_open", Locale::PtBr)), "{done}");
        assert!(!next.contains("run close"), "a resposta não manda fechar: {done}");

        let approved_now = round(root, "x", Some(&verdict(root, 1, "approved", "Tudo certo.")));
        assert_eq!(approved_now["ok"], json!(true), "{approved_now}");
        assert_eq!(approved_now["command"], json!("mustard-rt run close --spec x"), "{approved_now}");
    }

    /// A última onda em andamento entrega e sobra tarefa no backlog: a rodada
    /// nunca manda fechar. A tarefa que a entrega desta mesma rodada soltou
    /// ainda não virou lote, e a resposta manda rodar de novo, com a linha
    /// pronta; a rodada seguinte despacha o lote, e só com o backlog vazio a
    /// rodada manda fechar. Com tarefa presa — nenhuma pronta e nada em
    /// andamento — a resposta nomeia as tarefas presas e não manda fechar.
    #[test]
    fn round_does_not_say_to_close_with_a_task_in_the_backlog() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved_with(root, "x", &[(1, &["src/a.rs"], &[])], |said| {
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            let first = log.visible().into_iter().find(|e| e.event_type == "task").map(|e| e.id).unwrap();
            let crit = log.visible().into_iter().find(|e| e.event_type == "criterion").map(|e| e.id).unwrap();
            write(root, "x", "task", json!({"text": "Tarefa que espera a da onda 1.",
                "files": [{"path": "src/b.rs"}], "depends_on": [first], "covers": [crit], "origin": said}));
        });
        std::fs::write(root.join("src/b.rs"), "fn dois() {}\n").unwrap();

        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1], "a tarefa solta ainda espera a onda 1: {first}");

        let delivered_now = round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        assert_eq!(delivered_now["ok"], json!(true), "{delivered_now}");
        assert_eq!(waves_in(&delivered_now, "dispatch"), Vec::<u64>::new(), "{delivered_now}");
        let command = delivered_now["command"].as_str().unwrap_or_default();
        assert_eq!(command, "mustard-rt run round --spec x", "com tarefa no backlog, a rodada manda rodar de novo: {delivered_now}");
        crate::commands::flow::resume::assert_parses(command);
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let loose = log
            .visible()
            .into_iter()
            .find(|e| e.event_type == "task" && e.wave().is_none())
            .map(|e| codes.get(&e.id).cloned().unwrap())
            .expect("a tarefa solta segue no backlog");
        let expected = translate("round.backlog_left", Locale::PtBr).replace("{tasks}", &loose).replace("{command}", command);
        assert!(delivered_now["next"].as_str().unwrap_or_default().ends_with(&expected), "{delivered_now}");

        let again = round(root, "x", None);
        assert_eq!(waves_in(&again, "dispatch"), vec![2], "a rodada de novo despacha o lote: {again}");

        let done = round(root, "x", Some(&delivered(root, 2, "Saiu.", &["src/b.rs"])));
        assert_eq!(done["command"], json!("mustard-rt run close --spec x"), "backlog vazio, a rodada manda fechar: {done}");

        // A tarefa presa: duas tarefas soltas que dependem uma da outra, com
        // a onda 1 entregue e nada em andamento. Nenhuma fica pronta, e a
        // rodada as nomeia em vez de mandar fechar.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved_with(root, "x", &[(1, &["src/a.rs"], &[])], |said| {
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            let crit = log.visible().into_iter().find(|e| e.event_type == "criterion").map(|e| e.id).unwrap();
            let one = id_of(&write(root, "x", "task", json!({"text": "Tarefa presa um.",
                "files": [{"path": "src/c.rs"}], "depends_on": [], "covers": [crit], "origin": said})));
            let two = id_of(&write(root, "x", "task", json!({"text": "Tarefa presa dois.",
                "files": [{"path": "src/d.rs"}], "depends_on": [one], "covers": [crit], "origin": said})));
            // A gravação recusa o círculo; ele chega pela spec gravada antes
            // dessa trava, direto no arquivo de eventos.
            crate::shared::spec_state::seed_event(root, "x", "task", json!({"text": "Tarefa presa um.",
                "files": [{"path": "src/c.rs"}], "depends_on": [two], "covers": [crit], "origin": said,
                "replaces": one}));
        });
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1], "as tarefas presas não viram lote: {first}");
        let stuck = round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        assert_eq!(stuck["ok"], json!(true), "{stuck}");
        assert_eq!(waves_in(&stuck, "dispatch"), Vec::<u64>::new(), "{stuck}");
        assert!(stuck.get("command").is_none(), "tarefa presa não tem linha de fechamento: {stuck}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let held: Vec<String> = log
            .visible()
            .into_iter()
            .filter(|e| e.event_type == "task" && e.wave().is_none())
            .map(|e| codes.get(&e.id).cloned().unwrap())
            .collect();
        assert_eq!(held.len(), 2, "{held:?}");
        let expected = translate("round.backlog_stuck", Locale::PtBr).replace("{tasks}", &held.join(", "));
        assert!(stuck["next"].as_str().unwrap_or_default().ends_with(&expected), "{stuck}");
    }

    /// Duas rodadas ao mesmo tempo, sem relatório, com uma onda pronta, leem
    /// a spec antes de qualquer uma pegar a trava. Uma despacha a onda, e a
    /// outra lê a spec, com a trava presa, depois do envio dela: vê a onda em
    /// andamento e não a solta de novo. A onda tem um envio só, as duas
    /// respostas a mostram em andamento com esse envio, e a spec entra na
    /// execução uma vez.
    #[test]
    fn two_rounds_at_the_same_time_dispatch_a_wave_only_once() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);

        let outs = two_rounds_at_once(root, None);
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
            let running = out["running"].as_array().cloned().unwrap_or_default();
            assert_eq!(running.len(), 1, "{outs:?}");
            assert_eq!(running[0]["wave"], json!(1), "{outs:?}");
            assert_eq!(running[0]["send"], json!(sends[0]), "{outs:?}");
        }
        let entered = visible.iter().filter(|e| e.event_type == "state" && e.str_field("phase") == Some("running"));
        assert_eq!(entered.count(), 1, "the spec enters the run once: {outs:?}");
    }

    /// A rodada lista os arquivos que a cópia de uma onda em andamento já
    /// mudou pelo caminho certo, sem a letra de estado do `git status
    /// --porcelain` na frente — inclusive na primeira linha, cujo espaço
    /// inicial o trim da saída inteira apaga —, e o arquivo renomeado sai
    /// com o caminho novo.
    #[test]
    fn the_running_wave_lists_each_changed_file_by_its_path() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs", "src/b.rs", "src/c.rs", "src/e.rs"], &[])]);

        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let copy = recorded_copy(&log, 1).expect("a onda ganhou cópia");
        let copy_path = Path::new(&copy.path);

        // Três arquivos mudam sem entrar no índice: "src/a.rs" abre a lista
        // do git em ordem alfabética, e o estado dela (" M") começa com
        // espaço — a linha que o trim da saída inteira encurta, perdendo o
        // espaço inicial. "src/d.rs" é novo, e "src/e.rs" vira "src/f.rs".
        std::fs::write(copy_path.join("src/a.rs"), "fn dois() {}\n").unwrap();
        std::fs::write(copy_path.join("src/b.rs"), "fn tres() {}\n").unwrap();
        std::fs::write(copy_path.join("src/c.rs"), "fn quatro() {}\n").unwrap();
        std::fs::write(copy_path.join("src/d.rs"), "fn novo() {}\n").unwrap();
        git_at(copy_path, &["mv", "src/e.rs", "src/f.rs"]);

        let running = round(root, "x", None);
        let files: Vec<String> = running["running"][0]["files"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|f| f.as_str().map(str::to_string))
            .collect();

        assert_eq!(
            files,
            vec![
                "src/a.rs".to_string(),
                "src/b.rs".to_string(),
                "src/c.rs".to_string(),
                "src/f.rs".to_string(),
                "src/d.rs".to_string(),
            ],
            "{running}"
        );
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
        let copy = mustard_core::io::wave_prompt::slot_path(root, "x", 98);
        std::fs::create_dir_all(&copy).unwrap();
        let mut orphaned = Command::new("sleep")
            .arg("30")
            .current_dir(&copy)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn a process in the wave's copy");
        crate::commands::flow::stuck::wait_until_spawned(orphaned.id(), "sleep");
        std::fs::remove_dir_all(&copy).unwrap();

        let out = round(root, "x", None);
        let warned = out["warnings"].as_array().cloned().unwrap_or_default();
        let pid = orphaned.id().to_string();
        assert!(warned.iter().any(|w| w["reason"] == json!("stuck-ended") && w["hint"].as_str().unwrap_or_default().contains(&pid)), "{out}");
        let _ = orphaned.wait();
    }

    /// Outra rodada, no meio do despacho, segura a trava do passo do git e
    /// roda o git dentro de uma vaga viva cujo envio ainda não gravou. A
    /// rodada que já entrou — a leitura de entrada, que pega e solta a mesma
    /// trava, veio antes do despacho da outra — espera a trava antes de
    /// procurar processo preso: quando ela procura, o envio daquela vaga já
    /// está gravado, e o git da outra rodada segue vivo, sem aviso de
    /// processo encerrado. Sem relógio: o teste segue quando a lista de
    /// travas do sistema mostra a rodada parada na trava.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_round_never_ends_the_git_of_a_round_preparing_a_slot() {
        use crate::commands::git_settle::{git_step_lock, waiting_for_lock};
        use mustard_core::io::wave_prompt::{shown, slot_path};
        use std::os::unix::fs::MetadataExt;
        use std::process::{Command, Stdio};

        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        // A vaga viva: a pasta de código e o `.git` em arquivo, como a cópia
        // ligada ao projeto que a outra rodada está preparando.
        let slot = slot_path(root, "x", 0);
        std::fs::create_dir_all(slot.join("src")).unwrap();
        std::fs::write(slot.join(".git"), "gitdir: /nowhere\n").unwrap();

        let entry = round_entry(root, None);
        let held = git_step_lock(root).unwrap();
        let lock = root.join(".claude").join("spec").join("round-git.lock");
        let inode = std::fs::metadata(&lock).unwrap().ino();
        let mut preparing = Command::new("sleep")
            .arg("30")
            .current_dir(slot.join("src"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn the other round's git in the slot");
        crate::commands::flow::stuck::wait_until_spawned(preparing.id(), "sleep");

        let out = std::thread::scope(|scope| {
            let arriving = scope.spawn(|| round_from(root, None, entry));
            while !waiting_for_lock(inode) {
                assert!(!arriving.is_finished(), "the round ran without waiting for the git step lock");
                std::thread::yield_now();
            }
            // A outra rodada termina o despacho: grava o envio da onda com a
            // vaga que preparou, por um Claude Code vivo, e solta a trava.
            let (claude_pid, claude_started) = crate::commands::flow::stuck::sender_process();
            crate::shared::spec_state::seed_event(
                root,
                "x",
                "send",
                json!({"wave": 1, "role": "wave", "text": "pedido", "lines": 1, "chars": 6, "items": [],
                    "mustard": "0", "author": "binary", "copy": shown(&slot),
                    "claude_pid": claude_pid, "claude_started": claude_started}),
            );
            drop(held);
            arriving.join().unwrap()
        });

        let alive = preparing.try_wait().ok().flatten().is_none();
        let _ = preparing.kill();
        let _ = preparing.wait();
        assert!(alive, "the other round's git in its slot must stay alive: {out}");
        assert_eq!(out["ok"], json!(true), "{out}");
        let warned = out["warnings"].as_array().cloned().unwrap_or_default();
        assert!(!warned.iter().any(|w| w["reason"] == json!("stuck-ended")), "{out}");
    }

    /// A hora de agora deslocada em `hours` horas: no formato da hora dos
    /// eventos, para gravar um evento à mão, e em segundos, para a hora de um
    /// commit.
    fn hours_from_now(hours: i64) -> (String, i64) {
        let at = chrono::Local::now() + chrono::Duration::hours(hours);
        (at.format("%Y-%m-%dT%H:%M:%S%:z").to_string(), at.timestamp())
    }

    /// Um commit em `root` que muda só `file`, com a hora `epoch`, em
    /// segundos.
    fn commit_at(root: &Path, file: &str, body: &str, epoch: i64) {
        std::fs::write(root.join(file), body).unwrap();
        let date = format!("{epoch} +0000");
        for args in [vec!["add", "--", file], vec!["commit", "-q", "-m", "muda o arquivo"]] {
            let out = std::process::Command::new("git")
                .args(&args)
                .env("GIT_AUTHOR_DATE", &date)
                .env("GIT_COMMITTER_DATE", &date)
                .current_dir(root)
                .output()
                .expect("git");
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        }
    }

    /// A obra aprovada `x` com duas tarefas. A do plano, na onda 1, sobre
    /// `src/a.rs`, foi escrita agora, depois do último commit nesse arquivo.
    /// A outra, sobre `src/b.rs`, teve o texto escrito duas horas atrás, e um
    /// commit mudou o arquivo dela uma hora atrás; com `wave`, ela nasce
    /// nessa onda, e sem, no backlog, de onde a rodada a põe numa onda. Uma
    /// regra do projeto todo entra no pedido por padrão. Devolve o número e o
    /// código dessa tarefa.
    fn task_changed_after_its_text(root: &Path, wave: Option<u64>) -> (u64, String) {
        let (text_at, _) = hours_from_now(-2);
        let mut written = None;
        approved_with(root, "x", &[(1, &["src/a.rs"], &[])], |said| {
            let rule = "Vale sempre: a saudação é curta.";
            id_of(&write(root, "x", "rule", json!({"title": rule, "text": rule, "example": "e", "keys": ["k"],
                "applies_to": {"files": ["**"]}, "origin": said})));
            // O lote que a rodada forma leva os critérios que as tarefas dele
            // cobrem: a tarefa cobre o critério da obra.
            let criterion = log_of(root).visible().into_iter().find(|e| e.event_type == "criterion").map(|e| e.id);
            let mut task = json!({"title": "Entregar a tarefa", "agent": "- conferir pelo teste",
                "text": "Trocar a saudação.", "files": [{"path": "src/b.rs"}], "depends_on": [], "covers": [criterion],
                "origin": said});
            if let Some(wave) = wave {
                task["wave"] = json!(wave);
            }
            let path = store::spec_file(root, "x").unwrap();
            let draft = task.as_object().cloned().expect("an object");
            written = Some(store::write_at(&path, "task", draft, &[], &text_at).expect("the task"));
        });
        let (_, commit) = hours_from_now(-1);
        commit_at(root, "src/b.rs", "fn saudacao() {}\n", commit);
        let written = written.expect("the task was written");
        (written.id, written.code.expect("a task has a code"))
    }

    /// A tarefa com arquivo mudado num commit depois do texto dela sai na
    /// mesma rodada, como qualquer outra: a rodada não para a onda nem pede a
    /// conferência da tarefa no código a quem conduz, porque a espera em que
    /// o pedido aparecia não existe mais.
    #[test]
    fn a_task_whose_file_changed_after_its_text_leaves_in_the_same_round_without_a_check() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (id, code) = task_changed_after_its_text(root, Some(1));

        let out = round(root, "x", None);

        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["dispatch"][0]["wave"], json!(1), "{out}");
        assert!(out.get("analysis").is_none(), "{out}");
        let next = out["next"].as_str().unwrap_or_default();
        assert!(!next.contains(&code) && !next.contains("agente separado"), "{out}");
        assert_eq!(log_of(root).current(id).and_then(|task| task.wave()), Some(1), "the task stays in its wave");
    }

    /// A mudança que a onda manda terminada em ponto aparece no aviso da
    /// mudança registrada com um ponto só; a que vem sem ponto ganha o da
    /// frase.
    #[test]
    fn a_change_ending_in_a_period_shows_one_period_in_the_recorded_change() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        round(root, "x", None);
        let with_period = json!({"wave": 1, "text": "Parei.", "replan": "Dividir a onda em duas.", "undone": []});
        assert_eq!(returned(root, with_period)["ok"], json!(true));
        let bare = json!({"wave": 2, "text": "Parei.", "replan": "Dividir a onda em três", "undone": []});
        assert_eq!(returned(root, bare)["ok"], json!(true));
        let out = round(root, "x", None);
        let warnings: Vec<&Value> = out["warnings"].as_array().into_iter().flatten().filter(|w| w["reason"] == json!("plan-changed")).collect();
        let hint_of = |wave: u64| {
            warnings.iter().find(|w| w["wave"] == json!(wave)).and_then(|w| w["hint"].as_str()).unwrap_or_default().to_string()
        };
        assert!(hint_of(1).contains("A mudança: Dividir a onda em duas. Conte-a"), "{out}");
        assert!(hint_of(2).contains("A mudança: Dividir a onda em três. Conte-a"), "{out}");
        assert!(!hint_of(1).contains(".."), "{out}");
    }
}
