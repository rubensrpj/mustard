//! `subagent_inject` — o pedido da onda, montado pelo número dela.
//!
//! No despacho de um agente (`PreToolUse` de `Task` ou `Agent`), o texto da
//! tarefa pode ser só um bilhete: uma linha [`TICKET`] com a spec e o número
//! da onda, como `MUSTARD-WAVE: mustard-enxuto 24`. O gancho troca o texto
//! inteiro pelo pedido que o binário monta para aquela onda — a lista dos
//! itens, com as lições e as skills —, pela mesma montagem da rodada e da
//! página, e o agente recebe o pedido já no corpo, sem ler arquivo nenhum.
//!
//! Antes de trocar, o gancho confere duas coisas: a spec está aprovada, e a
//! onda do bilhete existe. Quando uma delas falha, ou quando o bilhete não se
//! lê, o despacho é barrado, e o motivo diz o que falta. O gancho
//! nunca manda o agente ler um arquivo no lugar do pedido.
//!
//! Quem instrui a onda é o Mustard, e não o condutor. O despacho a um agente
//! do Mustard que abre com o título do pedido de uma onda sai só com o que a
//! rodada monta para ela: o título e o comando que lê o pedido. O texto a
//! mais que o condutor escreveu sai, e o recado ao condutor diz isso; o
//! despacho que já é esse texto passa como veio, e o título de uma onda que a
//! spec não tem em andamento é barrado com o motivo. A exceção é a onda cuja
//! volta a rodada recusou depois de o Claude Code que a mandou fechar, levando
//! o agente dela, e a onda cuja volta quem conduz a obra reprovou: ela sai a
//! um agente novo, com o mesmo texto seguido do trecho de conserto — na
//! reprovação, o motivo de quem reprovou —, e ele trabalha na mesma cópia. O
//! despacho dele fica
//! gravado na spec, e outro agente novo é barrado enquanto o Claude Code que
//! o abriu segue aberto: a cópia nunca tem dois agentes. O título segue na
//! primeira linha: é por ele que a rodada acha a conversa do agente e soma o
//! gasto da onda. A mensagem do condutor a um agente de onda já aberto
//! (`SendMessage`), depois de a conferência depois da onda recusar a volta
//! dele, sai só com o trecho que a rodada devolveu para aquela onda; o agente
//! de destino é reconhecido pelo título da primeira mensagem da conversa
//! dele. A mensagem ao agente de uma onda cuja volta quem conduz a obra
//! reprovou, quando a conversa dele começou antes da volta reprovada, é
//! barrada, com o Claude Code do envio aberto ou fechado: a onda saiu dele, e
//! o motivo vai só ao agente novo. A mensagem a outro agente, ou sem trecho
//! gravado para a volta da onda, passa como veio.
//!
//! Um despacho sem bilhete a um agente do Mustard (`mustard-wave` ou
//! `mustard-review`), num projeto com `mustard.json`, ganha no topo a linha
//! dos dois idiomas do projeto, lida da configuração: é assim que os
//! consertos e as revisões cujo texto o orquestrador escreve recebem o idioma
//! dos nomes. O texto que já traz a linha, como o pedido da rodada,
//! passa como veio. O despacho ao `mustard-review` que o orquestrador
//! escreve — a revisão do levantamento e a do pull request — ganha no fim as
//! regras do projeto, os arquivos de regras da raiz e das pastas onde a obra
//! mexe, porque o revisor é instalado sem eles; o que manda ler o pedido da
//! revisão final não ganha, porque esse pedido já as traz. Qualquer outro
//! despacho sem bilhete é uma tarefa
//! qualquer e também passa como veio: o gancho não escolhe skill, não injeta
//! memória e não avalia a volta do agente. O pedido ao agente de exploração
//! do Claude Code sobe com a resposta curta do mapa no topo só quando a busca
//! cravou; a resposta parcial não vai.

use std::path::Path;

use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::domain::spec_events::{Refusal, SpecLog};
use mustard_core::domain::spec_state::{PhaseWriter, SpecState as _, State};
use mustard_core::domain::wave_prompt::{carries_project_rules, language_line, project_rules_section, wave_of_title, wave_title};
use mustard_core::io::spec_events as store;
use mustard_core::io::transcript::{agent_opening, heading_of};
use mustard_core::io::wave_prompt::{Flight, project_rules, prompts, touched_files};
use mustard_core::platform::error::Error;
use mustard_core::platform::i18n::Locale;
use mustard_core::{AGENT_NAMES, ProjectConfig};
use serde_json::{Value, json};

use crate::commands::spec_events::write::record_locked;
use crate::hooks::write::write_gate::say;
use crate::shared::spec_state::DiskSpecState;
use crate::shared::word_search;

/// O começo da linha do bilhete: depois dele vêm a spec e o número da onda.
pub const TICKET: &str = "MUSTARD-WAVE:";

/// A ferramenta com que o condutor manda mais uma mensagem a um agente já
/// aberto: `to` é o agente, `message` o texto.
const SEND_MESSAGE: &str = "SendMessage";

/// O pedido do subagente, no despacho de um agente.
pub struct SubagentInject;

/// O que o texto da tarefa traz.
#[derive(Debug, PartialEq, Eq)]
enum Ticket {
    /// Nenhum bilhete: uma tarefa qualquer.
    Absent,
    /// Um bilhete que não se lê; guarda o que veio depois da marca.
    Unreadable(String),
    /// A onda `wave` da spec `spec`.
    Wave { spec: String, wave: u64 },
}

/// O bilhete do texto da tarefa: a primeira linha que começa com [`TICKET`],
/// com a spec e um número de onda maior que zero, e nada mais.
fn ticket_of(prompt: &str) -> Ticket {
    let Some(rest) = prompt.lines().find_map(|line| line.trim().strip_prefix(TICKET)) else {
        return Ticket::Absent;
    };
    let mut parts = rest.split_whitespace();
    match (parts.next(), parts.next().and_then(|n| n.parse::<u64>().ok()), parts.next()) {
        (Some(spec), Some(wave), None) if wave > 0 => Ticket::Wave { spec: spec.to_string(), wave },
        _ => Ticket::Unreadable(rest.trim().to_string()),
    }
}

