//! Host protocol adapter. Codex command/patch events are normalized locally;
//! Claude transcript formats, approval witnesses and settings healing do not
//! run against a Codex session. Installation never implies hook trust.
use mustard_core::domain::model::contract::{HookInput, Outcome, Trigger, Verdict};
use serde_json::{Value, json};

fn normalized(input: &HookInput) -> HookInput {
    let mut copy = input.clone();
    if !copy.raw.is_object() { copy.raw = json!({}); }
    copy.raw["mustard_host"] = json!("codex");
    match input.tool_name.as_deref() {
        Some("exec_command" | "shell_command") => {
            copy.tool_name = Some("Bash".into());
            if copy.tool_input.get("command").is_none() && let Some(cmd) = copy.tool_input.get("cmd").cloned() {
                copy.tool_input["command"] = cmd;
            }
        }
        Some("spawn_agent") => {
            copy.tool_name = Some("Agent".into());
            if let Some(message) = copy.tool_input.get("message").cloned() { copy.tool_input["prompt"] = message; }
            if let Some(agent) = copy.tool_input.get("agent_type").cloned() { copy.tool_input["subagent_type"] = agent; }
        }
        Some("request_user_input") => {
            // The host's actual PostToolUse answer is the witness. Never
            // reinterpret assistant prose or a Claude transcript as Codex input.
            if let Some(questions)=input.tool_input.get("questions").and_then(Value::as_array) {
                let mut answers=serde_json::Map::new();
                for question in questions {
                    let (Some(id),Some(text))=(question["id"].as_str(),question["question"].as_str()) else {continue;};
                    if questions.iter().filter(|q| q["id"].as_str()==Some(id)).count()!=1
                        || questions.iter().filter(|q| q["question"].as_str()==Some(text)).count()!=1 {continue;}
                    if let Some(selected)=input.raw.pointer("/tool_response/answers").and_then(|a|a.get(id)).and_then(|a|a.get("answers")).and_then(Value::as_array) {
                        answers.insert(text.into(),json!(selected));
                    }
                }
                copy.tool_name=Some("AskUserQuestion".into());
                copy.raw["tool_response"]=json!({"answers":answers});
            }
        }
        _ => {}
    }
    copy
}

/// Enumerate both ends of moves and every add/update/delete. An unrecognized
/// patch is refused, never silently treated as an unprotected write.
fn patch_files(command: &str) -> Result<Vec<String>, String> {
    if !command.starts_with("*** Begin Patch\n") || !command.trim_end().ends_with("*** End Patch") {
        return Err("[Mustard] Patch format was not recognized; no file was authorized.".into());
    }
    let mut files = Vec::new();
    for line in command.lines() {
        for prefix in ["*** Add File: ", "*** Update File: ", "*** Delete File: ", "*** Move to: "] {
            if let Some(path) = line.strip_prefix(prefix) {
                if path.trim().is_empty() { return Err("[Mustard] Patch contains an empty path.".into()); }
                files.push(path.to_string());
            }
        }
    }
    if files.is_empty() { return Err("[Mustard] Patch has no recognized file operation.".into()); }
    files.sort(); files.dedup(); Ok(files)
}

pub(crate) fn codex(event: &str, original: &HookInput) -> Option<String> {
    let input = normalized(original);
    let root = mustard_core::io::spec_events::spec_root(std::path::Path::new(input.cwd.as_deref().unwrap_or(".")));
    let config=mustard_core::ProjectConfig::load(&root);
    if !config.enabled() { return None; }
    // PostCompact is confirmed completion, unlike PreCompact or a lower
    // usage reading. The shared registry has no Claude PostCompact event.
    if event == "PostCompact" {
        crate::shared::host_observation::observe(&root,"codex",event,&input);
        if !input.is_subagent() {
            crate::hooks::session::conversation_size::reset_conductor_notice(&input,&root);
        }
        return None;
    }
    let mut outcome = if event == "PreToolUse" && input.tool_name.as_deref() == Some("apply_patch") {
        let command = input.tool_input.as_str().or_else(|| input.tool_input.get("command").and_then(Value::as_str)).unwrap_or_default();
        let mut outcome = Outcome::allow();
        match patch_files(command) {
            Err(reason) => outcome.fold(Verdict::Deny { reason }),
            Ok(files) => for file in files {
                let mut edit = input.clone(); edit.tool_name = Some("Edit".into()); edit.tool_input = json!({"file_path":file});
                let checked = crate::dispatch::run_codex_event(Some(Trigger::PreToolUse), &edit);
                if checked.is_blocking() { outcome = checked; break; }
                outcome.warnings.extend(checked.warnings);
            },
        }
        outcome
    } else {
        crate::dispatch::run_codex_event(Trigger::from_event_name(event), &input)
    };
    // The rtk swap already ran inside the command guard, the same step the
    // Claude Code hook takes: Codex gets one rewrite, never a second pass.
    if let Verdict::Rewrite { tool_input, .. } = &mut outcome.verdict {
        if original.tool_name.as_deref() == Some("spawn_agent") {
            let mut actual = original.tool_input.clone();
            if let Some(prompt) = tool_input.get("prompt") { actual["message"] = prompt.clone(); }
            *tool_input = actual;
        } else if matches!(original.tool_name.as_deref(),Some("exec_command"|"shell_command")) && original.tool_input.get("command").is_none()
            && let Some(command)=tool_input.get("command").cloned() {
            let mut actual=original.tool_input.clone(); actual["cmd"]=command; *tool_input=actual;
        }
        // Canonical Codex Bash accepts command. Keep all of the host's other
        // fields rather than replacing the invocation with a Claude shape.
    }
    output(event, &outcome)
}

