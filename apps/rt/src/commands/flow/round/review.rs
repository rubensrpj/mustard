//! A revisão olha a cópia de verdade, e não o relato do agente. O recibo
//! nativo da leitura e o aceite do condutor se prendem à impressão digital
//! do diff da onda contra a base dela: os arquivos mudados e o conteúdo. Nem
//! `USAGE` nem a compilação são revisão. Uma onda independente segue
//! enquanto outra espera.
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::domain::spec_state::{PhaseWriter, SpecState};
use mustard_core::io::{sha256::Sha256, spec_events as store};
use mustard_core::platform::i18n::Locale;
use serde_json::{Value, json};

use super::RoundRefusal;
use super::commit::{copy_of, fork_point, format_round_files, git, real_changed_files, repo_of};
use super::report::{WaveReport, returned_waves};
use super::stops::{HeldReturn, hold_refused};
use crate::commands::git_settle::submodules_of;
use crate::commands::spec_events::write::record;
use crate::shared::spec_state::DiskSpecState;

pub(crate) struct Approval {
    pub wave: u64,
    pub fingerprint: String,
    pub note: String,
}

pub(crate) fn view_at(start: &Path, named: Option<&str>, wave: u64) -> Value {
    let project = crate::commands::spec_events::project(start);
    let result = (|| {
        let spec = named.map(str::to_owned).or_else(|| DiskSpecState::new(start).active(None))
            .ok_or_else(|| "No current spec".to_string())?;
        let _lock = super::commit::git_lock(&project.root).map_err(|e| e.message(project.lang))?;
        let path = store::spec_file(&project.root, &spec).map_err(|e| e.message(project.lang))?;
        let log = store::read(&path).map_err(|e| e.message(project.lang))?.ok_or("Spec not found")?;
        let (mut waves, _) = returned_waves(&log);
        let returned = waves.iter_mut().find(|w| w.wave == wave).ok_or("No pending delivery for this wave")?;
        let dir = review_dir(&project.root, &log, wave);
        if let Some(actual) = real_changed_files(&project.root, &log, wave).filter(|files| !files.is_empty()) {
            returned.files = actual;
        }
        validate_paths(std::slice::from_ref(returned)).map_err(|e| e.message(project.lang))?;
        let formatted = format_round_files(&dir, &returned.files);
        let (fingerprint, diff) = snapshot(&project.root, &log, returned)?;
        receipt(start, &spec, returned, "view", &fingerprint, None).map_err(|e| e.message(project.lang))?;
        let hint = match project.lang {
            Locale::PtBr => {
                "Leia o diff, confira regras/skills e testes. Para aceitar, use REVIEWED com wave, fingerprint e note na rodada. Para recusar, use REJECTED com o motivo. Não peça aprovação ao usuário para uma revisão técnica já autorizada."
            }
            Locale::EnUs => {
                "Read the diff, check project rules/skills and tests. Accept with REVIEWED (wave, fingerprint, note) in the round report, or reject with REJECTED and a reason. Technical review does not require another user approval."
            }
        };
        let planned = mustard_core::domain::wave_prompt::wave_files(&log, wave);
        let outside: Vec<_> = returned.files.iter().filter(|file| !planned.iter().any(|scope| crate::shared::dag::files_cross(scope, file))).collect();
        let rules = review_rules(&project.root, &dir, &returned.files);
        Ok(json!({"ok":true,"spec":spec,"wave":wave,"fingerprint":fingerprint,"files":returned.files,
            "planned_files":planned,"outside_scope":outside,"project_rules":rules,"diff":diff,"formatted":formatted.formatted,"missing_formatters":formatted.missing,"hint":hint}))
    })();
    result.unwrap_or_else(|hint: String| json!({"ok":false,"reason":"wave-review-unavailable","wave":wave,"hint":hint}))
}

/// A resposta curta da revisão de onda: a impressão digital logo depois de
/// `ok`, o diff inteiro e cada regra do projeto pelo caminho e pelo tamanho
/// do texto, em bytes. O texto da regra vem com `--full` ou lendo o arquivo.
pub(crate) fn short(report: &Value) -> Value {
    let mut out = serde_json::Map::new();
    out.insert("ok".into(), report["ok"].clone());
    out.insert("fingerprint".into(), report["fingerprint"].clone());
    for (key, value) in report.as_object().into_iter().flatten() {
        let value = match key.as_str() {
            "ok" | "fingerprint" => continue,
            "project_rules" => json!(value.as_array().into_iter().flatten().map(|rule| json!({
                "source": rule["source"], "path": rule["path"], "bytes": rule["text"].as_str().map_or(0, str::len)})).collect::<Vec<_>>()),
            _ => value.clone(),
        };
        out.insert(key.clone(), value);
    }
    Value::Object(out)
}

