//! O tamanho de cada onda: as linhas que ela pôs, as que tirou, os testes
//! novos e os arquivos que mudou. A conferência depois da onda o mede, com o
//! disco já juntado. Ele vira a linha de tamanho, que vai à resposta da
//! rodada e ao corpo do commit para quem conduz e quem lê o histórico verem o
//! tamanho de cada entrega, e vira o achado que devolve ao agente a onda que
//! cresce além do que a tarefa pede.
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
//! - Crescimento: postas menos tiradas, para que a troca de nomes, que põe e
//!   tira o mesmo, nunca conte. Recusa a onda que cresce mais que o piso e
//!   mais que o múltiplo da mediana de linhas postas pelos commits da rodada
//!   (a mesma que o pedido da onda cita, de `wave_size`), e a que traz mais
//!   testes novos que o piso e que o permitido por critério ou regra coberto.
//!   Sem histórico bastante o projeto não tem régua, e fica só a linha.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use mustard_core::domain::ast::is_test_path;
use mustard_core::domain::spec_events::SpecLog;
use mustard_core::io::wave_size;
use mustard_core::platform::i18n::{translate, Locale};

use super::commit::{check_commit_text, git, repo_of, AfterWave, Finding};
use super::removed_check::{in_test_lines, same_piece};
use crate::commands::git_settle::submodules_of;

/// Os tipos de declaração que contam como teste quando estão num trecho de
/// teste: o que se executa. O tipo, o campo e a constante que o teste declara
/// são dele, mas não são um teste.
const TEST_KINDS: &[&str] = &["function", "method"];

/// Quantas medianas de linhas postas o código de uma onda pode crescer. A
/// onda que só passa de uma mediana é uma onda grande normal; três é a que o
/// usuário aprovou como grande demais, vista contra as ondas reais da obra.
const GROWTH_MEDIANS: u64 = 3;

/// O crescimento, em linhas, abaixo do qual a onda nunca recusa, qualquer
/// que seja a mediana: num projeto de ondas pequenas, três medianas são
/// poucas linhas, e cortar o que cabe numa leitura só custa mais do que poupa.
const GROWTH_FLOOR: u64 = 600;

/// Quantos testes novos cada critério ou regra que a onda cobre paga. Cada
/// comportamento ganha um teste, e o que passa de dois por critério é, quase
/// sempre, teste que repete outro.
const TESTS_PER_CRITERION: usize = 2;

/// Os testes novos abaixo dos quais a onda nunca recusa, qualquer que seja o
/// número de critérios e regras: a onda de um critério só ainda pode provar a
/// regra e os dois casos de borda dela.
const TESTS_FLOOR: usize = 6;

/// O tamanho de uma onda: o que a linha de tamanho mostra e o que a conferência
/// mede contra o limite.
pub(super) struct Size {
    wave: u64,
    added: u64,
    removed: u64,
    tests: usize,
    files: usize,
}

impl Size {
    /// O quanto o código cresceu: postas menos tiradas, sem passar de zero.
    fn growth(&self) -> u64 {
        self.added.saturating_sub(self.removed)
    }
}

/// O tamanho de cada onda de `maps`, em `root` com a junção já no disco.
pub(super) fn measure(root: &Path, maps: &AfterWave) -> Vec<Size> {
    let subs = submodules_of(root);
    maps.changed
        .iter()
        .map(|(wave, files)| {
            let (added, removed) = moved_lines(root, &subs, files);
            Size { wave: *wave, added, removed, tests: new_tests(maps, files), files: files.len() }
        })
        .collect()
}

/// A linha de tamanho de cada onda de `sizes`, pelo número dela.
pub(super) fn lines(sizes: &[Size], lang: Locale) -> Vec<(u64, String)> {
    sizes
        .iter()
        .map(|size| {
            let line = translate("round.size.line", lang)
                .replace("{wave}", &size.wave.to_string())
                .replace("{added}", &size.added.to_string())
                .replace("{removed}", &size.removed.to_string())
                .replace("{tests}", &size.tests.to_string())
                .replace("{files}", &size.files.to_string());
            (size.wave, line)
        })
        .collect()
}

