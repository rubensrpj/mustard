//! Os comandos do fluxo de uma spec, um por passo. Hoje moram aqui o `open`,
//! que abre a spec: a branch, o `spec.ndjson` com o mesmo nome e o nascimento
//! em levantamento; o `grill`, que grava o tipo de trabalho e monta a lista de
//! pontos do levantamento; o `answer`, que grava a resposta de um ponto e o
//! fecha; e o `reopen`, o caminho de volta, que leva a spec
//! ao levantamento de novo com o motivo gravado; e o `plan`, que monta o
//! pedido de cada onda, confere o plano e leva a spec do levantamento para o
//! plano; e o `round`, a rodada de ondas, que despacha, grava o que voltou,
//! formata e faz o commit; e o `triage`, que reorganiza a lista aberta pelo
//! classificador das atividades. Cada passo responde por [`answer()`], que
//! grava a chamada no arquivo de operações da spec antes de imprimir.

pub mod answer;
pub mod cli;
pub mod close;
pub mod common;
pub mod discard;
pub(crate) mod evidence;
pub(crate) mod goal;
pub mod grill;
pub mod open;
pub mod plan;
pub mod reopen;
pub mod resume;
pub mod round;
pub(crate) mod stuck;
pub mod triage;
pub(crate) mod validation;

use std::path::Path;
use std::time::Instant;

use serde_json::Value;

use crate::commands::spec_events::conversation::record_output_call;

/// Os passos do fluxo: os comandos que respondem por [`answer()`] e as duas
/// portas do pull request, a que o abre e a que o une. É por esta lista que a
/// retomada diz o último passo, sem a retomada mesma, que só lê; a procura, a
/// leitura, as buscas e o mapa não são passo.
pub(crate) const STEPS: &[&str] = &[
    "evidence",
    "open",
    "grill",
    "answer",
    "reopen",
    "plan",
    "round",
    "review-wave",
    "close",
    "discard",
    "triage",
    "resume",
    "pr-open",
    "pr-merge",
];

/// A resposta de um passo do fluxo: grava a chamada no arquivo de operações
/// da spec, imprime o relatório e sai com 1 na recusa. `named` é a spec que a
/// chamada nomeou; `full` pede a resposta inteira em vez da curta
/// ([`projection`]). O comando é um de [`STEPS`].
pub(crate) fn answer(command: &str, root: &Path, named: Option<&str>, started: Instant, report: &Value, full: bool) {
    // Host-facing transport keeps action/evidence fields, without indentation overhead.
    // Measure exactly these bytes rather than a synthetic pretty JSON copy.
    let projected = projection(command, report, full);
    let report = projected.as_ref().unwrap_or(report);
    let output=report.to_string()+"\n";
    let _ = record_output_call(root, command, named, started, report,&output);
    print!("{output}");
    if report.get("ok").and_then(Value::as_bool) != Some(true) {
        std::process::exit(1);
    }
}

