//! Reusable, source-bound execution profiles. Native extraction only produces
//! navigation recipes; authored procedures require a review of their exact
//! fingerprint. Neither observations nor model prose become project rules.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::domain::{knowledge::Source, pattern};
use crate::io::{knowledge, project_map, sha256::Sha256};

mod export;
pub use export::export;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    pub description: String,
    /// Explicit globs; scope is not permission to edit those files.
    pub paths: Vec<String>,
    pub procedure: String,
    pub sources: Vec<Source>,
    /// Native navigation or authored procedure, never inferred authority.
    pub origin: String,
    #[serde(default)]
    pub reviewed: Option<String>,
}

pub fn fingerprint(profile: &Profile) -> String {
    let mut copy = profile.clone();
    copy.reviewed = None;
    digest(&serde_json::to_vec(&copy).unwrap_or_default())
}

pub fn review_fingerprint(root: &Path, profile: &Profile) -> String {
    let files = profile.sources.iter().map(|source| source.file.clone()).collect::<Vec<_>>();
    let rules = knowledge::RuleContext::of(root, &files);
    digest(format!("{}:{}", fingerprint(profile), rules.sha256).as_bytes())
}

fn digest(bytes: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(bytes);
    hash.hex_digest()
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn validate(root: &Path, profile: &Profile) -> Result<(), String> {
    if !valid_name(&profile.name) || profile.description.trim().is_empty() || profile.description.len() > 400
        || profile.procedure.trim().is_empty() || profile.procedure.len() > 8_000
        || profile.paths.is_empty() || profile.paths.len() > 256 || profile.sources.is_empty() || profile.sources.len() > 32
        || !["native-navigation", "authored"].contains(&profile.origin.as_str()) {
        return Err("expertise-invalid-profile".into());
    }
    for path in &profile.paths {
        if path.is_empty() || path.len() > 500 || path.starts_with('/') || path.contains(['\\', '\n', '\r'])
            || path.split('/').any(|part| ["..", "."].contains(&part))
            || globset::GlobBuilder::new(path).literal_separator(true).build().is_err() {
            return Err("expertise-invalid-scope".into());
        }
    }
    if !sources_current(root, profile) { return Err("expertise-source-changed-or-invalid".into()); }
    Ok(())
}

fn sources_current(root: &Path, profile: &Profile) -> bool {
    let mut hashes = BTreeMap::new();
    !profile.sources.is_empty() && profile.sources.iter().all(|source| knowledge::current(root, source, &mut hashes))
}

pub fn list(root: &Path) -> Result<Vec<Profile>, String> {
    let db = project_map::open_existing(&project_map::model_path(root)).map_err(|e| format!("{e:?}"))?;
    let mut stmt = db.conn().prepare("SELECT payload FROM expertise ORDER BY id").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
    rows.map(|row| serde_json::from_str(&row.map_err(|e| e.to_string())?).map_err(|e| e.to_string())).collect()
}

fn save(root: &Path, profile: &Profile) -> Result<bool, String> {
    let payload = serde_json::to_string(profile).map_err(|e| e.to_string())?;
    let mut db = project_map::open_existing(&project_map::model_path(root)).map_err(|e| format!("{e:?}"))?;
    let mut changed = false;
    db.write(|tx| {
        let previous: Option<String> = tx.query_row("SELECT payload FROM expertise WHERE id=?1", [&profile.name], |r| r.get(0)).ok();
        if previous.as_deref() != Some(&payload) {
            tx.execute("DELETE FROM expertise WHERE id=?1", [&profile.name])?;
            tx.execute("INSERT INTO expertise(id,payload) VALUES (?1,?2)", params![profile.name, payload])?;
            changed = true;
        }
        Ok(())
    }).map_err(|e| e.to_string())?;
    Ok(changed)
}

/// Record only a draft. A supplied `reviewed` value cannot authorize a model's
/// own output; review is a separate explicit command over the current draft.
pub fn record(root: &Path, profile: &Profile) -> Result<Value, String> {
    if profile.origin != "authored" { return Err("expertise-authored-origin-required".into()); }
    validate(root, profile)?;
    let mut draft = profile.clone();
    draft.reviewed = None;
    if let Some(old) = list(root)?.into_iter().find(|p| p.name == profile.name) {
        if old.origin != "authored" { return Err("expertise-native-name-reserved".into()); }
        if fingerprint(&old) == fingerprint(&draft) { draft.reviewed = old.reviewed; }
    }
    let changed = save(root, &draft)?;
    Ok(json!({"ok":true,"name":draft.name,"fingerprint":review_fingerprint(root,&draft),"active":active(root,&draft),"changed":changed}))
}

pub fn review(root: &Path, name: &str, expected: &str) -> Result<Value, String> {
    let mut profile = list(root)?.into_iter().find(|p| p.name == name).ok_or("expertise-not-found")?;
    validate(root, &profile)?;
    if profile.origin != "authored" || review_fingerprint(root,&profile) != expected { return Err("expertise-review-fingerprint-mismatch".into()); }
    profile.reviewed = Some(expected.into());
    save(root, &profile)?;
    Ok(json!({"ok":true,"name":name,"active":true,"fingerprint":expected}))
}

pub fn active(root: &Path, profile: &Profile) -> bool {
    sources_current(root, profile) && (profile.origin == "native-navigation" || profile.origin == "authored" && profile.reviewed.as_deref() == Some(review_fingerprint(root,profile).as_str()))
}

pub fn matches(profile: &Profile, files: &[String]) -> bool {
    profile.paths.iter().filter_map(|path| globset::GlobBuilder::new(path).literal_separator(true).build().ok())
        .any(|glob| files.iter().any(|file| glob.compile_matcher().is_match(file)))
}

pub fn selected(root: &Path, files: &[String]) -> Vec<Profile> {
    list(root).unwrap_or_default().into_iter().filter(|p| active(root, p) && matches(p, files)).collect()
}

/// Derive roles using the existing language/framework-agnostic import graph.
/// Repeated structure identifies a navigation scope, never a normative rule.
/// Unchanged profiles are not rewritten. Existing authored profiles survive.
pub fn sync(root: &Path) -> Result<Value, String> {
    let map = project_map::read(root).map_err(|e| format!("{e:?}"))?;
    let roles = pattern::learn(&map).roles;
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (file, role) in roles { groups.entry(role).or_default().push(file); }
    let db = project_map::open_existing(&project_map::model_path(root)).map_err(|e| format!("{e:?}"))?;
    let mut stmt = db.conn().prepare("SELECT path,line,end_line,sha256 FROM knowledge_symbols ORDER BY path,line").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |row| Ok(Source { file: row.get(0)?, line: row.get::<_, u32>(1)?.into(), end_line: row.get::<_, u32>(2)?.into(), sha256: row.get(3)? })).map_err(|e| e.to_string())?;
    let sources: Vec<_> = rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
    drop(stmt); drop(db);
    let existing = list(root)?;
    let mut changed = 0;
    let mut present = BTreeSet::new();
    for (role, paths) in groups {
        // One-off files are not evidence of a reusable specialization.
        if paths.len() < 2 || paths.len() > 256 { continue; }
        let mut used = BTreeSet::new();
        let mut hashes = BTreeMap::new();
        let examples: Vec<_> = sources.iter().filter(|source| paths.contains(&source.file)
            && knowledge::current(root, source, &mut hashes) && used.insert(source.file.clone())).take(4).cloned().collect();
        if examples.len() < 2 { continue; }
        let name = format!("area-{}", &digest(role.as_bytes())[..16]);
        present.insert(name.clone());
        if existing.iter().any(|p| p.name == name && p.origin != "native-navigation") { continue; }
        let profile = Profile { name, description: format!("Navigate the project's {role} area; use current examples and mandatory project rules."), paths,
            procedure: "Use Mustard to inspect current examples, callers, contracts and tests for this task. Compare neighboring implementations before changing code. Static similarity is not a business rule. Follow all mandatory project/spec rules. Work only on the assigned task; stop for a missing decision or access. Run its pertinent tests and return evidence.".into(),
            sources: examples, origin: "native-navigation".into(), reviewed: None };
        changed += usize::from(save(root, &profile)?);
    }
    // Withdraw obsolete native roles, without deleting a manually maintained file.
    let mut db = project_map::open_existing(&project_map::model_path(root)).map_err(|e| format!("{e:?}"))?;
    db.write(|tx| { for old in &existing { if old.origin == "native-navigation" && !present.contains(&old.name) {
        tx.execute("DELETE FROM expertise WHERE id=?1", [&old.name])?; changed += 1;
    }} Ok(()) }).map_err(|e| e.to_string())?;
    drop(db);
    let withdrawn = export::withdraw_stale(root)?;
    Ok(json!({"ok":true,"native_profiles":present.len(),"changed":changed,"withdrawn_exports":withdrawn,"model_calls":0}))
}

