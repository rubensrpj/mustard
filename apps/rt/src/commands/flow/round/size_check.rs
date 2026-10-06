//! O tamanho de cada onda: as linhas que ela pôs, as que tirou e os arquivos
//! que mudou. A conferência depois da onda o mede, com o disco já juntado.
//! Ele vira a linha de tamanho, que vai à resposta da rodada e ao corpo do
//! commit para quem conduz e quem lê o histórico verem o tamanho de cada
//! entrega. A linha é só dado: a rodada nunca devolve uma onda pelo tamanho,
//! e a onda pronta e testada sai inteira no commit, com quantas linhas e
//! quantos testes ela trouxer.
//!
//! - Postas e tiradas: o `git diff --numstat` do último commit contra o disco,
//!   nos arquivos da onda, repositório por repositório; o arquivo novo, que o
//!   git ainda não rastreia, conta todas as linhas como postas. O arquivo que
//!   duas ondas da rodada mudaram conta nas duas.
//! - O arquivo apagado não põe nada: só tira as linhas que tinha.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mustard_core::platform::i18n::{translate, Locale};

use super::commit::{check_commit_text, git, repo_of};
use crate::commands::git_settle::submodules_of;

/// O tamanho de uma onda: o que a linha de tamanho mostra.
pub(super) struct Size {
    wave: u64,
    added: u64,
    removed: u64,
    files: usize,
}

/// O tamanho de cada onda de `changed`, pelos arquivos que ela mudou, em
/// `root` com a junção já no disco.
pub(super) fn measure(root: &Path, changed: &[(u64, Vec<String>)]) -> Vec<Size> {
    let subs = submodules_of(root);
    changed
        .iter()
        .map(|(wave, files)| {
            let (added, removed) = moved_lines(root, &subs, files);
            Size { wave: *wave, added, removed, files: files.len() }
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
                .replace("{files}", &size.files.to_string());
            (size.wave, line)
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
    // Cada trecho que acaba em quebra é uma linha, e o último, sem quebra, também.
    u64::try_from(bytes.split_inclusive(|byte| *byte == b'\n').count()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};
    use tempfile::tempdir;

    use super::super::imports_check::tests::mine_giving;
    use super::super::tests::{approved, delivered, git_at, git_text, last_commit, round, round_with_mine, warning_of};
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
    fn size_line_counts_added_removed_and_files_of_the_wave() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let base = json!({"modules": [module("src/a.rs", &[("sum", 1)], &[]), module("src/b.rs", &[("old", 1)], &[])]});
        project(root, &base);
        // A onda põe uma linha em `src/a.rs`, troca a segunda de `src/b.rs` e
        // cria `tests/fresh.rs`, com três linhas que o git ainda não rastreia;
        // os testes novos que o mapa de depois traz não entram na linha.
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
        assert_eq!(size["hint"], json!("onda 1: +6 -1, 3 arquivos"), "{out}");
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
        assert!(body.ends_with("\n\n- onda 1: +4 -0, 3 arquivos"), "{body}");
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
        let lines = lines(&measure(root, &[(1, vec!["gone.rs".to_string()])]), Locale::EnUs);
        assert_eq!(lines, vec![(1, "wave 1: +0 -3, 1 files".to_string())]);
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
        let sizes = vec![(1, "onda 1: +2 -0, 2 arquivos".to_string())];
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

    /// Um projeto com a onda 1 mudando `src/big.rs`, de uma linha no commit,
    /// e doze commits de ondas antigas que põem 100 linhas cada: história
    /// bastante para uma mediana do tamanho das entregas. O mapa da base traz
    /// o `src/big.rs` sem declaração nenhuma.
    fn with_history(root: &Path) {
        approved(root, "x", &[(1, &["src/big.rs"], &[])]);
        std::fs::create_dir_all(root.join("history")).unwrap();
        for n in 0..12 {
            write_lines(root, &format!("history/w{n}.txt"), "x", 100);
            git_at(root, &["add", "--", "history"]);
            git_at(root, &["commit", "-q", "-m", &format!("feat(onda-{}): onda antiga", 100 + n)]);
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

    #[test]
    fn a_wave_far_over_the_old_limits_is_committed_without_a_size_finding() {
        // Doze entregas de 100 linhas dão ao projeto uma mediana de tamanho,
        // e ainda assim a onda que põe 2400 linhas e 20 testes novos sai
        // inteira no commit da rodada, só com a linha de tamanho.
        let dir = tempdir().unwrap();
        let root = dir.path();
        with_history(root);
        let head = git_text(root, &["rev-parse", "HEAD"]);
        let out = back_with(root, 2400, 20);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_ne!(git_text(root, &["rev-parse", "HEAD"]), head, "the round commits the wave: {out}");
        assert_eq!(warning_of(&out, "wave-size")["hint"], json!("onda 1: +2400 -1, 1 arquivos"), "{out}");
    }
}
