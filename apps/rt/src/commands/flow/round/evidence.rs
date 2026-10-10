//! Current scan imports are packing hints, never new semantic dependencies.
//! Both endpoints must match their scanned Git blobs. Missing or stale
//! evidence preserves native grouping by declared files.
use crate::shared::judgement::Board;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub(super) fn augment(root: &Path, board: &mut Board) {
    use mustard_core::io::project_map::{Need, blobs_of, model_path, read_for};
    let Ok(canonical) = root.canonicalize() else {
        return;
    };
    let root = canonical.as_path();
    let files: BTreeSet<_> = board
        .backlog
        .iter()
        .chain(board.running.iter().flat_map(|wave| &wave.tasks))
        .flat_map(|task| &task.files)
        .filter(|file| safe_file(root, file))
        .cloned()
        .collect();
    let mut imports = BTreeMap::new();
    let mut endpoints = files.clone();
    let requested: Vec<_> = files.iter().map(String::as_str).collect();
    let Ok(map) = read_for(root, Need::Imports(&requested)) else {
        return;
    };
    for module in &map.modules {
        let deps: Vec<_> = module
            .deps
            .iter()
            .filter(|dep| safe_file(root, dep))
            .cloned()
            .collect();
        endpoints.extend(deps.iter().cloned());
        imports.insert(module.path.clone(), deps);
    }
    if imports.is_empty() {
        return;
    }
    let paths: Vec<_> = endpoints.iter().map(String::as_str).collect();
    let Ok(stored) = blobs_of(&model_path(root), &paths) else {
        return;
    };
    let args: Vec<_> = ["hash-object", "--"]
        .into_iter()
        .chain(paths.iter().copied())
        .collect();
    let current = mustard_core::platform::git::run(root, &args);
    if !current.ok || current.stdout.lines().count() != paths.len() {
        return;
    }
    let valid: BTreeSet<_> = paths
        .into_iter()
        .zip(current.stdout.lines())
        .filter(|(file, hash)| stored.get(*file).is_some_and(|old| old == hash))
        .map(|(file, _)| file.to_string())
        .collect();
    for task in board
        .backlog
        .iter_mut()
        .chain(board.running.iter_mut().flat_map(|wave| &mut wave.tasks))
    {
        let reads: BTreeSet<_> = task
            .reads
            .iter()
            .cloned()
            .chain(
                task.files
                    .iter()
                    .filter(|file| valid.contains(*file))
                    .flat_map(|file| imports.get(file).into_iter().flatten())
                    .filter(|file| valid.contains(*file))
                    .cloned(),
            )
            .collect();
        task.reads = reads.into_iter().collect();
    }
}

fn safe_file(root: &Path, file: &str) -> bool {
    mustard_core::io::wave_prompt::local_file_inside(file)
        && !file.contains(['*', '?', '['])
        && root
            .join(file)
            .canonicalize()
            .is_ok_and(|path| path.starts_with(root) && path.is_file())
}

