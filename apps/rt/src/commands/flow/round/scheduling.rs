//! Cheap, read-only explanation of the declared queue. Source relationships
//! are checked at dispatch; observing the queue never scans or calls a model.
use super::{
    backlog::{Hold, reserving_waves},
    leftovers::is_cleanup,
    planning::needs_planning,
    queue,
};
use crate::shared::dag::files_cross;
use mustard_core::domain::{spec_events::SpecLog, spec_state::State};
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

/// Quantas tarefas o quadro do painel mostra; o resto sai só contado.
const SHOWN: usize = 40;

/// Os motivos da tarefa pronta — sem dependência aberta, sem trava e com
/// item coberto — que ficou na fila: sem vaga, com arquivo reservado por
/// onda aberta, limpeza à espera das outras, ou pronta e fora da montagem —
/// esta com o motivo exato que a montagem devolveu, quando ela o deu.
const HELD: [&str; 4] = ["capacity", "file-reservation", "cleanup-last", "ready"];

/// Os motivos exatos da pronta com vaga que a montagem deixou de fora: a
/// relação de leitura e escrita do scan com uma onda, o julgamento do Jev, a
/// reserva atrás de outra tarefa que ficou e a chegada ao backlog nesta
/// rodada.
const EXACT: [&str; 4] = ["read-write-relation", "judged-clash", "behind-held-task", "arrived-this-round"];

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
/// `ready`; as dependências ainda abertas; e, por onda do plano que reserva
/// arquivo dela ([`reserving_waves`]), com pedido aberto ou não, os arquivos
/// dela que cruzam com os da onda.
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
    let reserving = reserving_waves(log);
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
        let holders: BTreeMap<u64, Vec<String>> = reserving
            .iter()
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
/// reservado, diz o arquivo e a onda. A pronta com vaga que a montagem
/// deixou de fora sai com o motivo exato que ela devolveu (`assembled`, pelo
/// código da tarefa): a relação de leitura e escrita do scan com uma onda, o
/// julgamento do Jev, a reserva atrás de outra tarefa ou a chegada nesta
/// rodada. `None` quando nenhuma tarefa pronta ficou.
pub(super) fn held(root: &Path, log: &SpecLog, lang: Locale, assembled: &BTreeMap<String, Hold>) -> Option<Held> {
    let mut queue = queue(root, log);
    for row in queue.rows.iter_mut().filter(|row| row.reason == "ready") {
        if let Some(hold) = assembled.get(&row.code) {
            row.reason = EXACT[match hold {
                Hold::Contract(_) => 0,
                Hold::Judged(_) => 1,
                Hold::Behind(..) => 2,
                Hold::Arrived => 3,
            }];
        }
    }
    let rows: Vec<&Row> = queue.rows.iter().filter(|row| HELD.contains(&row.reason) || EXACT.contains(&row.reason)).collect();
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
    for row in rows.iter().filter(|row| row.reason == "file-reservation") {
        line.push(translate("round.queued.line_file", lang).replace("{task}", &row.code).replace("{detail}", &detail(row)));
    }
    for row in rows.iter().filter(|row| EXACT.contains(&row.reason)) {
        line.push(translate("round.queued.line_held", lang).replace("{task}", &row.code).replace("{detail}", &detail(row)));
    }
    let other = named(&|reason| ["cleanup-last", "ready"].contains(&reason));
    if !other.is_empty() {
        line.push(translate("round.queued.line_other", lang).replace("{tasks}", &other));
    }
    Some(Held { tasks, line: line.join(" ") })
}

