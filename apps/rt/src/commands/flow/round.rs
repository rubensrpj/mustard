//! `mustard-rt run round [--spec <nome>]` — uma rodada de ondas.
//!
//! É a porta única da execução, e cada rodada é uma chamada só. Sem relatório,
//! a rodada despacha: escolhe as ondas que podem sair juntas, monta o pedido
//! de cada uma, grava o envio com o pedido exato como foi injetado e marca a
//! spec como em execução na primeira rodada. Com o relatório da rodada
//! anterior (`--report`), ela primeiro fecha o que voltou e só então despacha
//! a rodada seguinte.
//!
//! **O relatório é o que os agentes devolvem, como veio.** A rodada lê, do
//! texto recebido, cada linha `<DELIVERED>{…}</DELIVERED>` do agente de onda e
//! cada linha `<VERDICT>{…}</VERDICT>` do revisor, no formato que os textos
//! deles ensinam. A linha da entrega traz a onda, a entrega, os arquivos, o
//! resumo do commit e, quando é o caso, a prova nova de um critério cujo teste
//! mudou de nome, as ondas que o conserto fecha e a mudança de plano; a do
//! veredito traz a onda, o resultado, o texto e cada critério pelo código que
//! a página mostra. Com isso a rodada grava o veredito e depois a entrega —
//! também na onda que o conserto fecha, o que pede a revisão dela de novo —,
//! grava a versão nova do critério com a prova nova, formata só os arquivos da
//! rodada e faz o commit com a mensagem montada do resumo.
//!
//! **O que trava.** Uma spec que ainda não foi aprovada; um relatório sem
//! nenhuma das duas linhas, ou com uma linha sem campo obrigatório; um
//! `entregou` acima do teto de caracteres; um arquivo entregue que está
//! reservado para outra onda em andamento, ou que não está no disco nem no
//! git; uma mensagem de commit fora do
//! modelo (título e corpo acima do teto, link do claude.ai, o nome do modelo,
//! assinatura de coautoria ou e-mail de alguém); o relatório em que um agente
//! diz que o plano da onda não funciona, que para a rodada e só segue com o
//! "sim" do usuário. O "sim" da mudança de plano é o clique em "Aceitar" na
//! pergunta dela, gravado pela testemunha como na aprovação da spec, e nunca a
//! leitura que o modelo faz de uma frase: a rodada não aceita código nenhum de
//! quem a chama.
//!
//! **O que para sem travar.** A onda reprovada depois da segunda rodada de
//! conserto segura só ela e as ondas que dependem dela: o resto da rodada
//! segue, e a resposta traz a pergunta ao usuário com os vereditos dela. A
//! onda que sai do plano deixa de contar, na rodada e no fechamento.
//!
//! **O que avisa.** O formatador que o projeto declara e que não foi achado
//! sai pelo nome, em vez de a formatação ser pulada em silêncio; e a prova
//! nova que sai verde sem rodar teste nenhum sai pelo código do critério.
//!
//! A página da spec e a do projeto são refeitas no fim da rodada, e a resposta
//! manda publicá-las: a rodada é um dos marcos de publicação. Nenhum endereço
//! é impresso na conversa.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use mustard_core::platform::git as git_exec;

use mustard_core::domain::spec_events::{
    check_message, normalize, validate, Block, BlockQuery, MessageRefusal, Refusal, SpecEvent, SpecLog,
    DELIVERED_MAX_CHARS, MESSAGE_BODY_MAX, MESSAGE_TITLE_MAX,
};
use mustard_core::domain::spec_state::{PhaseWriter, SpecState, State};
use mustard_core::io::spec_events as store;
use mustard_core::io::wave_prompt::prompts;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Map, Value};

use crate::commands::spec_events::{self, read::checkout, write::record};
use crate::commands::wave::wave_overlap_check::wave_graph;
use crate::shared::spec_state::{session_from_env, DiskSpecState};

/// As opções de `mustard-rt run round`.
pub struct RoundOpts {
    /// Qualquer pasta dentro do repositório.
    pub root: PathBuf,
    /// A spec cuja rodada corre; sem ela, a spec atual.
    pub spec: Option<String>,
    /// O relatório da rodada anterior, em JSON.
    pub report: Option<String>,
}

/// Quantas ondas saem juntas quando o projeto não diz outra coisa: duas, que é
/// quanto a máquina aguenta compilando ao mesmo tempo.
const DEFAULT_PARALLEL: usize = 2;

/// Quantas rodadas de conserto uma onda tem. A reprovação que vem depois da
/// última delas para a onda e as que dependem dela: o problema é de desenho,
/// e vai ao usuário.
const MAX_FIX_ROUNDS: usize = 2;

