//! A resposta da rodada e o próximo passo: a recusa, com a mensagem no idioma
//! do projeto, e o caminho de uma chamada — conferir a fase, fechar o que
//! voltou, entregar ao orquestrador os candidatos da onda que ainda não tem
//! escolha, despachar as ondas prontas e dizer o que fazer em seguida. A
//! rodada não pede a revisão de onda nenhuma: quem confere o trabalho é o
//! agente de teste dedicado que o fechamento pede, uma vez por obra.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mustard_core::domain::spec_events::{Block, BlockQuery, Refusal, SpecEvent, SpecLog};
use mustard_core::domain::spec_state::{not_closed_yet, returns_to_running, PhaseWriter, SpecState, State};
use mustard_core::domain::wave_prompt::{estimate_tokens, token_cap_message, wave_files};
use mustard_core::io::spec_events as store;
use mustard_core::io::wave_prompt::{prompts, recorded_copy, Flight};
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Map, Value};

use super::commit::git_lock;
use super::queue::{
    analyse, analysis_lines, backlog_left, backlog_ready, dispatch_backlog, emptied_backlog_waves, first_unfinished, max_parallel, next_waves,
    open_copies, open_sends, orphaned_waves, sent_items, silent_minutes, task_files, waves_in_progress, Analysed,
};
use super::report::Taken;
use super::stops::{change_question, stopped_waves, waves_stuck};
use super::{can_run, RoundOpts, DONE_STEP};
use crate::commands::spec_events::{read::checkout, write::record};
use crate::commands::wave::wave_overlap_check::wave_graph;
use crate::shared::spec_state::DiskSpecState;

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
    /// Um agente disse que o plano da onda não funciona: a rodada para e
    /// mostra a mudança proposta, com a pergunta que decide.
    Replan { wave: u64, change: String, code: String },
    /// O git recusou o commit.
    Git { detail: String },
    /// O repositório principal não compilou antes do commit da rodada.
    BuildFailed { command: String, output: String },
    /// A prova de um critério que as ondas deste relatório cobrem não
    /// executou ou não passou: nada foi comitado.
    CriterionProofFailed { code: String, command: String, output: String },
    /// O pedido de uma onda passa do teto de tokens: a rodada recusa antes de
    /// gravar o envio, com o tamanho medido e o teto.
    TokenCap { wave: u64, tokens: u64 },
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
            Self::Git { .. } => "git-refused".into(),
            Self::BuildFailed { .. } => "round-build-failed".into(),
            Self::CriterionProofFailed { .. } => "round-criterion-proof-failed".into(),
            Self::TokenCap { .. } => "wave-token-cap".into(),
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
            Self::Replan { wave, change, code } => fill(
                "round.replan",
                &[
                    ("{wave}", wave.to_string()),
                    ("{change}", change.clone()),
                    ("{question}", change_question(*wave, change, lang)),
                    ("{code}", code.clone()),
                    ("{yes}", translate("change.accept", lang).to_string()),
                    ("{no}", translate("change.decline", lang).to_string()),
                ],
            ),
            Self::Git { detail } => fill("round.git_refused", &[("{detail}", detail.clone())]),
            Self::BuildFailed { command, output } => {
                fill("round.build_failed", &[("{command}", command.clone()), ("{output}", output.clone())])
            }
            Self::CriterionProofFailed { code, command, output } => fill(
                "round.criterion_proof_failed",
                &[("{code}", code.clone()), ("{command}", command.clone()), ("{output}", output.clone())],
            ),
            Self::TokenCap { wave, tokens } => {
                token_cap_message(*wave, *tokens, lang).unwrap_or_default()
            }
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
        // A pergunta da mudança vai pronta, em palavras, com as opções e com
        // o código que vai no cabeçalho dela: o enunciado quem pergunta pode
        // reescrever com as palavras do usuário, e é o cabeçalho, não a
        // frase, que diz à testemunha qual mudança o clique decide.
        if let Self::Replan { wave, change, code } = self {
            out["question"] = json!(change_question(*wave, change, lang));
            out["header"] = json!(code);
            out["options"] = json!([translate("change.accept", lang), translate("change.decline", lang)]);
        }
        out
    }
}

