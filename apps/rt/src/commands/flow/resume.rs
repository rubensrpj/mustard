//! `mustard-rt run resume [--spec <nome>]` — a retomada de uma spec.
//!
//! Lê só o estado, que é o que a retomada precisa: a fase em que a spec está,
//! a branch e a base dela. Dali sai o próximo passo, em palavras e como
//! comando, e é essa resposta que conduz a conversa de volta ao ponto em que
//! ela parou. Nada mais do arquivo de eventos é lido, e nenhum endereço de
//! página entra na resposta: o link mora na barra de status.
//!
//! É também o que o `/mustard:continue` chama — o botão de reserva, porque a
//! retomada já acontece sozinha no início da sessão: a linha de retomada
//! ([`resume_line`]) — a spec, a fase, o último passo e o próximo item — sai
//! igual nos dois lugares.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::{Block, BlockQuery, Refusal, SpecLog};
use mustard_core::domain::spec_state::{SpecState, State};
use mustard_core::domain::survey;
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Value};

use crate::commands::spec_events::{self, read::checkout};
use crate::shared::spec_state::{session_from_env, DiskSpecState};

/// As opções de `mustard-rt run resume`.
pub struct ResumeOpts {
    /// Qualquer pasta dentro do repositório.
    pub root: PathBuf,
    /// A spec retomada; sem ela, a spec atual.
    pub spec: Option<String>,
}

/// O núcleo testável de [`run_cmd`]. A sessão vem do ambiente. Nunca entra em
/// pânico.
pub(crate) fn resume_at(opts: &ResumeOpts) -> Value {
    resume_for(opts, session_from_env().as_deref())
}

/// [`resume_at`] com a sessão recebida, que é como um teste a escolhe.
pub(crate) fn resume_for(opts: &ResumeOpts, session: Option<&str>) -> Value {
    let project = spec_events::project(&opts.root);
    let lang = project.lang;
    let refuse = |refusal: &Refusal| spec_events::refused(refusal, lang);

    let spec = match opts.spec.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(spec) => spec.to_string(),
        None => match DiskSpecState::new(&checkout(&opts.root)).active(session) {
            Some(spec) => spec,
            None => return refuse(&Refusal::NoCurrentSpec),
        },
    };
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
    let mut out = json!({
        "ok": true,
        "spec": spec,
        "phase": phase,
        "line": resume_line(&spec, &log, lang),
        "next": translate(next_key(phase), lang),
        "command": next_command(phase, &spec, &state),
    });
    if let Some(branch) = state.branch {
        out["branch"] = json!(branch);
    }
    if let Some(base) = state.base {
        out["base"] = json!(base);
    }
    out
}

/// A linha de retomada da spec `spec`: a spec, a fase, o último passo do
/// fluxo e o próximo item. É o que o início da sessão coloca depois de
/// `/clear`, e o que o `resume` devolve em `line`.
pub(crate) fn resume_line(spec: &str, log: &SpecLog, lang: Locale) -> String {
    let phase = State::from_log(log).phase.unwrap_or("survey");
    let none = translate("resume.none", lang);
    translate("resume.line", lang)
        .replace("{spec}", spec)
        .replace("{phase}", phase)
        .replace("{last}", &last_step(log).unwrap_or_else(|| none.to_string()))
        .replace("{next}", &next_item(spec, phase, log, lang).unwrap_or_else(|| none.to_string()))
}

/// A linha de retomada da spec atual da sessão `session`, vista de `root`.
/// `None` sem spec atual, sem arquivo de eventos e na spec que já terminou,
/// que não tem para onde voltar.
pub(crate) fn current_line(root: &Path, session: Option<&str>) -> Option<String> {
    let project = spec_events::project(root);
    let spec = DiskSpecState::new(&checkout(root)).active(session)?;
    let log = store::read(&store::spec_file(&project.root, &spec).ok()?).ok()??;
    let phase = State::from_log(&log).phase;
    if matches!(phase, Some("delivered" | "discarded")) {
        return None;
    }
    Some(resume_line(&spec, &log, project.lang))
}

/// O último passo do fluxo: o comando da chamada mais nova que deu certo.
fn last_step(log: &SpecLog) -> Option<String> {
    log.block(BlockQuery::Block(Block::Conversation))
        .into_iter()
        .filter(|e| e.event_type == "call" && e.str_field("result") == Some("ok"))
        .filter_map(|e| e.str_field("command"))
        .map(str::trim)
        .rfind(|command| !command.is_empty())
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
            let planned: BTreeSet<u64> = log
                .block(BlockQuery::Block(Block::Waves))
                .into_iter()
                .filter(|e| e.event_type == "wave")
                .filter_map(|e| e.wave())
                .collect();
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
        .find(|(fase, _)| *fase == phase)
        .and_then(|(_, nome)| step_command(nome, spec, state))
        .map_or(Value::Null, Value::from)
}

/// A linha inteira do passo `name` da spec `spec`, com as opções que o passo
/// exige tiradas do estado: o pull request sai da branch da spec para a base
/// dela. `None` quando o estado não traz o que o passo exige — uma linha sem
/// uma opção obrigatória é recusada pelo binário antes de fazer qualquer coisa.
pub fn step_command(name: &str, spec: &str, state: &State) -> Option<String> {
    match name {
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
    use mustard_core::platform::i18n::Locale;
    use crate::commands::spec_events::write::record_open;
    use std::path::Path;
    use tempfile::tempdir;

    fn resume(root: &Path, spec: &str) -> Value {
        resume_for(&ResumeOpts { root: root.to_path_buf(), spec: Some(spec.to_string()) }, None)
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

        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join("x"));
        let out = resume(root, "x");
        assert_eq!(out["phase"], json!("approved"), "{out}");
        assert_eq!(out["command"], json!("mustard-rt run round --spec x"), "{out}");
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
        let crit = seed("criterion", json!({"when": "a", "then": "b", "proof": "p", "origin": said}));
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
        let crit = seed("criterion", json!({"when": "a", "then": "b", "proof": "p", "origin": said}));
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
        assert_eq!(resume_line("x", &log, Locale::PtBr), line);
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

    /// Sem spec nenhuma, a retomada recusa dizendo que não há spec atual.
    #[test]
    fn resuming_without_a_current_spec_is_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        let refused = resume(root, "nao-existe");
        assert_eq!(refused["ok"], json!(false), "{refused}");
        assert_eq!(refused["reason"], json!("no-spec-file"), "{refused}");
    }
}
