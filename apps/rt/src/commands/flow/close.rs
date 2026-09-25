//! `mustard-rt run close [--spec <nome>]` — o fechamento de uma spec.
//!
//! É a porta única do fechamento: grava o que voltou da última rodada,
//! confere se a obra terminou mesmo, roda os dois comandos do servidor — o
//! lint e a suíte inteira, o `lintCommand` e o `testCommand` do
//! `mustard.json` — em ambiente limpo, roda cada critério uma vez e grava a
//! execução de cada um, e então fecha — grava a fase `closed`, que arma a
//! cobrança das pendências pela mesma porta, solta a spec da sessão e prepara
//! a cópia para o banco de dados da página. A pasta de uma spec fechada fica
//! com o arquivo de eventos e a pasta da cópia (`copy/`), e nenhuma página.
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
//! revisão: manda o revisor gravar o dele.
//! Enquanto nada muda depois da máquina verde, a volta não roda o lint nem os
//! critérios de novo. A reprovação fica na onda que ele apontou, que volta
//! como conserto pela rodada; entregue o conserto, o fechamento pede o mesmo
//! agente de novo, mas só para conferir o conserto, não a obra inteira. Em
//! até duas voltas de conserto sem aprovar, a onda para e a decisão passa a
//! ser do usuário — o mesmo limite de qualquer conserto.
//!
//! **A cópia do revisor.** É o binário que cria a cópia onde o agente de
//! teste dedicado confere a obra, pela mesma porta das cópias de onda, antes
//! de a máquina rodar, e que a apaga quando a obra fecha. Uma cópia que
//! chegou com mudança — o corte que o revisor fez para ver a prova cair e não
//! desfez — trava o fechamento, com os arquivos: ler código sabotado como se
//! fosse o da obra é pior do que parar.
//!
//! **O que trava.** Onda sem commit; onda cuja última revisão reprovou e
//! ainda não recebeu o conserto; pedido do usuário que nenhuma onda entregou;
//! a cópia do revisor com mudança, ou que não pôde ser criada; o lint ou a
//! suíte que falham, com a saída; critério cuja prova não passou; e
//! critério cuja prova saiu verde sem rodar teste nenhum — a saída do
//! executor diz zero teste, que é o que um nome de teste errado dá, e a
//! recusa traz o comando e o número que ela leu. Cada recusa diz qual onda
//! refazer — não basta os testes passarem.
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

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::{Block, BlockQuery, Refusal, SpecEvent, SpecLog};
use mustard_core::domain::spec_index::title_of;
use mustard_core::domain::spec_state::{final_approval, last_change, PhaseWriter, SpecState, State};
use mustard_core::domain::wave_prompt::{count_lines, recorded_choice, requested_model, unowned};
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Map, Value};

use crate::commands::flow::round::Caller;
use crate::commands::spec_events::{self, read::checkout, write::record};
use crate::shared::spec_state::{session_from_env, DiskSpecState};

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
    NotRunning { phase: String },
    /// Uma onda que não tem commit nenhum.
    WaveWithoutCommit { wave: u64 },
    /// Uma onda cuja última revisão reprovou.
    WaveRejected { wave: u64 },
    /// Um pedido do usuário que nenhuma onda entregou.
    RequestNotDelivered { code: String },
    /// O backlog ainda tem tarefa, com os códigos delas.
    BacklogNotEmpty { tasks: String },
    /// O lint do projeto falhou.
    LintFailed { command: String, output: String },
    /// A suíte inteira do projeto falhou.
    SuiteFailed { command: String, output: String },
    /// A cópia do revisor final chegou com mudança, com os arquivos dela.
    ReviewCopyDirty { copy: String, files: String },
    /// A cópia do revisor final não pôde ser criada, com o que o git disse.
    ReviewCopyFailed { copy: String, detail: String },
    /// Um critério cuja prova não passou.
    CriterionFailed { code: String, output: String },
    /// Um critério cuja prova saiu verde sem rodar teste nenhum, com o
    /// comando dela e o número de testes que a saída dele disse.
    CriterionRanNoTest { code: String, command: String, tests: u64 },
}

impl CloseRefusal {
    fn reason(&self) -> String {
        match self {
            Self::Refused(refusal) => refusal.reason().to_string(),
            Self::Report(refusal) => refusal.reason(),
            Self::NotRunning { .. } => "close-not-running".into(),
            Self::WaveWithoutCommit { .. } => "wave-without-commit".into(),
            Self::WaveRejected { .. } => "wave-rejected".into(),
            Self::RequestNotDelivered { .. } => "request-not-delivered".into(),
            Self::BacklogNotEmpty { .. } => "backlog-not-empty".into(),
            Self::LintFailed { .. } => "lint-failed".into(),
            Self::SuiteFailed { .. } => "suite-failed".into(),
            Self::ReviewCopyDirty { .. } => "review-copy-dirty".into(),
            Self::ReviewCopyFailed { .. } => "review-copy-failed".into(),
            Self::CriterionFailed { .. } => "criterion-failed".into(),
            Self::CriterionRanNoTest { .. } => "criterion-ran-no-test".into(),
        }
    }

    fn message(&self, lang: Locale) -> String {
        let fill = |key: &str, slots: &[(&str, String)]| {
            slots.iter().fold(translate(key, lang).to_string(), |text, (slot, value)| text.replace(slot, value))
        };
        match self {
            Self::Refused(refusal) => refusal.message(lang),
            Self::Report(refusal) => refusal.message(lang),
            Self::NotRunning { phase } => fill("close.not_running", &[("{phase}", phase.clone())]),
            Self::WaveWithoutCommit { wave } => {
                fill("close.wave_without_commit", &[("{wave}", wave.to_string())])
            }
            Self::WaveRejected { wave } => fill("close.wave_rejected", &[("{wave}", wave.to_string())]),
            Self::RequestNotDelivered { code } => {
                fill("close.request_not_delivered", &[("{code}", code.clone())])
            }
            Self::BacklogNotEmpty { tasks } => fill("close.backlog_not_empty", &[("{tasks}", tasks.clone())]),
            Self::LintFailed { command, output } => {
                fill("close.lint_failed", &[("{command}", command.clone()), ("{output}", output.clone())])
            }
            Self::SuiteFailed { command, output } => {
                fill("close.suite_failed", &[("{command}", command.clone()), ("{output}", output.clone())])
            }
            Self::ReviewCopyDirty { copy, files } => {
                fill("close.review_copy_dirty", &[("{copy}", copy.clone()), ("{files}", files.clone())])
            }
            Self::ReviewCopyFailed { copy, detail } => {
                fill("close.review_copy_failed", &[("{copy}", copy.clone()), ("{detail}", detail.clone())])
            }
            Self::CriterionFailed { code, output } => {
                fill("close.criterion_failed", &[("{code}", code.clone()), ("{output}", output.clone())])
            }
            Self::CriterionRanNoTest { code, command, tests } => fill(
                "close.criterion_ran_no_test",
                &[("{code}", code.clone()), ("{command}", command.clone()), ("{count}", tests.to_string())],
            ),
        }
    }

    fn to_value(&self, lang: Locale) -> Value {
        if let Self::Report(refusal) = self {
            return refusal.to_value(lang);
        }
        json!({ "ok": false, "reason": self.reason(), "hint": self.message(lang) })
    }
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
    match run_close(opts, &project.root, lang, caller) {
        Ok(report) => report,
        Err(refusal) => refusal.to_value(lang),
    }
}

