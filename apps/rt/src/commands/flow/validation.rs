//! Execution measurements use the spec's existing event log. Presentation
//! reads these records; it never reruns validation to obtain a duration.

use std::path::Path;

use mustard_core::domain::spec_events::Refusal;
use mustard_core::domain::spec_state::PhaseWriter;
use serde_json::{Map, Value, json};

use crate::commands::review::qa_run::ProofRun;
use crate::commands::spec_events::write::record;

/// A timer defaults to failure so early returns are measured too. Inclusive
/// stages describe wall time; readers must not add them to their child stages.
pub(crate) struct Stage<'a> {
    root: &'a Path,
    spec: &'a str,
    phase: &'a str,
    name: &'a str,
    started: std::time::Instant,
    result: &'static str,
    inclusive: bool,
}

impl<'a> Stage<'a> {
    pub(crate) fn new(root: &'a Path, spec: &'a str, phase: &'a str, stage: &'a str, inclusive: bool) -> Self {
        Self { root, spec, phase, name: stage, started: std::time::Instant::now(), result: "fail", inclusive }
    }
    pub(crate) fn passed(&mut self) {
        self.result = "pass";
    }
    pub(crate) fn skipped(&mut self) {
        self.result = "skipped";
    }
}

impl Drop for Stage<'_> {
    fn drop(&mut self) {
        let draft = json!({"phase":self.phase,"stage":self.name,"result":self.result,
            "ms":self.started.elapsed().as_millis() as u64,"scope":if self.inclusive {"inclusive"}else{"leaf"},
            "version":mustard_core::harness_version(),
            "attempt":std::process::id(),"author":"binary"});
        if let Err(error) = record(self.root, self.spec, "stage_run", draft.as_object().cloned().unwrap_or_default(), PhaseWriter::Binary) {
            eprintln!("[Mustard] stage measurement was not recorded: {}", error.reason());
        }
    }
}

/// Content and execution inputs, rather than just HEAD. Failure to read any
/// input disables reuse; it never turns an incomplete digest into a receipt.
pub(super) fn fingerprint(root: &Path, log: &mustard_core::domain::spec_events::SpecLog) -> Option<String> {
    use mustard_core::io::sha256::Sha256;
    use mustard_core::platform::git;
    let files = git::run(root, &["ls-files", "--cached", "--others", "--exclude-standard", "-z"]);
    let files = files.out()?;
    let mut names: Vec<String> = files.split('\0').filter(|s| !s.is_empty()).map(str::to_string).collect();
    // Confirmed local inputs are often ignored by Git, including .env files.
    // Their contents enter only the digest, never the event or diagnostic.
    let config_inputs = mustard_core::ProjectConfig::load(root);
    for name in config_inputs.local_files.iter().flatten() {
        if !mustard_core::io::wave_prompt::local_file_inside(name) {
            return None;
        }
        names.push(name.clone());
    }
    names.sort_unstable();
    names.dedup();
    let mut digest = Sha256::new();
    let config = root.join("mustard.json");
    match std::fs::read(config) {
        Ok(bytes) => digest.update(&bytes),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => digest.update(b"no-config"),
        Err(_) => return None,
    }
    digest.update(b"mustard-final-validation-v2\0");
    for name in names {
        // The log and its projection change while validating. They are state,
        // not code inputs; criteria and config enter the digest separately.
        if name.starts_with(".claude/spec/")
            || name.starts_with(".claude/project/")
            || name.starts_with(".claude/pending/")
            || name.starts_with(".claude/grain.db")
            || name.starts_with(".claude/mustard/pages/")
            || name.starts_with(".claude/mustard/publications/")
            || name.starts_with(".claude/judgements/")
        {
            continue;
        }
        digest.update(name.as_bytes());
        digest.update(&[0]);
        let path = root.join(name);
        match std::fs::symlink_metadata(&path) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => digest.update(b"deleted\0"),
            Err(_) => return None,
            Ok(meta) if meta.file_type().is_symlink() => {
                digest.update(std::fs::read_link(&path).ok()?.to_str()?.as_bytes());
                // Content can change without changing the link's spelling.
                // A broken/external/directory link cannot prove reuse safely.
                let resolved = path.canonicalize().ok()?;
                if !resolved.starts_with(root.canonicalize().ok()?) || !resolved.is_file() {
                    return None;
                }
                digest.update(&std::fs::read(resolved).ok()?);
            }
            Ok(meta) if meta.is_file() => {
                digest.update(&std::fs::read(path).ok()?);
            }
            _ => return None,
        }
        digest.update(&[0]);
    }
    for event in log
        .block(mustard_core::domain::spec_events::BlockQuery::Block(mustard_core::domain::spec_events::Block::Criteria))
        .into_iter()
        .filter(|e| e.event_type == "criterion")
    {
        digest.update(&serde_json::to_vec(&event.fields).ok()?);
    }
    let mut env: Vec<_> = std::env::vars_os()
        .filter(|(name, _)| {
            let name = name.to_string_lossy();
            !matches!(name.as_ref(), "_" | "SHLVL" | "PWD" | "OLDPWD") && !name.starts_with("CLAUDE_") && !name.contains("SESSION")
        })
        .collect();
    env.sort();
    for (name, value) in env {
        digest.update(name.to_str()?.as_bytes());
        digest.update(&[0]);
        digest.update(value.to_str()?.as_bytes());
        digest.update(&[0]);
    }
    Some(digest.hex_digest())
}

