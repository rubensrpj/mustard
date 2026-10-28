//! `mustard-rt run close [--spec <nome>]` — o fechamento de uma spec.
//!
//! É a porta única do fechamento: grava o que voltou da última rodada,
//! confere se a obra terminou mesmo, roda os comandos do servidor — a
//! compilação, o lint e a suíte inteira, o `buildCommand`, o `lintCommand` e
//! o `testCommand` do `mustard.json` — em ambiente limpo, roda a prova de cada
//! critério, e a de cada pedido que uma tarefa entregue cobre, uma vez no
//! commit da obra e outra no código de antes dela, grava a execução de cada
//! uma, e então fecha — grava a fase `closed`, que arma a
//! cobrança das pendências pela mesma porta, solta a spec da sessão e atualiza
//! o estado local observado pelo painel. A pasta da spec mantém os eventos
//! e dados locais de validação. Publicação externa é explícita.
//!
//! **O agente de teste dedicado.** Nenhuma spec fecha sem ele — nem a de uma
//! onda só —, e a rodada não pede a revisão de onda nenhuma: com a máquina
//! verde, o fechamento devolve o pedido dele, com as entregas da obra, os
//! critérios, as mudanças da branch, as emendas gravadas entre as ondas e o
//! que cada onda deixou aberto, e só fecha — e só então devolve o pull
//! request — quando o veredito que ele grava na spec, com `run write
//! verdict` enquanto o pedido de revisão está aberto, volta aprovado: o
//! fechamento o assume, sem relatório, e grava o veredito oficial no lugar
//! da volta. Sem esse veredito, o fechamento não abre outro pedido de
//! revisão: manda o revisor gravar o dele. A exceção é o pedido que ficou
//! para trás de um item gravado depois dele, que o pedido novo lista: aí o
//! fechamento grava o pedido novo, e a aprovação do antigo não fecha a obra.
//! Enquanto nada muda depois da máquina verde, a volta não roda o lint nem os
//! critérios de novo. A reprovação fica na onda que ele apontou, que volta
//! como conserto pela rodada; entregue o conserto, o fechamento pede o mesmo
//! agente de novo, mas só para conferir o conserto, não a obra inteira. Em
//! até duas voltas de conserto sem aprovar, a onda para e a decisão passa a
//! ser do usuário — o mesmo limite de qualquer conserto. Os achados que o
//! veredito aprovado traz em `leftovers` viram tarefas no backlog, e a
//! resposta manda rodar a rodada no lugar do pull request.
//!
//! **A cópia do revisor.** É o binário que cria a cópia onde o agente de
//! teste dedicado confere a obra, pela mesma porta das cópias de onda, antes
//! de a máquina rodar, e que a apaga quando a obra fecha. Uma cópia que
//! chegou com mudança — o que um revisor anterior deixou nela e não desfez —
//! trava o fechamento, com os arquivos: ler código mexido como se fosse o da
//! obra é pior do que parar. Quem prova que cada prova caía antes da obra é a
//! máquina, e não o revisor.
//!
//! **A pasta de compilação.** No fim, a obra fechada apaga da pasta principal
//! as pastas descartáveis declaradas em `buildOutput` que ultrapassam 15 GB;
//! as menores são preservadas para reutilizar o cache. A resposta informa o resultado.
//!
//! **O que trava.** Onda sem commit; onda cuja última revisão reprovou e
//! ainda não recebeu o conserto; pedido do usuário que nenhuma onda entregou;
//! a cópia do revisor com mudança, ou que não pôde ser criada; a
//! compilação, o lint ou a suíte que falham, com a saída e, quando o projeto
//! declara arquivos locais, o nome de cada um, que a cópia limpa não tem; critério cuja prova
//! não passou;
//! critério cuja prova saiu verde sem rodar teste nenhum — a saída do
//! executor diz zero teste, que é o que um nome de teste errado dá, e a
//! recusa traz o comando e o número que ela leu; e critério cuja prova já
//! passava, rodando teste, no código de antes da obra — o commit base, com os
//! arquivos de teste da obra por cima —, que não prova nada novo. A prova do
//! pedido que uma tarefa entregue cobre, como o nascido de um achado, trava
//! pelos mesmos motivos. Cada recusa
//! diz qual onda refazer — não basta os testes passarem.
//!
//! **As pendências da obra.** A resposta traz cada pendência aberta nascida
//! na obra, com a pergunta de destino e a linha que grava a resposta do
//! usuário. `--pending-later "P-<n>=<o motivo>"` é o "fica para depois": a
//! pendência solta da obra e passa a ser do projeto, com o motivo. A resposta
//! vale na chamada que fecha e nas de depois — é lendo a pergunta que o
//! usuário responde, e obra fechada não pergunta de novo. É aviso, nunca
//! recusa: sem resposta nenhuma, a obra fecha do mesmo jeito.
//!
//! O fechamento não chama a função antiga de fechar, que grava arquivos do
//! formato velho: ela ficou onde estava, e a fase `closed` passa a sair só por
//! aqui.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::{Block, BlockQuery, Refusal, SpecEvent, SpecLog};
use mustard_core::domain::spec_index::title_of;
use mustard_core::domain::spec_state::{PhaseWriter, SpecState, State, final_approval, review_send};
use mustard_core::domain::wave_prompt::{count_lines, delivered_requests, recorded_choice, unowned};
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Map, Value, json};

use crate::commands::flow::round::Caller;
use crate::commands::review::qa_run::ProofFault;
use crate::commands::spec_events::{self, write::record};
use crate::shared::build_removal::{remove_build_output, removal_warnings};
use crate::shared::spec_state::{DiskSpecState, checkout, session_from_env};

mod clean_copy;
#[cfg(test)]
mod outdated_request_tests;
mod ownerless_tests;
mod proof_runs;
#[cfg(test)]
mod review_request_tests;
#[cfg(test)]
mod session_answer_tests;
#[cfg(test)]
mod stale_marks;
mod unused;

/// As opções de `mustard-rt run close`.
#[derive(Default)]
pub struct CloseOpts {
    /// Qualquer pasta dentro do repositório.
    pub root: PathBuf,
    /// A spec que fecha; sem ela, a spec atual.
    pub spec: Option<String>,
    /// O relatório da última rodada, em JSON, no mesmo formato da rodada.
    pub report: Option<String>,
    /// A resposta "fica para depois" do usuário a uma pendência desta obra,
    /// uma por item, `P-83=o motivo`. Cada uma solta a pendência da obra e a
    /// passa ao projeto; o motivo, depois do `=`, é opcional.
    pub pending_later: Vec<String>,
}

/// Por que a spec não fechou.
enum CloseRefusal {
    /// Uma recusa do arquivo de eventos.
    Refused(Refusal),
    /// O relatório da última rodada foi recusado pela mesma porta da rodada.
    Report(crate::commands::flow::round::RoundRefusal),
    /// A spec não está em execução.
    NotRunning {
        phase: String,
    },
    /// Uma onda que não tem commit nenhum.
    WaveWithoutCommit {
        wave: u64,
    },
    /// Uma onda cuja última revisão reprovou.
    WaveRejected {
        wave: u64,
    },
    /// Um pedido do usuário que nenhuma onda entregou.
    RequestNotDelivered {
        code: String,
    },
    /// O backlog ainda tem tarefa, com os códigos delas.
    BacklogNotEmpty {
        tasks: String,
    },
    /// A compilação declarada do projeto falhou. `local_files` são os
    /// arquivos locais que o `mustard.json` declara e que a cópia limpa não
    /// tem. `no_wave` diz por que nenhuma onda reabriu, quando a saída não
    /// cita arquivo que uma onda da obra mudou.
    BuildFailed {
        command: String,
        output: String,
        local_files: Vec<String>,
        no_wave: Option<String>,
    },
    /// O lint do projeto falhou, com os mesmos `local_files` e `no_wave` da
    /// compilação.
    LintFailed {
        command: String,
        output: String,
        local_files: Vec<String>,
        no_wave: Option<String>,
    },
    /// A suíte inteira do projeto falhou, com os mesmos `local_files` e
    /// `no_wave` do lint.
    SuiteFailed {
        command: String,
        output: String,
        local_files: Vec<String>,
        no_wave: Option<String>,
    },
    /// A cópia do revisor final chegou com mudança, com os arquivos dela.
    ReviewCopyDirty {
        copy: String,
        files: String,
    },
    /// A cópia do revisor final não pôde ser criada, com o que o git disse.
    ReviewCopyFailed {
        copy: String,
        detail: String,
    },
    /// A cópia limpa em que a máquina roda não pôde ser criada, com o que o
    /// git disse.
    CleanCopyFailed {
        copy: String,
        detail: String,
    },
    /// Um critério, ou um pedido (`of`), cuja prova não passou.
    CriterionFailed {
        of: ProofOf,
        code: String,
        output: String,
    },
    /// A análise de declarações sem uso recusou, com o texto dos achados que
    /// recusam e o mesmo `no_wave` do lint.
    NewUnused {
        text: String,
        no_wave: Option<String>,
    },
    ValidationInputsChanged,
    /// Um critério, ou um pedido (`of`), cuja prova saiu verde sem rodar
    /// teste nenhum, com o comando dela e o número de testes que a saída
    /// dele disse.
    CriterionRanNoTest {
        of: ProofOf,
        code: String,
        command: String,
        tests: u64,
    },
    /// Um critério, ou um pedido (`of`), cuja prova saiu verde citando um
    /// nome de teste que não existe no projeto, com o nome que faltou.
    CriterionMissingTest {
        of: ProofOf,
        code: String,
        name: String,
    },
    /// Um critério, ou um pedido (`of`), cuja prova já passava no código de
    /// antes da obra, rodando teste, com o comando dela e o número de testes
    /// que a saída disse.
    CriterionPassedBefore {
        of: ProofOf,
        code: String,
        command: String,
        tests: u64,
    },
    /// Mais de uma recusa achada no mesmo passo, na ordem em que apareceram:
    /// a resposta lista todas de uma vez, e o motivo é o da primeira.
    Many(Vec<CloseRefusal>),
}

/// De quem é a prova que a máquina do fechamento roda: de um critério, ou de
/// um pedido que uma tarefa entregue cobre, como o nascido de um achado. A
/// recusa fala do item certo, e a falha reabre as ondas que o cobrem.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ProofOf {
    Criterion,
    Request,
}

impl ProofOf {
    /// De quem é a prova do item de número `id` da obra `log`.
    fn item(log: &SpecLog, id: u64) -> Self {
        if log.get(id).is_some_and(|item| item.event_type == "request") { Self::Request } else { Self::Criterion }
    }

    /// A chave do texto da recusa: `criterion`, a do critério, ou `request`,
    /// a do pedido.
    fn key(self, criterion: &'static str, request: &'static str) -> &'static str {
        match self {
            Self::Criterion => criterion,
            Self::Request => request,
        }
    }
}

impl CloseRefusal {
    /// As recusas `found` numa só: nenhuma é `None`, uma é ela mesma, e mais
    /// de uma vira [`CloseRefusal::Many`].
    fn all(mut found: Vec<CloseRefusal>) -> Option<CloseRefusal> {
        match found.len() {
            0 => None,
            1 => found.pop(),
            _ => Some(Self::Many(found)),
        }
    }

    fn reason(&self) -> String {
        match self {
            Self::Many(all) => all.first().map(Self::reason).unwrap_or_default(),
            Self::Refused(refusal) => refusal.reason().to_string(),
            Self::Report(refusal) => refusal.reason(),
            Self::NotRunning { .. } => "close-not-running".into(),
            Self::WaveWithoutCommit { .. } => "wave-without-commit".into(),
            Self::WaveRejected { .. } => "wave-rejected".into(),
            Self::RequestNotDelivered { .. } => "request-not-delivered".into(),
            Self::BacklogNotEmpty { .. } => "backlog-not-empty".into(),
            Self::BuildFailed { .. } => "build-failed".into(),
            Self::LintFailed { .. } => "lint-failed".into(),
            Self::SuiteFailed { .. } => "suite-failed".into(),
            Self::ReviewCopyDirty { .. } => "review-copy-dirty".into(),
            Self::ReviewCopyFailed { .. } => "review-copy-failed".into(),
            Self::CleanCopyFailed { .. } => "clean-copy-failed".into(),
            Self::CriterionFailed { .. } => "criterion-failed".into(),
            Self::NewUnused { .. } => "final-new-unused".into(),
            Self::ValidationInputsChanged => "validation-inputs-changed".into(),
            Self::CriterionRanNoTest { .. } => "criterion-ran-no-test".into(),
            Self::CriterionMissingTest { .. } => "criterion-missing-test".into(),
            Self::CriterionPassedBefore { .. } => "criterion-passed-before".into(),
        }
    }

    fn message(&self, lang: Locale) -> String {
        let fill = |key: &str, slots: &[(&str, String)]| {
            slots.iter().fold(translate(key, lang).to_string(), |text, (slot, value)| text.replace(slot, value))
        };
        // A compilação, o lint ou a suíte que caiu: a recusa da chave `key`,
        // os arquivos locais que a cópia limpa não tem e por que nenhuma onda
        // reabriu, cada um numa linha.
        let command_failed = |key: &str, command: &str, output: &str, local_files: &[String], no_wave: Option<&str>| {
            let refusal = fill(key, &[("{command}", command.to_string()), ("{output}", output.to_string())]);
            with_no_wave(without_local_files(refusal, local_files, lang), no_wave)
        };
        match self {
            Self::Many(all) => all.iter().map(|refusal| refusal.message(lang)).collect::<Vec<_>>().join("\n\n"),
            Self::Refused(refusal) => refusal.message(lang),
            Self::NewUnused { text, no_wave } => with_no_wave(text.clone(), no_wave.as_deref()),
            Self::ValidationInputsChanged => translate("close.validation_inputs_changed", lang).into(),
            Self::Report(refusal) => refusal.message(lang),
            Self::NotRunning { phase } => fill("close.not_running", &[("{phase}", phase.clone())]),
            Self::WaveWithoutCommit { wave } => fill("close.wave_without_commit", &[("{wave}", wave.to_string())]),
            Self::WaveRejected { wave } => fill("close.wave_rejected", &[("{wave}", wave.to_string())]),
            Self::RequestNotDelivered { code } => fill("close.request_not_delivered", &[("{code}", code.clone())]),
            Self::BacklogNotEmpty { tasks } => fill("close.backlog_not_empty", &[("{tasks}", tasks.clone())]),
            Self::BuildFailed { command, output, local_files, no_wave } => command_failed("close.build_failed", command, output, local_files, no_wave.as_deref()),
            Self::LintFailed { command, output, local_files, no_wave } => command_failed("close.lint_failed", command, output, local_files, no_wave.as_deref()),
            Self::SuiteFailed { command, output, local_files, no_wave } => command_failed("close.suite_failed", command, output, local_files, no_wave.as_deref()),
            Self::ReviewCopyDirty { copy, files } => fill("close.review_copy_dirty", &[("{copy}", copy.clone()), ("{files}", files.clone())]),
            Self::ReviewCopyFailed { copy, detail } => fill("close.review_copy_failed", &[("{copy}", copy.clone()), ("{detail}", detail.clone())]),
            Self::CleanCopyFailed { copy, detail } => fill("close.clean_copy_failed", &[("{copy}", copy.clone()), ("{detail}", detail.clone())]),
            Self::CriterionFailed { of, code, output } => {
                fill(of.key("close.criterion_failed", "close.request_proof_failed"), &[("{code}", code.clone()), ("{output}", output.clone())])
            }
            Self::CriterionRanNoTest { of, code, command, tests } => fill(
                of.key("close.criterion_ran_no_test", "close.request_proof_ran_no_test"),
                &[("{code}", code.clone()), ("{command}", command.clone()), ("{count}", tests.to_string())],
            ),
            Self::CriterionMissingTest { of, code, name } => {
                fill(of.key("close.criterion_missing_test", "close.request_proof_missing_test"), &[("{code}", code.clone()), ("{name}", name.clone())])
            }
            Self::CriterionPassedBefore { of, code, command, tests } => fill(
                of.key("close.criterion_passed_before", "close.request_proof_passed_before"),
                &[("{code}", code.clone()), ("{command}", command.clone()), ("{count}", tests.to_string())],
            ),
        }
    }

    fn to_value(&self, lang: Locale) -> Value {
        if let Self::Report(refusal) = self {
            return refusal.to_value(lang);
        }
        // A lista leva os campos da primeira recusa — a pergunta de uma onda
        // parada, por exemplo —, com o texto de todas e o motivo de cada uma.
        if let Self::Many(all) = self {
            let mut out = all.first().map_or_else(|| json!({}), |first| first.to_value(lang));
            out["ok"] = json!(false);
            out["reason"] = json!(self.reason());
            out["hint"] = json!(self.message(lang));
            out["reasons"] = json!(all.iter().map(Self::reason).collect::<Vec<_>>());
            return out;
        }
        json!({ "ok": false, "reason": self.reason(), "hint": self.message(lang) })
    }
}

/// A recusa `refusal` da compilação, do lint ou da suíte, seguida, numa linha própria, de
/// por que nenhuma onda reabriu (`no_wave`), quando é o caso.
fn with_no_wave(refusal: String, no_wave: Option<&str>) -> String {
    match no_wave {
        Some(why) => format!("{refusal}\n{why}"),
        None => refusal,
    }
}

/// A recusa `refusal` da compilação, do lint ou da suíte, seguida, numa linha
/// própria, de que o comando rodou na cópia limpa sem os arquivos locais que o
/// projeto declara (`local_files`), cada um nomeado. Quem só tem o `.env` na
/// máquina entende por que o comando caiu. Sem arquivo local declarado, a
/// recusa fica como está.
fn without_local_files(refusal: String, local_files: &[String], lang: Locale) -> String {
    if local_files.is_empty() {
        return refusal;
    }
    let files = local_files.iter().map(|file| format!("`{file}`")).collect::<Vec<_>>().join(", ");
    format!("{refusal}\n{}", translate("close.ran_without_local_files", lang).replace("{files}", &files))
}

/// O núcleo testável de [`run_cmd`]. A sessão e a pasta de configuração da
/// plataforma vêm do ambiente. Nunca entra em pânico.
pub(crate) fn close_at(opts: &CloseOpts) -> Value {
    let session = session_from_env();
    let config_dir = mustard_core::claude_config_dir();
    close_in(opts, Caller { session: session.as_deref(), config_dir: config_dir.as_deref() })
}

/// [`close_at`] com a sessão recebida, que é como um teste a escolhe, sem a
/// pasta de configuração da plataforma: o consumo da última onda não é
/// medido.
#[cfg(test)]
pub(crate) fn close_for(opts: &CloseOpts, session: Option<&str>) -> Value {
    close_in(opts, Caller { session, config_dir: None })
}

/// [`close_at`] com a sessão e a pasta de configuração da plataforma
/// recebidas (`caller`), de onde a última onda assumida tem o consumo medido.
fn close_in(opts: &CloseOpts, caller: Caller<'_>) -> Value {
    let project = spec_events::project(&opts.root);
    let lang = project.lang;
    // O código que a cópia do revisor guardou antes de zerar vai na resposta
    // também quando o fechamento recusa depois disso: a ref já existe, e a
    // próxima chamada acha a cópia limpa e não teria mais o que dizer.
    let mut review_kept = Vec::new();
    // A pergunta ao Jev de cada volta que espera a decisão sai depois de
    // soltas a trava do passo do git e a das cópias, que o fechamento segura
    // até o fim; a que ele não respondeu vai em `reserves`, e a volta segura
    // o fechamento até o recibo.
    let mut asking = (String::new(), Vec::new());
    let mut value = match run_close(opts, &project.root, lang, caller, (&mut review_kept, &mut asking)) {
        Ok(report) => report,
        Err(refusal) => {
            let mut value = refusal.to_value(lang);
            // A recusa fica gravada na spec, e não só na resposta desta
            // chamada: é dela que a retomada sabe que o último fechamento
            // caiu, e por quê, em vez de mandar rodar outra rodada.
            record_refused_close(&opts.root, opts.spec.as_deref(), &value);
            for kept in &review_kept {
                let hint = crate::commands::flow::round::code_kept_hint(kept, lang);
                spec_events::pages::push_warning(&mut value, "code-kept", &hint);
            }
            value
        }
    };
    if value["ok"] == true {
        let fixed = [translate("close.final_review", lang).replace("{spec}", value["spec"].as_str().unwrap_or_default())];
        crate::commands::flow::round::once_per_session(&project.root, caller.session, &mut value, &fixed);
    }
    let (spec, waiting) = asking;
    let reserves = crate::commands::flow::round::ask_after_locks(&opts.root, &project.root, &spec, &waiting, lang);
    crate::shared::jev::reserve::attach(&mut value, reserves, "mustard-rt run close", lang);
    value
}

/// Grava, na spec, que este fechamento caiu, e com que motivo. Melhor
/// esforço: a gravação que falha não muda a resposta, e a retomada volta a
/// não saber do fechamento, como antes desta marca.
///
/// Sem spec nomeada não há onde gravar: a recusa de "nenhuma spec atual" é
/// justamente a de quem não tem spec.
fn record_refused_close(start: &Path, spec: Option<&str>, value: &Value) {
    let Some(spec) = spec.map(str::trim).filter(|spec| !spec.is_empty()) else {
        return;
    };
    let Some(reason) = value.get("reason").and_then(Value::as_str) else {
        return;
    };
    let draft = json!({"phase": "final", "stage": super::common::REFUSED_STAGE, "result": "fail", "ms": 0,
        "output": reason, "author": "binary"});
    let _ = record(start, spec, "stage_run", draft.as_object().cloned().unwrap_or_default(), PhaseWriter::Binary);
}

