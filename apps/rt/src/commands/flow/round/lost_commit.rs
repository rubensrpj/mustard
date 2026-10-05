//! O commit que a rodada fez e não chegou a anotar: a rodada que cai entre o
//! commit e a gravação dele deixa a entrega oficial gravada, o commit no git
//! e nenhum evento `commit` na spec, e o fechamento recusa a obra. A rodada
//! seguinte acha esse commit no git, pelo título que ela mesma montaria, e o
//! anota, sem fazer commit novo.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mustard_core::domain::spec_events::{Block, BlockQuery, SpecLog};
use mustard_core::domain::spec_state::PhaseWriter;
use mustard_core::io::fs::lock::LockedFile;
use mustard_core::platform::i18n::Locale;
use serde_json::{json, Value};

use super::answer::RoundRefusal;
use super::commit::{commit_draft, commit_message, git, needs_commit};
use super::report::WaveReport;
use crate::commands::spec_events::write::record;

/// As ondas do plano cuja entrega oficial mais recente mudou arquivo e que
/// nenhum evento `commit` cita, pela mesma leitura do fechamento. Cada uma vem
/// com a volta que a rodada assumiu — o resumo do commit e as ondas que ela
/// conserta vêm da última volta que a entrega oficial substituiu, porque a
/// entrega oficial não os guarda — e com o instante, em segundos, do envio
/// que a despachou: o commit dela só pode ter vindo depois.
fn orphans(log: &SpecLog) -> BTreeMap<u64, (WaveReport, i64)> {
    let committed: BTreeSet<u64> = log
        .block(BlockQuery::Block(Block::Progress))
        .into_iter()
        .filter(|e| e.event_type == "commit")
        .flat_map(|e| e.ints("waves"))
        .collect();
    let planned = log.planned_waves();
    let dispatched = log.last_dispatch_by_wave();
    log.last_by_wave("delivered")
        .into_iter()
        .filter(|(wave, _)| planned.contains(wave) && !committed.contains(wave))
        .filter_map(|(wave, id)| {
            let official = log.get(id)?;
            let listed = official.fields.get("files").and_then(Value::as_array);
            let files = listed.map(|all| all.iter().filter_map(Value::as_str).map(str::to_string).collect());
            let last_return = official.replaced().into_iter().max().and_then(|id| log.get(id))?;
            let send = dispatched.get(&wave).and_then(|id| log.get(*id))?;
            let sent = chrono::DateTime::parse_from_rfc3339(send.at()).ok()?.timestamp();
            let report = WaveReport {
                wave,
                delivered: official.str_field("text").unwrap_or_default().to_string(),
                files: files.unwrap_or_default(),
                commit: last_return.str_field("commit").map(str::to_string),
                proofs: Vec::new(),
                fixes: last_return.ints("fixes"),
                replan: None,
                undone: Vec::new(),
                leftovers: Vec::new(),
                agreed: Vec::new(),
                returns: Vec::new(),
                usage: Default::default(),
            };
            needs_commit(&report.files).then_some((wave, (report, sent)))
        })
        .collect()
}

/// As ondas do escopo do título `title` (`feat(onda-7): …`,
/// `fix(ondas-8-6): …`), na ordem em que o título as cita. `None` quando o
/// título não tem escopo de ondas, ou quando o escopo conta ondas que não
/// cita (`ondas-1-2+9`): esse título não diz quais ondas o commit leva.
fn scope_waves(title: &str) -> Option<Vec<u64>> {
    let (head, _) = title.split_once("): ")?;
    let (_, scope) = head.split_once('(')?;
    let waves: Vec<u64> = scope.split('-').skip(1).map(|part| part.parse().ok()).collect::<Option<_>>()?;
    (!waves.is_empty()).then_some(waves)
}

/// Grava na spec o commit que uma rodada anterior fez e não anotou, um por
/// commit achado, e devolve o que gravou. Roda com a trava do passo do git
/// presa (`_held`). O commit casa quando veio depois do envio de cada onda do
/// escopo dele, cada uma é órfã, o título remontado das últimas voltas delas
/// ([`commit_message`]) é igual ao do commit e ele traz todos os arquivos das
/// entregas — o arquivo de um submódulo vem no ponteiro dele. Sem commit que
/// case, nada é gravado; nenhum commit novo nasce no git.
pub(super) fn record_lost_commits(
    start: &Path,
    root: &Path,
    spec: &str,
    log: &SpecLog,
    lang: Locale,
    _held: &LockedFile,
) -> Result<Vec<Value>, RoundRefusal> {
    let mut orphans = orphans(log);
    let Some(since) = orphans.values().map(|(_, sent)| *sent).min() else {
        return Ok(Vec::new());
    };
    let listed = git(root, &["log", &format!("--max-age={since}"), "--reverse", "--format=%H%x09%ct%x09%s"])
        .unwrap_or_default();
    let mut recorded = Vec::new();
    for line in listed.lines() {
        let mut parts = line.splitn(3, '\t');
        let (Some(sha), Some(at), Some(title)) = (parts.next(), parts.next(), parts.next()) else { continue };
        let at: i64 = at.parse().unwrap_or_default();
        let Some(waves) = scope_waves(title) else { continue };
        if !waves.iter().all(|wave| orphans.get(wave).is_some_and(|(_, sent)| *sent <= at)) {
            continue;
        }
        let (reports, sent): (Vec<WaveReport>, Vec<i64>) =
            waves.iter().filter_map(|wave| orphans.remove(wave)).unzip();
        let Some(files) = carried(root, sha, title, &reports, lang) else {
            orphans.extend(reports.into_iter().zip(sent).map(|(report, sent)| (report.wave, (report, sent))));
            continue;
        };
        let mut covered: Vec<u64> = Vec::new();
        for wave in reports.iter().flat_map(|report| std::iter::once(report.wave).chain(report.fixes.iter().copied())) {
            if !covered.contains(&wave) {
                covered.push(wave);
            }
        }
        let draft = commit_draft(root, sha, title, &covered, &files);
        let written = record(start, spec, "commit", draft, PhaseWriter::Binary).map_err(RoundRefusal::Refused)?;
        recorded.push(json!({ "type": "commit", "id": written.written.id, "sha": sha, "waves": covered }));
    }
    Ok(recorded)
}