/// O passo que a rodada devolve quando todas as ondas estão entregues e
/// aprovadas.
pub const DONE_STEP: &str = "close";

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
    /// Um arquivo entregue está reservado para outra onda em andamento.
    FileReserved { file: String, wave: u64, other: u64 },
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
            Self::FileReserved { .. } => "round-file-reserved".into(),
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
            Self::FileReserved { file, wave, other } => fill(
                "round.file_reserved",
                &[("{file}", file.clone()), ("{wave}", wave.to_string()), ("{other}", other.to_string())],
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

/// A linha da entrega, como o agente de onda a devolve.
const DELIVERED_LINE: &str = "DELIVERED";
/// A linha do veredito, como o revisor a devolve.
const VERDICT_LINE: &str = "VERDICT";

/// O que a linha `DELIVERED` de uma onda trouxe.
pub(crate) struct WaveReport {
    pub wave: u64,
    pub delivered: String,
    pub files: Vec<String>,
    /// O resumo do commit, de onde a mensagem é montada.
    pub commit: Option<String>,
    /// As provas novas: o critério, pelo código ou pelo número, e o comando.
    pub proofs: Vec<(Value, String)>,
    /// As ondas que este conserto fecha.
    pub fixes: Vec<u64>,
    pub replan: Option<String>,
}

/// O que a linha `VERDICT` de uma onda trouxe: os campos do veredito, com a
/// onda à parte.
pub(crate) struct VerdictReport {
    pub wave: u64,
    pub fields: Map<String, Value>,
}

/// O relatório de uma rodada: as entregas e os vereditos que ele traz. O
/// fechamento lê o relatório da última rodada pela mesma porta.
pub(crate) struct Report {
    pub waves: Vec<WaveReport>,
    pub verdicts: Vec<VerdictReport>,
}

/// O que a rodada fez com um relatório.
pub(crate) struct Taken {
    /// O que foi gravado, na ordem.
    pub recorded: Vec<Value>,
    /// Os arquivos formatados.
    pub formatted: Vec<String>,
    /// Os avisos: formatador não achado e prova nova que não roda teste.
    pub warnings: Vec<Value>,
    /// O commit feito, quando houve arquivo entregue.
    pub commit: Option<Value>,
}

/// O núcleo testável de [`run`]. A sessão vem do ambiente. Nunca entra em
/// pânico.
pub(crate) fn round_at(opts: &RoundOpts) -> Value {
    round_for(opts, session_from_env().as_deref())
}

/// [`round_at`] com a sessão recebida, que é como um teste a escolhe.
pub(crate) fn round_for(opts: &RoundOpts, session: Option<&str>) -> Value {
    let project = spec_events::project(&opts.root);
    let lang = project.lang;
    match run_round(opts, &project.root, lang, session) {
        Ok(report) => report,
        Err(refusal) => refusal.to_value(lang),
    }
}

fn run_round(
    opts: &RoundOpts,
    root: &Path,
    lang: Locale,
    session: Option<&str>,
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

    let Taken { mut recorded, formatted, warnings, commit } =
        match opts.report.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
            Some(raw) => take_report(&opts.root, root, &spec, raw, &log, lang)?,
            None => Taken { recorded: Vec::new(), formatted: Vec::new(), warnings: Vec::new(), commit: None },
        };

    // A primeira rodada leva a spec para a execução.
    let entering = phase == "approved";
    if entering {
        crate::commands::spec_events::write::record_phase(&opts.root, &spec, "running", session);
    }

    let log = store::read(&path)
        .map_err(RoundRefusal::Refused)?
        .ok_or_else(|| RoundRefusal::Refused(Refusal::NoSpecFile { spec: spec.clone() }))?;
    let codes = log.codes();

    // O despacho da rodada seguinte: as ondas prontas, no máximo o que o
    // projeto deixa compilar ao mesmo tempo contando as que já estão em
    // andamento, nunca duas que dividem arquivo — nem com uma em andamento —,
    // e nenhuma que a onda parada pelo limite de consertos segura.
    let running = waves_in_progress(&log);
    let stuck = waves_stuck(&log);
    let next = next_waves(&log, max_parallel(root), &running, &stuck);
    // O pedido de cada onda lista as outras em andamento, contando as que
    // saem junto com ela nesta rodada.
    let flight: BTreeSet<u64> = running.keys().chain(&next).copied().collect();
    let built = prompts(root, &spec, &log, lang, &flight);
    let mut dispatched: Vec<Value> = Vec::new();
    let mut in_flight: BTreeMap<u64, String> = running
        .iter()
        .map(|(wave, sent)| (*wave, codes.get(sent).cloned().unwrap_or_else(|| sent.to_string())))
        .collect();
    for wave in &next {
        let Some(prompt) = built.iter().find(|p| p.wave == *wave) else { continue };
        let mut draft = Map::new();
        draft.insert("wave".into(), json!(wave));
        draft.insert("role".into(), json!("wave"));
        draft.insert("text".into(), json!(prompt.text));
        draft.insert("lines".into(), json!(prompt.lines));
        draft.insert("chars".into(), json!(prompt.text.chars().count()));
        draft.insert("items".into(), json!(sent_items(&log, *wave)));
        draft.insert("mustard".into(), json!(env!("CARGO_PKG_VERSION")));
        draft.insert("author".into(), json!("binary"));
        let written = record(&opts.root, &spec, "send", draft, PhaseWriter::Binary)
            .map_err(RoundRefusal::Refused)?;
        recorded.push(json!({ "wave": wave, "type": "send", "id": written.written.id }));
        dispatched.push(json!({ "wave": wave, "lines": prompt.lines, "prompt": prompt.text }));
        let code = written.written.code.clone().unwrap_or_else(|| written.written.id.to_string());
        in_flight.insert(*wave, code);
    }
    let reviews = reviews_due(&log, &built);

    // O próximo passo: despachar o que saiu agora; esperar as que estão em
    // andamento; fechar, com tudo entregue e aprovado; ou dizer qual onda
    // falta, quando nada se move.
    let report_back = translate("round.report", lang);
    let mut command: Option<String> = None;
    let then = if !dispatched.is_empty() || !reviews.is_empty() {
        format!("{} {report_back}", translate("round.next", lang))
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
    // A pergunta da onda parada vem antes do resto, que segue sem ela.
    let (stopped, asked) = stopped_waves(&stuck, &codes, lang);
    let then = asked.into_iter().chain(Some(then).filter(|t| !t.is_empty())).collect::<Vec<_>>().join(" ");

    // A página sai no fim do passo, uma vez, e a rodada manda publicá-la,
    // menos quando ela não pôde ser refeita.
    let pages = crate::commands::spec_events::pages::refresh(root, &spec, lang);

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
        "reviews": reviews,
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
    if let Ok(pages) = &pages {
        out["md"] = json!(pages.md);
        out["html"] = json!(pages.html);
    }
    if !warnings.is_empty() {
        out["warnings"] = json!(warnings);
    }
    crate::commands::spec_events::pages::end_milestone(&mut out, pages.as_ref(), "round", &then, lang);
    if let Some(command) = command {
        out["command"] = json!(command);
    }
    if let Some(number) = rewritten {
        out["pr"] = json!({ "number": number, "body": "rewritten" });
    }
    Ok(out)
}

/// As ondas paradas pelo limite de consertos, na resposta da rodada: cada uma
/// com a pergunta ao usuário e os vereditos que a pararam, por inteiro — é com
/// eles que o usuário decide —, e o texto que manda fazer cada pergunta.
fn stopped_waves(
    stuck: &BTreeMap<u64, Vec<&SpecEvent>>,
    codes: &BTreeMap<u64, String>,
    lang: Locale,
) -> (Vec<Value>, Vec<String>) {
    let max = MAX_FIX_ROUNDS.to_string();
    stuck
        .iter()
        .map(|(wave, verdicts)| {
            let listed: Vec<(String, &str)> = verdicts
                .iter()
                .map(|v| (codes.get(&v.id).cloned().unwrap_or_else(|| v.id.to_string()), v.str_field("text").unwrap_or_default()))
                .collect();
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

/// A spec na fase `phase` pode ter ondas despachadas: está aprovada, ou já em
/// execução. É a mesma pergunta para a rodada e para o gancho que monta o
/// pedido no despacho.
pub(crate) fn can_run(phase: &str) -> bool {
    matches!(phase, "approved" | "running")
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

// ---------------------------------------------------------------------------
// O relatório da rodada
// ---------------------------------------------------------------------------

/// Fecha o que voltou de uma rodada, a partir do texto `raw` com as linhas
/// dos agentes: confere tudo, formata os arquivos da rodada e faz o commit, e
/// só então grava os vereditos, as entregas, a versão nova de cada critério
/// com prova nova e o commit. O git, que pode recusar, roda antes da primeira
/// gravação: a chamada corrigida depois de uma recusa grava tudo uma vez só.
/// A rodada e o fechamento fecham o relatório por aqui.
pub(crate) fn take_report(
    start: &Path,
    root: &Path,
    spec: &str,
    raw: &str,
    log: &SpecLog,
    lang: Locale,
) -> Result<Taken, RoundRefusal> {
    let report = parse_report(raw)?;
    // O agente que diz que o plano da onda não funciona para a rodada: a
    // mudança proposta é mostrada, e só o clique do usuário em "Aceitar",
    // gravado pela testemunha, a deixa seguir.
    for wave in &report.waves {
        if let Some(change) = &wave.replan {
            let code = replan_code(wave.wave, change);
            if !change_accepted(log, wave.wave, &code) {
                return Err(RoundRefusal::Replan { wave: wave.wave, change: change.clone(), code });
            }
        }
    }
    for wave in &report.waves {
        let chars = wave.delivered.chars().count();
        if chars > DELIVERED_MAX_CHARS {
            return Err(RoundRefusal::DeliveredTooLong { wave: wave.wave, chars });
        }
    }
    reserved_elsewhere(log, &report.waves)?;
    unknown_file(root, &report.waves)?;
    // A mensagem do commit é montada e conferida junto das outras travas,
    // antes de qualquer gravação: recusá-la depois de gravar o entregou e o
    // veredito faria a chamada seguinte, com a mensagem corrigida, duplicar os
    // dois.
    let message = commit_message(&report.waves, lang)?;
    let checked = check_reports(start, spec, &report).map_err(RoundRefusal::Refused)?;

    let mut warnings: Vec<Value> = Vec::new();
    // A formatação roda uma vez por rodada, só nos arquivos da rodada.
    let mut files: Vec<String> = Vec::new();
    for file in report.waves.iter().flat_map(|w| w.files.iter()) {
        if !files.contains(file) {
            files.push(file.clone());
        }
    }
    let outcome = format_round_files(root, &files);
    for name in outcome.missing {
        warnings.push(json!({
            "reason": "formatter-not-found",
            "hint": translate("round.formatter_missing", lang).replace("{name}", &name),
        }));
    }
    // O git roda com a trava da spec presa: duas rodadas ao mesmo tempo no
    // mesmo checkout não dividem o índice, e um commit nunca leva o arquivo
    // da outra.
    let path = store::spec_file(root, spec).map_err(RoundRefusal::Refused)?;
    let made = match message {
        Some((title, body)) => {
            let sha = store::with_locked_log(&path, |_| make_commit(root, &title, &body, &files))
                .map_err(RoundRefusal::Refused)?
                .ok_or_else(|| RoundRefusal::Refused(Refusal::NoSpecFile { spec: spec.to_string() }))??;
            Some((sha, title))
        }
        None => None,
    };
    let (recorded, proofs) = record_reports(start, spec, checked).map_err(RoundRefusal::Refused)?;
    let commit = match made {
        Some((sha, title)) => {
            let mut waves: Vec<u64> = Vec::new();
            for n in report.waves.iter().flat_map(|w| std::iter::once(w.wave).chain(w.fixes.iter().copied())) {
                if !waves.contains(&n) {
                    waves.push(n);
                }
            }
            Some(record_commit(start, root, spec, &sha, &title, &waves, &files)?)
        }
        None => None,
    };
    // A prova nova roda uma vez: a que sai verde sem rodar teste nenhum é
    // avisada agora, antes de o fechamento recusá-la.
    for (code, proof) in proofs {
        if crate::commands::review::qa_run::run_proof(&proof, root).ran_no_test {
            warnings.push(json!({
                "reason": "proof-ran-no-test",
                "hint": translate("round.proof_ran_no_test", lang).replace("{code}", &code),
            }));
        }
    }
    Ok(Taken { recorded, formatted: outcome.formatted, warnings, commit })
}

/// Os trechos entre `<tag>` e `</tag>` de `raw`, na ordem.
fn tagged<'a>(raw: &'a str, tag: &str) -> Vec<&'a str> {
    let (open, close) = (format!("<{tag}>"), format!("</{tag}>"));
    let mut out = Vec::new();
    let mut rest = raw;
    while let Some(at) = rest.find(&open) {
        let after = &rest[at + open.len()..];
        let Some(end) = after.find(&close) else { break };
        out.push(after[..end].trim());
        rest = &after[end + close.len()..];
    }
    out
}

/// O objeto JSON de uma linha, com a onda dela.
fn line_object(body: &str, line: &'static str) -> Result<(u64, Map<String, Value>), RoundRefusal> {
    let parsed: Value =
        serde_json::from_str(body).map_err(|e| RoundRefusal::BadReport { detail: format!("{line}: {e}") })?;
    let Value::Object(fields) = parsed else {
        return Err(RoundRefusal::BadReport { detail: format!("{line}: {body}") });
    };
    let wave = fields.get("wave").and_then(Value::as_u64).ok_or(RoundRefusal::LineField { line, field: "wave" })?;
    Ok((wave, fields))
}

/// O relatório da rodada anterior: as linhas `DELIVERED` e `VERDICT` que o
/// texto recebido traz, como os agentes as devolvem. O resto do texto não é
/// lido.
pub(crate) fn parse_report(raw: &str) -> Result<Report, RoundRefusal> {
    let text = |fields: &Map<String, Value>, key: &str| {
        fields.get(key).and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()).map(str::to_string)
    };
    let mut waves = Vec::new();
    for body in tagged(raw, DELIVERED_LINE) {
        let (wave, fields) = line_object(body, DELIVERED_LINE)?;
        let field = |field| RoundRefusal::LineField { line: DELIVERED_LINE, field };
        let delivered = text(&fields, "text").ok_or_else(|| field("text"))?;
        let files: Vec<String> = fields
            .get("files")
            .and_then(Value::as_array)
            .ok_or_else(|| field("files"))?
            .iter()
            .filter_map(Value::as_str)
            .map(|f| f.trim().replace('\\', "/"))
            .filter(|f| !f.is_empty())
            .collect();
        let replan = text(&fields, "replan");
        let commit = text(&fields, "commit");
        // Arquivo entregue pede commit, e o commit sai do resumo.
        if commit.is_none() && !files.is_empty() && replan.is_none() {
            return Err(field("commit"));
        }
        let mut proofs = Vec::new();
        for proof in fields.get("proofs").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default() {
            let criterion = proof.get("criterion").filter(|c| !c.is_null()).cloned().ok_or_else(|| field("proofs"))?;
            let command = proof.get("proof").and_then(Value::as_str).map(str::trim).filter(|p| !p.is_empty());
            proofs.push((criterion, command.ok_or_else(|| field("proofs"))?.to_string()));
        }
        let fixes = fields
            .get("fixes")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(Value::as_u64).filter(|n| *n != wave).collect())
            .unwrap_or_default();
        waves.push(WaveReport { wave, delivered, files, commit, proofs, fixes, replan });
    }
    let mut verdicts = Vec::new();
    for body in tagged(raw, VERDICT_LINE) {
        let (wave, mut fields) = line_object(body, VERDICT_LINE)?;
        fields.remove("wave");
        verdicts.push(VerdictReport { wave, fields });
    }
    if waves.is_empty() && verdicts.is_empty() {
        return Err(RoundRefusal::LineMissing);
    }
    Ok(Report { waves, verdicts })
}

/// Um arquivo entregue que está reservado para outra onda em andamento: duas
/// ondas no mesmo arquivo seriam dois agentes editando o mesmo arquivo ao mesmo
/// tempo. As ondas que entregam neste relatório já não estão em andamento.
fn reserved_elsewhere(log: &SpecLog, waves: &[WaveReport]) -> Result<(), RoundRefusal> {
    let reporting: BTreeSet<u64> = waves.iter().flat_map(|w| std::iter::once(w.wave).chain(w.fixes.clone())).collect();
    let running: Vec<u64> =
        waves_in_progress(log).into_keys().filter(|n| !reporting.contains(n)).collect();
    let reserved = task_files(log);
    for wave in waves {
        for file in &wave.files {
            if let Some(other) = running.iter().find(|n| reserved.get(n).is_some_and(|files| files.contains(file))) {
                return Err(RoundRefusal::FileReserved { file: file.clone(), wave: wave.wave, other: *other });
            }
        }
    }
    Ok(())
}

/// O número do critério `reference`, dado pelo código que a página mostra ou
/// pelo número, na versão mais nova. Um critério que a spec não tem é
/// recusado.
fn criterion_id(log: &SpecLog, reference: &Value) -> Result<u64, Refusal> {
    let unknown = || Refusal::UnknownTarget {
        target: mustard_core::domain::spec_events::EventRef::from_value(reference)
            .unwrap_or(mustard_core::domain::spec_events::EventRef::Code(reference.to_string())),
    };
    let codes = log.codes();
    let id = match reference {
        Value::Number(n) => n.as_u64().ok_or_else(unknown)?,
        Value::String(code) => {
            let code = code.trim();
            match code.parse::<u64>() {
                Ok(n) => n,
                Err(_) => codes.iter().filter(|(_, c)| c.as_str() == code).map(|(id, _)| *id).max().ok_or_else(unknown)?,
            }
        }
        _ => return Err(unknown()),
    };
    let current = log.current(id).filter(|e| e.event_type == "criterion").ok_or_else(unknown)?;
    Ok(current.id)
}

/// O que [`record_reports`] devolve: o que foi gravado e, de cada prova nova,
/// o código do critério e o comando.
type RecordedReport = (Vec<Value>, Vec<(String, String)>);

/// O que [`check_reports`] conferiu e [`record_reports`] grava: cada veredito
/// e cada entregou já montado, com a onda, e o número de cada critério com
/// prova nova, com o comando.
struct CheckedReport {
    verdicts: Vec<(u64, Map<String, Value>)>,
    deliveries: Vec<(u64, Map<String, Value>)>,
    proofs: Vec<(u64, String)>,
}

/// Monta o que voltou e passa cada veredito e cada entregou pela conferência
/// que a gravação faz primeiro, sem gravar nada: a linha sem campo
/// obrigatório nunca deixa gravada a que veio antes dela. O entregou vai
/// também em cada onda que o conserto fecha.
fn check_reports(start: &Path, spec: &str, report: &Report) -> Result<CheckedReport, Refusal> {
    let path = store::spec_file(&crate::commands::spec_events::project(start).root, spec)?;
    let log = store::read(&path)?.ok_or_else(|| Refusal::NoSpecFile { spec: spec.to_string() })?;
    // Os critérios citados existem, antes de qualquer gravação.
    let mut verdicts = Vec::new();
    for verdict in &report.verdicts {
        let mut draft = verdict.fields.clone();
        if let Some(Value::Array(criteria)) = draft.get_mut("criteria") {
            for item in criteria.iter_mut() {
                if let Some(reference) = item.get("criterion").cloned() {
                    item["criterion"] = json!(criterion_id(&log, &reference)?);
                }
            }
        }
        draft.insert("wave".into(), json!(verdict.wave));
        draft.insert("author".into(), json!("review"));
        validate(&normalize(draft.clone(), "verdict"))?;
        verdicts.push((verdict.wave, draft));
    }
    let mut deliveries = Vec::new();
    for report in &report.waves {
        for wave in std::iter::once(report.wave).chain(report.fixes.iter().copied()) {
            let mut draft = Map::new();
            draft.insert("wave".into(), json!(wave));
            draft.insert("text".into(), json!(report.delivered));
            draft.insert("files".into(), json!(report.files));
            draft.insert("author".into(), json!("wave"));
            validate(&normalize(draft.clone(), "delivered"))?;
            deliveries.push((wave, draft));
        }
    }
    let mut proofs = Vec::new();
    for wave in &report.waves {
        for (reference, proof) in &wave.proofs {
            proofs.push((criterion_id(&log, reference)?, proof.clone()));
        }
    }
    Ok(CheckedReport { verdicts, deliveries, proofs })
}

/// Grava o que [`check_reports`] conferiu, pela mesma porta de gravação das
/// outras: primeiro os vereditos, que julgam entregas já gravadas; depois o
/// entregou de cada onda e a versão nova de cada critério com prova nova.
/// Devolve o que foi gravado e, de cada prova nova, o código do critério e o
/// comando.
fn record_reports(start: &Path, spec: &str, checked: CheckedReport) -> Result<RecordedReport, Refusal> {
    let CheckedReport { verdicts, deliveries, proofs } = checked;
    let path = store::spec_file(&crate::commands::spec_events::project(start).root, spec)?;
    let read = || store::read(&path)?.ok_or_else(|| Refusal::NoSpecFile { spec: spec.to_string() });
    let mut recorded = Vec::new();
    for (wave, draft) in verdicts {
        let written = record(start, spec, "verdict", draft, PhaseWriter::Binary)?;
        recorded.push(json!({ "wave": wave, "type": "verdict", "id": written.written.id }));
    }
    for (wave, draft) in deliveries {
        let written = record(start, spec, "delivered", draft, PhaseWriter::Binary)?;
        recorded.push(json!({ "wave": wave, "type": "delivered", "id": written.written.id }));
    }
    let mut ran = Vec::new();
    for (id, proof) in proofs {
        let log = read()?;
        let Some(criterion) = log.current(id) else { continue };
        let mut draft: Map<String, Value> = criterion
            .fields
            .iter()
            .filter(|(key, _)| !["v", "id", "code", "at", "type", "search", "author", "replaces"].contains(&key.as_str()))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        draft.insert("proof".into(), json!(proof));
        draft.insert("replaces".into(), json!(criterion.id));
        draft.insert("author".into(), json!("wave"));
        let code = log.codes().get(&criterion.id).cloned().unwrap_or_else(|| criterion.id.to_string());
        let written = record(start, spec, "criterion", draft, PhaseWriter::Binary)?;
        recorded.push(json!({ "type": "criterion", "id": written.written.id, "replaces": criterion.id }));
        ran.push((code, proof));
    }
    Ok((recorded, ran))
}

/// O código da mudança proposta, que vai na pergunta que a decide: a onda e
/// uma chave do texto da mudança, para que um "sim" nunca sirva para outra.
fn replan_code(wave: u64, change: &str) -> String {
    let key = crate::commands::agent::render::prompt_ref::fnv1a64(&[change.trim()]) & 0x00ff_ffff;
    format!("onda-{wave}-{key:06x}")
}

/// A pergunta que decide a mudança de código `code`, no idioma `lang`.
fn change_question(code: &str, lang: Locale) -> String {
    translate("change.question", lang).replace("{code}", code)
}

/// A mudança de código `code`, proposta pela onda `wave`, foi aceita: o
/// clique mais novo do usuário na pergunta dela, gravado pela testemunha
/// depois do último pedido da onda, é o "Aceitar". Um clique em "Recusar"
/// depois dele desfaz o "sim"; um clique de antes do pedido não vale para ele.
///
/// Só conta a mensagem de autor `user` com a testemunha. O `run write` recusa
/// toda mensagem com a testemunha, de qualquer autor, e recusa rever ou tirar
/// uma delas, com ou sem a recusa da fala digitada: só a testemunha grava o
/// clique.
fn change_accepted(log: &SpecLog, wave: u64, code: &str) -> bool {
    let langs = [Locale::PtBr, Locale::EnUs];
    let questions: Vec<String> = langs.iter().map(|lang| change_question(code, *lang)).collect();
    let sent = log.last_by_wave("send").get(&wave).copied().unwrap_or(0);
    let last_click = log
        .block(BlockQuery::Block(Block::Conversation))
        .into_iter()
        .filter(|e| e.event_type == "message" && e.id > sent && e.str_field("author") == Some("user"))
        .filter_map(|e| e.fields.get("witness"))
        .filter(|w| {
            w.get("question")
                .and_then(Value::as_str)
                .is_some_and(|q| questions.iter().any(|asked| asked == q.trim()))
        })
        .filter_map(|w| w.get("answer").and_then(Value::as_str))
        .next_back();
    last_click.is_some_and(|answer| langs.iter().any(|lang| translate("change.accept", *lang) == answer.trim()))
}

/// A mensagem do commit da rodada, montada do resumo que cada entrega traz e
/// já conferida: o título no molde do repositório (`tipo(escopo): frase`),
/// com o resumo da primeira onda, e o corpo com uma linha por onda. O tipo é
/// `fix` quando a rodada traz um conserto, e `feat` nos outros casos. `None`
/// quando nenhuma entrega traz arquivo.
fn commit_message(waves: &[WaveReport], lang: Locale) -> Result<Option<(String, String)>, RoundRefusal> {
    let committed: Vec<(&WaveReport, &str)> = waves
        .iter()
        .filter(|w| !w.files.is_empty())
        .filter_map(|w| w.commit.as_deref().map(|summary| (w, summary)))
        .collect();
    let Some((_, first)) = committed.first() else {
        return Ok(None);
    };
    let numbers: Vec<String> = committed.iter().map(|(w, _)| w.wave.to_string()).collect();
    let scope_key = if numbers.len() == 1 { "round.commit.scope.one" } else { "round.commit.scope.many" };
    let scope = translate(scope_key, lang).replace("{waves}", &numbers.join("-"));
    let kind = if committed.iter().any(|(w, _)| !w.fixes.is_empty()) { "fix" } else { "feat" };
    let title = format!("{kind}({scope}): {first}");
    let body: Vec<String> = committed
        .iter()
        .map(|(w, summary)| {
            let mut line = translate("round.commit.line", lang)
                .replace("{wave}", &w.wave.to_string())
                .replace("{summary}", summary);
            if !w.fixes.is_empty() {
                let fixed: Vec<String> = w.fixes.iter().map(u64::to_string).collect();
                line.push(' ');
                line.push_str(&translate("round.commit.fixes", lang).replace("{waves}", &fixed.join(", ")));
            }
            line
        })
        .collect();
    let body = body.join("\n");
    check_commit_text(&title, &body)?;
    Ok(Some((title, body)))
}

/// A mensagem de commit cabe no modelo, pela MESMA conferência que o pull
/// request usa.
///
/// As duas eram a mesma regra escrita duas vezes — os mesmos tetos, a mesma
/// lista do que nunca vai, o mesmo achador de e-mail — e uma regra escrita duas
/// vezes é uma regra que vale em um lugar só assim que alguém mexer no outro.
/// A conferência mora no núcleo; aqui fica só a tradução para a recusa da
/// rodada, que é o que muda entre as duas portas.
fn check_commit_text(title: &str, body: &str) -> Result<(), RoundRefusal> {
    check_message(title, body, MESSAGE_TITLE_MAX, MESSAGE_BODY_MAX).map_err(|refusal| match refusal
    {
        MessageRefusal::TooLong { part, chars, max } => {
            RoundRefusal::CommitTooLong { part: part.to_string(), chars, max }
        }
        MessageRefusal::Forbidden { found, .. } => RoundRefusal::CommitForbidden { found },
        // O commit não tira o título da spec: o relatório o traz. Uma spec sem
        // objetivo não é recusa desta porta.
        MessageRefusal::NoTitle => RoundRefusal::CommitForbidden { found: String::new() },
    })
}

// ---------------------------------------------------------------------------
// A formatação da rodada
// ---------------------------------------------------------------------------

/// As extensões que o Prettier trata.
const PRETTIER_EXTS: &[&str] =
    &[".ts", ".tsx", ".js", ".jsx", ".json", ".css", ".md", ".html", ".scss"];

/// Os sinais de que o projeto tem Prettier configurado.
const PRETTIER_SIGNS: &[&str] = &[
    "node_modules/.bin/prettier",
    ".prettierrc",
    ".prettierrc.js",
    ".prettierrc.json",
    "prettier.config.js",
];

/// O que a formatação da rodada fez: os arquivos formatados e os formatadores
/// que o projeto declara e que não foram achados.
#[derive(Debug, Default, PartialEq, Eq)]
struct Formatting {
    formatted: Vec<String>,
    missing: Vec<String>,
}

/// Formata só os arquivos da rodada, com o formatador que o projeto já tem:
/// o Prettier configurado ou o `dotnet format` de um projeto .NET. Num projeto
/// sem formatador configurado nada é formatado e nada é avisado; o formatador
/// declarado e não achado sai pelo nome, em vez de a formatação ser pulada em
/// silêncio.
fn format_round_files(root: &Path, files: &[String]) -> Formatting {
    format_with(root, files, &|program, args| run(root, program, args))
}

/// [`format_round_files`] com o executor recebido, que é como um teste o
/// escolhe sem depender do que está instalado na máquina.
fn format_with(root: &Path, files: &[String], exec: &dyn Fn(&str, &[&str]) -> bool) -> Formatting {
    let mut out = Formatting::default();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let prettier: Vec<&String> = files
        .iter()
        .filter(|f| seen.insert(f.as_str()))
        .filter(|f| PRETTIER_EXTS.contains(&extension(f).as_str()))
        .filter(|f| root.join(f).is_file())
        .collect();
    if !prettier.is_empty() && PRETTIER_SIGNS.iter().any(|sign| root.join(sign).exists()) {
        let mut args: Vec<&str> = vec!["prettier", "--write"];
        args.extend(prettier.iter().map(|f| f.as_str()));
        if exec("npx", &args) {
            out.formatted.extend(prettier.iter().map(|f| (*f).clone()));
        } else {
            out.missing.push("Prettier".to_string());
        }
    }
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let sharp: Vec<&String> = files
        .iter()
        .filter(|f| seen.insert(f.as_str()))
        .filter(|f| extension(f) == ".cs")
        .filter(|f| root.join(f).is_file())
        .collect();
    if !sharp.is_empty() && let Some(project) = dotnet_project(root) {
        let mut ok = true;
        for file in &sharp {
            ok &= exec("dotnet", &["format", &project, "--include", file, "--no-restore"]);
        }
        if ok {
            out.formatted.extend(sharp.iter().map(|f| (*f).clone()));
        } else {
            out.missing.push("dotnet format".to_string());
        }
    }
    out
}

/// A extensão de um caminho, em minúsculas e com o ponto; vazia sem extensão.
fn extension(path: &str) -> String {
    let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match base.rfind('.') {
        Some(i) if i > 0 => base[i..].to_ascii_lowercase(),
        _ => String::new(),
    }
}

/// O `.sln` ou o `.csproj` da raiz do projeto, que diz que ele é um projeto
/// .NET. `None` quando não há nenhum.
fn dotnet_project(root: &Path) -> Option<String> {
    let entries = std::fs::read_dir(root).ok()?;
    let mut sln = None;
    let mut csproj = None;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.to_ascii_lowercase().ends_with(".sln") {
            sln = Some(name);
        } else if name.to_ascii_lowercase().ends_with(".csproj") {
            csproj = Some(name);
        }
    }
    sln.or(csproj)
}