/// O pedido da onda `wave` da spec `spec`, montado do disco a partir de
/// `start`, ou o motivo de não despachar: a spec sem arquivo de eventos, a
/// spec que não está aprovada e a onda que o plano não tem.
fn assemble(start: &Path, spec: &str, wave: u64) -> Result<String, String> {
    let project = crate::commands::spec_events::project(start);
    let lang = project.lang;
    let log = spec_log(&project.root, spec, lang)?;
    let phase = State::from_log(&log).phase.unwrap_or("survey");
    if !crate::commands::flow::round::can_run(phase) {
        return Err(say("subagent.not_approved", lang, &[("{spec}", spec), ("{phase}", phase)]));
    }
    let wanted = wave.to_string();
    // As ondas em andamento são as da rodada, que já gravou o pedido desta e
    // o das que saíram junto: o pedido montado aqui é o mesmo que ela devolveu.
    let running = crate::commands::flow::round::waves_in_progress(&log).into_keys().collect();
    let flight = Flight { running, ..Flight::default() };
    let prompt = prompts(&project.root, spec, &log, lang, &flight)
        .into_iter()
        .find(|built| built.wave == wave)
        .ok_or_else(|| say("subagent.no_wave", lang, &[("{spec}", spec), ("{wave}", &wanted)]))?;
    Ok(prompt.text)
}

/// Os eventos da spec `spec` do projeto em `root`, ou o motivo de não os ter:
/// o nome que não é de spec e a spec sem arquivo de eventos.
fn spec_log(root: &Path, spec: &str, lang: Locale) -> Result<SpecLog, String> {
    let refused = |refusal: Refusal| refusal.message(lang);
    let path = store::spec_file(root, spec).map_err(refused)?;
    store::read(&path).map_err(refused)?.ok_or_else(|| refused(Refusal::NoSpecFile { spec: spec.to_string() }))
}

/// O despacho da onda `wave` da spec `spec` com o texto que a rodada monta
/// para ela ([`crate::commands::flow::round::wave_dispatch`]) no lugar do que
/// o condutor escreveu. A onda recusada cujo agente se foi com o Claude Code
/// que a mandou sai a um agente novo, um só ([`new_agent_fix`]), com o mesmo
/// texto seguido do trecho de conserto. Fora isso, a onda que a spec não tem
/// em andamento é barrada.
fn wave_dispatch(input: &HookInput, start: &Path, spec: &str, wave: u64) -> Verdict {
    let project = crate::commands::spec_events::project(start);
    let lang = project.lang;
    let log = match spec_log(&project.root, spec, lang) {
        Ok(log) => log,
        Err(reason) => return Verdict::Deny { reason },
    };
    let text = crate::commands::flow::round::wave_dispatch(&project.root, spec, wave, lang);
    if crate::commands::flow::round::waves_in_progress(&log).contains_key(&wave) {
        return replaced(input, "prompt", text, "subagent.dispatch_replaced", wave, lang);
    }
    let Some(fix) = new_agent_fix(&project.root, spec, wave) else {
        let reason = say("subagent.wave_not_running", lang, &[("{spec}", spec), ("{wave}", &wave.to_string())]);
        return Verdict::Deny { reason };
    };
    let lead = say("subagent.new_agent_fix", lang, &[("{wave}", &wave.to_string())]);
    replaced(input, "prompt", format!("{text}\n\n{lead}\n\n{}", fix.trim_end()), "subagent.new_agent", wave, lang)
}

/// O trecho de conserto da onda `wave` da spec `spec` que espera um agente
/// novo ([`crate::commands::flow::round::waves_awaiting_new_agent`]) — o da
/// conferência que recusou a volta, ou o motivo de quem a reprovou —, com o
/// despacho dele gravado: o envio da onda ganha uma versão com o Claude Code
/// atual ([`crate::commands::flow::stuck::sender_process`]), e outro despacho
/// é barrado enquanto esse Claude Code segue aberto; fechado também ele, a
/// onda volta a aceitar um agente novo. A versão guarda o lugar em que o envio
/// despachou a onda, então a onda segue voltada e o trecho segue valendo. Ler,
/// conferir e gravar correm com a trava do arquivo de eventos presa: de dois
/// despachos na mesma resposta, só um passa. Nada sem a gravação feita.
fn new_agent_fix(root: &Path, spec: &str, wave: u64) -> Option<String> {
    let path = store::spec_file(root, spec).ok()?;
    let taken = store::with_locked_writer(&path, |locked| {
        let file = crate::commands::flow::round::waves_awaiting_new_agent(root, spec, locked.log()).remove(&wave)?;
        let fix = std::fs::read_to_string(file).ok()?;
        let (pid, started) = crate::commands::flow::stuck::sender_process();
        let extra = json!({"claude_pid": pid, "claude_started": started}).as_object().cloned()?;
        let draft = crate::commands::flow::round::send_revision(locked.log(), wave, extra)?;
        record_locked(locked, root, spec, "send", draft, PhaseWriter::Binary).ok()?;
        Some(fix)
    });
    taken.ok().flatten().flatten()
}

