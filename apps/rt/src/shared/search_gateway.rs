//! Shared gateway for CLI and host adapters; never evaluates shell request text.
use mustard_core::domain::code_search::{Invocation, Request};
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use serde_json::json;
use std::path::Path;

/// A variável em que o gancho do Bash entrega ao comando nativo a identidade
/// do agente que o pediu; a sessão segue nas variáveis da sessão
/// ([`super::spec_state::shell_export`]).
const AGENT_VAR: &str = "MUSTARD_AGENT_ID";

pub(crate) fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// Entrega ao comando nativo a identidade do subagente cujo gancho o liberou.
pub(crate) fn agent_export(agent: &str) -> String {
    format!("export {AGENT_VAR}={}\n", quote(agent))
}

/// O subagente que o gancho nomeou para este processo, por [`agent_export`].
fn agent_from_env() -> Option<String> {
    std::env::var(AGENT_VAR).ok().map(|id| id.trim().to_string()).filter(|id| !id.is_empty())
}

pub(crate) fn answer(
    cwd: &Path,
    request: &Request,
) -> Result<mustard_core::io::code_search::Answer, String> {
    request.validate()?;
    let cwd = cwd.canonicalize().map_err(|e| e.to_string())?;
    let root = mustard_core::io::spec_events::spec_root(&cwd);
    let git = mustard_core::platform::git::run(&cwd, &["rev-parse", "--show-toplevel"]);
    let tree = if git.ok {
        std::path::PathBuf::from(git.stdout.trim())
    } else {
        root.clone()
    };
    let tree = tree.canonicalize().map_err(|e| e.to_string())?;
    guard(&root, &cwd, request)?;
    if mustard_core::io::code_search::operations::requested(request) {
        let index_root = if mustard_core::io::project_map::model_path(&tree).is_file() {
            &tree
        } else {
            &root
        };
        let mut answer =
            mustard_core::io::code_search::operations::execute(index_root, &tree, &cwd, request)?;
        let learning = answer.report["learning"].clone();
        if learning["needs_scan"] == true {
            match mustard_core::Scan::locate()
                .scan_native(&tree, &mustard_core::io::project_map::model_path(&tree))
            {
                Ok(report) => {
                    let saved = mustard_core::io::knowledge::observations::scanned(
                        index_root, &tree, &learning,
                    );
                    answer.report["learning"]["scan"] = json!({"status":"refreshed-native","files_read":report.read.len(),"acknowledged":saved.is_ok(),"local_model_calls":0,"remote_model_calls":0});
                    if saved.is_ok() {
                        answer.report["learning"]["needs_scan"] = json!(false);
                    }
                    mustard_core::io::code_search::operations::recross(&tree, &tree, &mut answer)?;
                }
                Err(error) => {
                    answer.report["learning"]["scan"] =
                        json!({"status":"pending","reason":error.to_string()});
                }
            }
        }
        answer.report["choice_gate"]=json!("native-operation");
        return Ok(answer);
    }
    let selector = if request.choose && !request.intent.trim().is_empty()
        && request.tool!="Read" && request.purpose!=mustard_core::domain::knowledge::investigation::Purpose::Locate {
        super::knowledge_selection::KnowledgeSelector::configured(&root)
    } else {
        None
    };
    let mut answer = mustard_core::io::code_search::execute_native(&cwd, request)?;
    mustard_core::io::code_search::observe(&root,&tree,&cwd,request,&mut answer);
    let learning = answer.report["learning"].clone();
    let mut index_root = root.clone();
    if learning["needs_scan"] == true {
        // A linked checkout gets its own structural snapshot. Its observations
        // remain keyed by checkout in the anchor; main's scan is never replaced.
        let scan = mustard_core::Scan::locate()
            .scan_native(&tree, &mustard_core::io::project_map::model_path(&tree));
        match scan {
            Ok(report) => {
                index_root.clone_from(&tree);
                let saved =
                    mustard_core::io::knowledge::observations::scanned(&root, &tree, &learning);
                answer.report["learning"]["scan"] = json!({"status":"refreshed-native","files_read":report.read.len(),"acknowledged":saved.is_ok(),"local_model_calls":0,"remote_model_calls":0});
                if saved.is_ok() {
                    answer.report["learning"]["needs_scan"] = json!(false);
                    answer.report["learning"]["scanned_paths"] = learning["pending_paths"].clone();
                    answer.report["learning"]["pending_paths"] = json!([]);
                }
            }
            Err(error) => {
                answer.report["learning"]["scan"] =
                    json!({"status":"pending","reason":error.to_string()});
            }
        }
    } else if tree != root && mustard_core::io::project_map::model_path(&tree).is_file() {
        index_root.clone_from(&tree);
    }
    let mut local_refreshes=Vec::new();
    if let Some(refresh)=refresh_code_vectors(&index_root,request) {local_refreshes.push(refresh);}
    mustard_core::io::code_search::enrich(
            &index_root,
            &tree,
            &cwd,
            request,
            &mut answer,
            selector
                .as_ref()
                .map(|s| s as &dyn mustard_core::domain::knowledge::selection::SymbolSelector),
        );
    // A complementary source hit can discover a file the original command
    // did not mention. Refresh it before classifying incomplete candidates.
    if answer.report["task_context"]["learning"]["needs_scan"]==true
        && answer.report["remote_model_calls"]==0 {
        let learning=answer.report["task_context"]["learning"].clone();
        match mustard_core::Scan::locate().scan_native(&tree,&mustard_core::io::project_map::model_path(&tree)) {
            Ok(report)=>{
                let saved=mustard_core::io::knowledge::observations::scanned(&index_root,&tree,&learning);
                if let Some(refresh)=refresh_code_vectors(&tree,request) {local_refreshes.push(refresh);}
                mustard_core::io::code_search::enrich(&tree,&tree,&cwd,request,&mut answer,selector.as_ref().map(|s|s as &dyn mustard_core::domain::knowledge::selection::SymbolSelector));
                answer.report["task_context"]["learning"]["scan"]=json!({"status":"refreshed-native","files_read":report.read.len(),"acknowledged":saved.is_ok(),"local_model_calls":0,"remote_model_calls":0});
            },
            Err(error)=>answer.report["task_context"]["learning"]["scan"]=json!({"status":"pending","reason":error.to_string()}),
        }
    }
    if !local_refreshes.is_empty() {
        let computed:u64=local_refreshes.iter().filter_map(|refresh|refresh["computed_vectors"].as_u64()).sum();
        answer.report["local_index_refresh"]=json!({"attempts":local_refreshes,"computed_vectors":computed,"remote_model_calls":0});
    }
    answer.report["choice_gate"]=json!(choice_gate(&root,request,selector.is_some()));
    Ok(answer)
}

