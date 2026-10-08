//! As tarefas com que uma onda saiu: as que o pedido que a despachou por
//! último levava, cada uma na versão vigente. A volta da onda é conferida
//! contra elas, e não contra a marca de onda de hoje: a tarefa regravada
//! depois do envio — sem a onda, ou com outro texto — continua valendo como
//! tarefa daquela volta, e a ordem em que quem conduz regrava a tarefa e roda
//! a rodada não decide se a volta é assumida. A que outra onda levou também
//! conta, e fica com a onda que a levou.

use mustard_core::domain::spec_events::{SpecEvent, SpecLog};
use serde_json::Value;

/// As tarefas da onda `wave` como ela saiu: cada tarefa que os itens
/// (`items`) do envio que a despachou por último levavam marcada com a onda,
/// na versão vigente, sem repetir. A tarefa tirada da spec depois do envio
/// não conta. O envio gravado antes de guardar os itens, e a onda sem envio,
/// ficam com as tarefas que hoje levam a marca da onda.
pub(super) fn sent_tasks(log: &SpecLog, wave: u64) -> Vec<&SpecEvent> {
    let sent = log.last_dispatch_by_wave().get(&wave).and_then(|id| log.get(*id));
    let Some(items) = sent.and_then(|send| send.fields.get("items")).and_then(Value::as_array) else {
        return log.visible().into_iter().filter(|e| e.event_type == "task" && e.wave() == Some(wave)).collect();
    };
    let mut tasks: Vec<&SpecEvent> = Vec::new();
    let left_with = items.iter().filter_map(Value::as_u64).filter_map(|id| log.get(id));
    for task in left_with.filter(|item| item.event_type == "task" && item.wave() == Some(wave)) {
        if let Some(now) = log.current(task.id)
            && !tasks.iter().any(|had| had.id == now.id)
        {
            tasks.push(now);
        }
    }
    tasks
}

#[cfg(test)]
mod tests {
    use mustard_core::io::spec_events as store;
    use mustard_core::platform::i18n::{Locale, translate};
    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::super::report::backlog_return;
    use super::super::stops::replan_code;
    use super::super::tests::{
        DECISION, QUESTION, approved, approved_with, change_asked, click, delivered, id_of, request_at, returned, round, waves_in, write,
    };

    /// A tarefa que a volta deixou por fazer e que quem conduz regravou sem a
    /// onda antes de a rodada assumir a volta continua valendo como tarefa
    /// daquela volta: a rodada assume a volta em vez de recusá-la, a entrega
    /// oficial guarda a tarefa por fazer, e o pedido que leva a tarefa adiante
    /// abre com a linha do resumo da volta.
    #[test]
    fn a_task_rewritten_after_the_return_still_counts_as_left_undone_by_it() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let sent = round(root, "x", None);
        assert_eq!(waves_in(&sent, "dispatch"), vec![1], "{sent}");
        let spec = || store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let log = spec();
        let task = log.visible().into_iter().find(|e| e.event_type == "task" && e.wave() == Some(1)).cloned().unwrap();
        let code = log.codes()[&task.id].clone();

        let wrote = returned(root, json!({"wave": 1, "text": "Parei no limite.", "undone": [&code]}));
        assert_eq!(wrote["ok"], json!(true), "{wrote}");
        let criterion = log.visible().into_iter().find(|e| e.event_type == "criterion").map(|e| e.id).unwrap();
        let mut rewritten = backlog_return(&task);
        rewritten.insert("text".into(), json!("Tarefa da onda 1, reescrita depois da volta."));
        rewritten.insert("covers".into(), json!([criterion]));
        let rewrote = write(root, "x", "task", Value::Object(rewritten));
        assert_eq!(rewrote["ok"], json!(true), "{rewrote}");

        let took = round(root, "x", None);
        assert_eq!(took["ok"], json!(true), "{took}");
        let refused = took["warnings"].as_array().into_iter().flatten().any(|w| w["reason"] == json!("undone-not-in-wave"));
        assert!(!refused, "the return is taken, not held: {took}");
        let log = spec();
        assert!(!log.visible().iter().any(|e| e.event_type == "delivered" && e.wave() == Some(1)), "retired wave must not integrate: {took}");
        assert!(took.get("commit").is_none(), "no retired commit: {took}");
        let report = log.unassumed_returns().into_iter().find(|e| e.wave() == Some(1)).unwrap();
        assert_eq!(report.fields["undone"], json!([&code]));
        let summary = log.codes()[&report.id].clone();