fn run_close(
    opts: &CloseOpts,
    root: &Path,
    lang: Locale,
    caller: Caller<'_>,
) -> Result<Value, CloseRefusal> {
    let session = caller.session;
    let spec = match opts.spec.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(spec) => spec.to_string(),
        None => DiskSpecState::new(&checkout(&opts.root))
            .active(session)
            .ok_or(CloseRefusal::Refused(Refusal::NoCurrentSpec))?,
    };
    let path = store::spec_file(root, &spec).map_err(CloseRefusal::Refused)?;
    let read = |path: &Path| -> Result<SpecLog, CloseRefusal> {
        store::read(path)
            .map_err(CloseRefusal::Refused)?
            .ok_or_else(|| CloseRefusal::Refused(Refusal::NoSpecFile { spec: spec.clone() }))
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

    // Nada fica preso: todo processo que um agente deixou rodando — um laço
    // de espera, ou um comando na cópia de uma onda já apagada — é encerrado
    // no fechamento, e a resposta diz qual.
    let stuck_hint =
        crate::commands::flow::stuck::report_line(&crate::commands::flow::stuck::end_stuck_processes(root), lang);

    // O que voltou da última rodada entra antes das conferências, pela mesma
    // porta da rodada, com o commit: a volta que a última onda gravou na spec
    // é assumida aqui, com ou sem relatório, e é o commit dela que fecha a
    // última onda.
    let raw = opts.report.as_deref().map(str::trim).filter(|r| !r.is_empty());
    let recorded: Vec<Value> =
        crate::commands::flow::round::take_report(&opts.root, root, &spec, raw, &log, lang, caller)
            .map_err(CloseRefusal::Report)?
            .recorded;

    // A spec antiga passa para o backlog antes de ler as ondas: a onda
    // desenhada à mão que nunca saiu não recusa o fechamento, porque a versão
    // nova a ignora.
    crate::commands::flow::round::convert_hand_waves(&opts.root, root, &spec, lang).map_err(CloseRefusal::Refused)?;
    let log = read(&path)?;
    finished(&log)?;
    // O pedido de revisão só fecha com o veredito que o revisor grava e que a
    // leitura acima assume: sem ele, o fechamento não abre outro pedido, e a
    // resposta manda o revisor gravar o veredito.
    if crate::commands::flow::round::open_review(&log).is_some() {
        return Err(CloseRefusal::Report(crate::commands::flow::round::RoundRefusal::VerdictMissing));
    }

    // A cópia do revisor sai antes da máquina, pela mesma porta das cópias de
    // onda: a que chegou com mudança trava aqui, antes de gastar a suíte, e
    // a limpa vai para o commit da obra. Com a obra já aprovada, ninguém mais
    // revisa, e a cópia só espera ser apagada lá embaixo. Os arquivos locais
    // que não chegaram a ela viram aviso no pedido do revisor.
    let not_copied = if final_approved(&log) { Vec::new() } else { prepare_review_copy(root, &spec, &log)? };

    // A máquina antes do agente de teste dedicado: os dois comandos do
    // servidor e cada critério, uma vez por fechamento. A volta que só
    // confere a aprovação, sem nada mudado desde a máquina verde, não roda
    // nada de novo.
    let (runs, undeclared) =
        if proved_since_last_change(&log) { (Vec::new(), Vec::new()) } else { machine(opts, root, &spec, &log, lang)? };
    // O caminho de volta roda depois que a máquina passa, e nunca trava: os
    // testes sem dono viram aviso na resposta, e não recusa.
    let unowned_tests = unowned_test_hints(root, &log, lang);

    let log = read(&path)?;
    if !final_approved(&log) {
        let prompt = mustard_core::io::wave_prompt::final_review(root, &spec, &log, lang);
        // O pedido do agente de revisão final é gravado como evento de
        // envio antes de sair daqui, pela mesma porta que grava o pedido de
        // cada onda: o texto inteiro, o papel de revisão e o modelo — nunca
        // um segundo caminho de gravação. O molde do revisor não vai junto:
        // ele mora no projeto, e o papel já diz qual é.
        let mut draft = Map::new();
        draft.insert("role".into(), json!("review"));
        draft.insert("lines".into(), json!(count_lines(&prompt)));
        draft.insert("chars".into(), json!(prompt.chars().count()));
        draft.insert("text".into(), json!(prompt));
        draft.insert("model".into(), json!(requested_model("review")));
        draft.insert("mustard".into(), json!(env!("CARGO_PKG_VERSION")));
        draft.insert("author".into(), json!("binary"));
        record(&opts.root, &spec, "send", draft, PhaseWriter::Binary).map_err(CloseRefusal::Refused)?;
        let next = translate("close.final_review", lang).replace("{spec}", &spec);
        let mut out = json!({
            "ok": true,
            "spec": spec,
            "phase": "running",
            "recorded": recorded,
            "criteria": runs,
            "review": { "final": true, "prompt": prompt },
            "next": next,
        });
        if let Some(hint) = &stuck_hint {
            spec_events::pages::push_warning(&mut out, "stuck-ended", hint);
        }
        for hint in &undeclared {
            spec_events::pages::push_warning(&mut out, "server-command-not-declared", hint);
        }
        for hint in &unowned_tests {
            spec_events::pages::push_warning(&mut out, "unowned-test", hint);
        }
        let copy = mustard_core::io::wave_prompt::shown(&mustard_core::io::wave_prompt::final_copy_path(root, &spec));
        for file in &not_copied {
            let hint = crate::commands::flow::round::local_file_missing(file, &copy, lang);
            spec_events::pages::push_warning(&mut out, "local-file-missing", &hint);
        }
        return Ok(out);
    }

    // A aceitação do veredito final grava a tabela de rastreabilidade: uma
    // linha por item do combinado, com a verificação e o arquivo que o
    // próprio veredito já trouxe por item, e a situação. Sem essa tabela,
    // ninguém consultava depois qual item tem qual verificação, nem onde o
    // comportamento mora — o resultado morria no veredito.
    if let Some(verdict) = final_approval(&log) {
        record_tracking_table(&opts.root, &spec, verdict).map_err(CloseRefusal::Refused)?;
    }

    // A fase `closed` sai só por aqui, e é a mesma porta que arma a cobrança
    // das pendências. A função antiga de fechar, que grava arquivos do formato
    // velho, não é chamada.
    crate::commands::spec_events::write::record_phase(&opts.root, &spec, "closed", session);
    if let Some(sid) = session {
        crate::shared::context::session::unbind_session_spec(&opts.root.to_string_lossy(), sid);
    }
    // A troca do binário instalado do próprio Mustard acontece uma vez só,
    // aqui: depois da aprovação final, nunca a cada rodada. Sem onda
    // entregue na spec inteira, sem comando de teste declarado, ou fora da
    // raiz que constrói o próprio `mustard-rt`, nada roda.
    let reinstall_warning =
        crate::commands::flow::round::reinstall_binary(root, !log.delivered_waves().is_empty(), lang);
    // A obra fechou: ninguém mais revisa, e a cópia do revisor sai daqui,
    // com o que tiver dentro — o que ele cortou para ver a prova cair não é
    // trabalho de ninguém, e ficar para o próximo fechamento é o defeito.
    let review_copy_kept = remove_review_copy(root, &spec, lang);
    // O fechamento é um marco: a cópia para o banco da página sai aqui, com a
    // fase fechada na linha da spec da página do projeto.
    let prepared = crate::commands::spec_events::pages::copy::prepare(root, &spec, lang);

    // O pull request é o passo seguinte, e a linha dele sai pronta, com a base
    // e a branch tiradas do estado — pela mesma tabela que a retomada usa.
    let command = crate::commands::flow::resume::next_command("closed", &spec, &State::from_log(&read(&path)?));

    let mut out = json!({
        "ok": true,
        "spec": spec,
        "phase": "closed",
        "recorded": recorded,
        "criteria": runs,
    });
    if let Some(hint) = &stuck_hint {
        spec_events::pages::push_warning(&mut out, "stuck-ended", hint);
    }
    if let Some(warning) = &reinstall_warning {
        let hint = warning["hint"].as_str().unwrap_or_default();
        spec_events::pages::push_warning(&mut out, "binary-not-reinstalled", hint);
    }
    if let Some(hint) = &review_copy_kept {
        spec_events::pages::push_warning(&mut out, "review-copy-kept", hint);
    }
    for hint in &undeclared {
        spec_events::pages::push_warning(&mut out, "server-command-not-declared", hint);
    }
    for hint in &unowned_tests {
        spec_events::pages::push_warning(&mut out, "unowned-test", hint);
    }
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
    let carried: BTreeSet<u64> = log
        .last_by_wave("send")
        .keys()
        .filter_map(|wave| recorded_choice(&log, *wave))
        .flat_map(|choice| choice.added.into_iter().map(|(id, _)| id))
        .collect();
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
    crate::commands::spec_events::pages::end_milestone(&mut out, prepared.as_ref(), &spec, "close", &then, lang);
    if !command.is_null() {
        out["command"] = command;
    }
    Ok(out)
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
        let Some(item) = crate::commands::event::pending::open_born_in(root, log).into_iter().find(|i| i.id == id)
        else {
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

/// A máquina do fechamento: os dois comandos que o servidor roda — o lint e
/// a suíte inteira —, cada um em ambiente limpo ([`clean_env`]), e depois
/// cada critério, uma vez, com a execução de cada um gravada. O comando que
/// falha recusa antes de qualquer critério rodar; o critério que falha recusa
/// depois de todos rodarem.
///
/// **Um lugar só.** Os dois comandos são o `lintCommand` e o `testCommand` do
/// `mustard.json`: a mesma declaração que o pedido de cada onda cita e que a
/// reinstalação do binário usa, e não uma segunda lista escrita aqui. O
/// projeto que quer o fechamento igual ao servidor declara ali, ao pé da
/// letra, o que o servidor roda — no próprio Mustard, as linhas `Test` e
/// `Clippy` de `.github/workflows/ci.yml`.
///
/// **O projeto que não declara.** Fora do próprio Mustard, um projeto pode
/// não ter servidor nenhum, ou não ter dito ao Mustard o que ele roda: o
/// comando que falta não roda, e a resposta leva um aviso por chave que
/// falta, dizendo que o fechamento não promete o que o servidor vai dizer.
/// É aviso, nunca recusa: recusar travaria todo projeto que nunca declarou,
/// sem nada que ele pudesse consertar no código da obra.
fn machine(
    opts: &CloseOpts,
    root: &Path,
    spec: &str,
    log: &SpecLog,
    lang: Locale,
) -> Result<(Vec<Value>, Vec<String>), CloseRefusal> {
    let declared = mustard_core::ProjectConfig::load(root).commands();
    let mut undeclared: Vec<String> = Vec::new();
    for (key, command) in [("lintCommand", declared.lint), ("testCommand", declared.test)] {
        let Some(command) = command else {
            undeclared.push(translate("close.server_command_not_declared", lang).replace("{key}", key));
            continue;
        };
        // Nenhum dos dois é prova de critério: eles não prometem rodar um
        // teste pelo nome, e a leitura de quantos testes a saída diz fica
        // fora do caminho deles. O teto também é o deles, de uma hora, e não
        // o de um critério: a suíte inteira leva o tempo que o projeto pede.
        let out = crate::commands::review::qa_run::run_server_command(&clean_env(&command), root);
        if out.result != "pass" {
            return Err(if key == "lintCommand" {
                CloseRefusal::LintFailed { command, output: out.output }
            } else {
                CloseRefusal::SuiteFailed { command, output: out.output }
            });
        }
    }

    // Cada critério roda uma vez, e cada execução é gravada.
    let criteria = criteria_list(log);
    let (outcomes, failed) = crate::commands::review::qa_run::run_criteria_proofs(root, &criteria);
    let mut runs: Vec<Value> = Vec::new();
    for (id, code, out) in &outcomes {
        let mut draft = Map::new();
        draft.insert("criterion".into(), json!(id));
        draft.insert("result".into(), json!(out.result));
        draft.insert("exit".into(), json!(out.exit));
        draft.insert("ms".into(), json!(out.ms));
        draft.insert("author".into(), json!("binary"));
        if !out.output.trim().is_empty() {
            draft.insert("output".into(), json!(out.output));
        }
        record(&opts.root, spec, "criterion_run", draft, PhaseWriter::Binary)
            .map_err(CloseRefusal::Refused)?;
        runs.push(json!({ "criterion": code, "result": out.result, "exit": out.exit, "ms": out.ms }));
    }
    match failed {
        Some(failed) => Err(match failed.ran_no_test {
            Some(tests) => {
                CloseRefusal::CriterionRanNoTest { code: failed.code, command: failed.command, tests }
            }
            None => CloseRefusal::CriterionFailed { code: failed.code, output: failed.output },
        }),
        None => Ok((runs, undeclared)),
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

/// Cria a cópia do revisor final da spec `spec` no commit da obra, pela
/// mesma porta das cópias de onda (`ensure_copy`), com a trava do passo do
/// git presa, como a rodada cria as dela. A cópia que já existe e está limpa
/// vai para o commit; a que tem mudança recusa, com os arquivos, e a
/// recusa diz como descartar — é o corte que o revisor anterior deixou, e
/// revisar por cima dele é ler código sabotado como se fosse o da obra.
/// Como a cópia de onda, ela recebe os arquivos locais do projeto pelo
/// conteúdo; devolve os que não chegaram.
fn prepare_review_copy(root: &Path, spec: &str, log: &SpecLog) -> Result<Vec<String>, CloseRefusal> {
    use mustard_core::io::wave_prompt::{final_copy_path, final_review_commit, shown};
    use mustard_core::platform::git;
    let path = final_copy_path(root, spec);
    let copy = shown(&path);
    let failed = |detail: String| CloseRefusal::ReviewCopyFailed { copy: copy.clone(), detail };
    if path.join(".git").is_file() {
        let status = git::run(&path, &["status", "--porcelain", "--untracked-files=all"]);
        if !status.ok {
            return Err(failed(status.stderr.trim().to_string()));
        }
        let files: Vec<&str> =
            status.stdout.lines().filter(|line| line.len() > 3).map(|line| line[3..].trim()).collect();
        if !files.is_empty() {
            return Err(CloseRefusal::ReviewCopyDirty { copy: copy.clone(), files: files.join(", ") });
        }
    }
    let commit = match final_review_commit(root, log) {
        Some(sha) => sha,
        None => git::run(root, &["rev-parse", "HEAD"]).result().map_err(&failed)?,
    };
    let _held = crate::commands::git_settle::git_step_lock(root).map_err(&failed)?;
    crate::commands::flow::round::ensure_copy(root, &path, &commit).map_err(failed)
}

/// Apaga a cópia do revisor final da spec `spec`, quando ela existe, com a
/// trava do passo do git presa. Devolve o aviso quando o git não deixou
/// apagar: a obra fecha do mesmo jeito, e o aviso diz qual pasta ficou.
fn remove_review_copy(root: &Path, spec: &str, lang: Locale) -> Option<String> {
    use mustard_core::io::wave_prompt::{final_copy_path, shown};
    let path = final_copy_path(root, spec);
    if !path.exists() {
        return None;
    }
    let copy = shown(&path);
    let removed = crate::commands::git_settle::git_step_lock(root).and_then(|_held| {
        mustard_core::platform::git::run(root, &["worktree", "remove", "--force", &copy]).result().map(|_| ())
    });
    removed.err().map(|detail| {
        translate("close.review_copy_kept", lang).replace("{copy}", &copy).replace("{detail}", &detail)
    })
}

/// A máquina já passou depois da última mudança: cada critério vigente tem,
/// depois dela, uma execução, e a mais nova passou. O critério só roda com o
/// lint verde, então a mesma leitura diz que o lint passou.
fn proved_since_last_change(log: &SpecLog) -> bool {
    let since = last_change(log);
    let visible = log.block(BlockQuery::Block(Block::Criteria));
    let criteria: Vec<u64> = visible.iter().filter(|e| e.event_type == "criterion").map(|e| e.id).collect();
    !criteria.is_empty()
        && criteria.iter().all(|id| {
            visible
                .iter()
                .rev()
                .find(|e| e.event_type == "criterion_run" && e.id > since && e.int("criterion") == Some(*id))
                .is_some_and(|run| run.str_field("result") == Some("pass"))
        })
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
            let verification =
                entry.get("text").and_then(Value::as_str).unwrap_or_default().trim().to_string();
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

/// A obra terminou? Recusa enquanto houver onda sem commit — tirando a que
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
fn finished(log: &SpecLog) -> Result<(), CloseRefusal> {
    if let Some(wave) = crate::commands::flow::round::waves_pending_fix(log).into_keys().next() {
        return Err(CloseRefusal::WaveRejected { wave });
    }

    let committed: BTreeSet<u64> = log
        .block(BlockQuery::Block(Block::Progress))
        .into_iter()
        .filter(|e| e.event_type == "commit")
        .flat_map(|e| e.ints("waves"))
        .collect();
    // A onda que voltou só conferindo, sem arquivo mudado, fecha sem commit:
    // a leitura é a mesma que a rodada usa ao escolher o que comitar.
    let checked_only = crate::commands::flow::round::waves_checked_only(log);
    for wave in log.planned_waves() {
        if !committed.contains(&wave) && !checked_only.contains(&wave) {
            return Err(CloseRefusal::WaveWithoutCommit { wave });
        }
    }

    // A tarefa que ainda está no backlog não foi entregue por onda nenhuma: a
    // leitura do backlog é a mesma da rodada e da formação do lote.
    let left = crate::commands::flow::round::backlog_left(log);
    if !left.is_empty() {
        let codes = log.codes();
        let tasks = left.iter().map(|id| codes.get(id).cloned().unwrap_or_else(|| id.to_string())).collect::<Vec<_>>();
        return Err(CloseRefusal::BacklogNotEmpty { tasks: tasks.join(", ") });
    }

    // Um pedido do usuário que chegou depois da última entrega não foi
    // entregue por onda nenhuma.
    let last_delivered = log
        .block(BlockQuery::Block(Block::Waves))
        .into_iter()
        .filter(|e| e.event_type == "delivered")
        .map(|e| e.id)
        .max()
        .unwrap_or(0);
    let codes = log.codes();
    for request in log.block(BlockQuery::Block(Block::Notes)).into_iter().filter(|e| e.event_type == "request") {
        if request.id > last_delivered {
            return Err(CloseRefusal::RequestNotDelivered {
                code: codes.get(&request.id).cloned().unwrap_or_else(|| request.id.to_string()),
            });
        }
    }
    Ok(())
}

/// Os critérios vigentes da spec: id, código e a prova de cada um, na ordem
/// em que a leitura os dá. A máquina roda a prova de cada um daqui, e o
/// caminho de volta ([`unowned_test_hints`]) usa a mesma lista para saber
/// quais testes já têm dono.
fn criteria_list(log: &SpecLog) -> Vec<(u64, String, String)> {
    let codes = log.codes();
    log.block(BlockQuery::Block(Block::Criteria))
        .into_iter()
        .filter(|e| e.event_type == "criterion")
        .filter_map(|e| {
            let proof = e.str_field("proof")?.trim().to_string();
            Some((e.id, codes.get(&e.id).cloned().unwrap_or_else(|| e.id.to_string()), proof))
        })
        .collect()
}

/// O hash da árvore vazia do git — a base de quem toca um arquivo que nasceu
/// no próprio commit, sem pai para comparar.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// A base de onde ler o que a obra mudou em `file`: o pai do commit mais
/// velho, entre os que esta spec gravou, que tocou `file` — ou a árvore
/// vazia, quando esse commit não tem pai (o arquivo nasceu nele).
fn obra_base(root: &Path, log: &SpecLog, file: &str) -> Option<String> {
    let sha = log
        .block(BlockQuery::Block(Block::Progress))
        .into_iter()
        .find(|e| {
            e.event_type == "commit"
                && e.fields
                    .get("files")
                    .and_then(Value::as_array)
                    .is_some_and(|files| files.iter().any(|f| f.as_str() == Some(file)))
        })
        .and_then(|e| e.str_field("sha").map(str::to_string))?;
    let parent = mustard_core::platform::git::run(root, &["rev-parse", &format!("{sha}^")]);
    Some(parent.out().unwrap_or_else(|| EMPTY_TREE.to_string()))
}

/// As linhas do arquivo `file`, no `HEAD` de hoje, que mudaram desde `base` —
/// lidas do `git diff` de verdade, e não supostas a partir do que uma tarefa
/// dizia ir tocar.
fn changed_lines(root: &Path, base: &str, file: &str) -> BTreeSet<usize> {
    let out = mustard_core::platform::git::run(root, &["diff", "--unified=0", base, "HEAD", "--", file]);
    let Some(text) = out.out() else { return BTreeSet::new() };
    let mut lines = BTreeSet::new();
    for hunk in text.lines().filter(|l| l.starts_with("@@")) {
        let Some(plus) = hunk.split_whitespace().nth(2) else { continue };
        let plus = plus.trim_start_matches('+');
        let (start, count) = match plus.split_once(',') {
            Some((s, c)) => (s.parse::<usize>().unwrap_or(0), c.parse::<usize>().unwrap_or(1)),
            None => (plus.parse::<usize>().unwrap_or(0), 1),
        };
        if count == 0 {
            lines.insert(start.max(1));
        } else {
            lines.extend(start..start + count);
        }
    }
    lines
}

/// O nome depois de `fn ` numa linha de assinatura, sem os parênteses que
/// vêm depois.
fn fn_name(line: &str) -> Option<String> {
    let rest = &line[line.find("fn ")? + 3..];
    let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    (!name.is_empty()).then_some(name)
}

/// Cada teste de Rust do texto de hoje, com a linha do `#[test]` e a última
/// linha do corpo dele — a chave dos parênteses do próprio texto, e não um
/// índice guardado à parte.
fn test_spans(content: &str) -> Vec<(String, usize, usize)> {
    let lines: Vec<&str> = content.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim_start().starts_with("#[test]") {
            let attr_line = i + 1;
            let mut j = i + 1;
            while j < lines.len() && j < i + 6 && !lines[j].contains("fn ") {
                j += 1;
            }
            if let Some(name) = lines.get(j).and_then(|line| fn_name(line)) {
                let mut depth = 0i32;
                let mut opened = false;
                let mut end = j;
                for (k, line) in lines.iter().enumerate().skip(j) {
                    for ch in line.chars() {
                        match ch {
                            '{' => {
                                depth += 1;
                                opened = true;
                            }
                            '}' => depth -= 1,
                            _ => {}
                        }
                    }
                    end = k;
                    if opened && depth <= 0 {
                        break;
                    }
                }
                out.push((name, attr_line, end + 1));
            }
        }
        i += 1;
    }
    out
}

/// Os testes de `file` que a obra criou ou mudou: os que [`test_spans`] acha
/// no texto de hoje e cujo trecho — do `#[test]` à última chave do corpo —
/// tem alguma linha que o `git diff` desde a base da obra marca como mudada.
fn changed_test_names(root: &Path, log: &SpecLog, file: &str) -> Vec<String> {
    let Some(base) = obra_base(root, log, file) else { return Vec::new() };
    let Ok(content) = std::fs::read_to_string(root.join(file)) else { return Vec::new() };
    let changed = changed_lines(root, &base, file);
    if changed.is_empty() {
        return Vec::new();
    }
    test_spans(&content)
        .into_iter()
        .filter(|(_, start, end)| changed.iter().any(|line| line >= start && line <= end))
        .map(|(name, _, _)| name)
        .collect()
}

/// O caminho de volta: os testes que as entregas desta obra criaram ou
/// mudaram e que nenhum critério cita na prova dele — cada um vira um aviso
/// pronto, com o nome do teste e o arquivo. Não trava o fechamento.
fn unowned_test_hints(root: &Path, log: &SpecLog, lang: Locale) -> Vec<String> {
    let criteria = criteria_list(log);
    let mut out = Vec::new();
    for file in log.delivered_files() {
        if !file.ends_with(".rs") {
            continue;
        }
        for name in changed_test_names(root, log, &file) {
            if !criteria.iter().any(|(_, _, proof)| proof.contains(&name)) {
                out.push(
                    translate("close.unowned_test", lang).replace("{name}", &name).replace("{file}", &file),
                );
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::flow::round::{round_for, RoundOpts};
    use crate::commands::spec_events::write::{record_open, seed_at, WriteOpts};
    use mustard_core::domain::spec_events::SpecEvent;
    use std::process::Command;
    use tempfile::tempdir;

    fn write(root: &Path, spec: &str, event_type: &str, mut body: Value) -> Value {
        if event_type == "task" {
            let map = body.as_object_mut().expect("a tarefa é um objeto");
            map.entry("files").or_insert_with(|| json!([]));
            map.entry("depends_on").or_insert_with(|| json!([]));
        }
        seed_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: Some(spec.to_string()),
            event_type: event_type.into(),
            json: body.to_string(),
        })
    }

    /// A entrega que o agente da onda grava pela porta do binário, antes de
    /// a rodada assumi-la. Ela precisa sair gravada: a rodada lê a volta da
    /// spec, e não do relatório.
    fn returned(root: &Path, spec: &str, body: Value) {
        let out = crate::commands::spec_events::write::write_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: Some(spec.to_string()),
            event_type: "delivered".into(),
            json: body.to_string(),
        });
        assert_eq!(out["ok"], json!(true), "a volta não gravou: {out}");
    }

    fn id_of(report: &Value) -> u64 {
        report["id"].as_u64().unwrap_or_else(|| panic!("não gravou: {report}"))
    }

    fn git_at(root: &Path, args: &[&str]) {
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

    /// O arquivo da tarefa da onda `n`.
    fn wave_file(n: u64) -> String {
        format!("src/w{n}.rs")
    }

    /// [`ready_to_close`] com `waves` ondas soltas, cada uma com a tarefa num
    /// arquivo dela, despachadas e entregues juntas. A rodada não pede
    /// revisão de onda nenhuma: a entrega já basta, e falta só o fechamento
    /// pedir o agente de teste dedicado.
    fn ready_with_waves(root: &Path, spec: &str, proofs: &[&str], waves: u64) {
        ready_with_checked(root, spec, proofs, waves, &[]);
    }

    /// [`ready_with_waves`] em que as ondas de `checked` voltam só
    /// conferindo: sem arquivo mudado, cada uma sozinha numa rodada depois
    /// das outras, e por isso sem commit.
    fn ready_with_checked(root: &Path, spec: &str, proofs: &[&str], waves: u64, checked: &[u64]) {
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        for n in 1..=waves {
            std::fs::write(root.join(wave_file(n)), "fn um() {}\n").unwrap();
        }
        git_at(root, &["init", "-q"]);
        git_at(root, &["add", "-A"]);
        git_at(root, &["commit", "-q", "-m", "semente"]);
        git_at(root, &["config", "user.email", "t@t"]);
        git_at(root, &["config", "user.name", "t"]);
        git_at(root, &["config", "commit.gpgsign", "false"]);

        assert_eq!(record_open(root, spec, &format!("feature/{spec}"), "dev"), Ok(true));
        let said = id_of(&write(root, spec, "message", json!({"author": "user", "text": "o objetivo"})));
        let _crits: Vec<u64> = proofs
            .iter()
            .map(|proof| {
                id_of(&write(root, spec, "criterion",
                    json!({"when": format!("a onda roda e prova com {proof}"), "then": "a suíte passa",
                           "proof": proof, "form": "ubiquitous", "origin": said})))
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
        let gate = id_of(&write(root, spec, "criterion",
            json!({"when": "a onda roda", "then": "a suíte passa", "proof": "git --version", "form": "ubiquitous",
                "origin": said})));
        for n in 1..=waves {
            write(root, spec, "wave", json!({"n": n, "text": format!("Onda {n}."), "criteria": [gate],
                "done_when": "A suíte passa.", "origin": said}));
            write(root, spec, "task", json!({"wave": n, "text": format!("Tarefa da onda {n}."),
                "files": [{"path": wave_file(n)}], "origin": said}));
        }
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join(spec));
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":4}"#).unwrap();

        let round = |report: Option<String>| {
            round_for(&RoundOpts { root: root.to_path_buf(), spec: Some(spec.to_string()), report }, None)
        };
        let dispatch = round(None);
        assert_eq!(dispatch["ok"], json!(true), "{dispatch}");
        let mut delivered = false;
        for n in (1..=waves).filter(|n| !checked.contains(n)) {
            std::fs::write(root.join(wave_file(n)), "fn um() {}\nfn dois() {}\n").unwrap();
            returned(root, spec, json!({"wave": n, "text": "Saiu.", "files": [wave_file(n)], "commit": "a soma sai"}));
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

    /// O veredito que o revisor grava pela porta do binário. Devolve a
    /// resposta da gravação, com a recusa quando ela recusa.
    fn judged(root: &Path, spec: &str, body: Value) -> Value {
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
    fn verdict_written(root: &Path, spec: &str, body: Value) -> Option<String> {
        let wrote = judged(root, spec, body);
        assert_eq!(wrote["ok"], json!(true), "o veredito não gravou: {wrote}");
        None
    }

    /// O veredito final do agente de teste dedicado aprovando a obra
    /// inteira, gravado por ele.
    fn approve(root: &Path, spec: &str) -> Option<String> {
        verdict_written(root, spec, json!({"final": true, "result": "approved", "text": "A obra está pronta."}))
    }

    /// O fechamento, com a volta transparente do agente de teste dedicado: a
    /// obra pronta para fechar sempre passa por ele agora, mesmo a de uma
    /// onda só, e quem só quer o resultado final não precisa cuidar disso.
    /// Os testes que conferem o pedido dele por dentro chamam `close_for`
    /// direto. Com o pedido já aberto por uma chamada anterior, o fechamento
    /// espera o veredito, e o agente o grava do mesmo jeito.
    fn close(root: &Path, spec: &str) -> Value {
        let out = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some(spec.to_string()), report: None, ..Default::default() }, None);
        if out["review"]["final"] == json!(true) || out["reason"] == json!("review-verdict-missing") {
            return close_for(
                &CloseOpts { root: root.to_path_buf(), spec: Some(spec.to_string()), report: approve(root, spec), ..Default::default() },
                None,
            );
        }
        out
    }

    /// O pedido do agente de revisão final vira evento de envio no
    /// spec.ndjson antes de sair para quem despacha: o texto inteiro, o
    /// papel de revisão e o modelo — pela mesma porta que já grava o pedido
    /// de cada onda, sem onda dona nenhuma.
    #[test]
    fn o_pedido_da_revisao_vira_evento_de_envio() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);

        let path = store::spec_file(root, "x").unwrap();
        let before = store::read(&path).unwrap().unwrap();
        let sent_before = before.visible().into_iter().filter(|e| e.event_type == "send").count();

        let asked =
            close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() }, None);
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        let prompt = asked["review"]["prompt"].as_str().unwrap_or_default().to_string();
        assert!(!prompt.is_empty(), "{asked}");

        let after = store::read(&path).unwrap().unwrap();
        let sent: Vec<&SpecEvent> = after.visible().into_iter().filter(|e| e.event_type == "send").collect();
        assert_eq!(sent.len(), sent_before + 1, "grava exatamente um envio novo: {sent:?}");
        let sent = sent.last().unwrap_or_else(|| panic!("nenhum envio gravado"));
        assert_eq!(sent.str_field("role"), Some("review"), "{sent:?}");
        assert_eq!(sent.str_field("text"), Some(prompt.as_str()), "o texto gravado é o pedido inteiro que voltou: {sent:?}");
        assert!(sent.str_field("model").is_some_and(|m| !m.is_empty()), "o modelo pedido vai junto: {sent:?}");
        assert!(sent.wave().is_none(), "a revisão final não é dona de onda nenhuma: {sent:?}");
    }

    /// O revisor grava o veredito, e o fechamento o assume sem relatório. A
    /// volta gravada antes de o fechamento pedir a revisão é recusada. Pedida
    /// a revisão, o fechamento sem veredito gravado não abre outro pedido: a
    /// resposta manda o revisor gravar o dele. Gravado o veredito, o
    /// fechamento grava o oficial, com `replaces` para a volta, e fecha.
    #[test]
    fn o_fechamento_assume_o_veredito_gravado_pelo_revisor() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let approved = json!({"final": true, "result": "approved", "text": "A obra está pronta."});
        let early = judged(root, "x", approved.clone());
        assert_eq!(early["reason"], json!("no-open-review"), "sem pedido de revisão, a volta não entra: {early}");

        let close = || {
            close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() }, None)
        };
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

    /// O caminho de volta: a onda entrega um teste novo, e o único critério
    /// da spec não o cita na prova dele. O fechamento não trava — a obra
    /// fecha do mesmo jeito —, mas a resposta traz um aviso de teste sem
    /// dono com o nome do teste e o arquivo.
    #[test]
    fn o_fechamento_aponta_o_teste_sem_dono() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let spec = "x";
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        std::fs::write(root.join("src/w1.rs"), "fn um() {}\n").unwrap();
        git_at(root, &["init", "-q"]);
        git_at(root, &["add", "-A"]);
        git_at(root, &["commit", "-q", "-m", "semente"]);
        git_at(root, &["config", "user.email", "t@t"]);
        git_at(root, &["config", "user.name", "t"]);
        git_at(root, &["config", "commit.gpgsign", "false"]);

        assert_eq!(record_open(root, spec, &format!("feature/{spec}"), "dev"), Ok(true));
        let said = id_of(&write(root, spec, "message", json!({"author": "user", "text": "o objetivo"})));
        // Único critério da spec: sempre verde, e não cita o teste que a
        // onda vai entregar.
        let gate = id_of(&write(
            root,
            spec,
            "criterion",
            json!({"when": "a onda roda", "then": "a suíte passa", "proof": "git --version", "form": "ubiquitous",
                "origin": said}),
        ));
        write(
            root,
            spec,
            "wave",
            json!({"n": 1, "text": "Onda 1.", "criteria": [gate], "done_when": "A suíte passa.", "origin": said}),
        );
        write(
            root,
            spec,
            "task",
            json!({"wave": 1, "text": "Tarefa da onda 1.", "files": [{"path": "src/w1.rs"}], "origin": said}),
        );
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join(spec));
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":4}"#).unwrap();

        let round = |report: Option<String>| {
            round_for(&RoundOpts { root: root.to_path_buf(), spec: Some(spec.to_string()), report }, None)
        };
        let dispatch = round(None);
        assert_eq!(dispatch["ok"], json!(true), "{dispatch}");

        std::fs::write(
            root.join("src/w1.rs"),
            "#[cfg(test)]\nmod tests {\n    #[test]\n    fn soma_um_mais_um() { assert_eq!(1 + 1, 2); }\n}\n",
        )
        .unwrap();
        returned(root, spec, json!({"wave": 1, "text": "Saiu.", "files": ["src/w1.rs"], "commit": "soma o teste novo"}));
        let back = round(None);
        assert_eq!(back["ok"], json!(true), "{back}");
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();

        let closed = close(root, spec);
        assert_eq!(closed["phase"], json!("closed"), "{closed}");
        let warnings = closed["warnings"].as_array().cloned().unwrap_or_default();
        let found = warnings
            .iter()
            .find(|w| w["reason"] == json!("unowned-test"))
            .unwrap_or_else(|| panic!("nenhum aviso de teste sem dono: {closed}"));
        let hint = found["hint"].as_str().unwrap_or_default();
        assert!(
            hint.contains("soma_um_mais_um") && hint.contains("src/w1.rs"),
            "o aviso traz o nome do teste e o arquivo: {hint}"
        );
    }

    /// O fechamento roda cada critério uma vez — os dois critérios da spec
    /// aparecem, cada um com uma execução —, grava cada execução, pede o
    /// agente de teste dedicado mesmo com uma onda só, e, aprovado ele, grava
    /// a fase fechada e deixa a pasta da spec com exatamente três arquivos.
    #[test]
    fn closing_runs_each_criterion_once_and_leaves_three_files_in_the_folder() {
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

        let out = close(root, "x");
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["phase"], json!("closed"), "{out}");
        assert!(out.get("review").is_none(), "{out}");

        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let criteria: Vec<u64> =
            log.visible().into_iter().filter(|e| e.event_type == "criterion").map(|e| e.id).collect();
        // Os dois de `proofs`, mais o que cobre a onda na cópia de teste —
        // sempre verde, para a rodada não travar a entrega.
        assert_eq!(criteria.len(), 3, "a montagem tem dois critérios e o que cobre a onda");
        let runs: Vec<&SpecEvent> =
            log.visible().into_iter().filter(|e| e.event_type == "criterion_run").collect();
        let ran: Vec<u64> = runs.iter().filter_map(|e| e.int("criterion")).collect();
        assert_eq!(ran, criteria, "cada critério roda, e uma vez só");
        assert!(runs.iter().all(|e| e.str_field("result") == Some("pass")), "{runs:?}");
        assert_eq!(State::from_log(&log).phase, Some("closed"));

        let folder = root.join(".claude").join("spec").join("x");
        let mut names: Vec<String> = std::fs::read_dir(&folder)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        assert_eq!(names, ["copy", "spec.ndjson"], "a pasta fechada tem o arquivo de eventos e a cópia, e nenhuma página");
        assert!(!root.join(".claude/spec/project.html").exists(), "nem a página do projeto");
    }

    /// A troca do binário instalado do próprio Mustard acontece uma vez só,
    /// aqui: a resposta que ainda pede o agente de teste dedicado — antes da
    /// aprovação final — nunca tenta reinstalar nada, mesmo com a onda já
    /// entregue e comitada. Só a chamada que fecha de fato, depois do
    /// veredito aprovado, chama a reinstalação; com a suíte do projeto
    /// vermelha, o aviso de que o binário instalado continua o de antes é a
    /// prova de que ela rodou.
    #[test]
    fn a_troca_do_binario_acontece_uma_vez_so_no_fechamento_depois_da_aprovacao_final() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        std::fs::create_dir_all(root.join("apps/rt")).unwrap();
        std::fs::write(root.join("apps/rt/Cargo.toml"), b"[package]\nname=\"mustard-rt\"\n").unwrap();
        // A suíte passa enquanto a marca existe: verde na máquina do
        // fechamento, que agora a roda e recusa quando ela cai, e vermelha na
        // reinstalação, depois que a marca sai — é esse vermelho que prova
        // que a reinstalação rodou, sem instalar nada de verdade.
        std::fs::write(root.join("suite-verde"), b"").unwrap();
        std::fs::write(root.join("mustard.json"), br#"{"testCommand":"test -f suite-verde"}"#).unwrap();

        let asked =
            close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() }, None);
        assert_eq!(asked["ok"], json!(true), "{asked}");
        assert_eq!(asked["phase"], json!("running"), "{asked}");
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        let warnings_before = asked["warnings"].as_array().cloned().unwrap_or_default();
        assert!(
            warnings_before.iter().all(|w| w["reason"] != json!("binary-not-reinstalled")),
            "a onda entregue e comitada, mas ainda sem aprovação final, não mexe no binário instalado: {asked}"
        );

        std::fs::remove_file(root.join("suite-verde")).unwrap();
        let out = close_for(
            &CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: approve(root, "x"), ..Default::default() },
            None,
        );
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["phase"], json!("closed"), "{out}");
        let warnings = out["warnings"].as_array().cloned().unwrap_or_default();
        assert!(
            warnings.iter().any(|w| w["reason"] == json!("binary-not-reinstalled")),
            "aprovada a obra, o fechamento tenta reinstalar uma vez, e a suíte vermelha vira aviso: {out}"
        );
    }

    /// A cópia do revisor final nasce pelo binário, no commit da obra, e é
    /// ele que a apaga quando a obra fecha. A cópia que chega com mudança — o
    /// corte que um revisor fez para ver a prova cair e não desfez — trava o
    /// fechamento seguinte, nomeando o arquivo; desfeito o corte, o
    /// fechamento volta a pedir a revisão sobre a mesma cópia.
    #[test]
    fn a_copia_do_revisor_e_criada_e_apagada_pelo_binario() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let copy = mustard_core::io::wave_prompt::final_copy_path(root, "x");
        let shown = mustard_core::io::wave_prompt::shown(&copy);
        assert!(!copy.exists(), "ninguém criou a cópia antes do fechamento");
        let ask = |report: Option<String>| {
            close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report, ..Default::default() }, None)
        };
        let git_out = |dir: &Path, args: &[&str]| {
            let out = Command::new("git").args(args).current_dir(dir).output().expect("git");
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };

        let asked = ask(None);
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        assert!(copy.join(".git").is_file(), "o fechamento criou a cópia como checkout ligado: {asked}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let commit = mustard_core::io::wave_prompt::final_review_commit(root, &log).expect("a obra comitou");
        assert_eq!(git_out(&copy, &["rev-parse", "HEAD"]), commit, "a cópia está no commit da obra");
        let prompt = asked["review"]["prompt"].as_str().unwrap_or_default();
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

        // Aprovada a obra, o binário apaga a cópia, e o git não a lista mais.
        let closed = ask(approve(root, "x"));
        assert_eq!(closed["phase"], json!("closed"), "{closed}");
        assert!(!copy.exists(), "o fechamento apagou a cópia: {closed}");
        assert!(!git_out(root, &["worktree", "list", "--porcelain"]).contains(&shown), "{closed}");
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
        let copy = mustard_core::io::wave_prompt::final_copy_path(root, "x");
        let shown = mustard_core::io::wave_prompt::shown(&copy);
        let ask = |report: Option<String>| {
            close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report, ..Default::default() }, None)
        };
        let regular = |file: &Path| std::fs::symlink_metadata(file).is_ok_and(|m| m.file_type().is_file());

        let asked = ask(None);
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        assert!(regular(&copy.join(".env")), "o arquivo local chega como arquivo comum: {asked}");
        assert_eq!(std::fs::read_to_string(copy.join(".env")).unwrap(), "SEGREDO=1\n");
        std::fs::write(copy.join(".env"), "MUDOU=1\n").unwrap();
        assert_eq!(std::fs::read_to_string(root.join(".env")).unwrap(), "SEGREDO=1\n", "a cópia não escreve no principal");
        let warnings = asked["warnings"].as_array().cloned().unwrap_or_default();
        let missing: Vec<&str> = warnings
            .iter()
            .filter(|w| w["reason"] == json!("local-file-missing"))
            .filter_map(|w| w["hint"].as_str())
            .collect();
        assert_eq!(missing.len(), 1, "{asked}");
        assert!(missing[0].contains("`falta.env`") && missing[0].contains(&shown), "{}", missing[0]);

        std::fs::remove_file(copy.join(".env")).unwrap();
        std::fs::write(root.join(".env"), "SEGREDO=2\n").unwrap();
        let rejected = json!({"final": true, "result": "rejected", "text": "Refazer a conferência."});
        let again = ask(verdict_written(root, "x", rejected));
        assert_eq!(again["review"]["final"], json!(true), "o arquivo local não suja a cópia: {again}");
        assert!(regular(&copy.join(".env")), "{again}");
        assert_eq!(std::fs::read_to_string(copy.join(".env")).unwrap(), "SEGREDO=2\n", "{again}");
        let _ = std::fs::remove_dir_all(mustard_core::io::wave_prompt::copies_dir(root));
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

    /// O que o roteiro gravou para o papel `role`, uma chave por linha.
    fn probed(root: &Path, role: &str) -> std::collections::BTreeMap<String, String> {
        let text = std::fs::read_to_string(root.join(format!("ran-{role}"))).unwrap_or_default();
        text.lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    /// O fechamento roda o lint e a suíte que o `mustard.json` declara — o
    /// lugar só onde o projeto diz o que o servidor roda —, cada um em
    /// ambiente limpo: casa nova e vazia, que some depois, sem a identidade
    /// global do git, sem as variáveis do Claude Code e, no Linux, fora da
    /// árvore de processos de quem chamou. O lint que falha recusa antes da
    /// suíte; a suíte que falha recusa também, antes de critério nenhum; os
    /// dois verdes deixam a máquina seguir para o revisor.
    #[test]
    fn o_fechamento_roda_os_comandos_do_servidor_em_ambiente_limpo() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        std::fs::write(root.join("probe.sh"), PROBE).unwrap();
        let close_with = |lint: &str, test: &str| {
            for role in ["lint", "test"] {
                let _ = std::fs::remove_file(root.join(format!("ran-{role}")));
            }
            std::fs::write(root.join("mustard.json"), json!({ "lintCommand": lint, "testCommand": test }).to_string())
                .unwrap();
            close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), ..Default::default() }, None)
        };

        let refused = close_with("sh probe.sh lint 1", "sh probe.sh test");
        assert_eq!(refused["reason"], json!("lint-failed"), "{refused}");
        assert!(!root.join("ran-test").exists(), "com o lint vermelho, a suíte nem roda");

        let refused = close_with("sh probe.sh lint", "sh probe.sh test 1");
        assert_eq!(refused["reason"], json!("suite-failed"), "a suíte vermelha trava o fechamento: {refused}");
        assert!(refused["hint"].as_str().unwrap_or_default().contains("`sh probe.sh test 1`"), "{refused}");
        assert_eq!(criterion_runs(root), 0, "nenhum critério roda com a suíte vermelha");

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
    fn o_fechamento_roda_os_comandos_do_servidor_em_ambiente_limpo_e_avisa_o_que_falta() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        std::fs::write(root.join("probe.sh"), PROBE).unwrap();
        std::fs::write(root.join("mustard.json"), json!({ "lintCommand": "sh probe.sh lint" }).to_string()).unwrap();
        let asked =
            close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), ..Default::default() }, None);
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        assert!(root.join("ran-lint").is_file(), "o que está declarado roda");
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
    fn a_suite_do_fechamento_nao_usa_o_teto_do_criterio() {
        use crate::commands::review::qa_run::{ceiling_secs, with_timeout_variable, Ceiling};
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
        std::fs::write(root.join("mustard.json"), json!({ "lintCommand": "sleep 2", "testCommand": "sleep 2" }).to_string())
            .unwrap();
        let asked = with_timeout_variable("1", || {
            close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), ..Default::default() }, None)
        });
        assert_ne!(asked["reason"], json!("lint-failed"), "o lint não é cortado pelo teto do critério: {asked}");
        assert_ne!(asked["reason"], json!("suite-failed"), "a suíte não é cortada pelo teto do critério: {asked}");
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
    }

    /// O fechamento roda a prova de cada critério por `run_proof` uma vez, e
    /// a execução grava `pass`.
    #[test]
    fn closing_runs_each_proof_once_and_records_pass() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["echo running 1 test"]);

        let asked =
            close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() }, None);
        assert_eq!(asked["ok"], json!(true), "{asked}");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let runs: Vec<&SpecEvent> =
            log.visible().into_iter().filter(|e| e.event_type == "criterion_run").collect();
        // A prova de `proofs`, mais a do critério que cobre a onda na cópia
        // de teste — sempre verde, para a rodada não travar a entrega.
        assert_eq!(runs.len(), 2, "{runs:?}");
        assert!(runs.iter().all(|r| r.str_field("result") == Some("pass")), "{runs:?}");
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
        let question =
            crate::commands::event::pending::destination_question("P-1", "Medir o antivírus", "x", Locale::PtBr);
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
        // A branch de trabalho da obra: é dela que a pendência nasce, e é por
        // ela que a gravação sabe a obra dona.
        git_at(root, &["checkout", "-q", "-b", "feature/x"]);

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
        let ids = |items: Vec<crate::commands::event::pending::OpenPending>| -> Vec<String> {
            items.into_iter().map(|item| item.id).collect()
        };
        assert_eq!(ids(crate::commands::event::pending::open_pending_born_in(root, "x")), ["P-1", "P-2"]);
        assert!(
            ids(crate::commands::event::pending::open_project_pending(root)).is_empty(),
            "nenhuma é do projeto enquanto a obra corre",
        );

        // O fechamento pede o agente de teste dedicado e faz as perguntas; o
        // usuário responde "fica para depois" só à primeira, com o motivo.
        let asked = close_for(
            &CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() },
            None,
        );
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
        let ledger: Value = serde_json::from_str(
            &std::fs::read_to_string(root.join(".claude/pending/ledger.json")).expect("ledger"),
        )
        .expect("ledger json");
        let item = |id: &str| -> Value {
            ledger["items"]
                .as_array()
                .expect("items")
                .iter()
                .find(|item| item["id"] == json!(id))
                .cloned()
                .expect("item")
        };
        assert_eq!(item("P-1")["owner"], json!("project"), "{ledger}");
        assert_eq!(item("P-1")["born"], json!("x"), "{ledger}");
        assert_eq!(item("P-1")["later"], json!("o antivírus é de outra obra"), "{ledger}");
        assert_eq!(item("P-1")["status"], json!("open"), "solta não é fechada: {ledger}");
        assert!(item("P-2").get("owner").is_none(), "a que ele não soltou continua da obra: {ledger}");

        // A obra já fechada ainda grava a resposta: é depois de ler a
        // pergunta que o usuário responde, e o fechamento não pergunta duas
        // vezes.
        let late = close_for(
            &CloseOpts {
                root: root.to_path_buf(),
                spec: Some("x".into()),
                report: None,
                pending_later: vec!["P-2".into()],
            },
            None,
        );
        assert_eq!(late["ok"], json!(true), "{late}");
        assert_eq!(late["pending_released"], json!([{"id": "P-2", "title": "Trocar o relógio", "owner": "project"}]));
        assert!(ids(crate::commands::event::pending::open_pending_born_in(root, "x")).is_empty());
        assert_eq!(ids(crate::commands::event::pending::open_project_pending(root)), ["P-1", "P-2"]);

        // Sem resposta nenhuma, a obra fechada segue recusando como sempre: a
        // porta não abriu, só a resposta passa por ela.
        let again = close_for(
            &CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() },
            None,
        );
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
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        std::fs::write(root.join(wave_file(1)), "fn um() {}\n").unwrap();
        git_at(root, &["init", "-q"]);
        git_at(root, &["add", "-A"]);
        git_at(root, &["commit", "-q", "-m", "semente"]);
        git_at(root, &["config", "user.email", "t@t"]);
        git_at(root, &["config", "user.name", "t"]);
        git_at(root, &["config", "commit.gpgsign", "false"]);

        assert_eq!(record_open(root, "x", "feature/x", "dev"), Ok(true));
        let said = id_of(&write(root, "x", "message", json!({"author": "user", "text": "o objetivo"})));
        let crit = id_of(&write(root, "x", "criterion", json!({"when": "a onda roda e prova com git --version",
            "then": "a suíte passa", "proof": "git --version", "form": "ubiquitous", "origin": said})));
        // O código de cada decisão sai da ordem em que ela nasce na spec: a
        // primeira decisão gravada ganha o código de número um, a segunda o
        // de número dois. A primeira se liga à tarefa pela palavra-chave e é
        // candidata da onda um; a segunda não serve a onda nenhuma.
        id_of(&write(root, "x", "decision",
            json!({"text": "Sem dono, a onda um leva.", "keys": ["tarefa"], "why": "w", "origin": said})));
        id_of(&write(root, "x", "decision",
            json!({"text": "Sem dono, nenhuma onda leva.", "keys": ["k"], "why": "w", "origin": said})));
        write(root, "x", "wave", json!({"n": 1, "text": "Onda 1.", "criteria": [crit],
            "done_when": "A suíte passa.", "origin": said}));
        write(root, "x", "task", json!({"wave": 1, "text": "Tarefa da onda 1.",
            "files": [{"path": wave_file(1)}], "origin": said}));
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join("x"));
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":4}"#).unwrap();

        let round = |report: Option<String>| {
            round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".to_string()), report }, None)
        };
        // Sem escolha, a onda espera: ela tem item sem dono para julgar.
        let asked = round(None);
        assert_eq!(asked["ok"], json!(true), "{asked}");
        assert!(asked.get("dispatch").is_none() || asked["dispatch"].as_array().is_some_and(Vec::is_empty), "{asked}");

        // A escolha do orquestrador leva só a primeira decisão sem dono.
        let added = json!([{"item": "MSTD-DEC-0001", "why": "Ela entra na onda um."}]);
        let analysis = json!({"wave": 1, "removed": [], "added": added});
        let dispatched = round(Some(format!("<ANALYSIS>{analysis}</ANALYSIS>")));
        assert_eq!(dispatched["ok"], json!(true), "{dispatched}");

        std::fs::write(root.join(wave_file(1)), "fn um() {}\nfn dois() {}\n").unwrap();
        // A entrega responde pela decisão que o pedido da onda levou.
        returned(root, "x", json!({"wave": 1, "text": "Saiu.", "files": [wave_file(1)], "commit": "a soma sai",
            "agreed": [{"item": "MSTD-DEC-0001", "met": true}]}));
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
    fn a_aceitacao_grava_a_tabela_de_rastreabilidade() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        std::fs::write(root.join(wave_file(1)), "fn um() {}\n").unwrap();
        git_at(root, &["init", "-q"]);
        git_at(root, &["add", "-A"]);
        git_at(root, &["commit", "-q", "-m", "semente"]);
        git_at(root, &["config", "user.email", "t@t"]);
        git_at(root, &["config", "user.name", "t"]);
        git_at(root, &["config", "commit.gpgsign", "false"]);

        assert_eq!(record_open(root, "x", "feature/x", "dev"), Ok(true));
        let said = id_of(&write(root, "x", "message", json!({"author": "user", "text": "o objetivo"})));
        let crit = id_of(&write(root, "x", "criterion", json!({"when": "a onda roda e prova com git --version",
            "then": "a suíte passa", "proof": "git --version", "form": "ubiquitous", "origin": said})));
        let rule = id_of(&write(root, "x", "rule", json!({"text": "A trava confere o programa.",
            "keys": ["trava"], "example": "rm -rf pasta é barrado.", "origin": said})));
        let dec = id_of(&write(root, "x", "decision",
            json!({"text": "Sem prova extra.", "keys": ["k"], "why": "w", "origin": said})));
        write(root, "x", "wave", json!({"n": 1, "text": "Onda 1.", "criteria": [crit],
            "done_when": "A suíte passa.", "origin": said}));
        write(root, "x", "task", json!({"wave": 1, "text": "Tarefa da onda 1.",
            "files": [{"path": wave_file(1)}], "origin": said}));
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join("x"));
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":4}"#).unwrap();

        let round = |report: Option<String>| {
            round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".to_string()), report }, None)
        };
        // Com a regra e a decisão sem dono, a onda espera a escolha do
        // orquestrador; sem item a acrescentar, ela sai como está.
        round(None);
        let analysis = json!({"wave": 1, "removed": [], "added": []});
        let dispatched = round(Some(format!("<ANALYSIS>{analysis}</ANALYSIS>")));
        assert_eq!(dispatched["ok"], json!(true), "{dispatched}");
        std::fs::write(root.join(wave_file(1)), "fn um() {}\nfn dois() {}\n").unwrap();
        returned(root, "x", json!({"wave": 1, "text": "Saiu.", "files": [wave_file(1)], "commit": "a soma sai"}));
        let back = round(None);
        assert_eq!(back["ok"], json!(true), "{back}");
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();

        let asked = close_for(
            &CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() },
            None,
        );
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
            &CloseOpts {
                root: root.to_path_buf(),
                spec: Some("x".into()),
                report: verdict_written(root, "x", approval),
                ..Default::default()
            },
            None,
        );
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["phase"], json!("closed"), "{out}");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let tracking =
            log.visible().into_iter().find(|e| e.event_type == "tracking").expect("a tabela ficou gravada");
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
    fn close_with_lint(root: &Path, lint: &str, report: Option<String>) -> Value {
        std::fs::write(root.join("mustard.json"), json!({ "lintCommand": lint }).to_string()).unwrap();
        close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report, ..Default::default() }, None)
    }

    /// Quantas execuções de critério a spec tem.
    fn criterion_runs(root: &Path) -> usize {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        log.visible().iter().filter(|e| e.event_type == "criterion_run").count()
    }

    /// O fechamento roda o lint do projeto inteiro. O lint que falha recusa
    /// com a saída dele, antes de critério nenhum rodar. O que passa roda no
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
        let ran = |root: &Path| root.join("lint-rodou").is_dir();

        // O lint que falha recusa com a saída dele, antes de critério nenhum
        // rodar.
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_with_waves(root, "x", &["git --version"], 2);
        let refused = close_with_lint(root, "git lint-que-nao-existe", None);
        assert_eq!(refused["reason"], json!("lint-failed"), "{refused}");
        let hint = refused["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("git lint-que-nao-existe") && hint.contains("lint-que-nao-existe' is not a git command"), "{hint}");
        assert_eq!(criterion_runs(root), 0, "nenhum critério roda com o lint vermelho");

        // O lint que passa roda no projeto, e a máquina verde pede o agente
        // de teste dedicado.
        let asked = close_with_lint(root, lint, None);
        assert_eq!(asked["ok"], json!(true), "{asked}");
        assert_eq!(asked["phase"], json!("running"), "{asked}");
        assert!(ran(root), "o lint rodou antes do agente");
        // O critério de `proofs`, mais o que cobre as ondas na cópia de
        // teste — sempre verde, para a rodada não travar a entrega.
        assert_eq!(asked["criteria"].as_array().map(Vec::len), Some(2), "{asked}");
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        let prompt = asked["review"]["prompt"].as_str().unwrap_or_default();
        assert!(prompt.contains(translate("prompt.final.fixed", Locale::PtBr)), "{prompt}");
        assert!(prompt.contains("código repetido entre ondas") && prompt.contains("verificação que uma apagou da outra"), "{prompt}");
        for n in [1, 2] {
            assert!(prompt.contains(&format!("MSTD-WAVE-000{n}")), "a onda {n} está no pedido: {prompt}");
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
        std::fs::write(root.join(wave_file(2)), "fn um() {}\nfn tres() {}\n").unwrap();
        let line = json!({"wave": 2, "text": "Sem repetir a 1.", "files": [wave_file(2)], "commit": "a onda 2 sem repetição"});
        returned(root, "x", line);
        let back = round(None);
        assert_eq!(back["ok"], json!(true), "{back}");
        assert!(back.get("reviews").is_none(), "a rodada não pede revisão do conserto: {back}");

        // Depois do conserto, a máquina roda de novo, e o pedido volta —
        // agora só com o conserto, não a obra inteira de novo.
        std::fs::remove_dir_all(root.join("lint-rodou")).unwrap();
        let again = close_with_lint(root, lint, None);
        assert_eq!(again["review"]["final"], json!(true), "{again}");
        assert!(ran(root), "{again}");
        let fix_prompt = again["review"]["prompt"].as_str().unwrap_or_default();
        assert!(fix_prompt.contains(translate("prompt.fix.final", Locale::PtBr)), "{fix_prompt}");
        assert!(fix_prompt.contains("MSTD-WAVE-0002") && !fix_prompt.contains("MSTD-WAVE-0001"), "só o conserto: {fix_prompt}");

        // Aprovado, a spec fecha sem rodar a máquina outra vez.
        std::fs::remove_dir_all(root.join("lint-rodou")).unwrap();
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
        let finals: Vec<&SpecEvent> = log
            .visible()
            .into_iter()
            .filter(|e| e.event_type == "verdict" && e.fields.get("final") == Some(&json!(true)))
            .collect();
        let results: Vec<Option<&str>> = finals.iter().map(|e| e.str_field("result")).collect();
        assert_eq!(results, [Some("rejected"), Some("approved")], "as duas revisões finais ficaram gravadas");
        assert!(finals.iter().all(|e| e.fields.get("criteria").is_none()), "e nenhuma delas confere critério");
    }

    /// O agente de teste dedicado fecha toda obra: a de duas ondas, que
    /// entrega as duas, e a de uma onda só, que entrega a dela. A rodada
    /// nunca pede revisão de onda nenhuma, o fechamento pede o agente
    /// dedicado com um pedido que leva as entregas da obra, e quando ele
    /// aponta um problema numa onda, o conserto sai pela rodada e o
    /// fechamento pede o mesmo agente de novo, só para conferir o conserto —
    /// em até duas voltas; na terceira reprovação seguida, a onda para e a
    /// decisão passa a ser do usuário.
    #[test]
    fn the_dedicated_test_agent_closes_every_work() {
        let waves_in = |out: &Value, field: &str| -> Vec<u64> {
            out[field].as_array().cloned().unwrap_or_default().iter().filter_map(|d| d["wave"].as_u64()).collect()
        };

        // A obra de duas ondas: as duas entregam, sem revisão nenhuma da
        // rodada, e o fechamento pede o agente dedicado, com as duas
        // entregas no pedido.
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_with_waves(root, "x", &["git --version"], 2);
        let asked = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() }, None);
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        let prompt = asked["review"]["prompt"].as_str().unwrap_or_default();
        for n in [1, 2] {
            assert!(prompt.contains(&format!("MSTD-DELIV-000{n}")), "a entrega da onda {n} está no pedido: {prompt}");
        }

        // A obra de uma onda só entrega a dela e passa pela mesma exigência.
        let solo = tempdir().unwrap();
        let solo_root = solo.path();
        ready_to_close(solo_root, "x", &["git --version"]);
        let asked_solo = close_for(&CloseOpts { root: solo_root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() }, None);
        assert_eq!(asked_solo["review"]["final"], json!(true), "uma onda só também pede o agente: {asked_solo}");
        let solo_prompt = asked_solo["review"]["prompt"].as_str().unwrap_or_default();
        assert!(solo_prompt.contains("MSTD-DELIV-0001"), "a entrega da onda única está no pedido: {solo_prompt}");
        let round_solo =
            round_for(&RoundOpts { root: solo_root.to_path_buf(), spec: Some("x".into()), report: None }, None);
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
        std::fs::write(root.join(wave_file(2)), "fn um() {}\nfn tres() {}\n").unwrap();
        let line = json!({"wave": 2, "text": "Consertou.", "files": [wave_file(2)], "commit": "conserta a onda 2"});
        returned(root, "x", line);
        let back = round(None);
        assert_eq!(back["ok"], json!(true), "{back}");
        assert!(back.get("reviews").is_none(), "a rodada não pede revisão do conserto: {back}");

        // Entregue o conserto, o fechamento pede o mesmo agente de novo, só
        // para conferir o conserto — não a obra inteira de novo.
        let rechecked = close(None);
        assert_eq!(rechecked["review"]["final"], json!(true), "{rechecked}");
        let fix_prompt = rechecked["review"]["prompt"].as_str().unwrap_or_default();
        assert!(fix_prompt.contains(translate("prompt.fix.final", Locale::PtBr)), "{fix_prompt}");
        assert!(fix_prompt.contains("MSTD-WAVE-0002") && !fix_prompt.contains("MSTD-WAVE-0001"), "só o conserto: {fix_prompt}");

        // A segunda volta de conserto: reprovado de novo, o conserto sai mais
        // uma vez.
        let refused_again = close(reject("Ainda falta."));
        assert_eq!(refused_again["reason"], json!("wave-rejected"), "{refused_again}");
        let fix_again = round(None);
        assert_eq!(waves_in(&fix_again, "dispatch"), vec![2], "{fix_again}");
        std::fs::write(root.join(wave_file(2)), "fn um() {}\nfn quatro() {}\n").unwrap();
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
    /// dedicado reprova a onda 1 de duas, e a aprovação final, sem onda, fica
    /// gravada na onda 2, a última — nunca ganha um veredito próprio a onda 1.
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
        std::fs::write(root.join(wave_file(1)), "fn um() {}\nfn tres() {}\n").unwrap();
        let line = json!({"wave": 1, "text": "Sem faltar o teste.", "files": [wave_file(1)], "commit": "conserta a onda 1"});
        returned(root, "x", line);
        let back = round(None);
        assert_eq!(back["ok"], json!(true), "{back}");

        // (2) Entregue o conserto, a fila solta a onda: a rodada não tem mais
        // nada a despachar nem a esperar, e manda fechar.
        let released = round(None);
        assert_eq!(released["command"], json!("mustard-rt run close --spec x"), "{released}");

        // O agente de teste dedicado confere só o conserto, e aprova a obra
        // inteira: a aprovação sem onda fica gravada na última onda do plano
        // (a 2), não na 1, que foi a reprovada.
        let fix_prompt = close(None)["review"]["prompt"].as_str().unwrap_or_default().to_string();
        assert!(fix_prompt.contains("MSTD-WAVE-0001") && !fix_prompt.contains("MSTD-WAVE-0002"), "{fix_prompt}");
        let approved = json!({"final": true, "result": "approved", "text": "O conserto ficou certo."});
        let closed = close(verdict_written(root, "x", approved));
        assert_eq!(closed["phase"], json!("closed"), "{closed}");

        // (3) Nenhuma onda fica com o estado de reprovada, nem a 1, cujo
        // último veredito próprio continua sendo a reprovação: a aprovação
        // que fechou a obra não é dela.
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let states = crate::commands::flow::round::wave_states(&log);
        let rejected: Vec<u64> = states
            .into_iter()
            .filter(|(_, s)| *s == mustard_core::view::document::WaveState::Rejected)
            .map(|(n, _)| n)
            .collect();
        assert_eq!(rejected, Vec::<u64>::new(), "{closed}");
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
        draft.insert("criteria".to_string(), json!([{"criterion": criterion_id, "tests_rule": true}]));
        record(root, "x", "verdict", draft, PhaseWriter::Binary).expect("veredito antigo gravado");

        // A rodada não vê conserto pendente: nada a despachar.
        let round = round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None }, None);
        assert_eq!(round["dispatch"].as_array().map(Vec::len).unwrap_or_default(), 0, "{round}");

        // O fechamento pede o agente de teste dedicado com o pedido da obra
        // inteira, sem entrar em modo de conserto.
        let asked = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() }, None);
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        let prompt = asked["review"]["prompt"].as_str().unwrap_or_default();
        assert!(!prompt.contains(translate("prompt.fix.final", Locale::PtBr)), "não é modo de conserto: {prompt}");
        for n in [1, 2] {
            assert!(prompt.contains(&format!("MSTD-WAVE-000{n}")), "a onda {n} está no pedido: {prompt}");
        }
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

        let rounded = crate::commands::flow::round::round_for(
            &crate::commands::flow::round::RoundOpts {
                root: root.to_path_buf(),
                spec: Some("x".into()),
                report: None,
            },
            None,
        );
        // Lida antes do fechamento: a pasta dos lotes é refeita a cada
        // marco, e o fechamento reconstrói a dele por cima.
        let round_next = full_next(root, "x", "round", &rounded);
        let closed = close(root, "x");

        for (report, milestone, next) in
            [(&rounded, "round", round_next), (&closed, "close", closed["next"].as_str().unwrap_or_default().to_string())]
        {
            assert_eq!(report["publish"], json!(["spec", "project"]), "{report}");
            for page in ["spec", "project"] {
                let record = format!(r#"'{{"page":"{page}","milestone":"{milestone}","#);
                assert!(next.contains(&record), "{milestone} says how to record the {page} page: {next}");
            }
            assert!(next.contains("write copy"), "{milestone}: {next}");
            let shown = report.to_string();
            assert!(!shown.contains("http"), "nenhum endereço na resposta: {shown}");
        }

        // Os passos comuns, numa spec em levantamento.
        let other = tempdir().unwrap();
        let root = other.path();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        assert_eq!(record_open(root, "y", "feature/y", "dev"), Ok(true));
        let said = write(root, "y", "message", json!({"author": "user", "text": "Quero a busca de lições."}));
        let context = write(root, "y", "context", json!({"text": "Quero a busca de lições.", "origin": id_of(&said)}));
        let grilled = crate::commands::flow::grill::grill_for(
            &crate::commands::flow::grill::GrillOpts {
                root: root.to_path_buf(),
                spec: Some("y".into()),
                kinds: Some("feature".into()),
                condensed: false,
            },
            None,
        );
        assert_eq!(grilled["ok"], json!(true), "{grilled}");
        for report in [&grilled, &context] {
            assert!(report.get("publish").is_none(), "um passo comum não manda publicar: {report}");
            assert!(report.get("copy").is_none(), "nem copiar: {report}");
            let shown = report.to_string();
            assert!(!shown.contains("write publish"), "um passo comum não manda publicar: {shown}");
            assert!(!shown.contains("write copy"), "nem copiar: {shown}");
        }
    }

    /// O fim do `next` de cada marco, com o comando que a resposta devolve: a
    /// rodada com tudo entregue e aprovado manda fechar, e o fechamento manda
    /// abrir o pull request.
    fn then_of(report: &Value, key: &str) -> String {
        let command = report["command"].as_str().unwrap_or_else(|| panic!("sem comando: {report}"));
        translate(key, Locale::PtBr).replace("{command}", command)
    }

    /// O `next` do marco `milestone`, com a instrução de publicar e copiar
    /// por extenso: na rodada, ela sai do arquivo que a resposta manda ler,
    /// porque a resposta em si leva só a linha curta.
    fn full_next(root: &Path, spec: &str, milestone: &str, report: &Value) -> String {
        let next = report["next"].as_str().unwrap_or_default().to_string();
        if milestone != "round" {
            return next;
        }
        let file = root.join(".claude").join("spec").join(spec).join("copy").join("next.md");
        std::fs::read_to_string(&file).map_or(next.clone(), |order| format!("{order} {next}"))
    }

    /// Com um item de texto que parece senha, a rodada e o fechamento mandam
    /// publicar e copiar assim mesmo e dizem o código do item a expurgar; o
    /// item fica fora da cópia.
    #[test]
    fn a_withheld_item_is_named_and_no_longer_holds_the_publish_of_the_round_and_the_close() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let said = write(root, "x", "message", json!({"author": "user", "text": "anota"}));
        let note = write(root, "x", "note",
            json!({"text": "GITHUB_TOKEN=a1b2c3d4e5f6g7h8i9j0", "keys": ["token"], "origin": id_of(&said)}));
        let code = note["code"].as_str().unwrap_or_default().to_string();

        // A ordem por extenso da rodada é lida do arquivo antes do
        // fechamento rodar: a pasta dos lotes é refeita a cada marco, e o
        // fechamento reconstrói a dele por cima.
        let rounded = round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None }, None);
        let round_then = then_of(&rounded, "round.close");
        let round_next = full_next(root, "x", "round", &rounded);
        let closed = close(root, "x");
        let close_then = then_of(&closed, "close.next");

        for (report, milestone, next, then) in [
            (&rounded, "round", round_next, round_then),
            (&closed, "close", closed["next"].as_str().unwrap_or_default().to_string(), close_then),
        ] {
            assert_eq!(report["ok"], json!(true), "{report}");
            assert_eq!(report["publish"], json!(["spec", "project"]), "{milestone}: {report}");
            assert_eq!(report["withheld"], json!([code]), "{milestone}: {report}");
            assert!(next.contains(&code) && next.contains("write purge"), "{milestone}: {next}");
            assert!(next.contains("write publish") && next.ends_with(&then), "{milestone}: {next}");
            let warned = report["warnings"].as_array().cloned().unwrap_or_default();
            assert!(warned.iter().any(|w| w["hint"].as_str().unwrap_or_default().contains(&code)), "{report}");
            let items = crate::commands::spec_events::pages::copy::sent_items(root, report);
            assert!(!items.contains(&id_of(&note)), "{milestone}: the item stays out of the copy");
        }
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
        // Um arquivo no lugar da pasta da cópia impede de prepará-la.
        let folder = root.join(".claude/spec/x/copy");
        std::fs::remove_dir_all(&folder).unwrap();
        std::fs::write(&folder, "não é pasta").unwrap();

        let rounded = round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None }, None);
        let closed = close(root, "x");
        let failed = translate("page.copy.failed", Locale::PtBr);
        for (report, milestone, then) in
            [(&rounded, "round", then_of(&rounded, "round.close")), (&closed, "close", then_of(&closed, "close.next"))]
        {
            assert_eq!(report["ok"], json!(true), "{milestone}: {report}");
            assert!(report.get("publish").is_none() && report.get("copy").is_none(), "{milestone}: {report}");
            let next = report["next"].as_str().unwrap_or_default();
            assert!(!next.contains("write publish") && !next.contains("write copy"), "{milestone}: {next}");
            assert_eq!(next, format!("{failed} {then}"), "{milestone}");
            let warned = report["warnings"].as_array().cloned().unwrap_or_default();
            assert!(warned.iter().any(|w| w["reason"] == json!("io-failed")), "{milestone} says why: {report}");
        }
    }

    /// O fechamento devolve a linha inteira do pull request, com a base e a
    /// branch da spec, o binário a aceita, e o próximo passo em palavras a
    /// traz.
    #[test]
    fn closing_answers_the_whole_pull_request_line() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);

        let out = close(root, "x");
        assert_eq!(out["ok"], json!(true), "{out}");
        let line = "mustard-rt run pr-open --base dev --head feature/x --spec x";
        assert_eq!(out["command"], json!(line), "{out}");
        crate::commands::flow::resume::assert_parses(line);
        assert!(out["next"].as_str().unwrap_or_default().ends_with(&then_of(&out, "close.next")), "{out}");
    }

    /// Ao gravar a fase fechada, o fechamento arma a cobrança das pendências
    /// pela mesma porta que grava a fase.
    #[test]
    fn closing_arms_the_charge_of_the_pending_items() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        assert_eq!(close_for(
            &CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None, ..Default::default() },
            Some("s-fecha"),
        )["review"]["final"], json!(true));
        let closed = close_for(
            &CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: approve(root, "x"), ..Default::default() },
            Some("s-fecha"),
        );
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
        let crit = id_of(&write(root, "x", "criterion",
            json!({"when": "a onda roda", "then": "passa", "proof": "git --version", "form": "ubiquitous",
                "origin": said})));
        write(root, "x", "wave", json!({"n": 2, "text": "Onda 2.", "criteria": [crit],
            "done_when": "passa", "origin": said}));
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

    /// O fechamento com tarefa no backlog recusa com a razão própria e nomeia a
    /// tarefa; tirada a tarefa da spec, o mesmo fechamento passa.
    #[test]
    fn o_fechamento_recusa_com_tarefa_no_backlog() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let said = id_of(&write(root, "x", "message", json!({"author": "user", "text": "mais uma"})));
        let task = id_of(&write(root, "x", "task", json!({"text": "Tarefa que ficou no backlog.",
            "files": [{"path": "src/w1.rs"}], "depends_on": [], "origin": said})));
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
        let wave = id_of(&write(root, "x", "wave", json!({"n": 2, "text": "Onda 2.", "criteria": [crit],
            "done_when": "passa", "origin": said})));
        let task = id_of(&write(root, "x", "task", json!({"wave": 2, "text": "Tarefa.",
            "files": [{"path": "src/a.rs"}], "origin": said})));
        for attempt in 0..3 {
            crate::shared::spec_state::seed_event(root, "x", "send", json!({"wave": 2, "role": "wave",
                "text": "pedido", "lines": 1, "chars": 6, "items": [wave], "mustard": "0", "author": "binary"}));
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
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"prova\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/lib.rs"),
            "pub fn soma(a: u32, b: u32) -> u32 { a + b }\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn soma_de_dois() { assert_eq!(super::soma(1, 1), 2); }\n}\n",
        )
        .unwrap();
        std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
        ready_to_close(
            root,
            "x",
            &["cargo test --lib -- tests::soma_de_dois --exact", "cargo test --lib -- tests::soma --exact"],
        );

        let refused = close(root, "x");
        assert_eq!(refused["reason"], json!("criterion-ran-no-test"), "{refused}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let criteria: Vec<u64> = log.visible().into_iter().filter(|e| e.event_type == "criterion").map(|e| e.id).collect();
        let wrong_name = "cargo test --lib -- tests::soma --exact";
        let expected = translate("close.criterion_ran_no_test", Locale::PtBr)
            .replace("{code}", &codes[&criteria[1]])
            .replace("{command}", wrong_name)
            .replace("{count}", "0");
        assert_eq!(refused["hint"], json!(expected), "{refused}");
        let hint = refused["hint"].as_str().unwrap_or_default();
        assert!(hint.contains(wrong_name) && hint.contains('0'), "a recusa diz o comando e o número: {hint}");
        let runs: Vec<(Option<u64>, Option<&str>)> = log
            .visible()
            .into_iter()
            .filter(|e| e.event_type == "criterion_run")
            .map(|e| (e.int("criterion"), e.str_field("result")))
            .collect();
        // O terceiro é o critério que cobre a onda na cópia de teste — sempre
        // verde, para a rodada não travar a entrega.
        assert_eq!(
            runs,
            vec![(Some(criteria[0]), Some("pass")), (Some(criteria[1]), Some("fail")), (Some(criteria[2]), Some("pass"))]
        );
        assert_eq!(State::from_log(&log).phase, Some("running"), "a spec não fechou");
    }

    /// A leitura do número de testes não é só do cargo: a prova que roda outro
    /// executor e sai verde dizendo zero teste trava o fechamento do mesmo
    /// jeito, com o comando e o número na recusa; a que roda pelo menos um
    /// teste passa.
    #[test]
    fn a_proof_that_ran_zero_tests_outside_cargo_blocks_the_close_too() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let zero = "echo Tests: 0 total";
        ready_to_close(root, "x", &["echo Tests: 3 total", zero]);

        let refused = close(root, "x");
        assert_eq!(refused["reason"], json!("criterion-ran-no-test"), "{refused}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let criteria: Vec<u64> = log.visible().into_iter().filter(|e| e.event_type == "criterion").map(|e| e.id).collect();
        let expected = translate("close.criterion_ran_no_test", Locale::PtBr)
            .replace("{code}", &codes[&criteria[1]])
            .replace("{command}", zero)
            .replace("{count}", "0");
        assert_eq!(refused["hint"], json!(expected), "{refused}");
        let runs: Vec<(Option<u64>, Option<&str>)> = log
            .visible()
            .into_iter()
            .filter(|e| e.event_type == "criterion_run")
            .map(|e| (e.int("criterion"), e.str_field("result")))
            .collect();
        // O terceiro é o critério que cobre a onda na cópia de teste — sempre
        // verde, para a rodada não travar a entrega.
        assert_eq!(
            runs,
            vec![(Some(criteria[0]), Some("pass")), (Some(criteria[1]), Some("fail")), (Some(criteria[2]), Some("pass"))]
        );
        assert_eq!(State::from_log(&log).phase, Some("running"), "a spec não fechou");
    }

    /// Os dois executores que dizem zero sem escrever número são lidos pela
    /// linha de resumo de cada um, e não por uma frase qualquer. O vitest sai
    /// com código 0 quando o filtro por nome não casa teste nenhum e escreve
    /// `Tests  no tests`: essa prova é recusada. O go escreve a marca dele por
    /// pacote, então a prova em que um pacote não rodou teste ao lado de outro
    /// que rodou passa — a corrida rodou teste. E a execução recusada guarda o
    /// que o executor escreveu, e não uma frase montada sobre ela.
    #[test]
    fn a_proof_that_ran_zero_tests_is_read_by_the_summary_line_of_each_runner() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let go_mixed = "echo ok x/pkg 0.002s [no tests to run] && echo ok x/outro 0.02s";
        let vitest_zero = "echo Tests no tests";
        ready_to_close(root, "x", &[go_mixed, vitest_zero]);

        let refused = close(root, "x");
        assert_eq!(refused["reason"], json!("criterion-ran-no-test"), "{refused}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let criteria: Vec<u64> = log.visible().into_iter().filter(|e| e.event_type == "criterion").map(|e| e.id).collect();
        let expected = translate("close.criterion_ran_no_test", Locale::PtBr)
            .replace("{code}", &codes[&criteria[1]])
            .replace("{command}", vitest_zero)
            .replace("{count}", "0");
        assert_eq!(refused["hint"], json!(expected), "{refused}");
        let runs: Vec<(Option<&str>, Option<&str>)> = log
            .visible()
            .into_iter()
            .filter(|e| e.event_type == "criterion_run")
            .map(|e| (e.str_field("result"), e.str_field("output")))
            .collect();
        // O terceiro é o critério que cobre a onda na cópia de teste —
        // sempre verde, para a rodada não travar a entrega.
        assert_eq!(runs.len(), 3, "os dois critérios de `proofs` e o da onda rodaram: {runs:?}");
        assert_eq!(runs[0], (Some("pass"), None), "o go com um pacote sem teste e outro com teste passa");
        assert_eq!(runs[1].0, Some("fail"));
        assert_eq!(
            runs[1].1,
            Some("Tests no tests"),
            "a execução recusada guarda o que o executor escreveu: {runs:?}"
        );
        assert_eq!(runs[2].0, Some("pass"), "{runs:?}");
        assert_eq!(State::from_log(&log).phase, Some("running"), "a spec não fechou");
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
        let runs: Vec<Option<&str>> = log
            .visible()
            .into_iter()
            .filter(|e| e.event_type == "criterion_run")
            .map(|e| e.str_field("result"))
            .collect();
        // O segundo é o critério que cobre a onda na cópia de teste — sempre
        // verde, para a rodada não travar a entrega.
        assert_eq!(runs, vec![Some("pass"), Some("pass")], "a prova que não é teste passou: {runs:?}");
        assert_eq!(State::from_log(&log).phase, Some("closed"));
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
        let runs: Vec<&SpecEvent> =
            log.visible().into_iter().filter(|e| e.event_type == "criterion_run").collect();
        // O segundo é o critério que cobre a onda na cópia de teste — sempre
        // verde, para a rodada não travar a entrega.
        assert_eq!(runs.len(), 2, "a execução fica gravada");
        assert_eq!(runs[0].str_field("result"), Some("fail"));
        assert_eq!(runs[1].str_field("result"), Some("pass"));
        assert_eq!(State::from_log(&log).phase, Some("running"), "a spec não fechou");
    }
}