fn choice_gate(root:&Path,request:&Request,provider:bool)->&'static str {
    use mustard_core::domain::knowledge::investigation::Purpose;
    if request.tool=="Read" {return "exact-read";}
    if request.purpose==Purpose::Locate {return "locate";}
    if !request.choose {return "not-requested";}
    if request.intent.trim().is_empty() {return "missing-intent";}
    if provider {return "eligible";}
    let config=mustard_core::ProjectConfig::load(root);
    if !config.ai_fallback_enabled() {return "ai-fallback-disabled";}
    if !mustard_core::io::jev_gate::setting_allows(config.judgement_filter("search")) {return "purpose-not-configured";}
    "provider-unavailable"
}

fn refresh_code_vectors(root:&Path,request:&Request)->Option<serde_json::Value> {
    use mustard_core::domain::knowledge::investigation::Purpose;
    if request.purpose==Purpose::Locate || request.tool=="Read" || request.intent.trim().is_empty()
        || !mustard_core::ProjectConfig::load(root).ai_vectors_enabled() {return None;}
    let path=mustard_core::io::project_map::model_path(root);
    if !path.is_file() {return None;}
    Some(match mustard_core::io::code_vectors::fill_at(&path,root) {
        Ok(count)=>json!({"status":"current-local-code-index","computed_vectors":count,"remote_model_calls":0}),
        Err(reason)=>json!({"status":"unavailable; lexical fallback preserved","reason":reason.to_string(),"remote_model_calls":0}),
    })
}

