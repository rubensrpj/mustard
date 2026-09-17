//! `mustard-rt run round [--spec <nome>]` — uma rodada de ondas.
//!
//! É a porta única da execução, e cada rodada é uma chamada só. Sem relatório,
//! a rodada despacha: escolhe as ondas que podem sair juntas, monta o pedido
//! de cada uma, grava o envio com o pedido exato como foi injetado e marca a
//! spec como em execução na primeira rodada. Com o relatório da rodada
//! anterior (`--report`), ela primeiro fecha o que voltou — grava o que cada
//! onda entregou e o veredito da revisão, formata só os arquivos da rodada,
//! faz o commit da rodada — e só então despacha a rodada seguinte.
//!
//! **O que trava.** Uma spec que ainda não foi aprovada; um `entregou` acima
//! do teto de caracteres; uma mensagem de commit fora do modelo (título e
//! corpo acima do teto, link do claude.ai, o nome do modelo, assinatura de
//! coautoria ou e-mail de alguém); e o relatório em que um agente diz que o
//! plano da onda não funciona, que para a rodada e só segue com o "sim" do
//! usuário. O "sim" é o clique em "Aceitar" na pergunta da mudança, gravado
//! pela testemunha como na aprovação da spec, e nunca a leitura que o modelo
//! faz de uma frase: a rodada não aceita código nenhum de quem a chama.
//!
//! **O que avisa.** O formatador que o projeto declara e que não foi achado
//! sai pelo nome, em vez de a formatação ser pulada em silêncio.
//!
//! A página da spec e a do projeto são refeitas no fim da rodada, e a resposta
//! manda publicá-las: a rodada é um dos marcos de publicação. Nenhum endereço
//! é impresso na conversa.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use mustard_core::platform::git as git_exec;