/// Os achados de tamanho de `sizes`, no plano `log`: a onda que cresce mais
/// que o limite de linhas e a que traz mais testes novos que o limite de
/// testes, as duas recusadas. O histórico só é lido quando alguma onda passa
/// dos pisos; sem ondas bastante nele, não há achado nenhum.
pub(super) fn findings(root: &Path, sizes: &[Size], log: &SpecLog, lang: Locale) -> Vec<Finding> {
    if !sizes.iter().any(|size| size.growth() > GROWTH_FLOOR || size.tests > TESTS_FLOOR) {
        return Vec::new();
    }
    let Some(median) = wave_size::median_added(root, lang) else { return Vec::new() };
    let limit = GROWTH_FLOOR.max(GROWTH_MEDIANS * median);
    let mut out = Vec::new();
    for size in sizes {
        if size.growth() > limit {
            let text = translate("round.size.over", lang)
                .replace("{wave}", &size.wave.to_string())
                .replace("{added}", &size.added.to_string())
                .replace("{removed}", &size.removed.to_string())
                .replace("{growth}", &size.growth().to_string())
                .replace("{limit}", &limit.to_string())
                .replace("{median}", &median.to_string());
            out.push(Finding { wave: size.wave, refuses: true, text });
        }
        let covered = covered_count(log, size.wave);
        let allowed = TESTS_FLOOR.max(TESTS_PER_CRITERION * covered);
        if size.tests > allowed {
            let text = translate("round.size.tests", lang)
                .replace("{wave}", &size.wave.to_string())
                .replace("{tests}", &size.tests.to_string())
                .replace("{covered}", &covered.to_string())
                .replace("{limit}", &allowed.to_string());
            out.push(Finding { wave: size.wave, refuses: true, text });
        }
    }
    out
}