/// A recusa das ondas `cycle`, que dependem umas das outras em círculo.
fn wave_loop_message(cycle: &[u64], lang: Locale) -> String {
    let waves: Vec<String> = cycle.iter().map(u64::to_string).collect();
    translate("plan.wave_loop", lang).replace("{waves}", &waves.join(", "))
}

/// As ondas a reenviar nesta rodada, cada uma com o número do pedido
/// anterior: as órfãs, de um Claude Code que fechou, e as pausadas por este
/// relatório — a onda pausada sai de novo na mesma rodada. A onda de lote que
/// perdeu todas as tarefas para o backlog — o que o corte de uma onda de lote,
/// no mesmo relatório, acabou de fazer — nunca entra aqui: sem tarefa
/// nenhuma, reenviar seria despachar uma onda vazia.
fn resend_targets(log: &SpecLog, paused: &[u64]) -> BTreeMap<u64, u64> {
    let last_sends = log.last_by_wave("send");
    let mut out = orphaned_waves(log);
    for wave in paused {
        if let Some(sent) = last_sends.get(wave) {
            out.entry(*wave).or_insert(*sent);
        }
    }
    let graph = wave_graph(log);
    let emptied = emptied_backlog_waves(log, &graph);
    out.retain(|wave, _| !emptied.contains(wave));
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

/// As tarefas das ondas `waves` que pedem conferência no código, pelo
/// código: as que têm arquivo declarado mudado num commit da branch
/// posterior ao texto vigente delas. Sem git, sem arquivo declarado ou sem
/// hora legível, a tarefa fica fora.
fn tasks_to_check(root: &Path, log: &SpecLog, waves: &BTreeSet<u64>, codes: &BTreeMap<u64, String>) -> Vec<String> {
    log.visible()
        .into_iter()
        .filter(|e| e.event_type == "task" && e.wave().is_some_and(|wave| waves.contains(&wave)))
        .filter(|task| changed_after_text(root, log, task))
        .map(|task| codes.get(&task.id).cloned().unwrap_or_else(|| task.id.to_string()))
        .collect()
}

/// `true` quando o último commit que toca um arquivo declarado da tarefa é
/// posterior ao texto vigente dela: o instante da última versão que mudou o
/// texto ou os arquivos. A versão que só põe a tarefa numa onda — que a
/// rodada grava antes de mostrar a escolha — herda o instante da anterior;
/// contada, ela esconderia todo commit anterior à própria rodada.
fn changed_after_text(root: &Path, log: &SpecLog, task: &SpecEvent) -> bool {
    let files = task_files(task);
    if files.is_empty() {
        return false;
    }
    let mut written = task;
    while let Some(previous) = written.replaced().first().and_then(|id| log.get(*id)) {
        if previous.fields.get("text") != written.fields.get("text") || task_files(previous) != files {
            break;
        }
        written = previous;
    }
    let Ok(at) = chrono::DateTime::parse_from_rfc3339(written.at().trim()) else {
        return false;
    };
    let mut args = vec!["log", "-1", "--format=%ct", "HEAD", "--"];
    args.extend(files.iter().map(String::as_str));
    mustard_core::platform::git::run(root, &args)
        .out()
        .and_then(|seconds| seconds.parse::<i64>().ok())
        .is_some_and(|commit| commit > at.timestamp())
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

/// A parte das ondas em andamento do pedido anterior (`## Execução`), trocada
/// pela de agora: a marca é a linha do rótulo
/// (`prompt.execution.running`) e as linhas indentadas logo depois dela, uma
/// por onda, no mesmo formato que o primeiro envio escreve. Sem onda em
/// andamento nenhuma agora, a linha do rótulo e as dela somem; sem elas no
/// texto anterior e com onda em andamento agora, elas nascem no fim da
/// seção. Sem a seção `## Execução` no texto anterior, nada muda.
fn refresh_running(previous: &str, running: &[(u64, Vec<String>)], lang: Locale) -> String {
    let header = format!("## {}", translate("prompt.part.execution", lang));
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

    // A spec antiga passa para o backlog antes de qualquer leitura das ondas:
    // a onda desenhada à mão que nunca saiu sai da leitura, e as tarefas dela
    // entram no backlog. Numa spec já convertida nada é gravado, e a leitura
    // segue a mesma.
    let log = if super::convert::convert_hand_waves(&opts.root, root, &spec, lang)
        .map_err(RoundRefusal::Refused)?
        .is_empty()
    {
        log
    } else {
        store::read(&path)
            .map_err(RoundRefusal::Refused)?
            .ok_or_else(|| RoundRefusal::Refused(Refusal::NoSpecFile { spec: spec.clone() }))?
    };

    // O backlog é lido como estava ao entrar na rodada, antes de o relatório
    // dela mexer em onda ou tarefa: a tarefa que o corte de uma onda de lote
    // devolve solta, agora mesmo, fica solta até a rodada seguinte — só a que
    // já estava pronta antes desta rodada começar é empacotada aqui.
    let log_on_entry = log.clone();

    // A rodada assume, antes de despachar, a volta que cada onda gravou na
    // spec, com ou sem relatório: o relatório traz só as linhas de quem
    // despacha. Sem volta e sem linha a assumir, nada é juntado, e a rodada
    // vai direto ao despacho, com a escolha de cada onda.
    let raw = opts.report.as_deref().map(str::trim).filter(|r| !r.is_empty());
    let Taken { mut recorded, formatted, mut warnings, commit, paused } =
        super::report::take_report_with_mine(&opts.root, root, &spec, raw, &log, lang, mine)?;
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

    // O mapa volta ao commit atual antes de montar os pedidos: um commit à
    // mão ou um pull podem ter mudado o código fora da rodada, e sem isto a
    // sugestão da onda seguinte apontaria linhas velhas.
    super::commit::refresh_map_if_stale(root, mine);

    // O backlog forma os lotes das tarefas que já estavam prontas antes desta
    // rodada começar, pela leitura de entrada (`log_on_entry`): o binário
    // grava a onda e as tarefas dela, com autor próprio, e só depois a
    // rodada lê as ondas que existem — as novas e as já entregues. A tarefa
    // que o corte de uma onda de lote acabou de devolver solta, no relatório
    // desta mesma chamada, fica solta até a rodada seguinte; formar o lote
    // pela leitura já mexida pelo relatório a empacotaria de novo na mesma
    // rodada que a soltou.
    dispatch_backlog(&opts.root, &spec, &log_on_entry).map_err(RoundRefusal::Refused)?;
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
    // A escolha antes do envio, antes da cópia: a onda com item do projeto
    // todo, item sem dono ou lição a julgar só sai com a escolha do
    // orquestrador; sem ela, a resposta traz os candidatos dela, e a onda fica
    // para a rodada que trouxer a escolha.
    let Analysed { go, choices, asked, warnings: ignored } = analyse(root, &log, &ready, &given, lang);
    warnings.extend(ignored);
    let (copies, not_copied) = open_copies(root, &spec, &log, &held_lock, &go, &occupied, false, lang);
    warnings.extend(not_copied);
    let next: Vec<u64> = go.into_iter().filter(|wave| copies.contains_key(wave)).collect();
    // O pedido de cada onda lista as outras em andamento, contando as que
    // saem junto com ela nesta rodada, e traz a cópia dela e a escolha do
    // orquestrador.
    let flight = Flight { running: occupied.keys().chain(&next).copied().collect(), copies, choices };
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
        draft.insert("claude_pid".into(), json!(claude_pid));
        draft.insert("claude_started".into(), json!(claude_started));
        draft.insert("author".into(), json!("binary"));
        let written = record(&opts.root, &spec, "send", draft, PhaseWriter::Binary)
            .map_err(RoundRefusal::Refused)?;
        recorded.push(json!({ "wave": wave, "type": "send", "id": written.written.id }));
        dispatched.push(json!({ "wave": wave, "lines": prompt.lines, "prompt": prompt.text, "agent": agent }));
        let code = written.written.code.clone().unwrap_or_else(|| written.written.id.to_string());
        in_flight.insert(*wave, (code, written.written.id));
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
        // As ondas em andamento de agora, e não as do envio anterior: a
        // rodada que reenvia já sabe quem está em curso ([`flight`]).
        let running: Vec<(u64, Vec<String>)> =
            flight.running.iter().filter(|n| **n != wave).map(|n| (*n, wave_files(&log, *n))).collect();
        let text = resend_text(prior.str_field("text").unwrap_or_default(), &running, &steps, lang);
        draft.insert("chars".into(), json!(text.chars().count()));
        draft.insert("lines".into(), json!(text.lines().count()));
        draft.insert("text".into(), json!(text));
        // O modelo pedido é o do envio original: um reenvio não remonta o
        // input, só acrescenta o aviso do que mudou na cópia. O agente é o
        // `wave`, o de toda onda, mesmo quando o envio antigo chamou o agente
        // de tarefa única, que foi juntado a ele e não existe mais no projeto.
        let agent = "wave";
        draft.insert("agent".into(), json!(agent));
        if let Some(model) = prior.str_field("model") {
            draft.insert("model".into(), json!(model));
        }
        draft.insert("items".into(), prior.fields.get("items").cloned().unwrap_or_else(|| json!([])));
        draft.insert("mustard".into(), json!(env!("CARGO_PKG_VERSION")));
        draft.insert("copy".into(), json!(copy.path));
        if let Some(dir) = &copy.build_dir {
            draft.insert("build_dir".into(), json!(dir));
        }
        draft.insert("resends".into(), json!(previous));
        draft.insert("claude_pid".into(), json!(claude_pid));
        draft.insert("claude_started".into(), json!(claude_started));
        draft.insert("author".into(), json!("binary"));
        let written = record(&opts.root, &spec, "send", draft, PhaseWriter::Binary)
            .map_err(RoundRefusal::Refused)?;
        recorded.push(json!({ "wave": wave, "type": "send", "id": written.written.id }));
        dispatched.push(json!({ "wave": wave, "lines": text.lines().count(), "prompt": text, "agent": agent }));
        let code = written.written.code.clone().unwrap_or_else(|| written.written.id.to_string());
        in_flight.insert(wave, (code, written.written.id));
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
    // andamento; dizer qual onda falta, quando nada se move; rodar de novo,
    // ou nomear as tarefas presas, quando o backlog ainda tem tarefa; ou
    // fechar, com tudo entregue e aprovado e o backlog vazio. A rodada não pede revisão de onda nenhuma:
    // quem confere o trabalho é o agente de teste dedicado que o fechamento
    // pede, uma vez por obra.
    let report_back = translate("round.report", lang);
    let mut command: Option<String> = None;
    let then = if !dispatched.is_empty() {
        format!("{} {report_back}", translate("round.next", lang))
    } else if !asked.is_empty() {
        String::new()
    } else if !running.is_empty() {
        let waves: Vec<String> = running.keys().map(u64::to_string).collect();
        format!("{} {report_back}", translate("round.waiting", lang).replace("{waves}", &waves.join(", ")))
    } else if !stuck.is_empty() {
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
    // A pergunta da onda parada vem antes do resto, que segue sem ela; o
    // pedido da escolha vem logo depois.
    let (stopped, question) = stopped_waves(&stuck, &codes, lang);
    let waiting: Vec<String> = asked.iter().filter_map(|a| a["wave"].as_u64()).map(|n| n.to_string()).collect();
    // A conferência das tarefas no código só entra quando um commit mudou
    // arquivo de alguma delas depois do texto; sem nenhuma, a frase não sai.
    let analysis = (!asked.is_empty()).then(|| {
        let choice = translate("round.analysis", lang).replace("{waves}", &waiting.join(", "));
        let waves: BTreeSet<u64> = asked.iter().filter_map(|a| a["wave"].as_u64()).collect();
        let tasks = tasks_to_check(root, &log, &waves, &codes);
        if tasks.is_empty() {
            choice
        } else {
            format!("{choice} {}", translate("round.analysis_check", lang).replace("{tasks}", &tasks.join(", ")))
        }
    });
    let then = question
        .into_iter()
        .chain(analysis)
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
    if !asked.is_empty() {
        out["analysis"] = json!(asked);
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

/// O fim de toda resposta da rodada, a que despacha e a que recusa um ciclo
/// depois de gravar: a cópia para o banco da página sai uma vez, e a rodada
/// manda copiá-la, menos quando ela não pôde ser preparada; e, com o pull
/// request aberto, o corpo dele é refeito do mesmo arquivo de eventos que
/// acabou de mudar — um corpo que descreve a rodada anterior é pior do que
/// nenhum, e foi por isso que existiu um portão só para reparar que ele tinha
/// envelhecido.
fn end_answer(root: &Path, spec: &str, out: &mut Value, then: &str, lang: Locale) {
    let prepared = crate::commands::spec_events::pages::copy::prepare(root, spec, lang);
    crate::commands::spec_events::pages::end_milestone(out, prepared.as_ref(), spec, "round", then, lang);
    shorten_publish_order(root, spec, out, then, lang);
    if let Some(number) = rewrite_open_pr(root, spec) {
        out["pr"] = json!({ "number": number, "body": "rewritten" });
    }
}

/// A instrução de publicar e copiar a página, que [`crate::commands::spec_events::pages::end_milestone`]
/// monta por extenso em `out["next"]` — com a lista inteira dos lotes, numerada
/// quando há mais de uma ordem —, sai dali: o texto vai para
/// `.claude/spec/<spec>/copy/next.md`, sob a pasta da spec, e `out["next"]`
/// fica só com uma linha curta que manda ler o arquivo, seguida do `then`, que
/// já era curto. O que o orquestrador copia não muda, ele mesmo e sem agente:
/// só onde a ordem mora. Sem instrução de página — `next` já é só o `then` —,
/// nada muda; falha de disco também deixa `next` como estava.
fn shorten_publish_order(root: &Path, spec: &str, out: &mut Value, then: &str, lang: Locale) {
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

    /// A rodada é quem despacha a onda de conserto que a porta do pull
    /// request reprovado abre numa obra já fechada: é por ela que o conserto
    /// chega ao commit, na mesma branch, com a spec parada no pull request
    /// aberto. Sem onda de conserto aberta, a spec fechada continua sem
    /// rodada — a fresta é só essa.
    #[test]
    fn o_pull_request_reprovado_tem_porta_de_conserto_despachada_pela_rodada() {
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
        // A tarefa ganha linha própria, pelo código, e a leitura aparece uma
        // vez só, na linha de como ler — o comando do pedido inteiro e o de
        // um item pelo código —, com o caminho do repositório principal: a
        // onda trabalha na cópia que a rodada criou.
        assert!(prompt.lines().any(|l| l.starts_with("- `MSTD-TASK-0001`") && l.contains("src/a.rs")), "{prompt}");
        let example = translate("prompt.read.wave", Locale::PtBr)
            .replace("{root}", &format!("--root {} ", mustard_core::io::wave_prompt::shown(root)))
            .replace("{spec}", "x")
            .replace("{n}", "1");
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
    fn a_leitura_das_ondas_nao_repete_o_molde_nem_o_pedido() {
        use crate::commands::spec_events::read::{read_for, ReadOpts};

        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        let prompt = out["dispatch"][0]["prompt"].as_str().unwrap_or_default().to_string();
        assert!(!prompt.is_empty(), "{out}");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let sent = log.visible().into_iter().find(|e| e.event_type == "send").cloned().expect("the send");
        assert_eq!(sent.str_field("agent"), Some("wave"), "{sent:?}");
        assert_eq!(sent.str_field("template"), None, "{sent:?}");

        let read = |block: &str| -> Value {
            let opts = ReadOpts { root: root.to_path_buf(), spec: Some("x".into()), block: block.into(), term: None };
            serde_json::from_str(&read_for(&opts, None).expect("the block reads")).expect("the output is JSON")
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

    /// Os campos de um envio já gravado, prontos para virar a base de um novo
    /// (mesmo texto, mesmos itens, mesma cópia): quem chama troca só o que
    /// precisa. Só os dois testes de onda órfã usam, e eles só valem no
    /// Linux.
    #[cfg(target_os = "linux")]
    fn resend_draft(sent: &SpecEvent) -> Value {
        let mut draft = json!({
            "wave": sent.wave().unwrap(),
            "role": "wave",
            "text": sent.str_field("text").unwrap_or_default(),
            "lines": sent.int("lines").unwrap_or(1),
            "chars": sent.int("chars").unwrap_or(1),
            "items": sent.fields.get("items").cloned().unwrap_or_else(|| json!([])),
            "mustard": "0",
            "copy": sent.str_field("copy").unwrap_or_default(),
        });
        // A pasta de compilação segue com o pedido: sem ela, a rodada
        // seguinte acha a vaga livre mesmo com a cópia desta onda ainda lá,
        // e deixa duas ondas dividirem a mesma pasta.
        if let Some(dir) = sent.str_field("build_dir") {
            draft["build_dir"] = json!(dir);
        }
        draft
    }

    /// Grava um envio à mão, com a hora `at`: supera o envio mais novo da
    /// mesma onda, porque a leitura pega sempre o de maior número. Só os
    /// dois testes de onda órfã usam, e eles só valem no Linux.
    #[cfg(target_os = "linux")]
    fn seed_send_at(root: &Path, draft: Value, at: &str) {
        let path = store::spec_file(root, "x").unwrap();
        store::write_at(&path, "send", draft.as_object().cloned().unwrap(), &[], at).unwrap();
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
        let first_prompt = first["dispatch"][0]["prompt"].as_str().unwrap_or_default().to_string();

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
        let first_prompt = first["dispatch"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["wave"].as_u64() == Some(1))
            .and_then(|d| d["prompt"].as_str())
            .unwrap_or_default()
            .to_string();
        assert!(first_prompt.contains("Onda 2") && first_prompt.contains("Onda 3"), "{first_prompt}");

        // A onda 2 entrega — libera a vaga dela, que a 4 assume — no mesmo
        // relatório que pausa a onda 1.
        let report = format!("{}\n{}", delivered(root, 2, "Saiu.", &["src/b.rs"]), line("PAUSED", json!({"wave": 1})));
        let out = round(root, "x", Some(&report));
        assert_eq!(out["ok"], json!(true), "{out}");
        let mut sent_now = waves_in(&out, "dispatch");
        sent_now.sort_unstable();
        assert_eq!(sent_now, vec![1, 4], "a 4 assume a vaga da 2, e a 1 reenvia: {out}");

        let resent = out["dispatch"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["wave"].as_u64() == Some(1))
            .and_then(|d| d["prompt"].as_str())
            .unwrap_or_default()
            .to_string();
        assert!(resent.contains("Onda 3") && resent.contains("Onda 4"), "a 3 segue e a 4 entrou: {resent}");
        assert!(!resent.contains("Onda 2"), "a 2 já entregou: a lista velha não segue no reenvio: {resent}");

        // O resto do pedido, antes da seção de execução, não mudou.
        let header = format!("## {}", translate("prompt.part.execution", Locale::PtBr));
        let before = |text: &str| text.split(&header).next().unwrap_or_default().to_string();
        assert_eq!(before(&resent), before(&first_prompt), "{resent}");
    }

    /// A onda órfã segue ocupando a vaga e a cópia dela até o reenvio: com o
    /// teto de compilação em 1, uma onda fresca não sai por cima da órfã na
    /// mesma rodada em que ela é reenviada — a pasta de compilação é a mesma,
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
        // `make` é o comando de compilação de verdade agora: a rodada roda
        // ele antes de comitar, e sem um Makefile de verdade o teste
        // pegaria a recusa de build em vez do fluxo que ele testa.
        std::fs::write(root.join("Makefile"), "default:\n\t@true\n").unwrap();
        let text = |out: &Value, field: &str, wave: u64| -> String {
            let found = out[field].as_array().into_iter().flatten().find(|d| d["wave"] == json!(wave));
            found.and_then(|d| d["prompt"].as_str()).unwrap_or_default().to_string()
        };
        let first = text(&round(root, "x", None), "dispatch", 1);
        for line in ["- Compile com `make`.", "- Teste com `make test`.", "  - Onda 2: `src/b.rs`"] {
            assert!(first.contains(line), "{line}: {first}");
        }
        assert!(!first.contains(translate("prompt.fix.wave", Locale::PtBr)), "{first}");

        round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        write(root, "x", "decision", json!({"author": "user", "text": "A soma aceita negativos.", "keys": ["soma"],
            "why": "o usuário pediu", "waves": [1]}));
        // O veredito final, com o item combinado vigente atendido: sem a
        // lista `agreed`, a revisão final seria recusada por faltar item,
        // antes de a rodada montar o pedido do conserto que este teste prova.
        seed_review(root);
        let rejected_with_agreed = judged(root, json!({"wave": 1, "result": "rejected", "final": true,
            "text": "faltou o teste", "criteria": [{"criterion": "MSTD-CRIT-0001", "tests_rule": true}],
            "agreed": [{"item": "MSTD-DEC-0001", "met": true}]}));
        assert_eq!(rejected_with_agreed["ok"], json!(true), "{rejected_with_agreed}");
        let fix = text(&round(root, "x", None), "dispatch", 1);
        let heading =
            format!("## {}\n\n{}", translate("prompt.part.items", Locale::PtBr), translate("prompt.fix.wave", Locale::PtBr));
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

        let text = |out: &Value, field: &str, wave: u64| -> String {
            let found = out[field].as_array().into_iter().flatten().find(|d| d["wave"] == json!(wave));
            found.and_then(|d| d["prompt"].as_str()).unwrap_or_default().to_string()
        };
        let fix = text(&round(root, "x", Some(&verdict(root, 1, "rejected", findings))), "dispatch", 1);
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

    /// A última onda em andamento entrega e sobra tarefa no backlog: a rodada
    /// nunca manda fechar. A tarefa que a entrega desta mesma rodada soltou
    /// ainda não virou lote, e a resposta manda rodar de novo, com a linha
    /// pronta; a rodada seguinte despacha o lote, e só com o backlog vazio a
    /// rodada manda fechar. Com tarefa presa — nenhuma pronta e nada em
    /// andamento — a resposta nomeia as tarefas presas e não manda fechar.
    #[test]
    fn a_rodada_nao_manda_fechar_com_tarefa_no_backlog() {
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
        let copy = mustard_core::io::wave_prompt::copy_path(root, "x", 99, false);
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