fn review_dir(root: &Path, log: &SpecLog, wave: u64) -> std::path::PathBuf {
    real_changed_files(root, log, wave).filter(|files| !files.is_empty())
        .and_then(|_| copy_of(log, wave)).unwrap_or_else(|| root.to_path_buf())
}

fn review_rules(root: &Path, copy: &Path, files: &[String]) -> Vec<Value> {
    let current = mustard_core::io::wave_prompt::project_rules(root, files);
    let mut rules: Vec<_> = current.iter().map(|rule| json!({"source":"project","path":rule.path,"text":rule.text})).collect();
    if copy != root {
        for rule in mustard_core::io::wave_prompt::project_rules(copy, files) {
            if !current.iter().any(|old| old.path == rule.path && old.text == rule.text) {
                rules.push(json!({"source":"wave-copy","path":rule.path,"text":rule.text}));
            }
        }
    }
    rules
}

/// A impressão digital da revisão da onda `wave` e o diff que o condutor lê.
/// A impressão é o SHA-256 só do diff da cópia contra a base da onda — o
/// commit em que a cópia nasceu ([`fork_point`]) —: o caminho de cada arquivo
/// mudado e o diff dele contra essa base; no arquivo que o git não acompanha,
/// o tipo, o modo e o conteúdo dele na cópia. O HEAD do repositório
/// principal, a spec, as regras e a volta da onda ficam de fora: o commit de
/// outra onda, um item gravado na spec ou outra volta com o mesmo código não
/// mudam a impressão, e o aceite já dado continua valendo. Uma linha a mais
/// no diff muda.
fn snapshot(root: &Path, log: &SpecLog, wave: &WaveReport) -> Result<(String, String), String> {
    let dir = review_dir(root, log, wave.wave);
    let subs = submodules_of(root);
    let mut bases: BTreeMap<PathBuf, String> = BTreeMap::new();
    let mut hash = Sha256::new();
    let mut diff = String::new();
    let mut files = wave.files.clone();
    files.sort();
    files.dedup();
    for file in files {
        if Path::new(&file).components().any(|part| !matches!(part, Component::Normal(_))) {
            return Err(format!("Invalid review path: {file}"));
        }
        // A junção grava no principal o que lê da cópia: o caminho que sai de
        // um dos dois por uma pasta ligada é recusado antes.
        for base in [root, dir.as_path()] {
            if let Some(parent) = base.join(&file).parent().and_then(|parent| std::fs::canonicalize(parent).ok()) {
                let inside = std::fs::canonicalize(base).map_err(|e| e.to_string())?;
                if !parent.starts_with(inside) { return Err(format!("Review path leaves the repository: {file}")); }
            }
        }
        let (repo, inner) = repo_of(&dir, &subs, &file);
        let base = match bases.get(&repo) {
            Some(base) => base.clone(),
            None => {
                let base = fork_point(&repo, &repo_of(root, &subs, &file).0)?;
                bases.insert(repo.clone(), base.clone());
                base
            }
        };
        let tracked = git(&repo, &["--literal-pathspecs", "diff", "--no-ext-diff", "--binary", &base, "--", &inner])?;
        let path = dir.join(&file);
        hash.update(file.as_bytes());
        hash.update(&[0]);
        if tracked.is_empty() { untracked_state(&mut hash, &path)?; } else { hash.update(tracked.as_bytes()); }
        hash.update(&[0]);
        let _ = write!(diff, "\n--- {file} ---\n");
        let shown = if tracked.is_empty() {
            if std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink()) {
                format!("[Symlink → {}]", std::fs::read_link(&path).map_err(|e| e.to_string())?.display())
            } else {
                std::fs::read_to_string(&path).unwrap_or_else(|_| "[Binary or deleted file; inspect its contents separately]".into())
            }
        } else { tracked };
        if crate::shared::paths::sensitive_pattern(&file).is_some() || file == "mustard.json" {
            diff.push_str("[Protected file: use the masked config reader or inspect locally]");
        } else {
            diff.push_str(&crate::shared::secret::without_secrets(&shown));
        }
        diff.push('\n');
    }
    Ok((hash.hex_digest(), diff))
}

