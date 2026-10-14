//! Cheap, read-only explanation of the declared queue. Source relationships
//! are checked at dispatch; observing the queue never scans or calls a model.
use super::{leftovers::is_cleanup, planning::needs_planning, queue};
use crate::shared::dag::files_cross;
use mustard_core::domain::{spec_events::SpecLog, spec_state::State};
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

/// Quantas tarefas o quadro do painel mostra; o resto sai só contado.
const SHOWN: usize = 40;

/// Os motivos da tarefa pronta — sem dependência aberta, sem trava e com
/// item coberto — que ficou na fila: sem vaga, com arquivo reservado por
/// onda aberta, limpeza à espera das outras, ou pronta e fora da montagem.
const HELD: [&str; 4] = ["capacity", "file-reservation", "cleanup-last", "ready"];

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
/// `ready`; as dependências ainda abertas; e, por onda aberta que reserva
/// arquivo dela, os arquivos dela que cruzam com os da onda.
struct Row {
    id: u64,
    code: String,
    reason: &'static str,
    dependencies: Vec<String>,
    holders: BTreeMap<u64, Vec<String>>,
}

fn queue(root: &Path, log: &SpecLog) -> Queue {
    let running = queue::waves_in_progress(log);
    let done = queue::waves_done(log, &running);
    let population = queue::backlog_population(log, &done);
    let left = queue::backlog_left(log);
    let uncovered = queue::backlog_uncovered(log);
    let codes = log.codes();
    let graph = crate::commands::wave::wave_overlap_check::wave_graph(log);
    let open = queue::open_sends(log);
    let capacity = queue::max_parallel(root);
    let declared = mustard_core::ProjectConfig::load(root).max_compiling_waves().is_some();
    let free = capacity.saturating_sub(open.len());
    let active = State::from_log(log).phase.is_some_and(super::can_run);
    let code = |id: u64| codes.get(&id).cloned().unwrap_or_else(|| id.to_string());
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
            .collect();
        let holders: BTreeMap<u64, Vec<String>> = open
            .keys()
            .filter_map(|wave| {
                let reserved = graph.files.get(wave)?;
                let crossed: Vec<String> =
                    entry.files.iter().filter(|file| reserved.iter().any(|other| files_cross(file, other))).cloned().collect();
                (!crossed.is_empty()).then_some((*wave, crossed))
            })
            .collect();
        let cleanup_wait = is_cleanup(task)
            && population
                .iter()
                .any(|other| !other.done && log.get(other.id).is_some_and(|e| !is_cleanup(e)));
        let reason = if !active {
            "spec-not-running"
        } else if needs_planning(task) {
            // A tarefa presa sem arquivo nenhum, como a que nasce da sobra
            // que não cita arquivo: espera quem conduz a obra planejá-la.
            "needs-planning"
        } else if entry.blocked {
            "task-blocked"
        } else if uncovered.contains(id) {
            "missing-criteria"
        } else if !dependencies.is_empty() {
            "dependencies"
        } else if !holders.is_empty() {
            "file-reservation"
        } else if cleanup_wait {
            "cleanup-last"
        } else if free == 0 {
            "capacity"
        } else {
            "ready"
        };
        rows.push(Row { id: *id, code: code(*id), reason, dependencies, holders });
    }
    Queue { backlog: left.len(), capacity, declared, occupied: open.keys().copied().collect(), free, rows }
}

pub(crate) fn snapshot(root: &Path, log: &SpecLog) -> Value {
    let queue = queue(root, log);
    let ready = queue.rows.iter().filter(|row| row.reason == "ready").count();
    let tasks: Vec<Value> = queue
        .rows
        .iter()
        .take(SHOWN)
        .map(|row| {
            let task = log.get(row.id);
            json!({"task":row.code,"title":task.and_then(|task| task.str_field("title")),"reason":row.reason,
            "dependencies":row.dependencies,"holding_waves":row.holders.keys().collect::<Vec<_>>(),
            "blocked_by":task.and_then(|task| task.str_field("blocked_by"))})
        })
        .collect();
    json!({"backlog":queue.backlog,"ready":ready,"blocked":queue.backlog.saturating_sub(ready),
        "capacity":queue.capacity,"occupied_wave_slots":queue.occupied.len(),"free_wave_slots":queue.free,"tasks":tasks,
        "omitted_tasks":queue.backlog.saturating_sub(SHOWN),"basis":"declared-queue; source relationships verified at dispatch"})
}

/// As tarefas prontas que ficaram na fila, cada uma com o motivo e o
/// detalhe dele (`tasks`, o `held` da resposta da rodada), e a linha que o
/// próximo passo diz delas (`line`).
pub(super) struct Held {
    pub(super) tasks: Vec<Value>,
    pub(super) line: String,
}

