//! Review the actual worktree, rather than accepting the agent's account.
//! A native view receipt and the conductor's approval bind the delivery,
//! base contents, file modes and changed contents. Neither USAGE nor a build
//! is a review. An independent wave can still proceed while one waits.
use std::fmt::Write as _;
use std::path::{Component, Path};

use mustard_core::domain::spec_events::{Block, BlockQuery, SpecLog};
use mustard_core::domain::spec_state::{PhaseWriter, SpecState};
use mustard_core::io::{sha256::Sha256, spec_events as store};
use mustard_core::platform::i18n::Locale;
use serde_json::{Value, json};

use super::RoundRefusal;
use super::commit::{copy_of, format_round_files, git, real_changed_files, repo_of};
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

fn snapshot(root: &Path, log: &SpecLog, wave: &WaveReport) -> Result<(String, String), String> {
    let dir = review_dir(root, log, wave.wave);
    let subs = submodules_of(root);
    let mut hash = Sha256::new();
    hash.update(&wave.wave.to_le_bytes());
    hash.update(&wave.returns.last().copied().unwrap_or_default().to_le_bytes());
    // Rules, the exact request and current agreed items are part of what
    // the conductor approves, so changing them invalidates the receipt too.
    hash.update(serde_json::to_string(&review_rules(root, &dir, &wave.files)).map_err(|e| e.to_string())?.as_bytes());
    for block in [Block::Agreed, Block::Specification, Block::Criteria] {
        for item in log.block(BlockQuery::Block(block)) {
            hash.update(&item.id.to_le_bytes());
        }
    }
    if let Some(sent) = log.last_by_wave("send").get(&wave.wave).and_then(|id| log.get(*id)) {
        hash.update(sent.str_field("text").unwrap_or_default().as_bytes());
    }
    let mut diff = String::new();
    let mut files = wave.files.clone();
    files.sort();
    files.dedup();
    for file in files {
        if Path::new(&file).components().any(|part| !matches!(part, Component::Normal(_))) {
            return Err(format!("Invalid review path: {file}"));
        }
        hash.update(file.as_bytes());
        hash.update(&[0]);
        // Include both sides of the eventual merge, file modes and symlinks.
        for base in [root, dir.as_path()] {
            let path = base.join(&file);
            if let Some(parent) = path.parent().and_then(|parent| std::fs::canonicalize(parent).ok()) {
                let inside = std::fs::canonicalize(base).map_err(|e| e.to_string())?;
                if !parent.starts_with(inside) { return Err(format!("Review path leaves the repository: {file}")); }
            }
            match std::fs::symlink_metadata(&path) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    hash.update(b"symlink");
                    hash.update(std::fs::read_link(&path).map_err(|e| e.to_string())?.to_string_lossy().as_bytes());
                }
                Ok(meta) => {
                    hash.update(b"file");
                    hash.update(&[u8::from(meta.permissions().readonly())]);
                    #[cfg(unix)] {
                        use std::os::unix::fs::PermissionsExt;
                        hash.update(&meta.permissions().mode().to_le_bytes());
                    }
                    hash.update(&std::fs::read(&path).map_err(|e| e.to_string())?);
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => hash.update(b"absent"),
                Err(e) => return Err(e.to_string()),
            }
            hash.update(&[0]);
        }
        let (repo, inner) = repo_of(&dir, &subs, &file);
        let tracked = git(&repo, &["--literal-pathspecs", "diff", "--no-ext-diff", "--binary", "HEAD", "--", &inner])?;
        hash.update(tracked.as_bytes());
        let _ = write!(diff, "\n--- {file} ---\n");
        let shown = if tracked.is_empty() {
            let path = dir.join(&file);
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

fn has_receipt(log: &SpecLog, wave: &WaveReport, step: &str, hash: &str) -> bool {
    let stage = format!("wave-review-{step}:{}", wave.wave);
    log.events.iter().any(|e| {
        e.event_type == "stage_run" && e.str_field("author") == Some("binary")
        && e.str_field("stage") == Some(stage.as_str()) && e.str_field("fingerprint") == Some(hash)
        && e.int("attempt") == wave.returns.last().copied()
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
    use super::super::tests::{approved, returned, round};
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
        let changed_rule = bare_round(root, Some(approve(&fresh)));
        assert!(changed_rule["warnings"].as_array().unwrap().iter().any(|w| w["reason"] == "wave-review-required"), "{changed_rule}");
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

}
