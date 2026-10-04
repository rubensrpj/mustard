//! A linha de tamanho de cada onda: as linhas que ela pôs, as que tirou, os
//! testes novos e os arquivos que mudou. A conferência depois da onda a monta,
//! com o disco já juntado, e ela vai à resposta da rodada e ao corpo do
//! commit, para quem conduz e quem lê o histórico verem o tamanho de cada
//! entrega. É dado da onda e nunca um achado: nada aqui recusa.
//!
//! - Postas e tiradas: o `git diff --numstat` do último commit contra o disco,
//!   nos arquivos da onda, repositório por repositório; o arquivo novo, que o
//!   git ainda não rastreia, conta todas as linhas como postas. O arquivo que
//!   duas ondas da rodada mudaram conta nas duas.
//! - Testes novos: a função ou o método que está no mapa de depois, dentro de
//!   um trecho de teste do arquivo da onda ou num arquivo de teste inteiro, e
//!   não está no mapa da base. Vale em toda língua que o scan reconhece, sem
//!   ler o texto de marca nenhuma de teste.
//! - O arquivo apagado não põe nada e não tem teste: só tira.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mustard_core::domain::ast::is_test_path;
use mustard_core::platform::i18n::{translate, Locale};

use super::commit::{check_commit_text, git, repo_of, AfterWave};
use super::removed_check::{in_test_lines, same_piece};
use crate::commands::git_settle::submodules_of;

/// Os tipos de declaração que contam como teste quando estão num trecho de
/// teste: o que se executa. O tipo, o campo e a constante que o teste declara
/// são dele, mas não são um teste.
const TEST_KINDS: &[&str] = &["function", "method"];

/// A linha de tamanho de cada onda de `maps`, pelo número dela, em `root`
/// com a junção já no disco.
pub(super) fn lines(root: &Path, maps: &AfterWave, lang: Locale) -> Vec<(u64, String)> {
    let subs = submodules_of(root);
    maps.changed
        .iter()
        .map(|(wave, files)| {
            let (added, removed) = moved_lines(root, &subs, files);
            let line = translate("round.size.line", lang)
                .replace("{wave}", &wave.to_string())
                .replace("{added}", &added.to_string())
                .replace("{removed}", &removed.to_string())
                .replace("{tests}", &new_tests(maps, files).to_string())
                .replace("{files}", &files.len().to_string());
            (*wave, line)
        })
        .collect()
}

/// A mensagem de commit com as linhas de tamanho no fim do corpo, uma por
/// onda, depois de uma linha em branco. A linha é dado e nunca recusa: se com
/// ela a mensagem deixa de caber no modelo, volta a de antes.
pub(super) fn in_body(message: Option<(String, String)>, sizes: &[(u64, String)]) -> Option<(String, String)> {
    let (title, body) = message?;
    let listed: Vec<String> = sizes.iter().map(|(_, line)| format!("- {line}")).collect();
    let longer = format!("{body}\n\n{}", listed.join("\n"));
    if listed.is_empty() || check_commit_text(&title, &longer).is_err() {
        return Some((title, body));
    }
    Some((title, longer))
}

/// As linhas postas e as tiradas nos arquivos `files`, contra o último commit
/// de cada repositório que os guarda.
fn moved_lines(root: &Path, subs: &[String], files: &[String]) -> (u64, u64) {
    let mut by_repo: BTreeMap<PathBuf, Vec<&str>> = BTreeMap::new();
    let inner: Vec<(PathBuf, String)> = files.iter().map(|file| repo_of(root, subs, file)).collect();
    for (repo, path) in &inner {
        by_repo.entry(repo.clone()).or_default().push(path);
    }
    let (mut added, mut removed) = (0, 0);
    for (repo, paths) in by_repo {
        let diff = [&["diff", "--numstat", "--no-renames", "HEAD", "--"][..], &paths].concat();
        for line in git(&repo, &diff).unwrap_or_default().lines() {
            // O arquivo binário vem como `-` nas duas colunas e não conta.
            let mut columns = line.split('\t').map(|column| column.parse::<u64>().unwrap_or(0));
            added += columns.next().unwrap_or(0);
            removed += columns.next().unwrap_or(0);
        }
        let untracked = [&["ls-files", "-z", "--others", "--exclude-standard", "--"][..], &paths].concat();
        for path in git(&repo, &untracked).unwrap_or_default().split('\0').filter(|path| !path.is_empty()) {
            added += text_lines(&repo.join(path));
        }
    }
    (added, removed)
}

