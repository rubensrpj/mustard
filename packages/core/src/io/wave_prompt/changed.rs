//! O que mudou, no git, nos arquivos de cada tarefa de uma onda depois que o
//! texto dela foi escrito. Quando um commit toca o arquivo de uma tarefa
//! depois disso, ela pode já estar feita ou ter mudado, e o pedido da onda
//! avisa o agente com os commits e os arquivos, para ele conferir no código
//! antes de mudar.
//!
//! O texto vigente da tarefa vale pela versão mais antiga que o repete: a
//! que só põe a tarefa numa onda, ou que mexe no que o texto não diz, herda
//! a hora da anterior; contada, ela esconderia todo commit anterior à
//! própria rodada. A versão que reescreve o texto, os arquivos ou a parte do
//! agente já leva em conta o código daquele instante, e o aviso cala até um
//! commit novo. Sem git, sem arquivo declarado ou sem hora legível, a tarefa
//! fica de fora.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use crate::domain::spec_events::{SpecEvent, SpecLog};
use crate::domain::wave_prompt::TaskChange;

/// Quantos commits o aviso cita, no máximo, do mais novo ao mais velho.
const COMMITS_SHOWN: &str = "5";

/// O que mudou nos arquivos de cada tarefa de `tasks` depois do texto dela,
/// pelo código da tarefa (`codes`); a tarefa sem mudança fica de fora.
pub(super) fn since_text(
    root: &Path,
    log: &SpecLog,
    tasks: &[&SpecEvent],
    codes: &BTreeMap<u64, String>,
) -> BTreeMap<String, TaskChange> {
    tasks
        .iter()
        .filter_map(|task| {
            let code = codes.get(&task.id).cloned().unwrap_or_else(|| task.id.to_string());
            Some((code, changed_after_text(root, log, task)?))
        })
        .collect()
}

/// Os arquivos que a tarefa declara.
fn declared_files(task: &SpecEvent) -> Vec<String> {
    let files = task.fields.get("files").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    files.iter().filter_map(|file| file.get("path").and_then(Value::as_str)).map(str::to_string).collect()
}

/// A hora, em segundos, em que o texto vigente da tarefa foi escrito: a da
/// versão mais antiga da cadeia que tem o mesmo texto, os mesmos arquivos e a
/// mesma parte do agente. `None` sem hora legível.
fn written_at(log: &SpecLog, task: &SpecEvent) -> Option<i64> {
    let files = declared_files(task);
    let mut written = task;
    while let Some(previous) = written.replaced().first().and_then(|id| log.get(*id)) {
        let same = previous.fields.get("text") == written.fields.get("text")
            && declared_files(previous) == files
            && previous.fields.get("agent") == written.fields.get("agent");
        if !same {
            break;
        }
        written = previous;
    }
    chrono::DateTime::parse_from_rfc3339(written.at().trim()).ok().map(|at| at.timestamp())
}

