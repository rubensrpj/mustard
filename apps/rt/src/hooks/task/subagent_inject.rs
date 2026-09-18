//! `subagent_inject` — o pedido da onda, montado pelo número dela.
//!
//! No despacho de um agente (`PreToolUse` de `Task` ou `Agent`), o texto da
//! tarefa pode ser só um bilhete: uma linha [`TICKET`] com a spec e o número
//! da onda, como `MUSTARD-WAVE: mustard-enxuto 24`. O gancho troca o texto
//! inteiro pelo pedido que o binário monta para aquela onda — a lista dos
//! itens, com as lições e as skills —, pela mesma montagem da rodada e da
//! página, e o agente recebe o pedido já no corpo, sem ler arquivo nenhum.
//!
//! Antes de trocar, o gancho confere duas coisas: a spec está aprovada, e o
//! pedido cabe no teto de linhas. Quando uma delas falha, ou quando o bilhete
//! não se lê, o despacho é barrado, e o motivo diz o que falta. O gancho
//! nunca manda o agente ler um arquivo no lugar do pedido.
//!
//! Um despacho sem bilhete é uma tarefa qualquer e passa como veio: o gancho
//! não escolhe skill, não injeta memória e não avalia a volta do agente.

use std::path::Path;

use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::domain::spec_events::Refusal;
use mustard_core::domain::spec_state::State;
use mustard_core::io::spec_events as store;
use mustard_core::io::wave_prompt::{prompts, Flight};
use mustard_core::platform::error::Error;
use mustard_core::platform::i18n::Locale;
use serde_json::Value;

use crate::hooks::write::write_gate::say;

/// O começo da linha do bilhete: depois dele vêm a spec e o número da onda.
pub const TICKET: &str = "MUSTARD-WAVE:";

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
/// spec que não está aprovada, a onda que o plano não tem e o pedido acima do
/// teto de linhas.
fn assemble(start: &Path, spec: &str, wave: u64) -> Result<String, String> {
    let project = crate::commands::spec_events::project(start);
    let lang = project.lang;
    let refused = |refusal: Refusal| refusal.message(lang);
    let path = store::spec_file(&project.root, spec).map_err(refused)?;
    let log = store::read(&path)
        .map_err(refused)?
        .ok_or_else(|| refused(Refusal::NoSpecFile { spec: spec.to_string() }))?;
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
    match prompt.too_long {
        Some(refusal) => Err(refused(refusal)),
        None => Ok(prompt.text),
    }
}

/// O texto da tarefa, no `tool_input.prompt`.
fn dispatch_prompt(input: &HookInput) -> &str {
    input.tool_input.get("prompt").and_then(Value::as_str).unwrap_or_default()
}