fn output(event: &str, outcome: &Outcome) -> Option<String> {
    let mut specific = json!({"hookEventName":event});
    let mut context = outcome.warnings.clone();
    match &outcome.verdict {
        Verdict::Deny { reason } if event == "PreToolUse" => {
            specific["permissionDecision"] = json!("deny"); specific["permissionDecisionReason"] = json!(reason);
        }
        Verdict::Deny { reason } if ["Stop", "SubagentStop", "UserPromptSubmit"].contains(&event) =>
            return Some(json!({"decision":"block","reason":reason}).to_string()),
        Verdict::Deny { reason } => context.push(reason.clone()),
        Verdict::Rewrite { tool_input, note } if event == "PreToolUse" => {
            specific["permissionDecision"] = json!("allow"); specific["updatedInput"] = tool_input.clone();
            if let Some(note) = note { context.push(note.clone()); }
        }
        Verdict::Inject { context: text } => context.push(text.clone()),
        Verdict::Warn { message } => context.push(message.clone()),
        Verdict::ToolOutput { context: Some(text), .. } => {
            // Codex does not document replacement tool stdout. Keep the native
            // output and pass only the contextual projection.
            context.push(text.clone());
        }
        _ => {}
    }
    if ["PreCompact", "PostCompact"].contains(&event) {
        return (!context.is_empty()).then(||json!({"systemMessage":context.join("\n\n")}).to_string());
    }
    if !context.is_empty() { specific["additionalContext"] = json!(context.join("\n\n")); }
    if specific.as_object().is_some_and(|value| value.len() == 1) { return None; }
    if !["PreToolUse", "PostToolUse", "SessionStart", "UserPromptSubmit", "Stop", "SubagentStart", "SubagentStop"].contains(&event) { return None; }
    Some(json!({"hookSpecificOutput":specific}).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    /// A rewritten command goes back as replacement arguments on the
    /// tool-call event and never as an approval of the native execution.
    #[test]
    fn a_rewrite_never_approves_native_execution() {
        let mut out=Outcome::allow();
        out.fold(Verdict::Rewrite {tool_input:json!({"command":"'/opt/rtk' git status --short","yield_time_ms":1000}),note:None});
        let view:Value=serde_json::from_str(&output("PreToolUse",&out).unwrap()).unwrap();
        assert_eq!(view["hookSpecificOutput"]["updatedInput"]["command"],"'/opt/rtk' git status --short");
        assert_eq!(view["hookSpecificOutput"]["updatedInput"]["yield_time_ms"],1000);
        assert!(output("PermissionRequest",&out).is_none(),"rewriting must not approve execution");
    }
    #[test]
    fn codex_post_tool_answer_records_approval_through_the_native_witness() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() { return; }
        use mustard_core::io::spec_events as store;
        let dir=tempfile::tempdir().unwrap(); let root=dir.path();
        std::fs::write(root.join("mustard.json"),"{}").unwrap();
        let path=store::spec_file(root,"epic").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        store::write(&path,"task",json!({"text":"Somar dois números.","author":"binary"}).as_object().unwrap().clone(),&[]).unwrap();
        store::write(&path,"state",json!({"phase":"plan","branch":"feature/epic","base":"dev"}).as_object().unwrap().clone(),&[]).unwrap();
        crate::shared::context::session::bind_session_spec(&root.to_string_lossy(),"codex-witness","epic");
        let input:HookInput=serde_json::from_value(json!({"cwd":root,"hook_event_name":"PostToolUse","session_id":"codex-witness","tool_name":"request_user_input",
            "tool_input":{"questions":[{"id":"approval","header":"Spec","question":"Aprovar esta spec?","options":[{"label":"Aprovar"},{"label":"Ajustar"}]}]},
            "tool_response":{"answers":{"approval":{"answers":["Aprovar"]}}}})).unwrap();
        codex("PostToolUse",&input);
        let log=store::read(&path).unwrap().unwrap();
        assert!(log.events.iter().any(|e|e.event_type=="state" && e.str_field("phase")==Some("approved") && e.str_field("author")==Some("user")));
    }
    #[test]
    fn codex_question_witness_requires_a_native_correlated_answer() {
        let input:HookInput=serde_json::from_value(json!({"tool_name":"request_user_input",
            "tool_input":{"questions":[{"id":"approval","question":"Aprova?","options":[{"label":"Aprovar"},{"label":"Ajustar"}]}]},
            "tool_response":{"answers":{"approval":{"answers":["Aprovar"]},"unrelated":{"answers":["Aprovar"]}}}})).unwrap();
        let canonical=normalized(&input); assert_eq!(canonical.ask_answers().items.len(),1);
        assert_eq!(canonical.ask_answers().items[0].labels,vec!["Aprovar"]);
        let mut duplicate=input.clone();
        duplicate.tool_input["questions"]=json!([{"id":"approval","question":"Aprova?"},{"id":"approval","question":"Outra?"}]);
        assert!(normalized(&duplicate).ask_answers().items.is_empty());
        duplicate.tool_input["questions"]=json!([{"id":"approval","question":"Aprova?"},{"id":"other","question":"Aprova?"}]);
        assert!(normalized(&duplicate).ask_answers().items.is_empty());
        let mut absent=input; absent.raw["tool_response"]=json!({"text":"The user approved"});
        assert!(normalized(&absent).ask_answers().items.is_empty());
    }
    #[test]
    fn a_mixed_patch_checks_every_file_and_respects_disabled_hooks() {
        let dir=tempfile::tempdir().unwrap(); let root=dir.path();
        std::fs::write(root.join("mustard.json"),"{}").unwrap();
        let input:HookInput=serde_json::from_value(json!({"cwd":root,"tool_name":"apply_patch","tool_input":{"command":"*** Begin Patch\n*** Add File: a.rs\n+fn ok() {}\n*** Update File: .claude/spec/x/spec.ndjson\n@@\n-old\n+new\n*** End Patch"}})).unwrap();
        let denied:Value=serde_json::from_str(&codex("PreToolUse",&input).unwrap()).unwrap();
        assert_eq!(denied["hookSpecificOutput"]["permissionDecision"],"deny");
        assert!(!root.join("a.rs").exists(),"the adapter never executes part of a patch");
        std::fs::write(root.join("mustard.json"),r#"{"enabled":false}"#).unwrap();
        assert!(codex("PreToolUse",&input).is_none());
    }
    #[test]
    fn codex_normalization_preserves_host_fields_and_patch_moves() {
        let call: HookInput = serde_json::from_value(json!({"tool_name":"exec_command","tool_input":{"cmd":"rg restore .","yield_time_ms":1000},"session_id":"s"})).unwrap();
        let native = normalized(&call);
        assert_eq!(native.tool_name.as_deref(), Some("Bash"));
        assert_eq!(native.tool_input["command"], "rg restore .");
        assert_eq!(native.tool_input["yield_time_ms"], 1000);
        assert_eq!(patch_files("*** Begin Patch\n*** Update File: a.rs\n*** Move to: b.rs\n@@\n-old\n+new\n*** Delete File: c.rs\n*** Add File: d.rs\n+x\n*** End Patch").unwrap(), vec!["a.rs","b.rs","c.rs","d.rs"]);
        assert!(patch_files("not a patch").is_err());
    }
    #[test]
    fn output_uses_only_supported_codex_fields() {
        let mut out = Outcome::allow(); out.fold(Verdict::Rewrite { tool_input: json!({"command":"rg foo ."}), note: None });
        let value: Value = serde_json::from_str(&output("PreToolUse", &out).unwrap()).unwrap();
        assert_eq!(value["hookSpecificOutput"]["updatedInput"]["command"], "rg foo .");
        assert!(value.get("continue").is_none());
        assert!(output("SessionEnd", &out).is_none());
    }
}