/// A mensagem do condutor a um agente já aberto. Quando o destino é o agente
/// de uma onda — a primeira mensagem da conversa dele abre com o título do
/// pedido da onda — que a reprovação de quem conduz tirou dela
/// ([`crate::commands::flow::round::replaced_by_rejection`]), a mensagem é
/// barrada ([`replaced_agent_reason`]): o motivo vai só ao agente novo, e a
/// cópia nunca tem dois agentes. Toda chamada daquele agente já é recusada
/// pela trava do tamanho da conversa, também a que ele faria acordado pela
/// mensagem; a recusa aqui poupa essa volta e diz a quem conduz para onde
/// mandar o motivo. Quando a rodada gravou o trecho da
/// conferência depois da onda para a volta do agente da onda
/// ([`crate::commands::flow::round::fix_file`]), a mensagem sai só com esse
/// trecho. O resto passa como veio.
fn fix_message(input: &HookInput, ctx: &Ctx) -> Verdict {
    let root = ctx.project_dir_or_cwd(input);
    let project = crate::commands::spec_events::project(Path::new(&root));
    let lang = project.lang;
    let agent = input.tool_input.get("to").and_then(Value::as_str).unwrap_or_default();
    let destination = input.transcript_path().and_then(|transcript| agent_opening(Path::new(transcript), agent));
    let Some(((spec, wave), started)) = destination.and_then(|(heading, started)| Some((wave_of_title(&heading, lang)?, started))) else {
        return Verdict::Allow;
    };
    let Ok(log) = spec_log(&project.root, &spec, lang) else {
        return Verdict::Allow;
    };
    if crate::commands::flow::round::replaced_by_rejection(&log, wave, started) {
        return Verdict::Deny { reason: replaced_agent_reason(&project.root, &spec, &log, wave, lang) };
    }
    let fix = crate::commands::flow::round::fix_file(&project.root, &spec, &log, wave).and_then(|file| std::fs::read_to_string(file).ok());
    match fix {
        Some(text) => replaced(input, "message", text, "subagent.fix_replaced", wave, lang),
        None => Verdict::Allow,
    }
}

/// O motivo de barrar a mensagem ao agente que a reprovação de quem conduz
/// tirou da onda `wave` da spec `spec`: com a onda à espera do agente novo
/// ([`crate::commands::flow::round::waves_awaiting_new_agent`]), o título que
/// o despacha; com o agente novo já despachado, que ele recebeu o motivo.
fn replaced_agent_reason(root: &Path, spec: &str, log: &SpecLog, wave: u64, lang: Locale) -> String {
    let number = wave.to_string();
    if crate::commands::flow::round::waves_awaiting_new_agent(root, spec, log).contains_key(&wave) {
        let title = wave_title(spec, wave, lang);
        return say("subagent.rejected_agent", lang, &[("{wave}", &number), ("{title}", &title)]);
    }
    say("subagent.rejected_agent_working", lang, &[("{wave}", &number)])
}

/// O campo `field` da ferramenta trocado por `text`, com o recado `note` da
/// onda `wave` ao condutor; o que já é `text` passa como veio.
fn replaced(input: &HookInput, field: &str, text: String, note: &str, wave: u64, lang: Locale) -> Verdict {
    let sent = input.tool_input.get(field).and_then(Value::as_str).unwrap_or_default();
    if sent.trim_end() == text.trim_end() {
        return Verdict::Allow;
    }
    with_field(input, field, text, Some(say(note, lang, &[("{wave}", &wave.to_string())])))
}

/// O texto da tarefa, no `tool_input.prompt`.
fn dispatch_prompt(input: &HookInput) -> &str {
    input.tool_input.get("prompt").and_then(Value::as_str).unwrap_or_default()
}

/// Se o despacho vai a um agente do Mustard: o `subagent_type` é `mustard-`
/// seguido do nome de um dos agentes que o instalador grava.
fn to_mustard_agent(input: &HookInput) -> bool {
    input.tool_input.get("subagent_type").and_then(Value::as_str).and_then(|kind| kind.strip_prefix("mustard-")).is_some_and(|name| AGENT_NAMES.contains(&name))
}

/// O despacho com `text` no lugar do texto da tarefa e os outros campos como
/// vieram.
fn with_prompt(input: &HookInput, text: String) -> Verdict {
    with_field(input, "prompt", text, None)
}

/// A ferramenta com `text` no campo `field`, os outros campos como vieram e
/// o recado `note` a quem a chamou.
fn with_field(input: &HookInput, field: &str, text: String, note: Option<String>) -> Verdict {
    let mut tool_input = input.tool_input.clone();
    match tool_input.as_object_mut() {
        Some(fields) => {
            fields.insert(field.to_string(), Value::String(text));
            Verdict::Rewrite { tool_input, note }
        }
        None => Verdict::Allow,
    }
}

/// Se o despacho vai ao agente de exploração do Claude Code.
fn to_explore_agent(input: &HookInput) -> bool {
    input.tool_input.get("subagent_type").and_then(Value::as_str).is_some_and(|kind| kind.trim().eq_ignore_ascii_case("explore"))
}

/// O pedido ao agente de exploração com a resposta curta do mapa no topo,
/// quando o mapa cravou a resposta; senão, como veio.
fn explore(input: &HookInput, ctx: &Ctx) -> Verdict {
    let root = ctx.project_dir_or_cwd(input);
    let prompt = dispatch_prompt(input);
    match word_search::hook_ask(&root, input, ctx, prompt) {
        Some(answer) => with_prompt(input, format!("{answer}\n\n{prompt}")),
        None => Verdict::Allow,
    }
}

/// O despacho sem bilhete: a um agente do Mustard, num projeto com
/// `mustard.json`, o despacho de uma onda, que abre com o título do pedido
/// dela, sai com o texto que a rodada monta ([`wave_dispatch`]); qualquer
/// outro ganha no topo a linha dos idiomas, salvo quando já a traz. O
/// despacho ao revisor ganha também, no fim, as regras do projeto
/// ([`review_rules`]). O resto passa como veio.
fn without_ticket(input: &HookInput, ctx: &Ctx) -> Verdict {
    let root = ctx.project_dir_or_cwd(input);
    if !to_mustard_agent(input) || !ProjectConfig::exists(Path::new(&root)) {
        return Verdict::Allow;
    }
    let language = ctx.config.language();
    let prompt = dispatch_prompt(input);
    if let Some((spec, wave)) = wave_of_title(heading_of(prompt), language.text_or_default()) {
        return wave_dispatch(input, Path::new(&root), &spec, wave);
    }
    let line = language_line(&language);
    let mut text = if prompt.contains(&line) { prompt.to_string() } else { format!("{line}\n\n{prompt}") };
    if let Some(rules) = review_rules(input, Path::new(&root), prompt, language.text_or_default()) {
        text = format!("{}\n\n{rules}", text.trim_end());
    }
    if text == prompt {
        return Verdict::Allow;
    }
    with_prompt(input, text)
}

