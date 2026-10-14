//! Reuse a bounded, source-bound native-session analysis as a draft procedure.
//! The writer cannot activate its own draft: exact review remains separate.
use super::{Profile, active, digest, list, record};
use crate::io::knowledge::{
    Interpretation,
    enrichment::{self, SessionReceipt},
};
use serde_json::{Value, json};
use std::path::Path;

fn name(query: &str, file: Option<&str>) -> String {
    let normalized = query.split_whitespace().collect::<Vec<_>>().join(" ");
    let words = crate::domain::text::fold_accents(&normalized).to_lowercase();
    let slug = words.split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty()).take(5).collect::<Vec<_>>().join("-");
    let slug = slug.chars().take(36).collect::<String>();
    let slug = slug.trim_matches('-');
    format!("{}-{}", if slug.is_empty() { "procedure" } else { slug },
        &digest(format!("{normalized}\0{}", file.unwrap_or_default()).as_bytes())[..12])
}

fn existing_name(root: &Path, query: &str, file: Option<&str>) -> Result<String, String> {
    let current = name(query, file);
    // Existing source-bound procedures stay reusable after an upgrade. Do
    // not regenerate a valid older procedure solely to rename its files.
    let normalized = query.split_whitespace().collect::<Vec<_>>().join(" ");
    let legacy = format!("procedure-{}", &digest(format!("{normalized}\0{}", file.unwrap_or_default()).as_bytes())[..16]);
    Ok(if list(root)?.iter().any(|profile| profile.name == legacy) { legacy } else { current })
}

fn topic(query: &str) -> String {
    format!(
        "Reusable implementation procedure for this project task: {query}. Identify current patterns, contracts, relevant tests and limits from supplied code. Cite evidence; separate observations from mandatory rules; state gaps. Do not infer a rule or access permission."
    )
}

pub fn prepare(
    root: &Path,
    query: &str,
    file: Option<&str>,
    host: &str,
    model: Option<&str>,
) -> Result<Value, String> {
    if query.trim().is_empty() || query.len() > 1500 {
        return Err("expertise-short-task-required".into());
    }
    if !["claude", "codex"].contains(&host) {
        return Err("expertise-unknown-host".into());
    }
    let name = existing_name(root, query, file)?;
    if let Some(profile) = list(root)?.into_iter().find(|p| p.name == name) {
        // Reuse a current draft even before review; never pay to rewrite it.
        if super::sources_current(root, &profile)
            && profile
                .rule_context
                .as_ref()
                .is_some_and(|rules| rules.current(root))
        {
            return Ok(
                json!({"ok":true,"cached":true,"draft":profile,"active":active(root,&profile),
                "fingerprint":super::review_fingerprint(root,&profile),"session_model_calls":0,
                "next":"Inspect the existing procedure and current project rules; review its exact fingerprint before activation."}),
            );
        }
    }
    let mut report = enrichment::prepare_session(root, root, &topic(query), file, 8, host, model)?;
    report["expertise_name"] = json!(name);
    report["next"] = json!(
        "If cached, reuse the interpretation as a draft procedure. If pending, reuse its existing delegation. Otherwise use the current host's isolated native subagent, latest Haiku/Luna and medium effort, with at most 100000 total input tokens. Submit its receipt using expertise --complete-session <file> --query <same task> and the same --file. Review the resulting draft separately; do not start another CLI/API."
    );
    if report["cached"] == true
        && let Some(value) = report.get("interpretation")
    {
        let note: Interpretation =
            serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
        report["draft_result"] = draft(root, &name, &note)?;
    }
    Ok(report)
}

pub fn complete(
    root: &Path,
    query: &str,
    file: Option<&str>,
    receipt: &SessionReceipt,
) -> Result<Value, String> {
    if query.trim().is_empty() || query.len() > 1500 {
        return Err("expertise-short-task-required".into());
    }
    // Reconstruct the expected topic/request to prevent an unrelated receipt
    // from being silently attached to a task-specific procedure.
    let prepared = enrichment::prepare_session(root, root, &topic(query), file, 8, "claude", None)?;
    if let Some(expected) = prepared["request_id"].as_str()
        && expected != receipt.request_id
    {
        return Err("expertise-session-task-mismatch".into());
    }
    let mut report = enrichment::complete_session(root, root, receipt)?;
    if prepared["cached"] == true
        && prepared["interpretation"]["id"] != report["interpretation"]["id"]
    {
        return Err("expertise-session-task-mismatch".into());
    }
    let note: Interpretation =
        serde_json::from_value(report["interpretation"].clone()).map_err(|e| e.to_string())?;
    report["draft_result"] = draft(root, &existing_name(root, query, file)?, &note)?;
    Ok(report)
}

