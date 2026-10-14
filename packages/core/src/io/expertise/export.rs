//! Native host exports with content ownership. A name/marker alone is never
//! permission to overwrite a user's agent or skill.
use super::{Profile, active, digest, fingerprint, list};
use std::{collections::BTreeSet, fmt::Write, path::Path};
use serde_json::{Value,json};
use rusqlite::params;
use crate::io::{fs,project_map};
use crate::io::fs::lock::LockedFile;
use rusqlite::OptionalExtension;

fn assets(root: &Path, profile: &Profile, host: &str) -> Result<Vec<(String, String)>, String> {
    let name = format!("mustard-{}", profile.name);
    let description = serde_json::to_string(&profile.description).map_err(|e| e.to_string())?;
    let mut references = String::new();
    for source in &profile.sources {
        let _ = writeln!(references,"- `{}` lines {}–{}; SHA-256 `{}`.",source.file,source.line,source.end_line,source.sha256);
    }
    // Scope belongs to the native registry. Repeating hundreds of paths in
    // frontmatter wastes context and does not improve host skill routing.
    let skill = format!("---\nname: {name}\ndescription: {description}\nmetadata: {{\"source\":\"mustard-expertise\"}}\n---\n\n{}\n\nReferences are navigation evidence, not project rules. Recheck current source before using a conclusion.\n\n{references}", profile.procedure);
    let config = crate::domain::config::ProjectConfig::load(root);
    let body = crate::platform::seeds::agent_texts(config.language().text_or_default())[0].1;
    match host {
        "claude" => {
            // Executor constraints are the same compiled template as the generic
            // agent; specialization cannot remove task/proof/scope protections.
            let configured = crate::platform::seeds::with_agent_settings(body, config.agent_settings());
            let (header, text) = configured.strip_prefix("---\n").and_then(|body| body.split_once("\n---"))
                .ok_or("expertise-executor-template-invalid")?;
            let header = header.lines().map(|line| {
                if line.starts_with("name:") { format!("name: {name}") }
                else if line.starts_with("description:") { format!("description: {description}") }
                else {line.to_owned()}
            }).collect::<Vec<_>>().join("\n");
            let common = root.join(".claude/agents/mustard/wave.md").to_string_lossy().replace('\\', "/");
            let agent = if root.join(".claude/agents/mustard/wave.md").is_file() {
                format!("---\n{header}\nskills: [{name}]\n---\n\nFollow the wave execution protocol in `{common}` before working. The dispatch provides the task, mandatory project rules and exact evidence. Use Mustard for source search. Work in the assigned copy, preserve the approved scope, record pertinent tests and return `blocked_by`/`undone` when access or decisions are missing. Do not commit or change the plan. After 150,000 work tokens beyond initial input and inherited summaries, finish the current task, minimal tests and summary; start no new task. Initial 50,000 means the signal at total 200,000.")
            } else {
                format!("---\n{header}\nskills: [{name}]\n---{text}")
            };
            Ok(vec![(format!(".claude/skills/{name}/SKILL.md"), skill),
                (format!(".claude/agents/{name}.md"), format!("{agent}\n\nSpecialization: follow `{}` for the requested area; mandatory rules and wave scope take precedence.\n", format_args!(".claude/skills/{name}/SKILL.md")))])
        }
        "codex" => {
            let common = body.strip_prefix("---\n").and_then(|body| body.split_once("\n---")).map(|(_, body)| body.trim()).ok_or("expertise-executor-template-invalid")?;
            let protocol = root.join(".claude/agents/mustard/wave.md");
            let common = if protocol.is_file() {
                format!("Read the shared wave protocol at {} before working. Follow the dispatched task and mandatory project rules. Use Mustard for source search. Do not commit or change the plan. Record pertinent tests and return blocked_by/undone for missing access or decisions. After 150,000 work tokens beyond initial input and inherited summaries, finish the task, minimal tests and summary; start no new task. Initial 50,000 means the signal at total 200,000.", protocol.to_string_lossy())
            } else {common.replace("Claude Code", "Codex")};
            let instructions = format!("{common}\n\nUse .agents/skills/{name}/SKILL.md for the requested area. Its scope is navigation evidence, not edit authorization.");
            let agent = toml::to_string(&json!({"name":name,"description":profile.description,"developer_instructions":instructions})).map_err(|e| e.to_string())?;
            Ok(vec![(format!(".agents/skills/{name}/SKILL.md"), skill), (format!(".codex/agents/{name}.toml"), agent)])
        }
        _ => Err("expertise-unknown-host".into()),
    }
}

fn inside(root: &Path, relative: &str) -> Result<(), String> {
    let canonical = root.canonicalize().map_err(|e| e.to_string())?;
    let mut ancestor = root.join(relative);
    while !ancestor.exists() { if !ancestor.pop() { return Err("expertise-export-outside-root".into()); } }
    if ancestor.canonicalize().map_err(|e| e.to_string())?.starts_with(canonical) { Ok(()) }
    else { Err("expertise-export-outside-root".into()) }
}