impl Check for SubagentInject {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        let (spec, wave) = match ticket_of(dispatch_prompt(input)) {
            Ticket::Absent => return Ok(Verdict::Allow),
            Ticket::Wave { spec, wave } => (spec, wave),
            Ticket::Unreadable(found) => {
                let lang: Locale = ctx.config.language().text_or_default();
                let reason = say("subagent.ticket_unreadable", lang, &[("{ticket}", TICKET), ("{found}", &found)]);
                return Ok(Verdict::Deny { reason });
            }
        };
        let root = ctx.project_dir_or_cwd(input);
        Ok(match assemble(Path::new(&root), &spec, wave) {
            Ok(text) => {
                let mut tool_input = input.tool_input.clone();
                match tool_input.as_object_mut() {
                    Some(fields) => {
                        fields.insert("prompt".to_string(), Value::String(text));
                        Verdict::Rewrite { tool_input }
                    }
                    None => Verdict::Allow,
                }
            }
            Err(reason) => Verdict::Deny { reason },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::spec_events::write::{record_open, seed_at, WriteOpts};
    use serde_json::json;
    use tempfile::tempdir;

    fn ctx(root: &Path) -> Ctx {
        let mut ctx = Ctx::for_test(root.to_string_lossy().into_owned(), Some(Trigger::PreToolUse));
        ctx.config = mustard_core::ProjectConfig::load(root);
        ctx
    }

    fn dispatch(root: &Path, prompt: &str) -> Verdict {
        let input = HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            tool_name: Some("Task".to_string()),
            tool_input: json!({ "prompt": prompt, "subagent_type": "general-purpose", "description": "onda" }),
            ..HookInput::default()
        };
        SubagentInject.evaluate(&input, &ctx(root)).expect("never errors")
    }

    fn write(root: &Path, event_type: &str, body: Value) -> u64 {
        let out = seed_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: Some("x".to_string()),
            event_type: event_type.into(),
            json: body.to_string(),
        });
        out["id"].as_u64().unwrap_or_else(|| panic!("não gravou: {out}"))
    }

    /// Uma spec `x` com uma onda de `tasks` tarefas, ainda no levantamento.
    fn planned(root: &Path, tasks: usize) {
        planned_with(root, tasks, false);
    }

    /// A spec de [`planned`]; com `skills`, cada tarefa nomeia uma skill
    /// própria, gravada no disco, e cada uma ocupa uma linha do pedido.
    fn planned_with(root: &Path, tasks: usize, skills: bool) {
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        assert_eq!(record_open(root, "x", "feature/x", "dev"), Ok(true));
        let said = write(root, "message", json!({"author": "user", "text": "o objetivo"}));
        let crit = write(
            root,
            "criterion",
            json!({"when": "a onda roda", "then": "a suíte passa", "proof": "cargo test", "origin": said}),
        );
        write(root, "wave", json!({"n": 1, "text": "Onda 1.", "criteria": [crit], "done_when": "A suíte passa.",
            "origin": said}));
        for i in 0..tasks {
            let mut task = json!({"wave": 1, "text": format!("Tarefa {i}."), "origin": said});
            if skills {
                let dir = root.join(".claude").join("skills").join(format!("s{i}"));
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(dir.join("SKILL.md"), format!("# s{i}\n")).unwrap();
                task["skill"] = json!(format!("s{i}"));
            }
            write(root, "task", task);
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
            Verdict::Rewrite { tool_input } => {
                assert_eq!(tool_input["prompt"], json!(assembled(root)));
                let prompt = tool_input["prompt"].as_str().unwrap();
                assert!(prompt.contains("- `waves`: MSTD-WAVE-0001, MSTD-TASK-0001\n"), "{prompt}");
                assert_eq!(prompt.matches("mustard-rt run read").count(), 1, "{prompt}");
                assert_eq!(tool_input["subagent_type"], json!("general-purpose"));
                assert_eq!(tool_input["description"], json!("onda"));
            }
            other => panic!("the ticket is expanded, got {other:?}"),
        }
    }

    /// Sem aprovação, sem a onda no plano, com o pedido acima do teto ou com
    /// o bilhete estragado, o despacho é barrado com o motivo, e o motivo
    /// nunca manda ler um arquivo.
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

        // Os códigos das tarefas cabem numa linha só: o que passa do teto é
        // uma parte de uma linha por item, como a das skills.
        let big = tempdir().unwrap();
        planned_with(big.path(), mustard_core::domain::wave_prompt::MAX_LINES, true);
        approve(big.path());
        let long = denied(dispatch(big.path(), "MUSTARD-WAVE: x 1"));
        assert!(long.contains(&mustard_core::domain::wave_prompt::MAX_LINES.to_string()), "{long}");
    }

    /// Uma tarefa sem bilhete passa como veio, e o gancho não age fora do
    /// despacho.
    #[test]
    fn a_task_without_a_ticket_passes_untouched() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        assert_eq!(dispatch(root, "Investigue o gancho.\nSKILL: foo"), Verdict::Allow);
        let input = HookInput {
            tool_name: Some("Task".to_string()),
            tool_input: json!({ "prompt": "MUSTARD-WAVE: x 1" }),
            ..HookInput::default()
        };
        let after = Ctx::for_test(root.to_string_lossy().into_owned(), Some(Trigger::PostToolUse));
        assert_eq!(SubagentInject.evaluate(&input, &after).unwrap(), Verdict::Allow);
    }
}
