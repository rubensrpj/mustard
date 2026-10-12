//! Two-phase enrichment by a subagent of the caller's existing session.
//! This port never starts a CLI or makes a generation API call. The native
//! receipt binds the reply to the exact evidence actually supplied.
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Evidence, GenerationRequest, evidence, interpretation, request};
use crate::io::fs::{self, lock::LockedFile};
use crate::io::knowledge::{Interpretation, current, interpretations, record_at};
use crate::io::sha256::Sha256;

mod context;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Prepared {
    version: u32,
    host: String,
    requested_model: String,
    request: GenerationRequest,
    sources: Vec<Evidence>,
    rule_context: crate::io::knowledge::RuleContext,
}

/// Result returned by the native subagent of the current host session.
/// Usage remains unknown unless the host actually exposes it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionReceipt {
    pub request_id: String,
    pub model: String,
    /// Actual host effort, only when exposed. Requested effort is always medium.
    #[serde(default)]
    pub effort: Option<String>,
    pub result: Value,
    /// Full prompt, including host instructions, tools and cached input.
    /// Not just the uncached portion of Claude's usage record.
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

fn token(value: &str) -> bool {
    !value.is_empty() && value.len() <= 120 && value.chars().all(|c| c.is_ascii_alphanumeric() || "-_.:/".contains(c))
}

fn identity(prepared: &Prepared) -> Result<String, String> {
    let mut hash = Sha256::new();
    hash.update(&serde_json::to_vec(prepared).map_err(|e| e.to_string())?);
    Ok(hash.hex_digest())
}

/// Reuse an analysis across hosts and preferred models. Question, prompt
/// schema and every source still bind the result, including uncited sources.
fn analysis_identity(prepared: &Prepared) -> String {
    let mut hash=Sha256::new();
    hash.update(json!({"version":2,"request":prepared.request,"sources":prepared.sources,"rule_context":prepared.rule_context}).to_string().as_bytes());
    hash.hex_digest()
}

fn directory(root: &Path) -> PathBuf {
    root.join(".claude/.cache/knowledge-session")
}

fn cached(root: &Path, tree: &Path, prepared: &Prepared) -> Result<Option<Interpretation>, String> {
    let id=format!("session-{}",analysis_identity(prepared));
    let found = interpretations(root).map_err(|e| format!("{e:?}"))?.into_iter().find(|note| note.id == id);
    let mut hashes = Default::default();
    Ok(found.filter(|note| prepared.rule_context.current(tree) && note.sources.iter().all(|source|
        prepared.sources.iter().any(|e| e.source == *source) && current(tree, source, &mut hashes))))
}