/// As linhas do arquivo de texto `path`; o binário e o que não se lê não têm.
fn text_lines(path: &Path) -> u64 {
    let Ok(bytes) = std::fs::read(path) else { return 0 };
    if bytes.contains(&0) {
        return 0;
    }
    let breaks = bytes.iter().filter(|byte| **byte == b'\n').count();
    let open_last = usize::from(bytes.last().is_some_and(|byte| *byte != b'\n'));
    u64::try_from(breaks + open_last).unwrap_or(0)
}

/// Os testes novos dos arquivos `files`: o que o mapa de depois traz num
/// trecho de teste (ou num arquivo de teste inteiro) e o da base não traz, na
/// mesma peça de mesmo nome, tipo e dono.
fn new_tests(maps: &AfterWave, files: &[String]) -> usize {
    files
        .iter()
        .filter_map(|file| maps.after.module(file))
        .map(|now| {
            let before = maps.base.module(&now.path);
            now.declarations
                .iter()
                .filter(|decl| TEST_KINDS.contains(&decl.kind.as_str()))
                .filter(|decl| is_test_path(&now.path) || in_test_lines(now, decl.line))
                .filter(|decl| !before.is_some_and(|old| old.declarations.iter().any(|was| same_piece(was, decl))))
                .count()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};
    use tempfile::tempdir;

    use super::super::imports_check::tests::mine_giving;
    use super::super::tests::{approved, delivered, git_at, last_commit, round, round_with_mine, warning_of};
    use super::*;

    /// Um arquivo do mapa com as funções `functions` (nome e linha) e os
    /// trechos de teste `tests` (primeira e última linha de cada um).
    fn module(path: &str, functions: &[(&str, u64)], tests: &[[u64; 2]]) -> Value {
        let declarations: Vec<Value> = functions
            .iter()
            .map(|(name, line)| json!({"kind": "function", "name": name, "line": line, "end_line": line + 1}))
            .collect();
        json!({"path": path, "language": "rust", "declarations": declarations, "test_lines": tests})
    }

    /// O mapa de `modules`, do jeito que o scan o grava.
    fn map(modules: &[Value]) -> mustard_core::domain::project_map::ProjectMap {
        serde_json::from_value(json!({"modules": modules})).unwrap()
    }

    /// Os mapas da conferência com a onda 1 mudando `files`.
    fn maps(base: &[Value], after: &[Value], files: &[&str]) -> AfterWave {
        let changed = vec![(1, files.iter().map(|file| (*file).to_string()).collect())];
        AfterWave { base: map(base), after: map(after), changed }
    }

    #[test]
    fn a_test_file_counts_every_function_as_a_test() {
        // Um arquivo de teste inteiro: cada função dele conta, o tipo que ele
        // declara não, e a função que a base já tinha também não.
        let types = json!({"kind": "struct", "name": "Fixture", "line": 1, "end_line": 2});
        let mut after = module("tests/soma.rs", &[("adds", 4), ("subtracts", 8), ("helper", 12), ("old", 16)], &[]);
        after["declarations"].as_array_mut().unwrap().push(types);
        let base = module("tests/soma.rs", &[("old", 1)], &[]);
        let found = maps(&[base], &[after], &["tests/soma.rs"]);
        assert_eq!(new_tests(&found, &["tests/soma.rs".to_string()]), 3);
    }

    #[test]
    fn only_the_test_part_of_a_program_file_counts_as_tests() {
        let after = module("src/soma.rs", &[("sum", 3), ("adds", 21), ("subtracts", 25), ("old", 29)], &[[18, 40]]);
        let base = module("src/soma.rs", &[("sum", 3), ("old", 12)], &[[10, 20]]);
        let found = maps(&[base], &[after], &["src/soma.rs"]);
        assert_eq!(new_tests(&found, &["src/soma.rs".to_string()]), 2);
    }

    /// Um projeto com `src/a.rs` e `src/b.rs` comitados, a onda 1 mudando os
    /// dois e o `tests/fresh.rs`, que ainda não existe, e o mapa da base `base`.
    fn project(root: &Path, base: &Value) {
        approved(root, "x", &[(1, &["src/a.rs", "src/b.rs", "tests/fresh.rs"], &[])]);
        for (path, text) in [("src/a.rs", "fn sum() {}\n"), ("src/b.rs", "fn old() {}\n// segunda\n")] {
            std::fs::create_dir_all(root.join("src")).unwrap();
            std::fs::write(root.join(path), text).unwrap();
        }
        // O arquivo que a onda cria não existe no commit da base.
        std::fs::remove_file(root.join("tests/fresh.rs")).unwrap();
        git_at(root, &["add", "-A", "--", ".", ":(exclude).claude"]);
        git_at(root, &["commit", "-q", "-m", "arquivos"]);
        round(root, "x", None);
        mustard_core::io::project_map::write_text(root, &base.to_string()).unwrap();
    }

    #[test]
    fn size_line_counts_added_removed_tests_and_files_of_the_wave() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let base = json!({"modules": [module("src/a.rs", &[("sum", 1)], &[]), module("src/b.rs", &[("old", 1)], &[])]});
        project(root, &base);
        // A onda põe uma linha em `src/a.rs`, troca a segunda de `src/b.rs` e
        // cria `tests/fresh.rs`, com três linhas que o git ainda não rastreia;
        // o mapa de depois traz dois testes novos em `src/a.rs` e três no
        // arquivo de teste novo.
        std::fs::create_dir_all(root.join("tests")).unwrap();
        std::fs::write(root.join("tests/fresh.rs"), "fn fresh() {}\nfn other() {}\nfn last() {}\n").unwrap();
        let report = delivered(root, 1, "A soma mudou.", &["src/a.rs", "src/b.rs", "tests/fresh.rs"]);
        std::fs::write(root.join("src/b.rs"), "fn old() {}\n// terceira\n").unwrap();
        let after = json!({"modules": [
            module("src/a.rs", &[("sum", 1), ("adds", 4), ("subtracts", 6)], &[[3, 8]]),
            module("src/b.rs", &[("old", 1)], &[]),
            module("tests/fresh.rs", &[("fresh", 1), ("other", 2), ("last", 3)], &[]),
        ]});
        let out = round_with_mine(root, "x", Some(&report), &mine_giving(after));
        assert_eq!(out["ok"], json!(true), "{out}");
        // `src/a.rs`: +1; `src/b.rs`: +1 -1; `tests/fresh.rs`: as três linhas e o
        // comentário que a entrega pôs, +4.
        let size = warning_of(&out, "wave-size");
        assert_eq!(size["wave"], json!(1), "{size}");
        assert_eq!(size["hint"], json!("onda 1: +6 -1, 5 testes, 3 arquivos"), "{out}");
    }

    #[test]
    fn the_round_commit_body_carries_the_size_line() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let base = json!({"modules": [module("src/a.rs", &[("sum", 1)], &[]), module("src/b.rs", &[("old", 1)], &[])]});
        project(root, &base);
        std::fs::create_dir_all(root.join("tests")).unwrap();
        std::fs::write(root.join("tests/fresh.rs"), "fn fresh() {}\n").unwrap();
        let report = delivered(root, 1, "A soma mudou.", &["src/a.rs", "src/b.rs", "tests/fresh.rs"]);
        let after = json!({"modules": [
            module("src/a.rs", &[("sum", 1)], &[]),
            module("src/b.rs", &[("old", 1)], &[]),
            module("tests/fresh.rs", &[("fresh", 1)], &[]),
        ]});
        let out = round_with_mine(root, "x", Some(&report), &mine_giving(after));
        assert_eq!(out["ok"], json!(true), "{out}");
        let (_, body) = last_commit(root);
        assert!(body.contains("- onda 1: a onda 1 saiu"), "{body}");
        assert!(body.ends_with("\n\n- onda 1: +4 -0, 1 testes, 3 arquivos"), "{body}");
    }

    #[test]
    fn a_wave_that_only_deletes_shows_zero_added() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        git_at(root, &["init", "-q"]);
        std::fs::write(root.join("gone.rs"), "fn a() {}\nfn b() {}\nfn c() {}\n").unwrap();
        git_at(root, &["add", "-A"]);
        git_at(root, &["commit", "-q", "-m", "arquivos"]);
        std::fs::remove_file(root.join("gone.rs")).unwrap();
        let base = module("gone.rs", &[("a", 1), ("b", 2), ("c", 3)], &[]);
        let found = maps(&[base], &[], &["gone.rs"]);
        let lines = lines(root, &found, Locale::EnUs);
        assert_eq!(lines, vec![(1, "wave 1: +0 -3, 0 tests, 1 files".to_string())]);
    }

    #[test]
    fn a_message_that_would_not_fit_with_the_lines_keeps_the_body_it_had() {
        let body = "- onda 1: resumo\n".repeat(235);
        let message = Some(("feat(onda-1): resumo".to_string(), body.trim().to_string()));
        assert!(check_commit_text("feat(onda-1): resumo", body.trim()).is_ok());
        let sizes = vec![(1, "onda 1: +2 -0, 0 testes, 2 arquivos".to_string())];
        assert_eq!(in_body(message.clone(), &sizes), message);
    }
}