pub fn export(root: &Path, name: &str, host: &str) -> Result<Value, String> {
    let profile = list(root)?.into_iter().find(|p| p.name == name).ok_or("expertise-not-found")?;
    if profile.origin != "authored" { return Err("expertise-procedure-review-required; native navigation stays in the index".into()); }
    if !active(root, &profile) { return Err("expertise-review-or-current-sources-required".into()); }
    let assets = assets(root, &profile, host)?;
    inside(root, ".claude/.cache/expertise-export.lock")?;
    let directory = root.join(".claude/.cache");
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let _lock = LockedFile::exclusive_if_free(&directory.join("expertise-export.lock")).map_err(|e| e.to_string())?.ok_or("expertise-export-busy-retry")?;
    let mut db = project_map::open_existing(&project_map::model_path(root)).map_err(|e| format!("{e:?}"))?;
    // Validate the whole export before writing its first asset.
    for (path, _) in &assets {
        inside(root, path)?;
        if root.join(path).symlink_metadata().is_ok() {
            let bytes = std::fs::read(root.join(path)).map_err(|e| e.to_string())?;
            let previous: Option<String> = db.conn().query_row("SELECT payload FROM expertise_exports WHERE id=?1", [path], |r| r.get(0)).optional().map_err(|e| e.to_string())?;
            if previous.as_deref() != Some(digest(&bytes).as_str()) { return Err(format!("expertise-manual-file-preserved:{path}")); }
        }
    }
    let private=assets.iter().map(|(path,_)|format!("/{path}")).collect::<Vec<_>>();
    if crate::platform::git_exclude::ensure_excluded(root,&private).unavailable.is_some_and(|reason|reason.is_blocking()) {
        return Err("expertise-private-footprint-failed".into());
    }
    let mut changed = 0;
    for (path, text) in &assets {
        let absolute = root.join(path);
        if std::fs::read(&absolute).ok().as_deref() != Some(text.as_bytes()) {
            if let Some(parent) = absolute.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
            fs::write_atomic(&absolute, text.as_bytes()).map_err(|e| e.to_string())?;
            changed += 1;
        }
    }
    db.write(|tx| { for (path, text) in &assets {
        tx.execute("DELETE FROM expertise_exports WHERE id=?1", [path])?;
        tx.execute("INSERT INTO expertise_exports(id,payload) VALUES (?1,?2)", params![path, digest(text.as_bytes())])?;
    } Ok(()) }).map_err(|e| e.to_string())?;
    Ok(json!({"ok":true,"host":host,"profile":name,"fingerprint":fingerprint(&profile),"skill":assets[0].0,"agent":assets[1].0,"changed":changed,
        "discovery":"New agent definitions may require host rediscovery; use the generic executor with this skill in the current session until available.","model_calls":0}))
}

/// Remove only obsolete exports whose bytes still match our ownership ledger.
/// A user edit is preserved and reported; registry selection still rejects it.
pub(super) fn withdraw_stale(root: &Path) -> Result<Value, String> {
    let live: BTreeSet<String> = list(root)?.into_iter().filter(|p| p.origin == "authored" && active(root,p)).flat_map(|p|
        ["claude","codex"].into_iter().filter_map(move |host|assets(root,&p,host).ok()).flatten().map(|(path,_)|path)).collect();
    let directory = root.join(".claude/.cache");
    fs::create_dir_all(&directory).map_err(|e|e.to_string())?;
    let _lock = LockedFile::exclusive_if_free(&directory.join("expertise-export.lock")).map_err(|e|e.to_string())?.ok_or("expertise-export-busy-retry")?;
    let mut db = project_map::open_existing(&project_map::model_path(root)).map_err(|e|format!("{e:?}"))?;
    let rows: Vec<(String,String)> = {
        let mut stmt = db.conn().prepare("SELECT id,payload FROM expertise_exports").map_err(|e|e.to_string())?;
        stmt.query_map([],|row|Ok((row.get(0)?,row.get(1)?))).map_err(|e|e.to_string())?.collect::<rusqlite::Result<_>>().map_err(|e|e.to_string())?
    };
    let mut removed = Vec::new(); let mut preserved = Vec::new();
    for (path, expected) in rows.into_iter().filter(|(path,_)| !live.contains(path)) {
        inside(root,&path)?;
        match std::fs::read(root.join(&path)) {
            Ok(bytes) if digest(&bytes) == expected => {
                std::fs::remove_file(root.join(&path)).map_err(|e|e.to_string())?;
                if path.ends_with("/SKILL.md") && let Some(parent)=root.join(&path).parent() {let _=std::fs::remove_dir(parent);}
                removed.push(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => removed.push(path),
            _ => preserved.push(path),
        }
    }
    db.write(|tx| { for path in &removed {tx.execute("DELETE FROM expertise_exports WHERE id=?1",[path])?;} Ok(()) }).map_err(|e|e.to_string())?;
    Ok(json!({"removed":removed,"manual_edits_preserved":preserved}))
}