/// Roda um programa na raiz do projeto; `false` quando ele não está lá ou
/// saiu com erro.
fn run(root: &Path, program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

// ---------------------------------------------------------------------------
// O commit da rodada
// ---------------------------------------------------------------------------

/// Faz o commit da rodada com a mensagem já conferida e devolve o código dele.
/// Não grava nada: roda antes de qualquer gravação, e a recusa do git para a
/// rodada com a spec intacta.
fn make_commit(root: &Path, title: &str, body: &str, files: &[String]) -> Result<String, RoundRefusal> {
    // O arquivo que ainda existe entra pelo `add`. O apagado sai do índice por
    // outra porta: o `add` recusa o caminho que já saiu do índice, e a remoção
    // que só aconteceu no disco sai do mesmo jeito. O caminho que já não está
    // no índice não é erro — a remoção dele já estava pronta para o commit.
    let (present, gone): (Vec<&str>, Vec<&str>) =
        files.iter().map(String::as_str).partition(|file| root.join(file).exists());
    if !present.is_empty() {
        let mut add: Vec<&str> = vec!["add", "--"];
        add.extend(&present);
        git(root, &add).map_err(|detail| RoundRefusal::Git { detail })?;
    }
    if !gone.is_empty() {
        let mut remove: Vec<&str> = vec!["rm", "-r", "-q", "--cached", "--ignore-unmatch", "--"];
        remove.extend(&gone);
        git(root, &remove).map_err(|detail| RoundRefusal::Git { detail })?;
    }
    let mut args: Vec<&str> = vec!["commit", "-m", title];
    if !body.is_empty() {
        args.push("-m");
        args.push(body);
    }
    git(root, &args).map_err(|detail| RoundRefusal::Git { detail })?;
    let sha = git(root, &["rev-parse", "HEAD"]).map_err(|detail| RoundRefusal::Git { detail })?;
    Ok(sha.trim().to_string())
}

/// Grava no arquivo de eventos o commit `sha` feito pela rodada.
fn record_commit(
    start: &Path,
    root: &Path,
    spec: &str,
    sha: &str,
    title: &str,
    waves: &[u64],
    files: &[String],
) -> Result<Value, RoundRefusal> {
    let mut draft = Map::new();
    draft.insert("sha".into(), json!(sha));
    draft.insert("title".into(), json!(title));
    draft.insert("waves".into(), json!(waves));
    draft.insert("files".into(), json!(files));
    draft.insert("repo".into(), json!(repo_name(root)));
    draft.insert("author".into(), json!("binary"));
    record(start, spec, "commit", draft, PhaseWriter::Binary).map_err(RoundRefusal::Refused)?;
    Ok(json!({ "sha": sha, "title": title }))
}

/// Cada arquivo entregue está no disco ou no índice, contando o que saiu do
/// índice desde o último commit; senão, a recusa vem antes de gravar, e não
/// do commit, depois (a remoção aceita calada o caminho que não existe).
fn unknown_file(root: &Path, waves: &[WaveReport]) -> Result<(), RoundRefusal> {
    for wave in waves {
        for file in &wave.files {
            let known = root.join(file).exists()
                || git(root, &["ls-files", "--error-unmatch", "--with-tree=HEAD", "--", file]).is_ok();
            if !known {
                return Err(RoundRefusal::FileUnknown { file: file.clone(), wave: wave.wave });
            }
        }
    }
    Ok(())
}

/// O nome do repositório: o da pasta do projeto.
fn repo_name(root: &Path) -> String {
    root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "?".into())
}

/// Roda o git na raiz do projeto e devolve a saída; o erro vem como texto.
fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = git_exec::run(root, args);
    if out.ok {
        return Ok(out.stdout);
    }
    // Alguns motivos, como o de não haver nada a comitar, o git escreve na
    // saída normal, e a de erro vem vazia.
    let said = if out.stderr.trim().is_empty() { &out.stdout } else { &out.stderr };
    Err(said.trim().to_string())
}

// ---------------------------------------------------------------------------
// A escolha das ondas
// ---------------------------------------------------------------------------

/// Quantas ondas o projeto deixa compilar ao mesmo tempo.
fn max_parallel(root: &Path) -> usize {
    mustard_core::ProjectConfig::load(root).max_compiling_waves().unwrap_or(DEFAULT_PARALLEL)
}

/// As ondas que saem nesta rodada: as que ainda não saíram nem entregaram,
/// cujas dependências já foram entregues, no máximo `limit` junto com as que
/// estão em andamento (`running`), e nunca duas que declaram o mesmo arquivo —
/// duas ondas assim seriam dois agentes editando o mesmo arquivo ao mesmo
/// tempo. A onda em andamento conta como uma que já saiu nesta rodada: ocupa
/// uma vaga e reserva os arquivos das tarefas dela. A onda parada pelo limite
/// de consertos (`stuck`) não sai, nem a que depende dela, direta ou por outra
/// onda.
fn next_waves(
    log: &SpecLog,
    limit: usize,
    running: &BTreeMap<u64, u64>,
    stuck: &BTreeMap<u64, Vec<&SpecEvent>>,
) -> Vec<u64> {
    let graph = wave_graph(log);
    // A onda reprovada volta para a fila: sem isso o ciclo de conserto não
    // fecha, porque o fechamento recusa e diz qual refazer e a rodada nunca a
    // despacharia de novo.
    let to_redo = waves_to_redo(log);
    // O pedido gravado descreve o plano daquele momento: a onda que ganhou
    // versão nova depois dele, e ainda não entregou, sai de novo com o pedido
    // do plano atual.
    let replanned = waves_replanned(log);
    // O que já saiu da fila: a onda com pedido e também a onda que já
    // entregou. Só o pedido não basta, porque a onda entregue antes de a
    // rodada existir não tem pedido nenhum e apareceria como pronta para
    // sair — e sairia de novo um trabalho já feito.
    // Quais ondas já entregaram é pergunta do núcleo, e é ele que responde:
    // recalcular o filtro aqui deixava duas camadas decidindo a mesma coisa,
    // concordando hoje e livres para divergir amanhã.
    let delivered = log.delivered_waves();
    let already_out: BTreeSet<u64> = log
        .block(BlockQuery::Block(Block::Waves))
        .into_iter()
        .filter(|e| e.event_type == "send")
        .filter_map(SpecEvent::wave)
        .filter(|n| !replanned.contains(n))
        .chain(delivered.iter().copied())
        .filter(|n| !to_redo.contains(n))
        .collect();
    let mut depends: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for wave in log.block(BlockQuery::Block(Block::Waves)).into_iter().filter(|e| e.event_type == "wave") {
        if let Some(n) = wave.wave() {
            depends.insert(n, wave.ints("depends_on"));
        }
    }
    let files = task_files(log);
    let done = waves_done(log, running);
    let slots = limit.saturating_sub(running.len());
    let mut out: Vec<u64> = Vec::new();
    let mut taken: BTreeSet<String> =
        running.keys().flat_map(|n| files.get(n).cloned().unwrap_or_default()).collect();
    for n in ready_in_order(&graph, &depends, &already_out, &delivered, &done) {
        if out.len() >= slots {
            break;
        }
        if stuck.contains_key(&n) || dependencies_of(n, &depends).iter().any(|d| stuck.contains_key(d)) {
            continue;
        }
        let declared = files.get(&n).cloned().unwrap_or_default();
        if declared.iter().any(|f| taken.contains(f)) {
            continue;
        }
        taken.extend(declared);
        out.push(n);
    }
    out
}