fn run_close(
    opts: &CloseOpts,
    root: &Path,
    lang: Locale,
    caller: Caller<'_>,
    (review_kept, asking): (&mut Vec<crate::commands::flow::round::Kept>, &mut (String, Vec<crate::commands::flow::round::Waiting>)),
) -> Result<Value, CloseRefusal> {
    let session = caller.session;
    let spec = match opts.spec.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(spec) => spec.to_string(),
        None => DiskSpecState::new(&checkout(&opts.root)).active(session).ok_or(CloseRefusal::Refused(Refusal::NoCurrentSpec))?,
    };
    mustard_core::io::legacy_specs::refuse_legacy(root, &spec).map_err(CloseRefusal::Refused)?;
    let path = store::spec_file(root, &spec).map_err(CloseRefusal::Refused)?;
    let read = |path: &Path| -> Result<SpecLog, CloseRefusal> {
        store::read(path).map_err(CloseRefusal::Refused)?.ok_or_else(|| CloseRefusal::Refused(Refusal::NoSpecFile { spec: spec.clone() }))
    };
    let log = read(&path)?;

    // A resposta que o usuário deu a cada pendência desta obra é gravada
    // antes de qualquer conferência: ela vale tanto na chamada que fecha
    // quanto na volta, com a obra já fechada — é depois de ler as perguntas
    // do fechamento que ele responde, e obra fechada não pergunta de novo.
    let left_for_later = hand_pending_to_project(root, &spec, &log, &opts.pending_later);

    let phase = State::from_log(&log).phase.unwrap_or_default().to_string();
    if phase != "running" {
        if !left_for_later.is_empty() {
            return Ok(json!({ "ok": true, "spec": spec, "phase": phase, "pending_released": left_for_later }));
        }
        return Err(CloseRefusal::NotRunning { phase });
    }

    // Os agentes do projeto voltam ao texto deste programa no começo do
    // fechamento, bem antes do envio da revisão: o Claude Code leva alguns
    // segundos para reler o arquivo, e o revisor aberto logo depois da troca
    // ainda rodaria o texto antigo.
    let agents_hint = crate::commands::flow::round::agents_refreshed(root, lang);

    // O fechamento pega a trava das cópias uma vez, aqui, e a segura até
    // tirar as cópias da obra: uma rodada no meio do despacho termina de
    // gravar o envio da vaga que preparou antes, e nenhuma outra prepara
    // cópia enquanto ele confere a obra.
    let copies = crate::commands::git_settle::copies_lock(root)
        .map_err(|detail| CloseRefusal::Report(crate::commands::flow::round::RoundRefusal::Git { detail }))?;

    // Nada fica preso: todo processo que um agente deixou rodando — um laço
    // de espera, ou um comando na cópia de uma onda já apagada — é encerrado
    // no fechamento, e a resposta diz qual.
    let stuck_hint = crate::commands::flow::stuck::report_line(&crate::commands::flow::stuck::find_stuck_processes(root, &copies), lang);

    // O que voltou da última rodada entra antes das conferências, pela mesma
    // porta da rodada, com o commit: a volta que a última onda gravou na spec
    // é assumida aqui, com ou sem relatório, e é o commit dela que fecha a
    // última onda.
    let raw = opts.report.as_deref().map(str::trim).filter(|r| !r.is_empty());
    let mut taken = crate::commands::flow::round::take_report(&opts.root, root, &spec, raw, &log, lang, caller).map_err(CloseRefusal::Report)?;
    *asking = (spec.clone(), std::mem::take(&mut taken.asking));
    // O commit da última volta pede a passada do mapa em segundo plano; o
    // fechamento, cuja análise final lê o mapa, espera por ela.
    if taken.commit.is_some() {
        crate::commands::flow::round::refresh_map_now(root);
    }
    // A volta que ficou fora do commit — a que pede novo plano sem o clique
    // do usuário, ou a recusada por uma conferência dela — segura o
    // fechamento, que depende de todas: as outras voltas já foram assumidas
    // acima, e a recusa traz a pergunta que a decide ou o que falta à volta.
    // Cada uma entra na lista do que falta, com o resto, numa resposta só.
    let held: BTreeSet<u64> = taken.waiting.iter().map(|one| one.wave).collect();
    let mut missing: Vec<CloseRefusal> = taken.waiting.into_iter().map(|one| CloseRefusal::Report(one.refusal)).collect();
    let recorded: Vec<Value> = taken.recorded;

    let log = read(&path)?;
    super::validation::review_elapsed(root, &spec, &log).map_err(CloseRefusal::Refused)?;
    // A reprovação que o lint, a suíte ou a análise de declarações sem uso
    // gravaram sem citar arquivo da onda sai antes de a obra ser conferida: a
    // onda solta não trava mais o fechamento, que segue para a máquina sem
    // pedir conserto.
    let pending = crate::commands::flow::round::waves_pending_fix(&log);
    let log = if super::common::stale_marks::release(root, &spec, &log, pending, lang).map_err(CloseRefusal::Refused)?.is_empty() { log } else { read(&path)? };
    // A onda segurada acima não é cobrada de novo por falta de commit.
    missing.extend(unfinished(&log).into_iter().filter(|refusal| !matches!(refusal, CloseRefusal::WaveWithoutCommit { wave } if held.contains(wave))));
    // O pedido de revisão só fecha com o veredito que o revisor grava e que a
    // leitura acima assume: sem ele, o fechamento não abre outro pedido, e a
    // resposta manda o revisor gravar o veredito. O pedido aberto mais velho
    // que a última mudança da obra não espera: o revisor não recebeu o que
    // mudou depois dele, o veredito dele não aprovaria a obra, e o fechamento
    // abre um pedido novo, mais abaixo. O mesmo vale para o pedido que ficou
    // para trás de um item gravado depois dele: o pedido novo o lista, e a
    // aprovação do antigo não fecha a obra.
    let awaited = crate::commands::flow::round::awaited_review(&log);
    let outdated = super::common::outdated_request::find(&log, awaited);
    if awaited.is_some()
        && !held.contains(&crate::commands::flow::round::FINAL_REVIEW)
        && outdated.is_none()
    {
        missing.push(CloseRefusal::Report(crate::commands::flow::round::RoundRefusal::VerdictMissing));
    }
    if let Some(refusal) = CloseRefusal::all(missing) {
        return Err(refusal);
    }

    // A cópia do revisor sai antes da máquina, pela mesma porta das cópias de
    // onda: a que chegou com mudança trava aqui, antes de gastar a suíte, e
    // a limpa vai para o commit da obra. Com a obra já aprovada, ninguém mais
    // revisa, e a cópia só espera ser apagada lá embaixo. Os arquivos locais
    // que não chegaram a ela viram aviso no pedido do revisor. A vaga é
    // escolhida uma vez só: a mesma que é preparada vai gravada no envio da
    // revisão, e fica presa por ele até o veredito.
    let review_copy = mustard_core::io::wave_prompt::final_copy_path(root, &spec, &log);
    let not_copied = if final_approved(&log) && outdated.is_none() {
        Vec::new()
    } else {
        let (not_copied, kept) = prepare_review_copy(root, &spec, &review_copy, &log, &copies)?;
        *review_kept = kept;
        not_copied
    };

    // A máquina antes do agente de teste dedicado: os comandos do servidor e
    // cada critério, uma vez por fechamento. A volta que só
    // confere a aprovação, sem nada mudado desde a máquina verde, não roda
    // nada de novo.
    let (Machine { runs, stages, undeclared, from_base, before_unchecked }, reused) = {
        let _held = crate::commands::git_settle::git_step_lock(root)
            .map_err(|detail| CloseRefusal::Report(crate::commands::flow::round::RoundRefusal::Git { detail }))?;
        if super::validation::reusable(root, &log) {
            (Machine::default(), true)
        } else {
            let clean = clean_copy::prepare(root, &spec, &log, &copies)?;
            let before = proof_runs::prepare(root, &spec, &log, &copies)?;
            match machine(opts, root, (&clean, before.as_ref()), &spec, &log, lang) {
                Ok(ran) => (ran, false),
                Err(failure) => return Err(authorize_final_repair(root, &spec, &log, failure, lang)),
            }
        }
    };
    // O caminho de volta roda depois que a máquina passa, e nunca trava: os
    // testes sem dono viram aviso na resposta, e não recusa.
    let unowned_tests = ownerless_tests::at_close(root, &spec, &log, lang);
    let unused_hints = crate::commands::flow::round::final_unused(root, &spec, &log, lang, &[])
        .into_iter()
        .filter(|(refuses, _)| !refuses)
        .map(|(_, hint)| hint)
        .collect::<Vec<_>>();

    let log = read(&path)?;
    if !final_approved(&log) || outdated.is_some() {
        let review = mustard_core::io::wave_prompt::final_review(root, &spec, &log, lang);
        let prompt = review.text;
        // O pedido do agente de revisão final é gravado como evento de
        // envio antes de sair daqui, pela mesma porta que grava o pedido de
        // cada onda: o texto inteiro, o papel de revisão, o modelo, o esforço
        // e a vaga em que o revisor trabalha, no mesmo formato do envio de
        // onda — nunca um segundo caminho de gravação. O molde do revisor não
        // vai junto: ele mora no projeto, e o papel já diz qual é.
        let copy = mustard_core::io::wave_prompt::shown(&review_copy);
        let mut draft = Map::new();
        draft.insert("role".into(), json!("review"));
        draft.insert("copy".into(), json!(copy));
        draft.insert("lines".into(), json!(count_lines(&prompt)));
        draft.insert("chars".into(), json!(prompt.chars().count()));
        draft.insert("text".into(), json!(prompt));
        // Os itens que o pedido lista, para o veredito conferir que o revisor
        // leu cada um; o envio sem essa lista é de antes da conferência.
        draft.insert("read_items".into(), json!(review.listed));
        // O modelo com que o revisor é lançado, o de `agents.review`, e o
        // esforço que todo agente leva.
        let agents = mustard_core::ProjectConfig::load(root);
        draft.insert("model".into(), json!(agents.review_settings().model));
        draft.insert("effort".into(), json!(mustard_core::domain::config::DEFAULT_AGENT_EFFORT));
        draft.insert("mustard".into(), json!(env!("CARGO_PKG_VERSION")));
        draft.insert("author".into(), json!("binary"));
        record(&opts.root, &spec, "send", draft, PhaseWriter::Binary).map_err(CloseRefusal::Refused)?;
        let next = translate("close.final_review", lang).replace("{spec}", &spec);
        // A resposta não leva o pedido inteiro: leva o comando que o lê, e o
        // revisor lê o próprio pedido por ele, de dentro da cópia dele.
        let read = crate::commands::flow::round::read_command(root, &spec, "request-review");
        let mut out = json!({
            "ok": true,
            "spec": spec,
            "phase": "running",
            "recorded": recorded,
            "criteria": runs,
            "stages": stages,
            "validation": {"status":"pass","reused":reused},
            "review": { "final": true, "lines": count_lines(&prompt), "read": read },
            "next": next,
        });
        if let Some(hint) = &stuck_hint {
            spec_events::pages::push_warning(&mut out, "stuck-found", hint);
        }
        if let Some(outdated) = &outdated {
            spec_events::pages::push_warning(&mut out, "review-request-renewed", &outdated.hint(lang));
        }
        for hint in &undeclared {
            spec_events::pages::push_warning(&mut out, "server-command-not-declared", hint);
        }
        for hint in &from_base {
            spec_events::pages::push_warning(&mut out, "lint-from-the-base", hint);
        }
        if let Some(hint) = &before_unchecked {
            spec_events::pages::push_warning(&mut out, "proof-before-unchecked", hint);
        }
        for hint in &unused_hints {
            spec_events::pages::push_warning(&mut out, "unused-analysis", hint);
        }
        for hint in &unowned_tests {
            spec_events::pages::push_warning(&mut out, super::common::ownerless::REASON, hint);
        }
        for file in &not_copied {
            let hint = crate::commands::flow::round::local_file_missing(file, &copy, lang);
            spec_events::pages::push_warning(&mut out, "local-file-missing", &hint);
        }
        for kept in review_kept.iter() {
            let hint = crate::commands::flow::round::code_kept_hint(kept, lang);
            spec_events::pages::push_warning(&mut out, "code-kept", &hint);
        }
        if let Some((reason, hint)) = &agents_hint {
            spec_events::pages::push_warning(&mut out, reason, hint);
        }
        let withheld = spec_events::pages::secrets::withheld(&log);
        spec_events::pages::note_withheld(&mut out, &spec, &withheld, lang);
        return Ok(out);
    }

    // A aceitação do veredito final grava a tabela de rastreabilidade: uma
    // linha por item do combinado, com a verificação e o arquivo que o
    // próprio veredito já trouxe por item, e a situação. Sem essa tabela,
    // ninguém consultava depois qual item tem qual verificação, nem onde o
    // comportamento mora — o resultado morria no veredito.
    // Antes dela, os achados que o veredito aprovado traz em `leftovers` viram
    // tarefas no backlog, e a obra não fecha: a resposta manda rodar a rodada
    // no lugar do pull request. O fechamento seguinte recusa pelo backlog, e
    // a entrega das tarefas pede revisão final nova. O achado sem regra
    // escrita na spec não vira tarefa: vai ao usuário como pergunta, nesta
    // resposta, e não segura a obra.
    let mut questions = Vec::new();
    if let Some(verdict) = final_approval(&log) {
        let found = crate::commands::flow::round::record_final_leftovers(&opts.root, root, &spec, (&log, verdict), lang)
            .map_err(CloseRefusal::Refused)?;
        // O achado só em arquivo que o git ignora vira nota ao usuário, sem
        // tarefa: sem tarefa nenhuma, a obra fecha como sempre.
        if found.recorded.iter().any(|entry| entry["type"] == json!("task")) {
            let mut out = final_leftovers_answer(&spec, recorded, found, &read(&path)?, lang);
            out["criteria"] = json!(runs);
            out["stages"] = json!(stages);
            out["validation"] = json!({"status":"pass","reused":reused});
            if let Some(hint) = &stuck_hint {
                spec_events::pages::push_warning(&mut out, "stuck-found", hint);
            }
            if let Some((reason, hint)) = &agents_hint {
                spec_events::pages::push_warning(&mut out, reason, hint);
            }
            return Ok(out);
        }
        record_tracking_table(&opts.root, &spec, verdict).map_err(CloseRefusal::Refused)?;
        questions = found.questions;
    }

    // A fase `closed` sai só por aqui, e é a mesma porta que arma a cobrança
    // das pendências. A função antiga de fechar, que grava arquivos do formato
    // velho, não é chamada.
    crate::commands::spec_events::write::record_phase(&opts.root, &spec, "closed", session);
    crate::commands::spec_events::session_spec::unbind(&opts.root, session);
    // A obra fechou: ninguém mais trabalha nas cópias dela, e todas saem
    // daqui, cada vaga com o que tiver dentro e com a compilação — o que um
    // revisor deixou na cópia dele não é trabalho de ninguém. A que não sai
    // vira aviso, e a obra fecha do mesmo jeito.
    let mut removal = crate::commands::flow::round::remove_spec_copies_under(root, &spec, Some(&log), &copies);
    // Apagar o que as cópias têm no disco vem depois de soltar a trava delas.
    drop(copies);
    removal.delete_leaving(root);
    // Nenhuma onda volta mais para conserto: os trechos que a conferência
    // depois da onda deixou para o agente saem da pasta da spec.
    crate::commands::flow::round::sweep_fixes(root, &spec, None);
    // O retrato do mapa da base não serve mais a ninguém: a obra fechou, e a
    // análise de declarações sem uso já rodou contra ele. São dezenas de
    // megabytes por spec, e ninguém os apagava.
    let _ = std::fs::remove_file(mustard_core::io::wave_prompt::validation_base_map(root, &spec));
    // Por último, a pasta de compilação que o projeto declarou descartável
    // sai da pasta principal: ela só cresce, e a próxima obra a refaz uma vez.
    let build_output = remove_build_output(root, lang);
    // Closing updates local state only; external pages require an explicit request.

    // O pull request é o passo seguinte, e a linha dele sai pronta, com a base
    // e a branch tiradas do estado — pela mesma tabela que a retomada usa.
    let command = crate::commands::flow::resume::next_command("closed", &spec, &State::from_log(&read(&path)?));

    let mut out = json!({
        "ok": true,
        "spec": spec,
        "phase": "closed",
        "recorded": recorded,
        "criteria": runs,
        "stages": stages,
        "validation": {"status":"pass","reused":reused},
    });
    if let Some(hint) = &stuck_hint {
        spec_events::pages::push_warning(&mut out, "stuck-found", hint);
    }
    if let Some((reason, hint)) = &agents_hint {
        spec_events::pages::push_warning(&mut out, reason, hint);
    }
    for (reason, hint) in removal_warnings(&removal, lang) {
        spec_events::pages::push_warning(&mut out, reason, &hint);
    }
    // As cópias das OUTRAS obras que a limpeza tiraria, já que as desta
    // acabaram de sair: a obra que fechou é quando o usuário olha o projeto
    // inteiro, e a sobra de uma obra antiga ficava invisível até o disco
    // encher. A lista é a mesma da retomada.
    let orphans = crate::commands::maint::work_copies::orphans(root);
    if let Some(hint) = crate::commands::maint::work_copies::orphans_hint(&orphans, lang) {
        out["copies_left"] = json!(orphans);
        spec_events::pages::push_warning(&mut out, "copies-left", &hint);
    }
    build_output.tell(&mut out);
    for hint in &undeclared {
        spec_events::pages::push_warning(&mut out, "server-command-not-declared", hint);
    }
    for hint in &from_base {
        spec_events::pages::push_warning(&mut out, "lint-from-the-base", hint);
    }
    if let Some(hint) = &before_unchecked {
        spec_events::pages::push_warning(&mut out, "proof-before-unchecked", hint);
    }
    for hint in &unused_hints {
        spec_events::pages::push_warning(&mut out, "unused-analysis", hint);
    }
    for hint in &unowned_tests {
        spec_events::pages::push_warning(&mut out, super::common::ownerless::REASON, hint);
    }
    ask_about_findings(&mut out, questions, lang);
    // As pendências abertas nascidas nesta spec vão ao usuário na hora, para
    // ele decidir o destino de cada uma, e cada uma vai com a linha que grava
    // a resposta "fica para depois"; é aviso, nunca recusa — o fechamento
    // segue mesmo sem resposta.
    let born_open = crate::commands::event::pending::open_pending_born_in(root, &spec);
    if !born_open.is_empty() {
        let items: Vec<Value> = born_open
            .iter()
            .map(|item| {
                json!({
                    "id": item.id,
                    "title": item.title,
                    "question": crate::commands::event::pending::destination_question(&item.id, &item.title, &spec, lang),
                    "command": later_command(&spec, &item.id),
                })
            })
            .collect();
        out["pending"] = json!(items);
    }
    if !left_for_later.is_empty() {
        out["pending_released"] = json!(left_for_later);
    }
    // O item combinado sem dono que nenhum envio levou para o pedido de
    // onda nenhuma fica fora do código para sempre, sem que nada avise; a
    // resposta do fechamento avisa, com o código e o título de cada um — é
    // aviso, nunca recusa.
    let carried: BTreeSet<u64> =
        log.last_by_wave("send").keys().filter_map(|wave| recorded_choice(&log, *wave)).flat_map(|choice| choice.added.into_iter().map(|(id, _)| id)).collect();
    let codes = log.codes();
    for item in unowned(&log).into_iter().filter(|item| !carried.contains(&item.id)) {
        let code = codes.get(&item.id).cloned().unwrap_or_else(|| item.id.to_string());
        let title = title_of(item).unwrap_or_default();
        let hint = translate("close.unowned_item", lang).replace("{code}", &code).replace("{title}", &title);
        spec_events::pages::push_warning(&mut out, "unowned-item", &hint);
    }
    // O fechamento manda copiar, menos com a cópia que não pôde ser
    // preparada, que fica para a próxima — e o pull request vem depois.
    let then = match command.as_str() {
        Some(line) => translate("close.next", lang).replace("{command}", line),
        None => translate("resume.next.closed", lang).to_string(),
    };
    out["next"] = json!(then);
    out["panel"] = json!("/mustard:mods");
    let withheld = spec_events::pages::secrets::withheld(&log);
    spec_events::pages::note_withheld(&mut out, &spec, &withheld, lang);
    if !command.is_null() {
        out["command"] = command;
    }
    Ok(out)
}

/// A resposta do fechamento que gravou como tarefas os achados do veredito
/// final aprovado — `found`, cada evento gravado com o tipo e o número, e a
/// pergunta de cada achado sem regra escrita —, lidos em `log`: a obra segue
/// em execução, o que o fechamento gravou (`recorded`) ganha esses eventos, o
/// passo seguinte é a rodada, com o código de cada tarefa, no idioma `lang` —
/// nunca o pull request —, e as perguntas vão ao usuário
/// ([`ask_about_findings`]).
fn final_leftovers_answer(spec: &str, mut recorded: Vec<Value>, found: crate::commands::flow::round::FinalFindings, log: &SpecLog, lang: Locale) -> Value {
    let codes = log.codes();
    let tasks: Vec<String> = found
        .recorded
        .iter()
        .filter(|entry| entry["type"] == json!("task"))
        .filter_map(|entry| entry["id"].as_u64())
        .map(|id| codes.get(&id).cloned().unwrap_or_else(|| id.to_string()))
        .collect();
    let command = super::common::step_command("round", spec, &State::from_log(log)).unwrap_or_default();
    let next = translate("close.final_leftovers", lang).replace("{tasks}", &tasks.join(", ")).replace("{command}", &command);
    recorded.extend(found.recorded);
    let mut out = json!({ "ok": true, "spec": spec, "phase": "running", "recorded": recorded, "next": next, "command": command });
    ask_about_findings(&mut out, found.questions, lang);
    out
}

/// Leva à resposta `out` do fechamento as perguntas `questions`, uma por
/// achado da revisão final sem regra escrita na spec, em `questions`, com o
/// aviso que manda fazê-las ao usuário antes do pull request, no idioma
/// `lang`. Sem pergunta, a resposta não muda.
fn ask_about_findings(out: &mut Value, questions: Vec<Value>, lang: Locale) {
    if questions.is_empty() {
        return;
    }
    out["questions"] = json!(questions);
    spec_events::pages::push_warning(out, "final-finding-question", translate("finding.questions_hint", lang));
}

/// A linha que grava a resposta "fica para depois" da pendência `id`, pronta
/// para o assistente rodar assim que o usuário responder: ela sai junto com a
/// pergunta, e o motivo entra no lugar do `…`.
fn later_command(spec: &str, id: &str) -> String {
    format!("mustard-rt run close --spec {spec} --pending-later \"{id}=…\"")
}

/// Grava a resposta "fica para depois" de cada pendência desta obra: a
/// pendência solta da obra e passa a ser do projeto, com o motivo que o
/// usuário deu. Devolve o que saiu, na ordem em que ele respondeu.
///
/// Só sai a pendência ABERTA que nasceu nesta obra e ainda é dela — a mesma
/// leitura que fez a pergunta. Um número de fora, já resolvido ou já solto é
/// ignorado em silêncio: o aviso do fechamento é aviso, nunca recusa, e uma
/// resposta torta não pode segurar a obra.
fn hand_pending_to_project(root: &Path, spec: &str, log: &SpecLog, answers: &[String]) -> Vec<Value> {
    if answers.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<Value> = Vec::new();
    for answer in answers {
        let (id, later) = match answer.split_once('=') {
            Some((id, reason)) => (id.trim(), reason.trim()),
            None => (answer.trim(), ""),
        };
        let Some(id) = crate::commands::event::pending::pending_id(id) else {
            continue;
        };
        // A leitura da obra é refeita a cada resposta: a anterior já mudou a
        // lista, e é ela que diz quem ainda é da obra.
        let Some(item) = crate::commands::event::pending::open_born_in(root, log).into_iter().find(|i| i.id == id) else {
            continue;
        };
        let reason = Some(later).filter(|r| !r.is_empty());
        if crate::commands::event::pending::hand_to_project(root, &id, spec, reason) {
            let mut released = json!({ "id": item.id, "title": item.title, "owner": "project" });
            if let Some(reason) = reason {
                released["later"] = json!(reason);
            }
            out.push(released);
        }
    }
    out
}

/// A máquina do fechamento: os comandos que o servidor roda — a compilação,
/// o lint e a suíte inteira, nessa ordem —, cada um em ambiente limpo
/// ([`clean_env`]), e depois cada prova de [`super::validation::proof_list`]
/// — a dos critérios e a dos pedidos que as tarefas entregues cobrem —, uma
/// vez, na cópia limpa
/// `clean` do commit da obra, onde a prova tem de passar rodando teste, e uma
/// vez na cópia do código de antes (`before`, [`proof_runs`]), onde ela tem
/// de cair, com a execução de cada uma gravada. A compilação que falha recusa antes do
/// resto rodar, porque o resto cairia pelo mesmo erro. Com ela verde, o lint,
/// a suíte, os critérios e a análise de declarações sem uso rodam todos, e a
/// recusa lista tudo o que caiu numa resposta só. A
/// rodada junta e comita sem compilar: esta é a compilação de conferência da
/// obra.
///
/// **Um lugar só.** Os comandos são o `buildCommand`, o `lintCommand` e o
/// `testCommand` do `mustard.json`: a mesma declaração que o pedido de cada
/// onda cita, e não
/// uma segunda lista escrita aqui. O
/// projeto que quer o fechamento igual ao servidor declara ali, ao pé da
/// letra, o que o servidor roda — no próprio Mustard, as linhas `Test` e
/// `Clippy` de `.github/workflows/ci.yml`.
/// Invocações simples de ESLint com correção automática recebem `--no-fix`:
/// a validação mantém os erros e não reescreve os fontes. O recibo registra
/// o comando executado; scripts opacos não são reinterpretados.
///
/// **O projeto que não declara.** Fora do próprio Mustard, um projeto pode
/// não ter servidor nenhum, ou não ter dito ao Mustard o que ele roda: o
/// comando que falta não roda, e a resposta leva um aviso por chave que
/// falta, dizendo que o fechamento não promete o que o servidor vai dizer.
/// É aviso, nunca recusa: recusar travaria todo projeto que nunca declarou,
/// sem nada que ele pudesse consertar no código da obra. A compilação que
/// falta nem aviso dá: há linguagem sem passo de compilar, e o plano já avisa
/// o projeto que não a declara.
fn machine(
    opts: &CloseOpts,
    root: &Path,
    (clean, before): (&Path, Option<&proof_runs::BeforeCopy>),
    spec: &str,
    log: &SpecLog,
    lang: Locale,
) -> Result<Machine, CloseRefusal> {
    let mut timing = super::validation::Stage::new(root, spec, "final", "machine", true);
    let fingerprint = super::validation::fingerprint(root, log);
    let config = mustard_core::ProjectConfig::load(root);
    let declared = config.commands();
    // O comando de teste reconhece a prova que o relato de um pedido nomeia,
    // como na formação do lote.
    let test = declared.test.clone();
    // A cópia limpa nunca recebe os arquivos locais do projeto: a recusa dos
    // comandos os nomeia.
    let local_files = mustard_core::io::wave_prompt::local_files(&config);
    let mut undeclared: Vec<String> = Vec::new();
    let mut from_base: Vec<String> = Vec::new();
    let mut native_outputs = Vec::new();
    let mut stages: Vec<Value> = Vec::new();
    // Tudo o que cai entra numa recusa só. A compilação vermelha é a exceção:
    // o lint, a suíte e os critérios cairiam pelo mesmo erro dela.
    let mut failures: Vec<CloseRefusal> = Vec::new();
    for (key, command) in [("buildCommand", declared.build), ("lintCommand", declared.lint), ("testCommand", declared.test)] {
        let Some(command) = command else {
            if key != "buildCommand" {
                undeclared.push(translate("close.server_command_not_declared", lang).replace("{key}", key));
            }
            continue;
        };
        let command = if key == "lintCommand" { crate::commands::review::qa_run::readonly_lint_command(clean, &command) } else { command };
        // Nenhum deles é prova de critério: eles não prometem rodar um
        // teste pelo nome, e a leitura de quantos testes a saída diz fica
        // fora do caminho deles. O teto também é o deles, de uma hora, e não
        // o de um critério: a compilação e a suíte inteira levam o tempo que o
        // projeto pede.
        let out = crate::commands::review::qa_run::run_server_command(&clean_env(&command), clean);
        super::validation::record_run(root, spec, "final", key, &command, &out).map_err(CloseRefusal::Refused)?;
        stages.push(json!({ "stage": key, "result": out.result }));
        if out.result != "pass" {
            if key == "buildCommand" {
                return Err(CloseRefusal::BuildFailed { command, output: out.output, local_files, no_wave: None });
            }
            // O lint que só acusa arquivo que a obra não tocou e que a base
            // já acusava não é erro da obra: ele vira aviso, e o fechamento
            // segue. A suíte não entra nesta conta: um teste que cai fala de
            // código de qualquer lugar, e não do arquivo em que ele mora.
            if key == "lintCommand"
                && let Some(base_files) = already_in_the_base(root, log, &out.output)
            {
                let files = base_files.iter().map(|file| format!("`{file}`")).collect::<Vec<_>>().join(", ");
                let hint = translate("close.lint_from_the_base", lang)
                    .replace("{command}", &command)
                    .replace("{files}", &files)
                    .replace("{output}", &out.output);
                from_base.push(hint);
                native_outputs.push(out.evidence);
                continue;
            }
            failures.push(match key {
                "lintCommand" => CloseRefusal::LintFailed { command, output: out.output, local_files: local_files.clone(), no_wave: None },
                _ => CloseRefusal::SuiteFailed { command, output: out.output, local_files: local_files.clone(), no_wave: None },
            });
            continue;
        }
        native_outputs.push(out.evidence);
    }

    // Cada prova roda uma vez, e cada execução é gravada. A lista e as
    // execuções andam na mesma ordem: o pedido que nomeia duas provas aparece
    // duas vezes com o mesmo número, cada vez com o comando dela.
    let proofs = super::validation::proof_list(log, test.as_deref());
    let outcomes = crate::commands::review::qa_run::run_criteria_proofs(clean, &proofs);
    let mut runs: Vec<Value> = Vec::new();
    for ((id, code, command), (_, _, out)) in proofs.iter().zip(&outcomes) {
        native_outputs.push(out.evidence.clone());
        super::validation::record_run(root, spec, "final", &format!("criterion:{code}"), command, out).map_err(CloseRefusal::Refused)?;
        if let Some(fault) = out.fault() {
            let (of, code, command) = (ProofOf::item(log, *id), code.clone(), command.clone());
            failures.push(match fault {
                ProofFault::RanNoTest(tests) => CloseRefusal::CriterionRanNoTest { of, code, command, tests },
                ProofFault::MissingTest(name) => CloseRefusal::CriterionMissingTest { of, code, name },
                ProofFault::Failed(output) => CloseRefusal::CriterionFailed { of, code, output },
            });
        }
        let mut draft = Map::new();
        draft.insert("criterion".into(), json!(id));
        draft.insert("result".into(), json!(out.result));
        draft.insert("exit".into(), json!(out.exit));
        draft.insert("ms".into(), json!(out.ms));
        draft.insert("reused".into(), json!(out.reused));
        draft.insert("author".into(), json!("binary"));
        let kept_output = !out.output.trim().is_empty();
        if kept_output {
            draft.insert("output".into(), json!(out.output));
        }
        let written = record(&opts.root, spec, "criterion_run", draft, PhaseWriter::Binary).map_err(CloseRefusal::Refused)?;
        // Uma linha por critério, com o resultado e o motivo; a saída inteira
        // fica gravada na execução, e a linha diz como lê-la.
        let mut line = json!({ "criterion": code, "result": out.result });
        if let Some(fault) = out.fault() {
            line["reason"] = json!(fault_reason(&fault));
        }
        if out.reused {
            line["reused"] = json!(true);
        }
        if kept_output && let Some(run) = written.written.code.as_deref() {
            line["read"] = json!(crate::commands::flow::round::read_command(root, spec, &format!("item-{run}")));
        }
        runs.push(line);
    }
    // Cada prova roda de novo no código de antes da obra, onde tem de cair;
    // a que passa ali rodando teste não prova nada novo. Sem commit base, não
    // há código de antes, e a resposta avisa.
    let mut before_unchecked = None;
    match before {
        Some(copy) => {
            for ((id, code, command), (_, _, out)) in proofs.iter().zip(copy.run(&proofs)) {
                super::validation::record_run(root, spec, "final", &format!("criterion-before:{code}"), command, &out).map_err(CloseRefusal::Refused)?;
                if let Some(tests) = proof_runs::passed_before(&out) {
                    let of = ProofOf::item(log, *id);
                    failures.push(CloseRefusal::CriterionPassedBefore { of, code: code.clone(), command: command.clone(), tests });
                }
            }
        }
        None if !proofs.is_empty() => before_unchecked = Some(translate("close.proof_before_unchecked", lang).to_string()),
        None => {}
    }
    failures.extend(unused::refusal(root, spec, log, lang, &native_outputs));
    if let Some(refusal) = CloseRefusal::all(failures) {
        return Err(refusal);
    }
    let after = super::validation::fingerprint(root, log);
    if fingerprint != after {
        return Err(CloseRefusal::ValidationInputsChanged);
    }
    if let Some(key) = fingerprint {
        super::validation::receipt(root, spec, &key).map_err(CloseRefusal::Refused)?;
    }
    timing.passed();
    Ok(Machine { runs, stages, undeclared, from_base, before_unchecked })
}

/// O motivo curto de a prova não ter passado, o mesmo da recusa do
/// fechamento.
fn fault_reason(fault: &ProofFault) -> &'static str {
    match fault {
        ProofFault::RanNoTest(_) => "criterion-ran-no-test",
        ProofFault::MissingTest(_) => "criterion-missing-test",
        ProofFault::Failed(_) => "criterion-failed",
    }
}

/// A falha da máquina do fechamento (`failure`) reabre, por um evento próprio
/// do binário, só as ondas do plano que ela implica, pela fila e pelo envio
/// autorizado de sempre; não se passa por revisão nem aceita volta que
/// ninguém pediu. Devolve a recusa que o fechamento mostra.
///
/// O que a máquina do fechamento rodou e tem a dizer: a execução de cada
/// critério, os avisos dos comandos que o projeto não declara, os do lint que
/// só caiu no que já era da base e o de que as provas não rodaram no código
/// de antes da obra, por falta de commit base.
#[derive(Default)]
struct Machine {
    runs: Vec<Value>,
    /// Uma linha por comando do servidor que rodou, com o resultado.
    stages: Vec<Value>,
    undeclared: Vec<String>,
    from_base: Vec<String>,
    before_unchecked: Option<String>,
}