use mustard_core::domain::spec_events::{
    check_message, Block, BlockQuery, MessageRefusal, Refusal, SpecEvent, SpecLog,
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

/// Por que a rodada não correu.
enum RoundRefusal {
    /// Uma recusa do arquivo de eventos.
    Refused(Refusal),
    /// O relatório não é um objeto JSON com a lista de ondas.
    BadReport { detail: String },
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
    fn reason(&self) -> String {
        match self {
            Self::Refused(refusal) => refusal.reason().to_string(),
            Self::BadReport { .. } => "round-bad-report".into(),
            Self::NotApproved { .. } => "round-not-approved".into(),
            Self::DeliveredTooLong { .. } => "delivered-too-long".into(),
            Self::CommitTooLong { .. } => "commit-too-long".into(),
            Self::CommitForbidden { .. } => "commit-forbidden-text".into(),
            Self::Replan { .. } => "wave-plan-does-not-work".into(),
            Self::Git { .. } => "git-refused".into(),
        }
    }

    fn message(&self, lang: Locale) -> String {
        let fill = |key: &str, slots: &[(&str, String)]| {
            slots.iter().fold(translate(key, lang).to_string(), |text, (slot, value)| text.replace(slot, value))
        };
        match self {
            Self::Refused(refusal) => refusal.message(lang),
            Self::BadReport { detail } => fill("round.bad_report", &[("{detail}", detail.clone())]),
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

    fn to_value(&self, lang: Locale) -> Value {
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

/// O que uma onda devolveu à rodada. O fechamento lê o relatório da última
/// rodada pela mesma porta.
pub(crate) struct WaveReport {
    pub wave: u64,
    pub delivered: String,
    pub files: Vec<String>,
    pub verdict: Option<Value>,
    pub replan: Option<String>,
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

    let mut recorded: Vec<Value> = Vec::new();
    let mut formatted: Vec<String> = Vec::new();
    let mut warnings: Vec<Value> = Vec::new();
    let mut commit: Option<Value> = None;

    if let Some(raw) = opts.report.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
        let reports = parse_report(raw).map_err(|detail| RoundRefusal::BadReport { detail })?;
        // O agente que diz que o plano da onda não funciona para a rodada: a
        // mudança proposta é mostrada, e só o clique do usuário em "Aceitar",
        // gravado pela testemunha, a deixa seguir.
        for report in &reports {
            if let Some(change) = &report.replan {
                let code = replan_code(report.wave, change);
                if !change_accepted(&log, report.wave, &code) {
                    return Err(RoundRefusal::Replan { wave: report.wave, change: change.clone(), code });
                }
            }
        }
        for report in &reports {
            let chars = report.delivered.chars().count();
            if chars > DELIVERED_MAX_CHARS {
                return Err(RoundRefusal::DeliveredTooLong { wave: report.wave, chars });
            }
        }
        // A mensagem do commit é conferida junto das outras travas, antes de
        // qualquer gravação: recusá-la depois de gravar o entregou e o
        // veredito faria a chamada seguinte, com a mensagem corrigida,
        // duplicar os dois.
        let message = commit_message(raw)?;

        // O que voltou vira registro: o entregou de cada onda e o veredito da
        // revisão dela, pela mesma porta de gravação das outras.
        recorded = record_reports(&opts.root, &spec, &reports).map_err(RoundRefusal::Refused)?;

        // A formatação roda uma vez por rodada, só nos arquivos da rodada.
        let files: Vec<String> = reports.iter().flat_map(|r| r.files.clone()).collect();
        let outcome = format_round_files(root, &files);
        formatted = outcome.formatted;
        for name in outcome.missing {
            warnings.push(json!({
                "reason": "formatter-not-found",
                "hint": translate("round.formatter_missing", lang).replace("{name}", &name),
            }));
        }

        if let Some(message) = message {
            let waves: Vec<u64> = reports.iter().map(|r| r.wave).collect();
            commit = Some(make_commit(&opts.root, root, &spec, &message, &waves, &files)?);
        }
    }

    // A primeira rodada leva a spec para a execução.
    let entering = phase == "approved";
    if entering {
        crate::commands::spec_events::write::record_phase(&opts.root, &spec, "running", session);
    }

    // O despacho da rodada seguinte: as ondas prontas, no máximo o que o
    // projeto deixa compilar ao mesmo tempo, nunca duas que dividem arquivo.
    let log = store::read(&path)
        .map_err(RoundRefusal::Refused)?
        .ok_or_else(|| RoundRefusal::Refused(Refusal::NoSpecFile { spec: spec.clone() }))?;
    let next = next_waves(&log, max_parallel(root));
    let built = prompts(root, &spec, &log, lang);
    let mut dispatched: Vec<Value> = Vec::new();
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
    }

    // A página sai no fim do passo, uma vez, e a rodada manda publicá-la.
    let pages = crate::commands::spec_events::pages::refresh(root, &spec, lang).ok();

    // Com o pull request aberto, o corpo dele é refeito aqui: ele é montado do
    // mesmo arquivo de eventos que acabou de mudar, e um corpo que descreve a
    // rodada anterior é pior do que nenhum — foi por isso que existiu um portão
    // só para reparar que ele tinha envelhecido.
    let rewritten = rewrite_open_pr(root, &spec);

    let mut out = json!({
        "ok": true,
        "spec": spec,
        "recorded": recorded,
        "formatted": formatted,
        "dispatch": dispatched,
        "reviews": reviews_due(&log, &built),
        "publish": ["spec", "project"],
        "next": translate("round.next", lang),
    });
    if entering {
        out["phase"] = json!("running");
    }
    if let Some(commit) = commit {
        out["commit"] = commit;
    }
    if let Some(pages) = pages {
        out["md"] = json!(pages.md);
        out["html"] = json!(pages.html);
    }
    if !warnings.is_empty() {
        out["warnings"] = json!(warnings);
    }
    if let Some(number) = rewritten {
        out["pr"] = json!({ "number": number, "body": "rewritten" });
    }
    Ok(out)
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

/// O relatório da rodada anterior: uma entrada por onda, com o que ela
/// entregou, os arquivos que mexeu, o veredito da revisão e, quando é o caso,
/// a mudança de plano que o agente propõe.
pub(crate) fn parse_report(raw: &str) -> Result<Vec<WaveReport>, String> {
    let parsed: Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    let waves = parsed.get("waves").and_then(Value::as_array).ok_or_else(|| "waves".to_string())?;
    let mut out = Vec::new();
    for entry in waves {
        let wave = entry.get("wave").and_then(Value::as_u64).ok_or_else(|| "wave".to_string())?;
        out.push(WaveReport {
            wave,
            delivered: entry.get("delivered").and_then(Value::as_str).unwrap_or_default().to_string(),
            files: entry
                .get("files")
                .and_then(Value::as_array)
                .map(|list| list.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default(),
            verdict: entry.get("verdict").filter(|v| v.is_object()).cloned(),
            replan: entry
                .get("replan")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .map(str::to_string),
        });
    }
    Ok(out)
}

/// Grava o que voltou de cada onda: o entregou dela e, quando a revisão veio
/// junto, o veredito. É a mesma porta de gravação das outras, e o fechamento a
/// usa para o relatório da última rodada.
pub(crate) fn record_reports(
    start: &Path,
    spec: &str,
    reports: &[WaveReport],
) -> Result<Vec<Value>, Refusal> {
    let mut recorded = Vec::new();
    for report in reports {
        let mut draft = Map::new();
        draft.insert("wave".into(), json!(report.wave));
        draft.insert("text".into(), json!(report.delivered));
        draft.insert("files".into(), json!(report.files));
        draft.insert("author".into(), json!("wave"));
        let written = record(start, spec, "delivered", draft, PhaseWriter::Binary)?;
        recorded.push(json!({ "wave": report.wave, "type": "delivered", "id": written.written.id }));

        if let Some(verdict) = &report.verdict {
            let mut draft = verdict.as_object().cloned().unwrap_or_default();
            draft.insert("wave".into(), json!(report.wave));
            draft.insert("author".into(), json!("review"));
            let written = record(start, spec, "verdict", draft, PhaseWriter::Binary)?;
            recorded.push(json!({ "wave": report.wave, "type": "verdict", "id": written.written.id }));
        }
    }
    Ok(recorded)
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
/// Só conta a mensagem de autor `user` com a testemunha, que o `run write`
/// não grava: a fala do usuário chega pelos ganchos.
fn change_accepted(log: &SpecLog, wave: u64, code: &str) -> bool {
    let langs = [Locale::PtBr, Locale::EnUs];
    let questions: Vec<String> = langs.iter().map(|lang| change_question(code, *lang)).collect();
    let sent = last_sends(log).get(&wave).copied().unwrap_or(0);
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

/// A mensagem de commit do relatório, já conferida. `None` quando o relatório
/// não pede commit nenhum.
fn commit_message(raw: &str) -> Result<Option<(String, String)>, RoundRefusal> {
    let parsed: Value =
        serde_json::from_str(raw).map_err(|e| RoundRefusal::BadReport { detail: e.to_string() })?;
    let Some(commit) = parsed.get("commit").filter(|c| c.is_object()) else {
        return Ok(None);
    };
    let title = commit.get("title").and_then(Value::as_str).unwrap_or_default().trim().to_string();
    let body = commit.get("body").and_then(Value::as_str).unwrap_or_default().trim().to_string();
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

/// Faz o commit da rodada com a mensagem já conferida e grava o commit no
/// arquivo de eventos. Nada é gravado quando o git recusa.
fn make_commit(
    start: &Path,
    root: &Path,
    spec: &str,
    message: &(String, String),
    waves: &[u64],
    files: &[String],
) -> Result<Value, RoundRefusal> {
    let (title, body) = message;
    let mut add: Vec<&str> = vec!["add", "--"];
    add.extend(files.iter().map(String::as_str));
    if !files.is_empty() {
        git(root, &add).map_err(|detail| RoundRefusal::Git { detail })?;
    }
    let mut args: Vec<&str> = vec!["commit", "-m", title];
    if !body.is_empty() {
        args.push("-m");
        args.push(body);
    }
    git(root, &args).map_err(|detail| RoundRefusal::Git { detail })?;
    let sha = git(root, &["rev-parse", "HEAD"]).map_err(|detail| RoundRefusal::Git { detail })?;
    let sha = sha.trim().to_string();

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
    Err(out.stderr.trim().to_string())
}

// ---------------------------------------------------------------------------
// A escolha das ondas
// ---------------------------------------------------------------------------

/// Quantas ondas o projeto deixa compilar ao mesmo tempo.
fn max_parallel(root: &Path) -> usize {
    mustard_core::ProjectConfig::load(root).max_compiling_waves().unwrap_or(DEFAULT_PARALLEL)
}

/// As ondas que saem nesta rodada: as que ainda não saíram nem entregaram,
/// cujas dependências já foram entregues, no máximo `limit`, e nunca duas que
/// declaram o mesmo arquivo — duas ondas assim seriam dois agentes editando o
/// mesmo arquivo ao mesmo tempo.
fn next_waves(log: &SpecLog, limit: usize) -> Vec<u64> {
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
    let mut out: Vec<u64> = Vec::new();
    let mut taken: BTreeSet<String> = BTreeSet::new();
    for (n, files) in ready_in_order(&graph, &depends, &already_out, &delivered, log) {
        if out.len() >= limit {
            break;
        }
        if files.iter().any(|f| taken.contains(f)) {
            continue;
        }
        taken.extend(files);
        out.push(n);
    }
    out
}

/// As ondas que voltam para a fila: a última revisão delas reprovou, e o
/// conserto ainda não saiu — o pedido mais novo da onda é anterior a essa
/// reprovação. Depois que o conserto sai, a onda espera a revisão dele, e não
/// é despachada de novo pela mesma reprovação.
fn waves_to_redo(log: &SpecLog) -> BTreeSet<u64> {
    let mut rejected: BTreeMap<u64, u64> = BTreeMap::new();
    for verdict in log.block(BlockQuery::Block(Block::Review)).into_iter().filter(|e| e.event_type == "verdict") {
        let (Some(n), Some(result)) = (verdict.wave(), verdict.str_field("result")) else { continue };
        if result == "rejected" {
            rejected.insert(n, verdict.id);
        } else {
            rejected.remove(&n);
        }
    }
    let last_send = last_sends(log);
    rejected
        .into_iter()
        .filter(|(n, id)| last_send.get(n).is_none_or(|sent| sent < id))
        .map(|(n, _)| n)
        .collect()
}

/// O número do último pedido de cada onda.
fn last_sends(log: &SpecLog) -> BTreeMap<u64, u64> {
    let mut last_send: BTreeMap<u64, u64> = BTreeMap::new();
    for send in log.block(BlockQuery::Block(Block::Waves)).into_iter().filter(|e| e.event_type == "send") {
        if let Some(n) = send.wave() {
            last_send.insert(n, send.id);
        }
    }
    last_send
}

/// As ondas replanejadas depois do último pedido: a onda, ou uma tarefa dela,
/// ganhou versão nova depois do envio. A tarefa que muda de onda replaneja as
/// duas — a de onde saiu, pela versão que ela substitui, e a para onde foi.
fn waves_replanned(log: &SpecLog) -> BTreeSet<u64> {
    let last_send = last_sends(log);
    let by_id: BTreeMap<u64, &SpecEvent> = log.events.iter().map(|e| (e.id, e)).collect();
    let mut replanned = BTreeSet::new();
    for event in log.block(BlockQuery::Block(Block::Waves)) {
        if !matches!(event.event_type.as_str(), "wave" | "task") {
            continue;
        }
        let before = event.int("replaces").and_then(|old| by_id.get(&old)).and_then(|old| old.wave());
        for n in event.wave().into_iter().chain(before) {
            if last_send.get(&n).is_some_and(|sent| *sent < event.id) {
                replanned.insert(n);
            }
        }
    }
    replanned
}

/// As ondas prontas para sair, em ordem de nível e de número, cada uma com os
/// arquivos que as tarefas dela declaram. `already_out` são as ondas que já
/// saíram da fila e `delivered` as que já entregaram, que é o que solta as
/// ondas dependentes delas.
fn ready_in_order(
    graph: &crate::commands::wave::wave_overlap_check::WaveGraph,
    depends: &BTreeMap<u64, Vec<u64>>,
    already_out: &BTreeSet<u64>,
    delivered: &BTreeSet<u64>,
    log: &SpecLog,
) -> Vec<(u64, BTreeSet<String>)> {
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
    let mut ready: Vec<(u32, u64)> = depends
        .iter()
        .filter(|(n, _)| !already_out.contains(n))
        .filter(|(_, on)| on.iter().all(|d| delivered.contains(d)))
        .map(|(n, _)| (graph.level.get(n).copied().unwrap_or(0), *n))
        .collect();
    ready.sort_unstable();
    ready.into_iter().map(|(_, n)| (n, files.get(&n).cloned().unwrap_or_default())).collect()
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
fn waves_awaiting_review(log: &SpecLog) -> Vec<u64> {
    let mut last_verdict: BTreeMap<u64, u64> = BTreeMap::new();
    for verdict in log.block(BlockQuery::Block(Block::Review)).into_iter().filter(|e| e.event_type == "verdict") {
        if let Some(n) = verdict.wave() {
            last_verdict.insert(n, verdict.id);
        }
    }
    let mut last_delivered: BTreeMap<u64, u64> = BTreeMap::new();
    let mut last_send: BTreeMap<u64, u64> = BTreeMap::new();
    for event in log.block(BlockQuery::Block(Block::Waves)) {
        let Some(n) = event.wave() else { continue };
        match event.event_type.as_str() {
            "delivered" => {
                last_delivered.insert(n, event.id);
            }
            "send" => {
                last_send.insert(n, event.id);
            }
            _ => {}
        }
    }
    last_delivered
        .into_iter()
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

        let report = json!({"waves": [{"wave": 1, "delivered": "A soma saiu.", "files": ["src/a.rs"]}]});
        let out = round(root, "x", Some(&report.to_string()));
        assert_eq!(out["ok"], json!(true), "{out}");
        let reviews = out["reviews"].as_array().cloned().unwrap_or_default();
        assert_eq!(reviews.len(), 1, "a onda entregue sem veredito pede revisão: {out}");
        assert_eq!(reviews[0]["wave"], json!(1), "{out}");

        let verdict = json!({"waves": [{"wave": 1, "delivered": "De novo.", "files": ["src/a.rs"],
            "verdict": {"result": "approved", "text": "passou",
                        "criteria": [{"criterion": 1, "tests_rule": "confere a regra"}]}}]});
        let out = round(root, "x", Some(&verdict.to_string()));
        assert_eq!(out["reviews"], json!([]), "o veredito é mais novo que a entrega dele: {out}");
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        assert_eq!(log.visible().iter().filter(|e| e.event_type == "delivered").count(), 2);
        assert_eq!(log.visible().iter().filter(|e| e.event_type == "verdict").count(), 1);
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

        let report = json!({"waves": [{"wave": 1, "delivered": "A soma saiu.", "files": ["src/a.rs"],
            "verdict": {"result": "rejected", "text": "faltou o teste",
                        "criteria": [{"criterion": 1, "tests_rule": "confere a regra"}]}}]});
        let again = round(root, "x", Some(&report.to_string()));
        assert_eq!(again["dispatch"].as_array().map(Vec::len), Some(1), "a onda reprovada volta a sair: {again}");

        let quiet = round(root, "x", None);
        assert_eq!(quiet["dispatch"], json!([]), "o conserto já saiu, e a onda espera a revisão dele: {quiet}");
        assert_eq!(quiet["reviews"], json!([]), "o conserto ainda não voltou: nada a revisar: {quiet}");

        // O conserto entregue é mais novo que a reprovação, e por isso pede
        // revisão: é a revisão nova que tira o veredito velho da frente.
        let fixed = json!({"waves": [{"wave": 1, "delivered": "O teste entrou.", "files": ["src/a.rs"]}]});
        let back = round(root, "x", Some(&fixed.to_string()));
        let waves: Vec<u64> = back["reviews"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|review| review["wave"].as_u64())
            .collect();
        assert_eq!(waves, vec![1], "o conserto entregue volta para a revisão: {back}");
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
        let report = json!({"waves": [{"wave": 1, "delivered": "Saiu.", "files": ["src/a.rs"]}]});
        round(root, "x", Some(&report.to_string()));
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
        let report = json!({"waves": [{"wave": 1, "delivered": long, "files": ["src/a.rs"]}]});
        let refused = round(root, "x", Some(&report.to_string()));
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
        let report = json!({"waves": [{"wave": 1, "delivered": "Parei.", "files": ["src/a.rs"],
            "replan": change}]})
        .to_string();
        let stopped = round(root, "x", Some(&report));
        assert_eq!(stopped["reason"], json!("wave-plan-does-not-work"), "{stopped}");
        let question = stopped["question"].as_str().unwrap_or_default().to_string();
        assert_eq!(question, change_question(&replan_code(1, change), Locale::PtBr), "{stopped}");
        assert_eq!(stopped["options"], json!(["Aceitar", "Recusar"]), "{stopped}");
        let hint = stopped["hint"].as_str().unwrap_or_default();
        assert!(hint.contains(change) && hint.contains(&question), "{hint}");
        assert_eq!(delivered_count(root), 0);

        // O modelo não escreve o "sim": nem como mensagem do usuário, que o
        // `run write` recusa, nem como mensagem sua com a testemunha.
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
        assert_eq!(own["ok"], json!(true), "{own}");
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
        let delivered = |body: &str| {
            json!({"waves": [{"wave": 1, "delivered": "A soma saiu.", "files": ["src/a.rs"]}],
                   "commit": {"title": "feat: a soma", "body": body}})
        };
        let refused = round(root, "x", Some(&delivered("pedido de fulano@empresa.com.br").to_string()));
        assert_eq!(refused["reason"], json!("commit-forbidden-text"), "{refused}");
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        assert_eq!(log.visible().iter().filter(|e| e.event_type == "delivered").count(), 0, "nada foi gravado");

        let went = round(root, "x", Some(&delivered("O corpo limpo.").to_string()));
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

        let report = json!({"waves": [{"wave": 1, "delivered": "A soma saiu.", "files": ["src/a.rs"]}],
            "commit": {"title": "feat(onda-1): a soma sai", "body": "A onda 1 escreveu a soma."}});
        let out = round(root, "x", Some(&report.to_string()));
        assert_eq!(out["ok"], json!(true), "{out}");
        let sha = out["commit"]["sha"].as_str().unwrap_or_default().to_string();
        assert_eq!(sha.len(), 40, "{out}");
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let commit = log.visible().into_iter().find(|e| e.event_type == "commit").expect("commit");
        assert_eq!(commit.str_field("sha"), Some(sha.as_str()));
        assert_eq!(commit.ints("waves"), vec![1]);
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