/// Os commits da branch que tocam um arquivo declarado da tarefa depois do
/// texto dela, e os arquivos que eles tocaram; `None` quando nenhum.
fn changed_after_text(root: &Path, log: &SpecLog, task: &SpecEvent) -> Option<TaskChange> {
    let files = declared_files(task);
    if files.is_empty() {
        return None;
    }
    let written = written_at(log, task)?;
    let mut args = vec!["-c", "core.quotePath=false", "log", "-n", COMMITS_SHOWN, "--format=%x01%ct %h", "--name-only", "HEAD", "--"];
    args.extend(files.iter().map(String::as_str));
    let history = crate::platform::git::run(root, &args).out()?;
    let mut change = TaskChange::default();
    for commit in history.split('\u{1}').filter(|commit| !commit.trim().is_empty()) {
        let mut lines = commit.lines();
        let Some((seconds, hash)) = lines.next().and_then(|head| head.split_once(' ')) else { continue };
        if !seconds.parse::<i64>().is_ok_and(|at| at > written) {
            continue;
        }
        change.commits.push(hash.to_string());
        for file in lines.map(str::trim).filter(|file| !file.is_empty()) {
            if !change.files.iter().any(|seen| seen == file) {
                change.files.push(file.to_string());
            }
        }
    }
    (!change.commits.is_empty()).then_some(change)
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use serde_json::json;
    use tempfile::tempdir;

    use super::*;
    use crate::domain::spec_events::{normalize, parse_log, render_line, stamp};
    use crate::io::wave_prompt::{prompts, Flight};
    use crate::platform::i18n::Locale;

    /// A hora em que o texto da tarefa foi escrito, em segundos.
    const TEXT_AT: i64 = 1_790_000_000;

    /// O que o pedido diz sob a tarefa cujo arquivo mudou depois do texto.
    const WARNING: &str = "O git mudou o arquivo desta tarefa depois que o texto dela foi escrito";

    fn git(dir: &Path, args: &[&str], at: Option<&str>) {
        let mut command = Command::new("git");
        command.args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"]).args(args).current_dir(dir);
        if let Some(at) = at {
            command.env("GIT_AUTHOR_DATE", at).env("GIT_COMMITTER_DATE", at);
        }
        let out = command.output().expect("spawn git");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// Um repositório com `src/a.rs` e `src/b.rs` comitados mil segundos
    /// antes do texto.
    fn repository() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        git(dir.path(), &["init", "-q"], None);
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        for file in ["src/a.rs", "src/b.rs"] {
            std::fs::write(dir.path().join(file), "fn one() {}\n").unwrap();
        }
        git(dir.path(), &["add", "-A"], None);
        git(dir.path(), &["commit", "-q", "-m", "inicio"], Some(&format!("@{} +0000", TEXT_AT - 1_000)));
        dir
    }

    /// Um commit que muda `file`, `after` segundos depois do texto; devolve
    /// o hash curto.
    fn change(dir: &Path, file: &str, after: i64) -> String {
        std::fs::write(dir.join(file), format!("fn two() {{}} // {after}\n")).unwrap();
        git(dir, &["add", "-A"], None);
        git(dir, &["commit", "-q", "-m", "muda"], Some(&format!("@{} +0000", TEXT_AT + after)));
        let out = Command::new("git").args(["rev-parse", "--short", "HEAD"]).current_dir(dir).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A hora `after` segundos depois do texto, como o arquivo de eventos a escreve.
    fn at(after: i64) -> String {
        chrono::DateTime::from_timestamp(TEXT_AT + after, 0).unwrap().to_rfc3339()
    }

    /// Um arquivo de eventos escrito à mão, cada evento com a hora dele; o
    /// número de cada um é a posição na lista, a partir de 1.
    fn log(events: &[(&str, String, Value)]) -> SpecLog {
        let mut content = String::new();
        for (i, (event_type, at, body)) in events.iter().enumerate() {
            let mut map = normalize(body.as_object().cloned().unwrap_or_default(), event_type);
            map.insert("type".into(), json!(event_type));
            content.push_str(&render_line(&stamp(map, i as u64 + 1, None, at)));
            content.push('\n');
        }
        parse_log(&content)
    }

    /// A onda 1, com a tarefa que declara `files`, escrita no instante do texto.
    fn plan(files: &[&str]) -> SpecLog {
        let declared: Vec<Value> = files.iter().map(|path| json!({"path": path})).collect();
        log(&[
            ("wave", at(0), json!({"n": 1, "text": "Uma.", "criteria": [], "done_when": "pronto"})),
            ("task", at(0), json!({"wave": 1, "text": "Somar.", "files": declared})),
        ])
    }

    /// O pedido da onda 1, com a onda `running` ou fora de voo.
    fn request(root: &Path, log: &SpecLog, running: bool) -> String {
        let flight = Flight { running: if running { [1].into() } else { Default::default() }, ..Flight::default() };
        prompts(root, "teste", log, Locale::PtBr, &flight).remove(0).text
    }

    #[test]
    fn a_commit_on_the_task_file_after_its_text_reaches_the_request_of_the_wave_that_is_out() {
        let repo = repository();
        let hash = change(repo.path(), "src/a.rs", 1);
        let log = plan(&["src/a.rs", "src/b.rs"]);
        let text = request(repo.path(), &log, true);
        assert!(text.contains(&format!("{WARNING} (commits: {hash}; arquivos: `src/a.rs`).")), "{text}");
    }

    #[test]
    fn the_wave_that_is_not_out_gets_no_line() {
        let repo = repository();
        change(repo.path(), "src/a.rs", 1);
        let log = plan(&["src/a.rs"]);
        assert!(!request(repo.path(), &log, false).contains(WARNING));
    }

    #[test]
    fn a_commit_before_the_text_or_on_another_file_or_in_the_same_second_is_not_listed() {
        let repo = repository();
        change(repo.path(), "src/a.rs", -10);
        change(repo.path(), "src/b.rs", 1);
        let log = plan(&["src/a.rs"]);
        assert!(!request(repo.path(), &log, true).contains(WARNING));
        change(repo.path(), "src/a.rs", 0);
        assert!(!request(repo.path(), &log, true).contains(WARNING));
    }

    /// As mudanças de cada tarefa do arquivo de eventos `log`, pelo código dela.
    fn changes(root: &Path, log: &SpecLog) -> BTreeMap<String, TaskChange> {
        let tasks: Vec<&SpecEvent> = log.visible().into_iter().filter(|e| e.event_type == "task").collect();
        since_text(root, log, &tasks, &log.codes())
    }

    #[test]
    fn a_version_that_only_moves_the_task_into_a_wave_keeps_the_time_of_the_text() {
        let repo = repository();
        let hash = change(repo.path(), "src/a.rs", 10);
        let body = json!({"text": "Somar.", "files": [{"path": "src/a.rs"}]});
        let mut placed = body.clone();
        placed["wave"] = json!(1);
        placed["replaces"] = json!(1);
        let log = log(&[("task", at(0), body), ("task", at(100), placed)]);
        let found = changes(repo.path(), &log);
        assert_eq!(found.values().map(|change| change.commits.clone()).collect::<Vec<_>>(), vec![vec![hash]]);
    }

    #[test]
    fn a_version_that_rewrites_the_agent_part_silences_the_commits_before_it() {
        let repo = repository();
        change(repo.path(), "src/a.rs", 10);
        let body = json!({"text": "Somar.", "files": [{"path": "src/a.rs"}]});
        let mut checked = body.clone();
        checked["agent"] = json!("- a soma já mora em src/a.rs");
        checked["replaces"] = json!(1);
        let log = log(&[("task", at(0), body), ("task", at(100), checked)]);
        assert!(changes(repo.path(), &log).is_empty());
    }

    #[test]
    fn a_task_without_files_or_outside_git_is_left_out() {
        let repo = repository();
        change(repo.path(), "src/a.rs", 1);
        assert!(changes(repo.path(), &plan(&[])).is_empty());
        let outside = tempdir().unwrap();
        assert!(changes(outside.path(), &plan(&["src/a.rs"])).is_empty());
    }
}
