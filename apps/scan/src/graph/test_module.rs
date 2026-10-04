//! Os arquivos que um módulo declara como teste. O módulo sem corpo que o
//! arquivo marca como teste (`Module::test_modules`) nomeia um arquivo que é
//! todo de teste, como o módulo de teste escrito dentro do arquivo é: o mapa o
//! guarda como um trecho de teste que vai da linha 1 ao fim
//! ([`DECLARED_TEST_LINES`]), e quem lê o trecho trata o que o arquivo declara,
//! chama e cita como do teste. O que mora na pasta do módulo, os módulos dele,
//! também é de teste.
//!
//! O arquivo de um módulo se acha pela pasta dos módulos de quem o declara
//! ([`inner_folder`]), a mesma regra que liga o caminho sem apelido ao módulo
//! filho, e só vale o arquivo da língua do pai que o pai declara dentro de si
//! ([`is_declared_child`]). O arquivo que já é de teste pelo caminho
//! ([`is_test_path`]) fica como está: a chamada que ele escreve segue sendo uso
//! de quem ela chama, porque é ela que diz quem testa cada declaração.

use std::collections::{HashMap, HashSet};

use mustard_core::domain::ast::{is_declared_child, is_test_path};

use super::{build_stem_index, exact_path_candidate, inner_folder, join_dir};
use crate::model::{Module, DECLARED_TEST_LINES};

/// Põe em cada arquivo declarado como teste, e nos módulos dele, o trecho de
/// teste do arquivo inteiro ([`DECLARED_TEST_LINES`]). Refeito do projeto
/// inteiro em toda passada: o arquivo que nenhum módulo declara mais como
/// teste não o traz.
pub(crate) fn mark_declared(modules: &mut [Module]) {
    let declared = declared_files(modules);
    for module in modules.iter_mut().filter(|m| declared.contains(&m.path)) {
        if !module.test_lines.contains(&DECLARED_TEST_LINES) {
            module.test_lines.insert(0, DECLARED_TEST_LINES);
        }
    }
}

/// Os arquivos de `modules` que um módulo declara como teste ou que moram na
/// pasta de um deles, fora os que já são de teste pelo caminho.
fn declared_files(modules: &[Module]) -> HashSet<String> {
    if modules.iter().all(|m| m.test_modules.is_empty()) {
        return HashSet::new();
    }
    let stem_index = build_stem_index(modules);
    let paths: HashSet<&str> = modules.iter().map(|m| m.path.as_str()).collect();
    let language: HashMap<&str, &str> = modules.iter().map(|m| (m.path.as_str(), m.language.as_str())).collect();
    let mut roots: Vec<(String, &str)> = Vec::new();
    for parent in modules {
        let lang = parent.language.as_str();
        let folder = inner_folder(&parent.path, lang);
        for name in &parent.test_modules {
            for file in exact_path_candidate(&join_dir(&folder, name), lang, &stem_index, &paths) {
                if language.get(file.as_str()) == Some(&lang) && is_declared_child(&parent.path, &file, lang) {
                    roots.push((file, lang));
                }
            }
        }
    }
    modules
        .iter()
        .filter(|m| !is_test_path(&m.path))
        .filter(|m| {
            roots.iter().any(|(root, lang)| {
                m.language == *lang && (m.path == *root || is_declared_child(root, &m.path, lang))
            })
        })
        .map(|m| m.path.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(path: &str, language: &str, test_modules: &[&str]) -> Module {
        Module {
            path: path.to_string(),
            language: language.to_string(),
            test_modules: test_modules.iter().map(|name| (*name).to_string()).collect(),
            ..Default::default()
        }
    }

    /// Os caminhos que `mark_declared` deixa com o trecho do arquivo inteiro.
    fn marked(mut modules: Vec<Module>) -> Vec<String> {
        mark_declared(&mut modules);
        modules.into_iter().filter(|m| m.test_lines.contains(&DECLARED_TEST_LINES)).map(|m| m.path).collect()
    }

    #[test]
    fn the_file_a_parent_declares_as_test_is_a_test_block_and_its_sibling_is_not() {
        let found = marked(vec![
            module("src/lib.rs", "rust", &["helpers"]),
            module("src/helpers.rs", "rust", &[]),
            module("src/real.rs", "rust", &[]),
        ]);
        assert_eq!(found, vec!["src/helpers.rs".to_string()]);
    }

    #[test]
    fn the_module_of_a_plain_file_lives_in_the_folder_that_takes_its_name() {
        let found = marked(vec![
            module("src/a.rs", "rust", &["fx"]),
            module("src/a/fx.rs", "rust", &[]),
            module("src/fx.rs", "rust", &[]),
        ]);
        assert_eq!(found, vec!["src/a/fx.rs".to_string()]);
    }

    #[test]
    fn the_modules_below_a_test_module_are_test_too() {
        let found = marked(vec![
            module("src/lib.rs", "rust", &["helpers"]),
            module("src/helpers.rs", "rust", &[]),
            module("src/helpers/inner.rs", "rust", &[]),
            module("src/helpers/inner/deep.rs", "rust", &[]),
            module("src/helpers_other.rs", "rust", &[]),
        ]);
        assert_eq!(
            found,
            vec!["src/helpers.rs".to_string(), "src/helpers/inner.rs".to_string(), "src/helpers/inner/deep.rs".to_string()]
        );
    }

    #[test]
    fn a_test_module_that_is_a_folder_entry_marks_the_files_in_its_folder() {
        let found = marked(vec![
            module("src/a/mod.rs", "rust", &["x"]),
            module("src/a/x/mod.rs", "rust", &[]),
            module("src/a/x/y.rs", "rust", &[]),
            module("src/a/other.rs", "rust", &[]),
        ]);
        assert_eq!(found, vec!["src/a/x/mod.rs".to_string(), "src/a/x/y.rs".to_string()]);
    }

    #[test]
    fn a_file_of_another_language_with_the_same_stem_is_not_the_module() {
        let found = marked(vec![
            module("src/lib.rs", "rust", &["helpers"]),
            module("src/helpers.py", "python", &[]),
            module("src/helpers/inner.rs", "rust", &[]),
        ]);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_file_that_is_a_test_by_its_path_keeps_its_own_lines() {
        let found = marked(vec![module("src/lib.rs", "rust", &["foo_test"]), module("src/foo_test.rs", "rust", &[])]);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn marking_twice_keeps_one_block_and_the_blocks_of_the_file() {
        let mut helpers = module("src/helpers.rs", "rust", &[]);
        helpers.test_lines = vec![(5, 9)];
        let mut modules = vec![module("src/lib.rs", "rust", &["helpers"]), helpers];
        mark_declared(&mut modules);
        mark_declared(&mut modules);
        assert_eq!(modules[1].test_lines, vec![DECLARED_TEST_LINES, (5, 9)]);
    }

    #[test]
    fn nothing_is_marked_without_a_declared_test_module() {
        assert!(marked(vec![module("src/lib.rs", "rust", &[]), module("src/helpers.rs", "rust", &[])]).is_empty());
    }
}