/// Os arquivos das entregas `reports`, na ordem delas, quando o commit `sha`
/// é o que a rodada fez para elas: o título remontado ([`commit_message`]) é
/// igual a `title` e o commit traz cada arquivo — o de um submódulo, pelo
/// ponteiro dele. `None` quando não casa.
fn carried(root: &Path, sha: &str, title: &str, reports: &[WaveReport], lang: Locale) -> Option<Vec<String>> {
    let rebuilt = commit_message(reports, lang).ok().flatten()?.0;
    if rebuilt != title {
        return None;
    }
    let changed = git(root, &["diff-tree", "--no-commit-id", "--name-only", "-r", sha]).ok()?;
    let mut files: Vec<String> = Vec::new();
    for file in reports.iter().flat_map(|report| report.files.iter()) {
        if !files.contains(file) {
            files.push(file.clone());
        }
    }
    let in_commit = |file: &String| changed.lines().any(|path| file == path || file.starts_with(&format!("{path}/")));
    files.iter().all(in_commit).then_some(files)
}

#[cfg(test)]
mod tests {
    use mustard_core::domain::spec_events::SpecLog;
    use mustard_core::io::spec_events as store;
    use serde_json::json;
    use tempfile::tempdir;

    use crate::commands::flow::round::tests::*;
    use crate::shared::spec_state::seed_event;

    /// A rodada que caiu entre o commit e a gravação dele deixou a entrega
    /// oficial e o commit no git, sem evento: a rodada seguinte anota esse
    /// commit, sem fazer outro, e segue; a próxima não anota de novo.
    #[test]
    fn a_round_records_the_commit_a_fallen_round_left_unrecorded() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let log = || -> SpecLog { store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap() };
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        delivered(root, 1, "feito", &["src/a.rs"]);
        let back = log().events.iter().rev().find(|e| e.event_type == "delivered").map(|e| e.id).unwrap();
        seed_event(root, "x", "delivered",
            json!({"wave": 1, "text": "feito", "files": ["src/a.rs"], "replaces": [back], "author": "binary"}));
        git_at(root, &["commit", "-q", "-m", "feat(onda-1): a onda 1 saiu", "--", "src/a.rs"]);
        let sha = git_text(root, &["rev-parse", "HEAD"]);

        let out = round(root, "x", None);

        assert_eq!(out["ok"], json!(true), "{out}");
        let log_now = log();
        let commit = log_now.events.iter().find(|e| e.event_type == "commit").unwrap_or_else(|| panic!("{out}"));
        assert_eq!(commit.str_field("sha"), Some(sha.as_str()));
        assert_eq!(commit.str_field("title"), Some("feat(onda-1): a onda 1 saiu"));
        assert_eq!(commit.ints("waves"), vec![1]);
        assert_eq!(commit.fields["files"], json!(["src/a.rs"]));
        assert_eq!(git_text(root, &["rev-parse", "HEAD"]), sha, "no new commit in git");
        round(root, "x", None);
        assert_eq!(log().events.iter().filter(|e| e.event_type == "commit").count(), 1);
    }

    /// Sem commit no git que case com a rodada que caiu — outro resumo com o
    /// mesmo escopo, ou o título certo sem os arquivos da entrega —, nada é
    /// gravado.
    #[test]
    fn a_round_records_no_commit_when_git_has_none_matching_the_fallen_round() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let log = || -> SpecLog { store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap() };
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        delivered(root, 1, "feito", &["src/a.rs"]);
        let back = log().events.iter().rev().find(|e| e.event_type == "delivered").map(|e| e.id).unwrap();
        seed_event(root, "x", "delivered",
            json!({"wave": 1, "text": "feito", "files": ["src/a.rs"], "replaces": [back], "author": "binary"}));
        git_at(root, &["commit", "-q", "-m", "feat(onda-1): outra coisa", "--", "src/a.rs"]);
        std::fs::write(root.join("src/b.rs"), "fn two() {}\n").unwrap();
        git_at(root, &["add", "src/b.rs"]);
        git_at(root, &["commit", "-q", "-m", "feat(onda-1): a onda 1 saiu", "--", "src/b.rs"]);

        let out = round(root, "x", None);

        assert_eq!(out["ok"], json!(true), "{out}");
        assert!(log().events.iter().all(|e| e.event_type != "commit"), "{out}");
    }
}
