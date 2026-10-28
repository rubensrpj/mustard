//! Cheap, read-only explanation of the declared queue. Source relationships
//! are checked at dispatch; observing the queue never scans or calls a model.
use super::{backlog::Hold, planning::needs_planning, queue};
use mustard_core::domain::{spec_events::SpecLog, spec_state::{State, can_run}};
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

/// Quantas tarefas o quadro do painel mostra; o resto sai só contado.
const SHOWN: usize = 40;

/// Os motivos da tarefa pronta — sem dependência aberta, sem trava e com
/// item coberto — que ficou na fila: sem vaga, ou pronta e fora da montagem
/// — esta com o motivo exato que a montagem devolveu, quando ela o deu.
/// Nenhuma fica pelo arquivo de uma onda aberta — cada onda trabalha na
/// própria cópia — nem por ser limpeza; só a avaliação de uma skill espera as
/// tarefas abertas nos arquivos que a skill cita, com o motivo exato.
const HELD: [&str; 2] = ["capacity", "ready"];

/// O motivo da tarefa presa por uma dependência que não aponta tarefa nenhuma
/// da spec: segura só ela, com o nome da dependência no detalhe.
const BROKEN: &str = "broken-dependency";

/// Os motivos exatos da pronta com vaga que a montagem deixou de fora: o
/// julgamento do Jev, a chegada ao backlog nesta rodada, a decisão do Jev
/// que ela ainda espera, a onda do mesmo sentido em andamento e, na
/// avaliação de uma skill, as tarefas abertas nos arquivos que a skill cita.
const EXACT: [&str; 5] = ["judged-clash", "arrived-this-round", "jev-pending", "following-running-wave", "skill-waits-open-tasks"];

/// A fila declarada: as vagas do projeto, se o número vem do `mustard.json`
/// (`declared`), as ondas com pedido aberto que ocupam as vagas e cada
/// tarefa do backlog.
struct Queue {
    backlog: usize,
    capacity: usize,
    declared: bool,
    occupied: Vec<u64>,
    free: usize,
    rows: Vec<Row>,
}

/// Uma tarefa do backlog na fila declarada: o motivo que a segura, ou
/// `ready`, as dependências ainda abertas e as quebradas (`broken`), as que
/// não apontam tarefa nenhuma da spec. `dependencies` traz também as
/// quebradas.
struct Row {
    id: u64,
    code: String,
    reason: &'static str,
    dependencies: Vec<String>,
    broken: Vec<String>,
}

fn queue(root: &Path, spec: &str, log: &SpecLog) -> Queue {
    let running = queue::waves_in_progress(log);
    let done = queue::waves_done(log, &running);
    let population = queue::backlog_population(log, &done);
    let left = queue::backlog_left(log);
    let uncovered = queue::backlog_uncovered(log);
    let codes = log.codes();
    // As vagas pela conta única da rodada: a onda que voltou guarda a cópia
    // sem ocupar vaga.
    let vacancies = super::vacancy::vacancies(root, spec, log);
    let (capacity, free) = (vacancies.capacity, vacancies.free());
    let declared = mustard_core::ProjectConfig::load(root).max_compiling_waves().is_some();
    let active = State::from_log(log).phase.is_some_and(can_run);
    let code = |id: u64| codes.get(&id).cloned().unwrap_or_else(|| id.to_string());
    let broken = crate::commands::spec_events::task_dependents::broken_dependencies(log);
    let mut rows = Vec::new();
    for id in &left {
        let Some(task) = log.get(*id) else {
            continue;
        };
        let Some(entry) = population.iter().find(|task| task.id == *id) else {
            continue;
        };
        let dependencies: Vec<_> = entry
            .depends_on
            .iter()
            .filter(|id| !population.iter().any(|t| t.id == **id && t.done))
            .map(|id| code(*id))
            .chain(broken.get(id).into_iter().flatten().cloned())
            .collect();
        let reason = if !active {
            "spec-not-running"
        } else if needs_planning(task) {
            // A tarefa presa sem arquivo nenhum, como a que nasce da sobra
            // que não cita arquivo: espera quem conduz a obra planejá-la.
            "needs-planning"
        } else if broken.contains_key(id) {
            BROKEN
        } else if entry.blocked {
            "task-blocked"
        } else if uncovered.contains(id) {
            "missing-criteria"
        } else if !dependencies.is_empty() {
            "dependencies"
        } else if free == 0 {
            "capacity"
        } else {
            "ready"
        };
        rows.push(Row { id: *id, code: code(*id), reason, dependencies, broken: broken.get(id).cloned().unwrap_or_default() });
    }
    Queue { backlog: left.len(), capacity, declared, occupied: vacancies.working.union(&vacancies.holding).copied().collect(), free, rows }
}