/// O detalhe do motivo exato que a montagem deu à tarefa pronta que ficou.
fn exact(hold: &Hold, lang: Locale) -> String {
    let listed = |files: &mut dyn Iterator<Item = &String>| files.map(|file| format!("`{file}`")).collect::<Vec<_>>().join(", ");
    match hold {
        Hold::Contract(waves) => waves
            .iter()
            .flat_map(|(wave, relation)| {
                [("round.queued.reads", &relation.reads), ("round.queued.read_by", &relation.read_by)]
                    .into_iter()
                    .filter(|(_, files)| !files.is_empty())
                    .map(move |(key, files)| translate(key, lang).replace("{files}", &listed(&mut files.iter())).replace("{wave}", &wave.to_string()))
            })
            .collect::<Vec<_>>()
            .join("; "),
        Hold::Judged(chance) => translate("round.queued.judged", lang).replace("{chance}", &chance.to_string()),
        Hold::Behind(task, files) if files.is_empty() => translate("round.queued.behind_lot", lang).replace("{task}", task),
        Hold::Behind(task, files) => translate("round.queued.behind", lang).replace("{task}", task).replace("{files}", &listed(&mut files.iter())),
        Hold::Arrived => translate("round.queued.arrived", lang).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        backlog::dispatch_backlog,
        queue::{backlog_project, backlog_task_on, seed_running, spec_now},
        tests::{approved_with, id_of, round, waves_in, write},
    };
    use super::*;
    #[test]
    fn panel_explains_reservations_and_capacity_without_scanning_or_writing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        backlog_task_on(root, said, crit, "Change running file.", &["src/a.rs"]);
        let log = spec_now(root);
        dispatch_backlog(root, "x", &log, &log, 4, None).map(|(formed, _)| formed).unwrap();
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

    /// Num projeto TypeScript, a onda 1 muda o contrato `src/contract.ts`.
    /// Pelo mapa do scan de verdade, a tarefa do consumidor lê esse contrato e
    /// fica, com vaga livre; a que divide `src/shared.ts` com ela fica
    /// reservada atrás dela. A resposta da rodada diz o motivo exato de cada
    /// uma, e o próximo passo o repete.
    #[test]
    fn a_ready_task_reading_a_contract_a_running_wave_changes_is_held_with_that_reason() {
        let scan = mustard_core::Scan::locate();
        assert!(
            scan.is_compiled_alongside(),
            "o teste precisa do scan compilado junto com ele: rode `cargo build -p scan` antes de `cargo test -p mustard-rt`"
        );
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        for (file, text) in [
            ("src/contract.ts", "export interface Order { id: number }\n"),
            ("src/consumer.ts", "import { Order } from './contract';\nexport function total(order: Order): number { return order.id; }\n"),
            ("src/shared.ts", "export const label = 'total';\n"),
        ] {
            std::fs::write(root.join(file), text).unwrap();
        }
        backlog_task_on(root, said, crit, "Change the contract.", &["src/contract.ts"]);
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
        scan.scan(root, &mustard_core::io::project_map::model_path(root)).expect("the scan writes the map");
        let consumer = backlog_task_on(root, said, crit, "Change the consumer.", &["src/consumer.ts", "src/shared.ts"]);
        let neighbour = backlog_task_on(root, said, crit, "Change the label.", &["src/shared.ts"]);

        let out = round(root, "x", None);
        assert!(waves_in(&out, "dispatch").is_empty(), "{out}");
        let codes = spec_now(root).codes();
        let lang = crate::commands::spec_events::project(root).lang;
        let reads = translate("round.queued.reads", lang).replace("{files}", "`src/contract.ts`").replace("{wave}", "1");
        let behind = translate("round.queued.behind", lang).replace("{task}", &codes[&consumer]).replace("{files}", "`src/shared.ts`");
        let held: Vec<[String; 3]> = out["held"]
            .as_array()
            .unwrap_or_else(|| panic!("no held list: {out}"))
            .iter()
            .map(|row| ["task", "reason", "detail"].map(|key| row[key].as_str().unwrap_or_default().to_string()))
            .collect();
        let expected = vec![
            [codes[&consumer].clone(), "read-write-relation".to_string(), reads.clone()],
            [codes[&neighbour].clone(), "behind-held-task".to_string(), behind.clone()],
        ];
        assert_eq!(held, expected, "{out}");
        let next = out["next"].as_str().unwrap_or_default();
        assert!(next.contains(&reads) && next.contains(&behind), "{next}");
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
        let clashing = backlog_task_on(root, said, crit, "Change the parser.", &["src/a.rs"]);
        let on_entry = spec_now(root);
        let late = backlog_task_on(root, said, crit, "Change the menu.", &["src/b.rs"]);
        let locked = spec_now(root);
        let judge = |board: &Board| -> Result<Judged, mustard_core::domain::map_filter::FilterError> {
            let clash = |id: u64| if id == clashing { 0.8 } else { 0.0 };
            let tasks = board.backlog.iter().map(|task| (task.id, Judgement { kind: TaskKind::Feature, confidence: 0.9, clash: clash(task.id), size: 0.5 }));
            Ok(Judged { tasks: tasks.collect(), usage: mustard_core::domain::map_filter::FilterUsage::default() })
        };
        let (formed, held) = dispatch_backlog(root, "x", &on_entry, &locked, 4, Some(&judge)).unwrap();
        assert!(formed.is_empty(), "{formed:?}");
        let codes = locked.codes();
        assert_eq!(held, BTreeMap::from([(codes[&clashing].clone(), Hold::Judged(80)), (codes[&late].clone(), Hold::Arrived)]));
    }

    /// A onda 2 do plano espera a 1 e muda `web/app.ts`; a tarefa do backlog
    /// no mesmo arquivo não sai, porque a montagem reserva os arquivos de toda
    /// onda do plano aberta. O quadro diz a reserva com a onda 2, mesmo sem
    /// pedido dela, e não a mostra pronta.
    #[test]
    fn a_planned_wave_waiting_for_its_dependency_holds_its_file_on_the_board() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut task = 0;
        approved_with(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["web/app.ts"], &[1])], |said| {
            let log = spec_now(root);
            let crit = log.visible().into_iter().find(|e| e.event_type == "criterion").map(|e| e.id).unwrap();
            let body = json!({"text": "Change the page.", "files": [{"path": "web/app.ts"}], "depends_on": [], "covers": [crit], "origin": said});
            task = id_of(&write(root, "x", "task", body));
        });
        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        let log = spec_now(root);
        let code = log.codes()[&task].clone();
        let view = snapshot(root, &log);
        let row = view["tasks"].as_array().unwrap().iter().find(|row| row["task"] == json!(code)).cloned().unwrap_or_default();
        assert_eq!((row["reason"].clone(), row["holding_waves"].clone()), (json!("file-reservation"), json!([2])), "{view}");
    }
}