/// Assemble only current, bounded evidence for one explicit question. The
/// caller delegates `delegation.request` using its own Agent/spawn_agent tool,
/// then submits a SessionReceipt. A valid cached interpretation skips that
/// delegation; a missing host/model must fall back to the current session or
/// native retrieval, never to a new process or separate API credential.
pub fn prepare_session(root: &Path, tree: &Path, query: &str, file: Option<&str>, limit: usize, host: &str, model: Option<&str>) -> Result<Value, String> {
    if !["claude", "codex"].contains(&host) { return Err("knowledge-session-unknown-host".into()); }
    if query.trim().is_empty() || query.len() > 2000 { return Err("knowledge-enrichment-needs-short-query".into()); }
    let query=query.split_whitespace().collect::<Vec<_>>().join(" ");
    let model = model.unwrap_or(if host == "claude" { "haiku" } else { "gpt-6-luna" });
    if !token(model) { return Err("knowledge-session-invalid-model".into()); }
    let sources = evidence(root, tree, &query, file, limit)?;
    let rule_context = crate::io::knowledge::RuleContext::of(tree, &sources.iter().map(|s| s.source.file.clone()).collect::<Vec<_>>());
    let prepared = Prepared { version: 2, host: host.into(), requested_model: model.into(), request: request(&query, &sources)?, sources, rule_context };
    let id = identity(&prepared)?;
    if let Some(note) = cached(root, tree, &prepared)? {
        return Ok(json!({"ok":true,"cached":true,"interpretation":note,"session_model_calls":0,"remote_model_calls":0,"local_model_calls":0}));
    }
    let folder = directory(root);
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let analysis_id=analysis_identity(&prepared);
    let _lock = LockedFile::exclusive_if_free(&folder.join(format!("analysis-{analysis_id}.lock"))).map_err(|e| e.to_string())?.ok_or("knowledge-session-busy-retry")?;
    // Another process can have completed the work while evidence was read.
    if let Some(note)=cached(root,tree,&prepared)? {
        return Ok(json!({"ok":true,"cached":true,"interpretation":note,"session_model_calls":0,"remote_model_calls":0,"local_model_calls":0}));
    }
    let pending_path=folder.join(format!("analysis-{analysis_id}.pending"));
    if let Ok(pending)=fs::read_to_string(&pending_path)
        && pending.len()==64 && pending.bytes().all(|b|b.is_ascii_hexdigit())
        && let Ok(bytes)=std::fs::read(folder.join(format!("{pending}.json")))
        && bytes.len()<=100_000
        && let Ok(previous)=serde_json::from_slice::<Prepared>(&bytes)
        && identity(&previous)?==pending && analysis_identity(&previous)==analysis_id {
        return Ok(json!({"ok":true,"cached":false,"pending":true,"request_id":pending,
            "session_model_calls":0,"remote_model_calls":0,"local_model_calls":0,
            "next":"Reuse the existing delegation; do not start a second analysis. Complete its receipt, or cancel explicitly with knowledge --cancel-session <request_id> if no agent owns it."}));
    }
    let _request_lock = LockedFile::exclusive_if_free(&folder.join(format!("{id}.lock"))).map_err(|e| e.to_string())?.ok_or("knowledge-session-busy-retry")?;
    fs::write_atomic(folder.join(format!("{id}.json")), &serde_json::to_vec(&prepared).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    fs::write_atomic(pending_path,id.as_bytes()).map_err(|e|e.to_string())?;
    Ok(json!({"ok":true,"cached":false,"request_id":id,"session_model_calls":0,"remote_model_calls":0,"local_model_calls":0,
        "evidence_bytes":prepared.sources.iter().map(|e|e.excerpt.len()).sum::<usize>(),
        "delegation":{"host":host,"requested_model":model,"model_family":if host=="claude" {"haiku"} else {"luna"},
            "model_policy":"latest-available-in-family; resolve with current host session capabilities, not a pinned snapshot",
            "effort":"medium","fallback":"use-native-evidence-if-latest-family-or-medium-effort-is-unavailable",
            "execution":"native-subagent-in-current-session","access":"read-only; only supplied excerpts are needed",
            "max_input_tokens":context::TARGET,
            "context_policy":"Use an isolated task context; pass no parent transcript. The fixed input budget is 100000 total tokens, including instructions, tools and cache, for Haiku and Luna. Use native evidence if the host cannot isolate the task. Excerpt bytes alone do not verify total usage.",
            "request":prepared.request},
        "context_target_tokens":context::TARGET,
        "model_override":"--session-model selects a host preference; always resolve the latest available version of the selected family with medium effort.",
        "completion":{"request_id":id,"model":"inherit","effort":null,"result":{"title":"...","text":"... [s0]","source_ids":["s0"]},
            "input_tokens":null,"output_tokens":null},
        "usage_instruction":"Resolve the latest available Haiku/Luna in the current host and request medium effort. Report actual model, effort and full input only if exposed; otherwise keep inherit/null and do not claim compliance. Never infer usage from excerpt length.",
        "next":"Delegate only if interpretation is needed. Submit the result with mustard-rt run knowledge --complete-session <receipt.json>. Never launch claude/codex or a generation API."}))
}

/// Release only an unfinished preparation. Cancellation does not remove a
/// completed interpretation or invalidate its reusable evidence.
pub fn cancel_session(root:&Path,id:&str)->Result<Value,String> {
    if id.len()!=64 || !id.bytes().all(|b|b.is_ascii_hexdigit()) {return Err("knowledge-session-invalid-request-id".into());}
    let folder=directory(root);
    let bytes=std::fs::read(folder.join(format!("{id}.json"))).map_err(|e|e.to_string())?;
    if bytes.len()>100_000 {return Err("knowledge-session-request-too-large".into());}
    let prepared:Prepared=serde_json::from_slice(&bytes).map_err(|e|e.to_string())?;
    if identity(&prepared)?!=id {return Err("knowledge-session-request-changed".into());}
    let analysis_id=analysis_identity(&prepared);
    let _lock=LockedFile::exclusive_if_free(&folder.join(format!("analysis-{analysis_id}.lock"))).map_err(|e|e.to_string())?.ok_or("knowledge-session-busy-retry")?;
    let path=folder.join(format!("analysis-{analysis_id}.pending"));
    let released=fs::read_to_string(&path).ok().as_deref()==Some(id);
    if released {std::fs::remove_file(path).map_err(|e|e.to_string())?;}
    Ok(json!({"ok":true,"request_id":id,"released":released,"session_model_calls":0,"remote_model_calls":0,"local_model_calls":0}))
}

/// Validate every supplied source again, including uncited excerpts. The
/// result is an interpretation, not a verified fact or a project rule. A
/// repeated completion preserves a reviewed receipt and is idempotent.
pub fn complete_session(root: &Path, tree: &Path, receipt: &SessionReceipt) -> Result<Value, String> {
    if receipt.request_id.len() != 64 || !receipt.request_id.bytes().all(|b| b.is_ascii_hexdigit()) || !token(&receipt.model)
        || receipt.effort.as_deref().is_some_and(|value| !crate::domain::config::AGENT_EFFORTS.contains(&value)) {
        return Err("knowledge-session-invalid-receipt".into());
    }
    let folder = directory(root);
    let _lock = LockedFile::exclusive_if_free(&folder.join(format!("{}.lock", receipt.request_id))).map_err(|e| e.to_string())?.ok_or("knowledge-session-busy-retry")?;
    let path = folder.join(format!("{}.json", receipt.request_id));
    if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 100_000 { return Err("knowledge-session-request-too-large".into()); }
    let prepared: Prepared = serde_json::from_str(&fs::read_to_string(&path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    if prepared.version != 2 || identity(&prepared)? != receipt.request_id { return Err("knowledge-session-request-changed".into()); }
    let mut hashes = Default::default();
    if prepared.sources.iter().any(|e| !current(tree, &e.source, &mut hashes)) { return Err("knowledge-source-changed-during-generation".into()); }
    // Validate even a duplicate reply; cache reuse must not conceal malformed citations.
    if !prepared.rule_context.current(tree) { return Err("knowledge-rules-changed-during-generation".into()); }
    let mut note = interpretation(format!("session-{}", analysis_identity(&prepared)), &format!("session:{}:{}", prepared.host, receipt.model), &receipt.result.to_string(), &prepared.sources)?;
    if let Some(note) = cached(root, tree, &prepared)? {
        return Ok(json!({"ok":true,"cached":true,"interpretation":note,"session_model_calls":0,"remote_model_calls":0,"local_model_calls":0}));
    }
    note.rule_context = Some(prepared.rule_context.clone());
    record_at(root, tree, &note).map_err(|e| format!("{e:?}"))?;
    Ok(json!({"ok":true,"cached":false,"interpretation":note,"semantic_proof":false,"session_model_calls":1,
        "remote_model_calls":0,"local_model_calls":0,"host":prepared.host,"model":receipt.model,
        "effort":receipt.effort,"effort_target":"medium","effort_observed":receipt.effort.as_deref().map(|e|e=="medium"),
        "model_policy":"latest-available-in-family; actual selection requires host verification",
        "input_tokens":receipt.input_tokens,"output_tokens":receipt.output_tokens,
        "context":context::report(&prepared.host,&receipt.model,receipt.input_tokens),
        "billing":"current host session; no separate generation API call; subscription cost unknown"}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::tests::{fixture, index};

    #[test]
    fn pending_analysis_is_shared_across_hosts_and_can_be_explicitly_released() {
        let dir=fixture(); index(dir.path());
        let first=prepare_session(dir.path(),dir.path(),"backup",None,4,"claude",None).unwrap();
        let second=prepare_session(dir.path(),dir.path(),"backup",None,4,"codex",Some("gpt-6-luna")).unwrap();
        assert_eq!(second["pending"],true); assert_eq!(second["request_id"],first["request_id"]);
        assert!(second.get("delegation").is_none());
        let id=first["request_id"].as_str().unwrap();
        assert_eq!(cancel_session(dir.path(),id).unwrap()["released"],true);
        assert_eq!(cancel_session(dir.path(),id).unwrap()["released"],false);
        let next=prepare_session(dir.path(),dir.path(),"backup",None,4,"codex",None).unwrap();
        assert!(next.get("delegation").is_some()); assert!(next.get("pending").is_none());
    }

    #[test]
    fn changed_mandatory_rules_withdraw_reuse_and_reject_pending_completion() {
        let dir=fixture(); let root=dir.path();
        std::fs::write(root.join("AGENTS.md"), "Use existing transaction boundaries.\n").unwrap();
        let first=prepare_session(root,root,"backup",None,8,"codex",None).unwrap();
        let receipt=|request_id:String| SessionReceipt {request_id,model:"gpt-6-luna".into(),effort:Some("medium".into()),
            result:json!({"title":"Backup","text":"Restores a backup [s0].","source_ids":["s0"]}),input_tokens:Some(300),output_tokens:Some(20)};
        let old=receipt(first["request_id"].as_str().unwrap().into());
        complete_session(root,root,&old).unwrap();
        assert_eq!(prepare_session(root,root,"backup",None,8,"claude",None).unwrap()["cached"],true);
        std::fs::write(root.join("AGENTS.md"), "Use the revised transaction boundaries.\n").unwrap();
        assert!(complete_session(root,root,&old).unwrap_err().contains("rules-changed"));
        let second=prepare_session(root,root,"backup",None,8,"codex",None).unwrap();
        assert_eq!(second["cached"],false);
        assert_ne!(first["request_id"],second["request_id"]);
        let report=crate::io::knowledge::query(root,"backup",None,8,0,false).unwrap().0;
        assert_eq!(report["interpretations"],json!([]));
        assert_eq!(report["stale_interpretations"].as_array().unwrap().len(),1);
    }

    fn receipt(id: &Value) -> SessionReceipt {
        SessionReceipt { request_id: id.as_str().unwrap().into(), model: "inherit".into(), effort:None,
            result: json!({"title":"Restore","text":"Restores a backup [s0].","source_ids":["s0"]}), input_tokens: None, output_tokens: None }
    }

    #[test]
    fn current_session_enrichment_is_reusable_and_never_upgrades_a_hypothesis() {
        for (host, model) in [("claude", "haiku"), ("codex", "gpt-6-luna")] {
            let dir = fixture();
            let first = prepare_session(dir.path(), dir.path(), "backup", None, 8, host, None).unwrap();
            assert_eq!(first["delegation"]["requested_model"], model);
            assert_eq!(first["delegation"]["effort"], "medium");
            assert!(first["delegation"]["model_policy"].as_str().unwrap().starts_with("latest-available-in-family"));
            assert_eq!(first["session_model_calls"], 0);
            assert!(first["next"].as_str().unwrap().contains("--complete-session"));
            assert!(first["model_override"].as_str().unwrap().contains("--session-model"));
            let done = complete_session(dir.path(), dir.path(), &receipt(&first["request_id"])).unwrap();
            assert_eq!(done["interpretation"]["status"], "hypothesis");
            assert_eq!(done["session_model_calls"], 1);
            assert_eq!(done["remote_model_calls"], 0);
            assert!(done["input_tokens"].is_null());
            assert!(done["effort_observed"].is_null(),"a preference is not actual host telemetry");
            let mut reviewed: Interpretation = serde_json::from_value(done["interpretation"].clone()).unwrap();
            reviewed.status = "reviewed".into();
            record_at(dir.path(), dir.path(), &reviewed).unwrap();
            let again = prepare_session(dir.path(), dir.path(), "backup", None, 8, host, None).unwrap();
            assert_eq!(again["cached"], true);
            assert!(again.get("delegation").is_none());
            assert_eq!(again["interpretation"]["status"], "reviewed");
            let done = complete_session(dir.path(), dir.path(), &receipt(&first["request_id"])).unwrap();
            assert_eq!(done["session_model_calls"], 0);
            assert_eq!(done["interpretation"]["status"], "reviewed");
            let mut invalid=receipt(&first["request_id"]);
            invalid.result["title"]=json!(" ");
            assert!(complete_session(dir.path(),dir.path(),&invalid).is_err(),"cache must not conceal malformed replies");
        }
    }

    #[test]
    fn an_unchanged_analysis_is_shared_across_hosts_and_model_preferences() {
        let dir=fixture();
        let prepared=prepare_session(dir.path(),dir.path(),"backup",None,8,"claude",None).unwrap();
        assert_eq!(prepared["delegation"]["max_input_tokens"],100_000);
        complete_session(dir.path(),dir.path(),&receipt(&prepared["request_id"])).unwrap();
        for (host,model) in [("codex","gpt-6-luna"),("claude","claude-haiku-5-5"),("codex","inherit")] {
            let reused=prepare_session(dir.path(),dir.path(),"  backup \n",None,8,host,Some(model)).unwrap();
            assert_eq!(reused["cached"],true,"{reused}");
            assert_eq!(reused["session_model_calls"],0);
            assert!(reused.get("delegation").is_none());
        }
        assert_eq!(interpretations(dir.path()).unwrap().len(),1);
        std::fs::write(dir.path().join("unrelated.rs"),"fn unrelated() {}\n").unwrap();
        assert_eq!(prepare_session(dir.path(),dir.path(),"backup",None,8,"codex",None).unwrap()["cached"],true,
            "an unrelated edit does not invalidate the topic");
        let changed_question=prepare_session(dir.path(),dir.path(),"backup validation",None,8,"codex",None).unwrap();
        assert_eq!(changed_question["cached"],false,"a different question is not silently answered by the old analysis");
    }

    #[test]
    fn changed_code_bad_citations_and_tampered_requests_are_rejected() {
        for mode in ["changed", "citation", "request", "traversal", "empty"] {
            let dir = fixture();
            let prepared = prepare_session(dir.path(), dir.path(), "backup", None, 8, "codex", None).unwrap();
            let mut receipt = receipt(&prepared["request_id"]);
            match mode {
                "changed" => std::fs::write(dir.path().join("backup.rs"), "fn restore() { /* changed */ }\n").unwrap(),
                "citation" => receipt.result["source_ids"] = json!(["invented"]),
                "request" => {
                    let path = directory(dir.path()).join(format!("{}.json", receipt.request_id));
                    let mut saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
                    saved["request"]["prompt"] = json!("tampered");
                    std::fs::write(path, saved.to_string()).unwrap();
                }
                "empty" => receipt.result["title"] = json!(" "),
                _ => receipt.request_id = "../outside".into(),
            }
            assert!(complete_session(dir.path(), dir.path(), &receipt).is_err(), "{mode}");
            assert!(interpretations(dir.path()).unwrap().is_empty());
            if mode == "changed" {
                assert!(prepare_session(dir.path(), dir.path(), "backup", None, 8, "codex", None).is_err());
                index(dir.path());
                let next = prepare_session(dir.path(), dir.path(), "backup", None, 8, "codex", None).unwrap();
                assert_ne!(next["request_id"], prepared["request_id"]);
            }
        }
    }
}