/// As regras do projeto que o despacho ao revisor ganha no fim, quando o
/// texto é do orquestrador — a revisão do levantamento e a do pull request:
/// o revisor é instalado sem os `CLAUDE.md`, e o pedido que ele recebe é o
/// único caminho delas. São os arquivos de regras da raiz e das pastas dos
/// arquivos que a obra da spec atual mexe ([`touched_files`]), a mesma
/// leitura da revisão final. Nada ao agente de onda, ao texto que já traz a
/// seção e ao que manda ler o pedido gravado da revisão final, que já a traz;
/// nada também sem arquivo de regras com texto.
fn review_rules(input: &HookInput, root: &Path, prompt: &str, lang: Locale) -> Option<String> {
    let to_reviewer = input.tool_input.get("subagent_type").and_then(Value::as_str) == Some("mustard-review");
    if !to_reviewer || prompt.contains("run read request-review") || carries_project_rules(prompt, lang) {
        return None;
    }
    let disk = DiskSpecState::new(root);
    let log = disk.active(input.session_id.as_deref()).and_then(|spec| disk.log(&spec));
    project_rules_section(&project_rules(root, &touched_files(root, log.as_ref())), lang)
}

impl Check for SubagentInject {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        if input.tool_name.as_deref() == Some(SEND_MESSAGE) {
            return Ok(fix_message(input, ctx));
        }
        let (spec, wave) = match ticket_of(dispatch_prompt(input)) {
            Ticket::Absent if to_explore_agent(input) => return Ok(explore(input, ctx)),
            Ticket::Absent => return Ok(without_ticket(input, ctx)),
            Ticket::Wave { spec, wave } => (spec, wave),
            Ticket::Unreadable(found) => {
                let lang: Locale = ctx.config.language().text_or_default();
                let reason = say("subagent.ticket_unreadable", lang, &[("{ticket}", TICKET), ("{found}", &found)]);
                return Ok(Verdict::Deny { reason });
            }
        };
        let root = ctx.project_dir_or_cwd(input);
        Ok(match assemble(Path::new(&root), &spec, wave) {
            Ok(text) => with_prompt(input, text),
            Err(reason) => Verdict::Deny { reason },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::spec_events::write::{WriteOpts, record_open, seed_at};
    use mustard_core::platform::i18n::translate;
    use serde_json::json;
    use tempfile::tempdir;

    fn ctx(root: &Path) -> Ctx {
        let mut ctx = Ctx::for_test(root.to_string_lossy().into_owned(), Some(Trigger::PreToolUse));
        ctx.config = mustard_core::ProjectConfig::load(root);
        ctx
    }

    fn dispatch(root: &Path, prompt: &str) -> Verdict {
        dispatch_to(root, "general-purpose", prompt)
    }

    /// O despacho de `prompt` ao agente `agent`, como o Claude Code o manda.
    fn dispatch_to(root: &Path, agent: &str, prompt: &str) -> Verdict {
        let input = HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            tool_name: Some("Task".to_string()),
            tool_input: json!({ "prompt": prompt, "subagent_type": agent, "description": "onda" }),
            ..HookInput::default()
        };
        SubagentInject.evaluate(&input, &ctx(root)).expect("never errors")
    }

    /// O texto que o despacho reescrito leva ao agente.
    fn rewritten(verdict: Verdict) -> String {
        match verdict {
            Verdict::Rewrite { tool_input, .. } => tool_input["prompt"].as_str().unwrap_or_default().to_string(),
            other => panic!("the dispatch is rewritten, got {other:?}"),
        }
    }

    fn write(root: &Path, event_type: &str, body: Value) -> u64 {
        let out = seed_at(&WriteOpts { root: root.to_path_buf(), spec: Some("x".to_string()), event_type: event_type.into(), json: body.to_string() });
        out["id"].as_u64().unwrap_or_else(|| panic!("não gravou: {out}"))
    }

    /// Uma spec `x` com uma onda de `tasks` tarefas, ainda no levantamento.
    fn planned(root: &Path, tasks: usize) {
        planned_with(root, tasks, false);
    }

    /// Uma linha crua, direto no arquivo da spec, sem passar pela gravação:
    /// [`planned_with`] a usa para as tarefas além do teto que a onda nasce
    /// com, simulando a onda grande que já existia antes dele.
    fn append_raw(root: &Path, event_type: &str, body: Value, id: u64) {
        let mut map = mustard_core::domain::spec_events::normalize(body.as_object().cloned().unwrap_or_default(), event_type);
        map.insert("type".into(), json!(event_type));
        let line = mustard_core::domain::spec_events::render_line(&mustard_core::domain::spec_events::stamp(map, id, None, "2026-09-20T10:00:00-03:00"));
        use std::io::Write as _;
        let path = store::spec_file(root, "x").unwrap();
        let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        writeln!(file, "{line}").unwrap();
    }

    /// A spec de [`planned`]; com `skills`, cada tarefa nomeia uma skill
    /// própria, gravada no disco, e cada uma ocupa uma linha do pedido. As
    /// tarefas além do teto de uma onda nova vão direto no arquivo, sem
    /// passar pela gravação, para `tasks` continuar podendo passar de três.
    fn planned_with(root: &Path, tasks: usize, skills: bool) {
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        assert_eq!(record_open(root, "x", "feature/x", "dev"), Ok(true));
        let said = write(root, "message", json!({"author": "user", "text": "o objetivo"}));
        let crit = write(
            root,
            "criterion",
            json!({"when": "a onda roda", "then": "a suíte passa", "proof": "cargo test", "form": "ubiquitous",
                "origin": said}),
        );
        let mut next_id = write(
            root,
            "wave",
            json!({"n": 1, "text": "Onda 1.", "criteria": [crit],
            "done_when": "A suíte passa.", "origin": said}),
        );
        for i in 0..tasks {
            let mut task = json!({"wave": 1, "text": format!("Tarefa {i}."), "files": [], "depends_on": [], "origin": said});
            if skills {
                let dir = root.join(".claude").join("skills").join(format!("s{i}"));
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(dir.join("SKILL.md"), format!("# s{i}\n")).unwrap();
                task["skill"] = json!(format!("s{i}"));
            }
            if i < 3 {
                next_id = write(root, "task", task);
            } else {
                next_id += 1;
                append_raw(root, "task", task, next_id);
            }
        }
    }

    fn approve(root: &Path) {
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join("x"));
    }