pub fn report(root: &Path, files: &[String]) -> Result<Value, String> {
    let profiles: Vec<_> = list(root)?.into_iter().filter(|p| files.is_empty() || matches(p, files)).map(|p|
        json!({"fingerprint":review_fingerprint(root,&p),"active":active(root,&p),"profile":p})).collect();
    Ok(json!({"ok":true,"profiles":profiles,"model_calls":0}))
}

/// Used by the wave request. Only matching, current profiles become skills;
/// the generic executor is retained when a host has not discovered new agents.
pub fn wave_skills(root: &Path, files: &[String]) -> Vec<(String, String, std::path::PathBuf)> {
    selected(root, files).into_iter().filter_map(|profile| {
        let report = export(root, &profile.name, "claude").ok()?;
        let path = report["skill"].as_str()?;
        let absolute = root.join(path);
        let text = std::fs::read_to_string(&absolute).ok()?;
        Some((format!("mustard-{}", profile.name), text, absolute))
    }).collect()
}

/// Dynamic agents get the same dispatch checks only while the registry and
/// the actual host file agree. A forged name or an edited export is insufficient.
pub fn is_agent(root: &Path, name: &str, host: &str) -> bool {
    let Some(profile) = name.strip_prefix("mustard-").and_then(|name| list(root).ok()?.into_iter().find(|p| p.name == name)) else { return false; };
    if !active(root, &profile) { return false; }
    let path = match host {
        "codex" => format!(".codex/agents/{name}.toml"),
        "claude" => format!(".claude/agents/{name}.md"),
        _ => return false,
    };
    owned_asset(root, &path)
}

fn owned_asset(root: &Path, path: &str) -> bool {
    let Ok(bytes) = std::fs::read(root.join(path)) else { return false; };
    let Ok(db) = project_map::open_existing(&project_map::model_path(root)) else { return false; };
    let expected: Result<String, _> = db.conn().query_row("SELECT payload FROM expertise_exports WHERE id=?1", [path], |row|row.get(0));
    expected.is_ok_and(|expected| expected == digest(&bytes))
}

pub fn is_skill(root: &Path, name: &str, path: &Path) -> bool {
    let Some(profile) = name.strip_prefix("mustard-").and_then(|name| list(root).ok()?.into_iter().find(|p| p.name == name)) else { return false; };
    let Ok(relative) = path.strip_prefix(root) else { return false; };
    active(root, &profile) && owned_asset(root, &relative.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests;