/// O que o diff do git não mostra do arquivo `path` da cópia, para a
/// impressão: o arquivo novo fora do git entra pelo tipo, pelo modo e pelo
/// conteúdo; o link, pelo destino; o que não existe, como ausente.
fn untracked_state(hash: &mut Sha256, path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            hash.update(b"symlink");
            hash.update(std::fs::read_link(path).map_err(|e| e.to_string())?.to_string_lossy().as_bytes());
        }
        Ok(meta) => {
            hash.update(b"file");
            hash.update(&[u8::from(meta.permissions().readonly())]);
            #[cfg(unix)] {
                use std::os::unix::fs::PermissionsExt;
                hash.update(&meta.permissions().mode().to_le_bytes());
            }
            hash.update(&std::fs::read(path).map_err(|e| e.to_string())?);
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => hash.update(b"absent"),
        Err(e) => return Err(e.to_string()),
    }
    Ok(())
}

pub(super) fn validate_paths(waves: &[WaveReport]) -> Result<(), RoundRefusal> {
    for file in waves.iter().flat_map(|wave| &wave.files) {
        if file.split('/').any(|part| part == "node_modules") {
            return Err(RoundRefusal::Git { detail: "Dependency directories and node_modules symlinks cannot be wave deliverables".into() });
        }
        if Path::new(file).components().any(|part| !matches!(part, Component::Normal(_))) {
            return Err(RoundRefusal::Git { detail:format!("Invalid wave path: {file}") });
        }
    }
    Ok(())
}

fn receipt(start: &Path, spec: &str, wave: &WaveReport, step: &str, hash: &str, note: Option<&str>) -> Result<(), mustard_core::domain::spec_events::Refusal> {
    let mut fields = serde_json::Map::from_iter([
        ("author".into(), json!("binary")), ("phase".into(), json!("delivery")),
        ("stage".into(), json!(format!("wave-review-{step}:{}", wave.wave))),
        ("result".into(), json!("pass")), ("ms".into(), json!(0)),
        ("fingerprint".into(), json!(hash)), ("attempt".into(), json!(wave.returns.last().copied().unwrap_or_default())),
    ]);
    if let Some(note) = note { fields.insert("output".into(), json!(crate::shared::secret::without_secrets(note))); }
    record(start, spec, "stage_run", fields, PhaseWriter::Binary).map(|_| ())
}

/// O recibo do passo `step` da revisão da onda `wave` com a impressão `hash`,
/// de qualquer volta dela: a impressão é só o diff, e outra volta com o mesmo
/// código não pede outra leitura dele.
fn has_receipt(log: &SpecLog, wave: &WaveReport, step: &str, hash: &str) -> bool {
    let stage = format!("wave-review-{step}:{}", wave.wave);
    log.events.iter().any(|e| {
        e.event_type == "stage_run" && e.str_field("author") == Some("binary")
        && e.str_field("stage") == Some(stage.as_str()) && e.str_field("fingerprint") == Some(hash)
    })
}

pub(super) fn hold_unreviewed(root: &Path, spec: &str, log: &SpecLog, waves: &mut Vec<WaveReport>, approvals: &[Approval], lang: Locale) -> Vec<HeldReturn> {
    hold_refused(waves, |wave| {
        if wave.files.is_empty() { return Ok(());
        }
        let snapshot = snapshot(root, log, wave);
        if let Ok((hash, _)) = &snapshot {
            if has_receipt(log, wave, "accepted", hash) { return Ok(());
            }
            if let Some(approval) = approvals.iter().find(|a| a.wave == wave.wave && a.fingerprint == *hash)
                .filter(|_| has_receipt(log, wave, "view", hash)) {
                return receipt(root, spec, wave, "accepted", hash, Some(&approval.note)).map_err(RoundRefusal::Refused);
            }
        }
        let hint = match lang {
            Locale::PtBr => format!("A onda {} aguarda revisão do diff antes do commit. Rode `mustard-rt run review-wave --wave {} --spec {spec}`, leia o diff e aceite com REVIEWED (wave, fingerprint, note) ou recuse com REJECTED. O registro de uso não aprova código.", wave.wave, wave.wave),
            Locale::EnUs => format!("Wave {} awaits diff review before commit. Run `mustard-rt run review-wave --wave {} --spec {spec}`, read the diff and accept with REVIEWED (wave, fingerprint, note) or reject with REJECTED. Usage does not approve code.", wave.wave, wave.wave),
        };
        Err(RoundRefusal::PreparationRequired(json!({"ok":false,"reason":"wave-review-required","wave":wave.wave,
            "agent_rework_required":false,"hint":hint,"detail":snapshot.err()})))
    })
}