pub(super) fn links(board: &Board) -> BTreeSet<(u64, u64)> {
    let mut pairs = BTreeSet::new();
    for (at, left) in board.backlog.iter().enumerate() {
        for right in board.backlog.iter().skip(at + 1) {
            let relation = left.relation(right);
            // A global criterion alone does not identify a shared implementation.
            if relation.read_write || (relation.shared_read && relation.flow) {
                pairs.insert((left.id, right.id));
            }
        }
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::super::{
        backlog::dispatch_backlog,
        queue::{backlog_project, backlog_task_on, seed_running, spec_now, wave_order},
    };
    use super::*;
    use crate::shared::judgement::{BoardTask, BoardWave};
    use serde_json::json;

    fn task(id: u64, file: &str) -> BoardTask {
        BoardTask {
            id,
            title: format!("task {id}"),
            text: String::new(),
            agent: String::new(),
            files: vec![file.into()],
            depends_on: vec![],
            reads: vec![],
            criteria: BTreeSet::from([1]),
        }
    }
    fn scan_fixture(root: &Path) {
        use mustard_core::{
            domain::normalize::Languages,
            io::project_map::{model_path, save_at},
        };
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.rs"), "use crate::b; fn a() {}\n").unwrap();
        std::fs::write(root.join("src/b.rs"), "pub fn b() {}\n").unwrap();
        let hashes =
            mustard_core::platform::git::run(root, &["hash-object", "--", "src/a.rs", "src/b.rs"]);
        assert!(hashes.ok, "{}", hashes.stdout);
        let blobs: Vec<_> = hashes.stdout.lines().collect();
        save_at(
            &model_path(root),
            &json!({"root":root,"modules":[
            {"path":"src/a.rs","language":"rust","blob":blobs[0],"deps":["src/b.rs"]},
            {"path":"src/b.rs","language":"rust","blob":blobs[1],"deps":[]}]}),
            "source fixture",
            &Languages::new(["pt-BR", "en-US"]),
        )
        .unwrap();
    }
    #[test]
    fn imports_require_current_bytes_at_both_endpoints() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        scan_fixture(root);
        let fresh = Board {
            backlog: vec![task(1, "src/a.rs"), task(2, "src/b.rs")],
            running: vec![],
        };
        let mut board = fresh.clone();
        augment(root, &mut board);
        assert_eq!(board.backlog[0].reads, vec!["src/b.rs"]);
        assert_eq!(links(&board), BTreeSet::from([(1, 2)]));
        for file in ["src/a.rs", "src/b.rs"] {
            scan_fixture(root);
            std::fs::write(root.join(file), "changed\n").unwrap();
            let mut stale = fresh.clone();
            augment(root, &mut stale);
            assert!(links(&stale).is_empty(), "stale {file}");
        }
    }
    #[test]
    fn missing_map_global_criteria_and_common_reads_never_invent_write_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut a = task(1, "one/a.rs");
        let mut b = task(2, "two/b.rs");
        let mut board = Board {
            backlog: vec![a.clone(), b.clone()],
            running: vec![],
        };
        augment(root, &mut board);
        assert!(links(&board).is_empty());
        assert!(
            !a.relation(&b).semantic_interference(),
            "a global criterion is not a reason to call Jev"
        );
        a.reads = vec!["shared.rs".into()];
        b.reads = a.reads.clone();
        assert!(!a.relation(&b).read_write);
        assert!(!a.relation(&b).semantic_interference());
        board.backlog = vec![a, b];
        assert_eq!(
            links(&board),
            BTreeSet::from([(1, 2)]),
            "common context can pack without blocking"
        );
        assert!(
            !mustard_core::io::project_map::model_path(root).exists(),
            "observation creates no database"
        );
    }
    #[test]
    fn current_import_related_tasks_share_one_native_wave() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        scan_fixture(root);
        let a = backlog_task_on(root, said, crit, "Change consumer.", &["src/a.rs"]);
        let b = backlog_task_on(root, said, crit, "Change contract.", &["src/b.rs"]);
        let log = spec_now(root);
        assert_eq!(
            dispatch_backlog(root, "x", &log, &log, 4, None),
            Ok(vec![1])
        );
        assert_eq!(wave_order(root, 1), vec![a, b]);
    }
    #[test]
    fn a_running_contract_writer_holds_its_consumer_but_not_independent_work() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        scan_fixture(root);
        backlog_task_on(root, said, crit, "Change contract.", &["src/b.rs"]);
        let log = spec_now(root);
        assert_eq!(
            dispatch_backlog(root, "x", &log, &log, 4, None),
            Ok(vec![1])
        );
        seed_running(root, 1);
        let consumer = backlog_task_on(root, said, crit, "Change consumer.", &["src/a.rs"]);
        let log = spec_now(root);
        let never = |_: &Board| -> Result<
            crate::shared::judgement::Judged,
            mustard_core::domain::map_filter::FilterError,
        > { panic!("an exact native reservation needs no paid judgement") };
        assert_eq!(
            dispatch_backlog(root, "x", &log, &log, 4, Some(&never)),
            Ok(vec![])
        );
        let independent = backlog_task_on(root, said, crit, "Change docs.", &["README.md"]);
        let log = spec_now(root);
        assert_eq!(
            dispatch_backlog(root, "x", &log, &log, 4, None),
            Ok(vec![2])
        );
        assert_eq!(wave_order(root, 2), vec![independent]);
        assert_eq!(
            spec_now(root).current(consumer).and_then(|t| t.wave()),
            None
        );
    }
    #[test]
    fn shared_read_with_a_running_wave_remains_parallel() {
        let mut a = task(1, "one/a.rs");
        a.reads = vec!["common.rs".into()];
        let mut b = task(2, "two/b.rs");
        b.reads = a.reads.clone();
        let board = Board {
            backlog: vec![a],
            running: vec![BoardWave {
                n: 1,
                tasks: vec![b],
            }],
        };
        assert!(
            !board.backlog[0]
                .relation(&board.running[0].tasks[0])
                .read_write
        );
    }
}
