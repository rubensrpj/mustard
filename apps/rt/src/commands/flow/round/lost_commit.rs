//! O commit que já leva o código de entregas que a spec não liga a commit
//! nenhum. A rodada que cai entre o commit e as gravações deixa o código no
//! git e a spec sem o evento `commit`: com a entrega oficial já gravada, o
//! fechamento recusa a obra; com a volta ainda sem assumir, a rodada seguinte
//! não teria o que comitar e o git recusaria o commit. Nos dois casos a rodada
//! seguinte acha esse commit e o anota, sem fazer commit novo.
//!
//! O commit é achado pelo conteúdo, nunca pela mensagem, que não leva nada do
//! Mustard: é um commit que a spec ainda não cita, feito depois da volta de
//! cada onda, que traz todos os arquivos das entregas dela — o de um
//! submódulo, pelo ponteiro dele — e nenhum outro arquivo. Na volta ainda sem
//! assumir, a onda só é procurada quando a junção não traz nada que o
//! repositório principal não tenha e os arquivos dela estão no HEAD como no
//! disco.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::{Block, BlockQuery, SpecEvent, SpecLog};
use mustard_core::domain::spec_state::PhaseWriter;
use crate::commands::git_settle::CopiesLock;
use serde_json::{Value, json};

use super::answer::RoundRefusal;
use super::commit::{commit_draft, copy_of, fork_point, git, needs_commit, on_spec_branch, repo_of};
use super::report::WaveReport;
use crate::commands::git_settle::{submodule_holding, submodules_of};
use crate::commands::spec_events::write::record;

/// Uma onda à procura do commit que já leva o código dela: os arquivos da
/// entrega e o instante, em segundos, da volta dela.
struct Pending {
    wave: u64,
    files: Vec<String>,
    since: i64,
}

/// Um commit achado para as ondas `waves`, com os arquivos das entregas delas.
pub(super) struct Found {
    pub(super) sha: String,
    pub(super) title: String,
    pub(super) waves: Vec<u64>,
    pub(super) files: Vec<String>,
}

/// O instante, em segundos, em que o evento `event` foi gravado.
fn seconds(event: &SpecEvent) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(event.at()).ok().map(|at| at.timestamp())
}

/// Os commits do checkout `root` que já levam as entregas `pending`, na ordem
/// do histórico, cada um com as ondas que leva. Só conta o commit que a spec
/// `log` ainda não cita, de depois da volta de cada onda dele, que traz todos
/// os arquivos de cada uma e nenhum arquivo de fora delas.
fn find_commits(root: &Path, log: &SpecLog, mut pending: Vec<Pending>) -> Vec<Found> {
    let Some(since) = pending.iter().map(|one| one.since).min() else {
        return Vec::new();
    };
    let cited: BTreeSet<&str> = log.events.iter().filter(|e| e.event_type == "commit").filter_map(|e| e.str_field("sha")).collect();
    let subs = submodules_of(root);
    let in_main = |file: &str| submodule_holding(&subs, file).map_or_else(|| file.to_string(), |(sub, _)| sub.to_string());
    // Sem a leitura de mudança de nome, o arquivo movido sai pelos dois
    // caminhos, o velho e o novo, como a cópia da onda os lista.
    let since = format!("--max-age={since}");
    let args = ["-c", "core.quotePath=false", "log", "--no-renames", &since, "--reverse", "--format=%x01%H%x09%ct%x09%s", "--name-only"];
    let history = git(root, &args).unwrap_or_default();
    let mut found = Vec::new();
    for entry in history.split('\u{1}').filter(|entry| !entry.trim().is_empty()) {
        let mut lines = entry.lines();
        let mut head = lines.next().unwrap_or_default().splitn(3, '\t');
        let (Some(sha), Some(at), Some(title)) = (head.next(), head.next(), head.next()) else {
            continue;
        };
        let at: i64 = at.parse().unwrap_or_default();
        if cited.contains(sha) {
            continue;
        }
        let changed: BTreeSet<&str> = lines.map(str::trim).filter(|line| !line.is_empty()).collect();
        let (taken, rest): (Vec<Pending>, Vec<Pending>) = std::mem::take(&mut pending)
            .into_iter()
            .partition(|one| one.since <= at && one.files.iter().all(|file| changed.contains(in_main(file).as_str())));
        pending = rest;
        let covered: BTreeSet<String> = taken.iter().flat_map(|one| one.files.iter().map(|file| in_main(file))).collect();
        if taken.is_empty() || !changed.iter().all(|path| covered.contains(*path)) {
            pending.extend(taken);
            continue;
        }
        let mut files: Vec<String> = Vec::new();
        for file in taken.iter().flat_map(|one| one.files.iter()) {
            if !files.contains(file) {
                files.push(file.clone());
            }
        }
        found.push(Found { sha: sha.to_string(), title: title.to_string(), waves: taken.iter().map(|one| one.wave).collect(), files });
    }
    found
}