    /// O pedido que a rodada e a página montam para a onda 1.
    fn assembled(root: &Path) -> String {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        prompts(root, "x", &log, Locale::PtBr, &Flight::default()).into_iter().find(|p| p.wave == 1).expect("the wave's request").text
    }

    fn denied(verdict: Verdict) -> String {
        match verdict {
            Verdict::Deny { reason } => reason,
            other => panic!("the dispatch is refused, got {other:?}"),
        }
    }

    /// O bilhete é uma linha com a spec e o número da onda; o que não se lê
    /// assim é um bilhete estragado, e a tarefa sem a marca não é bilhete.
    #[test]
    fn the_ticket_carries_the_spec_and_the_wave_number() {
        assert_eq!(ticket_of("Faça a onda.\n"), Ticket::Absent);
        assert_eq!(ticket_of("MUSTARD-WAVE: x 2"), Ticket::Wave { spec: "x".into(), wave: 2 });
        assert_eq!(ticket_of("  MUSTARD-WAVE:   x   24  \nresto"), Ticket::Wave { spec: "x".into(), wave: 24 });
        for bad in ["MUSTARD-WAVE:", "MUSTARD-WAVE: x", "MUSTARD-WAVE: x dois", "MUSTARD-WAVE: x 0", "MUSTARD-WAVE: x 2 y"] {
            assert!(matches!(ticket_of(bad), Ticket::Unreadable(_)), "{bad}");
        }
    }