/// Quantos critérios e regras a onda `wave` cobre: os critérios que ela
/// aponta e as regras e os critérios que as tarefas dela citam em `covers`,
/// cada um uma vez, na versão vigente.
fn covered_count(log: &SpecLog, wave: u64) -> usize {
    let mut covered: BTreeSet<u64> = log.wave_criteria(wave).iter().map(|e| e.id).collect();
    let cited = log
        .visible()
        .into_iter()
        .filter(|e| e.event_type == "task" && e.wave() == Some(wave))
        .flat_map(|task| task.ints("covers"))
        .filter_map(|id| log.current(id));
    covered.extend(cited.filter(|e| matches!(e.event_type.as_str(), "criterion" | "rule")).map(|e| e.id));
    covered.len()
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
    // Cada trecho que acaba em quebra é uma linha, e o último, sem quebra, também.
    u64::try_from(bytes.split_inclusive(|byte| *byte == b'\n').count()).unwrap_or(0)
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
    use super::super::tests::{
        approved, approved_with, delivered, git_at, git_text, id_of, last_commit, round, round_with_mine, warning_of, write,
    };
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
        let lines = lines(&measure(root, &found), Locale::EnUs);
        assert_eq!(lines, vec![(1, "wave 1: +0 -3, 0 tests, 1 files".to_string())]);
    }

    #[test]
    fn a_text_file_counts_its_lines_with_or_without_the_last_break() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let cases: [(&[u8], u64); 7] = [
            (b"", 0),
            (b"\n", 1),
            (b"a", 1),
            (b"a\n", 1),
            (b"a\nb", 2),
            (b"a\n\n\nb\n", 4),
            (b"a\0b\n", 0),
        ];
        for (at, (bytes, lines)) in cases.into_iter().enumerate() {
            let path = root.join(format!("f{at}"));
            std::fs::write(&path, bytes).unwrap();
            assert_eq!(text_lines(&path), lines, "{bytes:?}");
        }
        assert_eq!(text_lines(&root.join("missing")), 0);
    }

    #[test]
    fn a_message_that_would_not_fit_with_the_lines_keeps_the_body_it_had() {
        let body = "- onda 1: resumo\n".repeat(235);
        let message = Some(("feat(onda-1): resumo".to_string(), body.trim().to_string()));
        assert!(check_commit_text("feat(onda-1): resumo", body.trim()).is_ok());
        let sizes = vec![(1, "onda 1: +2 -0, 0 testes, 2 arquivos".to_string())];
        assert_eq!(in_body(message.clone(), &sizes), message);
    }
    /// O arquivo de `path` com `count` linhas que começam por `prefix`.
    fn write_lines(root: &Path, path: &str, prefix: &str, count: usize) {
        use std::fmt::Write as _;
        let mut text = String::new();
        for n in 0..count {
            writeln!(text, "{prefix}{n}").unwrap();
        }
        std::fs::write(root.join(path), text).unwrap();
    }

    /// [`grown_with`] com `history` commits de uma onda cada, que põem `median`
    /// linhas.
    fn grown(root: &Path, history: usize, median: usize, before: usize, rules: usize) {
        let commits: Vec<(String, usize)> = (0..history).map(|n| (format!("onda-{}", 100 + n), median)).collect();
        grown_with(root, &commits, before, rules);
    }

    /// Um projeto com a onda 1 mudando `src/big.rs`, que tem `before` linhas no
    /// commit; a tarefa da onda cobre `rules` regras além do critério que a onda
    /// aponta; e um commit de onda antiga por item de `history`, do mais velho
    /// ao mais novo, com o escopo e as linhas que põe, no mapa da base com o
    /// `src/big.rs` dentro.
    fn grown_with(root: &Path, history: &[(String, usize)], before: usize, rules: usize) {
        approved_with(root, "x", &[], |said| {
            let log = mustard_core::io::spec_events::read(&mustard_core::io::spec_events::spec_file(root, "x").unwrap())
                .unwrap()
                .unwrap();
            let crit = log.visible().into_iter().find(|e| e.event_type == "criterion").unwrap().id;
            write(root, "x", "wave", json!({"n": 1, "text": "Onda 1.", "criteria": [crit], "done_when": "A suíte passa.", "origin": said}));
            let covers: Vec<u64> = (0..rules)
                .map(|n| {
                    let text = format!("Vale sempre: a regra {n}.");
                    id_of(&write(root, "x", "rule", json!({"title": text, "text": text, "example": "e", "keys": ["k"],
                        "applies_to": {"files": ["**"]}, "origin": said})))
                })
                .collect();
            write(root, "x", "task", json!({"wave": 1, "text": "Tarefa da onda 1.", "files": [{"path": "src/big.rs"}],
                "depends_on": [], "covers": covers, "origin": said}));
        });
        std::fs::create_dir_all(root.join("src")).unwrap();
        write_lines(root, "src/big.rs", "old", before);
        git_at(root, &["add", "-A", "--", ".", ":(exclude).claude"]);
        git_at(root, &["commit", "-q", "-m", "arquivos"]);
        for (n, (scope, lines)) in history.iter().enumerate() {
            std::fs::create_dir_all(root.join("history")).unwrap();
            write_lines(root, &format!("history/w{n}.txt"), "x", *lines);
            git_at(root, &["add", "--", "history"]);
            git_at(root, &["commit", "-q", "-m", &format!("feat({scope}): onda antiga")]);
        }
        round(root, "x", None);
        let base = json!({"modules": [module("src/big.rs", &[], &[])]});
        mustard_core::io::project_map::write_text(root, &base.to_string()).unwrap();
    }

    /// A volta da onda 1 com `src/big.rs` trocado por `count` linhas novas e o
    /// mapa de depois com `tests` testes novos nele.
    fn back_with(root: &Path, count: usize, tests: usize) -> Value {
        let report = delivered(root, 1, "A onda mudou.", &["src/big.rs"]);
        write_lines(root, "src/big.rs", "new", count);
        let names: Vec<(String, u64)> = (0..tests).map(|n| (format!("case_{n}"), 3 + 2 * n as u64)).collect();
        let functions: Vec<(&str, u64)> = names.iter().map(|(name, line)| (name.as_str(), *line)).collect();
        let span: Vec<[u64; 2]> = if tests == 0 { vec![] } else { vec![[1, 4 + 2 * tests as u64]] };
        let after = json!({"modules": [module("src/big.rs", &functions, &span)]});
        round_with_mine(root, "x", Some(&report), &mine_giving(after))
    }

    /// A conferência do tamanho recusou a volta?
    fn refused(out: &Value) -> bool {
        out["reason"] == json!("round-after-wave")
    }

    #[test]
    fn a_wave_over_three_medians_is_refused_with_the_numbers() {
        // Com a mediana em 300 o limite é 900; com ela em 100, o piso de 600.
        // O mais novo tira 69 e põe 3637; os outros ficam um de cada lado do
        // limite.
        let cases = [(300, 3637, 69, true), (300, 901, 0, true), (300, 900, 0, false), (300, 700, 0, false), (100, 601, 0, true)];
        for (median, added, before, over) in cases {
            let dir = tempdir().unwrap();
            let root = dir.path();
            grown(root, 12, median, before, 0);
            let head = git_text(root, &["rev-parse", "HEAD"]);
            let out = back_with(root, added, 0);
            assert_eq!(refused(&out), over, "median {median}, +{added} -{before}: {out}");
            if !over {
                continue;
            }
            let hint = out["hint"].as_str().unwrap_or_default();
            let limit = 600.max(3 * median);
            let said = format!(
                "A onda 1 pôs {added} linhas e tirou {before}: o código cresceu {}. O limite é {limit}, o maior entre 600 e \
                 três vezes a mediana de {median} linhas postas por onda.",
                added - before
            );
            assert!(hint.contains(&said), "{hint}");
            assert!(hint.contains("mande o resto para `leftovers` da entrega"), "{hint}");
            assert!(hint.contains("Onda 1, rodada de conserto 1 de 2"), "{hint}");
            assert_eq!(git_text(root, &["rev-parse", "HEAD"]), head, "nothing committed: {out}");
        }
    }

    #[test]
    fn a_wave_under_the_floor_never_refuses() {
        // Três medianas de 100 são 300, mas abaixo de 600 linhas de
        // crescimento nenhuma onda volta.
        for (added, before) in [(540, 28), (253, 48), (600, 0)] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            grown(root, 12, 100, before, 0);
            let out = back_with(root, added, 0);
            assert_eq!(out["ok"], json!(true), "+{added} -{before}: {out}");
            let size = warning_of(&out, "wave-size");
            assert_eq!(size["hint"], json!(format!("onda 1: +{added} -{before}, 0 testes, 1 arquivos")), "{out}");
        }
    }

    #[test]
    fn a_rename_wave_that_adds_and_removes_the_same_never_refuses() {
        for (added, removed) in [(941, 941), (1774, 2164)] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            grown(root, 12, 100, removed, 0);
            let out = back_with(root, added, 0);
            assert_eq!(out["ok"], json!(true), "+{added} -{removed}: {out}");
            let size = warning_of(&out, "wave-size");
            assert_eq!(size["hint"], json!(format!("onda 1: +{added} -{removed}, 0 testes, 1 arquivos")), "{out}");
        }
    }

    #[test]
    fn no_history_means_no_size_finding() {
        // Com nove commits de ondas antigas o projeto não tem régua, e a onda
        // do painel de gasto só ganha a linha; com dez, ela volta.
        for (history, over) in [(9, false), (10, true)] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            grown(root, history, 100, 69, 0);
            let out = back_with(root, 3637, 7);
            assert_eq!(refused(&out), over, "{history} commits: {out}");
            if !over {
                assert_eq!(out["ok"], json!(true), "{out}");
                assert_eq!(warning_of(&out, "wave-size")["hint"], json!("onda 1: +3637 -69, 7 testes, 1 arquivos"), "{out}");
            }
        }
    }

    #[test]
    fn too_many_tests_for_the_criteria_is_refused() {
        // A onda aponta um critério; cada regra que a tarefa cobre soma mais
        // um. O limite é o maior entre 6 e dois por critério ou regra. A onda
        // cresce 540 linhas, mais que três medianas de 100 e menos que o piso:
        // só os testes a recusam.
        let cases = [(0, 6, None), (0, 7, Some(6)), (3, 8, None), (3, 9, Some(8))];
        for (rules, tests, over) in cases {
            let dir = tempdir().unwrap();
            let root = dir.path();
            grown(root, 12, 100, 0, rules);
            let out = back_with(root, 540, tests);
            assert_eq!(refused(&out), over.is_some(), "{rules} rules, {tests} tests: {out}");
            if let Some(limit) = over {
                let hint = out["hint"].as_str().unwrap_or_default();
                assert!(!hint.contains("o código cresceu"), "{hint}");
                let said = format!(
                    "A onda 1 traz {tests} testes novos para {} critérios e regras cobertos. O limite é {limit}, o maior entre 6 e dois por critério ou regra.",
                    rules + 1
                );
                assert!(hint.contains(&said), "{hint}");
            }
        }
    }

    #[test]
    fn the_third_round_turns_into_a_question_to_the_user() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        grown(root, 12, 300, 69, 0);
        // A entrega e as duas voltas de conserto, todas grandes demais.
        let rounds: Vec<Value> = (0..3).map(|_| back_with(root, 3637, 0)).collect();
        assert_eq!(rounds[1]["reason"], json!("round-after-wave"), "{}", rounds[1]);
        assert!(rounds[1]["hint"].as_str().unwrap_or_default().contains("Onda 1, rodada de conserto 2 de 2"), "{}", rounds[1]);
        assert_eq!(rounds[1].get("question"), None, "{}", rounds[1]);
        let last = &rounds[2];
        assert_eq!(last["reason"], json!("round-after-wave-limit"), "{last}");
        let question = last["question"].as_str().unwrap_or_default();
        assert!(question.contains("A onda 1 ainda tem o que consertar depois de 2 rodadas de conserto"), "{last}");
        assert!(last["hint"].as_str().unwrap_or_default().contains("o código cresceu 3568"), "{last}");
    }

    #[test]
    fn the_limit_uses_the_median_of_commits_that_the_wave_request_cites() {
        // Seis commits de uma onda com 300 linhas e seis de duas ondas com 600:
        // cada commit é uma entrega, a mediana é a média dos dois do meio, 450,
        // a mesma que o pedido da onda cita, e o limite é três vezes ela.
        let commits: Vec<(String, usize)> =
            (0..12).map(|n| if n % 2 == 0 { (format!("onda-{}", 100 + n), 300) } else { (format!("ondas-{}-{}", 100 + n, 200 + n), 600) }).collect();
        for (added, over) in [(1350, false), (1351, true)] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            grown_with(root, &commits, 0, 0);
            assert_eq!(mustard_core::io::wave_size::median_added(root, Locale::PtBr), Some(450));
            let out = back_with(root, added, 0);
            assert_eq!(refused(&out), over, "+{added}: {out}");
            if over {
                let hint = out["hint"].as_str().unwrap_or_default();
                assert!(hint.contains("O limite é 1350, o maior entre 600 e três vezes a mediana de 450 linhas postas por onda."), "{hint}");
            }
        }
    }
}