/// Os arquivos que as tarefas de cada onda declaram.
fn task_files(log: &SpecLog) -> BTreeMap<u64, BTreeSet<String>> {
    let mut files: BTreeMap<u64, BTreeSet<String>> = BTreeMap::new();
    for task in log.block(BlockQuery::Block(Block::Waves)).into_iter().filter(|e| e.event_type == "task") {
        let Some(n) = task.wave() else { continue };
        let entry = files.entry(n).or_default();
        for file in task
            .fields
            .get("files")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|f| f.as_str().or_else(|| f.get("path").and_then(Value::as_str)))
        {
            entry.insert(file.replace('\\', "/"));
        }
    }
    files
}

/// As ondas em andamento, cada uma com o número do pedido dela: a onda tem
/// pedido e nenhuma entrega depois dele. O pedido mais antigo que a versão
/// mais nova da onda ou de uma tarefa dela descreve um plano que já mudou, e
/// não conta. Uma onda que já entregou só volta a sair por uma reprovação: o
/// pedido que veio depois de uma entrega, sem reprovação entre as duas, não é
/// trabalho em curso, nem o pedido de uma onda que saiu do plano.
pub(crate) fn waves_in_progress(log: &SpecLog) -> BTreeMap<u64, u64> {
    let planned = log.planned_waves();
    let replanned = waves_replanned(log);
    let verdicts = log.verdicts_by_wave();
    let mut deliveries: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for delivered in log.block(BlockQuery::Block(Block::Waves)).into_iter().filter(|e| e.event_type == "delivered") {
        if let Some(n) = delivered.wave() {
            deliveries.entry(n).or_default().push(delivered.id);
        }
    }
    log.last_by_wave("send")
        .into_iter()
        .filter(|(n, _)| planned.contains(n) && !replanned.contains(n))
        .filter(|(n, sent)| {
            let ids = deliveries.get(n).map(Vec::as_slice).unwrap_or_default();
            if ids.iter().any(|id| id > sent) {
                return false;
            }
            let judged_before = verdicts
                .get(n)
                .and_then(|list| list.iter().rev().find(|v| v.id < *sent))
                .and_then(|v| v.str_field("result"));
            !ids.iter().any(|id| id < sent) || judged_before == Some("rejected")
        })
        .collect()
}

/// As ondas paradas pelo limite de consertos, cada uma com as reprovações
/// seguidas que a pararam: depois da primeira reprovação vêm no máximo
/// [`MAX_FIX_ROUNDS`] rodadas de conserto, e a reprovação seguinte para. A
/// conta começa na versão mais nova do plano da onda: a onda que o usuário
/// replanejou volta à fila com a conta zerada.
fn waves_stuck(log: &SpecLog) -> BTreeMap<u64, Vec<&SpecEvent>> {
    let planned = last_planned(log);
    let mut out = BTreeMap::new();
    for (n, verdicts) in log.verdicts_by_wave() {
        let since = planned.get(&n).copied().unwrap_or(0);
        let mut rejected: Vec<&SpecEvent> = verdicts
            .iter()
            .rev()
            .take_while(|v| v.id > since && v.str_field("result") == Some("rejected"))
            .copied()
            .collect();
        if rejected.len() > MAX_FIX_ROUNDS {
            rejected.reverse();
            out.insert(n, rejected);
        }
    }
    out
}

/// As ondas entregues e aprovadas: têm entrega, não estão em andamento, não
/// esperam revisão, e a última revisão delas não reprovou. A onda entregue
/// antes de a rodada existir, sem pedido e sem veredito, está provada pelo
/// código que entrou.
fn waves_done(log: &SpecLog, running: &BTreeMap<u64, u64>) -> BTreeSet<u64> {
    let awaiting: BTreeSet<u64> = waves_awaiting_review(log).into_iter().collect();
    let rejected = log.last_rejected();
    log.delivered_waves()
        .into_iter()
        .filter(|n| !running.contains_key(n) && !awaiting.contains(n) && !rejected.contains_key(n))
        .collect()
}

/// A primeira onda planejada que ainda não está entregue e aprovada, com as
/// em andamento em `running`. `None` quando todas estão.
fn first_unfinished(log: &SpecLog, running: &BTreeMap<u64, u64>) -> Option<u64> {
    let done = waves_done(log, running);
    log.planned_waves().into_iter().find(|n| !done.contains(n))
}

/// A onda `n` depende de todas as outras que ainda não terminaram: as
/// dependências dela, diretas ou por outra onda, alcançam cada onda planejada
/// que não está entregue e aprovada. É a última onda da obra.
fn depends_on_all(n: u64, depends: &BTreeMap<u64, Vec<u64>>, done: &BTreeSet<u64>) -> bool {
    let reached = dependencies_of(n, depends);
    depends.keys().filter(|w| **w != n && !done.contains(w)).all(|w| reached.contains(w))
}

/// As ondas de que `n` depende, diretas ou por outra onda.
fn dependencies_of(n: u64, depends: &BTreeMap<u64, Vec<u64>>) -> BTreeSet<u64> {
    let mut reached: BTreeSet<u64> = BTreeSet::new();
    let mut stack: Vec<u64> = depends.get(&n).cloned().unwrap_or_default();
    while let Some(on) = stack.pop() {
        if reached.insert(on) {
            stack.extend(depends.get(&on).cloned().unwrap_or_default());
        }
    }
    reached
}

/// As ondas que voltam para a fila: a última revisão delas reprovou, e o
/// conserto ainda não saiu — o pedido mais novo da onda e a entrega mais nova
/// dela são anteriores a essa reprovação. Depois que o conserto sai, a onda espera a revisão dele, e não
/// é despachada de novo pela mesma reprovação.
fn waves_to_redo(log: &SpecLog) -> BTreeSet<u64> {
    let last_send = log.last_by_wave("send");
    let delivered = log.last_by_wave("delivered");
    log.last_rejected()
        .into_iter()
        .filter(|(n, id)| last_send.get(n).is_none_or(|sent| sent < id))
        .filter(|(n, id)| delivered.get(n).is_none_or(|fix| fix < id))
        .map(|(n, _)| n)
        .collect()
}

/// O número do evento mais novo do plano de cada onda: a versão mais nova da
/// onda ou de uma tarefa dela. A tarefa que muda de onda conta para as duas —
/// a de onde saiu, pela versão que ela substitui, e a para onde foi.
fn last_planned(log: &SpecLog) -> BTreeMap<u64, u64> {
    let by_id: BTreeMap<u64, &SpecEvent> = log.events.iter().map(|e| (e.id, e)).collect();
    let mut planned: BTreeMap<u64, u64> = BTreeMap::new();
    for event in log.block(BlockQuery::Block(Block::Waves)) {
        if !matches!(event.event_type.as_str(), "wave" | "task") {
            continue;
        }
        let before = event.int("replaces").and_then(|old| by_id.get(&old)).and_then(|old| old.wave());
        for n in event.wave().into_iter().chain(before) {
            let newest = planned.entry(n).or_insert(0);
            *newest = (*newest).max(event.id);
        }
    }
    planned
}

/// As ondas replanejadas depois do último pedido: a onda, ou uma tarefa dela,
/// ganhou versão nova depois do envio.
fn waves_replanned(log: &SpecLog) -> BTreeSet<u64> {
    let last_send = log.last_by_wave("send");
    last_planned(log)
        .into_iter()
        .filter(|(n, planned)| last_send.get(n).is_some_and(|sent| sent < planned))
        .map(|(n, _)| n)
        .collect()
}

/// As ondas prontas para sair, em ordem de nível e de número. `already_out`
/// são as ondas que já saíram da fila e `delivered` as que já entregaram, que
/// é o que solta as ondas dependentes delas. A onda que depende de todas as
/// outras só sai com as dependências entregues e aprovadas (`done`).
fn ready_in_order(
    graph: &crate::commands::wave::wave_overlap_check::WaveGraph,
    depends: &BTreeMap<u64, Vec<u64>>,
    already_out: &BTreeSet<u64>,
    delivered: &BTreeSet<u64>,
    done: &BTreeSet<u64>,
) -> Vec<u64> {
    let mut ready: Vec<(u32, u64)> = depends
        .iter()
        .filter(|(n, _)| !already_out.contains(n))
        .filter(|(n, on)| {
            let released = if depends_on_all(**n, depends, done) { done } else { delivered };
            on.iter().all(|d| released.contains(d))
        })
        .map(|(n, _)| (graph.level.get(n).copied().unwrap_or(0), *n))
        .collect();
    ready.sort_unstable();
    ready.into_iter().map(|(_, n)| n).collect()
}

/// As revisões que esta rodada pede: uma por onda entregue e ainda sem
/// veredito, com o pedido do revisor já montado — a lista de itens da onda, os
/// critérios e os defeitos já vistos naqueles arquivos.
fn reviews_due(log: &SpecLog, built: &[mustard_core::io::wave_prompt::WavePrompt]) -> Vec<Value> {
    waves_awaiting_review(log)
        .into_iter()
        .map(|wave| {
            let review = built.iter().find(|p| p.wave == wave).map(|p| p.review.clone()).unwrap_or_default();
            json!({ "wave": wave, "prompt": review })
        })
        .collect()
}

/// As ondas cuja entrega mais nova é posterior ao veredito mais novo: a
/// revisão delas é o que a rodada seguinte pede. Entra a onda que nunca foi
/// revisada, por não ter veredito nenhum, e entra também a onda reprovada que
/// já entregou o conserto — o conserto é mais novo que a reprovação. Excluir
/// toda onda que tem veredito fechava a porta da segunda: o conserto nunca
/// voltava para a revisão e o fechamento recusava para sempre, porque o último
/// veredito seguia sendo o que reprovou.
///
/// Sem veredito nenhum, só pede revisão a entrega que responde a um pedido da
/// rodada — a entrega mais nova que o pedido mais novo daquela onda. A onda
/// entregue antes de a rodada existir não tem pedido nenhum, e cobrar revisão
/// dela é cobrar de novo um trabalho já feito, provado pelo código que entrou.
/// A onda que saiu do plano não é revisada.
fn waves_awaiting_review(log: &SpecLog) -> Vec<u64> {
    let planned = log.planned_waves();
    let last_verdict: BTreeMap<u64, u64> =
        log.verdicts_by_wave().into_iter().filter_map(|(n, verdicts)| verdicts.last().map(|v| (n, v.id))).collect();
    let last_send = log.last_by_wave("send");
    log.last_by_wave("delivered")
        .into_iter()
        .filter(|(n, _)| planned.contains(n))
        .filter(|(n, id)| last_verdict.get(n).is_none_or(|judged| judged < id))
        .filter(|(n, id)| last_verdict.contains_key(n) || last_send.get(n).is_some_and(|sent| sent < id))
        .map(|(n, _)| n)
        .collect()
}

/// Os itens que o pedido de uma onda leva: os números de tudo que entrou nele.
fn sent_items(log: &SpecLog, wave: u64) -> Vec<u64> {
    log.step(&mustard_core::domain::spec_events::Step::Dispatch { wave })
        .into_iter()
        .map(|e| e.id)
        .collect()
}