fn guard(root: &Path, cwd: &Path, request: &Request) -> Result<(), String> {
    // A conferência vale para quem pediu a busca: o agente de onda que busca
    // na própria cópia não é o condutor preso nela, e a cópia de outra onda
    // continua barrada para ele.
    let session_id = super::spec_state::session_from_env();
    let agent_id = agent_from_env();
    let ctx = Ctx {
        project_dir: root.to_string_lossy().into_owned(),
        trigger: Some(Trigger::PreToolUse),
        config: mustard_core::ProjectConfig::load(root),
        ..Ctx::default()
    };
    let file_operation =
        request.tool == "Read" || mustard_core::io::code_search::operations::requested(request);
    let input = if file_operation {
        if request.input["file_path"].as_str().is_none() {
            return Err("search-missing-file_path".into());
        }
        HookInput {
            tool_name: Some("Read".into()),
            tool_input: {
                let mut value = request.input.clone();
                if let Some(file) = value["file_path"].as_str() {
                    value["file_path"] = json!(cwd.join(file));
                }
                value
            },
            cwd: Some(cwd.to_string_lossy().into_owned()),
            session_id,
            agent_id,
            ..HookInput::default()
        }
    } else {
        let (tool, args) = mustard_core::io::code_search::arguments(request)?;
        let command = std::iter::once(tool)
            .chain(args)
            .map(|arg| quote(&arg))
            .collect::<Vec<_>>()
            .join(" ");
        HookInput {
            tool_name: Some("Bash".into()),
            tool_input: json!({"command":command}),
            cwd: Some(cwd.to_string_lossy().into_owned()),
            session_id,
            agent_id,
            ..HookInput::default()
        }
    };
    let verdict = if file_operation {
        crate::hooks::write::write_gate::file_verdict(
            root.to_str().unwrap_or_default(),
            &input,
            &ctx,
        )
    } else {
        crate::hooks::bash::command_guard::CommandGuard
            .evaluate(&input, &ctx)
            .map_err(|e| e.to_string())?
    };
    match verdict {
        Verdict::Deny { reason } => Err(reason),
        Verdict::Rewrite { tool_input, note } => Err(format!(
            "{} Retry with input: {tool_input}",
            note.unwrap_or_default()
        )),
        _ => Ok(()),
    }
}

/// Rewrite literal file-search segments without moving shell operators.
/// Stream filters and ambiguous syntax retain the original permission path.
pub(crate) fn before_bash(input: &HookInput, ctx: &Ctx) -> Option<Verdict> {
    if !ctx.config.search_answer() {
        return None;
    }
    let command = input.tool_input.get("command")?.as_str()?;
    let ranges = crate::hooks::bash::lex::command_ranges(command);
    let compound = ranges.len() > 1;
    let mut replacement = command.to_string();
    let mut changed = false;
    for (at,range) in ranges.iter().enumerate().rev() {
        if at>0 {
            let separator=command[ranges[at-1].end..range.start].trim_start();
            if separator.starts_with('|') && !separator.starts_with("||") {continue;}
        }
        let part = &command[range.clone()];
        let trimmed = part.trim();
        // Only literal outer commands. Never evaluate substitutions or move
        // commands across &&, ||, pipes, redirects, loops or shell functions.
        if !literal_shell(trimmed) { continue; }
        let segments = crate::hooks::bash::lex::segments(trimmed);
        let Some(segment) = segments.first().filter(|_| segments.len() == 1) else {continue};
        if !matches!(segment.program.raw.as_str(), "rg" | "grep" | "git")
            || !segment.leading.is_empty() || !segment.redirects.is_empty()
            || !trimmed.strip_prefix(&segment.program.raw).is_some_and(|rest| rest.starts_with(char::is_whitespace))
            || segment.args.iter().any(|arg| arg.raw == arg.text && arg.raw.contains(['*', '?', '[', '{', '~'])) {
            continue;
        }
        let argv: Vec<_> = std::iter::once(segment.program.text.clone())
            .chain(segment.args.iter().map(|arg| arg.text.clone())).collect();
        let Ok(mut request) = Request::native(&argv) else {continue};
        if mustard_core::io::code_search::arguments(&request).is_err() {continue;}
        request.describe(input.tool_description().unwrap_or_default());
        let Ok(envelope) = serde_json::to_string(&Invocation::new(request)) else {continue};
        // `.` is evaluated when this segment runs, after any preceding cd.
        // Pipeline consumers receive exactly native bytes, without cards.
        let cwd = if compound {"."} else {input.cwd.as_deref().unwrap_or(&ctx.project_dir)};
        let routed = format!("mustard-rt run search --root {} --request {} {}",
            quote(cwd), quote(&envelope), if compound {"--raw"} else {"--shell-output"});
        let begin = range.start + part.len() - part.trim_start().len();
        replacement.replace_range(begin..begin + trimmed.len(), &routed);
        changed = true;
    }
    if !changed {return None;}
    let mut tool_input = input.tool_input.clone();
    tool_input["command"] = json!(replacement);
    Some(Verdict::Rewrite {tool_input, note: None})
}

