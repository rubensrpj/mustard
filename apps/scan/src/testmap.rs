//! Which tests cover each file: a test that imports the file, or a test that
//! keeps changing together with it in git. A file that carries its own tests
//! (an inline marker from `test-dirs.toml`) says so on its own.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use mustard_core::domain::ast::is_test_path;
use mustard_core::domain::project_map::{covers_by_history, history_stats, History};

use crate::model::Module;

/// How many covering tests a file keeps.
const MAX_TESTS: usize = 10;

/// The inline test markers declared in `test-dirs.toml` (data, not logic).
fn inline_markers() -> &'static [String] {
    static MARKERS: OnceLock<Vec<String>> = OnceLock::new();
    MARKERS.get_or_init(|| {
        let raw: toml::Value = toml::from_str(include_str!("../test-dirs.toml")).unwrap_or(toml::Value::Integer(0));
        raw.get("inline_markers")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|w| w.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    })
}

/// `true` when the content carries one of the inline test markers.
pub(crate) fn has_inline_tests(content: &str) -> bool {
    inline_markers().iter().any(|marker| content.contains(marker.as_str()))
}

/// Fill `tests` on every module from the resolved imports (`deps`) and the
/// history. Test files themselves get none.
pub(crate) fn assign(modules: &mut [Module], history: &History) {
    let tests: BTreeSet<String> = modules.iter().filter(|m| is_test_path(&m.path)).map(|m| m.path.clone()).collect();
    let mut found: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for test in modules.iter().filter(|m| tests.contains(&m.path)) {
        for dep in test.deps.iter().filter(|d| !tests.contains(*d)) {
            found.entry(dep.clone()).or_default().insert(test.path.clone());
        }
    }
    let stats = history_stats(history);
    for module in modules.iter().filter(|m| !tests.contains(&m.path)) {
        let commits = stats.commits.get(&module.path).copied().unwrap_or(0);
        let Some(partners) = stats.together.get(&module.path) else {
            continue;
        };
        for (other, &together) in partners {
            if tests.contains(other) && covers_by_history(together, commits) {
                found.entry(module.path.clone()).or_default().insert(other.clone());
            }
        }
    }
    for module in modules.iter_mut() {
        module.tests =
            found.remove(&module.path).map(|set| set.into_iter().take(MAX_TESTS).collect()).unwrap_or_default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::project_map::RawCommit;

    fn module(path: &str, deps: &[&str]) -> Module {
        Module { path: path.to_string(), deps: deps.iter().map(|d| (*d).to_string()).collect(), ..Default::default() }
    }

    #[test]
    fn a_test_covers_what_it_imports_and_what_it_keeps_changing_with() {
        let mut modules = vec![
            module("src/a.rs", &[]),
            module("src/b.rs", &[]),
            module("tests/a_test.rs", &["src/a.rs"]),
            module("tests/b_flow.rs", &[]),
        ];
        let commit = |id: &str, files: &[&str]| RawCommit {
            id: id.to_string(),
            at: 1,
            added: Vec::new(),
            changed: files.iter().map(|f| (*f).to_string()).collect(),
        };
        let history = History::from_raw(vec![
            commit("1", &["src/b.rs", "tests/b_flow.rs"]),
            commit("2", &["src/b.rs", "tests/b_flow.rs"]),
            commit("3", &["src/b.rs"]),
        ]);
        assign(&mut modules, &history);
        assert_eq!(modules[0].tests, vec!["tests/a_test.rs".to_string()]);
        assert_eq!(modules[1].tests, vec!["tests/b_flow.rs".to_string()]);
        assert!(modules[2].tests.is_empty());
    }

    #[test]
    fn the_inline_marker_is_read_from_the_data_file() {
        assert!(has_inline_tests("fn f() {}\n#[cfg(test)]\nmod tests {}\n"));
        assert!(!has_inline_tests("fn f() {}\n"));
    }
}