/// O estado de cada onda que já saiu, pela mesma leitura que decide o que a
/// rodada despacha: em andamento, entregue à espera da revisão, reprovada na
/// última revisão ou entregue e aprovada. A onda que não está aqui está por
/// fazer. A página da spec mostra este estado.
pub(crate) fn wave_states(log: &SpecLog) -> mustard_core::view::document::WaveStates {
    use mustard_core::view::document::WaveState;
    let running = waves_in_progress(log);
    let awaiting: BTreeSet<u64> = waves_awaiting_review(log).into_iter().collect();
    let rejected = log.last_rejected();
    let done = waves_done(log, &running);
    let mut states = mustard_core::view::document::WaveStates::new();
    for n in running.keys().chain(&awaiting).chain(rejected.keys()).chain(&done) {
        let state = if running.contains_key(n) {
            WaveState::Running
        } else if awaiting.contains(n) {
            WaveState::Delivered
        } else if rejected.contains_key(n) {
            WaveState::Rejected
        } else {
            WaveState::Approved
        };
        states.insert(*n, state);
    }
    states
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::spec_events::write::{record_open, seed_at, WriteOpts};
    use tempfile::tempdir;

    fn write(root: &Path, spec: &str, event_type: &str, body: Value) -> Value {
        seed_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: Some(spec.to_string()),
            event_type: event_type.into(),
            json: body.to_string(),
        })
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

    /// Um projeto com arquivos no git e uma spec já aprovada, com o plano que
    /// o teste pedir: uma entrada por onda, com os arquivos das tarefas dela e
    /// as ondas de que ela depende.
    fn approved(root: &Path, spec: &str, plan: &[(u64, &[&str], &[u64])]) {
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        for (_, files, _) in plan {
            for file in *files {
                let path = root.join(file);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, "fn um() {}\n").unwrap();
            }
        }
        git_at(root, &["init", "-q"]);
        git_at(root, &["add", "-A"]);
        git_at(root, &["commit", "-q", "-m", "semente"]);
        git_at(root, &["config", "user.email", "t@t"]);
        git_at(root, &["config", "user.name", "t"]);
        git_at(root, &["config", "commit.gpgsign", "false"]);

        assert_eq!(record_open(root, spec, &format!("feature/{spec}"), "dev"), Ok(true));
        let said = id_of(&write(root, spec, "message", json!({"author": "user", "text": "o objetivo"})));
        let crit = id_of(&write(
            root,
            spec,
            "criterion",
            json!({"when": "a onda roda", "then": "a suíte passa", "proof": "cargo test", "origin": said}),
        ));
        for (n, files, depends) in plan {
            let mut wave = json!({"n": n, "text": format!("Onda {n}."), "criteria": [crit],
                "done_when": "A suíte passa.", "origin": said});
            if !depends.is_empty() {
                wave["depends_on"] = json!(depends);
            }
            write(root, spec, "wave", wave);
            let declared: Vec<Value> = files.iter().map(|f| json!({"path": f})).collect();
            write(root, spec, "task", json!({"wave": n, "text": format!("Tarefa da onda {n}."),
                "files": declared, "origin": said}));
        }
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join(spec));
    }

    fn round(root: &Path, spec: &str, report: Option<&str>) -> Value {
        round_for(
            &RoundOpts { root: root.to_path_buf(), spec: Some(spec.to_string()), report: report.map(str::to_string) },
            None,
        )
    }

    /// Uma linha do fim de um agente, como os textos dele ensinam.
    fn line(tag: &str, body: Value) -> String {
        format!("<{tag}>{body}</{tag}>")
    }

    /// A linha `DELIVERED` da onda `wave`, com o resumo do commit. Cada
    /// arquivo entregue que existe ganha uma linha, para o commit ter o que
    /// levar.
    fn delivered(root: &Path, wave: u64, text: &str, files: &[&str]) -> String {
        for file in files {
            let path = root.join(file);
            if let Ok(before) = std::fs::read_to_string(&path) {
                std::fs::write(&path, format!("{before}// {text}\n")).unwrap();
            }
        }
        line("DELIVERED", json!({"wave": wave, "text": text, "files": files, "commit": format!("a onda {wave} saiu")}))
    }

    /// A linha `VERDICT` da onda `wave`, com o critério pelo código.
    fn verdict(wave: u64, result: &str, text: &str) -> String {
        line("VERDICT", json!({"wave": wave, "result": result, "text": text,
            "criteria": [{"criterion": "MSTD-CRIT-0001", "tests_rule": true}]}))
    }

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
        assert!(prompt.contains("--term MSTD-TASK-0001"), "{prompt}");

        // O envio gravado guarda o pedido exato, letra por letra.
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let sent: Vec<&SpecEvent> =
            log.visible().into_iter().filter(|e| e.event_type == "send").collect();
        assert_eq!(sent.len(), 1, "um envio por onda despachada");
        assert_eq!(sent[0].str_field("text"), Some(prompt.as_str()));
        assert_eq!(sent[0].wave(), Some(1));
    }

    /// Duas ondas sem dependência que mexem no mesmo arquivo nunca saem
    /// juntas, e o teto de compilações do projeto limita quantas saem.
    #[test]
    fn two_waves_never_go_out_together_when_they_share_a_file_and_the_cap_holds() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/a.rs"], &[]), (3, &["src/c.rs"], &[])]);

        let out = round(root, "x", None);
        let waves: Vec<u64> =
            out["dispatch"].as_array().cloned().unwrap_or_default().iter().filter_map(|d| d["wave"].as_u64()).collect();
        assert_eq!(waves, vec![1, 3], "a onda 2 divide arquivo com a 1: {out}");

        // Com o teto do projeto em 1, só uma onda sai por rodada.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":1}"#).unwrap();
        let out = round(root, "x", None);
        assert_eq!(out["dispatch"].as_array().map(Vec::len), Some(1), "{out}");
    }

    /// A onda que já entregou não é despachada de novo, mesmo sem pedido
    /// nenhum: a onda entregue antes de a rodada existir não tem pedido, e
    /// mandá-la sair seria mandar refazer um trabalho já feito.
    #[test]
    fn a_wave_that_already_delivered_does_not_go_out_again() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        write(
            root,
            "x",
            "delivered",
            json!({"wave": 1, "text": "A onda 1 saiu antes da rodada.", "files": ["src/a.rs"]}),
        );

        let out = round(root, "x", None);
        let waves: Vec<u64> =
            out["dispatch"].as_array().cloned().unwrap_or_default().iter().filter_map(|d| d["wave"].as_u64()).collect();
        assert_eq!(waves, vec![2], "a onda 1 já tem entrega: {out}");
    }

    /// A onda entregue antes de a rodada existir também não entra na lista de
    /// revisões: sem veredito nenhum e sem pedido, a entrega dela não responde
    /// a nada que esta rodada tenha mandado fazer.
    #[test]
    fn a_wave_delivered_before_the_round_is_not_asked_for_review() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        write(
            root,
            "x",
            "delivered",
            json!({"wave": 1, "text": "A onda 1 saiu antes da rodada.", "files": ["src/a.rs"]}),
        );

        let out = round(root, "x", None);
        assert_eq!(out["reviews"], json!([]), "a onda 1 entregou antes e nunca foi pedida: {out}");
    }

    /// A rodada grava o que cada onda entregou e o veredito da revisão dela, e
    /// passa a pedir a revisão do que entregou depois do último veredito.
    #[test]
    fn what_came_back_becomes_the_delivered_and_the_verdict_of_the_wave() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);

        let out = round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        assert_eq!(out["ok"], json!(true), "{out}");
        let reviews = out["reviews"].as_array().cloned().unwrap_or_default();
        assert_eq!(reviews.len(), 1, "a onda entregue sem veredito pede revisão: {out}");
        assert_eq!(reviews[0]["wave"], json!(1), "{out}");

        let out = round(root, "x", Some(&verdict(1, "approved", "passou")));
        assert_eq!(out["reviews"], json!([]), "o veredito é mais novo que a entrega dele: {out}");
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        assert_eq!(log.visible().iter().filter(|e| e.event_type == "delivered").count(), 1);
        let judged: Vec<&SpecEvent> = log.visible().into_iter().filter(|e| e.event_type == "verdict").collect();
        assert_eq!(judged.len(), 1);
        let criterion = log.visible().into_iter().find(|e| e.event_type == "criterion").map(|e| e.id).unwrap();
        assert_eq!(judged[0].fields["criteria"][0]["criterion"], json!(criterion), "the code became the number");
    }

    /// A onda cuja última revisão reprovou volta a ser despachada, e uma vez
    /// só: depois que o conserto sai, a mesma reprovação não a manda de novo.
    /// Entregue o conserto, ele volta para a revisão — é o que fecha o ciclo,
    /// porque sem uma revisão nova o veredito que reprovou valeria para sempre.
    #[test]
    fn a_rejected_wave_goes_out_again_and_only_once() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let first = round(root, "x", None);
        assert_eq!(first["dispatch"].as_array().map(Vec::len), Some(1), "{first}");

        round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        let again = round(root, "x", Some(&verdict(1, "rejected", "faltou o teste")));
        assert_eq!(again["dispatch"].as_array().map(Vec::len), Some(1), "a onda reprovada volta a sair: {again}");

        let quiet = round(root, "x", None);
        assert_eq!(quiet["dispatch"], json!([]), "o conserto já saiu, e a onda espera a revisão dele: {quiet}");
        assert_eq!(quiet["reviews"], json!([]), "o conserto ainda não voltou: nada a revisar: {quiet}");

        // O conserto entregue é mais novo que a reprovação, e por isso pede
        // revisão: é a revisão nova que tira o veredito velho da frente.
        let back = round(root, "x", Some(&delivered(root, 1, "O teste entrou.", &["src/a.rs"])));
        let waves: Vec<u64> = back["reviews"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|review| review["wave"].as_u64())
            .collect();
        assert_eq!(waves, vec![1], "o conserto entregue volta para a revisão: {back}");
    }

    /// O estado de cada onda que a página mostra acompanha a rodada: em
    /// andamento depois do pedido, entregue à espera da revisão, reprovada
    /// pela última revisão, em andamento de novo com o conserto e aprovada no
    /// fim; a onda que ainda não saiu não aparece, e a página a mostra por
    /// fazer.
    #[test]
    fn the_wave_states_follow_the_round() {
        use mustard_core::view::document::WaveState::{Approved, Delivered, Rejected, Running};
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[1])]);
        let states = || {
            let log = mustard_core::io::spec_events::read(&root.join(".claude/spec/x/spec.ndjson")).unwrap().unwrap();
            wave_states(&log).into_iter().collect::<Vec<_>>()
        };
        assert_eq!(states(), [], "nothing went out yet");
        round(root, "x", None);
        assert_eq!(states(), [(1, Running)]);
        round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        assert_eq!(states(), [(1, Delivered)], "the delivery waits for its review, and the last wave waits for it");
        // A reprovação gravada antes de a rodada seguinte despachar o conserto.
        let events = root.join(".claude/spec/x/spec.ndjson");
        let log = mustard_core::io::spec_events::read(&events).unwrap().unwrap();
        let crit = log.events.iter().find(|e| e.event_type == "criterion").map(|e| e.id).unwrap();
        let rejected = json!({"author": "review", "wave": 1, "result": "rejected", "text": "faltou o teste",
            "criteria": [{"criterion": crit, "tests_rule": true}]});
        mustard_core::io::spec_events::write(&events, "verdict", rejected.as_object().cloned().unwrap(), &[]).unwrap();
        assert_eq!(states(), [(1, Rejected)]);
        round(root, "x", None);
        assert_eq!(states(), [(1, Running)], "the fix went out");
        round(root, "x", Some(&delivered(root, 1, "O teste entrou.", &["src/a.rs"])));
        round(root, "x", Some(&verdict(1, "approved", "pronto")));
        assert_eq!(states(), [(1, Approved), (2, Running)]);
    }

    /// A onda que ganha versão nova depois do pedido volta para a fila, uma
    /// vez só: o pedido gravado descrevia o plano antigo. A tarefa que muda de
    /// onda replaneja as duas. A onda que já entregou não volta por
    /// replanejamento — só a reprovação a devolve.
    #[test]
    fn a_wave_replanned_after_its_send_goes_out_again_and_only_once() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        let first = round(root, "x", None);
        assert_eq!(first["dispatch"].as_array().map(Vec::len), Some(2), "{first}");

        let path = store::spec_file(root, "x").unwrap();
        let current = |kind: &str, n: u64| -> Value {
            let log = store::read(&path).unwrap().unwrap();
            let event = log
                .visible()
                .into_iter()
                .find(|e| e.event_type == kind && e.wave() == Some(n))
                .unwrap_or_else(|| panic!("sem {kind} da onda {n}"));
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
        let waves: Vec<u64> =
            again["dispatch"].as_array().cloned().unwrap_or_default().iter().filter_map(|d| d["wave"].as_u64()).collect();
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

    /// O que uma onda entregou acima do teto de caracteres é recusado, e nada
    /// é gravado.
    #[test]
    fn a_delivered_report_over_the_character_cap_is_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);

        let long = "a".repeat(DELIVERED_MAX_CHARS + 1);
        let refused = round(root, "x", Some(&delivered(root, 1, &long, &["src/a.rs"])));
        assert_eq!(refused["reason"], json!("delivered-too-long"), "{refused}");
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        assert_eq!(log.visible().iter().filter(|e| e.event_type == "delivered").count(), 0);
    }

    /// A resposta do usuário à pergunta `question`, dada pela testemunha dos
    /// gestos, como o harness a entrega depois do clique.
    fn click(root: &Path, session: &str, question: &str, answer: &str) {
        use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger};
        let input = HookInput {
            hook_event_name: Some("PostToolUse".to_string()),
            tool_name: Some("AskUserQuestion".to_string()),
            session_id: Some(session.to_string()),
            tool_input: json!({ "questions": [{ "question": question,
                "options": [{ "label": "Aceitar" }, { "label": "Recusar" }] }] }),
            raw: json!({ "tool_response": { "answers": { question: answer } } }),
            ..HookInput::default()
        };
        let ctx = Ctx::for_test(root.to_string_lossy().into_owned(), Some(Trigger::PostToolUse));
        crate::hooks::observe::approval_witness::ApprovalWitness.evaluate(&input, &ctx).expect("never errors");
    }

    fn delivered_count(root: &Path) -> usize {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        log.visible().iter().filter(|e| e.event_type == "delivered").count()
    }

    /// O agente que diz que o plano da onda não funciona para a rodada até o
    /// "sim" do usuário, e o "sim" é o clique em "Aceitar" na pergunta da
    /// mudança, gravado pela testemunha. A recusa mostra a mudança e a
    /// pergunta; uma mensagem escrita à mão pelo modelo, com a mesma pergunta
    /// e a mesma resposta, não destrava nada; o clique em "Recusar" também
    /// não; o clique em "Aceitar" destrava, e a rodada grava o que a onda
    /// entregou.
    #[test]
    fn a_wave_that_says_its_plan_does_not_work_stops_the_round_until_the_users_click() {
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
        let report = line("DELIVERED", json!({"wave": 1, "text": "Parei.", "files": ["src/a.rs"], "replan": change}));
        let stopped = round(root, "x", Some(&report));
        assert_eq!(stopped["reason"], json!("wave-plan-does-not-work"), "{stopped}");
        let question = stopped["question"].as_str().unwrap_or_default().to_string();
        assert_eq!(question, change_question(&replan_code(1, change), Locale::PtBr), "{stopped}");
        assert_eq!(stopped["options"], json!(["Aceitar", "Recusar"]), "{stopped}");
        let hint = stopped["hint"].as_str().unwrap_or_default();
        assert!(hint.contains(change) && hint.contains(&question), "{hint}");
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
        let witness = json!({ "question": question, "answer": "Aceitar" });
        let forged = by_hand(json!({ "author": "user", "text": format!("{question}\nAceitar"), "witness": witness }));
        assert_eq!(forged["reason"], json!("user-message-by-hook"), "{forged}");
        let own = by_hand(json!({ "text": format!("{question}\nAceitar"), "witness": witness }));
        assert_eq!(own["reason"], json!("user-message-by-hook"), "{own}");
        let still = round(root, "x", Some(&report));
        assert_eq!(still["reason"], json!("wave-plan-does-not-work"), "a forged yes accepts nothing: {still}");

        click(root, session, &question, "Recusar");
        let refused = round(root, "x", Some(&report));
        assert_eq!(refused["reason"], json!("wave-plan-does-not-work"), "a declined change stays stopped: {refused}");

        // O "sim" de uma mudança nunca serve para outra.
        click(root, session, &change_question(&replan_code(1, "Outra mudança."), Locale::PtBr), "Aceitar");
        let other = round(root, "x", Some(&report));
        assert_eq!(other["reason"], json!("wave-plan-does-not-work"), "{other}");

        click(root, session, &question, "Aceitar");
        let went = round(root, "x", Some(&report));
        assert_eq!(went["ok"], json!(true), "{went}");
        assert_eq!(delivered_count(root), 1, "the round records what the wave delivered");
    }

    /// A mensagem do commit tem título e corpo dentro do teto e nunca traz o
    /// link da conversa, o nome do modelo, a assinatura de coautoria nem o
    /// e-mail de ninguém; o commit é recusado até o e-mail sair.
    #[test]
    fn the_commit_message_is_checked_before_the_commit_is_made() {
        assert!(check_commit_text("feat: a soma", "O corpo.").is_ok());
        let cases = [
            ("a".repeat(MESSAGE_TITLE_MAX + 1), String::new(), "commit-too-long"),
            ("feat: a soma".into(), "a".repeat(MESSAGE_BODY_MAX + 1), "commit-too-long"),
            ("feat: a soma".into(), "https://claude.ai/code/x".into(), "commit-forbidden-text"),
            ("feat: a soma".into(), "Feito com Claude.".into(), "commit-forbidden-text"),
            ("feat: a soma".into(), "Co-Authored-By: alguem".into(), "commit-forbidden-text"),
            ("feat: a soma".into(), "pedido de fulano@empresa.com.br".into(), "commit-forbidden-text"),
        ];
        for (title, body, reason) in cases {
            let refused = check_commit_text(&title, &body).expect_err(&format!("{title} / {body}"));
            assert_eq!(refused.reason(), reason, "{title} / {body}");
        }
    }

    /// A mensagem do commit é conferida antes de qualquer gravação: o
    /// relatório com e-mail no corpo é recusado sem gravar o entregou, e a
    /// chamada seguinte, com a mensagem limpa, grava uma vez só.
    #[test]
    fn a_report_with_a_bad_commit_message_records_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);

        std::fs::write(root.join("src/a.rs"), "fn um() {}\nfn dois() {}\n").unwrap();
        let delivered = |summary: &str| {
            line("DELIVERED", json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"], "commit": summary}))
        };
        let refused = round(root, "x", Some(&delivered("pedido de fulano@empresa.com.br")));
        assert_eq!(refused["reason"], json!("commit-forbidden-text"), "{refused}");
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        assert_eq!(log.visible().iter().filter(|e| e.event_type == "delivered").count(), 0, "nada foi gravado");

        let went = round(root, "x", Some(&delivered("a soma sai")));
        assert_eq!(went["ok"], json!(true), "{went}");
        let log = store::read(&path).unwrap().unwrap();
        assert_eq!(log.visible().iter().filter(|e| e.event_type == "delivered").count(), 1, "sem duplicar");
    }

    /// A rodada faz o commit da rodada e grava o código dele na spec.
    #[test]
    fn the_round_commits_and_records_the_commit_on_the_spec() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        std::fs::write(root.join("src/a.rs"), "fn um() {}\nfn dois() {}\n").unwrap();
        git_at(root, &["config", "user.email", "t@t"]);
        git_at(root, &["config", "user.name", "t"]);
        git_at(root, &["config", "commit.gpgsign", "false"]);

        let report = line("DELIVERED", json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"],
            "commit": "a soma sai"}));
        let out = round(root, "x", Some(&report));
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["commit"]["title"], json!("feat(onda-1): a soma sai"), "{out}");
        let sha = out["commit"]["sha"].as_str().unwrap_or_default().to_string();
        assert_eq!(sha.len(), 40, "{out}");
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let commit = log.visible().into_iter().find(|e| e.event_type == "commit").expect("commit");
        assert_eq!(commit.str_field("sha"), Some(sha.as_str()));
        assert_eq!(commit.ints("waves"), vec![1]);
    }

    /// A linha do fim que o texto de um agente ensina, tirada do próprio
    /// texto, com os valores de exemplo trocados por `values`.
    fn taught_line(template: &str, tag: &str, values: &[(&str, &str)]) -> String {
        let open = format!("<{tag}>");
        let found = template.lines().find(|l| l.starts_with(&open)).unwrap_or_else(|| panic!("no {tag} line"));
        values.iter().fold(found.to_string(), |line, (from, to)| line.replacen(from, to, 1))
    }

    /// O subject e o corpo do último commit.
    fn last_commit(root: &Path) -> (String, String) {
        let out = Command::new("git").args(["log", "-1", "--format=%s%n%b"]).current_dir(root).output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        let (subject, body) = text.split_once('\n').unwrap_or((&text, ""));
        (subject.to_string(), body.trim().to_string())
    }

    /// A rodada aceita as linhas do fim exatamente como os textos dos agentes
    /// as ensinam, nos dois idiomas, dentro da resposta inteira do agente:
    /// grava a entrega e o veredito, e o commit sai com o título e o corpo
    /// montados do resumo. A resposta da rodada ensina o mesmo formato.
    #[test]
    fn the_round_takes_the_closing_lines_exactly_as_the_agent_texts_teach_and_commits() {
        for (lang, answer) in [(Locale::PtBr, "Entreguei a soma."), (Locale::EnUs, "I delivered the sum.")] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            approved(root, "x", &[(1, &["src/a.rs"], &[])]);
            let config = format!(r#"{{"language":{{"text":"{}"}}}}"#, lang.as_str());
            std::fs::write(root.join("mustard.json"), config).unwrap();
            let first = round(root, "x", None);
            let taught = translate("round.report", lang);
            assert!(first["next"].as_str().unwrap_or_default().contains(taught), "{first}");
            assert!(taught.contains("<DELIVERED>") && taught.contains("<VERDICT>") && taught.contains("\"commit\""));

            std::fs::write(root.join("src/a.rs"), "fn um() {}\nfn soma() {}\n").unwrap();
            let (wave_text, review_text) = mustard_core::agent_texts(lang)
                .iter()
                .fold((String::new(), String::new()), |(w, r), (name, body)| match *name {
                    "wave" => ((*body).to_string(), r),
                    "review" => (w, (*body).to_string()),
                    _ => (w, r),
                });
            let example = if lang == Locale::PtBr {
                [("<a entrega>", "A soma saiu, com o teste."), ("caminho/do/arquivo.rs", "src/a.rs"), ("<o resumo do commit>", "a soma sai")]
            } else {
                [("<the delivery>", "The sum is out, with its test."), ("path/to/file.rs", "src/a.rs"), ("<the commit summary>", "the sum ships")]
            };
            let line = taught_line(&wave_text, "DELIVERED", &example);
            let whole = format!("{answer}\n\nOs arquivos mudaram.\n\n{line}\n");
            let back = round(root, "x", Some(&whole));
            assert_eq!(back["ok"], json!(true), "{lang:?}: {back}");
            let (subject, body) = last_commit(root);
            let (title, summary) = if lang == Locale::PtBr {
                ("feat(onda-1): a soma sai", "- onda 1: a soma sai")
            } else {
                ("feat(wave-1): the sum ships", "- wave 1: the sum ships")
            };
            assert_eq!(subject, title, "{lang:?}");
            assert_eq!(body, summary, "{lang:?}");
            assert_eq!(back["commit"]["title"], json!(title), "{back}");
            let shown = Command::new("git").args(["show", "--name-only", "--format=", "HEAD"]).current_dir(root).output().unwrap();
            assert_eq!(String::from_utf8_lossy(&shown.stdout).trim(), "src/a.rs");

            let judged = round(root, "x", Some(&taught_line(&review_text, "VERDICT", &[])));
            assert_eq!(judged["ok"], json!(true), "{lang:?}: {judged}");
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            let visible = log.visible();
            let delivered = visible.iter().find(|e| e.event_type == "delivered").expect("the delivery");
            assert_eq!(delivered.str_field("text"), Some(example[0].1));
            assert_eq!(delivered.wave(), Some(1));
            let commit = visible.iter().find(|e| e.event_type == "commit").expect("the commit");
            assert_eq!(commit.str_field("title"), Some(title));
            let verdict = visible.iter().find(|e| e.event_type == "verdict").expect("the verdict");
            assert_eq!(verdict.str_field("result"), Some("approved"));
            let criterion = visible.iter().find(|e| e.event_type == "criterion").map(|e| e.id);
            assert_eq!(verdict.fields["criteria"][0]["criterion"].as_u64(), criterion);
            assert_eq!(judged["command"], json!("mustard-rt run close --spec x"), "{judged}");
        }
    }

    /// Sem a linha do fim, ou com a linha sem o resumo do commit, a rodada
    /// recusa e não grava nada; um critério que a spec não tem também.
    #[test]
    fn a_report_without_the_closing_line_or_its_fields_is_refused_and_records_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let lines_before = std::fs::read_to_string(store::spec_file(root, "x").unwrap()).unwrap().lines().count();

        let missing = round(root, "x", Some("Entreguei a soma, e os arquivos mudaram."));
        assert_eq!(missing["reason"], json!("round-line-missing"), "{missing}");
        assert_eq!(missing["hint"], json!(translate("round.line_missing", Locale::PtBr)), "{missing}");

        let without = line("DELIVERED", json!({"wave": 1, "text": "Saiu.", "files": ["src/a.rs"]}));
        let refused = round(root, "x", Some(&without));
        assert_eq!(refused["reason"], json!("round-line-field-missing"), "{refused}");
        let expected = translate("round.line_field", Locale::PtBr).replace("{line}", "DELIVERED").replace("{field}", "commit");
        assert_eq!(refused["hint"], json!(expected), "{refused}");

        let no_wave = line("VERDICT", json!({"result": "approved", "text": "passou", "criteria": []}));
        assert_eq!(round(root, "x", Some(&no_wave))["reason"], json!("round-line-field-missing"));

        let unknown = line("VERDICT", json!({"wave": 1, "result": "approved", "text": "passou",
            "criteria": [{"criterion": "MSTD-CRIT-0099", "tests_rule": true}]}));
        let refused = round(root, "x", Some(&unknown));
        assert_eq!(refused["ok"], json!(false), "{refused}");

        let lines_after = std::fs::read_to_string(store::spec_file(root, "x").unwrap()).unwrap().lines().count();
        assert_eq!(lines_after, lines_before, "nothing was recorded");
    }

    /// Tudo é conferido antes da primeira gravação. O caminho que não está no
    /// disco nem no git é recusado sozinho e junto de um caminho certo, e a
    /// chamada seguinte, com a linha corrigida, grava a entrega uma vez só. O
    /// veredito sem resultado depois de um válido também é recusado sem
    /// deixar o primeiro gravado.
    #[test]
    fn a_wrong_path_or_a_line_missing_a_field_is_refused_before_anything_is_recorded() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let spec_lines = || std::fs::read_to_string(store::spec_file(root, "x").unwrap()).unwrap().lines().count();
        let before = spec_lines();

        for files in [&["src/nao_existe.rs"][..], &["src/a.rs", "src/nao_existe.rs"][..]] {
            let wrong = delivered(root, 1, "Saiu.", files);
            let refused = round(root, "x", Some(&wrong));
            assert_eq!(refused["reason"], json!("round-file-unknown"), "{files:?}: {refused}");
            let expected = translate("round.file_unknown", Locale::PtBr)
                .replace("{file}", "src/nao_existe.rs")
                .replace("{wave}", "1");
            assert_eq!(refused["hint"], json!(expected), "{refused}");
            assert_eq!(spec_lines(), before, "{files:?}: nothing was recorded");
        }

        let valid = verdict(1, "approved", "passou");
        let no_result = line("VERDICT", json!({"wave": 1, "text": "sem resultado",
            "criteria": [{"criterion": "MSTD-CRIT-0001", "tests_rule": true}]}));
        let refused = round(root, "x", Some(&format!("{valid}\n{no_result}")));
        assert_eq!(refused["reason"], json!("missing-field"), "{refused}");
        assert_eq!(spec_lines(), before, "the valid verdict was not recorded either");

        let went = round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        assert_eq!(went["ok"], json!(true), "{went}");
        assert_eq!(delivered_count(root), 1, "the corrected line records the delivery once");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert!(log.visible().iter().all(|e| e.event_type != "verdict"), "no verdict was left behind");
    }

    /// A lista de arquivos entregue é conferida contra os arquivos
    /// reservados: o arquivo de outra onda ainda em andamento é recusado, e o
    /// de uma onda que já não está em andamento passa.
    #[test]
    fn a_delivered_file_reserved_for_another_wave_in_flight_is_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 2]);

        let refused = round(root, "x", Some(&delivered(root, 1, "Mexi na b também.", &["src/a.rs", "src/b.rs"])));
        assert_eq!(refused["reason"], json!("round-file-reserved"), "{refused}");
        let expected = translate("round.file_reserved", Locale::PtBr)
            .replace("{file}", "src/b.rs")
            .replace("{wave}", "1")
            .replace("{other}", "2");
        assert_eq!(refused["hint"], json!(expected), "{refused}");
        assert_eq!(delivered_count(root), 0);

        // As duas voltam juntas: nenhuma está mais em andamento.
        let both = format!("{}\n{}", delivered(root, 1, "A.", &["src/a.rs"]), delivered(root, 2, "B.", &["src/b.rs"]));
        let went = round(root, "x", Some(&both));
        assert_eq!(went["ok"], json!(true), "{went}");
        assert_eq!(last_commit(root).0, "feat(ondas-1-2): a onda 1 saiu");
    }

    /// O conserto que diz as ondas que fecha grava a entrega também nelas, o
    /// que pede a revisão de cada uma de novo sem mandá-las refazer; o commit
    /// é de conserto e leva as ondas consertadas.
    #[test]
    fn a_fix_records_the_delivery_on_the_waves_it_closes_and_asks_their_review_again() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":1}"#).unwrap();
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1]);
        let back = round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        assert_eq!(waves_in(&back, "dispatch"), vec![2], "{back}");
        // A onda 1 é reprovada com a única vaga ocupada pela 2: ela espera na
        // fila, e quem entrega o conserto dela é a 2.
        let rejected = round(root, "x", Some(&verdict(1, "rejected", "faltou o commit")));
        assert_eq!(waves_in(&rejected, "dispatch"), Vec::<u64>::new(), "{rejected}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert!(waves_to_redo(&log).contains(&1), "the rejected wave waits in the queue");

        std::fs::write(root.join("src/b.rs"), "fn um() {}\nfn conserto() {}\n").unwrap();
        let fix = line("DELIVERED", json!({"wave": 2, "text": "Consertei a onda 1.", "files": ["src/b.rs"],
            "commit": "o commit sai do resumo", "fixes": [1]}));
        let out = round(root, "x", Some(&fix));
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(waves_in(&out, "reviews"), vec![1, 2], "{out}");
        assert_eq!(waves_in(&out, "dispatch"), Vec::<u64>::new(), "the fixed wave is not redone: {out}");
        let (subject, body) = last_commit(root);
        assert_eq!(subject, "fix(onda-2): o commit sai do resumo");
        assert_eq!(body, "- onda 2: o commit sai do resumo (conserta: onda 1)");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let visible = log.visible();
        let fixed_delivery = visible.iter().rfind(|e| e.event_type == "delivered" && e.wave() == Some(1)).unwrap();
        assert_eq!(fixed_delivery.str_field("text"), Some("Consertei a onda 1."));
        let commit = visible.iter().rfind(|e| e.event_type == "commit").unwrap();
        assert_eq!(commit.ints("waves"), vec![2, 1]);
    }

    /// O pedido da onda nova traz os comandos do projeto e a outra onda que
    /// sai junto, com o arquivo dela. O do conserto traz também o veredito,
    /// a entrega anterior e a decisão gravada depois do envio; a revisão do
    /// conserto, as mesmas linhas, a entrega dele e a cópia no commit dele.
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
        for code in ["--term MSTD-VERD-0001`", "--term MSTD-DELIV-0001`", "--term MSTD-DEC-0001`"] {
            assert!(fix.split("\n## ").nth(1).unwrap_or_default().contains(code), "{code}: {fix}");
        }

        let back = round(root, "x", Some(&delivered(root, 1, "Teste acrescentado.", &["src/a.rs"])));
        let review = text(&back, "reviews", 1);
        let sha = back["commit"]["sha"].as_str().unwrap_or_default();
        assert!(review.contains(&format!("## Conserto\n\n{}", translate("prompt.fix.review", Locale::PtBr))), "{review}");
        let fix_part = review.split("\n## ").nth(1).unwrap_or_default();
        for line in ["--term MSTD-VERD-0001`", "--term MSTD-DELIV-0001`", "--term MSTD-DEC-0001`"] {
            assert!(fix_part.contains(line), "{line}: {review}");
        }
        assert!(review.contains("## O que esta onda entregou\n\n- MSTD-DELIV-0002 (entregou) — `mustard-rt run read waves --spec x --term MSTD-DELIV-0002`\n\n"), "{review}");
        assert!(!sha.is_empty() && review.contains(&format!("<pasta da cópia> {sha}`")), "{sha}: {review}");
    }

    /// A prova nova de um critério cujo teste mudou de nome vira a versão nova
    /// do critério, com o mesmo resto; a prova nova que sai verde sem rodar
    /// teste nenhum é avisada pelo código do critério.
    #[test]
    fn a_new_proof_becomes_the_criterions_new_version_and_one_that_runs_no_test_is_warned() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/lib.rs"], &[])]);
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"prova\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
        )
        .unwrap();
        std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
        round(root, "x", None);
        std::fs::write(
            root.join("src/lib.rs"),
            "#[cfg(test)]\nmod tests {\n    #[test]\n    fn soma_nova() { assert_eq!(1 + 1, 2); }\n}\n",
        )
        .unwrap();
        let proof = |name: &str| format!("cargo test --lib -- tests::{name} --exact");
        let report = |name: &str, summary: &str| {
            line("DELIVERED", json!({"wave": 1, "text": "O teste mudou de nome.", "files": ["src/lib.rs"],
                "commit": summary, "proofs": [{"criterion": "MSTD-CRIT-0001", "proof": proof(name)}]}))
        };
        let out = round(root, "x", Some(&report("soma_nova", "o teste muda de nome")));
        assert_eq!(out["ok"], json!(true), "{out}");
        assert!(out.get("warnings").is_none(), "the right name runs a test: {out}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let visible = log.visible();
        let criteria: Vec<&&SpecEvent> = visible.iter().filter(|e| e.event_type == "criterion").collect();
        assert_eq!(criteria.len(), 1, "the new version replaces the old one");
        assert_eq!(criteria[0].str_field("proof"), Some(proof("soma_nova").as_str()));
        assert_eq!(criteria[0].str_field("when"), Some("a onda roda"));
        assert!(criteria[0].int("replaces").is_some());
        assert_eq!(log.codes()[&criteria[0].id], "MSTD-CRIT-0001");

        std::fs::write(root.join("src/lib.rs"), "#[cfg(test)]\nmod tests {\n    #[test]\n    fn soma_nova() {}\n}\n").unwrap();
        let out = round(root, "x", Some(&report("soma", "a prova errada")));
        assert_eq!(out["ok"], json!(true), "{out}");
        let expected = translate("round.proof_ran_no_test", Locale::PtBr).replace("{code}", "MSTD-CRIT-0001");
        assert_eq!(out["warnings"], json!([{"reason": "proof-ran-no-test", "hint": expected}]), "{out}");
    }

    /// As ondas de uma resposta da rodada, num campo dela.
    fn waves_in(out: &Value, field: &str) -> Vec<u64> {
        out[field].as_array().cloned().unwrap_or_default().iter().filter_map(|d| d["wave"].as_u64()).collect()
    }

    /// A versão nova da tarefa da onda `n`: o plano da onda muda depois do
    /// pedido dela.
    fn replan(root: &Path, n: u64) {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let task = log
            .visible()
            .into_iter()
            .find(|e| e.event_type == "task" && e.wave() == Some(n))
            .unwrap_or_else(|| panic!("sem tarefa da onda {n}"));
        let mut fields = task.fields.clone();
        for key in ["v", "id", "code", "at", "search", "type", "author"] {
            fields.remove(key);
        }
        let mut body = Value::Object(fields);
        body["replaces"] = json!(task.id);
        body["text"] = json!(format!("Tarefa da onda {n}, revista."));
        id_of(&write(root, "x", "task", body));
    }

    /// A onda em andamento — com pedido e sem entrega depois dele — ocupa uma
    /// vaga do limite e reserva os arquivos das tarefas dela: a rodada não
    /// solta a onda que divide arquivo com ela, nem passa do limite contando
    /// as que já saíram. O pedido anterior ao replanejamento da onda não conta
    /// como andamento. A resposta lista as ondas em andamento com o código do
    /// pedido de cada uma.
    #[test]
    fn a_wave_in_flight_holds_a_slot_and_its_files_and_a_send_before_the_replan_does_not_count() {
        // A onda 2 divide arquivo com a 1, que está em andamento.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/a.rs"], &[]), (3, &["src/c.rs"], &[])]);
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1, 3], "{first}");
        let out = round(root, "x", Some(&delivered(root, 3, "Saiu.", &["src/c.rs"])));
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(waves_in(&out, "dispatch"), Vec::<u64>::new(), "a onda 2 divide arquivo com a 1: {out}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let sent = log
            .visible()
            .into_iter()
            .find(|e| e.event_type == "send" && e.wave() == Some(1))
            .map(|e| codes[&e.id].clone())
            .unwrap();
        assert_eq!(out["running"], json!([{"wave": 1, "send": sent}]), "{out}");

        // Duas ondas em andamento enchem o limite de duas.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[]), (3, &["src/c.rs"], &[])]);
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 2]);
        let full = round(root, "x", None);
        assert_eq!(waves_in(&full, "dispatch"), Vec::<u64>::new(), "as duas vagas estão ocupadas: {full}");
        assert_eq!(waves_in(&full, "running"), vec![1, 2], "{full}");

        // Replanejada depois do pedido, a onda 2 deixa de estar em andamento:
        // ela sai de novo, e a vaga dela não fica presa ao pedido velho.
        replan(root, 2);
        let again = round(root, "x", None);
        assert_eq!(waves_in(&again, "dispatch"), vec![2], "{again}");
        let running = again["running"].as_array().cloned().unwrap_or_default();
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let newest = log
            .visible()
            .into_iter()
            .filter(|e| e.event_type == "send" && e.wave() == Some(2))
            .map(|e| codes[&e.id].clone())
            .next_back()
            .unwrap();
        assert_eq!(running[1], json!({"wave": 2, "send": newest}), "o pedido novo é o que conta: {again}");

        // A onda 1 entregou antes de a rodada existir, e um pedido saiu para
        // ela depois, sem reprovação no meio: ela não está em andamento, e as
        // duas vagas ficam para as outras.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/a.rs"], &[]), (3, &["src/c.rs"], &[])]);
        write(root, "x", "delivered", json!({"wave": 1, "text": "Saiu antes da rodada.", "files": ["src/a.rs"]}));
        seed_send(root, 1);
        let free = round(root, "x", None);
        assert_eq!(waves_in(&free, "dispatch"), vec![2, 3], "{free}");
        assert_eq!(waves_in(&free, "running"), vec![2, 3], "a onda 1 não está em andamento: {free}");
    }

    /// O commit da rodada leva a remoção nos dois casos: o arquivo apagado só
    /// no disco e o que já saiu do índice. A mudança comum vai junto.
    #[test]
    fn the_round_commits_a_file_deleted_on_disk_and_one_already_removed_from_the_index() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs", "src/b.rs", "src/c.rs"], &[])]);
        round(root, "x", None);
        git_at(root, &["config", "user.email", "t@t"]);
        git_at(root, &["config", "user.name", "t"]);
        git_at(root, &["config", "commit.gpgsign", "false"]);

        std::fs::remove_file(root.join("src/a.rs")).unwrap();
        git_at(root, &["rm", "-q", "src/b.rs"]);
        std::fs::write(root.join("src/c.rs"), "fn um() {}\nfn tres() {}\n").unwrap();

        let files = ["src/a.rs", "src/b.rs", "src/c.rs"];
        let report = line("DELIVERED", json!({"wave": 1, "text": "Dois arquivos saíram.", "files": files,
            "commit": "tira dois arquivos"}));
        let out = round(root, "x", Some(&report));
        assert_eq!(out["ok"], json!(true), "{out}");

        let shown = Command::new("git")
            .args(["show", "--name-status", "--format=", "HEAD"])
            .current_dir(root)
            .output()
            .unwrap();
        let shown = String::from_utf8_lossy(&shown.stdout).to_string();
        let changes: Vec<&str> = shown.lines().filter(|line| !line.is_empty()).collect();
        assert_eq!(changes, ["D\tsrc/a.rs", "D\tsrc/b.rs", "M\tsrc/c.rs"], "{out}");
        let status = Command::new("git").args(["status", "--porcelain"]).current_dir(root).output().unwrap();
        let pending = String::from_utf8_lossy(&status.stdout).to_string();
        assert!(!pending.contains("src/"), "nada da onda ficou fora do commit: {pending}");
    }

    /// Cada relatório de `wrong` é recusado pelo git, com o motivo que o git
    /// deu, e não deixa nada gravado; depois de `fix`, a chamada que entrega
    /// `fixed` grava a entrega uma vez só.
    fn refused_by_git_records_nothing(root: &Path, wrong: &[String], fix: impl FnOnce(), fixed: &[&str]) {
        let spec_lines = || std::fs::read_to_string(store::spec_file(root, "x").unwrap()).unwrap().lines().count();
        let before = spec_lines();
        for report in wrong {
            let refused = round(root, "x", Some(report));
            assert_eq!(refused["reason"], json!("git-refused"), "{refused}");
            assert_eq!(spec_lines(), before, "nothing was recorded: {refused}");
            let bare = translate("round.git_refused", Locale::PtBr).replace("{detail}", "");
            assert_ne!(refused["hint"], json!(bare), "the refusal carries git's reason: {refused}");
        }
        fix();
        let went = round(root, "x", Some(&delivered(root, 1, "Saiu.", fixed)));
        assert_eq!(went["ok"], json!(true), "{went}");
        assert_eq!(delivered_count(root), 1, "the corrected call records the delivery once");
    }

    /// O caminho que existe fora do repositório, absoluto ou com `../`, é
    /// recusado pelo git sem gravar nada. A chamada corrigida leva ao commit
    /// um arquivo novo, que ainda não estava no git.
    #[test]
    fn a_path_outside_the_repository_is_refused_by_git_and_records_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let outside = tempdir().unwrap();
        std::fs::write(outside.path().join("fora.rs"), "fn fora() {}\n").unwrap();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);

        let name = outside.path().file_name().unwrap().to_string_lossy().to_string();
        let absolute = outside.path().join("fora.rs").to_string_lossy().to_string();
        let wrong: Vec<String> = [absolute, format!("../{name}/fora.rs")]
            .iter()
            .map(|path| delivered(root, 1, "Saiu.", &["src/a.rs", path.as_str()]))
            .collect();
        std::fs::write(root.join("src/novo.rs"), "fn novo() {}\n").unwrap();
        refused_by_git_records_nothing(root, &wrong, || {}, &["src/a.rs", "src/novo.rs"]);
        let shown = Command::new("git").args(["show", "--name-only", "--format=", "HEAD"]).current_dir(root).output();
        let shown = String::from_utf8_lossy(&shown.unwrap().stdout).to_string();
        assert!(shown.lines().any(|line| line == "src/novo.rs"), "the new file went into the commit: {shown}");
    }

    /// O caminho que o `.gitignore` ignora é recusado pelo git sem gravar
    /// nada, e a chamada sem ele grava a entrega uma vez só.
    #[test]
    fn an_ignored_path_is_refused_by_git_and_records_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        std::fs::write(root.join(".gitignore"), "src/gerado.rs\n").unwrap();
        std::fs::write(root.join("src/gerado.rs"), "fn gerado() {}\n").unwrap();

        let wrong = [delivered(root, 1, "Saiu.", &["src/a.rs", "src/gerado.rs"])];
        refused_by_git_records_nothing(root, &wrong, || {}, &["src/a.rs"]);
    }

    /// O gancho do commit que recusa não deixa nada gravado, e a chamada
    /// depois de o gancho sair grava a entrega uma vez só.
    #[cfg(unix)]
    #[test]
    fn a_commit_hook_that_refuses_records_nothing() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let hooks = root.join("ganchos");
        std::fs::create_dir_all(&hooks).unwrap();
        let hook = hooks.join("pre-commit");
        std::fs::write(&hook, "#!/bin/sh\necho 'o gancho recusou' >&2\nexit 1\n").unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        git_at(root, &["config", "core.hooksPath", &hooks.to_string_lossy()]);

        let wrong = [delivered(root, 1, "Saiu.", &["src/a.rs"])];
        refused_by_git_records_nothing(root, &wrong, || std::fs::remove_file(&hook).unwrap(), &["src/a.rs"]);
    }

    /// Com nada a comitar, o git recusa e dá o motivo na saída normal: a
    /// recusa traz esse motivo e não deixa nada gravado, e a chamada com o
    /// arquivo mudado grava a entrega uma vez só.
    #[test]
    fn nothing_to_commit_is_refused_with_gits_reason_and_records_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);

        let unchanged = [line("DELIVERED", json!({"wave": 1, "text": "Saiu.", "files": ["src/a.rs"],
            "commit": "a onda 1 saiu"}))];
        refused_by_git_records_nothing(root, &unchanged, || {}, &["src/a.rs"]);
    }

    /// Um pedido da onda `n` gravado sem passar pela rodada.
    fn seed_send(root: &Path, n: u64) {
        crate::shared::spec_state::seed_event(root, "x", "send", json!({"wave": n, "role": "wave",
            "text": "pedido", "lines": 1, "chars": 6, "items": [1], "mustard": "0", "author": "binary"}));
    }

    /// Cada onda tem no máximo duas rodadas de conserto. Depois da terceira
    /// reprovação seguida a rodada não a manda de novo, e a resposta traz a
    /// pergunta ao usuário com os três vereditos e as duas saídas. A parada
    /// segue até o plano da onda mudar; replanejada, ela volta à fila.
    #[test]
    fn a_wave_rejected_after_its_second_fix_round_is_not_sent_again_and_the_user_is_asked() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[1])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":1}"#).unwrap();
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1]);

        // Cada tentativa volta, e a revisão dela reprova.
        let rejected = |n: usize| {
            let out = round(root, "x", Some(&delivered(root, 1, &format!("Tentativa {n}."), &["src/a.rs"])));
            assert_eq!(out["ok"], json!(true), "{out}");
            round(root, "x", Some(&verdict(1, "rejected", &format!("reprovação {n}"))))
        };
        for n in 1..=2 {
            let fix = rejected(n);
            assert_eq!(waves_in(&fix, "dispatch"), vec![1], "rodada de conserto {n}: {fix}");
        }
        let sends = |root: &Path| {
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            log.visible().iter().filter(|e| e.event_type == "send").count()
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
        let question = translate("round.fix_limit.question", Locale::PtBr)
            .replace("{wave}", "1")
            .replace("{max}", &MAX_FIX_ROUNDS.to_string());
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

        // Parada continua parada, e a onda 2 também não sai.
        let still = round(root, "x", None);
        assert_eq!(waves_in(&still, "stopped"), vec![1], "{still}");
        assert_eq!(sends(root), 3);

        // O plano revisto devolve a onda à fila.
        replan(root, 1);
        let back = round(root, "x", None);
        assert_eq!(back["ok"], json!(true), "{back}");
        assert_eq!(waves_in(&back, "dispatch"), vec![1], "{back}");
        assert!(back.get("stopped").is_none(), "{back}");
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
    /// dela, direta ou por outra onda: a onda independente sai, a revisão
    /// pendente é pedida, e a resposta traz a pergunta com os vereditos da
    /// onda parada antes do resto. Tirada do plano, a onda parada deixa de
    /// contar: não segura mais nada nem é revisada; e a onda em andamento
    /// tirada do plano não ocupa vaga.
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
        assert_eq!(waves_in(&out, "reviews"), vec![4], "{out}");
        assert_eq!(waves_in(&out, "stopped"), vec![1], "{out}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let judged: Vec<&SpecEvent> = log.visible().into_iter().filter(|e| e.event_type == "verdict").collect();
        let verdicts: Vec<Value> =
            judged.iter().map(|e| json!({"code": codes[&e.id], "text": e.str_field("text").unwrap()})).collect();
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
        let targets: Vec<u64> = log
            .visible()
            .into_iter()
            .filter(|e| matches!(e.event_type.as_str(), "wave" | "task") && matches!(e.wave(), Some(1 | 5)))
            .map(|e| e.id)
            .collect();
        write(root, "x", "remove", json!({"targets": targets, "reason": "o usuário tirou as ondas do plano"}));
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert!(out.get("stopped").is_none(), "{out}");
        let mut sent = waves_in(&out, "dispatch");
        sent.sort_unstable();
        assert_eq!(sent, vec![3, 6], "{out}");
        assert_eq!(waves_in(&out, "reviews"), vec![4], "a onda fora do plano não é revisada: {out}");
        assert_eq!(waves_in(&out, "running"), vec![3, 6], "a onda fora do plano não ocupa vaga: {out}");
    }

    /// A rodada diz o próximo passo de cada situação: despachar o que saiu;
    /// esperar as ondas em andamento; dizer qual onda falta quando nada se
    /// move; e, com todas as ondas entregues e aprovadas, fechar — com a
    /// linha do fechamento pronta, que o binário aceita.
    #[test]
    fn the_round_answers_close_when_every_wave_is_approved_and_names_the_missing_one() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        let report_back = translate("round.report", Locale::PtBr);

        let first = round(root, "x", None);
        let next = first["next"].as_str().unwrap_or_default();
        assert!(next.ends_with(&format!("{} {report_back}", translate("round.next", Locale::PtBr))), "{first}");
        assert!(first.get("command").is_none(), "{first}");

        let waiting = round(root, "x", None);
        let next = waiting["next"].as_str().unwrap_or_default();
        let expected = translate("round.waiting", Locale::PtBr).replace("{waves}", "1, 2");
        assert!(next.ends_with(&format!("{expected} {report_back}")), "{waiting}");
        assert!(waiting.get("command").is_none(), "{waiting}");

        // As duas voltam; a 1 aprovada, a 2 reprovada. O conserto da 2 sai,
        // e o plano dela muda depois: nada se move, e a resposta diz que
        // falta a onda 2.
        let both = format!("{}\n{}", delivered(root, 1, "Saiu.", &["src/a.rs"]), delivered(root, 2, "Saiu.", &["src/b.rs"]));
        let back = round(root, "x", Some(&both));
        assert_eq!(waves_in(&back, "reviews"), vec![1, 2], "{back}");
        let judged = format!("{}\n{}", verdict(1, "approved", "passou"), verdict(2, "rejected", "faltou"));
        let fix = round(root, "x", Some(&judged));
        assert_eq!(waves_in(&fix, "dispatch"), vec![2], "{fix}");
        replan(root, 2);
        let held = round(root, "x", None);
        assert_eq!(waves_in(&held, "dispatch"), Vec::<u64>::new(), "{held}");
        assert_eq!(held["reviews"], json!([]), "{held}");
        assert_eq!(held["running"], json!([]), "{held}");
        let missing = translate("round.missing", Locale::PtBr).replace("{wave}", "2");
        assert!(held["next"].as_str().unwrap_or_default().ends_with(&missing), "{held}");
        assert!(held.get("command").is_none(), "{held}");

        // O conserto volta aprovado: tudo entregue e aprovado, a rodada manda
        // fechar.
        round(root, "x", Some(&delivered(root, 2, "Consertou.", &["src/b.rs"])));
        let done = round(root, "x", Some(&verdict(2, "approved", "passou")));
        assert_eq!(done["ok"], json!(true), "{done}");
        let command = done["command"].as_str().unwrap_or_default();
        assert_eq!(command, "mustard-rt run close --spec x", "{done}");
        crate::commands::flow::resume::assert_parses(command);
        let close = translate("round.close", Locale::PtBr).replace("{command}", command);
        assert!(done["next"].as_str().unwrap_or_default().ends_with(&close), "{done}");
    }

    /// A onda que depende de todas as outras só sai depois das aprovações
    /// delas, não só das entregas. A que depende de uma parte sai com a
    /// entrega, como antes.
    #[test]
    fn the_wave_that_depends_on_all_the_others_waits_for_their_approvals() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[]), (3, &["src/c.rs"], &[1, 2])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":3}"#).unwrap();
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 2]);

        let both = format!("{}\n{}", delivered(root, 1, "Saiu.", &["src/a.rs"]), delivered(root, 2, "Saiu.", &["src/b.rs"]));
        let out = round(root, "x", Some(&both));
        assert_eq!(waves_in(&out, "reviews"), vec![1, 2], "{out}");
        assert_eq!(waves_in(&out, "dispatch"), Vec::<u64>::new(), "entregues não bastam: {out}");

        let one = round(root, "x", Some(&verdict(1, "approved", "passou")));
        assert_eq!(one["ok"], json!(true), "{one}");
        assert_eq!(waves_in(&one, "dispatch"), Vec::<u64>::new(), "a onda 2 ainda espera revisão: {one}");
        let both = round(root, "x", Some(&verdict(2, "approved", "passou")));
        assert_eq!(waves_in(&both, "dispatch"), vec![3], "as duas aprovadas soltam a 3: {both}");

        // A onda 2 depende só da 1, e a 3 não passa por ela: a 2 sai com a
        // entrega da 1.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[1]), (3, &["src/c.rs"], &[])]);
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 3]);
        let out = round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        assert_eq!(waves_in(&out, "dispatch"), vec![2], "a entrega basta para quem não depende de todas: {out}");
    }

    /// Num projeto sem formatador configurado nada é formatado e nada é
    /// avisado; num projeto com Prettier configurado e sem Prettier no disco,
    /// o aviso sai com o nome do formatador, em vez de a formatação ser
    /// pulada em silêncio. Só os arquivos da rodada entram.
    #[test]
    fn the_formatter_of_the_project_runs_only_on_the_round_files_and_says_when_it_is_missing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        for name in ["a.ts", "fora.ts"] {
            std::fs::write(root.join("src").join(name), "const x=1\n").unwrap();
        }
        let files = vec!["src/a.ts".to_string()];

        let never = |_: &str, _: &[&str]| false;
        let always = |program: &str, args: &[&str]| {
            assert_eq!(program, "npx");
            assert!(args.contains(&"src/a.ts"), "{args:?}");
            assert!(!args.contains(&"src/fora.ts"), "só os arquivos da rodada: {args:?}");
            true
        };

        assert_eq!(format_with(root, &files, &never), Formatting::default(), "sem formatador, nada");

        std::fs::write(root.join(".prettierrc"), b"{}").unwrap();
        let out = format_with(root, &files, &always);
        assert_eq!(out.formatted, vec!["src/a.ts".to_string()]);
        assert!(out.missing.is_empty(), "{out:?}");

        let out = format_with(root, &files, &never);
        assert!(out.formatted.is_empty(), "{out:?}");
        assert_eq!(out.missing, vec!["Prettier".to_string()], "o formatador some pelo nome");
        assert_eq!(
            std::fs::read_to_string(root.join("src/fora.ts")).unwrap(),
            "const x=1\n",
            "o arquivo fora da rodada fica byte a byte"
        );
    }

    /// O ramo do projeto .NET: o formatador roda uma vez por arquivo da
    /// rodada, sempre com o projeto da raiz, nunca num arquivo de fora, e some
    /// pelo nome quando não está na máquina.
    #[test]
    fn the_dotnet_formatter_runs_once_per_round_file_and_says_when_it_is_missing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        for name in ["a.cs", "b.cs", "fora.cs"] {
            std::fs::write(root.join("src").join(name), "class A {}\n").unwrap();
        }
        let files = vec!["src/a.cs".to_string(), "src/b.cs".to_string()];

        let never = |_: &str, _: &[&str]| false;
        assert_eq!(format_with(root, &files, &never), Formatting::default(), "sem projeto .NET, nada");

        std::fs::write(root.join("Loja.csproj"), b"<Project />").unwrap();
        let calls: std::cell::RefCell<Vec<Vec<String>>> = std::cell::RefCell::new(Vec::new());
        let always = |program: &str, args: &[&str]| {
            assert_eq!(program, "dotnet");
            calls.borrow_mut().push(args.iter().map(|a| (*a).to_string()).collect());
            true
        };
        let out = format_with(root, &files, &always);
        assert_eq!(out.formatted, files);
        assert!(out.missing.is_empty(), "{out:?}");
        let calls = calls.into_inner();
        assert_eq!(calls.len(), 2, "uma chamada por arquivo da rodada: {calls:?}");
        assert!(calls.iter().all(|c| c.contains(&"Loja.csproj".to_string())), "{calls:?}");
        assert!(!calls.iter().any(|c| c.contains(&"src/fora.cs".to_string())), "{calls:?}");

        let out = format_with(root, &files, &never);
        assert!(out.formatted.is_empty(), "{out:?}");
        assert_eq!(out.missing, vec!["dotnet format".to_string()], "o formatador some pelo nome");
        assert_eq!(
            std::fs::read_to_string(root.join("src/fora.cs")).unwrap(),
            "class A {}\n",
            "o arquivo fora da rodada fica byte a byte"
        );
    }
}