pub(crate) fn literal_shell(command: &str) -> bool {
    let mut quote = None;
    let mut escaped = false;
    for c in command.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if quote == Some('\'') {
            if c == '\'' {
                quote = None;
            }
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if matches!(c, '$' | '`') {
            return false;
        }
        if quote == Some('"') {
            if c == '"' {
                quote = None;
            }
            continue;
        }
        if matches!(c, '\'' | '"') {
            quote = Some(c);
            continue;
        }
        if matches!(
            c,
            '\n' | '\r' | ';' | '|' | '&' | '<' | '>' | '(' | ')' | '#'
        ) {
            return false;
        }
    }
    quote.is_none() && !escaped
}

/// A classic hook cannot change a tool's name. Hand off discovery to the
/// intent-aware gateway. Exact Read calls stay on the host permission path;
/// their already executed excerpts are crossed in PostToolUse instead.
pub(crate) fn before_file(input: &HookInput, ctx: &Ctx) -> Option<Verdict> {
    if !ctx.config.search_answer() {
        return None;
    }
    let tool = input.tool_name.as_deref()?;
    if !matches!(tool, "Grep" | "Glob") {
        return None;
    }
    mustard_core::platform::process::program_location("rg")?;
    let cwd = input.cwd.as_deref().unwrap_or(&ctx.project_dir);
    let given = input
        .tool_input
        .get("path")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(".");
    let path = super::code_route::project_path(&ctx.project_dir, cwd, given)?;
    let canonical = path.abs.canonicalize().ok()?;
    if !canonical.starts_with(path.tree.canonicalize().ok()?) {
        return None;
    }
    let mut request = Request {
        tool: tool.into(),
        input: input.tool_input.clone(),
        intent: String::new(),
        purpose: Default::default(),
        choose: false,
    };
    request.describe(input.tool_description().unwrap_or_default());
    mustard_core::io::code_search::arguments(&request).ok()?;
    let command = format!(
        "mustard-rt run search --root {} --request {} --shell-output",
        quote(cwd),
        quote(&serde_json::to_string(&Invocation::new(request)).ok()?)
    );
    let reason = super::say::say(
        "search.gateway.route",
        ctx.config.language().text_or_default(),
        &[("{command}", &command)],
    );
    Some(Verdict::Deny { reason })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn choice_diagnostics_distinguish_exact_native_and_configured_investigations() {
        use mustard_core::domain::knowledge::investigation::Purpose;
        let dir=tempfile::tempdir().unwrap();
        let mut request=Request{tool:"Read".into(),input:json!({"file_path":"x.rs"}),
            intent:"Understand behavior".into(),purpose:Purpose::Understand,choose:true};
        assert_eq!(choice_gate(dir.path(),&request,true),"exact-read");
        request.tool="Grep".into();request.purpose=Purpose::Locate;
        assert_eq!(choice_gate(dir.path(),&request,true),"locate");
        request.purpose=Purpose::Understand;request.choose=false;
        assert_eq!(choice_gate(dir.path(),&request,true),"not-requested");
        request.choose=true;
        assert_eq!(choice_gate(dir.path(),&request,true),"eligible");
        assert_eq!(choice_gate(dir.path(),&request,false),"ai-fallback-disabled");
        std::fs::write(dir.path().join("mustard.json"),r#"{"ai":{"fallback":true}}"#).unwrap();
        assert_eq!(choice_gate(dir.path(),&request,false),"purpose-not-configured");
        std::fs::write(dir.path().join("mustard.json"),r#"{"ai":{"fallback":true},"judgement":{"search":{"filter":"jev"}}}"#).unwrap();
        assert_eq!(choice_gate(dir.path(),&request,false),"provider-unavailable");
    }

    fn call(root: &Path, tool: &str, value: serde_json::Value) -> (HookInput, Ctx) {
        let root = root.to_string_lossy().to_string();
        (
            HookInput {
                tool_name: Some(tool.into()),
                tool_input: value,
                cwd: Some(root.clone()),
                ..HookInput::default()
            },
            Ctx {
                project_dir: root,
                trigger: Some(Trigger::PreToolUse),
                ..Ctx::default()
            },
        )
    }
    #[test]
    fn simple_native_searches_rewrite_once_with_every_argument_and_description() {
        let dir = tempfile::tempdir().unwrap();
        for command in [
            "rg -n -i --glob '*.rs' 'restore quartz' src",
            "rg -n 'restore|quartz.*' src",
            "grep -Rnw 'restore' src",
            "git grep -n -F quartz -- src",
        ] {
            let (mut input, ctx) = call(
                dir.path(),
                "Bash",
                json!({"command":command,"description":"Understand persistence","timeout":5000}),
            );
            let original = input.tool_input.clone();
            let Some(Verdict::Rewrite { tool_input, .. }) = before_bash(&input, &ctx) else {
                panic!("{command}")
            };
            assert_eq!(tool_input["timeout"], 5000);
            assert!(!tool_input["command"].as_str().unwrap().ends_with(" --raw"),"a quoted regex alternation is not a pipeline");
            let parts = crate::hooks::bash::lex::segments(tool_input["command"].as_str().unwrap());
            let args = &parts[0].args;
            let at = args.iter().position(|arg| arg.text == "--request").unwrap();
            let request = Request::from_json(&args[at + 1].text).unwrap();
            let original_parts = crate::hooks::bash::lex::segments(command);
            assert_eq!(
                request.input["args"],
                json!(
                    original_parts[0]
                        .args
                        .iter()
                        .map(|arg| &arg.text)
                        .collect::<Vec<_>>()
                )
            );
            assert_eq!(request.intent, "Understand persistence");
            assert!(!request.choose);
            input.tool_input = tool_input;
            assert!(before_bash(&input, &ctx).is_none(), "no rewrite loop");
            assert_eq!(original["command"], command, "input is immutable");
        }
    }
    #[test]
    fn compound_searches_preserve_conditions_cwd_and_native_pipe_output() {
        let dir = tempfile::tempdir().unwrap();
        for command in ["cd src && grep -n save store.ts | head -5", "git status --short; rg -n save src || echo missing"] {
            let (input, ctx) = call(dir.path(), "Bash", json!({"command":command,"timeout":4000}));
            let Some(Verdict::Rewrite {tool_input,..}) = before_bash(&input,&ctx) else {panic!("{command}")};
            let routed = tool_input["command"].as_str().unwrap();
            assert!(routed.contains("--root '.'"), "{routed}");
            assert!(routed.contains(" --raw") && !routed.contains("--shell-output"), "{routed}");
            assert_eq!(tool_input["timeout"],4000);
            if command.starts_with("cd") {assert!(routed.starts_with("cd src && "));assert!(routed.ends_with(" | head -5"));}
            else {assert!(routed.starts_with("git status --short; "));assert!(routed.ends_with(" || echo missing"));}
            let mut again=input.clone();again.tool_input=tool_input;
            assert!(before_bash(&again,&ctx).is_none(), "no rewrite loop");
        }
    }

    #[test]
    fn unknown_shell_effects_keep_the_original_execution_path() {
        let dir = tempfile::tempdir().unwrap();
        for command in [
            "rg x src > result",
            "rg \"$TERM\" src",
            "FOO=x rg x src",
            "rtk rg x src",
            "rg x src/*.rs",
            "rg --pre helper x src",
            "git grep -O x",
            "git config user.name",
        ] {
            let (input, ctx) = call(dir.path(), "Bash", json!({"command":command}));
            assert!(before_bash(&input, &ctx).is_none(), "{command}");
        }
    }
    #[test]
    fn structured_host_description_preserves_investigation_purpose_in_versioned_contract() {
        let dir = tempfile::tempdir().unwrap();
        let (input, ctx) = call(dir.path(), "Bash", json!({
            "command":"rg -n 'current_copy' src",
            "description":"mustard:spec: Verify which operation exports the edited copy"
        }));
        let Some(Verdict::Rewrite { tool_input, .. }) = before_bash(&input, &ctx) else {
            panic!("annotated search should be routed");
        };
        let parts = crate::hooks::bash::lex::segments(tool_input["command"].as_str().unwrap());
        let args = &parts[0].args;
        let at = args.iter().position(|arg| arg.text == "--request").unwrap();
        let envelope: serde_json::Value = serde_json::from_str(&args[at + 1].text).unwrap();
        assert_eq!(envelope["schema_version"], 1);
        let request = Request::from_json(&args[at + 1].text).unwrap();
        assert_eq!(request.purpose, mustard_core::domain::knowledge::investigation::Purpose::Spec);
        assert_eq!(request.intent, "Verify which operation exports the edited copy");
        assert_eq!(request.input["args"], json!(["-n","current_copy","src"]));
        assert!(!request.choose);
    }
    #[test]
    fn typed_search_handoff_keeps_the_complete_request_and_unknown_fields_pass() {
        let dir = tempfile::tempdir().unwrap();
        for tool in ["Grep", "Glob"] {
            let value = if tool == "Grep" {
                json!({"pattern":"quartz.*","-i":true,"output_mode":"count","offset":2})
            } else {
                json!({"pattern":"**/*quartz*"})
            };
            let (input, ctx) = call(dir.path(), tool, value.clone());
            let Some(Verdict::Deny { reason }) = before_file(&input, &ctx) else {
                panic!("{tool}")
            };
            assert!(reason.contains("mustard-rt run search"));
            assert!(reason.contains(&serde_json::to_string(&value).unwrap()));
            let (input, ctx) = call(
                dir.path(),
                tool,
                json!({"pattern":"quartz","future_option":true}),
            );
            assert!(before_file(&input, &ctx).is_none());
        }
    }
    #[test]
    fn gateway_reads_apply_the_existing_secret_and_config_guards() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"), "{}").unwrap();
        std::fs::write(dir.path().join("private.pem"), "hidden-sentinel").unwrap();
        let req = Request {
            tool: "Read".into(),
            input: json!({"file_path":"private.pem","offset":1,"limit":1}),
            intent: String::new(),
            purpose: Default::default(),
            choose: false,
        };
        let error = answer(dir.path(), &req)
            .err()
            .expect("secret refused before reading");
        assert!(!error.contains("hidden-sentinel"));
        std::fs::create_dir(dir.path().join(".claude")).unwrap();
        std::fs::write(dir.path().join(".claude/settings.local.json"),r#"{"env":{"TYPESAFE_API_KEY":"private-env-sentinel"}}"#).unwrap();
        let mut req=req;
        req.input["file_path"]=json!(".claude/settings.local.json");
        let error=answer(dir.path(),&req).err().expect("private Jev key refused before reading");
        assert!(!error.contains("private-env-sentinel"));
        assert!(error.contains("***"));
    }
}