        assert_eq!(waves_in(&took, "dispatch"), vec![2], "{took}");
        let prompt = request_at(&took, 0);
        let read = translate("wave_prompt.summary.report_read", Locale::PtBr).replace("{code}", &summary);
        let opening = read.split("{root}").next().unwrap_or_default();
        assert!(prompt.contains(opening), "the request opens with the summary line: {prompt}");
        assert!(prompt.contains(&code), "the request carries the rewritten task: {prompt}");
    }

    /// A tarefa que a volta deixou por fazer e que, enquanto a volta esperava
    /// o clique do usuário, outra onda levou e entregou fica com essa onda: a
    /// rodada assume a volta sem aviso, a tarefa segue na outra onda na mesma
    /// versão, e nenhuma onda nova sai, nem para ela nem para o item que a
    /// volta não cumpriu, que o pedido da outra onda levou.
    #[test]
    fn a_task_another_wave_took_stays_with_it_when_the_old_return_is_taken() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let decision = std::cell::Cell::new(0);
        approved_with(root, "x", &[(1, &["src/a.rs"], &[])], |said| {
            let body = json!({"text": "A soma arredonda para baixo.", "why": "w", "waves": [1], "keys": ["k"],
                "origin": said});
            decision.set(id_of(&write(root, "x", "decision", body)));
        });
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1]);
        let spec = || store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let log = spec();
        let task = log.visible().into_iter().find(|e| e.event_type == "task" && e.wave() == Some(1)).cloned().unwrap();
        let (code, item) = (log.codes()[&task.id].clone(), log.codes()[&decision.get()].clone());
        let criterion = log.visible().into_iter().find(|e| e.event_type == "criterion").map(|e| e.id).unwrap();

        let change = "A tarefa da onda 1 vai para outra onda.";
        let unmet = json!([{"item": &item, "met": false, "text": "Falta arredondar para baixo."}]);
        let body = json!({"wave": 1, "text": "Parei no limite.", "replan": change, "changes_decision": DECISION,
            "undone": [&code], "agreed": unmet});
        assert_eq!(returned(root, body)["ok"], json!(true));
        let held = round(root, "x", None);
        assert_eq!(change_asked(&held)["wave"], json!(1), "{held}");

        let wave = json!({"n": 2, "text": "Onda 2.", "criteria": [criterion], "done_when": "A suíte passa.",
            "origin": task.fields["origin"]});
        assert_eq!(write(root, "x", "wave", wave)["ok"], json!(true));
        let mut moved = backlog_return(&task);
        moved.insert("wave".into(), json!(2));
        moved.insert("covers".into(), json!([decision.get()]));
        let moved = id_of(&write(root, "x", "task", Value::Object(moved)));
        let second = round(root, "x", None);
        assert_eq!(waves_in(&second, "dispatch"), vec![2], "{second}");
        delivered(root, 2, "A onda 2 saiu.", &["src/a.rs"]);
        assert_eq!(round(root, "x", None)["ok"], json!(true));

        let session = "s-tarefa-levada";
        crate::shared::context::session::bind_session_spec(&root.to_string_lossy(), session, "x");
        click(root, session, QUESTION, &replan_code(1, change), "Aceitar");
        let took = round(root, "x", None);
        assert_eq!(took["ok"], json!(true), "{took}");
        let warned = took["warnings"].as_array().into_iter().flatten().filter_map(|w| w["reason"].as_str());
        let warned: Vec<&str> = warned.filter(|r| ["undone-not-in-wave", "tasks-returned"].contains(r)).collect();
        assert!(warned.is_empty(), "the return is taken without a warning: {took}");
        let log = spec();
        assert!(!log.visible().iter().any(|e| e.event_type == "delivered" && e.wave() == Some(1)), "retired report must not integrate: {took}");
        assert!(log.unassumed_returns().iter().any(|e| e.wave() == Some(1)), "history remains");
        assert!(!log.planned_waves().contains(&1));
        let now = log.current(task.id).unwrap();
        assert_eq!((now.id, now.wave()), (moved, Some(2)), "the task stays with wave 2, same version: {took}");
        assert_eq!(waves_in(&took, "dispatch"), Vec::<u64>::new(), "no new wave: {took}");
        let backlog: Vec<_> = log.visible().into_iter().filter(|e| e.event_type == "task" && e.wave().is_none()).collect();
        assert!(backlog.is_empty(), "nothing goes back to the backlog: {backlog:?}");
    }
}