fn draft(root: &Path, name: &str, note: &Interpretation) -> Result<Value, String> {
    let mut paths: Vec<_> = note.sources.iter().map(|s| s.file.clone()).collect();
    paths.sort();
    paths.dedup();
    // Full source binding and explicit review are retained. Static analysis
    // is a candidate procedure, not a fact or project-wide instruction.
    let procedure = format!(
        "Source-bound candidate procedure; follow mandatory project/spec rules first. Recheck behavior and relevant tests before implementation.\n\n{}",
        note.text
    );
    if procedure.len() > 8_000 {
        return Err("expertise-procedure-too-long-review-and-condense".into());
    }
    record(
        root,
        &Profile {
            name: name.into(),
            description: note.title.chars().take(300).collect(),
            paths,
            procedure,
            sources: note.sources.clone(),
            origin: "authored".into(),
            reviewed: None,
            rule_context: note.rule_context.clone(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_session_procedure_is_reviewed_reused_and_invalidated_by_rules() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let text = "fn restore() {}\n";
        std::fs::write(root.join("backup.rs"), text).unwrap();
        let mut raw = json!({"modules":[{"path":"backup.rs","analysis":{"content_sha256":digest(text.as_bytes()),"parse_complete":true},
            "declarations":[{"name":"restore","kind":"function","line":1,"end_line":1,"doc":"Restores a backup"}]}]});
        crate::domain::knowledge::enrich(&mut raw);
        crate::io::project_map::write_text(root, &raw.to_string()).unwrap();
        let prepared = prepare(root, "restore backup", Some("backup.rs"), "codex", None).unwrap();
        let receipt = SessionReceipt {
            request_id: prepared["request_id"].as_str().unwrap().into(),
            model: "gpt-6-luna".into(),
            effort: Some("medium".into()),
            result: json!({"title":"Backup implementation procedure","text":"Inspect restore and its existing tests before changing behavior [s0].","source_ids":["s0"]}),
            input_tokens: Some(600),
            output_tokens: Some(50),
        };
        let result = complete(root, "restore backup", Some("backup.rs"), &receipt).unwrap();
        assert_eq!(result["draft_result"]["active"], false);
        let name = result["draft_result"]["name"].as_str().unwrap();
        let fingerprint = result["draft_result"]["fingerprint"].as_str().unwrap();
        super::super::review(root, name, fingerprint).unwrap();
        let reused = prepare(root, "restore backup", Some("backup.rs"), "claude", None).unwrap();
        assert_eq!(reused["cached"], true);
        assert_eq!(reused["active"], true);
        assert_eq!(reused["session_model_calls"], 0);
        std::fs::write(root.join("AGENTS.md"), "Use project schema validation.\n").unwrap();
        let changed = prepare(root, "restore backup", Some("backup.rs"), "claude", None).unwrap();
        assert_eq!(changed["cached"], false);
        assert!(changed.get("delegation").is_some());
        assert!(!super::super::active(root, &list(root).unwrap()[0]));
        assert!(
            complete(
                root,
                "restore backup differently",
                Some("backup.rs"),
                &receipt
            )
            .is_err()
        );
    }

    #[test]
    fn names_are_readable_bounded_and_keep_task_and_scope_identity() {
        let short = name("Validar transações do banco", Some("store.rs"));
        assert!(short.starts_with("validar-transacoes-do-banco-"), "{short}");
        assert_eq!(short, name("  Validar transações\n do banco ", Some("store.rs")));
        assert_ne!(short, name("Validar transações do banco", Some("other.rs")));
        assert!(name(&"a".repeat(1500), None).len() <= 56);
        assert!(name("修复读取", None).starts_with("procedure-"));
    }

    #[test]
    fn upgrading_the_name_scheme_keeps_an_existing_procedure() {
        let (dir, mut profile) = super::super::tests::fixture();
        let root = dir.path();
        let query = "reuse transactions";
        let file = Some("store.rs");
        profile.name = format!("procedure-{}", &digest(format!("{query}\0store.rs").as_bytes())[..16]);
        record(root, &profile).unwrap();
        let reused = prepare(root, query, file, "claude", None).unwrap();
        assert_eq!(reused["draft"]["name"], profile.name);
        assert_eq!(reused["session_model_calls"], 0);
        assert!(reused.get("delegation").is_none());
    }
}