    /// Com a spec aprovada, o bilhete vira o pedido inteiro da onda, o mesmo
    /// que a rodada monta, e os outros campos da tarefa ficam como vieram.
    #[test]
    fn an_approved_spec_turns_the_ticket_into_the_whole_request() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        planned(root, 1);
        approve(root);
        match dispatch(root, "MUSTARD-WAVE: x 1") {
            Verdict::Rewrite { tool_input, .. } => {
                assert_eq!(tool_input["prompt"], json!(assembled(root)));
                let prompt = tool_input["prompt"].as_str().unwrap();
                assert!(prompt.lines().any(|l| l == "1. Faça a tarefa MSTD-TASK-0001 — Tarefa 0."), "{prompt}");
                assert!(prompt.lines().any(|l| l == "## Como ler cada item"), "{prompt}");
                assert_eq!(prompt.matches("mustard-rt run read item-<código> ").count(), 1, "{prompt}");
                assert_eq!(prompt.matches("mustard-rt run read").count(), 2, "{prompt}");
                assert_eq!(tool_input["subagent_type"], json!("general-purpose"));
                assert_eq!(tool_input["description"], json!("onda"));
            }
            other => panic!("the ticket is expanded, got {other:?}"),
        }
    }

    /// Sem aprovação, sem a onda no plano ou com o bilhete estragado, o
    /// despacho é barrado com o motivo, e o motivo nunca manda ler um
    /// arquivo.
    #[test]
    fn the_dispatch_is_refused_with_the_reason_and_never_points_to_a_file() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        planned(root, 1);
        let lang = Locale::PtBr;
        let reasons = [
            (denied(dispatch(root, "MUSTARD-WAVE: x 1")), say("subagent.not_approved", lang, &[("{spec}", "x"), ("{phase}", "survey")])),
            (denied(dispatch(root, "MUSTARD-WAVE: x 1 2")), say("subagent.ticket_unreadable", lang, &[("{ticket}", TICKET), ("{found}", "x 1 2")])),
        ];
        approve(root);
        let missing = denied(dispatch(root, "MUSTARD-WAVE: x 7"));
        let unknown = denied(dispatch(root, "MUSTARD-WAVE: outra 1"));
        for (got, expected) in reasons.iter().cloned().chain([
            (missing, say("subagent.no_wave", lang, &[("{spec}", "x"), ("{wave}", "7")])),
            (unknown, Refusal::NoSpecFile { spec: "outra".into() }.message(lang)),
        ]) {
            assert_eq!(got, expected);
            assert!(!got.contains(".md") && !got.to_lowercase().contains("leia"), "{got}");
        }
    }

    /// Uma onda com 500 tarefas e 500 skills — bem além do antigo teto de
    /// 500 linhas do pedido — é despachada do mesmo jeito, com o pedido
    /// inteiro: nada é recusado por causa do tamanho.
    #[test]
    fn a_wave_far_past_the_old_line_cap_is_dispatched_whole() {
        let big = tempdir().unwrap();
        planned_with(big.path(), 500, true);
        approve(big.path());
        match dispatch(big.path(), "MUSTARD-WAVE: x 1") {
            Verdict::Rewrite { tool_input, .. } => {
                let prompt = tool_input["prompt"].as_str().unwrap().to_string();
                assert!(prompt.lines().count() > 500, "{prompt}");
                let task_lines = prompt.lines().filter(|l| l.contains(". Faça a tarefa MSTD-TASK-")).count();
                assert_eq!(task_lines, 500, "{prompt}");
                assert!(prompt.contains("s499"), "{prompt}");
            }
            other => panic!("the wave is dispatched whole, got {other:?}"),
        }
    }

    /// Num projeto que declara o código em português, o despacho sem bilhete
    /// a um agente do Mustard abre com a linha dos dois idiomas lida da
    /// configuração, e o resto do texto segue como veio. O texto que já traz
    /// a linha passa sem repeti-la; o despacho sem bilhete a outro agente
    /// passa como veio; e o bilhete da onda vira o pedido, que traz a linha
    /// uma vez.
    #[test]
    fn a_mustard_agent_dispatch_opens_with_the_project_languages() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        planned(root, 1);
        approve(root);
        std::fs::write(root.join("mustard.json"), r#"{"language":{"code":"pt-BR"}}"#).unwrap();
        let line = translate("prompt.languages", Locale::PtBr).replace("{text}", "pt-BR").replace("{code}", "pt-BR");

        let fix = rewritten(dispatch_to(root, "mustard-wave", "Conserte o teste da soma."));
        assert_eq!(fix, format!("{line}\n\nConserte o teste da soma."));

        assert_eq!(dispatch_to(root, "mustard-wave", &fix), Verdict::Allow);
        let round = assembled(root);
        assert_eq!(round.matches(&line).count(), 1, "{round}");

        assert_eq!(dispatch_to(root, "general-purpose", "Investigue o gancho."), Verdict::Allow);

        let ticket = rewritten(dispatch_to(root, "mustard-wave", "MUSTARD-WAVE: x 1"));
        assert_eq!(ticket, round);
    }

    /// O despacho que o orquestrador escreve ao revisor — a revisão do
    /// levantamento, a do pull request — ganha no fim as regras do projeto: o
    /// texto do `CLAUDE.md` da raiz e, quando a pasta de um arquivo das
    /// tarefas da spec da branch tem regras, também as dela, cada uma sob o
    /// caminho; sem arquivo de regras, sai como antes, só com a linha dos
    /// idiomas. Não as ganham: o texto que já traz a seção, o despacho que
    /// manda ler o pedido gravado da revisão final, que já as traz, o conserto
    /// ao agente de onda e o pedido da onda.
    #[test]
    fn a_review_dispatch_written_by_the_conductor_ends_with_the_project_rules() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        planned(root, 1);
        append_raw(root, "task", json!({"text": "Mexer na biblioteca.", "files": [{"path": "lib/sub/a.rs"}], "depends_on": []}), 900);
        approve(root);
        let init = std::process::Command::new("git").args(["init", "-q", "-b", "feature/x"]).current_dir(root).output().unwrap();
        assert!(init.status.success(), "{init:?}");
        std::fs::write(root.join("mustard.json"), r#"{"language":{"code":"pt-BR"}}"#).unwrap();
        let line = translate("prompt.languages", Locale::PtBr).replace("{text}", "pt-BR").replace("{code}", "pt-BR");
        let survey = "Confira o levantamento inteiro da spec x.";
        assert_eq!(rewritten(dispatch_to(root, "mustard-review", survey)), format!("{line}\n\n{survey}"), "no file");

        let rules = "# Regras\n\n- O instalador nunca grava na configuração do git.";
        std::fs::write(root.join("CLAUDE.md"), format!("{rules}\n")).unwrap();
        let review = rewritten(dispatch_to(root, "mustard-review", survey));
        let section = format!("## Regras do projeto\n\n{}\n\n{rules}", translate("prompt.project_rules.source", Locale::PtBr));
        assert_eq!(review, format!("{line}\n\n{survey}\n\n{section}"));
        assert_eq!(dispatch_to(root, "mustard-review", &review), Verdict::Allow, "the section is never repeated");

        std::fs::create_dir_all(root.join("lib/sub")).unwrap();
        std::fs::write(root.join("lib/sub/CLAUDE.md"), "- A biblioteca não lê o disco.\n").unwrap();
        let nested = rewritten(dispatch_to(root, "mustard-review", survey));
        let sources = translate("prompt.project_rules.sources", Locale::PtBr);
        let both = format!("## Regras do projeto\n\n{sources}\n\n### `CLAUDE.md`\n\n{rules}\n\n### `lib/sub/CLAUDE.md`\n\n- A biblioteca não lê o disco.");
        assert_eq!(nested, format!("{line}\n\n{survey}\n\n{both}"));

        let final_review = format!("{line}\n\nmustard-rt run read request-review --root /r --spec x");
        assert_eq!(dispatch_to(root, "mustard-review", &final_review), Verdict::Allow, "the recorded request has them");
        let fix = rewritten(dispatch_to(root, "mustard-wave", "Conserte o teste da soma."));
        assert_eq!(fix, format!("{line}\n\nConserte o teste da soma."));
        let wave = rewritten(dispatch_to(root, "mustard-wave", "MUSTARD-WAVE: x 1"));
        assert!(
            wave.contains("configuração do git") && !wave.contains("lê o disco") && wave == assembled(root),
            "the wave receives the root rules and only its touched directory rules: {wave}"
        );
    }

    /// O despacho que abre com o título de uma onda que a spec não tem em
    /// andamento é barrado com o motivo: a onda ainda não saiu, a spec não
    /// existe. O que não abre com o título de uma onda segue ganhando a linha
    /// dos idiomas: o conserto, o título de outro assunto, o de uma onda que
    /// não existe e o título que não está na primeira linha.
    #[test]
    fn a_wave_title_that_is_not_in_progress_is_refused_and_other_texts_gain_the_language_line() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        planned(root, 1);
        approve(root);
        std::fs::write(root.join("mustard.json"), r#"{"language":{"code":"pt-BR"}}"#).unwrap();
        let line = translate("prompt.languages", Locale::PtBr).replace("{text}", "pt-BR").replace("{code}", "pt-BR");
        let title = assembled(root).lines().next().unwrap_or_default().to_string();
        assert_eq!(title, "# x — onda 1");
        let command = "Leia o seu pedido inteiro com o comando abaixo:\n\nmustard-rt run read request-1 --root /r --spec x";

        for agent in ["mustard-wave", "mustard-review"] {
            assert_eq!(
                denied(dispatch_to(root, agent, &format!("{title}\n\n{command}"))),
                say("subagent.wave_not_running", Locale::PtBr, &[("{spec}", "x"), ("{wave}", "1")]),
                "{agent}"
            );
        }
        assert_eq!(
            denied(dispatch_to(root, "mustard-wave", &format!("# outra-obra — onda 12  \n\n{command}"))),
            Refusal::NoSpecFile { spec: "outra-obra".into() }.message(Locale::PtBr)
        );

        for not_a_wave in [
            format!("Conserte o teste da soma.\n\n{command}"),
            format!("# Conserte o teste da soma\n\n{command}"),
            format!("# x — onda 0\n\n{command}"),
            format!("Antes de tudo:\n{title}\n\n{command}"),
        ] {
            assert_eq!(rewritten(dispatch_to(root, "mustard-wave", &not_a_wave)), format!("{line}\n\n{not_a_wave}"));
        }
    }

    /// A spec `x` aprovada, com a onda 1 enviada pelo Claude Code `pid`, que
    /// começou em `started`, e a volta que o agente dela gravou depois. Devolve
    /// o arquivo do trecho de conserto dessa volta, ainda fora do disco, e o
    /// motivo com que o despacho de um agente novo é barrado.
    fn refused_wave(root: &Path, pid: u32, started: u64) -> (std::path::PathBuf, String) {
        planned(root, 1);
        approve(root);
        let seed = |event_type: &str, body: Value| crate::shared::spec_state::seed_event(root, "x", event_type, body);
        seed(
            "send",
            json!({"wave": 1, "role": "wave", "text": "o pedido", "lines": 1, "chars": 8, "mustard": "0",
            "author": "binary", "claude_pid": pid, "claude_started": started}),
        );
        seed("delivered", json!({"wave": 1, "text": "Feito.", "files": [], "returned": true}));
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let fix = crate::commands::flow::round::fix_file(root, "x", &log, 1).expect("the pending return");
        std::fs::create_dir_all(fix.parent().unwrap()).unwrap();
        (fix, say("subagent.wave_not_running", Locale::PtBr, &[("{spec}", "x"), ("{wave}", "1")]))
    }

    /// O trecho que a rodada devolveu para a onda 1.
    const SECTION: &str = "Onda 1, rodada de conserto 1 de 2:\n- `src/a.rs` linha 1 importa o que a regra barra.";

    /// A onda recusada ganha um agente novo só: de dois despachos ao mesmo
    /// tempo, um passa com o trecho e o outro é barrado, e o despacho seguinte
    /// também, enquanto o Claude Code que abriu o agente novo segue aberto.
    /// Fechado também ele, a onda volta a aceitar um agente novo.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_second_new_agent_is_refused_while_the_first_ones_claude_code_is_open() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (pid, started) = crate::commands::flow::round::closed_process();
        let (fix, refused) = refused_wave(root, pid, started);
        std::fs::write(&fix, format!("{SECTION}\n")).unwrap();
        let title = mustard_core::domain::wave_prompt::wave_title("x", 1, Locale::PtBr);
        let sent = format!("{title}\n\nConserte a onda.");
        let read = crate::commands::flow::round::read_command(root, "x", "request-1");
        let lead = say("subagent.new_agent_fix", Locale::PtBr, &[("{wave}", "1")]);
        let new_agent = format!("{title}\n\n{read}\n\n{lead}\n\n{SECTION}");

        let together = std::sync::Barrier::new(2);
        let verdicts: Vec<Verdict> = std::thread::scope(|scope| {
            let dispatches: Vec<_> = (0..2)
                .map(|_| {
                    scope.spawn(|| {
                        together.wait();
                        dispatch_to(root, "mustard-wave", &sent)
                    })
                })
                .collect();
            dispatches.into_iter().map(|dispatch| dispatch.join().unwrap()).collect()
        });
        let (passed, barred): (Vec<Verdict>, Vec<Verdict>) = verdicts.into_iter().partition(|verdict| matches!(verdict, Verdict::Rewrite { .. }));
        assert_eq!(passed.into_iter().map(rewritten).collect::<Vec<_>>(), vec![new_agent.clone()], "one new agent");
        assert_eq!(barred.into_iter().map(denied).collect::<Vec<_>>(), vec![refused.clone()]);
        assert_eq!(denied(dispatch_to(root, "mustard-wave", &sent)), refused, "the new agent's Claude Code is open");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let (pid, started) = crate::commands::flow::round::closed_process();
        let closed = json!({"claude_pid": pid, "claude_started": started}).as_object().cloned().unwrap();
        let draft = crate::commands::flow::round::send_revision(&log, 1, closed).expect("the wave's send");
        crate::shared::spec_state::seed_event(root, "x", "send", Value::Object(draft));
        assert_eq!(rewritten(dispatch_to(root, "mustard-wave", &sent)), new_agent, "its Claude Code closed too");
    }

    /// A onda cuja volta a rodada recusou, com o trecho de conserto em disco
    /// e o Claude Code do envio já fechado, sai a um agente novo: o título, o
    /// comando que lê o pedido, a frase de que o código já está na cópia e o
    /// trecho, com o recado ao condutor. Sem o trecho em disco, segue barrada.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_refused_wave_whose_sender_closed_goes_to_a_new_agent_with_the_fix() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (pid, started) = crate::commands::flow::round::closed_process();
        let (fix, refused) = refused_wave(root, pid, started);
        let title = mustard_core::domain::wave_prompt::wave_title("x", 1, Locale::PtBr);
        let sent = format!("{title}\n\nConserte a onda.");
        assert_eq!(denied(dispatch_to(root, "mustard-wave", &sent)), refused, "no section on disk");

        std::fs::write(&fix, format!("{SECTION}\n")).unwrap();
        let read = crate::commands::flow::round::read_command(root, "x", "request-1");
        let lead = say("subagent.new_agent_fix", Locale::PtBr, &[("{wave}", "1")]);
        match dispatch_to(root, "mustard-wave", &sent) {
            Verdict::Rewrite { tool_input, note } => {
                assert_eq!(tool_input["prompt"], json!(format!("{title}\n\n{read}\n\n{lead}\n\n{SECTION}")));
                assert_eq!(note, Some(say("subagent.new_agent", Locale::PtBr, &[("{wave}", "1")])));
            }
            other => panic!("the dispatch goes to a new agent, got {other:?}"),
        }
    }

    /// Com o Claude Code do envio ainda aberto, o conserto é do agente que fez
    /// a onda: o agente novo segue barrado, mesmo com o trecho em disco.
    #[test]
    fn a_refused_wave_whose_sender_is_open_still_refuses_a_new_agent() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (pid, started) = crate::commands::flow::stuck::sender_process();
        let (fix, refused) = refused_wave(root, pid, started);
        std::fs::write(&fix, format!("{SECTION}\n")).unwrap();
        let title = mustard_core::domain::wave_prompt::wave_title("x", 1, Locale::PtBr);
        assert_eq!(denied(dispatch_to(root, "mustard-wave", &format!("{title}\n\nConserte a onda."))), refused);
    }

    /// O pedido ao agente de exploração ganha no topo a resposta curta do
    /// mapa, com o arquivo, a função e as linhas, e o resto do texto segue
    /// como veio; o mesmo pedido a outro agente, o pedido sem nome de código,
    /// o que o mapa não acha, o sem mapa e o com a chave `search.answer`
    /// desligada passam como vieram, e o bilhete da onda segue igual.
    #[test]
    fn the_request_to_an_explorer_opens_with_the_short_answer_of_the_map() {
        let (_dir, root) = crate::shared::word_search::fixture::repo("{}");
        let prompt = "Mapeie o cálculo do frete. Onde `calcular_frete` é usado? Leia src/frete.rs e responda em português.";
        let sent = rewritten(dispatch_to(&root, "Explore", prompt));
        assert!(sent.starts_with("Antes de explorar, o Mustard consultou o mapa com este pedido.\n"), "{sent}");
        assert!(sent.contains("src/frete.rs\n  2-6 calcular_frete"), "{sent}");
        assert!(sent.ends_with(&format!("\n\n{prompt}")), "the request follows as it came: {sent}");
        assert_eq!(dispatch_to(&root, "general-purpose", prompt), Verdict::Allow);
        assert_eq!(dispatch_to(&root, "Explore", "Explore o repositório inteiro e resuma."), Verdict::Allow);
        assert_eq!(dispatch_to(&root, "Explore", "Onde fica `zzyzx_quebrada`?"), Verdict::Allow);
        let (_off, off) = crate::shared::word_search::fixture::repo(r#"{"search":{"answer":false}}"#);
        assert_eq!(dispatch_to(&off, "Explore", prompt), Verdict::Allow);
        let (_bare, bare) = crate::shared::word_search::fixture::repo("{}");
        std::fs::remove_file(mustard_core::io::project_map::model_path(&bare)).unwrap();
        assert_eq!(dispatch_to(&bare, "Explore", prompt), Verdict::Allow, "no map");
        let dir = tempdir().unwrap();
        planned(dir.path(), 1);
        approve(dir.path());
        assert_eq!(rewritten(dispatch_to(dir.path(), "Explore", "MUSTARD-WAVE: x 1")), assembled(dir.path()), "the wave ticket stays what it was");
    }

    /// O pedido ao agente de exploração cuja busca o mapa acha só em parte
    /// (a palavra está no comentário da função, não no nome) passa como veio,
    /// sem o bloco do mapa; o pedido cuja busca o mapa crava ganha o bloco, e
    /// o mesmo pedido parcial a outro agente também passa como veio.
    #[test]
    fn a_partial_request_to_an_explorer_goes_without_the_block_of_the_map() {
        let (_dir, root) = crate::shared::word_search::fixture::repo("{}");
        let partial = "Onde fica o `imposto` do frete? Responda em português.";
        let pinned = "Onde `calcular_frete` é usado? Leia src/frete.rs e responda em português.";
        assert_eq!(dispatch_to(&root, "Explore", partial), Verdict::Allow, "the partial answer stays out of the request");
        assert_eq!(dispatch_to(&root, "general-purpose", partial), Verdict::Allow);
        let sent = rewritten(dispatch_to(&root, "Explore", pinned));
        assert!(sent.starts_with("Antes de explorar, o Mustard consultou o mapa com este pedido.\n"), "{sent}");
        assert!(sent.contains("Cravado.") && sent.contains("src/frete.rs\n  2-6 calcular_frete"), "{sent}");
        assert!(sent.ends_with(&format!("\n\n{pinned}")), "the request follows as it came: {sent}");
    }

    /// Uma tarefa sem bilhete passa como veio, e o gancho não age fora do
    /// despacho. Fora de um projeto com `mustard.json`, nem o despacho a um
    /// agente do Mustard ganha a linha dos idiomas.
    #[test]
    fn a_task_without_a_ticket_passes_untouched() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        assert_eq!(dispatch(root, "Investigue o gancho.\nSKILL: foo"), Verdict::Allow);
        assert_eq!(dispatch_to(root, "mustard-wave", "Conserte o teste da soma."), Verdict::Allow);
        let input = HookInput { tool_name: Some("Task".to_string()), tool_input: json!({ "prompt": "MUSTARD-WAVE: x 1" }), ..HookInput::default() };
        let after = Ctx::for_test(root.to_string_lossy().into_owned(), Some(Trigger::PostToolUse));
        assert_eq!(SubagentInject.evaluate(&input, &after).unwrap(), Verdict::Allow);
    }
}
