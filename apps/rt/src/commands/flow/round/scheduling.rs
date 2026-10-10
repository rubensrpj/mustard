//! Cheap, read-only explanation of the declared queue. Source relationships
//! are checked at dispatch; observing the queue never scans or calls a model.
use super::{leftovers::is_cleanup, queue};
use mustard_core::domain::{spec_events::SpecLog, spec_state::State};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path};

pub(crate) fn snapshot(root: &Path, log: &SpecLog) -> Value {
    let running = queue::waves_in_progress(log);
    let done = queue::waves_done(log, &running);
    let population = queue::backlog_population(log, &done);
    let left = queue::backlog_left(log);
    let uncovered = queue::backlog_uncovered(log);
    let codes = log.codes();
    let graph = crate::commands::wave::wave_overlap_check::wave_graph(log);
    let open = queue::open_sends(log);
    let capacity = queue::max_parallel(root);
    let free = capacity.saturating_sub(open.len());
    let active = State::from_log(log).phase.is_some_and(super::can_run);
    let code = |id: u64| codes.get(&id).cloned().unwrap_or_else(|| id.to_string());
    let mut ready = 0;
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
        let holders: BTreeSet<_> = open
            .keys()
            .filter(|wave| {
                graph
                    .files
                    .get(wave)
                    .is_some_and(|files| crate::shared::dag::sets_cross(&entry.files, files))
            })
            .copied()
            .collect();
        let cleanup_wait = is_cleanup(task)
            && population
                .iter()
                .any(|other| !other.done && log.get(other.id).is_some_and(|e| !is_cleanup(e)));
        let reason = if !active {
            "spec-not-running"
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
        if reason == "ready" {
            ready += 1;
        }
        if rows.len() < 40 {
            rows.push(
                json!({"task":code(*id),"title":task.str_field("title"),"reason":reason,
            "dependencies":dependencies,"holding_waves":holders}),
            );
        }
    }
    json!({"backlog":left.len(),"ready":ready,"blocked":left.len().saturating_sub(ready),
        "capacity":capacity,"occupied_wave_slots":open.len(),"free_wave_slots":free,"tasks":rows,
        "omitted_tasks":left.len().saturating_sub(40),"basis":"declared-queue; source relationships verified at dispatch"})
}

#[cfg(test)]
mod tests {
    use super::super::{
        backlog::dispatch_backlog,
        queue::{backlog_project, backlog_task_on, seed_running, spec_now},
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
}
