//! A tarefa que espera plano: presa (`blocked_by`) e sem arquivo nenhum, como
//! a que nasce da sobra de onda que não cita arquivo. Ela não sai numa onda
//! até quem conduz a obra gravar uma versão nova com os arquivos e as
//! dependências. A fila do painel, a resposta da rodada e a leitura do
//! backlog a reconhecem por esta mesma leitura, para as três dizerem a mesma
//! coisa dela.

use std::collections::BTreeMap;

use mustard_core::domain::spec_events::{SpecEvent, SpecLog};
use mustard_core::platform::i18n::{Locale, translate};

use super::queue::{backlog_left, task_files};

/// `true` quando a tarefa `task` espera plano: traz a trava (`blocked_by`) e
/// não declara arquivo nenhum. Sem arquivo, um agente partiria sem saber o
/// que tocar, e a revisão não teria escopo a conferir.
pub(crate) fn needs_planning(task: &SpecEvent) -> bool {
    task.str_field("blocked_by").is_some() && task_files(task).is_empty()
}

/// O que a rodada diz do backlog em que nenhuma tarefa está pronta e nada
/// anda. As tarefas que esperam plano ([`needs_planning`]) saem numa linha
/// que diz o que falta a elas: os arquivos e as dependências. As outras saem
/// na linha das presas, por dependência aberta ou por item que não cobrem.
/// Cada tarefa vai pelo código de `codes`, ou pelo número, sem código.
pub(super) fn stuck_backlog_line(log: &SpecLog, codes: &BTreeMap<u64, String>, lang: Locale) -> String {
    let (planning, stuck): (Vec<u64>, Vec<u64>) =
        backlog_left(log).into_iter().partition(|id| log.get(*id).is_some_and(needs_planning));
    let named = |ids: &[u64]| ids.iter().map(|id| codes.get(id).cloned().unwrap_or_else(|| id.to_string())).collect::<Vec<_>>().join(", ");
    let mut lines = Vec::new();
    if !planning.is_empty() {
        lines.push(translate("round.backlog_needs_planning", lang).replace("{tasks}", &named(&planning)));
    }
    if !stuck.is_empty() {
        lines.push(translate("round.backlog_stuck", lang).replace("{tasks}", &named(&stuck)));
    }
    lines.join(" ")
}

#[cfg(test)]
mod tests {
    use mustard_core::domain::spec_state::PhaseWriter;
    use serde_json::{Value, json};

    use super::super::queue::{backlog_project, spec_now};
    use super::super::tests::{round, waves_in};
    use super::*;
    use crate::commands::spec_events::read::{ReadOpts, read_at};
    use crate::commands::spec_events::write::record;

    /// Uma tarefa gravada pela rodada, como a que nasce de uma sobra de onda.
    fn task_by_round(root: &std::path::Path, body: Value) -> u64 {
        record(root, "x", "task", body.as_object().cloned().unwrap(), PhaseWriter::Binary).unwrap().written.id
    }

    /// A linha do backlog lido agora que traz a tarefa de código `code`.
    fn backlog_line(root: &std::path::Path, code: &str) -> Value {
        let opts = ReadOpts { root: root.to_path_buf(), spec: Some("x".into()), block: "backlog".into(), term: None };
        let read: Value = serde_json::from_str(&read_at(&opts).unwrap()).unwrap();
        read["events"].as_array().into_iter().flatten().find(|line| line["code"] == json!(code)).cloned().unwrap_or(Value::Null)
    }

    /// Só a tarefa presa sem arquivo no backlog: a rodada não despacha nada e
    /// diz que ela espera arquivos e dependências, sem chamá-la de presa, e a
    /// leitura do backlog a marca como à espera de plano. Uma tarefa num
    /// arquivo TypeScript que depende dela entra depois: a rodada diz as duas
    /// coisas, cada tarefa na sua linha, e a leitura do backlog não marca a
    /// que já tem arquivo.
    #[test]
    fn the_round_and_the_backlog_say_a_task_without_files_waits_for_planning() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (_, crit) = backlog_project(root);
        let unplanned = task_by_round(
            root,
            json!({"title": "O disco enche", "text": "As cópias lotam o disco durante a obra.", "files": [],
                "depends_on": [], "author": "wave", "covers": [crit],
                "blocked_by": translate("round.leftover_without_files", Locale::PtBr)}),
        );
        let code = spec_now(root).codes()[&unplanned].clone();
        let stuck_opening = translate("round.backlog_stuck", Locale::PtBr).split("{tasks}").next().unwrap_or_default().to_string();

        let alone = round(root, "x", None);
        assert_eq!(alone["ok"], json!(true), "{alone}");
        assert_eq!(waves_in(&alone, "dispatch"), Vec::<u64>::new(), "a tarefa sem arquivo não sai: {alone}");
        assert!(alone.get("command").is_none(), "rodar de novo daria no mesmo: {alone}");
        let next = alone["next"].as_str().unwrap_or_default();
        let waits = translate("round.backlog_needs_planning", Locale::PtBr).replace("{tasks}", &code);
        assert!(next.ends_with(&waits), "{alone}");
        assert!(!next.contains(&stuck_opening), "a tarefa à espera de plano não sai como presa: {alone}");
        let line = backlog_line(root, &code);
        assert_eq!((&line["files"], &line["needs_planning"]), (&json!(0), &json!(true)), "{line}");

        std::fs::write(root.join("src/view.ts"), "export const view = 1;\n").unwrap();
        let after = task_by_round(
            root,
            json!({"title": "Tela do disco", "text": "Mostrar o disco na tela.", "files": [{"path": "src/view.ts"}],
                "depends_on": [unplanned], "author": "wave", "covers": [crit]}),
        );
        let after_code = spec_now(root).codes()[&after].clone();
        let both = round(root, "x", None);
        assert_eq!(waves_in(&both, "dispatch"), Vec::<u64>::new(), "{both}");
        let stuck = translate("round.backlog_stuck", Locale::PtBr).replace("{tasks}", &after_code);
        assert!(both["next"].as_str().unwrap_or_default().ends_with(&format!("{waits} {stuck}")), "{both}");
        assert_eq!(backlog_line(root, &after_code).get("needs_planning"), None, "a tarefa com arquivo não espera plano");
    }
}