pub(crate) fn snapshot(root: &Path, spec: &str, log: &SpecLog) -> Value {
    let queue = queue(root, spec, log);
    let ready = queue.rows.iter().filter(|row| row.reason == "ready").count();
    let tasks: Vec<Value> = queue
        .rows
        .iter()
        .take(SHOWN)
        .map(|row| {
            let task = log.get(row.id);
            json!({"task":row.code,"title":task.and_then(|task| task.str_field("title")),"reason":row.reason,
            "dependencies":row.dependencies,"blocked_by":task.and_then(|task| task.str_field("blocked_by"))})
        })
        .collect();
    json!({"backlog":queue.backlog,"ready":ready,"blocked":queue.backlog.saturating_sub(ready),
        "capacity":queue.capacity,"occupied_wave_slots":queue.occupied.len(),"free_wave_slots":queue.free,"tasks":tasks,
        "omitted_tasks":queue.backlog.saturating_sub(SHOWN),"basis":"declared-queue; source relationships verified at dispatch"})
}

/// As tarefas que ficaram na fila, cada uma com o motivo e o detalhe dele
/// (`tasks`, o `held` da resposta da rodada), e a linha que o próximo passo
/// diz delas (`line`): as prontas e a presa por dependência quebrada.
pub(super) struct Held {
    pub(super) tasks: Vec<Value>,
    pub(super) line: String,
}

/// O que segura cada tarefa que a rodada não despachou, lido da fila
/// declarada na spec de depois dos envios da rodada: as vagas que eles
/// ocuparam já contam. Entram a tarefa pronta e a presa por uma dependência
/// que não aponta tarefa nenhuma da spec; o detalhe desta nomeia só a
/// dependência quebrada, sem as abertas e válidas. Sem vaga, o detalhe diz o
/// limite de ondas compilando ao mesmo tempo, de onde vem o número — o
/// `maxCompilingWaves` do `mustard.json` ou o padrão — e as ondas que ocupam
/// as vagas. A pronta com vaga que a montagem deixou de fora sai com o motivo
/// exato que ela devolveu (`assembled`, pelo código da tarefa): o julgamento
/// do Jev ou a chegada nesta rodada. `None` quando nenhuma tarefa ficou.
pub(super) fn held(root: &Path, spec: &str, log: &SpecLog, lang: Locale, assembled: &BTreeMap<String, Hold>) -> Option<Held> {
    let mut queue = queue(root, spec, log);
    for row in queue.rows.iter_mut().filter(|row| row.reason == "ready") {
        if let Some(hold) = assembled.get(&row.code) {
            row.reason = EXACT[match hold {
                Hold::Judged(_) => 0,
                Hold::Arrived => 1,
                Hold::JevPending => 2,
                Hold::Following(_) => 3,
                Hold::OpenTasks(_) => 4,
            }];
        }
    }
    let rows: Vec<&Row> = queue.rows.iter().filter(|row| HELD.contains(&row.reason) || EXACT.contains(&row.reason) || row.reason == BROKEN).collect();
    if rows.is_empty() {
        return None;
    }
    let origin = translate(if queue.declared { "round.queued.origin_config" } else { "round.queued.origin_default" }, lang);
    let waves: Vec<String> = queue.occupied.iter().map(u64::to_string).collect();
    // Sem onda nenhuma ocupando vaga, só o limite zero segura a fila.
    let slots = translate(if waves.is_empty() { "round.queued.no_slots" } else { "round.queued.capacity" }, lang)
        .replace("{limit}", &queue.capacity.to_string())
        .replace("{origin}", origin)
        .replace("{occupied}", &waves.len().to_string())
        .replace("{waves}", &waves.join(", "));
    let detail = |row: &Row| -> String {
        match row.reason {
            "capacity" => slots.clone(),
            BROKEN => translate("round.queued.broken_dependency", lang).replace("{dependencies}", &row.broken.join(", ")),
            _ => assembled.get(&row.code).map_or_else(|| translate("round.queued.assembly", lang).to_string(), |hold| exact(hold, lang)),
        }
    };
    let tasks = rows.iter().map(|row| json!({"task": row.code, "reason": row.reason, "detail": detail(row)})).collect();
    let named = |keep: &dyn Fn(&str) -> bool| rows.iter().filter(|row| keep(row.reason)).map(|row| row.code.as_str()).collect::<Vec<_>>().join(", ");
    let mut line = Vec::new();
    let waiting = named(&|reason| reason == "capacity");
    if !waiting.is_empty() {
        line.push(translate("round.queued.line_capacity", lang).replace("{tasks}", &waiting).replace("{detail}", &slots));
    }
    for row in rows.iter().filter(|row| EXACT.contains(&row.reason) || row.reason == BROKEN) {
        line.push(translate("round.queued.line_held", lang).replace("{task}", &row.code).replace("{detail}", &detail(row)));
    }
    let other = named(&|reason| reason == "ready");
    if !other.is_empty() {
        line.push(translate("round.queued.line_other", lang).replace("{tasks}", &other));
    }
    Some(Held { tasks, line: line.join(" ") })
}

