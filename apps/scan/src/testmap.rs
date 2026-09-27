//! Which tests cover each file: a test that imports the file, or a test that
//! keeps changing together with it in git. A file that carries its own tests
//! (an inline marker from the core's test-file data, `test-files.toml`) says
//! so on its own, and covers what its test block imports.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mustard_core::domain::ast::{inline_test_markers, is_test_path, tested_name};
use mustard_core::domain::project_map::{covers_by_history, history_stats, History};

use crate::model::Module;

/// How many covering tests a file keeps.
const MAX_TESTS: usize = 10;

/// `true` when the content carries one of the inline test markers of the
/// core's test-file data.
pub(crate) fn has_inline_tests(content: &str) -> bool {
    inline_test_markers().iter().any(|marker| content.contains(marker.as_str()))
}

/// Por que um teste cobre um arquivo, do mais forte ao mais fraco: ele importa
/// o arquivo, ou só muda junto com ele no histórico.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Link {
    Imports,
    History,
}

/// Fill `tests` on every module from the resolved imports (`deps` of a test
/// file, `test_deps` of any file) and the history. Test files themselves get
/// none.
pub(crate) fn assign(modules: &mut [Module], history: &History) {
    let tests: BTreeSet<String> = modules.iter().filter(|m| is_test_path(&m.path)).map(|m| m.path.clone()).collect();
    let mut found: BTreeMap<String, BTreeMap<String, Link>> = BTreeMap::new();
    for test in modules.iter().filter(|m| tests.contains(&m.path)) {
        for dep in test.deps.iter().filter(|d| !tests.contains(*d)) {
            link(&mut found, dep, &test.path, Link::Imports);
        }
    }
    // O trecho de teste escrito dentro de um arquivo cobre o que ele importa.
    for module in modules.iter() {
        for dep in module.test_deps.iter().filter(|d| !tests.contains(*d)) {
            link(&mut found, dep, &module.path, Link::Imports);
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
                link(&mut found, &module.path, other, Link::History);
            }
        }
    }
    for module in modules.iter_mut() {
        module.tests = found.remove(&module.path).map(|links| ranked(&module.path, links)).unwrap_or_default();
    }
}

/// Liga `test` a `file`; o teste que chega pelos dois caminhos fica com o mais
/// forte.
fn link(found: &mut BTreeMap<String, BTreeMap<String, Link>>, file: &str, test: &str, why: Link) {
    let kept = found.entry(file.to_string()).or_default().entry(test.to_string()).or_insert(why);
    *kept = (*kept).min(why);
}

/// Os testes de `file` em ordem, cortados em [`MAX_TESTS`]: primeiro o que tem
/// o nome do arquivo, depois os que o importam, por último os que só mudam
/// junto com ele; empate em ordem alfabética. O corte vem depois da ordem, para
/// que o teste do próprio arquivo nunca perca a vaga para outro que só vem
/// antes no alfabeto.
fn ranked(file: &str, links: BTreeMap<String, Link>) -> Vec<String> {
    let name = Path::new(file).file_stem().and_then(|stem| stem.to_str());
    let same_name = |test: &str| name.is_some_and(|n| tested_name(test).is_some_and(|t| t.eq_ignore_ascii_case(n)));
    let mut order: Vec<(bool, Link, String)> =
        links.into_iter().map(|(test, why)| (!same_name(&test), why, test)).collect();
    order.sort();
    order.into_iter().take(MAX_TESTS).map(|(_, _, test)| test).collect()
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
    fn an_end_to_end_test_folder_covers_the_file_its_test_imports() {
        let mut modules = vec![module("src/pedido.ts", &[]), module("e2e/pedido.ts", &["src/pedido.ts"])];
        assign(&mut modules, &History::from_raw(Vec::new()));
        assert_eq!(modules[0].tests, vec!["e2e/pedido.ts".to_string()]);
        assert!(modules[1].tests.is_empty(), "o teste não ganha teste");
    }

    fn commit(id: &str, files: &[&str]) -> RawCommit {
        RawCommit { id: id.to_string(), at: 1, changed: files.iter().map(|f| (*f).to_string()).collect(), ..RawCommit::default() }
    }

    #[test]
    fn the_test_named_after_the_file_keeps_its_place_under_the_cap() {
        // Doze testes importam o serviço; o de mesmo nome é o último no
        // alfabeto, e o teto é dez.
        let target = "src/order.service.ts";
        let mut modules = vec![module(target, &[]), module("src/order.service.spec.ts", &[target])];
        let others: Vec<String> = (1..=11).map(|i| format!("e2e/case-{i:02}.spec.ts")).collect();
        modules.extend(others.iter().map(|path| module(path, &[target])));
        assign(&mut modules, &History::from_raw(Vec::new()));
        let mut expected = vec!["src/order.service.spec.ts".to_string()];
        expected.extend(others.iter().take(9).cloned());
        assert_eq!(modules[0].tests, expected, "o teste de mesmo nome fica, e os dois últimos no alfabeto saem");
    }

    #[test]
    fn a_test_that_imports_comes_before_one_that_only_changes_together() {
        // `src/b.rs` tem dez testes que o importam e um parceiro de histórico
        // que vem antes deles no alfabeto; `src/c.rs`, um de cada.
        let mut modules = vec![module("src/b.rs", &[]), module("src/c.rs", &[]), module("tests/a_flow.rs", &[])];
        let importers: Vec<String> = (1..=10).map(|i| format!("tests/b_{i:02}.rs")).collect();
        modules.extend(importers.iter().map(|path| module(path, &["src/b.rs"])));
        modules.push(module("tests/z_flow.rs", &["src/c.rs"]));
        let history = History::from_raw(vec![
            commit("1", &["src/b.rs", "src/c.rs", "tests/a_flow.rs"]),
            commit("2", &["src/b.rs", "src/c.rs", "tests/a_flow.rs"]),
            commit("3", &["src/b.rs", "src/c.rs"]),
        ]);
        assign(&mut modules, &history);
        assert_eq!(modules[0].tests, importers, "o parceiro de histórico perde a vaga para quem importa");
        assert_eq!(modules[1].tests, vec!["tests/z_flow.rs".to_string(), "tests/a_flow.rs".to_string()]);
    }

    #[test]
    fn the_inline_marker_is_read_from_the_data_file() {
        assert!(has_inline_tests("fn f() {}\n#[cfg(test)]\nmod tests {}\n"));
        assert!(!has_inline_tests("fn f() {}\n"));
    }
}