pub(crate) fn reusable(root: &Path, log: &mustard_core::domain::spec_events::SpecLog) -> bool {
    let Some(key) = fingerprint(root, log) else {
        return false;
    };
    log.events
        .iter()
        .rev()
        .find(|e| e.event_type == "stage_run" && e.str_field("stage") == Some("final-validation"))
        .is_some_and(|e| e.str_field("result") == Some("pass") && e.str_field("fingerprint") == Some(&key))
}

pub(super) fn receipt(root: &Path, spec: &str, key: &str) -> Result<(), Refusal> {
    let draft = json!({"phase":"final", "stage":"final-validation", "result":"pass", "ms":0,
        "fingerprint":key, "scope":"receipt", "version":mustard_core::harness_version(), "author":"binary"});
    let Some(draft) = draft.as_object().cloned() else {
        return Err(Refusal::NotAnObject { detail: String::new() });
    };
    record(root, spec, "stage_run", draft, PhaseWriter::Binary).map(|_| ())
}

pub(super) fn record_run(root: &Path, spec: &str, phase: &str, stage: &str, command: &str, out: &ProofRun) -> Result<(), Refusal> {
    let mut draft = Map::new();
    draft.insert("phase".into(), json!(phase));
    draft.insert("stage".into(), json!(stage));
    draft.insert("command".into(), json!(command));
    draft.insert("result".into(), json!(out.result));
    draft.insert("exit".into(), json!(out.exit));
    draft.insert("ms".into(), json!(out.ms));
    draft.insert("scope".into(), json!("leaf"));
    draft.insert("version".into(), json!(mustard_core::harness_version()));
    draft.insert("attempt".into(), json!(std::process::id()));
    draft.insert("author".into(), Value::String("binary".into()));
    if !out.output.is_empty() {
        draft.insert("output".into(), json!(out.output));
    }
    record(root, spec, "stage_run", draft, PhaseWriter::Binary).map(|_| ())
}

/// Review elapsed wall time includes the host's dispatch and waiting time;
/// it is observational and must not be added to machine stage durations.
pub(super) fn review_elapsed(root: &Path, spec: &str, log: &mustard_core::domain::spec_events::SpecLog) -> Result<(), Refusal> {
    let verdict = log.visible().into_iter().rev().find(|e| e.event_type == "verdict" && e.fields.get("final") == Some(&Value::Bool(true)));
    let Some(verdict) = verdict else {
        return Ok(());
    };
    let key = format!("review-verdict-{}", verdict.id);
    if log.events.iter().any(|e| e.event_type == "stage_run" && e.str_field("fingerprint") == Some(&key)) {
        return Ok(());
    }
    let Some(sent) = log.visible().into_iter().rev().find(|e| e.event_type == "send" && e.str_field("role") == Some("review") && e.id < verdict.id) else {
        return Ok(());
    };
    let Some(ms) = chrono::DateTime::parse_from_rfc3339(sent.at())
        .ok()
        .zip(chrono::DateTime::parse_from_rfc3339(verdict.at()).ok())
        .and_then(|(from, to)| u64::try_from((to - from).num_milliseconds()).ok())
    else {
        return Ok(());
    };
    let draft = json!({"phase":"final","stage":"independent-review-elapsed","result":if verdict.str_field("result")==Some("approved"){"pass"}else{"fail"},
        "ms":ms,"scope":"inclusive","attempt":sent.id,"version":mustard_core::harness_version(),"fingerprint":key,"author":"binary"});
    record(root, spec, "stage_run", draft.as_object().cloned().unwrap_or_default(), PhaseWriter::Binary).map(|_| ())
}