/// O único lugar que decide o que a resposta de um comando leva. A curta,
/// que é a de sempre, traz só o que a próxima ação pede: a rodada agrupa as
/// tarefas seguradas pelo motivo; a revisão de onda abre com a impressão
/// digital, traz o resumo — o diff só a pedido — e dá as regras por caminho
/// e tamanho; o
/// levantamento abre com a dica, o próximo ponto e os códigos dos abertos; o
/// backlog abre com o total, sem campo vazio por linha; e os ajustes dão só
/// os valores atuais. Com `full`, cada comando devolve a resposta inteira. A
/// recusa sai sempre inteira. `None` quando o comando não tem projeção.
pub(crate) fn projection(command: &str, report: &Value, full: bool) -> Option<Value> {
    let whole = match command {
        "grill" => Some(grill::projection(report)),
        _ => None,
    };
    if full || report.get("ok").and_then(Value::as_bool) != Some(true) {
        return whole;
    }
    let base = whole.as_ref().unwrap_or(report);
    match command {
        "grill" => Some(grill::short(base)),
        "round" => Some(round::short(base)),
        "review-wave" => Some(round::review::short(base)),
        "settings" => Some(crate::commands::panel::short_settings(base)),
        "backlog" => Some(crate::commands::spec_events::read::short_backlog(base)),
        _ => whole,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{RunCmd, flow::cli::FlowCmd, panel::cli::PanelCmd, spec_events::cli::SpecEventsCmd};
    use serde_json::json;

    #[derive(clap::Parser)]
    struct Harness {
        #[command(subcommand)]
        cmd: RunCmd,
    }

    /// O `--full` de cada comando que o pedido de cada onda nomeia, lido
    /// pelo parser do binário.
    fn full_of(line: &str) -> bool {
        let parsed: Harness = clap::Parser::try_parse_from(line.split_whitespace()).unwrap_or_else(|e| panic!("{line}: {e}"));
        match parsed.cmd {
            RunCmd::Flow(FlowCmd::Grill { full, .. } | FlowCmd::Round { full, .. } | FlowCmd::ReviewWave { full, .. })
            | RunCmd::SpecEvents(SpecEventsCmd::Read { full, .. })
            | RunCmd::Panel(PanelCmd::Settings { full, .. }) => full,
            _ => panic!("not a command with a short answer: {line}"),
        }
    }

    /// Cada comando de resposta curta aceita `--full`, e com ele a resposta é
    /// a de antes da curta: a projeção que o levantamento e a rodada já
    /// tinham e, nos outros, o relatório como veio. Sem ele, cada um encolhe.
    #[test]
    fn short_answer_gives_way_to_the_whole_answer_with_full() {
        for line in ["run grill --full", "run round --dispatch --full", "run review-wave --wave 1 --full", "run read backlog --full", "run settings --full"] {
            assert!(full_of(line), "{line}");
            assert!(!full_of(&line.replace(" --full", "")), "{line}");
        }
        let grill = json!({"ok":true,"spec":"x","work_type":4,"kinds":["fix"],"condensed":false,"points":[
            {"block":"context","gap":"Who uses it","from":"gap","origin":2,"id":5,"code":"MSTD-POINT-0001","status":"open","facts":["a.ts:1"],"reminders":[]},
            {"block":"context","gap":"Where it runs","from":"gap","origin":2,"id":6,"code":"MSTD-POINT-0002","status":"closed","facts":[]}],
            "reminders":0,"goal_approval":{"confirmed":true},"next":{"id":5},"hint":"Present MSTD-POINT-0001."});
        let round = json!({"ok":true,"spec":"x","dispatch":[],"running":[],"held":[
            {"task":"MSTD-TASK-0002","reason":"capacity","detail":"No free slot."},
            {"task":"MSTD-TASK-0003","reason":"capacity","detail":"No free slot."}],
            "next":"Wait."});
        let review = json!({"ok":true,"spec":"x","wave":1,"fingerprint":"abc","files":["web/app.ts"],
            "project_rules":[{"source":"project","path":"CLAUDE.md","text":"Never duplicate types."}],"diff":"+x","hint":"Read the diff."});
        let backlog = json!({"ok":true,"spec":"x","block":"backlog","count":1,
            "events":[{"id":7,"code":"MSTD-TASK-0001","wave":null,"files":2,"depends_on":[]}],"total":{"tasks":1,"files":2}});
        let settings = json!({"ok":true,"schema_version":1,"revision":"r1","values":{"rtk":false},
            "jev":{"usable_key":false},"rtk":{"own_hook_left":false},"index_present":false,"availability":{"scan":"ok"}});
        let today = [
            ("grill", &grill, Some(grill::projection(&grill))),
            ("round", &round, None),
            ("review-wave", &review, None),
            ("backlog", &backlog, None),
            ("settings", &settings, None),
        ];
        for (command, report, before) in today {
            assert_eq!(projection(command, report, true), before, "{command}");
            let short = projection(command, report, false).unwrap_or_else(|| panic!("{command} has a short answer"));
            let whole = before.unwrap_or_else(|| report.clone());
            assert!(short.to_string().len() < whole.to_string().len(), "{command}: {short}");
        }
        let short = projection("grill", &grill, false).unwrap();
        let keys: Vec<&String> = short.as_object().unwrap().keys().collect();
        assert_eq!(keys[..3], ["ok", "hint", "next"], "{short}");
        assert_eq!(short["open"], json!(["MSTD-POINT-0001"]), "{short}");
        assert!(short.get("points").is_none() && short.get("goal_approval").is_none(), "{short}");
        let short = projection("backlog", &backlog, false).unwrap();
        assert_eq!(short.as_object().unwrap().keys().nth(1).map(String::as_str), Some("total"), "{short}");
        assert_eq!(short["events"], json!([{"id":7,"code":"MSTD-TASK-0001","files":2}]), "{short}");
        let short = projection("settings", &settings, false).unwrap();
        assert_eq!(short, json!({"ok":true,"schema_version":1,"revision":"r1","values":{"rtk":false}}));
        let refused = json!({"ok":false,"reason":"wave-review-unavailable","hint":"No pending delivery."});
        assert_eq!(projection("review-wave", &refused, false), None, "a refusal leaves whole");
    }
}