/// As tarefas seguradas de [`held`] agrupadas: uma entrada por motivo e
/// detalhe, com o detalhe uma vez e os códigos das tarefas na ordem em que
/// vieram. Dezesseis tarefas sem vaga dão uma entrada só, com a frase da vaga
/// uma vez; duas julgadas pelo Jev com chances diferentes dão duas.
pub(crate) fn grouped(held: &[Value]) -> Vec<Value> {
    let mut groups: Vec<Value> = Vec::new();
    for row in held {
        let same = |group: &&mut Value| group["reason"] == row["reason"] && group["detail"] == row["detail"];
        match groups.iter_mut().find(same).and_then(|group| group["tasks"].as_array_mut()) {
            Some(tasks) => tasks.push(row["task"].clone()),
            None => groups.push(json!({"reason": row["reason"], "detail": row["detail"], "tasks": [row["task"]]})),
        }
    }
    groups
}

/// O detalhe do motivo exato que a montagem deu à tarefa pronta que ficou.
fn exact(hold: &Hold, lang: Locale) -> String {
    match hold {
        Hold::Judged(chance) => translate("round.queued.judged", lang).replace("{chance}", &chance.to_string()),
        Hold::Arrived => translate("round.queued.arrived", lang).to_string(),
        Hold::JevPending => translate("round.queued.jev_pending", lang).to_string(),
        Hold::Following(wave) => translate("round.queued.following", lang).replace("{wave}", &wave.to_string()),
        Hold::OpenTasks(tasks) => translate("round.queued.open_tasks", lang).replace("{tasks}", &tasks.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        backlog::dispatch_backlog,
        queue::{backlog_project, backlog_task_on, seed_running, spec_now},
        tests::{round, waves_in},
    };
    use super::*;
    /// A onda 1 roda com `src/a.rs`. A tarefa no mesmo arquivo e a de outro
    /// arquivo estão as duas prontas: o arquivo da onda em andamento não segura
    /// ninguém. Com uma vaga só, as duas esperam a vaga.
    #[test]
    fn panel_explains_capacity_without_scanning_or_writing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        backlog_task_on(root, said, crit, "Change running file.", &["src/a.rs"]);
        let log = spec_now(root);
        dispatch_backlog(root, "x", &log, &log, 4, (None, None)).map(|(formed, _)| formed).unwrap();
        seed_running(root, 1);
        backlog_task_on(root, said, crit, "Change same file.", &["src/a.rs"]);
        backlog_task_on(root, said, crit, "Change independent file.", &["src/b.rs"]);
        let log = spec_now(root);
        let before = format!("{log:?}");
        let view = snapshot(root, "x", &log);
        assert_eq!(view["backlog"], 2);
        assert_eq!(view["ready"], 2);
        assert_eq!(view["tasks"][0]["reason"], "ready");
        assert_eq!(view["tasks"][1]["reason"], "ready");
        assert_eq!(view["occupied_wave_slots"], 1);
        assert_eq!(view["free_wave_slots"], 3);
        assert_eq!(format!("{:?}", spec_now(root)), before);
        std::fs::write(root.join("mustard.json"), r#"{"maxCompilingWaves":1}"#).unwrap();
        let full = snapshot(root, "x", &log);
        assert_eq!(full["free_wave_slots"], 0);
        assert_eq!((full["tasks"][0]["reason"].clone(), full["tasks"][1]["reason"].clone()), (json!("capacity"), json!("capacity")));
        assert!(!mustard_core::io::project_map::model_path(root).exists());
    }

    /// Três vagas, uma delas com a onda que mudou `web/app.ts`. Cinco
    /// tarefas prontas: quatro sem arquivo em comum e uma no mesmo arquivo da
    /// onda. A rodada solta duas, nas duas vagas livres, e a resposta diz por
    /// que cada uma das outras três ficou: sem vaga, com o limite de três e de
    /// onde ele vem — a do arquivo da onda também, que nenhum arquivo segura.
    #[test]
    fn held_ready_tasks_are_listed_with_their_reason() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":3}"#).unwrap();
        backlog_task_on(root, said, crit, "Change the page.", &["web/app.ts"]);
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
        assert!(first.get("held").is_none(), "nothing ready stayed behind: {first}");
        let free: Vec<u64> = [("Change the parser.", "src/b.rs"), ("Change the menu.", "web/c.ts"), ("Change the form.", "web/d.ts"), ("Change the list.", "web/e.ts")]
            .into_iter()
            .map(|(text, file)| backlog_task_on(root, said, crit, text, &[file]))
            .collect();
        let same = backlog_task_on(root, said, crit, "Change the page again.", &["web/app.ts"]);

        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![2, 3], "{out}");
        let codes = spec_now(root).codes();
        let held: Vec<(String, String)> = out["held"]
            .as_array()
            .unwrap_or_else(|| panic!("no held list: {out}"))
            .iter()
            .map(|row| (row["task"].as_str().unwrap_or_default().to_string(), row["reason"].as_str().unwrap_or_default().to_string()))
            .collect();
        let expected: Vec<(String, String)> =
            [(free[2], "capacity"), (free[3], "capacity"), (same, "capacity")].map(|(id, reason)| (codes[&id].clone(), reason.to_string())).into();
        assert_eq!(held, expected, "{out}");
        let detail = |at: usize| out["held"][at]["detail"].as_str().unwrap_or_default().to_string();
        let lang = crate::commands::spec_events::project(root).lang;
        let origin = translate("round.queued.origin_config", lang);
        assert!(origin.contains("`maxCompilingWaves`") && origin.contains("`mustard.json`"), "{origin}");
        let slots = translate("round.queued.capacity", lang)
            .replace("{limit}", "3")
            .replace("{origin}", origin)
            .replace("{occupied}", "3")
            .replace("{waves}", "1, 2, 3");
        assert_eq!((detail(0), detail(1), detail(2)), (slots.clone(), slots.clone(), slots), "the limit, its source and the occupied waves: {out}");
        let next = out["next"].as_str().unwrap_or_default();
        assert!(next.contains(&detail(0)) && next.contains(&codes[&free[2]]) && next.contains(&codes[&free[3]]) && next.contains(&codes[&same]), "{next}");
    }

    /// Uma vaga só, ocupada pela onda 1, e dezesseis tarefas prontas num
    /// projeto TypeScript. A resposta inteira segura cada uma com a frase da
    /// vaga; a curta agrupa as dezesseis numa entrada, com a frase uma vez e
    /// os códigos na ordem.
    #[test]
    fn short_answer_names_the_full_slot_once_for_sixteen_held_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":1}"#).unwrap();
        backlog_task_on(root, said, crit, "Change the page.", &["web/app.ts"]);
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1]);
        let held: Vec<u64> = (0..16).map(|n| backlog_task_on(root, said, crit, &format!("Change screen {n}."), &[&format!("web/screen{n}.ts")])).collect();
        let out = round(root, "x", None);
        assert!(waves_in(&out, "dispatch").is_empty(), "{out}");
        let lang = crate::commands::spec_events::project(root).lang;
        let slots = translate("round.queued.capacity", lang)
            .replace("{limit}", "1")
            .replace("{origin}", translate("round.queued.origin_config", lang))
            .replace("{occupied}", "1")
            .replace("{waves}", "1");
        let whole = crate::commands::flow::projection("round", &out, true).unwrap();
        assert_eq!(whole["held"].to_string().matches(&slots).count(), 16, "{whole}");
        let short = crate::commands::flow::projection("round", &out, false).unwrap();
        let codes = spec_now(root).codes();
        let expected: Vec<Value> = held.iter().map(|id| json!(codes[id])).collect();
        assert_eq!(short["held"], json!([{"reason": "capacity", "detail": slots, "tasks": expected}]), "{short}");
        assert_eq!(short["held"].to_string().matches(&slots).count(), 1, "{short}");
        assert!(short.to_string().len() < whole.to_string().len(), "{short}");
    }

    /// O Jev julga que uma tarefa pronta muda o mesmo que uma onda em
    /// andamento: a montagem a deixa de fora com essa chance. A tarefa que
    /// chegou ao backlog depois da leitura de entrada da rodada fica para a
    /// seguinte, com esse motivo.
    #[test]
    fn the_assembly_names_the_jev_clash_and_the_task_that_arrived_this_round() {
        use crate::shared::{
            dag::{Judgement, TaskKind},
            judgement::{Board, Judged},
        };
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        // Três candidatas, que é o que faz a chamada paga sair sem nenhuma
        // onda em andamento para perguntar por: com duas ou menos, a montagem
        // não chama ninguém e sai pela regra de arquivos.
        for name in ["c.rs", "d.rs"] {
            std::fs::write(root.join("src").join(name), "fn one() {}\n").unwrap();
        }
        let clashing = backlog_task_on(root, said, crit, "Change the parser.", &["src/a.rs"]);
        let second = backlog_task_on(root, said, crit, "Change the menu.", &["src/b.rs"]);
        let third = backlog_task_on(root, said, crit, "Change the footer.", &["src/c.rs"]);
        let on_entry = spec_now(root);
        let late = backlog_task_on(root, said, crit, "Change the header.", &["src/d.rs"]);
        let locked = spec_now(root);
        let judge = |board: &Board| -> Result<Judged, mustard_core::domain::map_filter::FilterError> {
            let tasks = board.backlog.iter().map(|task| (task.id, Judgement { kind: TaskKind::Feature, confidence: 0.9, clash: 0.8 }));
            Ok(Judged { tasks: tasks.collect(), usage: mustard_core::domain::map_filter::FilterUsage::default() })
        };
        let (formed, held) = dispatch_backlog(root, "x", &on_entry, &locked, 4, (Some(&judge), None)).unwrap();
        assert!(formed.is_empty(), "{formed:?}");
        let codes = locked.codes();
        assert_eq!(
            held,
            BTreeMap::from([
                (codes[&clashing].clone(), Hold::Judged(80)),
                (codes[&second].clone(), Hold::Judged(80)),
                (codes[&third].clone(), Hold::Judged(80)),
                (codes[&late].clone(), Hold::Arrived),
            ])
        );
    }
}