/// O que segura cada tarefa pronta que a rodada não despachou, lido da fila
/// declarada na spec de depois dos envios da rodada: as vagas que eles
/// ocuparam já contam. Sem vaga, o detalhe diz o limite de ondas compilando
/// ao mesmo tempo, de onde vem o número — o `maxCompilingWaves` do
/// `mustard.json` ou o padrão — e as ondas que ocupam as vagas. Com arquivo
/// reservado, diz o arquivo e a onda. `None` quando nenhuma tarefa pronta
/// ficou.
pub(super) fn held(root: &Path, log: &SpecLog, lang: Locale) -> Option<Held> {
    let queue = queue(root, log);
    let rows: Vec<&Row> = queue.rows.iter().filter(|row| HELD.contains(&row.reason)).collect();
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
            "file-reservation" => row
                .holders
                .iter()
                .map(|(wave, files)| {
                    let files: Vec<String> = files.iter().map(|file| format!("`{file}`")).collect();
                    translate("round.queued.file", lang).replace("{files}", &files.join(", ")).replace("{wave}", &wave.to_string())
                })
                .collect::<Vec<_>>()
                .join("; "),
            "cleanup-last" => translate("round.queued.cleanup", lang).to_string(),
            _ => translate("round.queued.assembly", lang).to_string(),
        }
    };
    let tasks = rows.iter().map(|row| json!({"task": row.code, "reason": row.reason, "detail": detail(row)})).collect();
    let named = |keep: &dyn Fn(&str) -> bool| rows.iter().filter(|row| keep(row.reason)).map(|row| row.code.as_str()).collect::<Vec<_>>().join(", ");
    let mut line = Vec::new();
    let waiting = named(&|reason| reason == "capacity");
    if !waiting.is_empty() {
        line.push(translate("round.queued.line_capacity", lang).replace("{tasks}", &waiting).replace("{detail}", &slots));
    }
    for row in rows.iter().filter(|row| row.reason == "file-reservation") {
        line.push(translate("round.queued.line_file", lang).replace("{task}", &row.code).replace("{detail}", &detail(row)));
    }
    let other = named(&|reason| !["capacity", "file-reservation"].contains(&reason));
    if !other.is_empty() {
        line.push(translate("round.queued.line_other", lang).replace("{tasks}", &other));
    }
    Some(Held { tasks, line: line.join(" ") })
}

#[cfg(test)]
mod tests {
    use super::super::{
        backlog::dispatch_backlog,
        queue::{backlog_project, backlog_task_on, seed_running, spec_now},
        tests::{round, waves_in},
    };
    use super::*;
    #[test]
    fn panel_explains_reservations_and_capacity_without_scanning_or_writing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        backlog_task_on(root, said, crit, "Change running file.", &["src/a.rs"]);
        let log = spec_now(root);
        dispatch_backlog(root, "x", &log, &log, 4, None).unwrap();
        seed_running(root, 1);
        backlog_task_on(root, said, crit, "Change same file.", &["src/a.rs"]);
        backlog_task_on(root, said, crit, "Change independent file.", &["src/b.rs"]);
        let log = spec_now(root);
        let before = format!("{log:?}");
        let view = snapshot(root, &log);
        assert_eq!(view["backlog"], 2);
        assert_eq!(view["ready"], 1);
        assert_eq!(view["tasks"][0]["reason"], "file-reservation");
        assert_eq!(view["tasks"][1]["reason"], "ready");
        assert_eq!(view["occupied_wave_slots"], 1);
        assert_eq!(view["free_wave_slots"], 3);
        assert_eq!(format!("{:?}", spec_now(root)), before);
        std::fs::write(root.join("mustard.json"), r#"{"maxCompilingWaves":1}"#).unwrap();
        let full = snapshot(root, &log);
        assert_eq!(full["free_wave_slots"], 0);
        assert_eq!(full["tasks"][1]["reason"], "capacity");
        assert!(!mustard_core::io::project_map::model_path(root).exists());
    }

    /// Três vagas, uma delas com a onda que mudou `web/app.ts`. Cinco
    /// tarefas prontas: quatro sem arquivo em comum e uma no mesmo arquivo da
    /// onda. A rodada solta duas, nas duas vagas livres, e a resposta diz por
    /// que cada uma das outras três ficou: duas sem vaga, com o limite de
    /// três e de onde ele vem, e a do arquivo, com o arquivo e a onda.
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
            [(free[2], "capacity"), (free[3], "capacity"), (same, "file-reservation")].map(|(id, reason)| (codes[&id].clone(), reason.to_string())).into();
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
        assert_eq!((detail(0), detail(1)), (slots.clone(), slots), "the limit, its source and the occupied waves: {out}");
        let file = translate("round.queued.file", lang).replace("{files}", "`web/app.ts`").replace("{wave}", "1");
        assert_eq!(detail(2), file, "the file and the wave holding it: {out}");
        let next = out["next"].as_str().unwrap_or_default();
        assert!(next.contains(&detail(0)) && next.contains(&codes[&free[2]]) && next.contains(&codes[&free[3]]), "{next}");
        assert!(next.contains(&detail(2)) && next.contains(&codes[&same]), "{next}");
    }
}
