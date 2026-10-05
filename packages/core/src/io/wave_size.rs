//! O tamanho que uma entrega costuma ter neste projeto: a mediana das linhas
//! postas pelos commits que a rodada fez, lida do git. O pedido da onda a
//! cita, para o agente pôr só o que a tarefa pede.
//!
//! O commit da rodada traz no título o escopo das ondas que levou
//! (`feat(onda-12): …`, `fix(ondas-3-4): …`), no idioma do texto do projeto;
//! é por ele que o git o acha. Cada commit é uma entrega, e as linhas postas
//! dele são as do `git log --numstat`.
//!
//! Com menos de [`MIN_COMMITS`] commits da rodada na história, não há
//! mediana: pouca amostra não é régua.

use std::path::Path;

use crate::platform::git;
use crate::platform::i18n::{translate, Locale};

/// Quantos commits da rodada, dos mais novos, entram na conta.
const WINDOW: usize = 30;

/// Com menos commits da rodada que isto, a mediana não sai.
const MIN_COMMITS: usize = 10;

/// A mediana das linhas postas pelos últimos commits da rodada em `root`,
/// no escopo que o idioma `lang` dá ao título deles. `None` sem git, ou com
/// menos de [`MIN_COMMITS`] commits da rodada.
#[must_use]
pub fn median_added(root: &Path, lang: Locale) -> Option<u64> {
    let scope = |key| translate(key, lang).replace("{waves}", "");
    let title = format!("^(feat|fix)\\(({}|{})[0-9]", scope("round.commit.scope.one"), scope("round.commit.scope.many"));
    let window = WINDOW.to_string();
    let args = ["log", "-n", &window, "-E", "--grep", &title, "--no-renames", "--numstat", "--format=%x01", "HEAD"];
    let history = git::run(root, &args).out()?;
    let mut added: Vec<u64> = history
        .split('\u{1}')
        .filter(|commit| !commit.trim().is_empty())
        .map(|commit| {
            // O arquivo binário vem como `-` e não conta.
            commit.lines().filter_map(|line| line.split('\t').next()?.parse::<u64>().ok()).sum()
        })
        .collect();
    if added.len() < MIN_COMMITS {
        return None;
    }
    added.sort_unstable();
    let middle = added.len() / 2;
    Some(u64::midpoint(added[middle], added[(added.len() - 1) / 2]))
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use tempfile::{tempdir, TempDir};

    use super::*;
    use crate::domain::spec_events::parse_log;
    use crate::io::wave_prompt::{prompts, Flight};

    /// Um repositório em que cada título de `titles`, do mais velho ao mais
    /// novo, é um commit que põe tantas linhas quanto o número que o título
    /// traz no fim, depois de `#`.
    fn history(titles: &[String]) -> TempDir {
        let dir = tempdir().unwrap();
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
                .args(args)
                .current_dir(dir.path())
                .output()
                .expect("spawn git");
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        git(&["init", "-q"]);
        for (at, title) in titles.iter().enumerate() {
            let lines: usize = title.rsplit('#').next().unwrap().parse().unwrap();
            std::fs::write(dir.path().join(format!("f{at}.txt")), "linha\n".repeat(lines)).unwrap();
            git(&["add", "-A"]);
            git(&["commit", "-q", "-m", title]);
        }
        dir
    }

    #[test]
    fn the_median_is_taken_over_the_round_commits_of_the_language() {
        // Doze commits da rodada de 10 a 120 linhas, com dois que não são dela.
        let mut twelve: Vec<String> = (1..=12).map(|n| format!("feat(onda-{n}): entrega #{}", n * 10)).collect();
        twelve.insert(5, "docs: texto solto #5000".to_string());
        twelve.push("fix: conserto fora da rodada #7000".to_string());
        // Dez commits de várias ondas, no escopo em inglês.
        let mut english: Vec<String> = (1..=9).map(|n| format!("feat(waves-{n}-{}): delivery #{}", n + 1, n * 10)).collect();
        english.push("fix(wave-10): delivery #100".to_string());
        let few: Vec<String> = (1..MIN_COMMITS).map(|n| format!("feat(onda-{n}): entrega #10")).collect();
        for (titles, lang, expected) in [
            (&twelve, Locale::PtBr, Some(65)),
            (&english, Locale::EnUs, Some(55)),
            (&english, Locale::PtBr, None),
            (&few, Locale::PtBr, None),
        ] {
            assert_eq!(median_added(history(titles).path(), lang), expected, "{titles:?} {lang:?}");
        }
    }

    #[test]
    fn only_the_request_of_a_wave_that_is_out_carries_the_median() {
        let titles: Vec<String> = (1..=MIN_COMMITS).map(|n| format!("feat(onda-{n}): entrega #10")).collect();
        let dir = history(&titles);
        let events = |n: u64| {
            format!(
                r#"{{"v":1,"id":{a},"code":"MSTD-WAVE-000{n}","at":"2026-09-15T10:00:00-03:00","author":"binary","type":"wave","n":{n},"text":"A onda","criteria":[],"done_when":"passa"}}
{{"v":1,"id":{b},"code":"MSTD-TASK-000{n}","at":"2026-09-15T10:00:00-03:00","author":"binary","type":"task","wave":{n},"text":"Somar","files":[{{"path":"src/a.rs"}}]}}
"#,
                a = n * 2 - 1,
                b = n * 2,
            )
        };
        let log = parse_log(&(events(1) + &events(2)));
        let requests = |running: &[u64]| -> Vec<String> {
            let flight = Flight { running: running.iter().copied().collect(), ..Flight::default() };
            prompts(dir.path(), "teste", &log, Locale::PtBr, &flight).into_iter().map(|built| built.text).collect()
        };
        let line = "A mediana das entregas deste projeto é de 10 linhas postas";
        let out = requests(&[1]);
        assert!(out[0].contains(line), "{}", out[0]);
        assert!(!out[1].contains(line), "{}", out[1]);
        assert!(requests(&[]).iter().all(|text| !text.contains(line)));
    }
}