/// Existing round fixtures include a conductor who accepts a technically
/// valid delivery. Exercise the production view and acceptance paths; never
/// disable the gate in test builds. Gate tests call round_for directly.
#[cfg(test)]
pub(crate) fn test_conductor_review(root: &Path, spec: &str) {
    let Some(path) = store::spec_file(root, spec).ok() else { return };
    let Some(log) = store::read(&path).ok().flatten() else { return };
    for wave in returned_waves(&log).0 {
        let view = view_at(root, Some(spec), wave.wave);
        let Some(hash) = view["fingerprint"].as_str() else { continue };
        let Some(fresh) = store::read(&path).ok().flatten() else { continue };
        let mut waves = returned_waves(&fresh).0;
        super::copy_check::check_against_copies(root, &fresh, &mut waves, Locale::PtBr);
        let approvals = [Approval { wave: wave.wave, fingerprint: hash.into(), note: "Fixture conductor reviewed the actual diff and project rules".into() }];
        hold_unreviewed(root, spec, &fresh, &mut waves, &approvals, Locale::PtBr);
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{approved, git_at, returned, round, waves_in, write};
    use super::super::{RoundOpts, round_for};
    use super::*;
    use tempfile::tempdir;

    fn bare_round(root: &Path, report: Option<String>) -> Value {
        round_for(&RoundOpts { root:root.into(), spec:Some("x".into()), report }, None)
    }

    #[test]
    fn usage_cannot_commit_unreviewed_code_and_changed_code_invalidates_the_view() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let copy = copy_of(&log, 1).unwrap();
        std::fs::write(copy.join("src/a.rs"), "fn changed() {}\n").unwrap();
        let out = returned(root, json!({"wave":1,"text":"Concluída.","files":["src/a.rs"],"commit":"Altera comportamento"}));
        assert_eq!(out["ok"], true, "{out}");
        let held = bare_round(root, Some("<USAGE>{\"wave\":1}</USAGE>".into()));
        assert!(held["warnings"].as_array().unwrap().iter().any(|w| w["reason"] == "wave-review-required"), "{held}");
        assert!(store::read(&path).unwrap().unwrap().last_by_wave("delivered").is_empty());
        let view = view_at(root, Some("x"), 1);
        assert_eq!(view["ok"], true, "{view}");
        assert!(view["diff"].as_str().unwrap().contains("fn changed()"));
        let approve = |view: &Value| {
            format!("<REVIEWED>{}</REVIEWED>", json!({"wave":1,"fingerprint":view["fingerprint"],"note":"Li o diff e conferi regras e testes"}))
        };
        std::fs::write(copy.join("src/a.rs"), "fn changed_again() {}\n").unwrap();
        let stale = bare_round(root, Some(approve(&view)));
        assert!(stale["warnings"].as_array().unwrap().iter().any(|w| w["reason"] == "wave-review-required"), "{stale}");
        let fresh = view_at(root, Some("x"), 1);
        assert_ne!(view["fingerprint"], fresh["fingerprint"]);
        std::fs::write(root.join("CLAUDE.md"), "Use shared schemas; never duplicate types.\n").unwrap();
        std::fs::write(copy.join("src/extra.rs"), "fn extra() {}\n").unwrap();
        let fresh = view_at(root, Some("x"), 1);
        assert_eq!(fresh["planned_files"], json!(["src/a.rs"]));
        assert_eq!(fresh["outside_scope"], json!(["src/extra.rs"]));
        assert_eq!(fresh["project_rules"][0]["path"], "CLAUDE.md");
        assert!(fresh["project_rules"][0]["text"].as_str().unwrap().contains("never duplicate types"));
        let accepted = bare_round(root, Some(approve(&fresh)));
        assert_eq!(accepted["ok"], true, "{accepted}");
        assert!(accepted["commit"].is_object(), "{accepted}");
        assert!(std::fs::read_to_string(root.join("src/a.rs")).unwrap().contains("changed_again"));
    }
    /// A revisão de uma onda que mudou `web/app.ts`, com uma regra do projeto
    /// na raiz. A resposta curta abre com a impressão digital logo depois de
    /// `ok`, traz o diff inteiro e a regra só pelo caminho e pelo tamanho; a
    /// inteira traz o texto da regra.
    #[test]
    fn short_answer_opens_the_review_with_the_fingerprint_and_rules_without_text() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["web/app.ts"], &[])]);
        round(root, "x", None);
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let copy = copy_of(&log, 1).unwrap();
        std::fs::create_dir_all(copy.join("web")).unwrap();
        std::fs::write(copy.join("web/app.ts"), "export const changed = 1;\n").unwrap();
        let out = returned(root, json!({"wave":1,"text":"Concluída.","files":["web/app.ts"],"commit":"Muda a página"}));
        assert_eq!(out["ok"], true, "{out}");
        let rule = "Use shared schemas; never duplicate types.";
        std::fs::write(root.join("CLAUDE.md"), format!("{rule}\n")).unwrap();
        let view = view_at(root, Some("x"), 1);
        assert_eq!(view["ok"], true, "{view}");
        let short = crate::commands::flow::projection("review-wave", &view, false).unwrap();
        let keys: Vec<&String> = short.as_object().unwrap().keys().collect();
        assert_eq!(keys[..2], ["ok", "fingerprint"], "{short}");
        assert_eq!(short["fingerprint"], view["fingerprint"]);
        assert!(short["diff"].as_str().unwrap().contains("export const changed = 1;"), "{short}");
        assert_eq!(short["diff"], view["diff"]);
        assert_eq!(short["project_rules"], json!([{"source":"project","path":"CLAUDE.md","bytes":rule.len()}]), "{short}");
        assert!(!short.to_string().contains(rule), "{short}");
        assert_eq!(crate::commands::flow::projection("review-wave", &view, true), None, "the whole answer is the view itself");
        assert_eq!(view["project_rules"][0]["text"], rule);
    }

    /// A onda que recebe a tarefa que a rodada criou para refazer um item
    /// não cumprido tem como planejados os arquivos do item: na revisão, só
    /// o arquivo fora deles fica fora do escopo.
    #[test]
    fn the_wave_with_a_redone_task_plans_the_files_of_the_item_it_redoes() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        super::super::agreed::tests::returned_with_its_task_unmet(root);
        let sent = round(root, "x", None);
        assert_eq!(waves_in(&sent, "dispatch"), vec![2], "the redone task leaves in wave 2: {sent}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let copy = copy_of(&log, 2).unwrap();
        std::fs::write(copy.join("src/app.ts"), "export const changed = 1;\n").unwrap();
        std::fs::write(copy.join("src/extra.ts"), "export const extra = 1;\n").unwrap();
        let out = returned(root, json!({"wave":2,"text":"Concluída.","files":["src/app.ts","src/extra.ts"],"commit":"Refaz a metade"}));
        assert_eq!(out["ok"], true, "{out}");
        let view = view_at(root, Some("x"), 2);
        assert_eq!(view["planned_files"], json!(["src/app.ts", "src/Api.cs"]), "{view}");
        assert_eq!(view["outside_scope"], json!(["src/extra.ts"]), "{view}");
    }

    #[test]
    fn an_unreviewed_wave_does_not_stop_an_independent_approved_delivery() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        round(root, "x", None);
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        for (wave, file) in [(1, "src/a.rs"), (2, "src/b.rs")] {
            let copy = copy_of(&log, wave).unwrap();
            std::fs::write(copy.join(file), "fn implemented() {}\n").unwrap();
            let out = returned(root, json!({"wave":wave,"text":"Concluída.","files":[file],"commit":"Altera comportamento"}));
            assert_eq!(out["ok"], true, "{out}");
        }
        let view = view_at(root, Some("x"), 2);
        assert_eq!(view["ok"], true, "{view}");
        let out = bare_round(root, Some(format!("<REVIEWED>{}</REVIEWED>", json!({"wave":2,"fingerprint":view["fingerprint"],"note":"Li o diff, as regras e os testes"}))));
        assert_eq!(out["ok"], true, "{out}");
        assert!(out["warnings"].as_array().unwrap().iter().any(|w| w["reason"] == "wave-review-required" && w["wave"] == 1), "{out}");
        let log = store::read(&path).unwrap().unwrap();
        assert!(!log.last_by_wave("delivered").contains_key(&1));
        assert!(log.last_by_wave("delivered").contains_key(&2));
        assert!(std::fs::read_to_string(root.join("src/b.rs")).unwrap().contains("implemented"));
        assert!(!std::fs::read_to_string(root.join("src/a.rs")).unwrap().contains("implemented"));
    }

    /// O condutor lê as duas ondas e dá os dois aceites. O commit da onda 1,
    /// que também grava a prova nova do critério dela, não invalida o aceite
    /// da onda 2: a rodada seguinte, com os mesmos aceites, comita a onda 2
    /// sem pedir outra leitura do mesmo diff.
    #[test]
    fn an_accepted_review_survives_the_commit_of_another_wave() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        round(root, "x", None);
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        for (wave, file) in [(1, "src/a.rs"), (2, "src/b.rs")] {
            std::fs::write(copy_of(&log, wave).unwrap().join(file), "fn implemented() {}\n").unwrap();
            let mut body = json!({"wave":wave,"text":"Concluída.","files":[file],"commit":"Altera comportamento"});
            if wave == 1 {
                body["proofs"] = json!([{"criterion":"MSTD-CRIT-0001","proof":"git --help"}]);
            }
            let out = returned(root, body);
            assert_eq!(out["ok"], true, "{out}");
        }
        let accepted: Vec<String> = [1, 2]
            .into_iter()
            .map(|wave| {
                let view = view_at(root, Some("x"), wave);
                assert_eq!(view["ok"], true, "{view}");
                format!("<REVIEWED>{}</REVIEWED>", json!({"wave":wave,"fingerprint":view["fingerprint"],"note":"Li o diff, as regras e os testes"}))
            })
            .collect();
        let first = bare_round(root, Some(accepted[0].clone()));
        assert!(first["commit"].is_object(), "{first}");
        assert!(store::read(&path).unwrap().unwrap().last_by_wave("delivered").contains_key(&1));
        let both = bare_round(root, Some(accepted.concat()));
        assert!(both["commit"].is_object(), "{both}");
        let held = both["warnings"].as_array().into_iter().flatten().any(|w| w["reason"] == "wave-review-required");
        assert!(!held, "{both}");
        assert!(store::read(&path).unwrap().unwrap().last_by_wave("delivered").contains_key(&2));
        assert!(std::fs::read_to_string(root.join("src/b.rs")).unwrap().contains("implemented"));
    }

    /// A impressão é só o diff da cópia contra a base da onda: um commit
    /// vazio na base, um item novo na spec, uma regra nova do projeto e outra
    /// volta com o mesmo código não a mudam; uma linha a mais no diff muda.
    #[test]
    fn the_fingerprint_depends_only_on_the_diff() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let path = store::spec_file(root, "x").unwrap();
        let copy = copy_of(&store::read(&path).unwrap().unwrap(), 1).unwrap();
        std::fs::write(copy.join("src/a.rs"), "fn implemented() {}\n").unwrap();
        let deliver = || {
            let out = returned(root, json!({"wave":1,"text":"Concluída.","files":["src/a.rs"],"commit":"Altera comportamento"}));
            assert_eq!(out["ok"], true, "{out}");
        };
        let fingerprint = || {
            let view = view_at(root, Some("x"), 1);
            view["fingerprint"].as_str().map(str::to_owned).unwrap_or_else(|| panic!("{view}"))
        };
        deliver();
        let reviewed = fingerprint();
        git_at(root, &["commit", "-q", "--allow-empty", "-m", "vazio"]);
        let said = write(root, "x", "message", json!({"author":"user","text":"uma regra a mais"}));
        let rule = json!({"title":"Nome curto","text":"O nome da função é curto.","example":"e","keys":["k"],
            "applies_to":{"files":["**"]},"origin":said["id"]});
        assert_eq!(write(root, "x", "rule", rule)["ok"], true);
        std::fs::write(root.join("CLAUDE.md"), "Use shared schemas; never duplicate types.\n").unwrap();
        deliver();
        assert_eq!(fingerprint(), reviewed);
        std::fs::write(copy.join("src/a.rs"), "fn implemented() {}\nfn one_more() {}\n").unwrap();
        assert_ne!(fingerprint(), reviewed);
    }
}