/// As ondas de `waves` cujo código já está num commit que a spec `log` não
/// cita, cada commit com as ondas dele ([`find_commits`]): a rodada grava a
/// entrega e esse commit em vez de comitar de novo. Só é procurada a onda que
/// mudou arquivo, que a junção achou sem nada novo (`nothing_new`) e cujos
/// arquivos estão no HEAD como no disco; o instante é o da última volta dela.
/// A onda assim cujo código nenhum commit leva sozinho — comitado à mão junto
/// de outro arquivo, ou antes da volta — fica com o último commit que mudou os
/// arquivos dela depois do commit em que a cópia dela nasceu
/// ([`last_touching`]): o código já está no HEAD, e o commit novo que a rodada
/// tentaria o git recusaria por não haver nada a comitar. A onda sem cópia, ou
/// cujos arquivos ninguém mudou desde então, não mudou nada, e segue para o
/// commit, que diz por quê.
pub(super) fn already_committed(root: &Path, log: &SpecLog, waves: &[WaveReport], nothing_new: &BTreeSet<u64>) -> Vec<Found> {
    let subs = submodules_of(root);
    let in_head: Vec<&WaveReport> = waves
        .iter()
        .filter(|wave| needs_commit(&wave.files) && nothing_new.contains(&wave.wave))
        .filter(|wave| as_in_head(root, &subs, &wave.files))
        .collect();
    let pending = in_head
        .iter()
        .filter_map(|wave| {
            let since = wave.returns.iter().max().and_then(|id| log.get(*id)).and_then(seconds)?;
            Some(Pending { wave: wave.wave, files: wave.files.clone(), since })
        })
        .collect();
    let mut found = find_commits(root, log, pending);
    for wave in in_head {
        if found.iter().any(|one| one.waves.contains(&wave.wave)) {
            continue;
        }
        let fork = copy_of(log, wave.wave).and_then(|copy| fork_point(&copy, root).ok());
        if let Some((sha, title)) = fork.and_then(|fork| last_touching(root, &subs, &wave.files, &fork)) {
            found.push(Found { sha, title, waves: vec![wave.wave], files: wave.files.clone() });
        }
    }
    found
}