/// Os arquivos que a saída `output` acusa e que já eram da base: a saída não
/// cita arquivo nenhum da obra, cita ao menos um de fora, e todos os de fora
/// já apareciam na conferência que o primeiro despacho de onda gravou. `None`
/// quando a falha é da obra, e é ela que trava o fechamento.
///
/// Sem a marca do primeiro despacho, nada é da base: o fechamento cobra tudo,
/// como antes dela. Na correção do usuário o fechamento caiu num arquivo de
/// outro time, mudado na base seis dias antes, que a obra não toca; o projeto
/// dele já tinha cinco exceções para contornar isso.
fn already_in_the_base(root: &Path, log: &SpecLog, output: &str) -> Option<Vec<String>> {
    let planned: Vec<u64> = log.planned_waves().into_iter().collect();
    let work = super::validation::Work::of(root, log, &planned);
    let cited = super::validation::cited(root, output, &work);
    if cited.none || !cited.mine.is_empty() || cited.outside.is_empty() {
        return None;
    }
    let base = super::validation::base_findings(root, log, &work);
    cited.outside.iter().all(|file| base.contains(file)).then_some(cited.outside)
}

/// A compilação, o lint e a suíte reabrem só as ondas que declararam ou
/// entregaram um arquivo da obra que a saída cita, pela mesma leitura da
/// rodada ([`super::validation::cited`]); com a linha citada, só a onda cujo
/// commit mudou a linha por último. Sem onda citada, nenhuma reabre:
/// eles rodam pela primeira vez aqui, sobre a obra inteira, e não há base para
/// culpar ninguém; a recusa diz o que a saída citou, e a decisão é do
/// usuário. A análise de declarações sem uso passa pela mesma leitura, com o
/// texto dos achados que recusam, que cita o arquivo de cada declaração: a
/// onda de cada achado vem dos commits da obra, e o commit de duas ondas daria
/// a cada uma os arquivos da outra. O critério que falha reabre as ondas que
/// o cobrem. A prova de pedido que falha reabre as ondas das tarefas
/// entregues que cobrem o pedido, pela mesma leitura que pôs a prova dele na
/// lista ([`delivered_requests`]). Com mais de uma falha, cada onda reabre
/// uma vez só, com o texto de todas as falhas que a implicam.
fn authorize_final_repair(root: &Path, spec: &str, log: &SpecLog, failure: CloseRefusal, lang: Locale) -> CloseRefusal {
    let failures = match failure {
        CloseRefusal::Many(all) => all,
        one => vec![one],
    };
    let planned: Vec<u64> = log.planned_waves().into_iter().collect();
    // As falhas como a recusa as mostra e, por onda, as que a implicam.
    let mut shown: Vec<CloseRefusal> = Vec::new();
    let mut by_wave: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
    for failure in failures {
        let implicated: Vec<u64> = match &failure {
            CloseRefusal::BuildFailed { output, .. }
            | CloseRefusal::LintFailed { output, .. }
            | CloseRefusal::SuiteFailed { output, .. }
            | CloseRefusal::NewUnused { text: output, .. } => {
                let cited = super::validation::cited(root, output, &super::validation::Work::of(root, log, &planned));
                if cited.waves.is_empty() {
                    shown.push(no_wave_reopened(failure, &cited, lang));
                    continue;
                }
                cited.waves
            }
            CloseRefusal::CriterionFailed { of, code, .. }
            | CloseRefusal::CriterionRanNoTest { of, code, .. }
            | CloseRefusal::CriterionMissingTest { of, code, .. }
            | CloseRefusal::CriterionPassedBefore { of, code, .. } => {
                let item = log.codes().into_iter().find(|(_, c)| c == code).map(|(id, _)| id);
                match of {
                    ProofOf::Criterion => {
                        planned.iter().copied().filter(|w| item.is_none_or(|id| log.criteria_for_waves(&[*w]).iter().any(|c| c.id == id))).collect()
                    }
                    ProofOf::Request => {
                        let current = item.and_then(|id| log.current(id)).map(|request| request.id);
                        current.and_then(|id| delivered_requests(log).remove(&id)).unwrap_or_default().into_iter().collect()
                    }
                }
            }
            _ => {
                shown.push(failure);
                continue;
            }
        };
        for wave in implicated {
            by_wave.entry(wave).or_default().push(shown.len());
        }
        shown.push(failure);
    }
    let fingerprint = super::validation::fingerprint(root, log).unwrap_or_else(|| "unknown".into());
    for (wave, implicating) in by_wave {
        if log.events.iter().any(|e| {
            e.event_type == "validation_failure"
                && e.wave() == Some(wave)
                && e.str_field("fingerprint") == Some(&fingerprint)
                && log.last_by_wave("delivered").get(&wave).is_none_or(|id| *id < e.id)
        }) {
            continue;
        }
        let messages = implicating.iter().map(|at| shown[*at].message(lang));
        let text = messages.chain([translate("close.repair_next", lang).to_string()]).collect::<Vec<_>>().join("\n");
        let reason = shown[implicating[0]].reason();
        let files = mustard_core::domain::wave_prompt::wave_files(log, wave);
        let draft = json!({"wave":wave,"final":true,"result":"rejected","text":text,"reason":reason,
            "fingerprint":fingerprint,"files":files,"author":"binary"});
        if let Err(refusal) = record(root, spec, "validation_failure", draft.as_object().cloned().unwrap_or_default(), PhaseWriter::Binary) {
            return CloseRefusal::Refused(refusal);
        }
    }
    CloseRefusal::all(shown).unwrap_or(CloseRefusal::Many(Vec::new()))
}

/// A falha da compilação, do lint, da suíte ou da análise de declarações sem
/// uso (`failure`) cuja saída não cita arquivo de onda da obra, com o que ela
/// citou (`cited`) dito na recusa: os arquivos de
/// fora da obra, nenhum arquivo do projeto, ou arquivo que a branch mudou sem
/// onda que o declarasse ou entregasse; e, em todo caso, que nenhuma onda
/// reabriu.
fn no_wave_reopened(failure: CloseRefusal, cited: &super::validation::Cited, lang: Locale) -> CloseRefusal {
    let why = if cited.none {
        translate("close.cited_nothing", lang).to_string()
    } else if cited.outside.is_empty() {
        translate("close.cited_unowned", lang).to_string()
    } else {
        let files = cited.outside.iter().map(|file| format!("`{file}`")).collect::<Vec<_>>().join(", ");
        translate("close.cited_outside", lang).replace("{files}", &files)
    };
    match failure {
        CloseRefusal::BuildFailed { command, output, local_files, .. } => CloseRefusal::BuildFailed { command, output, local_files, no_wave: Some(why) },
        CloseRefusal::LintFailed { command, output, local_files, .. } => CloseRefusal::LintFailed { command, output, local_files, no_wave: Some(why) },
        CloseRefusal::SuiteFailed { command, output, local_files, .. } => CloseRefusal::SuiteFailed { command, output, local_files, no_wave: Some(why) },
        CloseRefusal::NewUnused { text, .. } => CloseRefusal::NewUnused { text, no_wave: Some(why) },
        other => other,
    }
}

/// `command` embrulhado para rodar como o servidor o roda, e não como a
/// máquina de quem programa: uma pasta de casa nova e vazia, sem a
/// identidade do git (nem a da configuração global, nem a das variáveis), sem
/// as variáveis do Claude Code e do Mustard e, no Linux, sem o processo do
/// editor entre os pais. Cada uma dessas três já deixou uma obra verde aqui e
/// vermelha no servidor: o teste que lia o `claude` entre os pais, e o que
/// comitava sem dizer quem era o autor.
///
/// A pasta do Cargo e a do rustup ficam onde estavam: são o compilador, não a
/// casa de ninguém, e sem elas a casa nova não compilaria nada. Sair de
/// baixo do editor pede um processo que troque de pai: `setsid --fork` solta
/// o comando, que avisa por um canal nomeado (`mkfifo`) o código com que
/// saiu, e a espera é a leitura desse canal, sem laço. Sem `setsid`, e fora
/// do Linux — onde nenhuma leitura procura o editor entre os pais —, o
/// comando roda direto, com o resto do ambiente limpo.
///
/// No Windows o comando vai como está: o executor pode cair no `cmd.exe`,
/// onde este embrulho não é comando nenhum.
fn clean_env(command: &str) -> String {
    if cfg!(windows) {
        return command.to_string();
    }
    let quoted = command.replace('\'', "'\\''");
    format!(
        "mustard_cmd='{quoted}'\n\
         mustard_tmp=$(mktemp -d) || exit 1\n\
         mkdir \"$mustard_tmp/home\" || exit 1\n\
         export CARGO_HOME=\"${{CARGO_HOME:-$HOME/.cargo}}\" RUSTUP_HOME=\"${{RUSTUP_HOME:-$HOME/.rustup}}\"\n\
         export HOME=\"$mustard_tmp/home\"\n\
         unset GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL GIT_COMMITTER_NAME GIT_COMMITTER_EMAIL EMAIL\n\
         for mustard_var in $(env | sed -n 's/^\\(CLAUDE[A-Za-z0-9_]*\\)=.*/\\1/p; s/^\\(MUSTARD_[A-Za-z0-9_]*\\)=.*/\\1/p'); do unset \"$mustard_var\"; done\n\
         export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_COUNT=1 \
         GIT_CONFIG_KEY_0=user.useConfigOnly GIT_CONFIG_VALUE_0=true\n\
         if [ \"$(uname -s)\" = Linux ] && command -v setsid >/dev/null 2>&1; then\n\
         mkfifo \"$mustard_tmp/exit\" || exit 1\n\
         setsid --fork sh -c '(eval \"$1\"); echo $? > \"$2\"' sh \"$mustard_cmd\" \"$mustard_tmp/exit\"\n\
         read mustard_code < \"$mustard_tmp/exit\"\n\
         else\n\
         (eval \"$mustard_cmd\"); mustard_code=$?\n\
         fi\n\
         rm -rf \"$mustard_tmp\"\n\
         exit \"${{mustard_code:-1}}\""
    )
}

/// Prepara a cópia do revisor final, na vaga `path`, no commit da obra, com
/// a trava das cópias presa (`_copies`), como a rodada prepara as dela. A vaga é a
/// da última onda ([`mustard_core::io::wave_prompt::final_copy_path`]), que
/// já tem a compilação da obra, e o envio da revisão a grava. Quando um
/// revisor já trabalhou nela depois da última onda — há envio de revisão
/// depois do último envio de onda —, a cópia com mudança recusa, com os
/// arquivos, e a recusa diz como descartar: é o que o revisor anterior
/// deixou, e revisar por cima dele é ler código mexido como se fosse o da
/// obra; a limpa vai para o commit. Sem revisão depois da última
/// onda, a mudança é da onda, que a rodada já comitou no principal, e a vaga
/// é zerada no commit. Como a cópia de onda, ela recebe os arquivos locais do
/// projeto pelo conteúdo; devolve os que não chegaram e o código que a limpeza
/// da vaga guardou antes de zerá-la.
fn prepare_review_copy(
    root: &Path,
    spec: &str,
    path: &Path,
    log: &SpecLog,
    _copies: &crate::commands::git_settle::CopiesLock,
) -> Result<(Vec<String>, Vec<crate::commands::flow::round::Kept>), CloseRefusal> {
    use mustard_core::io::wave_prompt::{final_review_commit, shown};
    use mustard_core::platform::git;
    let copy = shown(path);
    let failed = |detail: String| CloseRefusal::ReviewCopyFailed { copy: copy.clone(), detail };
    let reviewed = reviewed_after_last_wave(log);
    if reviewed && path.join(".git").is_file() {
        let status = git::run(path, &["status", "--porcelain", "--untracked-files=all"]);
        if !status.ok {
            return Err(failed(status.stderr.trim().to_string()));
        }
        let files: Vec<&str> = status.stdout.lines().filter(|line| line.len() > 3).map(|line| line[3..].trim()).collect();
        if !files.is_empty() {
            return Err(CloseRefusal::ReviewCopyDirty { copy: copy.clone(), files: files.join(", ") });
        }
    }
    let commit = match final_review_commit(root, log) {
        Some(sha) => sha,
        None => git::run(root, &["rev-parse", "HEAD"]).result().map_err(&failed)?,
    };
    let owner = crate::commands::flow::round::slot_owner(spec, log, path);
    let prepared = if reviewed {
        crate::commands::flow::round::ensure_copy(root, path, &commit, &owner)
    } else {
        crate::commands::flow::round::reset_slot(root, path, &commit, &owner)
    };
    let prepared = prepared.map_err(&failed)?;
    crate::commands::flow::round::prepare_copy_dependencies(root, path).map_err(failed)?;
    Ok((prepared.missing, prepared.kept))
}

/// Um revisor já recebeu pedido depois do último envio de onda: o pedido de
/// revisão mais novo ([`review_send`]) saiu depois do envio de onda mais
/// novo, ou não há envio de onda nenhum. Os dois contam pela versão que
/// despachou ([`SpecLog::dispatch_position`]): a versão que o lançamento do
/// revisor ou a volta de uma onda grava depois não muda quando cada um saiu.
fn reviewed_after_last_wave(log: &SpecLog) -> bool {
    let Some(review) = review_send(log, None) else {
        return false;
    };
    let waves = log.visible().into_iter().filter(|e| e.event_type == "send" && e.str_field("role") != Some("review"));
    waves.map(|e| log.dispatch_position(e.id)).max().is_none_or(|wave| review.asked > wave)
}

/// O agente de teste dedicado voltou aprovado depois da última mudança da
/// obra. A leitura é a do domínio
/// (`mustard_core::domain::spec_state::final_approval`), a mesma que o portão
/// do merge usa para não pedir confirmação por uma reprovação que esta
/// aprovação já quitou.
fn final_approved(log: &SpecLog) -> bool {
    final_approval(log).is_some()
}

/// A tabela de rastreabilidade, gravada a partir do que `verdict` — o
/// veredito final aceito — já trouxe por item em `agreed`: o item, a
/// verificação (`text`) e o arquivo (`files`, juntos por vírgula quando mais
/// de um). Nada é inventado — o item que a revisão respondeu sem arquivo ou
/// sem texto fica com o campo vazio na linha, em vez de um valor calculado.
fn record_tracking_table(root: &Path, spec: &str, verdict: &SpecEvent) -> Result<(), Refusal> {
    let items: Vec<Value> = verdict
        .fields
        .get("agreed")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("item").and_then(Value::as_u64).map(|id| (id, entry)))
        .map(|(id, entry)| {
            let verification = entry.get("text").and_then(Value::as_str).unwrap_or_default().trim().to_string();
            let file = entry
                .get("files")
                .and_then(Value::as_array)
                .map(|files| files.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "))
                .unwrap_or_default();
            let met = entry.get("met").and_then(Value::as_bool).unwrap_or(false);
            json!({ "item": id, "verification": verification, "file": file, "met": met })
        })
        .collect();
    // A obra sem item combinado nenhum (até 3 pontos, feita pelo
    // orquestrador) não tem o que tabular: sem linha, a tabela não é gravada
    // — o campo `items` é obrigatório, e uma lista vazia seria recusada.
    if items.is_empty() {
        return Ok(());
    }
    let mut draft = Map::new();
    draft.insert("items".into(), json!(items));
    draft.insert("author".into(), json!("binary"));
    record(root, spec, "tracking", draft, PhaseWriter::Binary).map(|_| ())
}

/// O que falta para a obra terminar, todas as recusas de uma vez, na ordem
/// em que a resposta as mostra: onda sem commit — tirando a que
/// voltou só conferindo, sem arquivo mudado
/// ([`crate::commands::flow::round::waves_checked_only`]) —, onda com o
/// conserto pendente ([`crate::commands::flow::round::waves_pending_fix`]:
/// a última revisão dela reprovou e nenhuma entrega chegou depois), tarefa
/// ainda no backlog ([`crate::commands::flow::round::backlog_left`]), ou
/// pedido do usuário que nenhuma onda entregou. Não basta os testes
/// passarem. As ondas e os vereditos são lidos como a rodada os lê: a onda
/// que saiu do plano não é cobrada.
///
/// A leitura é a mesma da fila e do estado da página, não a de
/// `waves_to_redo`: aquela deixa de listar a onda assim que a rodada a
/// despacha de novo, antes de o conserto chegar, e um fechamento nesse meio
/// tempo fecharia com o conserto ainda em andamento — o commit da entrega
/// original da onda, anterior à reprovação, já satisfaz a exigência de commit
/// acima, e nada mais a travaria.
///
/// A onda reprovada que já entregou o conserto não trava mais: falta o
/// agente de teste dedicado conferir esse conserto, e é o fechamento —
/// pedindo o agente de novo, mais adiante — que cobra isso, não esta função.
/// Sem essa saída, o conserto nunca chegaria ao agente de teste: nada mais
/// grava um veredito comum enquanto a rodada está no ar.
fn unfinished(log: &SpecLog) -> Vec<CloseRefusal> {
    let mut missing = Vec::new();
    let rejected: BTreeSet<u64> = crate::commands::flow::round::waves_pending_fix(log).into_keys().collect();
    missing.extend(rejected.iter().map(|wave| CloseRefusal::WaveRejected { wave: *wave }));

    let committed: BTreeSet<u64> =
        log.block(BlockQuery::Block(Block::Progress)).into_iter().filter(|e| e.event_type == "commit").flat_map(|e| e.ints("waves")).collect();
    // A onda que voltou só conferindo, sem arquivo mudado, fecha sem commit:
    // a leitura é a mesma que a rodada usa ao escolher o que comitar.
    let checked_only = crate::commands::flow::round::waves_checked_only(log);
    for wave in log.planned_waves() {
        if !committed.contains(&wave) && !checked_only.contains(&wave) && !rejected.contains(&wave) {
            missing.push(CloseRefusal::WaveWithoutCommit { wave });
        }
    }

    // A tarefa que ainda está no backlog não foi entregue por onda nenhuma: a
    // leitura do backlog é a mesma da rodada e da formação do lote.
    let codes = log.codes();
    let left = crate::commands::flow::round::backlog_left(log);
    if !left.is_empty() {
        let tasks = left.iter().map(|id| codes.get(id).cloned().unwrap_or_else(|| id.to_string())).collect::<Vec<_>>();
        missing.push(CloseRefusal::BacklogNotEmpty { tasks: tasks.join(", ") });
    }

    // Um pedido do usuário que chegou depois da última entrega não foi
    // entregue por onda nenhuma.
    let last_delivered = log.block(BlockQuery::Block(Block::Waves)).into_iter().filter(|e| e.event_type == "delivered").map(|e| e.id).max().unwrap_or(0);
    for request in log.block(BlockQuery::Block(Block::Notes)).into_iter().filter(|e| e.event_type == "request") {
        if request.id > last_delivered {
            missing.push(CloseRefusal::RequestNotDelivered { code: codes.get(&request.id).cloned().unwrap_or_else(|| request.id.to_string()) });
        }
    }
    missing
}