/// O último commit do checkout `root` depois de `fork` que mudou algum dos
/// arquivos `files` — o de um submódulo, pelo ponteiro dele —, com o título;
/// `None` quando o git não responde ou nenhum commit os mudou.
fn last_touching(root: &Path, subs: &[String], files: &[String], fork: &str) -> Option<(String, String)> {
    let mut paths: Vec<String> = Vec::new();
    for file in files {
        let path = submodule_holding(subs, file).map_or_else(|| file.clone(), |(sub, _)| sub.to_string());
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    let range = format!("{fork}..HEAD");
    let mut args = vec!["log", "-1", "--format=%H%x09%s", &range, "--"];
    args.extend(paths.iter().map(String::as_str));
    let line = git(root, &args).ok()?;
    let (sha, title) = line.trim().split_once('\t')?;
    Some((sha.to_string(), title.to_string()))
}

/// Os arquivos `files` do checkout `root` estão no disco como no HEAD do
/// repositório que guarda cada um; `false` quando o git não responde.
fn as_in_head(root: &Path, subs: &[String], files: &[String]) -> bool {
    let mut by_repo: Vec<(PathBuf, Vec<String>)> = Vec::new();
    for file in files {
        let (repo, inner) = repo_of(root, subs, file);
        match by_repo.iter_mut().find(|(known, _)| *known == repo) {
            Some((_, inners)) => inners.push(inner),
            None => by_repo.push((repo, vec![inner])),
        }
    }
    by_repo.iter().all(|(repo, inners)| {
        let mut args = vec!["status", "--porcelain", "--untracked-files=all", "--"];
        args.extend(inners.iter().map(String::as_str));
        git(repo, &args).is_ok_and(|status| status.trim().is_empty())
    })
}

/// As ondas do plano cuja entrega oficial mais recente mudou arquivo e que
/// nenhum evento `commit` cita, pela mesma leitura do fechamento, cada uma com
/// as ondas que ela conserta — da última volta que a entrega oficial
/// substituiu, porque a entrega oficial não as guarda — e com o instante
/// dessa volta: o commit dela só pode ter vindo depois.
fn orphans(log: &SpecLog) -> Vec<(Pending, Vec<u64>)> {
    let committed: BTreeSet<u64> = log
        .block(BlockQuery::Block(Block::Progress))
        .into_iter()
        .filter(|e| e.event_type == "commit")
        .flat_map(|e| e.ints("waves"))
        .collect();
    let planned = log.planned_waves();
    log.last_by_wave("delivered")
        .into_iter()
        .filter(|(wave, _)| planned.contains(wave) && !committed.contains(wave))
        .filter_map(|(wave, id)| {
            let official = log.get(id)?;
            let listed = official.fields.get("files").and_then(Value::as_array);
            let files: Vec<String> = listed.map(|all| all.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
            let last_return = official.replaced().into_iter().max().and_then(|id| log.get(id))?;
            let since = seconds(last_return)?;
            needs_commit(&files).then(|| (Pending { wave, files, since }, last_return.ints("fixes")))
        })
        .collect()
}

/// Grava na spec o commit que uma rodada anterior fez e não anotou, um por
/// commit achado ([`find_commits`]), e devolve o que gravou. Roda com a trava
/// das cópias presa (`_held`): nenhuma outra rodada comita nesse meio-tempo; e só no ramo da spec
/// ([`on_spec_branch`]). Sem commit achado, nada é gravado; nenhum commit novo
/// nasce no git.
pub(super) fn record_lost_commits(start: &Path, root: &Path, spec: &str, log: &SpecLog, _held: &CopiesLock) -> Result<Vec<Value>, RoundRefusal> {
    let (pending, fixes): (Vec<Pending>, BTreeMap<u64, Vec<u64>>) =
        orphans(log).into_iter().map(|(one, fixes)| { let wave = one.wave; (one, (wave, fixes)) }).unzip();
    // O commit é procurado no histórico do checkout: fora do ramo da spec, o
    // commit de outro ramo viraria o da onda, e a rodada recusa como recusaria
    // comitar ali.
    if !pending.is_empty() {
        on_spec_branch(root, log)?;
    }
    let mut recorded = Vec::new();
    for found in find_commits(root, log, pending) {
        let mut covered: Vec<u64> = Vec::new();
        for wave in found.waves.iter().flat_map(|wave| std::iter::once(*wave).chain(fixes.get(wave).into_iter().flatten().copied())) {
            if !covered.contains(&wave) {
                covered.push(wave);
            }
        }
        let draft = commit_draft(root, &found.sha, &found.title, &covered, &found.files);
        let written = record(start, spec, "commit", draft, PhaseWriter::Binary).map_err(RoundRefusal::Refused)?;
        recorded.push(json!({ "type": "commit", "id": written.written.id, "sha": found.sha, "waves": covered }));
    }
    Ok(recorded)
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
        git_at(root, &["commit", "-q", "-m", "docs: outra mensagem qualquer", "--", "src/a.rs"]);
        let sha = git_text(root, &["rev-parse", "HEAD"]);

        let out = round(root, "x", None);

        assert_eq!(out["ok"], json!(true), "{out}");
        let log_now = log();
        let commit = log_now.events.iter().find(|e| e.event_type == "commit").unwrap_or_else(|| panic!("{out}"));
        assert_eq!(commit.str_field("sha"), Some(sha.as_str()));
        assert_eq!(commit.str_field("title"), Some("docs: outra mensagem qualquer"));
        assert_eq!(commit.ints("waves"), vec![1]);
        assert_eq!(commit.fields["files"], json!(["src/a.rs"]));
        assert_eq!(git_text(root, &["rev-parse", "HEAD"]), sha, "no new commit in git");
        round(root, "x", None);
        assert_eq!(log().events.iter().filter(|e| e.event_type == "commit").count(), 1);
    }

    /// A rodada que caiu entre o commit e a gravação das entregas deixou o
    /// código da onda comitado e a volta sem assumir: a rodada seguinte acha
    /// esse commit pelo conteúdo — os arquivos da entrega, já no HEAD como a
    /// junção os deixaria, num commit depois da volta —, grava a entrega e o
    /// commit, e não tenta outro commit, que o git recusaria por não haver
    /// nada a comitar.
    #[test]
    fn a_round_that_fell_after_its_commit_records_the_delivery_without_committing_again() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let log = || -> SpecLog { store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap() };
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let copy = mustard_core::io::wave_prompt::slot_path(root, "x", 0);
        let joined = "fn one() { dois(); }\nfn dois() {}\n";
        std::fs::write(copy.join("src/a.rs"), joined).unwrap();
        let report = json!({"wave": 1, "text": "Saiu.", "files": ["src/a.rs"], "commit": "a entrega 1 sai"});
        assert_eq!(returned(root, report)["ok"], json!(true));
        std::fs::write(root.join("src/a.rs"), joined).unwrap();
        git_at(root, &["commit", "-q", "-m", "fix: um título qualquer", "--", "src/a.rs"]);
        let sha = git_text(root, &["rev-parse", "HEAD"]);

        let out = round(root, "x", None);

        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(git_text(root, &["rev-parse", "HEAD"]), sha, "no new commit in git: {out}");
        assert_eq!(delivered_count(root), 1, "{out}");
        let log_now = log();
        let commit = log_now.events.iter().find(|e| e.event_type == "commit").unwrap_or_else(|| panic!("{out}"));
        assert_eq!(commit.str_field("sha"), Some(sha.as_str()));
        assert_eq!(commit.str_field("title"), Some("fix: um título qualquer"));
        assert_eq!(commit.ints("waves"), vec![1]);
        assert_eq!(commit.fields["files"], json!(["src/a.rs"]));
    }

    /// A onda que move um arquivo traz o caminho velho e o novo, e o commit
    /// que a rodada que caiu fez com a mudança de nome é achado pelos dois:
    /// a rodada seguinte o grava, sem tentar outro commit.
    #[test]
    fn a_round_that_fell_after_committing_a_move_records_it_without_committing_again() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let log = || -> SpecLog { store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap() };
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let copy = mustard_core::io::wave_prompt::slot_path(root, "x", 0);
        git_at(&copy, &["mv", "src/a.rs", "src/soma.rs"]);
        let report = json!({"wave": 1, "text": "Saiu.", "files": ["src/a.rs", "src/soma.rs"], "commit": "a soma muda de arquivo"});
        assert_eq!(returned(root, report)["ok"], json!(true));
        git_at(root, &["mv", "src/a.rs", "src/soma.rs"]);
        git_at(root, &["commit", "-q", "-m", "refactor: a soma muda de arquivo"]);
        let sha = git_text(root, &["rev-parse", "HEAD"]);

        let out = round(root, "x", None);

        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(git_text(root, &["rev-parse", "HEAD"]), sha, "no new commit in git: {out}");
        let log_now = log();
        let commit = log_now.events.iter().find(|e| e.event_type == "commit").unwrap_or_else(|| panic!("{out}"));
        assert_eq!(commit.str_field("sha"), Some(sha.as_str()));
        let files: Vec<&str> = commit.fields["files"].as_array().unwrap().iter().filter_map(|f| f.as_str()).collect();
        assert!(files.contains(&"src/a.rs") && files.contains(&"src/soma.rs"), "{out}");
    }

    /// Fora do ramo da spec, a rodada não anota o commit perdido: um commit de
    /// outro ramo nunca vira o da onda. De volta ao ramo da spec, anota.
    #[test]
    fn a_lost_commit_is_recorded_only_on_the_spec_branch() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let log = || -> SpecLog { store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap() };
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        delivered(root, 1, "feito", &["src/a.rs"]);
        let back = log().events.iter().rev().find(|e| e.event_type == "delivered").map(|e| e.id).unwrap();
        seed_event(root, "x", "delivered",
            json!({"wave": 1, "text": "feito", "files": ["src/a.rs"], "replaces": [back], "author": "binary"}));
        git_at(root, &["checkout", "-q", "-b", "outro"]);
        git_at(root, &["commit", "-q", "-m", "docs: outra mensagem qualquer", "--", "src/a.rs"]);

        let refused = round(root, "x", None);
        assert_eq!(refused["reason"], json!("round-wrong-branch"), "{refused}");
        assert!(log().events.iter().all(|e| e.event_type != "commit"), "{refused}");
    }

    /// A volta ainda sem assumir cujo código já está no HEAD, comitado à mão
    /// junto de outro arquivo, não trava a rodada: sem commit que leve só a
    /// onda, vale o último commit que mudou os arquivos dela, e a rodada grava
    /// a entrega com ele, sem tentar outro commit.
    #[test]
    fn a_return_whose_code_is_already_in_head_with_other_files_does_not_lock_the_round() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let log = || -> SpecLog { store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap() };
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let copy = mustard_core::io::wave_prompt::slot_path(root, "x", 0);
        let joined = "fn one() { dois(); }\nfn dois() {}\n";
        std::fs::write(copy.join("src/a.rs"), joined).unwrap();
        let report = json!({"wave": 1, "text": "Saiu.", "files": ["src/a.rs"], "commit": "a entrega 1 sai"});
        assert_eq!(returned(root, report)["ok"], json!(true));
        std::fs::write(root.join("src/a.rs"), joined).unwrap();
        std::fs::write(root.join("src/b.rs"), "fn two() {}\n").unwrap();
        git_at(root, &["add", "src/a.rs", "src/b.rs"]);
        git_at(root, &["commit", "-q", "-m", "chore: junta à mão", "--", "src/a.rs", "src/b.rs"]);
        let sha = git_text(root, &["rev-parse", "HEAD"]);

        let out = round(root, "x", None);

        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(git_text(root, &["rev-parse", "HEAD"]), sha, "no new commit in git: {out}");
        assert_eq!(delivered_count(root), 1, "{out}");
        let log_now = log();
        let commit = log_now.events.iter().find(|e| e.event_type == "commit").unwrap_or_else(|| panic!("{out}"));
        assert_eq!(commit.str_field("sha"), Some(sha.as_str()));
        assert_eq!(commit.ints("waves"), vec![1]);
    }

    /// Sem commit no git que leve a entrega da rodada que caiu — o que traz o
    /// arquivo dela junto de um arquivo de fora, ou o que não traz o arquivo
    /// dela —, nada é gravado, seja qual for a mensagem.
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
        std::fs::write(root.join("src/b.rs"), "fn two() {}\n").unwrap();
        git_at(root, &["add", "src/b.rs"]);
        git_at(root, &["commit", "-q", "-m", "feat: a entrega 1 saiu", "--", "src/a.rs", "src/b.rs"]);
        std::fs::write(root.join("src/b.rs"), "fn three() {}\n").unwrap();
        git_at(root, &["commit", "-q", "-m", "feat: a entrega 1 saiu", "--", "src/b.rs"]);

        let out = round(root, "x", None);

        assert_eq!(out["ok"], json!(true), "{out}");
        assert!(log().events.iter().all(|e| e.event_type != "commit"), "{out}");
    }
}