/// A primeira recusa de [`unfinished`] para a obra `log`, pelo motivo e pela onda
/// cobrada quando há uma; `None` quando a obra está pronta para fechar. Os
/// testes da rodada conferem o fechamento lado a lado com a resposta dela.
#[cfg(test)]
pub(crate) fn finished_refusal(log: &SpecLog) -> Option<(String, Option<u64>)> {
    unfinished(log).into_iter().next().map(|refusal| {
        let wave = match &refusal {
            CloseRefusal::WaveWithoutCommit { wave } | CloseRefusal::WaveRejected { wave } => Some(*wave),
            _ => None,
        };
        (refusal.reason(), wave)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::flow::round::{RoundOpts, round_for as actual_round_for};
    use crate::commands::spec_events::write::{WriteOpts, record_open, seed_at};
    use mustard_core::domain::spec_events::SpecEvent;
    use std::process::Command;
    use tempfile::tempdir;

    pub(super) fn round_for(opts: &RoundOpts, session: Option<&str>) -> Value {
        crate::commands::flow::round::review::test_conductor_review(&opts.root, opts.spec.as_deref().unwrap_or("x"));
        actual_round_for(opts, session)
    }

    pub(super) fn close_for(opts: &CloseOpts, session: Option<&str>) -> Value {
        crate::commands::flow::round::review::test_conductor_review(&opts.root, opts.spec.as_deref().unwrap_or("x"));
        super::close_for(opts, session)
    }

    pub(super) fn write(root: &Path, spec: &str, event_type: &str, mut body: Value) -> Value {
        if event_type == "task" {
            let map = body.as_object_mut().expect("a tarefa é um objeto");
            map.entry("files").or_insert_with(|| json!([]));
            map.entry("depends_on").or_insert_with(|| json!([]));
        }
        seed_at(&WriteOpts { root: root.to_path_buf(), spec: Some(spec.to_string()), event_type: event_type.into(), json: body.to_string() })
    }

    /// A entrega que o agente da onda grava pela porta do binário, antes de
    /// a rodada assumi-la. Ela precisa sair gravada: a rodada lê a volta da
    /// spec, e não do relatório.
    pub(super) fn returned(root: &Path, spec: &str, body: Value) {
        let write = |body: &Value| {
            crate::commands::spec_events::write::write_at(&WriteOpts {
                root: root.to_path_buf(),
                spec: Some(spec.to_string()),
                event_type: "delivered".into(),
                json: body.to_string(),
            })
        };
        // O pedido não lido e a tarefa sem passo de término recusam a entrega:
        // o agente lê, grava o passo de cada tarefa e grava de novo.
        let out = crate::commands::flow::round::deliver_after_refusals(root, spec, &body, &write);
        assert_eq!(out["ok"], json!(true), "a volta não gravou: {out}");
    }

    pub(super) fn id_of(report: &Value) -> u64 {
        report["id"].as_u64().unwrap_or_else(|| panic!("não gravou: {report}"))
    }

    /// A pasta da cópia limpa em que a máquina do fechamento da obra `x` roda:
    /// é nela que os comandos do servidor escrevem o que deixam para trás.
    pub(super) fn check_copy(root: &Path) -> PathBuf {
        mustard_core::io::wave_prompt::spec_copies_dir(root, "x").join("check")
    }

    /// Comita o arquivo `file`, ainda que o git o ignore: a cópia limpa é um
    /// checkout do commit, e o script que o comando do servidor roda tem de
    /// estar nele.
    pub(super) fn commit_file(root: &Path, file: &str) {
        git_at(root, &["add", "-f", file]);
        git_at(root, &["commit", "-q", "-m", "script do servidor"]);
    }

    pub(super) fn git_at(root: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(root)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// Uma spec de uma onda, já aprovada, despachada, entregue, revisada e
    /// comitada: pronta para fechar. Ela ganha um critério por comando de
    /// `proofs`, na ordem em que eles vêm, e nenhum deles é coberto por
    /// onda nenhuma — é o fechamento, e só ele, que os prova.
    fn ready_to_close(root: &Path, spec: &str, proofs: &[&str]) {
        ready_with_waves(root, spec, proofs, 1);
    }

    /// A final machine rejection is cleared only by a native repair send and
    /// delivery. Changing the validation command alone cannot erase it.
    fn repair_machine_failure(root: &Path, spec: &str) {
        let log = store::read(&store::spec_file(root, spec).unwrap()).unwrap().unwrap();
        if log.last_rejected().is_empty() {
            return;
        }
        let dispatched = round_for(&RoundOpts { root: root.to_path_buf(), spec: Some(spec.into()), report: None }, None);
        assert_eq!(dispatched["ok"], true, "{dispatched}");
        let log = store::read(&store::spec_file(root, spec).unwrap()).unwrap().unwrap();
        for wave in log.last_rejected().keys() {
            let file = wave_file(*wave);
            let copy = mustard_core::io::wave_prompt::recorded_copy(&log, *wave).unwrap();
            let path = Path::new(&copy.path).join(&file);
            let text = std::fs::read_to_string(&path).unwrap();
            std::fs::write(path, format!("{text}\n// repair {}\n", log.events.len())).unwrap();
            returned(root, spec, json!({"wave":wave,"text":"Corrigir a validação.","commit":"corrigir validação","files":[file]}));
        }
        let integrated = round_for(&RoundOpts { root: root.to_path_buf(), spec: Some(spec.into()), report: None }, None);
        assert_eq!(integrated["ok"], true, "{integrated}");
    }

    /// O arquivo da tarefa da onda `n`.
    pub(super) fn wave_file(n: u64) -> String {
        format!("src/w{n}.rs")
    }

    /// [`ready_to_close`] com `waves` ondas soltas, cada uma com a tarefa num
    /// arquivo dela, despachadas e entregues juntas. A rodada não pede
    /// revisão de onda nenhuma: a entrega já basta, e falta só o fechamento
    /// pedir o agente de teste dedicado.
    pub(super) fn ready_with_waves(root: &Path, spec: &str, proofs: &[&str], waves: u64) {
        ready_with_checked(root, spec, proofs, waves, &[]);
    }

    /// [`ready_with_waves`] em que as ondas de `checked` voltam só
    /// conferindo: sem arquivo mudado, cada uma sozinha numa rodada depois
    /// das outras, e por isso sem commit.
    fn ready_with_checked(root: &Path, spec: &str, proofs: &[&str], waves: u64, checked: &[u64]) {
        let waves_files: Vec<String> = (1..=waves).map(wave_file).collect();
        ready_with_files(root, spec, proofs, &waves_files, checked, &[]);
    }

    /// [`ready_with_checked`] em que a onda `n` tem a tarefa no arquivo
    /// `waves_files[n - 1]`. O arquivo `.ts` nasce e é entregue em
    /// TypeScript; o resto, em Rust. Cada texto de `findings` vira a parte do
    /// agente de um pedido, como o achado de uma revisão vira, e a tarefa da
    /// primeira onda cobre esses pedidos.
    pub(super) fn ready_with_files(root: &Path, spec: &str, proofs: &[&str], waves_files: &[String], checked: &[u64], findings: &[&str]) {
        let waves = waves_files.len() as u64;
        let file_of = |n: u64| waves_files[(n - 1) as usize].clone();
        let typescript = |file: &str| file.ends_with(".ts");
        let mut files = vec![("mustard.json", "{}")];
        files.extend(waves_files.iter().map(|file| {
            (file.as_str(), if typescript(file) { "export function one(): number {\n  return 1;\n}\n" } else { "fn one() {}\n" })
        }));
        crate::shared::test_fixture::seeded_repo_on(root, &format!("feature/{spec}"), &files);

        assert_eq!(record_open(root, spec, &format!("feature/{spec}"), "dev"), Ok(true));
        let said = id_of(&write(root, spec, "message", json!({"author": "user", "text": "o objetivo"})));
        let _crits: Vec<u64> = proofs
            .iter()
            .map(|proof| {
                id_of(&write(
                    root,
                    spec,
                    "criterion",
                    json!({"when": format!("a onda roda e prova com {proof}"), "then": "a suíte passa",
                           "proof": proof, "form": "ubiquitous", "origin": said}),
                ))
            })
            .collect();
        // Nenhuma onda cobre os critérios de `proofs`: são os que este teste
        // quer ver o fechamento provar, e a rodada roda a prova de cada
        // critério que a onda cobre antes de comitar. Cobri-los aqui faria a
        // entrega travar na rodada, antes de chegar ao fechamento que o
        // teste examina. Cada onda cobre, em vez disso, um critério à parte,
        // sempre verde, gravado depois — por isso com o maior número — só
        // para satisfazer o campo obrigatório sem mexer nos índices que os
        // testes já leem de `proofs`.
        let gate = id_of(&write(
            root,
            spec,
            "criterion",
            json!({"when": "a onda roda", "then": "a suíte passa", "proof": "git --version", "form": "ubiquitous",
                "origin": said}),
        ));
        let asked: Vec<u64> = (1..)
            .zip(findings)
            .map(|(n, agent)| {
                let request = json!({"title": format!("Achado {n}"), "text": format!("O achado {n} da revisão."), "agent": agent,
                    "keys": ["achado"], "effect": "adjust_waves", "origin": said});
                id_of(&write(root, spec, "request", request))
            })
            .collect();
        for n in 1..=waves {
            write(
                root,
                spec,
                "wave",
                json!({"n": n, "text": format!("Onda {n}."), "criteria": [gate],
                "done_when": "A suíte passa.", "origin": said}),
            );
            let mut task = json!({"wave": n, "text": format!("Tarefa da onda {n}."),
                "files": [{"path": file_of(n)}], "origin": said});
            if n == 1 && !asked.is_empty() {
                task["covers"] = json!(asked);
            }
            write(root, spec, "task", task);
        }
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join(spec));
        crate::commands::flow::round::copies_leave_with_the_test(root);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":4}"#).unwrap();

        let round = |report: Option<String>| round_for(&RoundOpts { root: root.to_path_buf(), spec: Some(spec.to_string()), report }, None);
        let dispatch = round(None);
        assert_eq!(dispatch["ok"], json!(true), "{dispatch}");
        let mut delivered = false;
        for n in (1..=waves).filter(|n| !checked.contains(n)) {
            let file = file_of(n);
            let body = if typescript(&file) {
                "export function one(): number {\n  return dois();\n}\nfunction dois(): number {\n  return 2;\n}\n"
            } else {
                "fn one() { dois(); }\nfn dois() {}\n"
            };
            std::fs::write(root.join(&file), body).unwrap();
            returned(root, spec, json!({"wave": n, "text": "Saiu.", "files": [file], "commit": "a soma sai"}));
            delivered = true;
        }
        if delivered {
            let back = round(None);
            assert_eq!(back["ok"], json!(true), "{back}");
        }
        for n in checked {
            returned(root, spec, json!({"wave": n, "text": "Nada a mudar: a tarefa já estava entregue. Rodei git --version e passou."}));
            let back = round(None);
            assert_eq!(back["ok"], json!(true), "{back}");
            assert!(back.get("commit").is_none(), "a onda que só conferiu não comita: {back}");
        }
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
    }

    /// O pedido do revisor final, lido como o revisor o lê: rodando o comando
    /// que a resposta do fechamento traz em `review.read`, no lugar do texto.
    pub(super) fn review_prompt(asked: &Value) -> String {
        let command = asked["review"]["read"].as_str().unwrap_or_else(|| panic!("a resposta não traz o comando de leitura: {asked}"));
        crate::commands::spec_events::read::read_by_command(command)
    }

    /// O veredito que o revisor grava pela porta do binário. Devolve a
    /// resposta da gravação, com a recusa quando ela recusa.
    fn judged(root: &Path, spec: &str, body: Value) -> Value {
        crate::commands::flow::round::read_review(root, spec);
        crate::commands::spec_events::write::write_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: Some(spec.to_string()),
            event_type: "verdict".into(),
            json: body.to_string(),
        })
    }

    /// O veredito `body` gravado pelo revisor, com o pedido de revisão já
    /// aberto. Devolve o relatório que ele deixa depois de gravar: nenhum,
    /// porque o veredito mora na spec.
    pub(super) fn verdict_written(root: &Path, spec: &str, body: Value) -> Option<String> {
        let wrote = judged(root, spec, body);
        assert_eq!(wrote["ok"], json!(true), "o veredito não gravou: {wrote}");
        None
    }

    /// O veredito final do agente de teste dedicado aprovando a obra
    /// inteira, gravado por ele.
    pub(super) fn approve(root: &Path, spec: &str) -> Option<String> {
        verdict_written(root, spec, json!({"final": true, "result": "approved", "text": "A obra está pronta."}))
    }

    /// O fechamento, com a volta transparente do agente de teste dedicado: a
    /// obra pronta para fechar sempre passa por ele agora, mesmo a de uma
    /// onda só, e quem só quer o resultado final não precisa cuidar disso.
    /// Os testes que conferem o pedido dele por dentro chamam `close_for`
    /// direto. Com o pedido já aberto por uma chamada anterior, o fechamento
    /// espera o veredito, e o agente o grava do mesmo jeito.
    pub(super) fn close(root: &Path, spec: &str) -> Value {
        let out = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some(spec.to_string()), report: None, ..Default::default() }, None);
        if out["review"]["final"] == json!(true) || out["reason"] == json!("review-verdict-missing") {
            return close_for(&CloseOpts { root: root.to_path_buf(), spec: Some(spec.to_string()), report: approve(root, spec), ..Default::default() }, None);
        }
        out
    }

    #[test]
    fn final_close_reuses_actual_validation_and_dirty_code_config_or_ignored_local_inputs_invalidate_it() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "receipt", &["git --version"]);
        let ignore = std::fs::read_to_string(root.join(".gitignore")).unwrap_or_default();
        std::fs::write(root.join(".gitignore"), format!("{ignore}\n.env\n")).unwrap();
        std::fs::write(root.join(".env"), "FIXTURE_INPUT=first\n").unwrap();
        let config = json!({"lintCommand":"git --version","testCommand":"git --version","localFiles":[".env"]});
        std::fs::write(root.join("mustard.json"), config.to_string()).unwrap();
        let opts = CloseOpts { root: root.to_path_buf(), spec: Some("receipt".into()), ..Default::default() };
        let first = close_for(&opts, None);
        assert_eq!(first["ok"], true, "{first}");
        assert_eq!(first["validation"]["reused"], false);
        let read = || store::read(&store::spec_file(root, "receipt").unwrap()).unwrap().unwrap();
        let count = |log: &SpecLog| {
            log.events
                .iter()
                .filter(|event| {
                    event.event_type == "stage_run" && event.str_field("scope") == Some("leaf") && event.str_field("phase") == Some("final")
                })
                .count()
        };
        let before = count(&read());
        assert!(before >= 3, "lint, suite and criterion actually ran");
        approve(root, "receipt");
        let second = close_for(&opts, None);
        assert_eq!(second["phase"], "closed", "{second}");
        assert_eq!(second["validation"]["reused"], true, "{second}");
        let log = read();
        assert_eq!(count(&log), before, "review acceptance does not rerun unchanged checks");
        assert!(super::super::validation::reusable(root, &log), "closing changes runtime state, not validated project inputs");
        let code = std::fs::read(root.join(wave_file(1))).unwrap();
        std::fs::write(root.join(wave_file(1)), b"fn changed() {}\n").unwrap();
        assert!(!super::super::validation::reusable(root, &log), "dirty code changes matter without changing HEAD");
        std::fs::write(root.join(wave_file(1)), code).unwrap();
        std::fs::write(root.join(".env"), "FIXTURE_INPUT=second\n").unwrap();
        assert!(!super::super::validation::reusable(root, &log), "ignored declared input changes matter");
        std::fs::write(root.join(".env"), "FIXTURE_INPUT=first\n").unwrap();
        let mut changed = config.clone();
        changed["lintCommand"] = json!("git status --short");
        std::fs::write(root.join("mustard.json"), changed.to_string()).unwrap();
        assert!(!super::super::validation::reusable(root, &log), "command changes matter");
    }

    /// O pedido que o fechamento daria ao revisor final sai antes de valer
    /// pela leitura `request-review-preview`, sem gravar nada na spec: nem
    /// envio, nem leitura. Após a validação nativa, coincide com o pedido
    /// gravado pelo fechamento, sem listar o recibo da máquina, que já é prova.
    #[test]
    fn the_review_preview_prints_the_request_the_close_records_and_records_nothing() {
        use crate::commands::spec_events::read::{ReadOpts, read_for};

        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let path = store::spec_file(root, "x").unwrap();
        let before = std::fs::read(&path).unwrap();

        let opts = ReadOpts { root: root.to_path_buf(), spec: Some("x".into()), block: "request-review-preview".into(), term: None , session: None };
        let preview = read_for(&opts, None, root).unwrap_or_else(|refused| panic!("{refused}"));
        assert!(preview.lines().any(|line| line.starts_with("- ") && line.contains("MSTD-")), "{preview}");
        assert_eq!(std::fs::read(&path).unwrap(), before, "the preview records nothing");

        let asked = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() }, None);
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        let after=std::fs::read(&path).unwrap();
        let validated_preview=read_for(&opts,None,root).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(),after,"reading new receipts still records nothing");
        assert_eq!(review_prompt(&asked),validated_preview,"the preview of the validated state matches the recorded request");
        assert!(!validated_preview.contains("## Recibos da validação final registrada"), "the machine receipt is proof, not reading");
    }

    /// O pedido do agente de revisão final vira evento de envio no
    /// spec.ndjson antes de sair para quem despacha: o texto inteiro, o
    /// papel de revisão, o modelo e o esforço — pela mesma porta que já grava o pedido
    /// de cada onda, sem onda dona nenhuma. O envio grava a lista dos itens
    /// que o pedido lista, a mesma que o texto imprime, e o veredito só grava
    /// depois de o revisor ler cada um de dentro da cópia dele: a leitura
    /// feita do repositório principal não conta, e a recusa nomeia o item que
    /// falta.
    #[test]
    fn review_request_becomes_a_send_event_and_the_verdict_waits_for_the_reading() {
        use crate::commands::spec_events::read::{ReadOpts, read_for};

        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);

        let path = store::spec_file(root, "x").unwrap();
        // Dois combinados que nenhuma tarefa cobre: são o que fica sem prova
        // verde, e o pedido os lista.
        let said = store::read(&path).unwrap().unwrap().visible().into_iter().find(|e| e.event_type == "message").map(|e| e.id).unwrap();
        for (title, text) in [("Nome curto", "O nome da função é curto."), ("Erro claro", "A mensagem de erro diz o que fazer.")] {
            let rule = json!({"title": title, "agent": "- conferir no código", "text": text, "example": "e", "keys": ["k"], "applies_to": {"files": ["**"]}, "origin": said});
            let wrote = crate::commands::spec_events::write::write_at(&WriteOpts {
                root: root.to_path_buf(),
                spec: Some("x".into()),
                event_type: "rule".into(),
                json: rule.to_string(),
            });
            assert_eq!(wrote["ok"], json!(true), "{wrote}");
        }
        let before = store::read(&path).unwrap().unwrap();
        let sent_before = before.visible().into_iter().filter(|e| e.event_type == "send").count();

        let asked = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() }, None);
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        assert!(asked["review"].get("prompt").is_none(), "a resposta não leva o pedido inteiro: {asked}");
        assert_eq!(
            asked["review"]["read"],
            json!(format!("mustard-rt run read request-review --root {} --spec x", mustard_core::io::wave_prompt::shown(root))),
            "{asked}"
        );
        let prompt = review_prompt(&asked);
        assert!(!prompt.is_empty(), "{asked}");
        assert_eq!(asked["review"]["lines"], json!(count_lines(&prompt)), "{asked}");
        let opening = prompt.lines().next().unwrap_or_default();
        assert!(!opening.is_empty() && !asked.to_string().contains(opening), "o texto do pedido não está na resposta: {asked}");
        let next = asked["next"].as_str().unwrap_or_default();
        assert!(next.contains("review.read") && !next.contains("review.prompt"), "o passo cita o campo que a resposta traz: {next}");

        let after = store::read(&path).unwrap().unwrap();
        let sent: Vec<&SpecEvent> = after.visible().into_iter().filter(|e| e.event_type == "send").collect();
        assert_eq!(sent.len(), sent_before + 1, "grava exatamente um envio novo: {sent:?}");
        let sent = sent.last().unwrap_or_else(|| panic!("nenhum envio gravado"));
        assert_eq!(sent.str_field("role"), Some("review"), "{sent:?}");
        assert_eq!(sent.str_field("text"), Some(prompt.as_str()), "o texto gravado é o pedido inteiro que voltou: {sent:?}");
        assert_eq!(sent.str_field("model"), Some("sonnet"), "o modelo pedido vai junto, o padrão da instalação: {sent:?}");
        assert_eq!(sent.str_field("effort"), Some("xhigh"), "o esforço pedido vai junto, o padrão da instalação: {sent:?}");
        assert!(sent.wave().is_none(), "a revisão final não é dona de onda nenhuma: {sent:?}");

        let recorded: Vec<String> = sent
            .fields
            .get("read_items")
            .and_then(Value::as_array)
            .expect("the send records the list")
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect();
        let mut printed: Vec<String> = Vec::new();
        for line in prompt.lines().filter(|line| line.starts_with("- ")) {
            let Some(at) = line.find("MSTD-") else {
                continue;
            };
            let code = line[at..].split(' ').next().unwrap_or_default().to_string();
            if !printed.contains(&code) {
                printed.push(code);
            }
        }
        assert_eq!(printed.len(), 2, "only the two agreed items lack a green proof: {prompt}");
        assert_eq!(recorded, printed, "the send records the codes the request prints: {prompt}");

        let answers: Vec<Value> = recorded.iter().map(|code| json!({"item": code, "met": true})).collect();
        let copy = sent.str_field("copy").expect("the reviewer's copy").to_string();
        let read = |code: &str, from: &Path| {
            let opts = ReadOpts { root: root.to_path_buf(), spec: Some("x".into()), block: format!("item-{code}"), term: None , session: None };
            read_for(&opts, None, from).unwrap_or_else(|refused| panic!("{code}: {refused}"));
        };
        let approved = || {
            crate::commands::spec_events::write::write_at(&WriteOpts {
                root: root.to_path_buf(),
                spec: Some("x".into()),
                event_type: "verdict".into(),
                json: json!({"final": true, "result": "approved", "text": "A obra está pronta.", "agreed": answers}).to_string(),
            })
        };

        let refused = approved();
        assert_eq!(refused["reason"], json!("verdict-read-missing"), "{refused}");
        for code in &recorded {
            assert!(refused["hint"].as_str().unwrap_or_default().contains(code.as_str()), "{code}: {refused}");
        }
        for code in &recorded {
            read(code, root);
        }
        assert_eq!(approved()["reason"], json!("verdict-read-missing"), "reading from the main repository does not count");

        for code in &recorded[1..] {
            read(code, Path::new(&copy));
        }
        let refused = approved();
        assert_eq!(refused["reason"], json!("verdict-read-missing"), "{refused}");
        let hint = refused["hint"].as_str().unwrap_or_default();
        assert!(hint.contains(recorded[0].as_str()) && !hint.contains(recorded[1].as_str()), "{refused}");

        read(&recorded[0], Path::new(&copy));
        assert_eq!(approved()["ok"], json!(true), "the verdict passes once every item was read from the copy");
    }

    /// O envio do pedido da revisão final grava o modelo com que o revisor é
    /// lançado — o de `agents.review`, e sem ele o de `agents.model` — e o
    /// esforço `xhigh`, que todo agente leva, sobre o que a configuração diz.
    #[test]
    fn the_final_review_send_carries_the_model_the_reviewer_is_launched_with() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        std::fs::write(root.join("mustard.json"), br#"{"agents":{"model":"opus","effort":"medium","review":{"model":"haiku","effort":"max"}}}"#).unwrap();
        git_at(root, &["add", "-A"]);
        git_at(root, &["commit", "-q", "-m", "modelo dos agentes"]);

        let asked = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() }, None);
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let sent = log.visible().into_iter().rfind(|e| e.event_type == "send").unwrap_or_else(|| panic!("nenhum envio gravado"));
        assert_eq!(sent.str_field("role"), Some("review"), "{sent:?}");
        assert_eq!(sent.str_field("model"), Some("haiku"), "the reviewer's own model: {sent:?}");
        assert_eq!(sent.str_field("effort"), Some("xhigh"), "the effort is always xhigh: {sent:?}");
    }

    /// Com o `CLAUDE.md` na raiz do projeto e outro na pasta do arquivo de uma
    /// tarefa, o pedido da revisão final que o fechamento grava — o que o
    /// revisor lê pelo comando da resposta — fecha com a seção das regras do
    /// projeto, com o texto de cada um inteiro, uma vez, a raiz primeiro: o
    /// revisor é instalado sem os arquivos de instrução e só as recebe por
    /// aqui. A pasta com regras e sem arquivo da obra fica de fora.
    #[test]
    fn the_final_review_request_ends_with_the_rules_of_the_root_and_of_the_touched_folders() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let rules = "# Regras\n\n- O instalador nunca grava na configuração do git.\n- A prova instala numa pasta vazia.";
        std::fs::write(root.join("CLAUDE.md"), format!("\n{rules}\n\n")).unwrap();
        std::fs::write(root.join("src/CLAUDE.md"), "- As fontes não leem a rede.\n").unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("docs/CLAUDE.md"), "- Os textos seguem o guia.\n").unwrap();
        git_at(root, &["add", "-A"]);
        git_at(root, &["commit", "-q", "-m", "regras do projeto"]);

        let asked = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() }, None);
        let prompt = review_prompt(&asked);
        let lang = spec_events::project(root).lang;
        let file = |path: &str, text: &str| mustard_core::domain::wave_prompt::RulesFile { path: path.into(), text: text.into() };
        let both = [file("CLAUDE.md", rules), file("src/CLAUDE.md", "- As fontes não leem a rede.")];
        let section = mustard_core::domain::wave_prompt::project_rules_section(&both, lang).unwrap_or_default();
        assert!(prompt.ends_with(&format!("\n\n{section}\n")), "{prompt}");
        assert_eq!(prompt.matches("configuração do git").count(), 1, "{prompt}");
        assert!(!prompt.contains("seguem o guia"), "{prompt}");
    }

    /// O fechamento confere os agentes do projeto antes de pedir a revisão:
    /// o revisor com o texto de outra versão volta ao que a instalação
    /// escreveria, e a resposta que pede a revisão o nomeia. O agente de onda,
    /// já igual, não entra no aviso.
    #[test]
    fn closing_rewrites_the_stale_review_agent_before_asking_for_the_review() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let folder = root.join(".claude/agents/mustard");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("wave.md"), crate::commands::flow::round::shipped_agent(root, "wave")).unwrap();
        std::fs::write(folder.join("review.md"), "---\nname: mustard-review\n---\nO revisor antigo.\n").unwrap();

        let asked = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() }, None);
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        let review = std::fs::read_to_string(folder.join("review.md")).unwrap();
        assert_eq!(review, crate::commands::flow::round::shipped_agent(root, "review"));
        let warnings = asked["warnings"].as_array().cloned().unwrap_or_default();
        let said: Vec<&Value> = warnings.iter().filter(|w| w["reason"] == json!("agents-refreshed")).collect();
        assert_eq!(said.len(), 1, "{asked}");
        let hint = said[0]["hint"].as_str().unwrap_or_default();
        assert!(hint.contains(".claude/agents/mustard/review.md") && !hint.contains("wave.md"), "{hint}");
    }

    /// O revisor grava o veredito, e o fechamento o assume sem relatório. A
    /// volta gravada antes de o fechamento pedir a revisão é recusada. Pedida
    /// a revisão, o fechamento sem veredito gravado não abre outro pedido: a
    /// resposta manda o revisor gravar o dele. Gravado o veredito, o
    /// fechamento grava o oficial, com `replaces` para a volta, e fecha.
    #[test]
    fn closing_takes_the_verdict_written_by_the_reviewer() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let approved = json!({"final": true, "result": "approved", "text": "A obra está pronta."});
        let early = judged(root, "x", approved.clone());
        assert_eq!(early["reason"], json!("no-open-review"), "sem pedido de revisão, a volta não entra: {early}");

        let close = || close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() }, None);
        let read = || store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let asked_reviews = || {
            let log = read();
            log.visible().iter().filter(|e| e.event_type == "send" && e.str_field("role") == Some("review")).count()
        };
        let asked = close();
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        assert_eq!(asked_reviews(), 1, "{asked}");

        let waiting = close();
        assert_eq!(waiting["reason"], json!("review-verdict-missing"), "{waiting}");
        assert_eq!(waiting["hint"], json!(translate("spec_events.verdict_missing", Locale::PtBr)), "{waiting}");
        assert_eq!(asked_reviews(), 1, "o fechamento sem veredito não abre outro pedido de revisão");
        assert_eq!(State::from_log(&read()).phase, Some("running"), "a spec não fechou");

        let wrote = judged(root, "x", approved);
        assert_eq!(wrote["ok"], json!(true), "{wrote}");
        let returned_id = wrote["id"].as_u64().unwrap();
        let closed = close();
        assert_eq!(closed["phase"], json!("closed"), "{closed}");
        let log = read();
        let verdicts: Vec<&SpecEvent> = log.visible().into_iter().filter(|e| e.event_type == "verdict").collect();
        assert_eq!(verdicts.len(), 1, "{verdicts:?}");
        assert_eq!(verdicts[0].fields.get("replaces"), Some(&json!(returned_id)), "o oficial aponta a volta");
        assert_eq!(verdicts[0].str_field("result"), Some("approved"), "{verdicts:?}");
        assert_eq!(verdicts[0].str_field("author"), Some("review"), "{verdicts:?}");
        assert!(!verdicts[0].returned(), "o oficial não é volta: {verdicts:?}");
        assert_eq!(asked_reviews(), 1, "{closed}");
    }

    /// O revisor que grava o veredito enquanto o fechamento tem o passo do git
    /// e assume o pedido de revisão: a gravação espera a trava e, com o
    /// pedido já fechado pelo veredito oficial, é recusada, e o revisor grava
    /// de novo. Ela nunca responde ok com o veredito parado, sem quem o
    /// assuma. Sem relógio: o teste segue quando a lista de travas do sistema
    /// mostra a gravação parada na trava, ou falha quando ela termina antes,
    /// sem ter esperado.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_verdict_written_while_the_close_takes_the_review_is_refused_and_never_lost() {
        use crate::commands::git_settle::{git_step_lock, waiting_for_lock};
        use std::os::unix::fs::MetadataExt;

        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let asked = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() }, None);
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");

        let approved = json!({"final": true, "result": "approved", "text": "A obra está pronta."});
        let held = git_step_lock(root).unwrap();
        let inode = std::fs::metadata(root.join(".claude").join("spec").join("round-git.lock")).unwrap().ino();
        let refused = std::thread::scope(|scope| {
            let reviewer = scope.spawn(|| judged(root, "x", approved.clone()));
            while !waiting_for_lock(inode) {
                assert!(!reviewer.is_finished(), "the verdict was written while the close held the git step");
                std::thread::yield_now();
            }
            // O fechamento, com a trava presa, assume o pedido de revisão: grava
            // o veredito oficial e solta a trava.
            crate::shared::spec_state::seed_event(root, "x", "verdict", approved.clone());
            drop(held);
            reviewer.join().unwrap()
        });

        assert_eq!(refused["ok"], json!(false), "{refused}");
        assert_eq!(refused["reason"], json!("no-open-review"), "{refused}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let verdicts: Vec<&SpecEvent> = log.events.iter().filter(|e| e.event_type == "verdict").collect();
        assert_eq!(verdicts.len(), 1, "no verdict is left parked after the official one: {verdicts:?}");
        assert!(!verdicts[0].returned(), "{verdicts:?}");
    }

    /// O outro lado da mesma janela: o revisor que chega enquanto o passo do
    /// git está preso, mas antes de o fechamento assumir o pedido, espera a
    /// trava e grava: a resposta é ok, e o fechamento seguinte assume esse
    /// veredito e fecha a spec com ele.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_verdict_that_waits_for_the_git_step_is_written_and_the_close_takes_it() {
        use crate::commands::git_settle::{git_step_lock, waiting_for_lock};
        use std::os::unix::fs::MetadataExt;

        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let close = || close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() }, None);
        let asked = close();
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");

        let approved = json!({"final": true, "result": "approved", "text": "A obra está pronta."});
        let held = git_step_lock(root).unwrap();
        let inode = std::fs::metadata(root.join(".claude").join("spec").join("round-git.lock")).unwrap().ino();
        let wrote = std::thread::scope(|scope| {
            let reviewer = scope.spawn(|| judged(root, "x", approved.clone()));
            while !waiting_for_lock(inode) {
                assert!(!reviewer.is_finished(), "the verdict was written while another step held the git lock");
                std::thread::yield_now();
            }
            drop(held);
            reviewer.join().unwrap()
        });
        assert_eq!(wrote["ok"], json!(true), "{wrote}");

        let closed = close();
        assert_eq!(closed["phase"], json!("closed"), "{closed}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let verdicts: Vec<&SpecEvent> = log.visible().into_iter().filter(|e| e.event_type == "verdict").collect();
        assert_eq!(verdicts.len(), 1, "{verdicts:?}");
        assert_eq!(verdicts[0].fields.get("replaces"), Some(&json!(wrote["id"].as_u64().unwrap())), "{verdicts:?}");
    }

    /// O fechamento roda cada critério uma vez — os dois critérios da spec
    /// aparecem, cada um com uma execução —, grava cada execução, pede o
    /// agente de teste dedicado mesmo com uma onda só, e, aprovado ele, grava
    /// a fase fechada e deixa a pasta da spec com exatamente três arquivos. A
    /// resposta do fechamento traz a linha inteira do pull request.
    #[test]
    fn closing_runs_each_criterion_once_and_leaves_three_files_in_the_folder() {
        crate::commands::flow::round::map_after_rounds();
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version", "git --help"]);

        let asked = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() }, None);
        assert_eq!(asked["ok"], json!(true), "{asked}");
        assert_eq!(asked["phase"], json!("running"), "{asked}");
        // Os dois critérios de `proofs`, mais o que cobre a onda na cópia de
        // teste — sempre verde, para a rodada não travar a entrega.
        assert_eq!(asked["criteria"].as_array().map(Vec::len), Some(3), "os três critérios rodaram: {asked}");
        assert_eq!(asked["review"]["final"], json!(true), "a de uma onda só também pede o agente de teste: {asked}");
        // A base `dev` da spec não existe no repositório: sem código de antes,
        // a resposta avisa que as provas não rodaram nele.
        let warned = asked["warnings"].as_array().into_iter().flatten().any(|w| w["reason"] == json!("proof-before-unchecked"));
        assert!(warned, "{asked}");

        // O trecho de conserto que uma recusa deixou para o agente da onda
        // não fica na pasta da spec fechada.
        let dispatch = root.join(".claude/spec/x/.dispatch");
        std::fs::create_dir_all(&dispatch).unwrap();
        std::fs::write(dispatch.join("fix-1-9.md"), "Onda 1, rodada de conserto 1 de 2:").unwrap();
        let out = close(root, "x");
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["phase"], json!("closed"), "{out}");
        assert!(out.get("review").is_none(), "{out}");
        // A linha inteira do pull request, com a base e a branch da spec: o
        // binário a aceita, e o próximo passo em palavras a traz.
        let line = "mustard-rt run pr-open --base dev --head feature/x --spec x";
        assert_eq!(out["command"], json!(line), "{out}");
        crate::commands::flow::resume::assert_parses(line);
        assert!(out["next"].as_str().unwrap_or_default().ends_with(&then_of(&out, "close.next")), "{out}");

        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let criteria: Vec<u64> = log.visible().into_iter().filter(|e| e.event_type == "criterion").map(|e| e.id).collect();
        // Os dois de `proofs`, mais o que cobre a onda na cópia de teste —
        // sempre verde, para a rodada não travar a entrega.
        assert_eq!(criteria.len(), 3, "a montagem tem dois critérios e o que cobre a onda");
        let runs: Vec<&SpecEvent> = log.visible().into_iter().filter(|e| e.event_type == "criterion_run").collect();
        let ran: Vec<u64> = runs.iter().filter_map(|e| e.int("criterion")).collect();
        assert_eq!(ran, criteria, "cada critério roda, e uma vez só");
        assert!(runs.iter().all(|e| e.str_field("result") == Some("pass")), "{runs:?}");
        assert_eq!(State::from_log(&log).phase, Some("closed"));

        // A pasta da spec fechada guarda o estado, e mais nada: o retrato do
        // mapa da base mora fora do projeto, e o fechamento o apaga. Ele
        // chegou a 89 MB dentro da pasta de uma spec numa máquina de verdade.
        let folder = root.join(".claude").join("spec").join("x");
        let mut names: Vec<String> = std::fs::read_dir(&folder).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        names.sort();
        assert_eq!(names, ["spec.ndjson"], "state only, no automatic copy and no validation baseline");
        assert!(!mustard_core::io::wave_prompt::validation_base_map(root, "x").exists(), "o fechamento apagou o retrato do mapa da base");
        assert!(!root.join(".claude/spec/project.html").exists(), "nem a página do projeto");
    }

    /// A suíte do projeto roda uma vez só no fechamento: na máquina do
    /// fechamento, antes de pedir o agente de teste dedicado. A chamada que
    /// fecha de fato, depois do veredito aprovado, não a repete — nem num
    /// repositório que constrói o próprio `mustard-rt` —, e nada fala de
    /// binário reinstalado: o fechamento não troca programa nenhum.
    #[test]
    fn closing_does_not_repeat_the_suite_after_the_final_approval() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        std::fs::create_dir_all(root.join("apps/rt")).unwrap();
        std::fs::write(root.join("apps/rt/Cargo.toml"), b"[package]\nname=\"mustard-rt\"\n").unwrap();
        std::fs::write(root.join(".gitignore"), "suites-rodadas\n").unwrap();
        // A suíte roda na cópia limpa, que sai com a obra: o registro das
        // rodadas mora na pasta do projeto, por caminho absoluto. O comando
        // vai a um shell POSIX também no Windows, onde a barra invertida
        // escapa o caractere seguinte e `D:\tmp\x` viraria `D:tmpx`, um
        // arquivo na cópia limpa: o caminho vai com barras normais e entre
        // aspas simples, que também guardam um espaço no nome da pasta.
        let ledger = root.join("suites-rodadas");
        let suite = format!("echo rodou >> '{}'", mustard_core::io::wave_prompt::shown(&ledger));
        std::fs::write(root.join("mustard.json"), json!({ "testCommand": suite }).to_string()).unwrap();
        let runs = || std::fs::read_to_string(&ledger).unwrap_or_default().lines().count();

        let asked = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() }, None);
        assert_eq!(asked["ok"], json!(true), "{asked}");
        assert_eq!(asked["phase"], json!("running"), "{asked}");
        assert_eq!(runs(), 1, "a máquina do fechamento roda a suíte antes de pedir o agente de teste: {asked}");

        let out = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: approve(root, "x"), ..Default::default() }, None);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["phase"], json!("closed"), "{out}");
        assert_eq!(runs(), 1, "aprovada a obra, o fechamento não repete a suíte: {out}");
        let warnings = out["warnings"].as_array().cloned().unwrap_or_default();
        assert!(
            warnings.iter().all(|w| w["reason"].as_str().is_none_or(|reason| !reason.contains("reinstalled"))),
            "o fechamento não troca o programa instalado: {out}"
        );
    }

    /// O revisor final usa a vaga da última onda, que o binário põe no
    /// commit da obra, e é ele que apaga as cópias da obra quando ela fecha.
    /// O que a onda deixou na vaga, sem revisão pedida depois dela, sai: a
    /// vaga é zerada. A cópia que chega com mudança depois de uma revisão — o
    /// corte que um revisor fez para ver a prova cair e não desfez — trava o
    /// fechamento seguinte, nomeando o arquivo; desfeito o corte, o
    /// fechamento volta a pedir a revisão sobre a mesma cópia.
    #[test]
    fn reviewer_copy_is_created_and_deleted_by_the_binary() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let copy = mustard_core::io::wave_prompt::final_copy_path(root, "x", &log);
        let shown = mustard_core::io::wave_prompt::shown(&copy);
        let wave_copy = mustard_core::io::wave_prompt::recorded_copy(&log, 1).expect("the wave's copy").path;
        assert_eq!(shown, wave_copy, "o revisor usa a vaga da última onda");
        assert!(copy.join(".git").is_file(), "a vaga da onda ficou depois do commit dela");
        std::fs::write(copy.join(wave_file(1)), "fn wave_leftover() {}\n").unwrap();
        std::fs::write(copy.join("sobra.txt"), "sobra").unwrap();
        let ask = |report: Option<String>| close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report, ..Default::default() }, None);
        let git_out = |dir: &Path, args: &[&str]| {
            let out = Command::new("git").args(args).current_dir(dir).output().expect("git");
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };

        let asked = ask(None);
        assert_eq!(asked["review"]["final"], json!(true), "o que a onda deixou não trava a revisão: {asked}");
        assert!(copy.join(".git").is_file(), "{asked}");
        assert_eq!(git_out(&copy, &["status", "--porcelain"]), "", "a vaga foi zerada para a revisão: {asked}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let commit = mustard_core::io::wave_prompt::final_review_commit(root, &log).expect("a obra comitou");
        assert_eq!(git_out(&copy, &["rev-parse", "HEAD"]), commit, "a cópia está no commit da obra");
        let prompt = review_prompt(&asked);
        assert!(prompt.contains(&format!("`{shown}`")) && prompt.contains(&format!("`{commit}`")), "{prompt}");

        // O revisor corta uma prova, não desfaz e reprova a obra: o
        // fechamento seguinte não pede outra revisão por cima do corte.
        std::fs::write(copy.join(wave_file(1)), "fn cortado() {}\n").unwrap();
        let rejected = json!({"final": true, "result": "rejected", "text": "Refazer a conferência."});
        let refused = ask(verdict_written(root, "x", rejected));
        assert_eq!(refused["reason"], json!("review-copy-dirty"), "{refused}");
        let hint = refused["hint"].as_str().unwrap_or_default();
        assert!(hint.contains(&wave_file(1)) && hint.contains(&shown), "a recusa nomeia a cópia e o arquivo: {hint}");
        assert!(copy.join(wave_file(1)).is_file(), "a recusa não apaga nada");

        // Desfeito o corte, o pedido volta sobre a mesma cópia.
        git_at(&copy, &["checkout", "--", &wave_file(1)]);
        let again = ask(None);
        assert_eq!(again["review"]["final"], json!(true), "{again}");
        assert_eq!(git_out(&copy, &["rev-parse", "HEAD"]), commit, "{again}");

        // Aprovada a obra, o binário apaga todas as vagas dela, com o que
        // compilaram, e a pasta das cópias da obra, e o git não as lista
        // mais. A pasta principal e o que ela compilou ficam.
        let second = mustard_core::io::wave_prompt::slot_path(root, "x", 1);
        git_at(root, &["worktree", "add", "-q", "--detach", &second.to_string_lossy()]);
        std::fs::create_dir_all(second.join("target")).unwrap();
        std::fs::write(second.join("target").join("compilado"), "x").unwrap();
        let built = main_build(root);
        let closed = ask(approve(root, "x"));
        assert_eq!(closed["phase"], json!("closed"), "{closed}");
        assert!(!copy.exists() && !second.exists(), "o fechamento apagou as vagas: {closed}");
        assert!(!copy.parent().unwrap().exists(), "o fechamento apagou a pasta das cópias da obra: {closed}");
        let listed = git_out(root, &["worktree", "list", "--porcelain"]);
        assert!(!listed.contains(&shown) && !listed.contains(&mustard_core::io::wave_prompt::shown(&second)), "{listed}");
        assert_eq!(std::fs::read_to_string(&built).unwrap(), "compilado", "a compilação principal fica");
        assert!(root.join(wave_file(1)).is_file() && root.join(".git").is_dir(), "a pasta principal fica");
    }

    /// A obra que fecha tira as cópias dela, e a resposta avisa das cópias
    /// que sobraram de OUTRA obra, cada uma com o tamanho: o fechamento é
    /// quando o usuário olha o projeto inteiro, e a sobra de uma obra antiga
    /// ficava invisível até o disco encher.
    #[test]
    fn the_close_warns_about_the_copies_another_work_left_on_disk() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        // Uma obra fechada antes desta, cuja cópia ficou no disco.
        crate::shared::spec_state::seed_event(root, "velha", "state", json!({"phase": "closed", "author": "binary"}));
        let left = mustard_core::io::wave_prompt::copies_dir(root).join("velha").join("0");
        std::fs::create_dir_all(left.join("target")).unwrap();
        std::fs::write(left.join("target").join("compilado"), "x".repeat(4096)).unwrap();

        let closed = close(root, "x");
        assert_eq!(closed["phase"], json!("closed"), "{closed}");
        let shown = mustard_core::io::wave_prompt::shown(&mustard_core::io::wave_prompt::copies_dir(root).join("velha"));
        let hints = hints_of(&closed, "copies-left");
        assert_eq!(hints.len(), 1, "{closed}");
        assert!(hints[0].contains(&shown) && hints[0].contains("run clean --apply"), "{}", hints[0]);
        assert_eq!(closed["copies_left"][0]["path"], json!(shown), "{closed}");
        assert!(closed["copies_left"][0]["bytes"].as_u64().unwrap_or_default() >= 4096, "a sobra vai medida: {closed}");
        assert!(left.exists(), "o fechamento avisa da sobra de outra obra, e não a apaga");
    }

    /// As refs em que a limpeza das cópias guarda código, no repositório
    /// principal.
    fn kept_refs(root: &Path) -> Vec<String> {
        let out = Command::new("git").args(["for-each-ref", "--format=%(refname)", "refs/mustard/kept"]).current_dir(root).output().expect("git");
        String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect()
    }

    /// O conteúdo de `file` na ref `refname` do repositório `root`.
    fn kept_content(root: &Path, refname: &str, file: &str) -> String {
        let out = Command::new("git").args(["show", &format!("{refname}:{file}")]).current_dir(root).output().expect("git");
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    /// Os textos de aviso da resposta `out` com o motivo `reason`.
    fn hints_of<'a>(out: &'a Value, reason: &str) -> Vec<&'a str> {
        out["warnings"].as_array().into_iter().flatten().filter(|w| w["reason"] == json!(reason)).filter_map(|w| w["hint"].as_str()).collect()
    }

    /// A vaga da última onda, zerada para o revisor, guarda antes o que a
    /// onda deixou nela além do commit, e a resposta nomeia a onda, a ref e o
    /// comando para trazer o código de volta: a que pede a revisão e também a
    /// que recusa logo depois (a suíte vermelha), porque a ref existe e a
    /// chamada seguinte acharia a cópia já limpa e nada diria dele.
    #[test]
    fn the_close_names_the_code_it_kept_when_it_cleans_the_review_copy() {
        for (case, test_command) in [("the review is asked", None), ("the close refuses right after", Some("false"))] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            ready_to_close(root, "x", &["git --version"]);
            if let Some(command) = test_command {
                std::fs::write(root.join("mustard.json"), json!({ "testCommand": command }).to_string()).unwrap();
            }
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            let copy = mustard_core::io::wave_prompt::final_copy_path(root, "x", &log);
            std::fs::write(copy.join("sobra.txt"), "o que a onda deixou").unwrap();

            let asked = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), ..Default::default() }, None);
            match test_command {
                None => assert_eq!(asked["review"]["final"], json!(true), "{case}: {asked}"),
                Some(_) => assert_eq!(asked["reason"], json!("suite-failed"), "{case}: {asked}"),
            }
            let refs = kept_refs(root);
            assert_eq!(refs.len(), 1, "{case}: {refs:?}");
            assert_eq!(kept_content(root, &refs[0], "sobra.txt"), "o que a onda deixou", "{case}: {refs:?}");
            let hints = hints_of(&asked, "code-kept");
            assert_eq!(hints.len(), 1, "{case}: {asked}");
            let shown = mustard_core::io::wave_prompt::shown(&copy);
            assert!(hints[0].contains(&refs[0]) && hints[0].contains("onda 1") && hints[0].contains(&shown), "{case}: {}", hints[0]);
            assert!(hints[0].contains(&format!("git cherry-pick --no-commit {}", refs[0])), "{case}: {}", hints[0]);
        }
    }

    /// Uma vaga a mais da obra, com um arquivo dentro que o git do projeto
    /// não tem: o que o fechamento apagaria sem guardar.
    fn second_slot_with_code(root: &Path) -> PathBuf {
        let second = mustard_core::io::wave_prompt::slot_path(root, "x", 1);
        git_at(root, &["worktree", "add", "-q", "--detach", &second.to_string_lossy()]);
        std::fs::write(second.join("depois.txt"), "código de última hora").unwrap();
        second
    }

    /// O fechamento guarda o código que uma vaga tem além do commit antes de
    /// apagá-la: a vaga sai, o código fica sob uma ref do repositório
    /// principal, e a resposta nomeia a ref e o comando para trazê-lo de volta.
    #[test]
    fn the_close_keeps_the_code_a_copy_holds_beyond_the_commit_before_removing_it() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let ask = |report: Option<String>| close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report, ..Default::default() }, None);
        assert_eq!(ask(None)["review"]["final"], json!(true));
        let second = second_slot_with_code(root);

        let closed = ask(approve(root, "x"));
        assert_eq!(closed["phase"], json!("closed"), "{closed}");
        assert!(!second.exists(), "a vaga saiu: {closed}");
        let refs = kept_refs(root);
        assert_eq!(refs.len(), 1, "{refs:?}");
        assert!(refs[0].starts_with("refs/mustard/kept/x/"), "{refs:?}");
        assert_eq!(kept_content(root, &refs[0], "depois.txt"), "código de última hora", "{refs:?}");
        let hints = hints_of(&closed, "code-kept");
        assert_eq!(hints.len(), 1, "{closed}");
        assert!(hints[0].contains(&format!("git cherry-pick --no-commit {}", refs[0])), "{}", hints[0]);
    }

    /// A vaga cujo código o git não deixa guardar não é apagada: fica no
    /// disco com o arquivo dentro, a resposta nomeia a vaga e o motivo, e a
    /// obra fecha do mesmo jeito. A vaga que não tem o que guardar sai.
    #[test]
    fn a_copy_whose_code_could_not_be_kept_is_not_removed_and_the_close_says_why() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let ask = |report: Option<String>| close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report, ..Default::default() }, None);
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let clean = mustard_core::io::wave_prompt::final_copy_path(root, "x", &log);
        assert_eq!(ask(None)["review"]["final"], json!(true));
        let second = second_slot_with_code(root);
        // Uma ref no lugar da pasta das refs da obra: o git não cria as de dentro.
        let head = {
            let out = Command::new("git").args(["rev-parse", "HEAD"]).current_dir(root).output().expect("git");
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        git_at(root, &["update-ref", "refs/mustard/kept/x", &head]);

        let closed = ask(approve(root, "x"));
        assert_eq!(closed["phase"], json!("closed"), "{closed}");
        assert_eq!(std::fs::read_to_string(second.join("depois.txt")).unwrap(), "código de última hora", "{closed}");
        assert!(!clean.exists(), "a vaga sem código a guardar saiu: {closed}");
        let hints = hints_of(&closed, "code-not-kept");
        assert_eq!(hints.len(), 1, "{closed}");
        let shown = mustard_core::io::wave_prompt::shown(&second);
        assert!(hints[0].contains(&shown) && hints[0].contains("nada delas foi apagado"), "{}", hints[0]);
        assert!(hints_of(&closed, "code-kept").is_empty(), "nada foi guardado: {closed}");
    }

    /// O que a pasta principal compilou: um arquivo em `target`, que o git
    /// ignora. Devolve o caminho do arquivo.
    fn main_build(root: &Path) -> PathBuf {
        let exclude = root.join(".git").join("info").join("exclude");
        std::fs::create_dir_all(exclude.parent().unwrap()).unwrap();
        std::fs::write(&exclude, "target/\n").unwrap();
        let built = root.join("target").join("debug").join("mustard");
        std::fs::create_dir_all(built.parent().unwrap()).unwrap();
        std::fs::write(&built, "compilado").unwrap();
        built
    }

    /// A cópia da obra que não sai do disco vira aviso, nomeando a cópia, e
    /// o fechamento segue: a obra fecha assim mesmo, e a pasta principal e o
    /// que ela compilou ficam.
    #[cfg(unix)]
    #[test]
    fn a_copy_that_does_not_leave_becomes_a_warning_and_the_close_goes_on() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let copy = mustard_core::io::wave_prompt::final_copy_path(root, "x", &log);
        let ask = |report: Option<String>| close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report, ..Default::default() }, None);
        let asked = ask(None);
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        let built = main_build(root);

        // A pasta das cópias da obra sem escrita: a vaga não sai dela.
        let folder = copy.parent().unwrap().to_path_buf();
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o555)).unwrap();
        let closed = ask(approve(root, "x"));
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();

        assert_eq!(closed["ok"], json!(true), "{closed}");
        assert_eq!(closed["phase"], json!("closed"), "{closed}");
        let warnings = closed["warnings"].as_array().cloned().unwrap_or_default();
        let kept: Vec<&str> = warnings.iter().filter(|w| w["reason"] == json!("copies-kept")).filter_map(|w| w["hint"].as_str()).collect();
        assert_eq!(kept.len(), 1, "{closed}");
        assert!(kept[0].contains(&mustard_core::io::wave_prompt::shown(&copy)), "o aviso nomeia a cópia: {}", kept[0]);
        assert!(copy.exists(), "a cópia ficou no disco");
        assert_eq!(std::fs::read_to_string(&built).unwrap(), "compilado", "a compilação principal fica");
        assert!(root.join(wave_file(1)).is_file() && root.join(".git").is_dir(), "a pasta principal fica");
    }

    /// Uma pasta que o git do projeto ignora, com um arquivo dentro. Devolve o
    /// arquivo.
    fn ignored_folder(root: &Path, name: &str) -> PathBuf {
        let exclude = root.join(".git").join("info").join("exclude");
        std::fs::create_dir_all(exclude.parent().unwrap()).unwrap();
        let rules = std::fs::read_to_string(&exclude).unwrap_or_default();
        std::fs::write(&exclude, format!("{rules}{name}/\n")).unwrap();
        let file = root.join(name).join("dentro");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "x").unwrap();
        file
    }

    /// O fechamento apaga da pasta principal a pasta de compilação que o
    /// projeto declarou, só quando fecha — o pedido da revisão não apaga — e
    /// a resposta diz o que saiu. O resto fica: o código, o `.git` e a pasta
    /// ignorada que ninguém declarou. A pasta que não sai do disco vira aviso,
    /// e a obra fecha assim mesmo.
    #[test]
    fn closing_retains_small_declared_build_outputs_and_keeps_the_rest() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let built = main_build(root);
        let other = ignored_folder(root, "outra");
        // A pasta com uma pasta sem escrita dentro não sai do disco.
        #[cfg(unix)]
        let stuck = {
            use std::os::unix::fs::PermissionsExt;
            let file = ignored_folder(root, "presa");
            let inner = root.join("presa").join("trava");
            std::fs::create_dir_all(&inner).unwrap();
            std::fs::write(inner.join("arquivo"), "x").unwrap();
            std::fs::set_permissions(&inner, std::fs::Permissions::from_mode(0o555)).unwrap();
            (file, inner)
        };
        let declared = if cfg!(unix) { vec!["target", "presa"] } else { vec!["target"] };
        std::fs::write(root.join("mustard.json"), json!({"buildOutput": declared}).to_string()).unwrap();

        let asked = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), ..Default::default() }, None);
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        assert!(built.is_file(), "the review request removes nothing: {asked}");

        let closed = close(root, "x");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&stuck.1, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert_eq!(closed["ok"], json!(true), "{closed}");
        assert_eq!(closed["phase"], json!("closed"), "{closed}");
        assert!(closed.get("build_output_removed").is_none(), "{closed}");
        assert!(built.is_file(), "the small build is retained: {closed}");
        assert!(other.is_file(), "an ignored folder nobody declared stays");
        assert!(root.join(wave_file(1)).is_file() && root.join(".git").is_dir(), "the main folder stays");
        let kept: Vec<String> = closed["warnings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|w| w["reason"] == json!("build-output-kept"))
            .filter_map(|w| w["hint"].as_str().map(str::to_string))
            .collect();
        #[cfg(unix)]
        {
            assert_eq!(kept.len(), 2, "{closed}");
            assert!(kept.iter().any(|hint| hint.contains("`presa`")), "{kept:?}");
            assert!(stuck.0.is_file() || root.join("presa").exists(), "the folder that failed stays on disk");
        }
        #[cfg(not(unix))]
        assert_eq!(kept.len(), 1, "{closed}");
    }

    /// O fechamento procura processo preso só com a trava das cópias presa.
    /// Outra rodada, no meio do despacho, segura a trava e roda o git
    /// numa vaga viva cujo envio ainda não gravou: o fechamento espera a
    /// trava, e quando procura o envio daquela vaga já está gravado; o git da
    /// outra rodada segue vivo. Sem relógio: o teste segue quando a lista de
    /// travas do sistema mostra o fechamento parado na trava.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_close_never_ends_the_git_of_a_round_preparing_a_slot() {
        use crate::commands::git_settle::{copies_lock, waiting_for_lock};
        use mustard_core::io::wave_prompt::{shown, slot_path};
        use std::os::unix::fs::MetadataExt;
        use std::process::{Command, Stdio};

        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let slot = slot_path(root, "x", 1);
        std::fs::create_dir_all(slot.join("src")).unwrap();
        std::fs::write(slot.join(".git"), "gitdir: /nowhere\n").unwrap();

        let held = copies_lock(root).unwrap();
        let inode = std::fs::metadata(root.join(".claude").join("spec").join("round-copies.lock")).unwrap().ino();
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
            let closing = scope.spawn(|| close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), ..Default::default() }, None));
            while !waiting_for_lock(inode) {
                assert!(!closing.is_finished(), "the close ran without waiting for the copies lock");
                std::thread::yield_now();
            }
            // A outra rodada termina: grava o envio com a vaga que preparou e
            // solta a trava.
            crate::shared::spec_state::seed_event(
                root,
                "x",
                "send",
                json!({"role": "review", "text": "revise", "lines": 1, "chars": 6, "mustard": "0", "author": "binary",
                    "copy": shown(&slot)}),
            );
            drop(held);
            closing.join().unwrap()
        });

        let alive = preparing.try_wait().ok().flatten().is_none();
        let _ = preparing.kill();
        let _ = preparing.wait();
        assert!(alive, "the other round's git in its slot must stay alive: {out}");
        let warned = out["warnings"].as_array().cloned().unwrap_or_default();
        assert!(!warned.iter().any(|w| w["reason"] == json!("stuck-found")), "{out}");
    }

    /// A cópia do revisor final recebe os arquivos da lista de arquivos
    /// locais do projeto como a cópia da onda: no mesmo caminho, como arquivo
    /// comum, com o mesmo conteúdo, nunca como link; o que falta no principal
    /// vira aviso no pedido do revisor, e o pedido sai assim mesmo. O arquivo
    /// copiado, que o git ignora, não suja a cópia: depois de uma reprovação,
    /// o fechamento seguinte pede a revisão de novo sobre ela, que recebe os
    /// arquivos com o conteúdo de agora.
    #[test]
    fn the_final_review_copy_gets_the_local_files_too() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let exclude = root.join(".git").join("info").join("exclude");
        std::fs::create_dir_all(exclude.parent().unwrap()).unwrap();
        std::fs::write(&exclude, ".env\n").unwrap();
        std::fs::write(root.join(".env"), "SEGREDO=1\n").unwrap();
        std::fs::write(root.join("mustard.json"), br#"{"localFiles":[".env","falta.env"]}"#).unwrap();
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let copy = mustard_core::io::wave_prompt::final_copy_path(root, "x", &log);
        let shown = mustard_core::io::wave_prompt::shown(&copy);
        let ask = |report: Option<String>| close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report, ..Default::default() }, None);
        let regular = |file: &Path| std::fs::symlink_metadata(file).is_ok_and(|m| m.file_type().is_file());

        let asked = ask(None);
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        assert!(regular(&copy.join(".env")), "o arquivo local chega como arquivo comum: {asked}");
        assert_eq!(std::fs::read_to_string(copy.join(".env")).unwrap(), "SEGREDO=1\n");
        std::fs::write(copy.join(".env"), "MUDOU=1\n").unwrap();
        assert_eq!(std::fs::read_to_string(root.join(".env")).unwrap(), "SEGREDO=1\n", "a cópia não escreve no principal");
        let warnings = asked["warnings"].as_array().cloned().unwrap_or_default();
        let missing: Vec<&str> = warnings.iter().filter(|w| w["reason"] == json!("local-file-missing")).filter_map(|w| w["hint"].as_str()).collect();
        assert_eq!(missing.len(), 1, "{asked}");
        assert!(missing[0].contains("`falta.env`") && missing[0].contains(&shown), "{}", missing[0]);

        std::fs::remove_file(copy.join(".env")).unwrap();
        std::fs::write(root.join(".env"), "SEGREDO=2\n").unwrap();
        let rejected = json!({"final": true, "result": "rejected", "text": "Refazer a conferência."});
        let again = ask(verdict_written(root, "x", rejected));
        assert_eq!(again["review"]["final"], json!(true), "o arquivo local não suja a cópia: {again}");
        assert!(regular(&copy.join(".env")), "{again}");
        assert_eq!(std::fs::read_to_string(copy.join(".env")).unwrap(), "SEGREDO=2\n", "{again}");
    }

    /// O roteiro que os dois comandos do servidor rodam no teste: grava, num
    /// arquivo com o nome do papel, a casa, se o git tem identidade global,
    /// as variáveis do Claude Code e os processos pais, e sai com o código
    /// pedido.
    const PROBE: &str = r#"out="$PWD/ran-$1"
{
  echo "home=$HOME"
  if git config --global user.name >/dev/null 2>&1; then echo identity=yes; else echo identity=no; fi
  echo "claude=${CLAUDECODE:-}${CLAUDE_PROJECT_DIR:-}"
  pid=$$; chain=""
  while [ -n "$pid" ] && [ "$pid" -gt 1 ]; do
    chain="$chain $pid"
    pid=$(sed 's/.*) //' "/proc/$pid/stat" 2>/dev/null | cut -d' ' -f2)
  done
  echo "chain=$chain "
} > "$out"
exit "${2:-0}"
"#;

    #[cfg(unix)]
    #[test]
    fn closing_disables_recognized_eslint_fixes_and_keeps_lint_failure() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let eslint = root.join("eslint");
        std::fs::write(&eslint, "#!/bin/sh\nfor arg in \"$@\"; do if [ \"$arg\" = --no-fix ]; then echo 'lint error'; exit 1; fi; done\nprintf changed > source.txt\n").unwrap();
        std::fs::set_permissions(&eslint, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(root.join("source.txt"), "original").unwrap();
        std::fs::write(root.join("mustard.json"), json!({"lintCommand":"./eslint . --fix", "testCommand":"git --version"}).to_string()).unwrap();
        let out = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), ..Default::default() }, None);
        assert_eq!(out["reason"], json!("lint-failed"), "{out}");
        assert!(out["hint"].as_str().unwrap_or_default().contains("--no-fix"), "{out}");
        assert_eq!(std::fs::read_to_string(root.join("source.txt")).unwrap(), "original");
    }

    /// O lint que só acusa arquivo que a obra não tocou e que a base já
    /// acusava no primeiro despacho de onda não trava o fechamento: ele vira
    /// aviso, com os arquivos e com o que o comando disse. O mesmo lint
    /// acusando um arquivo da obra segue recusando. Sem a marca do primeiro
    /// despacho, o fechamento cobra tudo, como antes dela.
    ///
    /// Em campo (21/10/2026) o fechamento caiu num arquivo de outro time,
    /// mudado na base seis dias antes, que a obra não toca; o projeto do
    /// usuário já tinha cinco exceções escritas para contornar isso.
    #[cfg(unix)]
    #[test]
    fn the_close_does_not_block_on_a_lint_error_the_base_already_had() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        // O arquivo do outro time: o projeto o tem, e a obra não o toca.
        std::fs::write(root.join("src").join("outro.rs"), "fn outro() {}\n").unwrap();
        let lint = |named: &str| {
            let script = root.join("lint");
            std::fs::write(&script, format!("#!/bin/sh\necho '{named}:1:1 erro de formatação'\nexit 1\n")).unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
            commit_file(root, "lint");
            std::fs::write(root.join("mustard.json"), json!({"lintCommand":"./lint", "testCommand":"git --version"}).to_string()).unwrap();
            "./lint"
        };
        let closing = || close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), ..Default::default() }, None);
        let hints = |out: &Value, reason: &str| hints_of(out, reason).into_iter().map(str::to_string).collect::<Vec<_>>();

        // Sem a marca do primeiro despacho, o lint que cai trava o fechamento.
        lint("src/outro.rs");
        let strict = closing();
        assert_eq!(strict["reason"], json!("lint-failed"), "sem a marca da base, cobra tudo: {strict}");

        // A marca da base fica gravada com esse mesmo erro.
        assert_eq!(crate::commands::flow::validation::record_base_check(root, "x"), None, "a conferência da base ficou gravada");

        // O lint que acusa um arquivo da obra recusa, marca ou não.
        lint(&wave_file(1));
        let mine = closing();
        assert_eq!(mine["reason"], json!("lint-failed"), "o erro no arquivo da obra recusa: {mine}");

        // O mesmo erro do começo, agora reconhecido como da base, não trava.
        lint("src/outro.rs");
        let out = closing();
        assert_eq!(out["ok"], json!(true), "{out}");
        let said = hints(&out, "lint-from-the-base");
        assert_eq!(said.len(), 1, "{out}");
        assert!(said[0].contains("`src/outro.rs`"), "o aviso nomeia o arquivo da base: {}", said[0]);
        assert!(said[0].contains("erro de formatação"), "e repassa o que o comando disse: {}", said[0]);
        assert!(hints(&out, "lint-failed").is_empty(), "{out}");
    }

    /// Depois de um fechamento recusado, com todas as ondas entregues e o
    /// backlog vazio, a retomada diz o motivo e as saídas, e o comando do
    /// próximo passo é o do fechamento, nunca o da rodada: uma rodada ali
    /// sairia sem nada para enviar, e o motivo do fechamento não chegaria a
    /// ninguém.
    ///
    /// Em campo (21/10/2026) as duas tarefas estavam entregues e aprovadas, o
    /// fechamento tinha caído no lint, e a retomada mandou rodar a próxima
    /// rodada de ondas.
    #[cfg(unix)]
    #[test]
    fn the_resume_after_a_refused_close_says_the_reason_instead_of_the_round() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let script = root.join("lint");
        std::fs::write(&script, format!("#!/bin/sh\necho '{}:1:1 erro'\nexit 1\n", wave_file(1))).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        commit_file(root, "lint");
        std::fs::write(root.join("mustard.json"), json!({"lintCommand":"./lint", "testCommand":"git --version"}).to_string()).unwrap();

        let refused = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), ..Default::default() }, None);
        assert_eq!(refused["reason"], json!("lint-failed"), "{refused}");

        let resumed = crate::commands::flow::resume::resume_for(
            &crate::commands::flow::resume::ResumeOpts { root: root.to_path_buf(), spec: Some("x".into()) },
            None,
        );
        let said = resumed["close_refused"].as_str().unwrap_or_default();
        assert!(said.contains("lint-failed"), "a retomada diz o motivo: {resumed}");
        assert_eq!(resumed["next"], json!(said), "e é isso que ela manda fazer: {resumed}");
        assert_eq!(resumed["command"], json!("mustard-rt run close --spec x"), "{resumed}");

        // Com o lint verde, a retomada volta a falar só do fechamento.
        std::fs::write(root.join("mustard.json"), json!({"lintCommand":"git --version", "testCommand":"git --version"}).to_string()).unwrap();
        let again = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), ..Default::default() }, None);
        assert_eq!(again["review"]["final"], json!(true), "{again}");
        let after = crate::commands::flow::resume::resume_for(
            &crate::commands::flow::resume::ResumeOpts { root: root.to_path_buf(), spec: Some("x".into()) },
            None,
        );
        assert!(after.get("close_refused").is_none(), "{after}");
    }

    /// O que o roteiro gravou para o papel `role`, uma chave por linha.
    fn probed(root: &Path, role: &str) -> std::collections::BTreeMap<String, String> {
        let text = std::fs::read_to_string(check_copy(root).join(format!("ran-{role}"))).unwrap_or_default();
        text.lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    /// O fechamento roda o lint e a suíte que o `mustard.json` declara — o
    /// lugar só onde o projeto diz o que o servidor roda —, cada um em
    /// ambiente limpo: casa nova e vazia, que some depois, sem a identidade
    /// global do git, sem as variáveis do Claude Code e, no Linux, fora da
    /// árvore de processos de quem chamou. O lint que falha recusa, e a suíte
    /// roda assim mesmo; a suíte que falha recusa também, e os critérios rodam
    /// assim mesmo; os dois verdes deixam a máquina seguir para o revisor.
    #[test]
    fn closing_runs_the_server_commands_in_a_clean_environment() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        std::fs::write(root.join("probe.sh"), PROBE).unwrap();
        commit_file(root, "probe.sh");
        let exclude = root.join(".git/info/exclude");
        let mut ignores = std::fs::read_to_string(&exclude).unwrap_or_default();
        ignores.push_str("\nran-lint\nran-test\n");
        std::fs::write(exclude, ignores).unwrap();
        let close_with = |lint: &str, test: &str| {
            repair_machine_failure(root, "x");
            for role in ["lint", "test"] {
                let _ = std::fs::remove_file(check_copy(root).join(format!("ran-{role}")));
            }
            std::fs::write(root.join("mustard.json"), json!({ "lintCommand": lint, "testCommand": test }).to_string()).unwrap();
            close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), ..Default::default() }, None)
        };

        let refused = close_with("sh probe.sh lint 1", "sh probe.sh test");
        assert_eq!(refused["reason"], json!("lint-failed"), "{refused}");
        assert!(check_copy(root).join("ran-test").exists(), "com o lint vermelho, a suíte roda assim mesmo");

        let refused = close_with("sh probe.sh lint", "sh probe.sh test 1");
        assert_eq!(refused["reason"], json!("suite-failed"), "a suíte vermelha trava o fechamento: {refused}");
        assert!(refused["hint"].as_str().unwrap_or_default().contains("`sh probe.sh test 1`"), "{refused}");
        assert!(criterion_runs(root) > 0, "os critérios rodam com a suíte vermelha");

        let asked = close_with("sh probe.sh lint", "sh probe.sh test");
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        let real_home = std::env::var("HOME").unwrap_or_default();
        for role in ["lint", "test"] {
            let seen = probed(root, role);
            let home = seen.get("home").cloned().unwrap_or_default();
            assert!(!home.is_empty() && home != real_home, "{role} roda com casa própria: {seen:?}");
            assert!(!Path::new(&home).exists(), "a casa do {role} era de uma vez só: {seen:?}");
            assert_eq!(seen.get("identity").map(String::as_str), Some("no"), "{role} sem identidade global: {seen:?}");
            assert_eq!(seen.get("claude").map(String::as_str), Some(""), "{role} sem o Claude Code: {seen:?}");
            if cfg!(target_os = "linux") {
                let chain = seen.get("chain").cloned().unwrap_or_default();
                assert!(chain.split_whitespace().count() > 0, "{seen:?}");
                assert!(
                    !chain.split_whitespace().any(|pid| pid == std::process::id().to_string()),
                    "{role} saiu da árvore de processos de quem chamou: {seen:?}"
                );
            }
        }
        let warnings = asked["warnings"].as_array().cloned().unwrap_or_default();
        assert!(warnings.iter().all(|w| w["reason"] != json!("server-command-not-declared")), "{asked}");
    }

    /// O projeto que não declara um dos dois comandos do servidor fecha do
    /// mesmo jeito: o que falta não roda, e a resposta avisa qual chave falta.
    #[test]
    fn closing_runs_the_server_commands_in_a_clean_environment_and_warns_what_is_missing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        std::fs::write(root.join("probe.sh"), PROBE).unwrap();
        commit_file(root, "probe.sh");
        let exclude = root.join(".git/info/exclude");
        let mut ignores = std::fs::read_to_string(&exclude).unwrap_or_default();
        ignores.push_str("\nran-lint\nran-test\n");
        std::fs::write(exclude, ignores).unwrap();
        std::fs::write(root.join("mustard.json"), json!({ "lintCommand": "sh probe.sh lint" }).to_string()).unwrap();
        let asked = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), ..Default::default() }, None);
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        assert!(check_copy(root).join("ran-lint").is_file(), "o que está declarado roda");
        let hints: Vec<String> = asked["warnings"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter(|w| w["reason"] == json!("server-command-not-declared"))
            .map(|w| w["hint"].as_str().unwrap_or_default().to_string())
            .collect();
        assert_eq!(hints.len(), 1, "{asked}");
        assert!(hints[0].contains("`testCommand`"), "{hints:?}");
    }

    /// A suíte e o lint que o fechamento repete do servidor não usam o teto
    /// de uma prova de critério: o teto deles é de uma hora, com ou sem a
    /// variável `MUSTARD_QA_AC_TIMEOUT_SECS`, que continua valendo só para a
    /// prova. Pelo caminho de quem usa: com a variável injetada em 1 segundo,
    /// a suíte que leva 2 segundos passa, e o fechamento segue até o revisor;
    /// pela porta do critério ela seria cortada e o fechamento recusaria com
    /// a suíte vermelha.
    #[test]
    fn closing_suite_does_not_use_the_criterion_cap() {
        use crate::commands::review::qa_run::{Ceiling, ceiling_secs, with_timeout_variable};
        let hour = 60 * 60;
        for command in ["pnpm test", "pnpm lint"] {
            assert_eq!(ceiling_secs(Ceiling::ServerCommand, command, None, &[]), hour, "{command}");
            assert_eq!(ceiling_secs(Ceiling::ServerCommand, command, Some("1"), &[]), hour, "{command}");
            let declared = [command.to_string()];
            assert_eq!(ceiling_secs(Ceiling::ServerCommand, command, None, &declared), hour, "{command}");
        }
        assert_eq!(ceiling_secs(Ceiling::Criterion, "pnpm test", None, &[]), 120);
        assert_eq!(ceiling_secs(Ceiling::Criterion, "pnpm test", Some("1"), &[]), 1);

        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        std::fs::write(root.join("mustard.json"), json!({ "lintCommand": "sleep 2", "testCommand": "sleep 2" }).to_string()).unwrap();
        let asked = with_timeout_variable("1", || close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), ..Default::default() }, None));
        assert_ne!(asked["reason"], json!("lint-failed"), "o lint não é cortado pelo teto do critério: {asked}");
        assert_ne!(asked["reason"], json!("suite-failed"), "a suíte não é cortada pelo teto do critério: {asked}");
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
    }

    /// A pendência aberta que nasceu na spec fechada aparece na resposta do
    /// fechamento, com a pergunta de destino — a mesma linha que a gravação
    /// da pendência devolveria; é aviso, não recusa: o fechamento segue
    /// mesmo sem ela ser citada. Sem pendência nascida na spec, a resposta
    /// não traz o campo — o caso de antes de a pendência nascer.
    #[test]
    fn closing_lists_the_pending_items_born_in_the_spec_with_the_destination_question() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);

        // Antes de qualquer pendência nascer na spec: nenhum campo `pending`.
        let asked = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() }, None);
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        assert!(asked.get("pending").is_none(), "{asked}");

        // Uma pendência nasce e fica ligada à spec pelo registro `deferred`.
        let added = crate::commands::event::pending::pending_at(&crate::commands::event::pending::PendingOpts {
            root: root.to_path_buf(),
            add: true,
            title: Some("Medir o antivírus".into()),
            detail: Some("achado durante a spec x".into()),
            ..Default::default()
        });
        assert_eq!(added["ok"], json!(true), "{added}");
        // O `deferred` liga a pendência pela porta do binário, direto — a
        // mesma que a gravação da pendência usa; o `run write` da CLI recusa
        // o autor `binary`, que é só de gravações de dentro do binário.
        let mut draft = Map::new();
        draft.insert("text".to_string(), json!("pedido"));
        draft.insert("keys".to_string(), json!(["pedido"]));
        draft.insert("pending".to_string(), json!(1));
        draft.insert("author".to_string(), json!("binary"));
        record(root, "x", "deferred", draft, PhaseWriter::Binary).expect("deferred recorded");

        let out = close(root, "x");
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["phase"], json!("closed"), "{out}");
        let question = crate::commands::event::pending::destination_question("P-1", "Medir o antivírus", "x", Locale::PtBr);
        assert_eq!(
            out["pending"],
            json!([{"id": "P-1", "title": "Medir o antivírus", "question": question,
                    "command": "mustard-rt run close --spec x --pending-later \"P-1=…\""}]),
            "{out}",
        );
    }

    /// A pendência que nasceu na obra e que o usuário, no fechamento, mandou
    /// ficar para depois deixa de pertencer à obra e passa a ser pendência do
    /// projeto, sem dono de obra: a leitura da obra não a traz mais, a do
    /// projeto passa a trazer, e a lista guarda a obra em que ela nasceu, o
    /// dono novo e o motivo que ele deu. A que ele não citou continua da obra.
    #[test]
    fn a_pending_left_for_later_becomes_a_project_one() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        // O checkout já está na branch de trabalho da obra: é dela que a
        // pendência nasce, e é por ela que a gravação sabe a obra dona.

        // Duas pendências nascidas na obra `x`: a própria gravação as liga a
        // ela e devolve a pergunta de destino.
        for title in ["Medir o antivírus", "Trocar o relógio"] {
            let added = crate::commands::event::pending::pending_at(&crate::commands::event::pending::PendingOpts {
                root: root.to_path_buf(),
                add: true,
                title: Some(title.into()),
                detail: Some("achado durante a spec x".into()),
                ..Default::default()
            });
            assert_eq!(added["ok"], json!(true), "{added}");
            assert!(added["pending_question"].is_string(), "a pendência nasce ligada à obra: {added}");
        }
        let ids = |items: Vec<crate::commands::event::pending::OpenPending>| -> Vec<String> { items.into_iter().map(|item| item.id).collect() };
        assert_eq!(ids(crate::commands::event::pending::open_pending_born_in(root, "x")), ["P-1", "P-2"]);
        assert!(ids(crate::commands::event::pending::open_project_pending(root)).is_empty(), "nenhuma é do projeto enquanto a obra corre");

        // O fechamento pede o agente de teste dedicado e faz as perguntas; o
        // usuário responde "fica para depois" só à primeira, com o motivo.
        let asked = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() }, None);
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        let out = close_for(
            &CloseOpts {
                root: root.to_path_buf(),
                spec: Some("x".into()),
                report: approve(root, "x"),
                pending_later: vec!["P-1=o antivírus é de outra obra".into()],
            },
            None,
        );
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["phase"], json!("closed"), "{out}");
        assert_eq!(
            out["pending_released"],
            json!([{"id": "P-1", "title": "Medir o antivírus", "owner": "project",
                    "later": "o antivírus é de outra obra"}]),
            "{out}",
        );

        // A leitura da obra fica só com a que ele não soltou, e a do projeto
        // passa a trazer a solta.
        assert_eq!(ids(crate::commands::event::pending::open_pending_born_in(root, "x")), ["P-2"]);
        assert_eq!(ids(crate::commands::event::pending::open_project_pending(root)), ["P-1"]);
        // E a pergunta do fechamento já sai sem ela.
        assert_eq!(
            out["pending"],
            json!([{
                "id": "P-2",
                "title": "Trocar o relógio",
                "question": crate::commands::event::pending::destination_question(
                    "P-2", "Trocar o relógio", "x", Locale::PtBr),
                "command": "mustard-rt run close --spec x --pending-later \"P-2=…\"",
            }]),
            "{out}",
        );

        // A lista guarda a obra em que ela nasceu, o dono novo e o motivo; a
        // outra continua sem dono novo nenhum.
        let ledger: Value = serde_json::from_str(&std::fs::read_to_string(root.join(".claude/pending/ledger.json")).expect("ledger")).expect("ledger json");
        let item = |id: &str| -> Value { ledger["items"].as_array().expect("items").iter().find(|item| item["id"] == json!(id)).cloned().expect("item") };
        assert_eq!(item("P-1")["owner"], json!("project"), "{ledger}");
        assert_eq!(item("P-1")["born"], json!("x"), "{ledger}");
        assert_eq!(item("P-1")["later"], json!("o antivírus é de outra obra"), "{ledger}");
        assert_eq!(item("P-1")["status"], json!("open"), "solta não é fechada: {ledger}");
        assert!(item("P-2").get("owner").is_none(), "a que ele não soltou continua da obra: {ledger}");

        // A obra já fechada ainda grava a resposta: é depois de ler a
        // pergunta que o usuário responde, e o fechamento não pergunta duas
        // vezes.
        let late = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, pending_later: vec!["P-2".into()] }, None);
        assert_eq!(late["ok"], json!(true), "{late}");
        assert_eq!(late["pending_released"], json!([{"id": "P-2", "title": "Trocar o relógio", "owner": "project"}]));
        assert!(ids(crate::commands::event::pending::open_pending_born_in(root, "x")).is_empty());
        assert_eq!(ids(crate::commands::event::pending::open_project_pending(root)), ["P-1", "P-2"]);

        // Sem resposta nenhuma, a obra fechada segue recusando como sempre: a
        // porta não abriu, só a resposta passa por ela.
        let again = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() }, None);
        assert_eq!(again["ok"], json!(false), "{again}");
        assert_eq!(again["reason"], json!("close-not-running"), "{again}");
    }

    /// O item combinado sem dono que nenhum envio de onda levou fica fora do
    /// código para sempre, sem que nada avise; o fechamento agora avisa,
    /// pelo código e pelo título, sem recusar — e o segue fechando. O item
    /// sem dono que a onda um levou, pela escolha gravada no envio dela, não
    /// aparece no aviso.
    #[test]
    fn an_unowned_item_no_send_carried_is_reported_at_close() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        crate::shared::test_fixture::seeded_repo_on(root, "feature/x", &[("mustard.json", "{}"), (&wave_file(1), "fn one() {}\n")]);

        assert_eq!(record_open(root, "x", "feature/x", "dev"), Ok(true));
        let said = id_of(&write(root, "x", "message", json!({"author": "user", "text": "o objetivo"})));
        let crit = id_of(&write(
            root,
            "x",
            "criterion",
            json!({"when": "a onda roda e prova com git --version",
            "then": "a suíte passa", "proof": "git --version", "form": "ubiquitous", "origin": said}),
        ));
        // O código de cada decisão sai da ordem em que ela nasce na spec: a
        // primeira decisão gravada ganha o código de número um, a segunda o
        // de número dois. A primeira se liga à tarefa pela palavra-chave e é
        // candidata da onda um; a segunda não serve a onda nenhuma.
        id_of(&write(root, "x", "decision", json!({"text": "Sem dono, a onda um leva.", "keys": ["tarefa"], "why": "w", "origin": said})));
        id_of(&write(root, "x", "decision", json!({"text": "Sem dono, nenhuma onda leva.", "keys": ["k"], "why": "w", "origin": said})));
        write(
            root,
            "x",
            "wave",
            json!({"n": 1, "text": "Onda 1.", "criteria": [crit],
            "done_when": "A suíte passa.", "origin": said}),
        );
        write(
            root,
            "x",
            "task",
            json!({"wave": 1, "text": "Tarefa da onda 1.",
            "files": [{"path": wave_file(1)}], "origin": said}),
        );
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join("x"));
        crate::commands::flow::round::copies_leave_with_the_test(root);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":4}"#).unwrap();

        let round = |report: Option<String>| round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".to_string()), report }, None);
        // O Jev põe na onda só a primeira decisão sem dono.
        let first = crate::shared::spec_state::DiskSpecState::new(root)
            .log("x")
            .expect("the spec")
            .codes()
            .into_iter()
            .find(|(_, code)| code == "MSTD-DEC-0001")
            .map(|(id, _)| id)
            .expect("the first decision");
        let _jev = crate::commands::flow::round::item_choice::fake::answering(move |board| {
            board.items.iter().map(|item| (item.id, if item.id == first { 0.9 } else { 0.0 })).collect()
        });
        let dispatched = round(None);
        assert_eq!(dispatched["ok"], json!(true), "{dispatched}");
        assert!(dispatched["dispatch"].as_array().is_some_and(|sent| !sent.is_empty()), "{dispatched}");

        std::fs::write(root.join(wave_file(1)), "fn one() { dois(); }\nfn dois() {}\n").unwrap();
        // A entrega responde pela decisão que o pedido da onda levou.
        returned(
            root,
            "x",
            json!({"wave": 1, "text": "Saiu.", "files": [wave_file(1)], "commit": "a soma sai",
            "agreed": [{"item": "MSTD-DEC-0001", "met": true}]}),
        );
        let back = round(None);
        assert_eq!(back["ok"], json!(true), "{back}");
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();

        // O fechamento pede a revisão final, que responde pelas duas
        // decisões, dona ou não de onda: a lista `agreed` leva as duas.
        let asked = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() }, None);
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        let agreed = json!([
            {"item": "MSTD-DEC-0001", "met": true},
            {"item": "MSTD-DEC-0002", "met": true},
        ]);
        let approval = json!({"final": true, "result": "approved", "text": "A obra está pronta.", "agreed": agreed});
        let out = close_for(
            &CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: verdict_written(root, "x", approval), ..Default::default() },
            None,
        );
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["phase"], json!("closed"), "{out}");

        let warnings = out["warnings"].as_array().cloned().unwrap_or_default();
        let unowned: Vec<&Value> = warnings.iter().filter(|w| w["reason"] == json!("unowned-item")).collect();
        assert_eq!(unowned.len(), 1, "só a decisão que nenhuma onda levou avisa: {out}");
        let hint = unowned[0]["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("MSTD-DEC-0002"), "{hint}");
        assert!(hint.contains("Sem dono, nenhuma onda leva."), "{hint}");
        assert!(!hint.contains("MSTD-DEC-0001"), "a levada pela onda um não aparece: {hint}");
    }

    /// A aceitação do veredito final grava a tabela de rastreabilidade: uma
    /// linha por item do combinado, com o item, a verificação e o arquivo
    /// que o próprio veredito já trouxe por item, e a situação. O item
    /// atendido sem arquivo nenhum na resposta fica com o campo vazio — a
    /// tabela não inventa um.
    #[test]
    fn acceptance_writes_the_traceability_table() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        crate::shared::test_fixture::seeded_repo_on(root, "feature/x", &[("mustard.json", "{}"), (&wave_file(1), "fn one() {}\n")]);

        assert_eq!(record_open(root, "x", "feature/x", "dev"), Ok(true));
        let said = id_of(&write(root, "x", "message", json!({"author": "user", "text": "o objetivo"})));
        let crit = id_of(&write(
            root,
            "x",
            "criterion",
            json!({"when": "a onda roda e prova com git --version",
            "then": "a suíte passa", "proof": "git --version", "form": "ubiquitous", "origin": said}),
        ));
        let rule = id_of(&write(
            root,
            "x",
            "rule",
            json!({"text": "A trava confere o programa.",
            "keys": ["trava"], "example": "rm -rf pasta é barrado.", "origin": said}),
        ));
        let dec = id_of(&write(root, "x", "decision", json!({"text": "Sem prova extra.", "keys": ["k"], "why": "w", "origin": said})));
        write(
            root,
            "x",
            "wave",
            json!({"n": 1, "text": "Onda 1.", "criteria": [crit],
            "done_when": "A suíte passa.", "origin": said}),
        );
        write(
            root,
            "x",
            "task",
            json!({"wave": 1, "text": "Tarefa da onda 1.",
            "files": [{"path": wave_file(1)}], "origin": said}),
        );
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join("x"));
        crate::commands::flow::round::copies_leave_with_the_test(root);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":4}"#).unwrap();

        let round = |report: Option<String>| round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".to_string()), report }, None);
        // Com a regra e a decisão sem dono, a onda sai na mesma rodada, como está.
        let dispatched = round(None);
        assert_eq!(dispatched["ok"], json!(true), "{dispatched}");
        std::fs::write(root.join(wave_file(1)), "fn one() { dois(); }\nfn dois() {}\n").unwrap();
        returned(root, "x", json!({"wave": 1, "text": "Saiu.", "files": [wave_file(1)], "commit": "a soma sai"}));
        let back = round(None);
        assert_eq!(back["ok"], json!(true), "{back}");
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();

        let asked = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() }, None);
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        // A regra vem respondida com a verificação e dois arquivos; a
        // decisão vem atendida, mas sem texto e sem arquivo — o caso que a
        // tabela não pode inventar.
        let agreed = json!([
            {"item": "MSTD-RULE-0001", "met": true, "text": "A trava barra o comando.",
                "files": ["src/gate.rs", "src/lex.rs"]},
            {"item": "MSTD-DEC-0001", "met": true},
        ]);
        let approval = json!({"final": true, "result": "approved", "text": "A obra está pronta.", "agreed": agreed});
        let out = close_for(
            &CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: verdict_written(root, "x", approval), ..Default::default() },
            None,
        );
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["phase"], json!("closed"), "{out}");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let tracking = log.visible().into_iter().find(|e| e.event_type == "tracking").expect("a tabela ficou gravada");
        let rows = tracking.fields["items"].as_array().cloned().unwrap_or_default();
        assert_eq!(rows.len(), 2, "{rows:?}");
        let of = |id: u64| rows.iter().find(|r| r["item"] == json!(id)).unwrap_or_else(|| panic!("sem linha para {id}: {rows:?}"));
        let ruled = of(rule);
        assert_eq!(ruled["verification"], json!("A trava barra o comando."), "{ruled}");
        assert_eq!(ruled["file"], json!("src/gate.rs, src/lex.rs"), "{ruled}");
        assert_eq!(ruled["met"], json!(true), "{ruled}");
        let decided = of(dec);
        assert_eq!(decided["verification"], json!(""), "sem texto, o campo fica vazio: {decided}");
        assert_eq!(decided["file"], json!(""), "sem arquivo, o campo fica vazio, não inventado: {decided}");
        assert_eq!(decided["met"], json!(true), "{decided}");
    }

    /// O fechamento com um `mustard.json` que declara o lint.
    pub(super) fn close_with_lint(root: &Path, lint: &str, report: Option<String>) -> Value {
        std::fs::write(root.join(".gitignore"), "lint-rodou/\n").unwrap();
        std::fs::write(root.join("mustard.json"), json!({ "lintCommand": lint }).to_string()).unwrap();
        close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report, ..Default::default() }, None)
    }

    /// Quantas execuções de critério a spec tem.
    fn criterion_runs(root: &Path) -> usize {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        log.visible().iter().filter(|e| e.event_type == "criterion_run").count()
    }

    /// O fechamento roda o lint do projeto inteiro. O lint que falha recusa
    /// com a saída dele, depois de os critérios rodarem. O que passa roda no
    /// projeto e, com a máquina verde, a resposta é o pedido do agente de
    /// teste dedicado — mesmo a de uma onda só —, e não fecha nem devolve o
    /// pull request; a reprovação dele volta como conserto da onda que ele
    /// aponta, e o fechamento recusa enquanto ela não sai; entregue o
    /// conserto, o fechamento pede o mesmo agente de novo, mas só para
    /// conferir o conserto; e a spec só fecha e só devolve o pull request com
    /// ele aprovado depois da última mudança, sem rodar a máquina de novo.
    ///
    /// As duas linhas do agente de teste vêm sem critério nenhum, que é como
    /// ele as devolve: ele confere o encaixe das ondas, e não critério. As
    /// duas são gravadas assim mesmo.
    #[test]
    fn closing_runs_the_project_lint_and_opens_the_pull_request_only_after_the_dedicated_test_agent() {
        let lint = "git init -q lint-rodou";
        let ran = |root: &Path| check_copy(root).join("lint-rodou").is_dir();

        // O lint que falha recusa com a saída dele, e os critérios rodam
        // assim mesmo: a recusa lista tudo o que caiu de uma vez.
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_with_waves(root, "x", &["git --version"], 2);
        let refused = close_with_lint(root, "git lint-que-nao-existe", None);
        assert_eq!(refused["reason"], json!("lint-failed"), "{refused}");
        let hint = refused["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("git lint-que-nao-existe") && hint.contains("lint-que-nao-existe' is not a git command"), "{hint}");
        assert_eq!(criterion_runs(root), 2, "os critérios rodam com o lint vermelho");

        // O lint que passa roda no projeto, e a máquina verde pede o agente
        // de teste dedicado.
        repair_machine_failure(root, "x");
        std::fs::write(root.join(".git/info/exclude"), "lint-rodou/\n").unwrap();
        let asked = close_with_lint(root, lint, None);
        assert_eq!(asked["ok"], json!(true), "{asked}");
        assert_eq!(asked["phase"], json!("running"), "{asked}");
        assert!(ran(root), "o lint rodou antes do agente");
        // O critério de `proofs`, mais o que cobre as ondas na cópia de
        // teste — sempre verde, para a rodada não travar a entrega.
        assert_eq!(asked["criteria"].as_array().map(Vec::len), Some(2), "{asked}");
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        let prompt = review_prompt(&asked);
        assert!(prompt.contains(translate("prompt.final.fixed", Locale::PtBr)), "{prompt}");
        assert!(prompt.contains("código repetido entre ondas") && prompt.contains("verificação que uma apagou da outra"), "{prompt}");
        for n in [1, 2] {
            assert!(!prompt.contains(&format!("MSTD-WAVE-000{n}")), "a onda {n} já tem prova e não vai ao pedido: {prompt}");
        }
        assert!(asked.get("command").is_none(), "sem aprovação, nada de pull request: {asked}");
        let expected = translate("close.final_review", Locale::PtBr).replace("{spec}", "x");
        assert_eq!(asked["next"], json!(expected), "{asked}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert_eq!(State::from_log(&log).phase, Some("running"), "a spec não fechou");

        // O agente reprova apontando a onda 2: a onda 2 volta como conserto,
        // e o fechamento recusa enquanto ela não sai.
        let rejected = json!({"final": true, "wave": 2, "result": "rejected", "text": "A onda 2 repete a 1."});
        let out = close_with_lint(root, lint, verdict_written(root, "x", rejected));
        assert_eq!(out["reason"], json!("wave-rejected"), "{out}");
        assert!(out["hint"].as_str().unwrap_or_default().contains('2'), "{out}");
        let round = |report: Option<String>| round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report }, None);
        let fix = round(None);
        let sent: Vec<u64> = fix["dispatch"].as_array().unwrap().iter().filter_map(|d| d["wave"].as_u64()).collect();
        assert_eq!(sent, vec![2], "{fix}");
        std::fs::write(root.join(wave_file(2)), "fn one() { tres(); }\nfn tres() {}\n").unwrap();
        let line = json!({"wave": 2, "text": "Sem repetir a 1.", "files": [wave_file(2)], "commit": "a entrega 2 sem repetição"});
        returned(root, "x", line);
        let back = round(None);
        assert_eq!(back["ok"], json!(true), "{back}");
        assert!(back.get("reviews").is_none(), "a rodada não pede revisão do conserto: {back}");

        // Depois do conserto, a máquina roda de novo, e o pedido volta —
        // agora só com o conserto, não a obra inteira de novo.
        std::fs::remove_dir_all(check_copy(root).join("lint-rodou")).unwrap();
        let again = close_with_lint(root, lint, None);
        assert_eq!(again["review"]["final"], json!(true), "{again}");
        assert!(ran(root), "{again}");
        let fix_prompt = review_prompt(&again);
        assert!(fix_prompt.contains(translate("prompt.final.look_again", Locale::PtBr)), "{fix_prompt}");
        assert!(!fix_prompt.contains("MSTD-WAVE-0001") && !fix_prompt.contains("MSTD-TASK-0001"), "só o conserto: {fix_prompt}");

        // Aprovado, a spec fecha sem rodar a máquina outra vez.
        std::fs::remove_dir_all(check_copy(root).join("lint-rodou")).unwrap();
        let before = criterion_runs(root);
        let approved = json!({"final": true, "result": "approved", "text": "O conserto ficou certo."});
        let closed = close_with_lint(root, lint, verdict_written(root, "x", approved));
        assert_eq!(closed["ok"], json!(true), "{closed}");
        assert_eq!(closed["phase"], json!("closed"), "{closed}");
        assert!(closed.get("review").is_none(), "{closed}");
        assert_eq!(closed["command"], json!("mustard-rt run pr-open --base dev --head feature/x --spec x"), "{closed}");
        assert_eq!(criterion_runs(root), before, "a volta do agente de teste não roda os critérios de novo");
        assert!(!ran(root), "nem o lint");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let last = log.visible().into_iter().rfind(|e| e.event_type == "verdict").unwrap();
        assert_eq!(
            (last.wave(), last.fields.get("final"), last.str_field("result")),
            (None, Some(&json!(true)), Some("approved")),
            "a aprovação final não aponta onda: ela responde pelo combinado inteiro, não por uma onda dele"
        );
        let finals: Vec<&SpecEvent> = log.visible().into_iter().filter(|e| e.event_type == "verdict" && e.fields.get("final") == Some(&json!(true))).collect();
        let results: Vec<Option<&str>> = finals.iter().map(|e| e.str_field("result")).collect();
        assert_eq!(results, [Some("rejected"), Some("approved")], "as duas revisões finais ficaram gravadas");
        assert!(finals.iter().all(|e| e.fields.get("criteria").is_none()), "e nenhuma delas confere critério");
    }

    /// O agente de teste dedicado fecha toda obra: a de duas ondas, que
    /// entrega as duas, e a de uma onda só, que entrega a dela. A rodada
    /// nunca pede revisão de onda nenhuma, o fechamento pede o agente
    /// dedicado com um pedido que deixa de fora as entregas, que já são
    /// prova, e quando ele
    /// aponta um problema numa onda, o conserto sai pela rodada e o
    /// fechamento pede o mesmo agente de novo, só para conferir o conserto —
    /// em até duas voltas; na terceira reprovação seguida, a onda para e a
    /// decisão passa a ser do usuário.
    #[test]
    fn the_dedicated_test_agent_closes_every_work() {
        let waves_in =
            |out: &Value, field: &str| -> Vec<u64> { out[field].as_array().cloned().unwrap_or_default().iter().filter_map(|d| d["wave"].as_u64()).collect() };

        // A obra de duas ondas: as duas entregam, sem revisão nenhuma da
        // rodada, e o fechamento pede o agente dedicado; as entregas já são
        // prova e não vão ao pedido.
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_with_waves(root, "x", &["git --version"], 2);
        let asked = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() }, None);
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        let prompt = review_prompt(&asked);
        for n in [1, 2] {
            assert!(!prompt.contains(&format!("MSTD-DELIV-000{n}")), "a entrega da onda {n} já é prova e não vai ao pedido: {prompt}");
        }

        // A obra de uma onda só entrega a dela e passa pela mesma exigência.
        let solo = tempdir().unwrap();
        let solo_root = solo.path();
        ready_to_close(solo_root, "x", &["git --version"]);
        let asked_solo = close_for(&CloseOpts { root: solo_root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() }, None);
        assert_eq!(asked_solo["review"]["final"], json!(true), "uma onda só também pede o agente: {asked_solo}");
        let solo_prompt = review_prompt(&asked_solo);
        assert!(!solo_prompt.contains("MSTD-DELIV-0001"), "a entrega da onda única já é prova: {solo_prompt}");
        let round_solo = round_for(&RoundOpts { root: solo_root.to_path_buf(), spec: Some("x".into()), report: None }, None);
        assert!(round_solo.get("reviews").is_none(), "a rodada não pede revisão da onda: {round_solo}");

        // De volta à obra de duas ondas: o agente aponta um problema na onda
        // 2. O conserto sai pela rodada, sem revisão nenhuma pedida por ela.
        let round = |report: Option<String>| round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report }, None);
        let close = |report: Option<String>| close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report, ..Default::default() }, None);
        let reject = |text: &str| {
            let body = json!({"final": true, "wave": 2, "result": "rejected", "text": text});
            verdict_written(root, "x", body)
        };
        let refused = close(reject("Falta o teste da onda 2."));
        assert_eq!(refused["reason"], json!("wave-rejected"), "{refused}");
        let fix = round(None);
        assert_eq!(waves_in(&fix, "dispatch"), vec![2], "{fix}");
        assert!(fix.get("reviews").is_none(), "{fix}");
        std::fs::write(root.join(wave_file(2)), "fn one() { tres(); }\nfn tres() {}\n").unwrap();
        let line = json!({"wave": 2, "text": "Consertou.", "files": [wave_file(2)], "commit": "conserta a entrega 2"});
        returned(root, "x", line);
        let back = round(None);
        assert_eq!(back["ok"], json!(true), "{back}");
        assert!(back.get("reviews").is_none(), "a rodada não pede revisão do conserto: {back}");

        // Entregue o conserto, o fechamento pede o mesmo agente de novo, só
        // para conferir o conserto — não a obra inteira de novo.
        let rechecked = close(None);
        assert_eq!(rechecked["review"]["final"], json!(true), "{rechecked}");
        let fix_prompt = review_prompt(&rechecked);
        assert!(fix_prompt.contains(translate("prompt.final.look_again", Locale::PtBr)), "{fix_prompt}");
        assert!(!fix_prompt.contains("MSTD-WAVE-0001") && !fix_prompt.contains("MSTD-TASK-0001"), "só o conserto: {fix_prompt}");

        // A segunda volta de conserto: reprovado de novo, o conserto sai mais
        // uma vez.
        let refused_again = close(reject("Ainda falta."));
        assert_eq!(refused_again["reason"], json!("wave-rejected"), "{refused_again}");
        let fix_again = round(None);
        assert_eq!(waves_in(&fix_again, "dispatch"), vec![2], "{fix_again}");
        std::fs::write(root.join(wave_file(2)), "fn one() { quatro(); }\nfn quatro() {}\n").unwrap();
        let line = json!({"wave": 2, "text": "Consertou de novo.", "files": [wave_file(2)], "commit": "conserta de novo"});
        returned(root, "x", line);
        assert_eq!(round(None)["ok"], json!(true));

        // A terceira reprovação seguida para a onda: a rodada deixa de
        // despachá-la, e a decisão passa a ser do usuário. O fechamento pede
        // a revisão do segundo conserto antes de o revisor reprovar.
        assert_eq!(close(None)["review"]["final"], json!(true));
        close(reject("Ainda não."));
        let stopped = round(None);
        assert_eq!(stopped["stopped"][0]["wave"], json!(2), "{stopped}");
        assert_eq!(waves_in(&stopped, "dispatch"), Vec::<u64>::new(), "a rodada para de despachar: {stopped}");
        let after = close(None);
        assert_eq!(after["reason"], json!("wave-rejected"), "o fechamento segue recusando: {after}");
    }

    /// O conserto de uma onda que não é a última do plano: o agente de teste
    /// dedicado reprova a onda 1 de duas, e a aprovação final, sem onda, é
    /// gravada sem onda nenhuma — nunca ganha um veredito próprio a onda 1.
    /// Três coisas têm de continuar certas mesmo assim: (1) o fechamento
    /// recusa enquanto o conserto está só despachado, sem entrega ainda; (2)
    /// entregue o conserto, a rodada solta a onda e manda fechar; e (3) depois
    /// da aprovação final, nenhuma onda fica com o estado de reprovada.
    #[test]
    fn a_fix_of_a_non_final_wave_releases_the_queue_and_leaves_no_wave_rejected() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_with_waves(root, "x", &["git --version"], 2);
        let round = |report: Option<String>| round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report }, None);
        let close = |report: Option<String>| close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report, ..Default::default() }, None);

        // O fechamento pede o agente de teste dedicado, que aponta a onda 1,
        // não a última.
        assert_eq!(close(None)["review"]["final"], json!(true));
        let reject = json!({"final": true, "wave": 1, "result": "rejected", "text": "Falta o teste da onda 1."});
        let refused = close(verdict_written(root, "x", reject));
        assert_eq!(refused["reason"], json!("wave-rejected"), "{refused}");

        // A rodada despacha o conserto da onda 1.
        let fix = round(None);
        assert_eq!(fix["dispatch"].as_array().unwrap().iter().filter_map(|d| d["wave"].as_u64()).collect::<Vec<_>>(), vec![1], "{fix}");

        // (1) Só despachado, sem entrega: o fechamento continua recusando —
        // o commit da entrega original da onda 1, anterior à reprovação, não
        // pode bastar.
        let still_pending = close(None);
        assert_eq!(still_pending["reason"], json!("wave-rejected"), "{still_pending}");
        assert!(still_pending["hint"].as_str().unwrap_or_default().contains('1'), "{still_pending}");

        // A onda 1 entrega o conserto, com um código diferente do que já
        // estava no disco.
        std::fs::write(root.join(wave_file(1)), "fn one() { tres(); }\nfn tres() {}\n").unwrap();
        let line = json!({"wave": 1, "text": "Sem faltar o teste.", "files": [wave_file(1)], "commit": "conserta a entrega 1"});
        returned(root, "x", line);
        let back = round(None);
        assert_eq!(back["ok"], json!(true), "{back}");

        // (2) Entregue o conserto, a fila solta a onda: a rodada não tem mais
        // nada a despachar nem a esperar, e manda fechar.
        let released = round(None);
        assert_eq!(released["command"], json!("mustard-rt run close --spec x"), "{released}");

        // O agente de teste dedicado confere só o conserto, e aprova a obra
        // inteira: a aprovação sem onda é gravada sem onda nenhuma — nem na 1,
        // que foi a reprovada, nem na 2, a última do plano.
        let fix_prompt = review_prompt(&close(None));
        assert!(fix_prompt.contains(translate("prompt.final.look_again", Locale::PtBr)) && !fix_prompt.contains("MSTD-TASK-0002"), "{fix_prompt}");
        let approved = json!({"final": true, "result": "approved", "text": "O conserto ficou certo."});
        let closed = close(verdict_written(root, "x", approved));
        assert_eq!(closed["phase"], json!("closed"), "{closed}");

        // (3) Nenhuma onda fica com o estado de reprovada, nem a 1, cujo
        // último veredito próprio continua sendo a reprovação: a aprovação
        // que fechou a obra não é dela.
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let states = crate::commands::flow::round::wave_states(&log);
        let rejected: Vec<u64> = states.into_iter().filter(|(_, s)| *s == mustard_core::view::document::WaveState::Rejected).map(|(n, _)| n).collect();
        assert_eq!(rejected, Vec::<u64>::new(), "{closed}");
    }

    /// O fechamento que pede a revisão de volta, depois de um veredito final
    /// que reprovou e do conserto que a rodada comitou: o pedido lista o que
    /// mudou desde o veredito — ele e o commit do conserto, nunca os commits
    /// de antes dele. Nem a primeira revisão nem a de volta mandam rodar a
    /// suíte inteira, que o próprio fechamento acabou de rodar: as duas dizem
    /// que ela passou no commit da cópia.
    #[test]
    fn the_review_after_a_rejection_lists_what_changed_and_never_reruns_the_suite() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_with_waves(root, "x", &["git --version"], 2);
        std::fs::write(root.join("mustard.json"), json!({"testCommand": "git --version"}).to_string()).unwrap();
        let round = |report: Option<String>| round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report }, None);
        let close = |report: Option<String>| close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report, ..Default::default() }, None);
        let since = translate("prompt.part.since_verdict", Locale::PtBr);
        let listed = |prompt: &str| -> String {
            let Some((_, rest)) = prompt.split_once(&format!("## {since}\n")) else {
                return String::new();
            };
            rest.split("\n## ").next().unwrap_or_default().to_string()
        };
        let suite_line = |prompt: &str| {
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            let commit = mustard_core::io::wave_prompt::final_review_commit(root, &log).unwrap_or_else(|| "HEAD".into());
            let said = translate("prompt.review.suite", Locale::PtBr).replace("{command}", "git --version");
            assert!(prompt.contains(&said.replace("{commit}", &commit)), "a suíte passou no commit da cópia: {prompt}");
            assert!(!prompt.contains("Teste com `git --version`"), "a revisão não roda a suíte inteira: {prompt}");
        };

        let first = close(None);
        assert_eq!(first["review"]["final"], json!(true), "{first}");
        let prompt = review_prompt(&first);
        assert!(listed(&prompt).is_empty(), "a primeira revisão confere a obra inteira: {prompt}");
        suite_line(&prompt);

        let reject = json!({"final": true, "wave": 2, "result": "rejected", "text": "A onda 2 repete a 1."});
        assert_eq!(close(verdict_written(root, "x", reject))["reason"], json!("wave-rejected"));
        assert_eq!(round(None)["ok"], json!(true));
        std::fs::write(root.join(wave_file(2)), "fn one() { tres(); }\nfn tres() {}\n").unwrap();
        returned(root, "x", json!({"wave": 2, "text": "Sem repetir a 1.", "files": [wave_file(2)], "commit": "a entrega 2 sem repetir"}));
        assert_eq!(round(None)["ok"], json!(true));

        let again = close(None);
        assert_eq!(again["review"]["final"], json!(true), "{again}");
        let prompt = review_prompt(&again);
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let verdict = log.visible().into_iter().rfind(|e| e.event_type == "verdict" && e.fields.get("final") == Some(&json!(true))).map(|e| e.id).unwrap();
        let commits: Vec<u64> = log.visible().into_iter().filter(|e| e.event_type == "commit").map(|e| e.id).collect();
        let (before, after): (Vec<u64>, Vec<u64>) = commits.into_iter().partition(|id| *id < verdict);
        assert!(!before.is_empty() && !after.is_empty(), "um commit antes do veredito e o do conserto depois: {prompt}");
        let part = listed(&prompt);
        assert!(part.contains(&codes[&verdict]), "{prompt}");
        for id in after {
            assert!(part.contains(&codes[&id]), "o commit do conserto está no que mudou: {prompt}");
        }
        for id in before {
            assert!(!part.contains(&codes[&id]), "o commit de antes do veredito fica fora do que mudou: {prompt}");
        }
        suite_line(&prompt);
    }

    /// Um veredito de onda do fluxo antigo, sem o campo `final` — como o
    /// round-review de antes gravava —, reprova a onda 2. Sem essa marca, ele
    /// não pode pôr a spec em modo de conserto: a rodada não tem conserto
    /// pendente, e o fechamento pede o agente de teste dedicado com o pedido
    /// da obra inteira, não só da onda apontada.
    #[test]
    fn an_old_wave_verdict_never_turns_the_final_test_into_a_fix() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_with_waves(root, "x", &["git --version"], 2);

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let criterion_id = log.visible().into_iter().find(|e| e.event_type == "criterion").unwrap().id;
        let mut draft = Map::new();
        draft.insert("wave".to_string(), json!(2));
        draft.insert("result".to_string(), json!("rejected"));
        draft.insert("text".to_string(), json!("faltou algo, gravado do jeito antigo"));
        draft.insert("criteria".to_string(), json!([{"criterion": criterion_id}]));
        record(root, "x", "verdict", draft, PhaseWriter::Binary).expect("veredito antigo gravado");

        // A rodada não vê conserto pendente: nada a despachar.
        let round = round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None }, None);
        assert_eq!(round["dispatch"].as_array().map(Vec::len).unwrap_or_default(), 0, "{round}");

        // O fechamento pede o agente de teste dedicado com o pedido da obra
        // inteira, sem entrar em modo de conserto.
        let asked = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() }, None);
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        let prompt = review_prompt(&asked);
        assert!(!prompt.contains(translate("prompt.final.look_again", Locale::PtBr)), "não é modo de conserto: {prompt}");
    }

    /// A resposta da rodada e a do fechamento, numa spec ainda sem páginas
    /// publicadas, mandam publicar a página da spec e a do projeto, dizem como
    /// gravar as duas publicações e mandam copiar os lotes, e nenhuma delas
    /// traz o endereço de uma página para a conversa. A do plano prova o mesmo
    /// no teste dela. Um passo comum — o levantamento, a gravação de um item —
    /// não manda publicar nem copiar.
    #[test]
    fn the_round_and_the_close_order_the_publish_and_never_carry_a_link() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let rounded = round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None }, None);
        let closed = close(root, "x");
        for report in [&rounded, &closed] {
            assert_eq!(report["ok"], true, "{report}");
            assert!(report.get("publish").is_none() && report.get("copy").is_none());
            assert_eq!(report["panel"], "/mustard:mods");
            assert!(!report["next"].as_str().unwrap().contains("ArtifactData"));
        }
        assert!(!root.join(".claude/spec/x/copy").exists());
        assert!(!root.join(".claude/mustard/publications").exists());
        let export = crate::commands::panel::prepare_publication(root, "x", false);
        assert_eq!(export["ok"], true, "{export}");
        assert_eq!(export["published"], false);
        let before = std::fs::read(export["database"].as_str().unwrap()).unwrap();
        let _ = round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None }, None);
        assert_eq!(std::fs::read(export["database"].as_str().unwrap()).unwrap(), before, "a snapshot is not a live external database");
    }

    /// O fim do `next` de cada marco, com o comando que a resposta devolve: a
    /// rodada com tudo entregue e aprovado manda fechar, e o fechamento manda
    /// abrir o pull request.
    fn then_of(report: &Value, key: &str) -> String {
        let command = report["command"].as_str().unwrap_or_else(|| panic!("sem comando: {report}"));
        translate(key, Locale::PtBr).replace("{command}", command)
    }

    /// Com um item de texto que parece senha, a rodada e o fechamento mandam
    /// publicar e copiar assim mesmo e dizem o código do item a expurgar; o
    /// item fica fora da cópia.
    #[test]
    fn a_withheld_item_is_named_and_no_longer_holds_the_publish_of_the_round_and_the_close() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let said = write(root, "x", "message", json!({"author":"user","text":"anota"}));
        let note = write(root, "x", "note", json!({"text":"GITHUB_TOKEN=a1b2c3d4e5f6g7h8i9j0","keys":["token"],"origin":id_of(&said)}));
        let code = note["code"].as_str().unwrap();
        let rounded = round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None }, None);
        let closed = close(root, "x");
        for report in [&rounded, &closed] {
            assert_eq!(report["ok"], true, "{report}");
            assert_eq!(report["withheld"], json!([code]));
            assert!(report.get("copy").is_none() && report.get("publish").is_none());
            assert!(!report.to_string().contains("a1b2c3d4"));
        }
        assert!(!root.join(".claude/mustard/publications").exists());
        let export = crate::commands::panel::prepare_publication(root, "x", false);
        let database = std::fs::read_to_string(export["database"].as_str().unwrap()).unwrap();
        assert!(!database.contains("GITHUB_TOKEN") && !database.contains("a1b2c3d4"));
    }

    /// Quando a cópia para o banco não pode ser preparada, a rodada e o
    /// fechamento não mandam publicar nem copiar: dizem nos avisos por quê,
    /// dizem que a próxima cópia leva os mesmos itens e seguem com o próximo
    /// passo.
    #[test]
    fn a_copy_that_could_not_be_prepared_is_never_ordered() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        std::fs::write(root.join(".claude/spec/x/copy"), "legacy path cannot be a directory").unwrap();
        let out = close(root, "x");
        assert_eq!(out["ok"], true, "{out}");
        assert!(out.get("copy").is_none() && out.get("publish").is_none());
        assert!(!out["next"].as_str().unwrap().contains("ArtifactData"));
        assert!(!root.join(".claude/mustard/publications").exists());
    }

    /// Ao gravar a fase fechada, o fechamento arma a cobrança das pendências
    /// pela mesma porta que grava a fase.
    #[test]
    fn closing_arms_the_charge_of_the_pending_items() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        assert_eq!(
            close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() }, Some("s-fecha"),)["review"]["final"],
            json!(true)
        );
        let closed =
            close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: approve(root, "x"), ..Default::default() }, Some("s-fecha"));
        assert_eq!(closed["ok"], json!(true), "{closed}");
        assert_eq!(closed["phase"], json!("closed"), "{closed}");
        let armed = crate::commands::event::pending::armed_charges(root);
        assert!(armed.iter().any(|charge| charge.spec == "x"), "a cobrança ficou armada: {armed:?}");
    }

    /// A onda que volta só conferindo, sem arquivo mudado, passa pela rodada
    /// sem commit e o fechamento não a cobra: os dois leem o fim da onda pela
    /// mesma leitura. A onda cuja entrega mais recente mudou arquivo e não tem
    /// commit continua recusada.
    #[test]
    fn the_wave_that_only_checked_closes_without_a_commit() {
        let commits_of = |root: &Path, wave: u64| {
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            log.visible().iter().filter(|e| e.event_type == "commit" && e.ints("waves").contains(&wave)).count()
        };

        // A onda 2 volta sem arquivo pela rodada: nenhum commit a leva, e a
        // obra fecha.
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_with_checked(root, "x", &["git --version"], 2, &[2]);
        assert_eq!((commits_of(root, 1), commits_of(root, 2)), (1, 0), "só a onda com arquivo comitou");
        let closed = close(root, "x");
        assert_eq!(closed["ok"], json!(true), "{closed}");
        assert_eq!(closed["phase"], json!("closed"), "{closed}");

        // A mesma onda com uma entrega mais recente que mudou arquivo, sem
        // commit, volta a ser cobrada.
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_with_checked(root, "x", &["git --version"], 2, &[2]);
        write(root, "x", "delivered", json!({"wave": 2, "text": "Mexi depois.", "files": [wave_file(2)]}));
        assert_eq!(commits_of(root, 2), 0);
        let refused = close(root, "x");
        assert_eq!(refused["reason"], json!("wave-without-commit"), "{refused}");
        assert!(refused["hint"].as_str().unwrap_or_default().contains('2'), "{refused}");
    }

    /// O fechamento recusa enquanto houver onda sem commit, onda cuja última
    /// revisão foi reprovada ou pedido do usuário que nenhuma onda entregou, e
    /// diz qual onda refazer.
    #[test]
    fn closing_is_refused_while_the_work_is_not_finished() {
        // Onda sem commit.
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let said = id_of(&write(root, "x", "message", json!({"author": "user", "text": "mais uma"})));
        let crit = id_of(&write(
            root,
            "x",
            "criterion",
            json!({"when": "a onda roda", "then": "passa", "proof": "git --version", "form": "ubiquitous",
                "origin": said}),
        ));
        write(
            root,
            "x",
            "wave",
            json!({"n": 2, "text": "Onda 2.", "criteria": [crit],
            "done_when": "passa", "origin": said}),
        );
        // A onda precisa de tarefa: a que o binário formou e ficou sem
        // nenhuma sai do plano e não é cobrada.
        write(
            root,
            "x",
            "task",
            json!({"wave": 2, "text": "Tarefa da onda 2.", "files": [],
            "depends_on": [], "covers": [crit], "origin": said}),
        );
        let refused = close(root, "x");
        assert_eq!(refused["reason"], json!("wave-without-commit"), "{refused}");
        assert!(refused["hint"].as_str().unwrap_or_default().contains('2'), "{refused}");

        // Onda cuja última revisão reprovou.
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let crit = log.visible().into_iter().find(|e| e.event_type == "criterion").map(|e| e.id).unwrap();
        crate::shared::spec_state::seed_verdict(root, "x", 1, "rejected", crit);
        let refused = close(root, "x");
        assert_eq!(refused["reason"], json!("wave-rejected"), "{refused}");
        assert!(refused["hint"].as_str().unwrap_or_default().contains('1'), "{refused}");

        // Pedido do usuário que nenhuma onda entregou.
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        crate::shared::spec_state::seed_request(root, "x", "Quero também a barra de status.");
        let refused = close(root, "x");
        assert_eq!(refused["reason"], json!("request-not-delivered"), "{refused}");
    }

    /// O fechamento com mais de uma coisa faltando lista todas numa resposta
    /// só: a onda sem commit e o pedido do usuário que nenhuma onda entregou.
    #[test]
    fn closing_lists_everything_missing_in_one_answer() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let said = id_of(&write(root, "x", "message", json!({"author": "user", "text": "mais uma"})));
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let crit = log.visible().into_iter().find(|e| e.event_type == "criterion").map(|e| e.id).unwrap();
        write(root, "x", "wave", json!({"n": 2, "text": "Onda 2.", "criteria": [crit], "done_when": "passa", "origin": said}));
        write(root, "x", "task", json!({"wave": 2, "text": "Tarefa da onda 2.", "covers": [crit], "origin": said}));
        crate::shared::spec_state::seed_request(root, "x", "Quero também a barra de status.");

        let refused = close(root, "x");
        assert_eq!(refused["reason"], json!("wave-without-commit"), "{refused}");
        assert_eq!(refused["reasons"], json!(["wave-without-commit", "request-not-delivered"]), "{refused}");
        let hint = refused["hint"].as_str().unwrap_or_default();
        assert!(hint.contains(&translate("close.wave_without_commit", Locale::PtBr).replace("{wave}", "2")), "{refused}");
        assert!(hint.contains("chegou depois da última entrega"), "{refused}");
    }

    /// Com a compilação verde, o fechamento roda o lint, a suíte e cada
    /// critério mesmo depois de um deles cair, e a recusa lista todas as
    /// falhas numa resposta só.
    #[test]
    fn closing_lists_every_machine_failure_in_one_answer() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --nao-existe-esta-opcao"]);
        let config = json!({"lintCommand": "echo lint-mark; exit 1", "testCommand": "echo suite-mark; exit 1"});
        std::fs::write(root.join("mustard.json"), config.to_string()).unwrap();

        let refused = close(root, "x");
        assert_eq!(refused["reason"], json!("lint-failed"), "{refused}");
        assert_eq!(refused["reasons"], json!(["lint-failed", "suite-failed", "criterion-failed"]), "{refused}");
        let hint = refused["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("lint-mark") && hint.contains("suite-mark"), "{refused}");
        assert!(hint.contains("nao-existe-esta-opcao"), "{refused}");
        assert_eq!(criterion_runs(root), 2, "every criterion ran: {refused}");
    }

    /// O fechamento com tarefa no backlog recusa com a razão própria e nomeia a
    /// tarefa; tirada a tarefa da spec, o mesmo fechamento passa.
    #[test]
    fn closing_refuses_with_a_task_in_the_backlog() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let said = id_of(&write(root, "x", "message", json!({"author": "user", "text": "mais uma"})));
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let crit = log.visible().into_iter().find(|e| e.event_type == "criterion").expect("o critério").id;
        let task = id_of(&write(
            root,
            "x",
            "task",
            json!({"text": "Tarefa que ficou no backlog.",
            "files": [{"path": "src/w1.rs"}], "depends_on": [], "covers": [crit], "origin": said}),
        ));
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let code = log.codes().get(&task).cloned().expect("a tarefa tem código");

        let refused = close(root, "x");
        assert_eq!(refused["reason"], json!("backlog-not-empty"), "{refused}");
        let expected = translate("close.backlog_not_empty", Locale::PtBr).replace("{tasks}", &code);
        assert_eq!(refused["hint"], json!(expected), "{refused}");

        write(root, "x", "remove", json!({"targets": [task], "reason": "a tarefa saiu da obra"}));
        let closed = close(root, "x");
        assert_eq!(closed["ok"], json!(true), "{closed}");
    }

    /// A onda parada pelo limite de consertos trava o fechamento enquanto está
    /// no plano, e a rodada faz a pergunta dela. Tirada do plano, com a
    /// tarefa, ela deixa de contar nos dois: a rodada manda fechar e o
    /// fechamento passa, sem cobrar dela veredito nem commit.
    #[test]
    fn a_stuck_wave_taken_out_of_the_plan_no_longer_counts_in_the_round_or_the_close() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let said = id_of(&write(root, "x", "message", json!({"author": "user", "text": "mais uma"})));
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let crit = log.visible().into_iter().find(|e| e.event_type == "criterion").map(|e| e.id).unwrap();
        let wave = id_of(&write(
            root,
            "x",
            "wave",
            json!({"n": 2, "text": "Onda 2.", "criteria": [crit],
            "done_when": "passa", "origin": said}),
        ));
        let task = id_of(&write(
            root,
            "x",
            "task",
            json!({"wave": 2, "text": "Tarefa.",
            "files": [{"path": "src/a.rs"}], "origin": said}),
        ));
        for attempt in 0..3 {
            crate::shared::spec_state::seed_event(
                root,
                "x",
                "send",
                json!({"wave": 2, "role": "wave",
                "text": "pedido", "lines": 1, "chars": 6, "items": [wave], "mustard": "0", "author": "binary"}),
            );
            write(root, "x", "delivered", json!({"wave": 2, "text": format!("Tentativa {attempt}."), "files": ["src/a.rs"]}));
            crate::shared::spec_state::seed_verdict(root, "x", 2, "rejected", crit);
        }
        let round = || round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None }, None);

        let stopped = round();
        assert_eq!(stopped["stopped"][0]["wave"], json!(2), "{stopped}");
        let refused = close(root, "x");
        assert_eq!(refused["reason"], json!("wave-rejected"), "{refused}");
        assert!(refused["hint"].as_str().unwrap_or_default().contains('2'), "{refused}");

        write(root, "x", "remove", json!({"targets": [wave, task], "reason": "o usuário tirou a onda do plano"}));
        let rounded = round();
        assert!(rounded.get("stopped").is_none(), "{rounded}");
        assert_eq!(rounded["command"], json!("mustard-rt run close --spec x"), "{rounded}");
        let closed = close(root, "x");
        assert_eq!(closed["ok"], json!(true), "{closed}");
        assert_eq!(closed["phase"], json!("closed"), "{closed}");
    }

    /// Uma prova do cargo com o nome do teste errado e `--exact` sai verde sem
    /// rodar teste nenhum: o fechamento recusa, diz qual critério, o comando
    /// dela e o número de testes que a saída dele disse, e grava a execução
    /// como reprovada. A prova com o nome certo passa.
    #[test]
    fn a_proof_that_ran_zero_tests_blocks_the_close() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"prova\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n").unwrap();
        std::fs::write(
            root.join("src/lib.rs"),
            "pub fn sum(a: u32, b: u32) -> u32 { a + b }\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn sum_of_two() { assert_eq!(super::sum(1, 1), 2); }\n}\n",
        )
        .unwrap();
        std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
        ready_to_close(root, "x", &["cargo test --lib -- tests::sum_of_two --exact", "cargo test --lib -- tests::sum --exact"]);

        let refused = close(root, "x");
        assert_eq!(refused["reason"], json!("criterion-ran-no-test"), "{refused}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let criteria: Vec<u64> = log.visible().into_iter().filter(|e| e.event_type == "criterion").map(|e| e.id).collect();
        let wrong_name = "cargo test --lib -- tests::sum --exact";
        let expected = translate("close.criterion_ran_no_test", Locale::PtBr)
            .replace("{code}", &codes[&criteria[1]])
            .replace("{command}", wrong_name)
            .replace("{count}", "0");
        assert_eq!(refused["hint"], json!(expected), "{refused}");
        let hint = refused["hint"].as_str().unwrap_or_default();
        assert!(hint.contains(wrong_name) && hint.contains('0'), "a recusa diz o comando e o número: {hint}");
        let runs: Vec<(Option<u64>, Option<&str>)> =
            log.visible().into_iter().filter(|e| e.event_type == "criterion_run").map(|e| (e.int("criterion"), e.str_field("result"))).collect();
        // O terceiro é o critério que cobre a onda na cópia de teste — sempre
        // verde, para a rodada não travar a entrega.
        assert_eq!(runs, vec![(Some(criteria[0]), Some("pass")), (Some(criteria[1]), Some("fail")), (Some(criteria[2]), Some("pass"))]);
        assert_eq!(State::from_log(&log).phase, Some("running"), "a spec não fechou");
    }

    /// Uma prova verde que cita dois testes, um que existe num arquivo do
    /// projeto e outro que não existe em arquivo nenhum, trava o fechamento:
    /// a recusa diz qual critério e o nome que faltou, e a execução dele fica
    /// gravada como reprovada. A prova que cita só nomes presentes passa, e a
    /// que não cita nome nenhum segue como antes.
    #[test]
    fn a_green_proof_citing_a_test_that_exists_nowhere_blocks_the_close() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "#[test]\nfn sum_of_two() {}\n#[test]\nfn sum_of_three() {}\n").unwrap();
        let absent = "echo sum_of_two sum_of_four";
        ready_to_close(root, "x", &["echo Tests: 3 total", "echo sum_of_two sum_of_three", absent]);

        let refused = close(root, "x");
        assert_eq!(refused["reason"], json!("criterion-missing-test"), "{refused}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let criteria: Vec<u64> = log.visible().into_iter().filter(|e| e.event_type == "criterion").map(|e| e.id).collect();
        let expected = translate("close.criterion_missing_test", Locale::PtBr).replace("{code}", &codes[&criteria[2]]).replace("{name}", "sum_of_four");
        assert_eq!(refused["hint"], json!(expected), "{refused}");
        let hint = refused["hint"].as_str().unwrap_or_default();
        assert!(hint.contains(&codes[&criteria[2]]) && hint.contains("sum_of_four"), "{hint}");
        assert!(!hint.contains("sum_of_two"), "a recusa não acusa o nome que existe: {hint}");
        let runs: Vec<(Option<u64>, Option<&str>)> =
            log.visible().into_iter().filter(|e| e.event_type == "criterion_run").map(|e| (e.int("criterion"), e.str_field("result"))).collect();
        // O quarto é o critério que cobre a onda na cópia de teste — sempre
        // verde, para a rodada não travar a entrega.
        assert_eq!(
            runs,
            vec![(Some(criteria[0]), Some("pass")), (Some(criteria[1]), Some("pass")), (Some(criteria[2]), Some("fail")), (Some(criteria[3]), Some("pass")),]
        );
        assert_eq!(State::from_log(&log).phase, Some("running"), "a spec não fechou");
    }

    /// Os executores que dizem zero sem escrever número são lidos pela linha
    /// de resumo de cada um, e não por uma frase qualquer, e a prova que roda
    /// outro executor que não o cargo e sai verde dizendo zero teste trava o
    /// fechamento do mesmo jeito, com o comando e o número na recusa. O vitest
    /// sai com código 0 quando o filtro por nome não casa teste nenhum e
    /// escreve `Tests  no tests`: essa prova é recusada. O go escreve a marca
    /// dele por pacote, então a prova em que um pacote não rodou teste ao lado
    /// de outro que rodou passa — a corrida rodou teste. A que roda pelo menos
    /// um teste passa. E a execução recusada guarda o que o executor escreveu,
    /// e não uma frase montada sobre ela.
    #[test]
    fn a_proof_that_ran_zero_tests_is_read_by_the_summary_line_of_each_runner() {
        let go_mixed = "echo ok x/pkg 0.002s [no tests to run] && echo ok x/outro 0.02s";
        let cases = [
            ("go and vitest", go_mixed, "echo Tests no tests", "Tests no tests"),
            ("a total outside cargo", "echo Tests: 3 total", "echo Tests: 0 total", "Tests: 0 total"),
        ];
        for (case, ran, zero, written) in cases {
            let dir = tempdir().unwrap();
            let root = dir.path();
            ready_to_close(root, "x", &[ran, zero]);

            let refused = close(root, "x");
            assert_eq!(refused["reason"], json!("criterion-ran-no-test"), "{case}: {refused}");
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            let codes = log.codes();
            let criteria: Vec<u64> = log.visible().into_iter().filter(|e| e.event_type == "criterion").map(|e| e.id).collect();
            let expected = translate("close.criterion_ran_no_test", Locale::PtBr)
                .replace("{code}", &codes[&criteria[1]])
                .replace("{command}", zero)
                .replace("{count}", "0");
            assert_eq!(refused["hint"], json!(expected), "{case}: {refused}");
            let runs: Vec<(Option<&str>, Option<&str>)> =
                log.visible().into_iter().filter(|e| e.event_type == "criterion_run").map(|e| (e.str_field("result"), e.str_field("output"))).collect();
            // O terceiro é o critério que cobre a onda na cópia de teste —
            // sempre verde, para a rodada não travar a entrega.
            assert_eq!(runs.len(), 3, "{case}: os dois critérios de `proofs` e o da onda rodaram: {runs:?}");
            assert_eq!(runs[0], (Some("pass"), None), "{case}: a prova que rodou teste passa");
            assert_eq!(runs[1], (Some("fail"), Some(written)), "{case}: a execução recusada guarda o que o executor escreveu");
            assert_eq!(runs[2].0, Some("pass"), "{case}: {runs:?}");
            assert_eq!(State::from_log(&log).phase, Some("running"), "{case}: a spec não fechou");
        }
    }

    /// A leitura de quantos testes o comando rodou vale só na prova de um
    /// critério. O lint do projeto não passa por ela: um lint verde que
    /// escreve `Tests: 0 total` fecha a spec do mesmo jeito. E a prova de
    /// critério que não é comando de teste nenhum, cuja saída verde só cita
    /// "no tests" sem contagem de executor, passa: frase não é contagem.
    #[test]
    fn a_proof_that_ran_zero_tests_is_read_only_in_the_proof_of_a_criterion() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let not_a_test = "echo src/msg.rs: no tests found here";
        ready_to_close(root, "x", &[not_a_test]);

        let asked = close_with_lint(root, "echo Tests: 0 total", None);
        assert_eq!(asked["ok"], json!(true), "o lint verde não é lido como prova: {asked}");
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        let closed = close_with_lint(root, "echo Tests: 0 total", approve(root, "x"));
        assert_eq!(closed["ok"], json!(true), "{closed}");
        assert_eq!(closed["phase"], json!("closed"), "{closed}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let runs: Vec<Option<&str>> = log.visible().into_iter().filter(|e| e.event_type == "criterion_run").map(|e| e.str_field("result")).collect();
        // O segundo é o critério que cobre a onda na cópia de teste — sempre
        // verde, para a rodada não travar a entrega.
        assert_eq!(runs, vec![Some("pass"), Some("pass")], "a prova que não é teste passou: {runs:?}");
        assert_eq!(State::from_log(&log).phase, Some("closed"));
    }

    /// A revisão final aberta guarda, pelas rodadas, a vaga que o fechamento
    /// preparou e gravou no envio dela, até o veredito: enquanto o revisor
    /// trabalha na vaga a, com uma mudança sem comitar e um processo rodando
    /// nela, a onda 2 sai na vaga b e é comitada. Na rodada seguinte, o
    /// processo do revisor continua vivo, a onda 3 sai sem pegar a vaga a, e
    /// a mudança do revisor fica.
    #[test]
    fn an_open_final_review_keeps_its_slot_through_the_rounds_until_the_verdict() {
        use mustard_core::io::wave_prompt::{recorded_copy, shown, slot_path};
        use std::process::Stdio;

        /// O processo do revisor, encerrado no fim do teste, passe ele ou não.
        struct Reviewer(std::process::Child);
        impl Drop for Reviewer {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }

        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let path = store::spec_file(root, "x").unwrap();
        let spec_log = || store::read(&path).unwrap().unwrap();
        let asked = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() }, None);
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        let review = slot_path(root, "x", 0);
        let log = spec_log();
        let sent = log.visible().into_iter().rev().find(|e| e.event_type == "send" && e.str_field("role") == Some("review")).expect("o envio da revisão");
        assert_eq!(sent.str_field("copy"), Some(shown(&review).as_str()), "o envio grava a vaga que o fechamento preparou");

        std::fs::write(review.join(wave_file(1)), "fn revisto() {}\n").unwrap();
        // Um processo que fica vivo por dois minutos: o `sleep` do sistema, e,
        // onde ele não vem de fábrica, o `ping` que o Windows sempre tem.
        let mut waiting = if cfg!(windows) {
            let mut ping = Command::new("ping");
            ping.args(["-n", "121", "127.0.0.1"]);
            ping
        } else {
            let mut sleep = Command::new("sleep");
            sleep.arg("120");
            sleep
        };
        let mut reviewer =
            Reviewer(waiting.current_dir(&review).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().expect("o processo do revisor"));
        #[cfg(target_os = "linux")]
        crate::commands::flow::stuck::wait_until_spawned(reviewer.0.id(), "sleep");
        let alive = |reviewer: &mut Reviewer| reviewer.0.try_wait().ok().flatten().is_none();

        let said = log.visible().into_iter().find(|e| e.event_type == "message").map(|e| e.id).unwrap();
        let gate = log.visible().into_iter().rev().find(|e| e.event_type == "criterion").map(|e| e.id).unwrap();
        let plan = |n: u64| {
            write(
                root,
                "x",
                "wave",
                json!({"n": n, "text": format!("Onda {n}."), "criteria": [gate],
                "done_when": "A suíte passa.", "origin": said}),
            );
            write(
                root,
                "x",
                "task",
                json!({"wave": n, "text": format!("Tarefa da onda {n}."),
                "files": [{"path": wave_file(n)}], "origin": said}),
            );
        };
        let round = || round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None }, None);
        let sent_copy = |n: u64| recorded_copy(&spec_log(), n).map(|copy| copy.path);

        plan(2);
        let out = round();
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(sent_copy(2), Some(shown(&slot_path(root, "x", 1))), "a entrega 2 sai na vaga b: {out}");
        std::fs::write(slot_path(root, "x", 1).join(wave_file(2)), "fn dois() {}\n").unwrap();
        returned(root, "x", json!({"wave": 2, "text": "Saiu.", "files": [wave_file(2)], "commit": "a onda dois sai"}));
        let committed = round();
        assert_eq!(committed["ok"], json!(true), "{committed}");
        assert_eq!(std::fs::read_to_string(root.join(wave_file(2))).unwrap(), "fn dois() {}\n", "{committed}");
        assert!(alive(&mut reviewer), "o processo do revisor fica depois do commit da onda 2: {committed}");

        plan(3);
        let out = round();
        assert_eq!(out["ok"], json!(true), "{out}");
        let third = sent_copy(3).expect("a entrega 3 saiu numa vaga");
        assert_ne!(third, shown(&review), "a onda 3 não pega a vaga da revisão aberta: {out}");
        assert!(alive(&mut reviewer), "o processo do revisor continua vivo: {out}");
        let kept = std::fs::read_to_string(review.join(wave_file(1))).unwrap();
        assert_eq!(kept, "fn revisto() {}\n", "a mudança do revisor fica na vaga dele");
    }

    /// Um critério cuja prova não passa trava o fechamento, e a execução dele
    /// fica gravada assim mesmo: é o registro de que ele rodou.
    #[test]
    fn a_criterion_whose_proof_fails_blocks_the_close_and_stays_on_the_record() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --nao-existe-esta-opcao"]);

        let refused = close(root, "x");
        assert_eq!(refused["reason"], json!("criterion-failed"), "{refused}");
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let runs: Vec<&SpecEvent> = log.visible().into_iter().filter(|e| e.event_type == "criterion_run").collect();
        // O segundo é o critério que cobre a onda na cópia de teste — sempre
        // verde, para a rodada não travar a entrega.
        assert_eq!(runs.len(), 2, "a execução fica gravada");
        assert_eq!(runs[0].str_field("result"), Some("fail"));
        assert_eq!(runs[1].str_field("result"), Some("pass"));
        assert_eq!(State::from_log(&log).phase, Some("running"), "a spec não fechou");
    }

    /// A recusa de um critério cuja prova falha mostra o fim da saída dela,
    /// onde o motivo aparece: uma prova que escreve 200 linhas e falha na
    /// última recusa o fechamento com a última linha, e não com o começo. O
    /// que fica gravado na execução é o mesmo trecho.
    #[test]
    fn the_refusal_of_a_failing_proof_shows_the_end_of_its_output() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["sh -c 'i=1; while [ $i -le 200 ]; do echo \"linha $i\"; i=$((i+1)); done; exit 3'"]);

        let refused = close(root, "x");
        assert_eq!(refused["reason"], json!("criterion-failed"), "{refused}");
        let hint = refused["hint"].as_str().unwrap_or_default();
        assert!(hint.ends_with("linha 200"), "a recusa termina no fim da saída: {hint}");
        assert!(hint.contains("linha 161"), "e traz as últimas 40 linhas: {hint}");
        assert!(!hint.contains("linha 1\n"), "o começo da saída fica de fora: {hint}");

        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let run = log.visible().into_iter().find(|e| e.event_type == "criterion_run").unwrap();
        assert!(run.str_field("output").is_some_and(|out| out.ends_with("linha 200")), "{run:?}");
    }

    /// A base `dev` de onde a branch da spec saiu: nasce no commit semente,
    /// antes das ondas, e ganha `lib/fora.rs` num commit que a obra não fez,
    /// que a branch junta depois. Esse arquivo fica no projeto sem a branch o
    /// ter mudado; os das ondas, a branch mudou.
    pub(super) fn branched_from_dev(root: &Path) {
        let out = Command::new("git").args(["rev-list", "--max-parents=0", "HEAD"]).current_dir(root).output().unwrap();
        let seed = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let outside = tempdir().unwrap();
        let side = outside.path().join("dev");
        git_at(root, &["worktree", "add", "-q", "-b", "dev", &side.to_string_lossy(), &seed]);
        std::fs::create_dir_all(side.join("lib")).unwrap();
        std::fs::write(side.join("lib/fora.rs"), "fn fora() {}\n").unwrap();
        git_at(&side, &["add", "lib/fora.rs"]);
        git_at(&side, &["commit", "-q", "-m", "fora da obra"]);
        git_at(root, &["worktree", "remove", "--force", &side.to_string_lossy()]);
        git_at(root, &["merge", "-q", "--no-edit", "dev"]);
    }

    /// O lint do fechamento cai citando só um arquivo que a branch não mudou:
    /// nenhuma onda recebe reprovação, a recusa diz qual arquivo a saída citou
    /// e que ele não é da obra, e a rodada seguinte não despacha onda nenhuma.
    #[test]
    fn a_failure_outside_the_work_reopens_no_wave() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_with_waves(root, "x", &["git --version"], 2);
        branched_from_dev(root);

        let refused = close_with_lint(root, "echo error: unused variable at lib/fora.rs:1:4; exit 1", None);
        assert_eq!(refused["reason"], json!("lint-failed"), "{refused}");
        let said = translate("close.cited_outside", Locale::PtBr).replace("{files}", "`lib/fora.rs`");
        assert!(refused["hint"].as_str().unwrap_or_default().contains(&said), "{refused}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert!(!log.events.iter().any(|e| e.event_type == "validation_failure"), "no wave is rejected: {refused}");

        let next = round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None }, None);
        assert_eq!(next["dispatch"].as_array().map(Vec::len).unwrap_or_default(), 0, "{next}");
        assert_eq!(next["command"], json!("mustard-rt run close --spec x"), "{next}");
    }

    /// O lint do fechamento cai citando um arquivo de fora da obra e outro
    /// que a branch mudou e só a onda 2 declarou e entregou: só a onda 2
    /// reabre, com a saída no trecho de conserto dela, e a onda 1 segue
    /// entregue. A rodada seguinte despacha só a onda 2.
    #[test]
    fn a_cited_wave_file_reopens_only_that_wave() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_with_waves(root, "x", &["git --version"], 2);
        branched_from_dev(root);

        let lint = format!("echo error: lib/fora.rs:1:4 and {}:2:5 fail-mark; exit 1", wave_file(2));
        let refused = close_with_lint(root, &lint, None);
        assert_eq!(refused["reason"], json!("lint-failed"), "{refused}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert_eq!(log.last_rejected().keys().copied().collect::<Vec<_>>(), vec![2], "{refused}");
        let fix = mustard_core::domain::wave_prompt::fix_lines(&log, 2);
        assert!(fix.first().and_then(|e| e.str_field("text")).is_some_and(|text| text.contains("fail-mark")), "{fix:?}");
        assert!(mustard_core::domain::wave_prompt::fix_lines(&log, 1).is_empty());
        let states = crate::commands::flow::round::wave_states(&log);
        assert_eq!(states.get(&1), Some(&mustard_core::view::document::WaveState::Committed), "{states:?}");

        let next = round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None }, None);
        let sent: Vec<u64> = next["dispatch"].as_array().cloned().unwrap_or_default().iter().filter_map(|d| d["wave"].as_u64()).collect();
        assert_eq!(sent, vec![2], "{next}");
    }

    /// Num projeto TypeScript, o fechamento roda a compilação declarada antes
    /// do lint, da suíte e de qualquer critério. A compilação vermelha que cita
    /// o arquivo da onda 2 trava o fechamento com o comando e a saída dela,
    /// sem rodar o lint nem critério nenhum, e reabre só a onda 2.
    #[test]
    fn a_red_build_blocks_the_close_of_a_typescript_project() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_with_files(root, "x", &["git --version"], &["web/one.ts".to_string(), "web/two.ts".to_string()], &[], &[]);
        branched_from_dev(root);
        let build = "echo \"web/two.ts(2,10): error TS2304: Cannot find name 'dois'.\"; exit 1";
        let config = json!({"buildCommand": build, "lintCommand": "echo lint >> rodou", "testCommand": "echo suite >> rodou"});
        std::fs::write(root.join("mustard.json"), config.to_string()).unwrap();

        let refused = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), ..Default::default() }, None);
        assert_eq!(refused["reason"], json!("build-failed"), "{refused}");
        let hint = refused["hint"].as_str().unwrap_or_default();
        assert!(hint.contains(&format!("`{build}`")) && hint.contains("error TS2304"), "{refused}");
        assert!(!check_copy(root).join("rodou").exists(), "com a compilação vermelha, o lint e a suíte nem rodam: {refused}");
        assert_eq!(criterion_runs(root), 0, "{refused}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert_eq!(log.last_rejected().keys().copied().collect::<Vec<_>>(), vec![2], "{refused}");
    }

    /// O achado da revisão final que cita, pelo código, o critério escrito na
    /// spec, e o que diz só o texto de uma regra nova, cada um com a prova no
    /// detalhe.
    fn findings_of_the_final_review() -> (Value, Value) {
        let written = json!({"title": "A soma não trata estouro", "detail": "A soma estoura em src/w1.rs. Prova: `git --version`", "files": ["src/w1.rs"],
            "rule": "MSTD-CRIT-0001", "places": ["src/w1.rs"]});
        let unwritten = json!({"title": "O nome da função não diz o que ela faz", "detail": "A função de src/w1.rs se chama one. Prova: `git --version`",
            "files": ["src/w1.rs"], "rule": "Toda função diz no nome o que faz", "places": ["src/w1.rs"]});
        (written, unwritten)
    }

    /// A obra `x` pronta para fechar, cuja revisão final aprova com os dois
    /// achados de [`findings_of_the_final_review`]. Devolve a resposta do
    /// fechamento que assume o veredito e o número do critério que o primeiro
    /// achado cita.
    fn closed_with_a_written_and_an_unwritten_rule(root: &Path) -> (Value, u64) {
        ready_to_close(root, "x", &["git --version"]);
        let opts = CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), ..Default::default() };
        let asked = close_for(&opts, None);
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        let (written, unwritten) = findings_of_the_final_review();
        verdict_written(root, "x", json!({"final": true, "result": "approved", "text": "Pronta, com dois achados.", "leftovers": [written, unwritten]}));
        let out = close_for(&opts, None);
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let criterion = log.codes().into_iter().find(|(_, code)| code == "MSTD-CRIT-0001").map(|(id, _)| id).expect("the criterion");
        (out, criterion)
    }

    /// Os eventos vigentes do tipo `kind` da obra `x`.
    fn visible_of(root: &Path, kind: &str) -> Vec<SpecEvent> {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        log.visible().into_iter().filter(|event| event.event_type == kind).cloned().collect()
    }

    /// O achado que cita o critério escrito vira o pedido e a tarefa da regra,
    /// que cobre o critério. O achado de regra nova não vira pedido nem
    /// tarefa: a resposta do fechamento o leva ao usuário como pergunta, com
    /// o aviso que manda fazê-la antes do pull request.
    #[test]
    fn a_final_finding_without_a_written_rule_becomes_a_question_and_no_task() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (out, criterion) = closed_with_a_written_and_an_unwritten_rule(root);
        assert_eq!(out["phase"], json!("running"), "{out}");

        let (written, unwritten) = findings_of_the_final_review();
        let titled = |kind: &str, finding: &Value| visible_of(root, kind).into_iter().filter(|event| event.str_field("title") == finding["title"].as_str()).collect::<Vec<_>>();
        let tasks = titled("task", &written);
        assert_eq!(tasks.len(), 1, "{out}");
        assert!(tasks[0].ints("covers").contains(&criterion), "a tarefa da regra cobre o critério: {:?}", tasks[0]);
        assert_eq!(titled("request", &written).len(), 1, "{out}");
        assert!(titled("task", &unwritten).is_empty() && titled("request", &unwritten).is_empty(), "a regra nova não vira trabalho: {out}");

        let question = translate("finding.question", Locale::PtBr)
            .replace("{title}", unwritten["title"].as_str().unwrap())
            .replace("{rule}", unwritten["rule"].as_str().unwrap());
        let mut expected = unwritten.clone();
        expected.as_object_mut().unwrap().remove("files");
        expected["question"] = json!(question);
        assert_eq!(out["questions"], json!([expected]), "{out}");
        assert_eq!(hints_of(&out, "final-finding-question"), vec![translate("finding.questions_hint", Locale::PtBr)], "{out}");
    }

    /// A tarefa que o achado de regra escrita ganhou sai por `remove`, como
    /// qualquer outra tarefa. Nenhuma tarefa cobre mais o critério, e o
    /// fechamento seguinte recusa, pelo pedido do achado que nenhuma onda
    /// entregou, sem gravar a tarefa de novo.
    #[test]
    fn removing_the_task_of_a_rule_leaves_it_uncovered_and_the_close_refuses() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (_, criterion) = closed_with_a_written_and_an_unwritten_rule(root);
        let covering = |root: &Path| visible_of(root, "task").into_iter().filter(|task| task.ints("covers").contains(&criterion)).map(|task| task.id).collect::<Vec<_>>();
        let task = covering(root)[0];

        let removed = crate::commands::spec_events::write::write_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: Some("x".into()),
            event_type: "remove".into(),
            json: json!({"targets": [task], "reason": "não vale"}).to_string(),
        });
        assert_eq!(removed["ok"], json!(true), "{removed}");
        assert!(covering(root).is_empty(), "a regra fica descoberta");

        let refused = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), ..Default::default() }, None);
        assert_eq!((refused["ok"].clone(), refused["reason"].clone()), (json!(false), json!("request-not-delivered")), "{refused}");
        let (written, _) = findings_of_the_final_review();
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let request = visible_of(root, "request").into_iter().find(|request| request.str_field("title") == written["title"].as_str()).expect("the request");
        assert!(refused["hint"].as_str().unwrap_or_default().contains(&log.codes()[&request.id]), "{refused}");
        assert!(covering(root).is_empty(), "o fechamento recusado não grava a tarefa de novo");
    }
}
